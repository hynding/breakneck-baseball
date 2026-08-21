---
name: rigs-and-animation
description: Use when touching src/game/present/animation/, src/game/present/player/, src/game/models/player.glb, tools/build_player.py, tools/export_glb.py, or src/game/present/jersey.rs; when asked to "add an animation", change a clip or pose, or when "the model looks wrong". Covers the AnimClip/Playing/MoveIntent API, CLIP_TABLE + model_contract.rs, the Blender build/export pair, and jersey lettering.
---

# Rigs & Animation

The full narrative is in `reference/pipeline.md` — read it before changing the model pipeline
or clip system.

## The one API

All rig motion flows through `src/game/present/animation/`: systems insert a named `Playing`
clip (`AnimClip`) or write a `MoveIntent` — **never rotate rig parts or step transforms
directly**. Clips pose limbs and can:

- sink the rig root via the `root_drop` channel (`RigBaseY` holds each rig's resting height —
  crouches and scoops really lower the body), or
- lean it via `root_pitch` composed over the travel yaw (`Dive` for edge-of-reach catches,
  `Slide` for runners arriving at a bag).

`MoveIntent` is the seam for future player-controlled fielding (CPU choreography writes the same
component a controller would).

## Model pipeline (glTF)

Player rigs default to a skinned glTF model (`src/game/models/player.glb`), embedded via
`embedded_asset!`. Regenerating it is **always this pair, in order**:

```sh
blender --background --python tools/build_player.py                       # rebuild assets-src/player.blend
blender --background assets-src/player.blend --python tools/export_glb.py # export -> src/game/models/player.glb
```

Never hand-export from the Blender GUI — `export_glb.py` pins the settings (Y-up,
NLA-tracks-as-animations, skins) the runtime loader and `tests/model_contract.rs` depend on.
`tests/model_contract.rs` pins the `.glb` against `model_assets::CLIP_TABLE` (clip/material/bone
names plus tri/bone/size budgets) so Blender and Rust can only drift in ways CI catches.

`model_assets.rs` and `src/game/models/` stay at `src/game/` top level — `embedded_asset!`
derives paths from the file's own location; moving them breaks the build or silently breaks
model loading. `--features dev` swaps the embedded asset for a file-watched path so the `.glb`
hot-reloads on Blender export without a rebuild.

## Adding an animation

1. Author a new Blender action in `tools/build_player.py`, rebuild + export (pair above).
2. Add an `AnimClip` variant and a `CLIP_TABLE` row.
3. Let the compiler walk you through the rest — `duration()`/`limb_pose()` are exhaustive
   matches with no wildcard arm (plus `looping()` if the clip loops).
4. Run `cargo test model_contract`.

The glTF backend drives Bevy's `AnimationPlayer`/`AnimationGraph` with 150 ms cross-fades behind
the same `Playing` API; `PlayerModelId::Blocky` keeps the original procedural rig and sampler as
the fallback arm (reachable via `src/game/present/player/`'s mesh match).

## Recolouring & jerseys

Players are multi-part rigs recoloured to the fielding/batting team on scoreboard changes.
Umpire rigs (`FieldSpec::umpire_positions`, `RigUnit::Umpire`) wear fixed blacks and are
**skipped** by recolouring; the `PlateUmpire` crouches through the duel alongside the catcher.

Jersey lettering is procedural (`src/game/present/jersey.rs`): a built-in 5×7 bitmap font draws
surname + number into in-memory RGBA textures (name/number on back, number on chest and
shoulders) on quads hung off rig roots. `dress_jerseys` re-letters on half-inning flips,
batting-order advances, and substitutions, caching materials per player. **Roster names must be
A–Z only.**

## Verify

`cargo test model_contract` after model/tool changes; `cargo test` (e2e) after clip/choreography
changes; both-target `cargo check` if rendering code changed.
