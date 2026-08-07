# Player Creation Hub — Phase 4: The Creator Stage — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The hub itself: a dev-gated Creator stage (menu → **C** in debug builds) with a lit preview rig, egui editing panel (Identity / Gear / Colors / Animations), per-tab camera framing and preview clips, curated randomize, revert-on-entry-snapshot, save-to-RON — plus the portrait harness that gives the AI eyes, and the QA/hardening sweep the earlier phases parked.

**Architecture:** A `#[cfg(feature = "debug")]` `GameState::Creator` variant with its own stage (ground, three-point light, one preview rig spawned by the shared `spawn_rig`, own camera). The dressing/wiring/jersey/animation systems that were `Playing`-gated get a shared `dressing_active` run condition (Playing OR Creator) so the *same* systems dress the preview — the honesty property: the panel only mutates data (`CreatorState` working copy → `RosterDefs` + `Rosters`), and the ordinary pipeline reacts. Save serializes the working copy to `data/players.ron` (validated first); the Phase-1 dev watcher then treats it as the new truth. `portraits.rs` drives a windowed run that cycles every player through two framings and screenshots each to PNG (Bevy 0.15 `Screenshot` + `save_to_disk`), then exits.

**Tech Stack:** Bevy 0.15, egui (via the existing `debug` feature's bevy-inspector-egui dependency), `ron` pretty serializer, Bevy screenshot API.

**Spec:** `docs/superpowers/specs/2026-08-07-player-creation-hub-design.md` §5 (hub), §6 (AI access — portraits), §8 Phase 4. This phase also executes the parked QA/cleanup items listed in Task 5.

## Global Constraints

- PATH prefix for cargo: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`. Blender (if needed for Task 5 helmet work): `/Applications/Blender.app/Contents/MacOS/Blender` (not on PATH), always the sacred pair.
- EVERYTHING in this phase except Task 5's hardening items is `#[cfg(feature = "debug")]`-gated: zero code, zero variants, zero systems in shipping/wasm builds. `cargo check` (no features) and `cargo check --target wasm32-unknown-unknown` prove it every task.
- Full `cargo test` green EXCEPT the two known pre-existing failures (`e2e_camera_views::cycling_v_changes_view_and_toggles_the_catchers_visibility`, `e2e_settings::settings_edit_persists_and_game_starts`). Debug-gated tests run via `cargo test --features debug`.
- The panel mutates only data (`CreatorState`/`RosterDefs`/`Rosters`) — never meshes/materials directly (spec §5's honesty property).
- Save must never write an invalid file: `RosterDefs::validate` gates every write; a failed validation surfaces in the panel, leaves the file untouched.
- clippy `-D warnings` (with and without `--features "dev debug"`), fmt.
- Commit per task, ending:
Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>

---

### Task 1: `GameState::Creator` + stage + shared-pipeline gating

**Files:**
- Modify: `src/game/mod.rs` (cfg'd `Creator` variant; `dressing_active` run condition; register `CreatorPlugin` cfg'd)
- Create: `src/game/creator.rs` (state transitions, stage spawn/teardown, preview rig, camera, its plugin — panel comes in Task 2)
- Modify: `src/game/model_assets.rs`, `src/game/gear.rs`, `src/game/jersey.rs`, `src/game/animation.rs` (swap `run_if(in_state(GameState::Playing))` → `run_if(dressing_active)` on: `wire_rigs`, `dress_rigs`, `dress_jerseys` + `mount_jerseys_on_bones` + the identity chain, and the glTF/Blocky animation driver systems — exactly the systems the preview rig needs; check each file's registrations and list what you switched in your report)
- Modify: `src/game/menu.rs` (menu text hint, cfg'd)
- Test: `tests/e2e_creator.rs` (debug-feature-gated)

**Interfaces:**
- Produces: `GameState::Creator` (cfg debug); `mod.rs::dressing_active(state: Res<State<GameState>>) -> bool` (pub); `creator::PreviewRig` marker (pub); `creator::CreatorState` resource skeleton `{ pub team: Team, pub index: usize }` (grows in Task 2); preview rig carries `PlayerIdentity { team, index }` refreshed on selection change.

- [ ] **Step 1: The run condition + variant (compile-driven)**

In `mod.rs`:

```rust
/// The rig-dressing/wiring/animation pipeline runs while gameplay OR the
/// dev Creator stage is active — the Creator's honesty property is that
/// the exact same systems dress the preview rig.
pub fn dressing_active(state: Res<State<GameState>>) -> bool {
    #[cfg(feature = "debug")]
    {
        matches!(state.get(), GameState::Playing | GameState::Creator)
    }
    #[cfg(not(feature = "debug"))]
    {
        matches!(state.get(), GameState::Playing)
    }
}
```

Add the variant:

```rust
    /// Dev-only player-creation stage (menu → C in debug builds).
    #[cfg(feature = "debug")]
    Creator,
```

`cargo check --features debug` — fix any exhaustive `match` over `GameState` the compiler reveals (expected: none; run conditions and OnEnter/OnExit don't match exhaustively). `cargo check` (no features) must stay clean — proves the cfg discipline.

- [ ] **Step 2: Swap the run conditions**

Each listed system's `run_if(in_state(GameState::Playing))` becomes `run_if(crate::game::dressing_active)`. Do NOT switch gameplay systems (flow, fielding, runner, input, batter systems) — only the rig pipeline: wiring, dressing (skin/gear/jerseys/identity chain), animation drivers. Run the full suite — everything must stay green (in Playing the condition is identical).

- [ ] **Step 3: The Creator stage**

`src/game/creator.rs` (whole module `#![cfg(feature = "debug")]` via cfg on the `mod` declaration in mod.rs):

- `CreatorState { pub team: Team, pub index: usize }` resource (Default: Home, 0).
- `enter_creator`: on MainMenu, `keyboard.just_pressed(KeyCode::KeyC)` → `next_state.set(GameState::Creator)`.
- `exit_creator`: in Creator, Esc → `next_state.set(GameState::MainMenu)` (revert wiring lands in Task 2).
- `OnEnter(Creator)`: spawn (all with a `CreatorStage` marker for teardown): a 12×12 ground plane (reuse the field's grass color or a neutral `Color::srgb(0.25, 0.45, 0.25)`), three point/directional lights (key/fill/rim — intensities to taste), a `Camera3d` at roughly `(1.8, 1.6, 2.6)` looking at `(0, 1.0, 0)`, and the preview rig: construct the `RigModel` the same way `spawn_players` does (match on `theme.player_model`; extract that construction into a small `pub(crate) fn build_rig_model(...)` in player.rs rather than duplicating it), call `spawn_rig` at `Vec3::new(0.0, 0.6, 0.0)` facing the camera, insert `PreviewRig` + `PlayerIdentity { team: cs.team, index: cs.index }`, and `attach_jerseys` (create `JerseyAssets` via `jersey::make_assets` if the resource is absent — game start normally makes it).
- `OnExit(Creator)`: despawn_recursive everything `With<CreatorStage>` (rig included — respawned fresh next entry).
- `sync_preview_identity`: in Creator, when `CreatorState` changed, re-insert `PlayerIdentity` on the preview rig (the dress pipeline reacts through the normal `Changed` path).
- `preview_idle`: in Creator, a preview rig with no `Playing` gets `Playing::new(AnimClip::Idle)` (Task 3 makes this tab-aware).
- `CreatorPlugin` registers all of it (`OnEnter`/`OnExit` + `Update` systems `run_if(in_state(GameState::Creator))`, except `enter_creator` which runs in MainMenu); `mod.rs` adds `#[cfg(feature = "debug")] app.add_plugins(creator::CreatorPlugin);` beside `DebugPlugin`.
- `menu.rs`: add a cfg'd hint line to the menu text ("C — player creator" — follow how existing menu lines are built).

Note on `sync_identities`: it only queries `RosterRole` rigs; the preview rig has none, so the gameplay identity stamper never fights the creator's stamps. Say this in a comment.

- [ ] **Step 4: The e2e**

`tests/e2e_creator.rs`, first line `#![cfg(feature = "debug")]`:

```rust
//! Creator stage e2e (debug builds): C enters, the preview rig dresses
//! through the SAME pipeline gameplay uses, Esc leaves.
#![cfg(feature = "debug")]

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::creator::{CreatorState, PreviewRig};
use breakneck_baseball::game::gear::DressedAs;
use breakneck_baseball::game::GameState;
use common::{headless_app, run_until, tap_key};

#[test]
fn creator_stage_dresses_the_preview_rig() {
    let mut app = headless_app();
    tap_key(&mut app, KeyCode::KeyC);
    let entered = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Creator
    });
    assert!(entered.is_some(), "C on the menu must open the creator");
    let dressed = run_until(&mut app, 5_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&DressedAs, With<PreviewRig>>()
            .iter(world)
            .next()
            .is_some()
    });
    assert!(dressed.is_some(), "the preview rig must dress via the shared pipeline");
    // Selection change re-dresses: pick the away team's slot 2.
    {
        let mut cs = app.world_mut().resource_mut::<CreatorState>();
        cs.team = breakneck_baseball::game::Team::Away;
        cs.index = 2;
    }
    let redressed = run_until(&mut app, 1_000, |app| {
        let world = app.world_mut();
        world
            .query_filtered::<&DressedAs, With<PreviewRig>>()
            .iter(world)
            .next()
            .map(|d| d.team() == breakneck_baseball::game::Team::Away)
            .unwrap_or(false)
    });
    assert!(redressed.is_some(), "selection change must re-dress the preview");
    tap_key(&mut app, KeyCode::Escape);
    let left = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::MainMenu
    });
    assert!(left.is_some(), "Esc must return to the menu");
}
```

Run with `cargo test --features debug --test e2e_creator` (RED first — the state doesn't exist; then GREEN). Note: the egui panel isn't spawned headless-safely yet — Task 2 must keep panel systems tolerant of a missing egui context (they no-op).

- [ ] **Step 5: Suite + both-cfg checks + commit**

`cargo test` AND `cargo test --features debug --test e2e_creator`; `cargo check` (no features) + wasm check; clippy both feature sets; fmt.

```bash
git add src/game/mod.rs src/game/creator.rs src/game/model_assets.rs src/game/gear.rs src/game/jersey.rs src/game/animation.rs src/game/player.rs src/game/menu.rs tests/e2e_creator.rs
git commit -m "feat: dev Creator stage — state, stage, preview rig through the shared dress pipeline"
```

---

### Task 2: The panel — selector, tabs, live edit, revert

**Files:**
- Modify: `src/game/creator.rs` (CreatorState grows; egui panel system; revert)
- Modify: `src/game/appearance.rs` (extend `appearance_enum!` with a `VARIANTS: &[Self]` const alongside `NAMES`)

**Interfaces:**
- Produces: `CreatorState { team, index, pub tab: CreatorTab, pub working: RosterFile, pub snapshot: RosterFile, pub status: String }`; `CreatorTab { Identity, Gear, Colors, Animations }`; pure helpers unit-tested: `selected_def(&mut RosterFile, team, index) -> &mut PlayerDef` (index spans lineup+bench, 0..13), `apply_working(world-side): working → RosterDefs + Rosters` (see below).

- [ ] **Step 1: `VARIANTS` in the macro + unit tests**

Extend `appearance_enum!` so each enum also gets `pub const VARIANTS: &[Self] = &[...]` (same token list as NAMES — compiler-structural, no drift). Unit test: `VARIANTS.len() == NAMES.len()` for every appearance enum.

- [ ] **Step 2: State + pure ops (TDD)**

`CreatorState` gains `tab`, `working: RosterFile` (initialized from `RosterDefs` on Creator entry), `snapshot: RosterFile` (same moment), `status: String`. Pure fns with unit tests (no ECS):

```rust
/// The selected player's def — index 0..(pool len) spans lineup then bench.
pub fn selected_def(file: &mut RosterFile, team: Team, index: usize) -> &mut PlayerDef { ... }

/// Preview identity for a selection: bench players (index >= LINEUP_SIZE)
/// can't be addressed by TeamRoster::card, so preview-Rosters swaps the
/// selected bench player into lineup slot 0 and the identity says slot 0.
pub fn preview_rosters_and_identity(
    working: &RosterFile, team: Team, index: usize,
) -> (Rosters, PlayerIdentity) { ... }
```

Tests: selecting bench index 10 yields identity index 0 and a Rosters whose team lineup slot 0 is that player; selecting lineup index 3 yields identity index 3 with an unmodified ordering.

- [ ] **Step 3: The egui panel**

One `egui::SidePanel::left` system (in Creator only; take `Option<ResMut<bevy_egui::EguiContexts>>`-equivalent per how debug.rs gets its context — if the context is absent (headless tests), return):

- **Selector**: team toggle (Home/Away) + a scrollable list of all 13 names/numbers; clicking selects (updates `team`/`index`).
- **Tabs**: `Identity` (name text edit — uppercase-filtered A–Z ≤ 8, number drag 0..99), `Gear` (radio grids from `Headwear::VARIANTS`/`Eyewear::VARIANTS`/`Arms::VARIANTS` + chain checkbox — labels from `NAMES`), `Colors` (skin swatch row: color buttons from `SkinTone::VARIANTS` painted with `tone.color()`), `Animations` (stance/fidget/celebration radio grids; fidget includes a None option).
- Every edit mutates `selected_def(&mut cs.working, ...)` — nothing else. A change-application system (separate from the panel, `Changed<CreatorState>`-driven or a dirty flag) rebuilds: `*roster_defs = RosterDefs(cs.working.clone()); let (rosters, id) = preview_rosters_and_identity(...); *live_rosters = rosters;` and re-inserts `id` on the preview rig. (Mutating `RosterDefs` live means the Phase-1 hot-reload watcher's equality check sees the working copy — it won't clobber it with the on-disk file unless the disk changes; note this interaction in a comment.)
- **Revert button** (and automatic on Esc-exit): `cs.working = cs.snapshot.clone()` + reapply. **Save/Randomize/Portraits buttons** land in Tasks 3–4; render them disabled with a tooltip for now is fine, or omit.
- `cs.status` line at the bottom (save/validation feedback later).

Panel layout details are yours (this is a visual tool; debug.rs's egui patterns are the house style) — the data contract above is what review checks.

- [ ] **Step 4: e2e extension**

Extend `tests/e2e_creator.rs`: mutate `CreatorState.working` directly (set the selected player's headwear to `Bare`), trigger the apply path, assert the preview rig's `RigCapMeshes` all go `Visibility::Hidden`→ wait, `Bare` hides the cap with no replacement — assert hidden. (Headless: the panel never runs; the apply system must be panel-independent — that's the design point this test pins.)

- [ ] **Step 5: Suite + checks + commit**

```bash
git add src/game/creator.rs src/game/appearance.rs tests/e2e_creator.rs
git commit -m "feat: creator panel — selector, tabs, live-editing working copy, revert"
```

---

### Task 3: Camera framing, preview clips, randomize, save

**Files:**
- Modify: `src/game/creator.rs`

**Interfaces:**
- Produces: per-tab camera targets + `preview_clip` tab-awareness; `randomize_player(def: &mut PlayerDef, seed: u32)` (pure, curated, unit-tested); `save_working(cs: &CreatorState) -> Result<(), String>` (validate → pretty-RON → write `concat!(env!("CARGO_MANIFEST_DIR"), "/data/players.ron")`).

- [ ] **Step 1: Camera + clips**

- Camera targets per tab: Identity → full body (`(0.0, 1.1, 3.2)` look-at `(0, 1.0, 0)`); Gear/Colors → head close-up (`(0.35, 1.55, 1.1)` look-at `(0, 1.5, 0)`); Animations → batter's-box-ish three-quarter (`(2.2, 1.4, 2.2)` look-at `(0, 1.0, 0)`). A system lerps the Creator camera toward the active tab's target (`transform.translation.lerp(target, 1 - (-8.0 * dt).exp())` style). Tune by eye.
- `preview_idle` becomes tab-aware: Identity/Gear/Colors → the player's resolved stance loop (`animation::stance_clip(...)` — livelier than Idle and shows the bat); Animations → the *selected* style element: stance loops; selecting a fidget or celebration plays it once then returns to the stance (`Playing::then`). Re-trigger on selection change.

- [ ] **Step 2: Randomize (curated, pure, TDD)**

```rust
/// Curated randomize: coherent combinations, not uniform RGB clown output.
/// Deterministic in the seed (ai::hash01 mixes) so tests can pin it.
pub fn randomize_player(def: &mut PlayerDef, seed: u32) { ... }
```

Curation rules (pin with unit tests): skin uniform over the six tones; headwear weighted (Cap 40%, Helmet 25%, CapBackwards 20%, Bare 15%); eyewear mostly Bare (60%); chain 25%; arms uniform; stance uniform over the four; fidget None 40% else uniform; celebration Standard 70%. Tests: same seed → same output; 100 seeds → every headwear variant appears; no field ever left un-set (struct fully written). Panel gets per-player "Randomize" (seed from a bumping counter) applying through the same working-copy path.

- [ ] **Step 3: Save (TDD)**

```rust
pub fn save_working(working: &RosterFile) -> Result<(), String> {
    RosterDefs::validate(working)?;
    let text = ron::ser::to_string_pretty(working, ron::ser::PrettyConfig::new())
        .map_err(|e| e.to_string())?;
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/data/players.ron"), text)
        .map_err(|e| e.to_string())
}
```

Unit tests (writing to a temp path — refactor the path into a parameter with the const as the caller's default): saved text re-parses to an equal `RosterFile`; an invalid working copy (bad name) is rejected and writes nothing. NOTE in a comment + your report: a save rewrites the file in pretty-RON formatting (one-time diff noise vs the hand-authored file — accepted; the hub owns the file's format from now on). Panel Save button calls it, routes `Err` into `cs.status`, and on success also refreshes `cs.snapshot` (saved state is the new revert point).

- [ ] **Step 4: e2e + suite + commit**

Extend the e2e: randomize determinism already unit-pinned; e2e just asserts the save-path validation rejects an invalid name end-to-end (set name "bad!", call the save fn, expect Err, file untouched). Full suite both feature sets, checks, fmt.

```bash
git add src/game/creator.rs
git commit -m "feat: creator camera framing, preview clips, curated randomize, validated save"
```

---

### Task 4: The portrait harness (the AI's eyes)

**Files:**
- Create: `src/game/portraits.rs` (cfg debug)
- Modify: `src/main.rs` (arg parsing → resource, native+debug only), `src/game/mod.rs` (register cfg'd)

**Interfaces:**
- Produces: `cargo run --features "dev debug" -- --portraits <dir>` boots windowed, auto-enters Creator, cycles EVERY player (both teams, lineup+bench, 26 total) × two framings (full-body, head close-up), captures `<dir>/<team>-<index>-<name>-<framing>.png` via Bevy 0.15's `Screenshot::primary_window()` + `save_to_disk`, then exits the app.

- [ ] **Step 1: Implement**

- `main.rs`: `#[cfg(all(feature = "debug", not(target_arch = "wasm32")))]` parse `std::env::args` for `--portraits <dir>`; insert `portraits::PortraitRun { dir, .. }` resource before `app.run()`.
- `portraits.rs`: a driver state machine resource `{ dir: PathBuf, queue: Vec<(Team, usize)>, framing: Framing, settle: Timer, phase: ... }`:
  1. On startup with the resource present: force-enter Creator (set next state) once the app reaches MainMenu.
  2. For each (team, index): set `CreatorState { team, index, tab: Identity }` (full-body) → wait ~0.6 s settle (wiring + dressing + camera lerp) → spawn `Screenshot::primary_window()` observed by `save_to_disk(path)` → switch tab to Gear (head framing) → settle → capture close-up → next player.
  3. Queue empty → `AppExit` event.
- Names in filenames come from the working copy (sanitize to A–Z already guaranteed).
- Keep it dumb and sequential; total runtime ~35 s for 52 captures is fine.

- [ ] **Step 2: Run it (required, honest)**

`cargo run --features "dev debug" -- --portraits /tmp/bb-portraits` (a window opens and flickers through players — expected). Verify: 52 PNGs exist, non-trivial file sizes. **Open at least three PNGs and LOOK at them** (you can read image files): confirm a helmet player reads as helmeted, a chain/eye-black player shows the gear, framings match their names. Include what you saw in the report — this run is also the input for Task 5's visual QA.

- [ ] **Step 3: Suite + checks + commit**

Portrait code is runtime-only (no headless e2e — it needs a real window/GPU; say so in a comment). Full suite both feature sets stays green; `cargo check` no-features + wasm clean (cfg discipline).

```bash
git add src/game/portraits.rs src/main.rs src/game/mod.rs
git commit -m "feat: portrait harness — every player to PNG for AI visual QA"
```

---

### Task 5: QA & hardening sweep (the parked items, in one pass)

**Files:** as each item requires; `tools/build_player.py` + regenerated model only if item 1 needs it.

Work through the parked list; each item gets either a fix or a one-line written ruling in the report (no silent drops):

1. **Helmet silhouette** (parked P2): using Task 4's portraits, judge the sphere-helmet. If it reads as "recolored head", improve minimally: prefer a procedural tweak in `gear.rs` (slightly larger radius + a short front brim cuboid, cap-material) over Blender work. Re-run portraits for the affected players; include before/after judgment.
2. **Cap-backwards + eyewear offsets** (parked P2): verify via portraits; nudge offsets if visibly wrong.
3. **Fidget leg-pose blend from non-standard stances** (parked P3): assess severity from the Animations-tab preview or portraits; fix only if trivially safe (per-stance fidget leg keys are NOT trivial — a ruling that it stays, with the crossfade rationale, is acceptable).
4. **IdentitySet ↔ PhaseSet unordered + no isolated stale-stance test** (parked P3): add the explicit ordering edge (batter chain already `.after` both? verify — if IdentitySet and PhaseSet are both upstream of the batter chain the hole is closed; the remaining question is sync_identities vs PhaseSet — add `.after(flow::PhaseSet)` to the identity chain OR write the isolated e2e that pins the self-heal; either closes the item).
5. **Fidget pause-vs-reset regression test** (parked P3): commit the missing test — e2e or unit — that fails if someone reintroduces reset-on-every-non-qualifying-frame (e.g. scenario-seam: accumulate across two short PrePitch stretches separated by a forced non-PrePitch interlude, assert the fidget still fires).
6. **Cross-game fidget `Local` staleness** (parked P3): reset path — simplest is clearing via an `OnTransition` game-start hook or keying the identity-compare to also reset when `GameState` re-enters Playing; one small change + a comment.
7. **jersey.rs `Query<&mut Visibility>` idiom for cap-hiding** (parked P2, cosmetic): apply if touching gear.rs anyway, else rule.

Full suite both feature sets + checks + fmt; commit (may be 2–3 commits by area).

```bash
git commit -m "fix: QA & hardening sweep — parked findings from phases 2–3"
```

---

## Phase-exit checklist

- [ ] Suite green (both feature sets) except the two known pre-existing failures; wasm + no-feature checks prove cfg discipline; clippy/fmt clean.
- [ ] Portraits run committed as evidence in reports (PNGs themselves NOT committed to git).
- [ ] Every parked item fixed or explicitly ruled in the Task 5 report.
- [ ] TODO.md re-checked.
- [ ] Spec's §5/§6 deliverables all present (walk the spec section against the landed code in the final review).
