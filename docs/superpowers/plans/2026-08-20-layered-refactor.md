# Layered Module Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reorganize ~25.7k lines from a flat 31-file `src/game/` into a `core/sim/present/meta` hierarchy with no file over ~700 lines, migrate to edition 2024, and land a mechanical rust-skills lint/code pass — with zero behavior change.

**Architecture:** Five phases, each independently green and committed: (1) edition 2024, (2) pure `git mv` into layer directories behind a `pub use` facade in `game/mod.rs` that preserves every existing `game::<module>` path, (3) big-file splits into submodule directories behind `pub use` facades, (4) lint table + mechanical fixes + constant hoisting, (5) docs + merge. All work on branch `refactor/layered-modules`.

**Tech Stack:** Rust (edition 2024 after Phase 1), Bevy 0.15, Rapier 3D, wasm32-unknown-unknown second target.

**Spec:** `docs/superpowers/specs/2026-08-19-layered-refactor-design.md` — read it first; it defines the target tree, the named Phase-2 exceptions, the sanctioned layer back-references, and what Phase 4 must NOT touch.

## Global Constraints

- **Behavior freeze:** no gameplay, balance, timing, or tuning-value change anywhere. `tests/balance_sim.rs` is the arbiter and must not shift.
- **THE GATE** — run after every task, all must pass before that task's commit:
  ```sh
  export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
  cargo test
  cargo check --target wasm32-unknown-unknown
  cargo check --features "dev debug"
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo clippy --target wasm32-unknown-unknown -- -D warnings
  ```
  CI runs the clippy lines with `-D warnings` on every non-`main` push, so the branch must stay clippy-clean from Task 1 on. Run all cargo commands in the **foreground** (backgrounded cargo stalls in this environment).
- **Facade rule:** `game::rules`, `game::flow`, etc. stay the canonical paths. `game/mod.rs` re-exports every module; tests and internal `use crate::game::…` imports never change. `use`/`pub use` statements referring to the `core` layer must be written `self::core::…` or `crate::game::core::…` — never a bare leading `core::` (collides with the `core` crate).
- **Pure moves:** Phase 2/3 commits contain no semantic edits except the two named exceptions (Task 3: `appearance.rs` include path; `model_assets.rs` + `src/game/models/` do not move at all).
- **Visibility rule:** widen private items only as far as needed — `pub(super)` for sibling submodules inside a split directory; `pub(crate)` only if a different module needs it (none expected; if one appears, note it in the commit message).
- **Helper placement rule (splits):** a private helper moves with its only caller; a helper shared across submodules stays in the split's `mod.rs` (or moves to the submodule of its primary caller as `pub(super)`). Cluster tables below may shift by ±1 helper under this rule — that's fine; inventing new structure is not.
- **Wasm/WebGL2 UI gotcha and all other CLAUDE.md rules remain in force.**
- Commit messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.

---

### Task 1: Branch + green baseline

**Files:** none modified.

**Interfaces:**
- Produces: branch `refactor/layered-modules` with a verified-green starting point all later tasks build on.

- [ ] **Step 1: Create the branch**

```sh
git checkout -b refactor/layered-modules
```

- [ ] **Step 2: Run THE GATE (see Global Constraints)**

Expected: everything passes on the untouched tree. If anything fails here, STOP — the baseline is broken and the refactor must not start; report instead.

- [ ] **Step 3: Record the baseline**

No commit (nothing changed). Note the test count from `cargo test` output in your task report — later tasks must finish with the same count (plus any tests the task itself legitimately relocates).

---

### Task 2: Edition 2024 migration

**Files:**
- Modify: `Cargo.toml` (edition, rust-version)
- Modify: whatever `cargo fix --edition` touches (expected: little to nothing — the crate has no `unsafe`, no `static mut`, no bare `extern`, no RPIT, no lock-holding `if let` patterns)

**Interfaces:**
- Produces: `edition = "2024"`, `rust-version = "1.85"` in `[package]`; all later tasks compile against 2024 semantics.

- [ ] **Step 1: Run the migration fixups against 2021 first**

```sh
export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"
cargo fix --edition --all-targets --all-features
cargo fix --edition --target wasm32-unknown-unknown
```

The second run covers the wasm-only `cfg` branches (`settings.rs` localStorage arm). `cargo fix` may report "no changes" — that's a valid outcome.

- [ ] **Step 2: Bump the edition**

In `Cargo.toml` `[package]`: change `edition = "2021"` to `edition = "2024"` and add `rust-version = "1.85"` on the next line.

- [ ] **Step 3: Compile and fix fallout manually**

`cargo check --all-targets --all-features` and `cargo check --target wasm32-unknown-unknown`. Expected fallout is zero; if errors appear they'll be edition-2024 items (if-let temporary scopes, prelude changes). Fix minimally, changing no behavior. If a fix would alter drop timing in gameplay code, STOP and report — that's a spec-level risk, not an implementer call.

- [ ] **Step 4: Verify the lockfile is undisturbed**

```sh
git diff Cargo.lock
```

Expected: empty, and `grep -A1 'name = "wasm-bindgen"' Cargo.lock | head -4` still shows `0.2.126`. CI derives the wasm-bindgen CLI version from the lockfile; if it changed, restore it (`git checkout Cargo.lock`) and investigate before proceeding.

- [ ] **Step 5: Run THE GATE**

Expected: all green, same test count as Task 1.

- [ ] **Step 6: Commit**

```sh
git add -A
git commit -m "refactor: migrate to Rust edition 2024

cargo fix --edition fallout only; no behavior change.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Layered move (pure `git mv` + facade)

**Files:**
- Create: `src/game/core/mod.rs`, `src/game/sim/mod.rs`, `src/game/present/mod.rs`, `src/game/meta/mod.rs`
- Move (git mv): 26 modules per the table below
- Modify: `src/game/mod.rs` (module declarations → layers + facade re-exports)
- Modify: `src/game/meta/appearance.rs` — ONE literal only (named exception)
- Do NOT move: `src/game/mod.rs`, `src/game/model_assets.rs`, `src/game/models/` (embedded_asset! derives include + `embedded://` paths from the file's location — see spec "Named Phase-2 exceptions")

**Interfaces:**
- Produces: layer modules `game::core`, `game::sim`, `game::present`, `game::meta`; facade re-exports so every pre-existing path (`game::rules`, `game::flow`, …) still resolves. All split tasks (4–13) operate on the moved locations.

Move table (whole files, `git mv src/game/<name>.rs src/game/<layer>/<name>.rs`):

| Layer | Modules |
|---|---|
| `core/` | `rules.rs`, `variant.rs`, `roster.rs`, `theme.rs` |
| `sim/` | `flow.rs`, `fielding.rs`, `runner.rs`, `ball.rs`, `batting.rs`, `ai.rs`, `scenario.rs` |
| `present/` | `field.rs`, `camera.rs`, `player.rs`, `animation.rs`, `ui.rs`, `fx.rs`, `jersey.rs`, `audio.rs`, `juice.rs` |
| `meta/` | `settings.rs`, `menu.rs`, `input.rs`, `subs.rs`, `gear.rs`, `appearance.rs`, `debug.rs`, `portraits.rs`, `creator.rs` |

- [ ] **Step 1: Create layer directories and `git mv` all 26 files per the table**

- [ ] **Step 2: Write the four layer `mod.rs` files**

`src/game/core/mod.rs`:
```rust
pub mod roster;
pub mod rules;
pub mod theme;
pub mod variant;
```

`src/game/sim/mod.rs`:
```rust
pub mod ai;
pub mod ball;
pub mod batting;
pub mod fielding;
pub mod flow;
pub mod runner;
pub mod scenario;
```

`src/game/present/mod.rs`:
```rust
pub mod animation;
pub mod audio;
pub mod camera;
pub mod field;
pub mod fx;
pub mod jersey;
pub mod juice;
pub mod player;
pub mod ui;
```

`src/game/meta/mod.rs` (carries the debug-feature gates, exactly mirroring what `game/mod.rs` has today):
```rust
pub mod appearance;
#[cfg(feature = "debug")]
pub mod creator;
#[cfg(feature = "debug")]
pub mod debug;
pub mod gear;
pub mod input;
pub mod menu;
#[cfg(feature = "debug")]
pub mod portraits;
pub mod settings;
pub mod subs;
```

Check the current `src/game/mod.rs:6-38` before writing these: the gate assignments above must match what exists today (`creator`, `debug`, `portraits` are the gated ones). If they differ, today's file wins.

- [ ] **Step 3: Rewrite the declaration block in `src/game/mod.rs`**

Replace the flat `pub mod …;` block (lines 6–38 today) with:

```rust
pub mod core;
pub mod meta;
pub mod model_assets;
pub mod present;
pub mod sim;

pub use self::core::{roster, rules, theme, variant};
#[cfg(feature = "debug")]
pub use self::meta::{creator, debug, portraits};
pub use self::meta::{appearance, gear, input, menu, settings, subs};
pub use self::present::{animation, audio, camera, field, fx, jersey, juice, player, ui};
pub use self::sim::{ai, ball, batting, fielding, flow, runner, scenario};
```

(`self::core::` is mandatory — bare `core::` in a `use` resolves to the `core` crate.) Touch nothing else in `mod.rs` — `GameState`, `ScoreBoard`, `GamePlugin`, and the two `add_plugins` tuples stay byte-identical.

- [ ] **Step 4: The one named content edit**

In `src/game/meta/appearance.rs`, change the `include_str!` literal (line ~217):
`"../../data/players.ron"` → `"../../../data/players.ron"`.
Nothing else in the file changes. (The `env!("CARGO_MANIFEST_DIR")` watcher path nearby is location-independent — leave it.)

- [ ] **Step 5: Run THE GATE**

Expected: all green, same test count. If `unused_imports` or dead-code warnings fire from the re-export layout, fix the wiring (not the moved files).

- [ ] **Step 6: Commit**

```sh
git add -A
git commit -m "refactor: move modules into core/sim/present/meta layers

Pure git mv behind a pub use facade in game/mod.rs — every existing
game::<module> path still resolves. Named exceptions per spec:
model_assets.rs + models/ stay at src/game/ (embedded_asset! path
derivation); appearance.rs carries one include_str! literal bump
(../../data -> ../../../data) for its new depth.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: Split `core/rules.rs` → `core/rules/`

**Files:**
- Delete: `src/game/core/rules.rs` (3,177 lines: ~1,490 code + ~1,685 tests in one flat `mod tests`)
- Create: `src/game/core/rules/mod.rs`, `pitch.rs`, `contact.rs`, `resolve.rs`, `count.rs`, `advance.rs`, `steal.rs`, `predict.rs`, `test_support.rs`

**Interfaces:**
- Consumes: Task 3's layout.
- Produces: `game::rules::<item>` paths unchanged for every existing public item (via `pub use` in `rules/mod.rs`). `test_support` fixtures available to all rules submodule tests.

Cluster table (items by name as they exist today; apply the Helper placement rule for anything unlisted):

| Target | Items |
|---|---|
| `mod.rs` | consts `GRAVITY`, `CONTACT_HEIGHT`, `PITCH_SPEED`, `PLATE_HALF_WIDTH_M`, `BALL_RADIUS_M`, `ZONE_HALF_WIDTH`, `RIG_*`, `ZONE_LOW`, `ZONE_HIGH`, `TAG_UP_MIN_DIST`, `INFIELD_GATHER_RADIUS`, `POP_RADIUS`, `LINEUP_SIZE`; `Bases` (+impls), `BattingOrder` (+impl), `OutKind`, `Outcome`, `RunnerCall`, `BallCall`, `StrikeCall`; `mound_reset_pos`; `pub use` + `pub(crate) use` lines re-exporting every public item of the submodules; `#[cfg(test)] mod test_support;` |
| `pitch.rs` | `PitchKind` (+impl incl. `from_aim`), `pitch_velocity_kind`, `hit_spin`, `hit_velocity`, `is_in_zone`, `hits_batter`, `hit_by_pitch`, consts `BATTER_X_MIN`, `BATTER_Y_MAX` |
| `contact.rs` | `ContactKind`, `classify_contact`, `ContactClass`, `contact_class`, `RunnerBreak`, `runner_break`, `landed_past_infield`, `GROUNDER_HANG_SECS`, `contact_quality`, `apply_contact_quality`, `pci_contact_quality`, `pci_aim` (and `ContactQuality` if it is defined in rules.rs — if it lives in variant.rs, it stays there) |
| `resolve.rs` | `resolve_catch`, `resolve_gathered`, `throw_target`, `resolve_thrown`, `forced_runner_at`, `home_or_base`, `lead_force`, `runner_call_from_aim`, `aimed_base` |
| `count.rs` | `call_ball`, `call_strike`, `foul`, `charge_out`, `record_out`, `OutPlay`, `apply_batted_out`, `apply_double_play`, `apply_fielders_choice`, `reset_count`, `is_game_over` |
| `advance.rs` | `advance_hit`, `advance_hit_with_jump`, `advance_walk`, `apply_hit`, `advance_runners_only`, `advance_trailing`, `tag_up` |
| `steal.rs` | `steal_candidate`, `StealResult`, `attempt_steal`, `PickoffResult`, `attempt_pickoff`, `double_off_lead_runner` |
| `predict.rs` | `predict_landing`, `predict_landing_from`, `catch_time`, `best_catcher`, `fence_at`, `is_fair` |

Known cross-cluster private calls (become `pub(super)`): `count.rs::apply_batted_out` calls `advance.rs::{advance_trailing, tag_up}` and `steal.rs::double_off_lead_runner`; `resolve.rs` helpers may be used by `count.rs`. Widen exactly these as needed, nothing more.

- [ ] **Step 1: Create the directory and move code clusters**

`mkdir src/game/core/rules`. Move each cluster verbatim (cut-paste, no reformatting beyond rustfmt) into its target file. Each submodule starts with the `use` lines it needs (`use bevy::prelude::*;`, `use crate::game::variant::…`, `use super::…` for sibling items). `mod.rs` declares all submodules and re-exports their public items so `game::rules::resolve_thrown`-style paths are unchanged:

```rust
mod advance;
mod contact;
mod count;
mod pitch;
mod predict;
mod resolve;
mod steal;
#[cfg(test)]
mod test_support;

pub use advance::*;
pub use contact::*;
pub use count::*;
pub use pitch::*;
pub use predict::*;
pub use resolve::*;
pub use steal::*;
```

(Glob re-export is acceptable here because the submodules are private — the facade is the only public surface, exactly what exists today.)

- [ ] **Step 2: Split the test module**

Create `test_support.rs` gated `#[cfg(test)]`, containing the shared fixtures **byte-identical** to today's: `std_rules()`, `pace()`, `empty()`, `with()`, `loaded()` (rules.rs:1499–1521 today) and `std_field()` (~rules.rs:2096), marked `pub(super)`. Distribute the ~1,685 test lines to each submodule's `#[cfg(test)] mod tests` next to the code they exercise; each test module opens with `use super::*;` and `use super::super::test_support::*;`. Every test function moves — none dropped, none rewritten.

- [ ] **Step 3: Run THE GATE**

Expected: all green. `cargo test` total count identical to Task 1's baseline (tests moved, not changed). If a count differs, find the dropped test before proceeding.

- [ ] **Step 4: Commit**

```sh
git add -A
git commit -m "refactor: split rules.rs into core/rules/ submodules

Pure movement behind pub use facade; shared test fixtures extracted to
cfg(test) test_support.rs byte-identical. game::rules::* paths unchanged.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 5: Split `sim/flow.rs` → `sim/flow/`

**Files:**
- Delete: `src/game/sim/flow.rs` (1,408)
- Create: `src/game/sim/flow/mod.rs`, `pitch.rs`, `live.rs`, `result.rs`

**Interfaces:**
- Consumes: Tasks 3–4 layout. `game::rules::*` paths (unchanged).
- Produces: `game::flow::<item>` paths unchanged (Phase, Play, LeadState, BannerTone, events, `swing_dt_ms`, FlowPlugin, …).

| Target | Items |
|---|---|
| `mod.rs` | `Phase`, `Play` (+impls: `in_steal_window`, `pending_call`, `is_home_run`, `last_contact_quality`, …), `LeadState`, `BannerTone`, `BallInPlayEvent`, `LiveBallEvent`, `PitchCaughtEvent`, `ContactEvent`, `PlayBanner`, `PhaseSet`, `FlowPlugin`, submodule decls + `pub use` |
| `pitch.rs` | `swing_dt_ms`, `late_swing_z`, `steal_window_for`, `reset_flow`, `pre_pitch`, `wind_up`, `pitch_live` (keeps its `#[cfg(feature = "debug")] forced: Res<crate::game::debug::ForcedContact>` param — sanctioned back-reference, do not "fix"), `catcher_receives` |
| `live.rs` | `in_play`, `resolve_live_play`, `resolve_contact`, `announce_wall_bang`, `wants_send` |
| `result.rs` | `result_phase`, `hit`, `add_ball`, `add_strike`, `resolve_steal`, `end_pitch` |

`FlowPlugin::build` registers systems now living in submodules — reference them as `pitch::pre_pitch` etc. (or `pub(super)` + plain names via `use`); registration order and run conditions stay byte-identical. If `end_pitch` is called from `live.rs`/`pitch.rs`, mark it `pub(super)`.

- [ ] **Step 1: Create directory, move clusters, wire `mod.rs` (same facade pattern as Task 4 Step 1)**
- [ ] **Step 2: Move any `#[cfg(test)]` tests in flow.rs with their functions**
- [ ] **Step 3: Run THE GATE** — all green, baseline test count.
- [ ] **Step 4: Commit** (message pattern of Task 4: `refactor: split flow.rs into sim/flow/ submodules`, same trailer)

---

### Task 6: Split `present/field.rs` → `present/field/`

**Files:**
- Delete: `src/game/present/field.rs` (1,511)
- Create: `src/game/present/field/mod.rs`, `textures.rs`, `diamond.rs`, `stadium.rs`, `zone.rs`

**Interfaces:**
- Produces: `game::field::<item>` unchanged (markers, `FieldPlugin`, `PciCursorMarker`, consts `HALF_DIAGONAL`/`PITCH_DISTANCE` — which Task 15 later hoists to core).

| Target | Items |
|---|---|
| `mod.rs` | markers `GroundPlane`, `Base`, `PitchersMound`, `FoulPole`, `OutfieldWall`, `StrikeZoneOverlay`, `PciCursorMarker`, `ZoneFlash`, `FieldSurfaces`; consts (`HALF_DIAGONAL`, `PITCH_DISTANCE`, …); `FieldPlugin` |
| `textures.rs` | `grass_image`, `dirt_image`, `tiling_image` |
| `diamond.rs` | `spawn_field`, `spawn_bases`, `distance_point_to_segment`, `on_box_outline`, `spawn_flat_chalk`, `spawn_chalk_segment`, `spawn_batters_box`, `foul_line_span`, `spawn_foul_line`, `spawn_chalk_lines`, `spawn_stadium_mound` |
| `stadium.rs` | `spawn_ground_slab`, `spawn_stadium_ground`, `spawn_front_yard`, `spawn_foul_poles`, `spawn_outfield_wall`, `spawn_lighting` |
| `zone.rs` | `spawn_strike_zone`, `strike_zone_visibility`, `pci_cursor_visibility`, `trigger_zone_flash`, `restore_zone_flash` |

- [ ] **Step 1: Create directory, move clusters, wire `mod.rs` facade**
- [ ] **Step 2: Run THE GATE** — all green, baseline test count.
- [ ] **Step 3: Commit** (`refactor: split field.rs into present/field/ submodules`, same trailer)

---

### Task 7: Split `meta/creator.rs` → `meta/creator/` (debug feature)

**Files:**
- Delete: `src/game/meta/creator.rs` (1,313)
- Create: `src/game/meta/creator/mod.rs`, `panel.rs`, `preview.rs`, `randomize.rs`, `persist.rs`

**Interfaces:**
- Produces: `game::creator::<item>` unchanged. The whole directory stays behind `#[cfg(feature = "debug")]` at `meta/mod.rs` — the submodule files themselves carry no cfg gates.

| Target | Items |
|---|---|
| `mod.rs` | `CreatorTab`, `CreatorState`, `selected_def`, `selected_def_ref`, `preview_rosters_and_identity`, `enter_creator`, `exit_creator`, `apply_creator_edits`, `revert_creator_edits`, `sync_creator_from_external_reload`, `LastAppliedRoster`, `CreatorPlugin` |
| `panel.rs` | `creator_panel`, `render_creator_panel`, `render_identity_tab`, `radio_grid`, `render_gear_tab`, `render_colors_tab`, `render_animations_tab` |
| `preview.rs` | `PreviewRig`, `CreatorStage`, `PreviewKey`, `enter_creator_stage`, `exit_creator_stage`, `camera_target`, `lerp_creator_camera`, `preview_idle`, `retint_preview` |
| `randomize.rs` | `roll`, `pick_uniform`, `pick_headwear`, `pick_eyewear`, `pick_fidget`, `pick_celebration`, `randomize_player` |
| `persist.rs` | `save_working_to`, `save_working` |

- [ ] **Step 1: Create directory, move clusters, wire `mod.rs` facade**
- [ ] **Step 2: Run THE GATE** — the `cargo check --features "dev debug"` line is the one that actually compiles this code; also run `cargo test --features "dev debug"` this task (creator e2e tests live behind the feature).
- [ ] **Step 3: Commit** (`refactor: split creator.rs into meta/creator/ submodules`, same trailer)

---

### Task 8: Split `present/camera.rs` → `present/camera/`

**Files:**
- Delete: `src/game/present/camera.rs` (880)
- Create: `src/game/present/camera/mod.rs`, `framing.rs`, `rigs.rs`

**Interfaces:**
- Produces: `game::camera::<item>` unchanged (`CameraMode`, `DuelView`, `BALL_FOLLOW_DELAY`, pure framing fns used by tests).

| Target | Items |
|---|---|
| `mod.rs` | `CameraMode`, `DuelView` (+impl), `is_broadcast`, `is_orbit`, `toggle_duel_view`, `spawn_camera`, `toggle_camera_mode`, consts (`BALL_FOLLOW_DELAY`, …), `CameraPlugin` |
| `framing.rs` | `aspect_safe_duel_vfov`, `framed_ndc_y`, `framed_height_fraction`, `occludes`, `trot_orbit_eye`, `duel_framing_wanted` (pure math + their unit tests) |
| `rigs.rs` | `OrbitState`, `BroadcastRig`, `CameraKick`, `broadcast_camera`, `orbit_camera`, `zoom_camera`, `orbit_transform`, `hide_occluders`, `kick_on_hit`, `kick_on_wall_bang`, `decay_kick` |

- [ ] **Step 1: Create directory, move clusters (tests travel), wire `mod.rs` facade**
- [ ] **Step 2: Run THE GATE** — all green, baseline test count.
- [ ] **Step 3: Commit** (`refactor: split camera.rs into present/camera/ submodules`, same trailer)

---

### Task 9: Split `present/player.rs` → `present/player/`

**Files:**
- Delete: `src/game/present/player.rs` (865)
- Create: `src/game/present/player/mod.rs`, `rig.rs`, `behavior.rs`

**Interfaces:**
- Produces: `game::player::<item>` unchanged; `spawn_rig`, `build_rig_model`, `build_materials`, `sync_identities` keep their current `pub(crate)` visibility.

| Target | Items |
|---|---|
| `mod.rs` | markers `Pitcher`, `Batter`, `Fielder`, `CatcherRole`, `PlateUmpire`, `FacingDirection`, `RigUnit`, `RigUnitTag`, `GltfRig`; `PlayerPlugin` |
| `rig.rs` | `PartKind`, `RigPart`, `build_materials`, `umpire_materials`, `build_rig_model`, `spawn_players`, `spawn_rig`, `recolor_teams`, `recolor_gltf`, `sync_identities` |
| `behavior.rs` | `batter_stance`, `reset_batter_fidget_timer`, `batter_fidgets`, `catcher_crouch`, `trigger_swing`, `celebrate_home_run` |

Ordering caution: `sync_identities` participates in a pinned system ordering (`sync_runners` → `sync_identities`, per project memory) — registration in `PlayerPlugin` must stay byte-identical.

- [ ] **Step 1: Create directory, move clusters, wire `mod.rs` facade**
- [ ] **Step 2: Run THE GATE** — all green (e2e_identity.rs and e2e_gltf_rig.rs are the sensitive suites here).
- [ ] **Step 3: Commit** (`refactor: split player.rs into present/player/ submodules`, same trailer)

---

### Task 10: Split `meta/settings.rs` → `meta/settings/`

**Files:**
- Delete: `src/game/meta/settings.rs` (801)
- Create: `src/game/meta/settings/mod.rs`, `screen.rs`

**Interfaces:**
- Produces: `game::settings::<item>` unchanged (`Settings`, `BattingStyle`, `SettingsOpen`, `load_settings`/`save_settings`, `settings_closed`).

| Target | Items |
|---|---|
| `mod.rs` | `BattingStyle`, `PitchTrailStyle`, `TrailColor`, `Settings`, `SettingsOpen`, `SettingsPlugin`, `default_true`, `store_path`, `load_settings`, `save_settings`, both cfg pairs of `read_store`/`write_store`, `local_storage`, `apply_volume`, `persist_settings`, `settings_closed` — i.e. **all** cfg-gated persistence stays in `mod.rs` |
| `screen.rs` | markers `SettingsUi`, `SettingsCard`, `SettingsTitle`, `SettingsRowLabel`, `SettingsRowText`, `SettingsCursorRow`; `spawn_settings_screen`, `paint_settings_screen`, `toggle_settings`, `edit_settings`, `close_settings_on_exit` |

- [ ] **Step 1: Create directory, move clusters, wire `mod.rs` facade**
- [ ] **Step 2: Run THE GATE** — `cargo check --target wasm32-unknown-unknown` is the line that proves the cfg pair survived; `tests/e2e_settings.rs` exercises the `BREAKNECK_SETTINGS_PATH` seam.
- [ ] **Step 3: Commit** (`refactor: split settings.rs into meta/settings/ submodules`, same trailer)

---

### Task 11: Split `present/animation.rs` → `present/animation/`

**Files:**
- Delete: `src/game/present/animation.rs` (771)
- Create: `src/game/present/animation/mod.rs`, `poses.rs`, `driver.rs`

**Interfaces:**
- Produces: `game::animation::<item>` unchanged (`AnimClip`, `Playing`, `MoveIntent`, `RigBaseY`, clip lookup helpers). The `AnimClip` exhaustive matches (`duration()`/`looping()`/`limb_pose()`) keep their no-wildcard-arm structure — the compiler-walks-you-through-new-clips property is load-bearing per CLAUDE.md.

| Target | Items |
|---|---|
| `mod.rs` | `AnimClip` + inherent impls, `Playing`, `MoveIntent`, `RigBaseY`, `RigPlayer` (if present), `stance_clip`, `fidget_clip`, `celebration_clip`, `is_stance`, `is_fidget`, `AnimationPlugin` |
| `poses.rs` | `ease_out`, `bat_idle_rotation`, `bat_sweep_rotation`, `self_pose`, `limb_pose`, `root_drop`, `root_pitch` |
| `driver.rs` | `sample_clips`, `settle_removed`, `meter_stance_sink`, `locomote`, `start_clip`, `drive_graph_rigs`, `idle_graph_rigs`, `settle_graph_removed` |

- [ ] **Step 1: Create directory, move clusters, wire `mod.rs` facade**
- [ ] **Step 2: Run THE GATE** — all green (`tests/e2e_gltf_rig.rs`, `tests/model_contract.rs` sensitive).
- [ ] **Step 3: Commit** (`refactor: split animation.rs into present/animation/ submodules`, same trailer)

---

### Task 12: Split `present/ui.rs` → `present/ui/`

**Files:**
- Delete: `src/game/present/ui.rs` (740)
- Create: `src/game/present/ui/mod.rs`, `hud.rs`, `banner.rs`

**Interfaces:**
- Produces: `game::ui::<item>` unchanged (`hidden_tint` keeps `pub(crate)`).

| Target | Items |
|---|---|
| `mod.rs` | `hidden_tint`, all UI marker components, `UiPlugin` |
| `hud.rs` | `spawn_hud`, `spawn_base_ring`, `update_inning_text`, `update_score_text`, `update_count_dots`, `update_meter_bar`, `update_base_ring` |
| `banner.rs` | `spawn_duel_panels`, `update_duel_panels`, `show_banner`, `fade_banner`, `show_contact_stamp`, `fade_contact_stamp` |

Wasm UI rule applies unchanged: spawn-time painting and `hidden_tint` alpha discipline must not be disturbed by the move.

- [ ] **Step 1: Create directory, move clusters, wire `mod.rs` facade**
- [ ] **Step 2: Run THE GATE** — all green.
- [ ] **Step 3: Commit** (`refactor: split ui.rs into present/ui/ submodules`, same trailer)

---

### Task 13: Split `present/fx.rs` → `present/fx/`

**Files:**
- Delete: `src/game/present/fx.rs` (730)
- Create: `src/game/present/fx/mod.rs`, `trail.rs`, `particles.rs`

**Interfaces:**
- Produces: `game::fx::<item>` unchanged.

| Target | Items |
|---|---|
| `mod.rs` | `FxPlugin`, `start_hit_stop`, `end_hit_stop`, shared asset resources/handles types |
| `trail.rs` | `trail_spacing`, `trail_lifetime`, `fade_step`, `should_drop`, `trail_mesh`, `build_trail_assets`, `pitch_trail`, `tick_trail` |
| `particles.rs` | `build_fx_assets`, `spawn_landing_ring`, `update_landing_ring`, `contact_burst`, `wall_bang_burst`, `home_run_fireworks`, `bounce_dust`, `tick_particles` |

- [ ] **Step 1: Create directory, move clusters, wire `mod.rs` facade**
- [ ] **Step 2: Run THE GATE** — all green. This completes Phase 3: verify no file under `src/game/` exceeds ~700 lines (`wc -l $(find src/game -name '*.rs') | sort -rn | head`), excepting only files the spec left whole (`variant.rs` at 699 is the ceiling case).
- [ ] **Step 3: Commit** (`refactor: split fx.rs into present/fx/ submodules`, same trailer)

---

### Task 14: Lint table + warning fixes

**Files:**
- Modify: `Cargo.toml` (add `[lints]`)
- Modify: whatever the lints flag (expected small: CI already enforces default clippy `-D warnings`)

**Interfaces:**
- Produces: explicit lint policy all future code compiles under.

- [ ] **Step 1: Add the lint table to `Cargo.toml`**

```toml
[lints.rust]
unexpected_cfgs = "warn"

[lints.clippy]
correctness = { level = "deny", priority = -1 }
suspicious = { level = "warn", priority = -1 }
style = { level = "warn", priority = -1 }
complexity = { level = "warn", priority = -1 }
perf = { level = "warn", priority = -1 }
```

(Cargo auto-declares crate features for `unexpected_cfgs`; the custom `--cfg getrandom_backend` rustflags in `.cargo/config.toml` are never used in `#[cfg(...)]` in `src/`, so no `check-cfg` entries are needed.)

- [ ] **Step 2: Fix findings, layer by layer (core → sim → present → meta)**

Run `cargo clippy --all-targets --all-features 2>&1 | head -100`, fix mechanically, re-run. Rules: no behavior change; existing `#[allow(clippy::…)]` annotations stay unless the lint no longer fires; if a fix would change floating-point evaluation order or timing in gameplay code, `#[allow]` it with a one-line reason instead.

- [ ] **Step 3: Run THE GATE** — all green, baseline test count.
- [ ] **Step 4: Commit** (`chore: add lint table and fix clippy findings`, same trailer)

---

### Task 15: Constant hoisting into core

**Files:**
- Modify: `src/game/core/rules/mod.rs` (receives consts), `src/game/present/field/mod.rs`, `src/game/sim/ball.rs`, `src/game/core/variant.rs`

**Interfaces:**
- Produces: `field::{HALF_DIAGONAL, PITCH_DISTANCE}` and `ball::BALL_RADIUS` still resolve at their old paths via `pub use` shims; canonical definitions live in `core/rules/mod.rs`.

- [ ] **Step 1: Move the constants verbatim**

Move `HALF_DIAGONAL` and `PITCH_DISTANCE` from `field/mod.rs` and `BALL_RADIUS` from `ball.rs` into `core/rules/mod.rs` (values byte-identical). At each old home leave a shim: `pub use crate::game::rules::{HALF_DIAGONAL, PITCH_DISTANCE};` / `pub use crate::game::rules::BALL_RADIUS;`. Update `variant.rs` and `rules.rs` imports to pull from `core` directly (removing the sanctioned upward references).

- [ ] **Step 2: BALL_RADIUS dedup check**

Compare `ball::BALL_RADIUS` and `rules::BALL_RADIUS_M` values. If **identical**, collapse to one const (keep the `BALL_RADIUS_M` name, shim the other). If they differ at all, keep both and add a one-line comment on each stating why (they encode different things); do not "fix" the difference.

- [ ] **Step 3: Run THE GATE** — all green, baseline test count (balance_sim especially — constants feeding physics must not have drifted).
- [ ] **Step 4: Commit** (`refactor: hoist shared geometry/physics constants into core`, same trailer)

---

### Task 16: Hot-path memory pass

**Files:**
- Modify (audit list — change only what the rule below allows): `src/game/present/jersey.rs`, `src/game/present/field/textures.rs`, `src/game/present/fx/trail.rs`, `src/game/present/fx/particles.rs`, `src/game/present/ui/hud.rs`

**Interfaces:** none new — internal-only changes.

Rule: apply `mem-with-capacity` (pre-size a `Vec` whose final length is computable at the allocation site) and `mem-avoid-format`/`mem-write-over-format` (replace `format!` with a literal or `write!` into an existing buffer) **only where the fix is locally provable** — same bytes produced, no logic change. Anything requiring restructuring is out of scope; list it in the task report instead of doing it.

- [ ] **Step 1: Audit the five files**

For each: find `Vec::new()`/`vec![]` where final size is known (`with_capacity`), `String` churn in per-frame systems, `format!` where a literal works. Note that HUD text updates are usually change-detection-guarded (`Res<ScoreBoard>` change ticks) — a `format!` behind a change guard is NOT hot; leave it and say so in the report.

- [ ] **Step 2: Apply the provable fixes only**

Example shape (illustrative — apply to what the audit actually finds):
```rust
// before
let mut data = Vec::new();
for _ in 0..size * size { /* push 4 bytes */ }
// after
let mut data = Vec::with_capacity((size * size * 4) as usize);
```

- [ ] **Step 3: Run THE GATE** — all green, baseline test count.
- [ ] **Step 4: Commit** (`perf: pre-size known-length allocations in texture/fx/jersey paths`, same trailer)

---

### Task 17: `#[must_use]` + idiom pass on core/rules

**Files:**
- Modify: `src/game/core/rules/*.rs`

**Interfaces:** none new — attributes and equivalent-code rewrites only.

- [ ] **Step 1: Add `#[must_use]`**

Candidates: types `Outcome`, `OutPlay`, `ContactQuality` (if in rules), `StealResult`, `PickoffResult`, `BallCall`, `StrikeCall`, `RunnerBreak`; functions returning runs scored (`advance_hit`, `advance_hit_with_jump`, `advance_walk`, `apply_hit`, `hit_by_pitch`). Before each: check every call site — if any deliberately drops the value, skip that item and note it (adding `let _ =` churn to satisfy an attribute is backwards).

- [ ] **Step 2: Idiom fixes flagged by clippy but deferred, if any remain**

`matches!` for boolean pattern tests, `let-else` for early returns — only where clippy suggested in Task 14 and the fix was deferred. No new hunting.

- [ ] **Step 3: Run THE GATE** — all green, baseline test count.
- [ ] **Step 4: Commit** (`chore: must_use annotations and idiom fixes in core/rules`, same trailer)

---

### Task 18: Docs sweep

**Files:**
- Modify: `CLAUDE.md`, `docs/BASEBALL.md`, `TADA.md` (and `TODO.md` if a matching queued item exists — re-read it first per project convention)

**Interfaces:** none — prose only.

- [ ] **Step 1: Update CLAUDE.md's Architecture section**

Rewrite the module-path references for the new layout: `src/game/<module>.rs` mentions become their new homes (e.g. `rules.rs` → `src/game/core/rules/`); add a short paragraph after the plugin-registration sentence stating: the four layers (`core` = rules/data, `sim` = gameplay systems, `present` = audiovisual, `meta` = shell/persistence/tooling); the facade rule (new modules must be declared in their layer's `mod.rs` AND re-exported from `game/mod.rs`; `game::<module>` stays the canonical path); layering is a reading aid, not an enforced dependency rule, and the one debug-only `sim`→`meta` back-reference (`flow::pitch_live` reading `debug::ForcedContact`) is deliberate; `model_assets.rs` + `models/` must stay at `src/game/` top level because `embedded_asset!` derives paths from the file location.

- [ ] **Step 2: Sweep `docs/*.md` citations to module level**

`docs/BASEBALL.md:11` (`src/game/animation.rs` → `game::animation`) and `docs/BASEBALL.md:186` (`src/game/rules.rs` → `game::rules`); grep `docs/ -rn 'src/game'` for any others and reword the same way.

- [ ] **Step 3: Log completion**

Add a TADA.md entry (match its existing format); check off the TODO.md item if one covers this work.

- [ ] **Step 4: Run THE GATE** (docs shouldn't break it, but the gate is unconditional) and commit (`docs: update CLAUDE.md and doc citations for layered layout`, same trailer)

---

### Task 19: Merge and CI verification

**Files:** none (git operations).

- [ ] **Step 1: Final full verification on the branch**

THE GATE, plus `cargo test --features "dev debug"` and `cargo run`-free sanity: `cargo build --target wasm32-unknown-unknown` (full build, not just check).

- [ ] **Step 2: Merge**

```sh
git checkout main
git pull --ff-only
git merge --no-ff refactor/layered-modules -m "refactor: layered module hierarchy, edition 2024, lint pass

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push
```

If `main` moved since the branch was cut, rebase the branch first, re-run THE GATE, then merge.

- [ ] **Step 3: Watch CI**

`gh run watch` (or `gh run list --limit 2`) until both the CI workflow and the Pages deploy for the merge commit are green. If Pages fails on wasm-bindgen versioning, check `Cargo.lock` still pins 0.2.126 — that invariant was verified in Task 2 and must still hold.

---

## Self-Review Notes

- Spec coverage: Phase 1 → Task 2; Phase 2 (+ both named exceptions + facade) → Task 3; Phase 3 all ten splits → Tasks 4–13 (rules test_support in Task 4); Phase 4 lint table → 14, constant hoist + BALL_RADIUS dedup → 15, hot-path mem → 16, must_use/idioms → 17, out-of-scope list enforced by task rules; Phase 5 docs → 18, merge/CI → 19. Sanctioned back-references: flow debug param preserved in Task 5, constants resolved in Task 15.
- Cluster tables use item names verified against the current source by grep during design; line numbers are deliberately avoided (they drift after Tasks 2–3). The Helper placement rule covers unlisted private helpers.
- Baseline test-count tracking (Task 1 Step 3) is the drift detector every later task checks against.
