# Swappable Bat Models — Design

**Date:** 2026-08-17
**Status:** Approved (design reviewed twice by sub-agents, including an empirical
Blender 5.2 export probe; all findings folded in)

## Goal

Separate the bat from the player model into its own swappable asset library.
Each bat model carries **logical identifiers in the model itself**: named grip
markers (where it can be held) and a contact-surface definition (the barrel
segment + radii) that parameterizes the physics of ball contact.

Scope is **Option A**: the bat's measured geometry parameterizes the existing
analytic contact model (timing → quality → exit velocity). The asset data is
deliberately richer than A needs so that future options slot in without asset
rework:

- **B (future):** bat-space geometric contact point — intersect the ball's
  plate position against the barrel segment to blend *where on the bat* into
  quality/spray.
- **C (future):** real Rapier collider — build a tapered capsule from the same
  contact markers, behind its own collision group.

## Non-goals

- No physical ball-vs-bat collision now (C). The ball keeps ignoring player
  colliders; count integrity stays analytic.
- No reach change now: `reach_scale` is **cut** from Option A. Swing reach is a
  `flow.rs` const gate (`SWING_REACH_X`/`SWING_EARLY_Z`), not `Ruleset` data,
  and moves whiff rate directly; it rides with the B seam.
- No per-bat tinting (scene instances share glb material handles; a future
  tint must clone materials).
- `PlayerModelId::Blocky`'s procedural rig and bat are untouched.

## 1. Asset pipeline: `bats.glb`

New pair mirroring the player pipeline:

- `tools/build_bats.py` — builds `assets-src/bats.blend` from scratch.
- Export via `tools/export_glb.py`, generalized to derive its output path
  from the opened blend's filename (`player.blend → player.glb`,
  `bats.blend → bats.glb`) — one pinned-settings exporter, no per-asset
  copies. Never hand-export.
- Embedded via `embedded_asset!`; `--features dev` swaps to the file-watched
  path (mirror of the `player.glb` pattern in `model_assets.rs`).

**One Blender scene per bat.** Bevy spawns whole scenes only
(`Gltf::named_scenes`), so each bat (`BatClassic`, `BatLumber`, `BatQuick`)
lives in its own Blender scene; the exporter's default (`use_active_scene =
False`) exports all scenes. Verified: Bevy 0.15 `named_scenes` +
multi-scene glb work as needed.

**Markers.** Each bat root node has one mesh child plus five marker empties as
**direct children of the root** (node-local == bat-local; no transform
composition):

| Suffix | Meaning |
|---|---|
| `Grip.Knob` | Default grip point; aligned to the `Bat` bone origin at attach |
| `Grip.Choke` | Upper end of the legal grip segment (future choke-up lever) |
| `Contact.Start` | Barrel contact segment start |
| `Contact.Sweet` | Sweet spot |
| `Contact.End` | Barrel tip end of contact segment |

Blender object names are globally unique **per file**, so authored names are
prefixed (`BatClassic.Grip.Knob`, …). Both the runtime extractor and the
contract test resolve markers as **children-of-root matched by name suffix**,
never by bare global name — any prefixing scheme survives, and a mis-authored
blend fails CI.

**Radius encoding.** Each `Contact.*` empty's **object scale** (set explicitly
by `build_bats.py` as `obj.scale = (r, r, r)`; never the empty's display size,
which does not export) encodes the local barrel radius. Verified empirically:
object scale exports intact on empty nodes.

**Axis convention (pinned).** Under `export_yup`, Blender **+Z** → glTF **+Y**.
Bats are modeled vertically along Blender +Z (knob at origin, barrel up);
after export the grip→barrel axis is glTF/bone-local +Y, matching the `Bat`
bone's head→tail direction. `bat_contract.rs` checks
`Contact.Start < Contact.Sweet < Contact.End` on glTF translation **Y**.

**The Bat bone is the socket.** `tools/build_player.py` drops
`_bat_mesh_part()` and the `"Bat"` `MATERIALS` entry; the `Bat` **bone stays**
(every stance/swing/fidget/flip clip animates it, and its per-clip channels
stay). Verified empirically: the exporter does **not** prune a weightless bone
(`export_def_bones` defaults off), and the bone is animated besides — the
`"Bat"` node keeps exporting, so `wire_rigs`' bone lookup and
`model_contract.rs`' ATTACH_BONES-in-joints assertion keep passing.

**Initial library.**

- `BatClassic` — replicates the current in-game bat: 0.713 m knob-to-tip
  (the `Bat` bone's exact head→tail length), 0.032 m barrel radius. Visual
  no-op at swap-in; the resting pose and silhouette do not change.
- `BatLumber` — longer/heavier: smaller sweet segment, higher exit.
- `BatQuick` — shorter/lighter: bigger perfect window, lower exit.

Real regulation dimensions (≈42 in max length, 2.61 in max barrel diameter)
are documented **with sources** in a new `docs/BASEBALL.md` bat section first,
with the arcade deviation (Classic ≈ 0.713 m) noted, per house rules.

## 2. Data model

**`BatId`** is declared in `appearance.rs` via the existing
`appearance_enum!` macro (the serde-pure schema module — the `StanceId`
precedent): variants `Classic` (with `#[default]` **and** `#[serde(other)]`,
like every other appearance enum, so unknown future values degrade to Classic
instead of breaking the file), `Lumber`, `Quick`. The macro's generated
`NAMES`/`VARIANTS` feed the Creator radio grid and the strict-identifier
check.

**`src/game/bat_assets.rs`** (mirrors `model_assets.rs`):

- `BAT_TABLE` const — pins bat scene names + the five marker suffixes.
- `BatSpec` — all five marker positions + three radii in bat-local space,
  plus derived lengths (total, barrel segment, sweet segment). Extracted from
  the loaded `Gltf` by walking each bat root's children and matching name
  suffixes.
- `BatLibrary` resource — `BatId → (scene handle, BatSpec, BatProfile)`.
  Built by polling `Assets<Gltf>` (the `build_rig_animations` pattern).
  Under `--features dev`, rebuilt on `AssetEvent<Gltf>::Modified` so marker
  edits in Blender aren't visually-live-but-data-stale.

**`BatProfile { perfect_scale, solid_scale, exit_scale }`** — the pure
rules-facing collapse, computed by `BatSpec::profile(&self, classic: &BatSpec)`
as geometry ratios against the Classic spec: sweet-segment length ratio →
`perfect_scale`, contact-segment length ratio → `solid_scale`, a
length·radius² mass proxy → `exit_scale` (with a window trade so heavier ≠
strictly better). `BatProfile::NEUTRAL` is all-1.0; `profile(classic,
classic) == NEUTRAL` exactly (ratios of identical f32s are exactly 1.0).
The model **is** the stats — no hand-tuned side table.

## 3. Runtime attachment & swapping

- `wire_rigs`' bat-material show/hide is removed **along with**
  `BAT_MATERIAL`, `RigAnimations.bat_material`, and the `bat_meshes`
  collection — no dead default-handle code left behind. `RigBones.bat`
  (previously consumer-less) becomes the socket.
- New `dress_bats` system in the `gear.rs` style:
  - `.after(player::IdentitySet)`, `.run_if(dressing_active)` (covers Playing
    **and** Creator, so the preview rig dresses).
  - Only on rigs `With<Batter>` — the plate rig and the Creator preview carry
    the marker; run-out rigs spawn `RigUnit::Batter` *without* it and stay
    bat-less (current semantics, pinned by `e2e_gltf_rig.rs`).
  - Uses a **presence-marker** re-stamp guard (the `wire_rigs`
    `Without<...>` pattern plus a stamped `BatId` compare), not pure change
    filters — a rig wired before `bats.glb` finishes loading self-heals
    instead of consuming its trigger. (Known side effect: adding `bat` to
    `PlayerAppearance` makes gear's `DressedAs` compare re-dress on bat
    change; harmless.)
  - On change: `despawn_recursive` the old bat scene, spawn the new bat's
    scene as a child of `RigBones.bat`, offset so `Grip.Knob` sits at the
    bone origin (bone head = the hands). Orientation is identity — the bat's
    +Y is the bone's +Y by the axis convention above; bats are rotationally
    symmetric so roll doesn't matter.
- `CelebrateBatFlip` runs on the `With<Batter>` rig, which holds a bat — the
  flip keeps its bat in hand for free.

## 4. Rules integration (A now, B/C seams)

- `rules::contact_quality`, `rules::pci_contact_quality`, and
  `rules::apply_contact_quality` gain a `&BatProfile` parameter.
  Call sites (enumerated, complete): `flow.rs` swing site (~679/681/712) and
  the `rules.rs` unit tests (which pass `NEUTRAL`). `ai.rs`, `batting.rs`,
  `scenario.rs`, `debug.rs`, and the e2e tests never call these directly.
- **Composition order (pinned):** the profile scales the **base** windows
  first, producing one consistent "effective Ruleset" that Classic, Swing
  Meter, and PCI all see; PCI's miss-fraction shrink applies after.
  `pci_radius_m` untouched in Option A.
- **Window-ordering invariant (pinned):** `foul_ms` is both the grader's
  outer band and the reach gate (`flow::late_swing_z`, Swing Meter's forced
  whiff) and is deliberately unscaled. Required:
  `perfect_scale·perfect_ms ≤ solid_scale·solid_ms ≤ foul_ms` per bat × per
  shipped variant (contract-tested), **and** clamped at profile-application
  time, since the debug Tune tab live-edits windows at runtime.
- `flow.rs` resolves the batter's `BatId` from `Rosters` appearance
  (`rosters.team(batting_team).batting(order.current(...))`) →
  `Option<Res<BatLibrary>>`, grading with `NEUTRAL` while the asset loads.
- **CPU batters always grade with `NEUTRAL`**, mirroring `batting::style_for`'s
  "CPU always bats Classic" rule and decided the same way
  (`controllers.player_index(team) == None`). Bats are cosmetic for the CPU;
  `tests/balance_sim.rs` stays an invariant arbiter with zero retune.
- Debug `ForcedContact` still bypasses grading; the profile's `exit_scale`
  applies to forced qualities (accepted — useful for tuning).
- **B seam:** `BatSpec`'s full 3D geometry stays available in `BatLibrary`; a
  future bat-space contact-point function slots in beside `swing_dt_ms`.
- **C seam:** `Contact.Start→End` + per-point radii define a tapered capsule
  collider, behind its own collision group, later.

## 5. Creator hub & persistence

- `PlayerAppearance` gains `bat: BatId` (struct-level `#[serde(default)]`
  already covers it — the shipped `data/players.ron` carries no bat fields and
  every player resolves `Classic`; hot-reload path unaffected).
- Creator Gear tab adds bat cycling via `radio_grid`, **with a bat-visible
  camera framing**: while the bat row is the focused control the Gear tab
  uses the full-body framing (the Identity/portrait framing), otherwise it
  keeps its head close-up — `camera_target` becomes focused-row-aware for
  this one tab.
- Randomize may roll bats (cosmetic for CPU per the NEUTRAL rule).
- Portraits: unaffected; the bat appears naturally in the Full framing.

## 6. Testing

- **`tests/bat_contract.rs`** — pure `gltf`-crate parsing (no Blender, no
  Bevy app, the `model_contract.rs` pattern): per-bat scenes present; all five
  marker suffixes present as direct children of each bat root; marker scales
  uniform and in a sane radius band; `Contact.Start < Sweet < End` on glTF Y;
  tri budget; embedded-size cap (wasm pays per byte); the window-ordering
  invariant per bat × shipped variant.
- **Unit tests** (`rules.rs` / `bat_assets.rs`):
  `profile(classic, classic) == NEUTRAL` exactly; Lumber/Quick monotonicity
  (each field on the intended side of 1.0); profile-scaled window grading
  including the PCI composition order; CPU-grades-NEUTRAL at the flow helper
  level. (No "shipped roster is all-Classic" test — Creator Save/Randomize
  would legitimately break it; the invariant is CPU-NEUTRAL, not content.)
- **Updated tests:** `model_contract.rs` drops its `Bat`-material assertion,
  budgets adjusted; `e2e_gltf_rig.rs` bat-submesh assertions rewritten
  against `dress_bats` (scene-spawned bat exists under `RigBones.bat` of the
  `Batter` rig only); `appearance_contract.rs` `known_fields()` grows 8→9
  with a `("bat", BatId::NAMES)` row, the `problems.len() == 8` assertion and
  typo fixture and per-field loop updated; `appearance.rs`'s
  `variants_len_matches_names` gains `BatId`; the two `PlayerAppearance`
  struct literals without `..Default::default()` (appearance round-trip test,
  `creator::randomize_player`) gain the field.
- **Invariant:** existing e2e + balance suites pass **unchanged** (every
  default is `Classic → NEUTRAL`).
- `cargo check` on native **and** `wasm32-unknown-unknown`; full `cargo test`
  after touching flow/rules/menu/input/ai.

## Implementation order (suggested for the plan)

1. `docs/BASEBALL.md` bat-dimensions section (sources).
2. `BatId` in `appearance.rs` + appearance/creator contract-test updates.
3. `tools/build_bats.py` + export + `bats.glb` + `tests/bat_contract.rs`.
4. `bat_assets.rs` (`BatSpec`/`BatProfile`/`BatLibrary`) + unit tests.
5. Rules parameterization (`&BatProfile`) + flow resolution + CPU-NEUTRAL +
   invariant clamp + tests.
6. Player-model surgery (`build_player.py`, `model_contract.rs`,
   `wire_rigs` cleanup) + `dress_bats` + `e2e_gltf_rig.rs` rewrite.
7. Creator Gear-tab bat cycling + framing + randomize.
8. Full suite + both-target checks + balance_sim confirmation.
