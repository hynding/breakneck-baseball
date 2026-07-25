# glTF Player Models & Skeletal Animation — Design

**Date:** 2026-07-24
**Status:** Approved design, pending implementation plan

## Goal

Replace the primitive-assembled (`Blocky`) player rigs with a Blender-authored,
rigged, skeletally-animated glTF humanoid — mesh *and* animation — while
preserving the self-contained-binary property, the `AnimClip`/`Playing`/
`MoveIntent` caller API, and the existing test suite's timing semantics. Build
the asset pipeline so that iterating on models and animations stays fast as the
game keeps growing.

## Decisions

| # | Decision |
|---|---|
| 1 | **Full glTF animation** — mesh and clips come from the `.glb`; the sampler's `root_drop`/`root_pitch` channels are rebaked as bone motion inside the clips; affected tests updated |
| 2 | **Packaging: `embedded_asset!`** — the `.glb` compiles into the binary/wasm; no `assets/` dir ships, no CI deploy changes, single-artifact property preserved |
| 3 | **Authored in Blender via MCP** — we build the rig and clips; the `.blend` is a committed, versioned source file |
| 4 | **Scope: all people** — fielders, batter, pitcher, runners, umpires share one model; stadium/field/props stay procedural (`field.rs` untouched) |
| 5 | **Approach A** — `AnimClip`, `Playing`, `MoveIntent` survive unchanged as the caller-facing API; only `animation.rs`'s sampler backend is replaced with Bevy `AnimationPlayer`/`AnimationGraph` (the seam the module doc-comment reserved) |
| 6 | **Cross-fade blending** — ~150 ms `AnimationTransitions` between clips (run → dive no longer hard-cuts) |
| 7 | **Speed-scaling kept** — clips are authored at natural speed and playback-scaled to `AnimClip::duration()`; migrating load-bearing beats (pitch release, bat contact) to animation notify events is a noted follow-up, not in scope |
| 8 | **`Blocky` stays** as a fallback `PlayerModelId` arm — menu T-cycle A/B, regression escape hatch |

Deliberate omissions (arcade-appropriate, documented so they read as choices):
no IK (glove does not literally meet the ball; canned `Dive`/`ScoopBall`
clips), no root motion (`locomote` translating the entity + in-place `RunCycle`
is the responsive arcade norm), no motion matching.

## Section 1 — Blender rig & clip contract

**One `.glb`, one armature, one skinned mesh.** A single humanoid serves every
person on the field; team identity comes from recoloring, and umpires skip
recolor (matching today's `RigUnit::Umpire` convention).

**Armature:** minimal humanoid — root/hips, spine, head, upper+lower arms
(L/R), upper+lower legs (L/R). **The bat is a bone off the right hand** (bat
mesh skinned/parented to it), so swing clips animate arms and bat together.
This collapses the old split between `BatterSwing` (body) and `SwingBat`
(separate bat pivot) into one baked clip.

**Clip set — the naming contract.** Every `AnimClip` variant maps to one baked
NLA action named identically. The old procedural channels bake *into* clips as
bone motion:

| `AnimClip` | Baked content | Old channel absorbed |
|---|---|---|
| `WindUp` | pitcher rock-back, arm wind, leg kick | — |
| `ThrowRelease` | arm whips through release | — |
| `RunCycle` | looping run (in place) | — |
| `ScoopBall` | crouch-and-scoop | `root_drop` → hip dip |
| `GloveUp` | glove arm straight up | — |
| `CatcherCrouch` | looping crouch, glove presented | `root_drop` → folded legs, low hips |
| `Dive` | full-extension layout | `root_pitch` → face-first spine/hip lean |
| `Slide` | feet-first slide | `root_pitch` → lean-back |
| `BatterSwing` | body + bat sweep through the zone | absorbs `SwingBat`/`RecoverSwing` |
| `Idle` | neutral resting pose (new variant) | replaces `settle_removed`'s transform zeroing |

`SwingBat`/`RecoverSwing` enum variants remain so no caller breaks; both map to
the `BatterSwing` node. The standalone bat-pivot entity goes away for glTF rigs
— the bat is a bone, so inserting `Playing` on the rig root covers body and bat
in one clip; callers that targeted the bat pivot re-target the batter's rig
root.

**Materials:** one named `JerseyBody` base material is the recolor handle.
Skin/cap/glove are separate materials left untouched. Named attachment bones
(spine, chest, shoulders, right hand) are part of the contract for jersey
lettering and the bat.

## Section 2 — Asset pipeline & dev iteration loop

- **`assets-src/player.blend` is the committed source of truth.** Authored via
  the Blender MCP but saved and versioned — an editable artifact, not one-shot
  output.
- **Scripted export:** `tools/export_glb.py`, runnable headless
  (`blender --background assets-src/player.blend --python tools/export_glb.py`)
  or via MCP. Output lands at `src/game/models/player.glb`, next to the code
  that embeds it. No export-settings drift.
- **Dual-mode loading behind one helper:** release and wasm use
  `embedded_asset!`; the existing `dev` feature loads from the file path with
  Bevy's file watcher enabled → edit in Blender, re-export, the running game
  hot-reloads the model without recompiling. Callers never know which mode
  resolved the asset.
- **Contract-validation test:** a plain unit test using the `gltf` parser crate
  (dev-dependency, no Bevy) asserts the `.glb` contains every clip name in the
  table, the `JerseyBody` material, the named attachment bones, and stays under
  a size ceiling (protects the Pages deploy). A broken export fails CI with a
  named diff ("missing clip: Slide"), not a silently T-posing fielder.
- **One source-of-truth table:** `const CLIP_TABLE: &[(AnimClip, &str)]` used
  by both the runtime graph builder and the validation test — the contract
  cannot fork.

**Iteration recipes:** new animation = author action in Blender → add enum
variant + table row → insert `Playing::new(...)` from gameplay. New player
model = new `.glb` + a `Theme` entry. New prop on a player = mesh parented to a
named bone.

## Section 3 — Spawn & scene wiring (`player.rs`)

- `PlayerModelId` gains a `Gltf` arm carrying a model id; **`Theme` maps id →
  asset path**, so a new model is data in `theme.rs`, not code.
- Rig spawn becomes `SceneRoot(scene_handle)` under the existing rig-root
  entity (which keeps `MoveIntent`, `RigBaseY`-equivalent spawn height, roles,
  colliders). The glTF hierarchy populates **async** — embedded assets still
  load off-thread.
- A **rig-wiring system** watches `Added<AnimationPlayer>` under rig roots and
  finishes setup when the scene lands: inserts the `AnimationGraph` handle +
  `AnimationTransitions`, resolves named bones (`Name` components from glTF
  nodes) for jersey quads and the bat, tags the root ready. At `game_start`
  this resolves within a frame or two, before the first pitch duel.

## Section 4 — Animation driver (`animation.rs`)

- Startup builds one `AnimationGraph` from the loaded clips via `CLIP_TABLE`,
  stored as a `RigAnimations` resource (`AnimClip → AnimationNodeIndex`).
- `sample_clips` is replaced by a driver reacting to `Playing` insert/change:
  `transitions.play(&mut player, node, 150 ms)`, speed = authored ÷ target
  duration, repeat mode from `looping()`. **The `Playing` timer keeps running
  exactly as today** — it still owns chaining (`next`) and removal semantics,
  so `flow.rs` timing is untouched.
- `settle_removed` cross-fades to `Idle` instead of zeroing transforms.
- `locomote` is unchanged — it moves the root; bones ride on top (no root
  motion, so no conflict with the travel yaw).
- Deleted: `limb_pose`, `root_drop`, `root_pitch`, `RigLimb`, `RigBaseY`'s
  restore path for glTF rigs, and their unit tests — replaced by the
  contract-validation test and driver-mapping unit tests.

## Section 5 — Recolor, jerseys, umpires

- On rig-ready, recolor finds the `JerseyBody` material and swaps in a
  **per-team cached material instance** (the caching pattern `jersey.rs`
  already uses). Scoreboard-flip recolor mutates the two cached materials —
  cheaper than today's per-part walk.
- Jersey lettering quads re-parent from rig roots to **named bones** (spine for
  back name/number, chest, shoulders) so lettering rides the animated torso.
  `dress_jerseys`' triggers (half-inning flips, order advances, substitutions)
  are unchanged.
- Umpires: same scene, skipped by recolor, fixed blacks — today's convention.

## Section 6 — Testing & risks

- e2e tests assert game state, not pixels; the harness's 240 Hz virtual frames
  tick through async scene readiness. The rig-wiring system gets a staged test
  (spawn → tick until ready → assert graph wired, bones resolved).
- **Risks:** (a) skinned mesh on WebGL2 — supported by Bevy, verified on the
  wasm target *early*, not last; (b) hot-reload covers asset edits only —
  enum/table changes still need a recompile (correct); (c) no IK — deliberate
  arcade omission; (d) `.glb` size — one low-poly rig is tens of KB; the
  validation test's size ceiling guards the wasm deploy.

## Follow-ups (explicitly out of scope)

- Animation **notify events** for the two load-bearing beats (pitch release,
  bat contact) so clips can play at authored speed without gameplay constants.
- Additional model variants (home/away body types, crowd figures) via the
  `Theme` model-id map.
- IK glove targeting, if fidelity ambitions ever outgrow arcade.
