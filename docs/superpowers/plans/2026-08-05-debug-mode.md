# Debug Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `debug`-feature-gated egui panel (tabbed: Tune / Scenario / State / Gizmos / Time) with live `Ruleset` tuning + paste-ready diff export, a shared scenario library the e2e tests also consume, live state readouts, 3D gizmo overlays, and time controls — per `docs/superpowers/specs/2026-08-05-debug-mode-design.md`.

**Architecture:** Debug mode is a privileged reader/writer of resources that already exist. `Ruleset` is restructured into `counts`/`batting`/`pace` sub-structs (the inspector renders each as a collapsible section); gameplay-feel constants are promoted into `pace` with defaults equal to today's consts; `scenario.rs` (always compiled) applies game situations by mutating the authoritative resources, and existing change-detection (`sync_runners`, HUD, jerseys) re-mirrors everything automatically.

**Tech Stack:** Rust, Bevy 0.15, bevy_rapier3d 0.28, `bevy-inspector-egui` 0.28.x (sole new dependency, optional, re-exports `bevy_egui` + `egui`).

## Global Constraints

- Every cargo command needs: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"` (CLAUDE.md).
- `cargo test` (lib + e2e + balance sim) must be green at the end of every task. Promoted defaults must equal today's consts — `tests/balance_sim.rs` bands must not move.
- Dual target: after physics/render-adjacent changes run `cargo check` **and** `cargo check --target wasm32-unknown-unknown` (with `--features debug` from Task 3 on).
- The `debug` feature must never be enabled in the Pages release build (`.github/workflows/pages.yml` is untouched).
- Home plate at origin, +Z toward the field; first base at −X.
- Commit after every task with the `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` trailer.

## File Structure

- `src/game/variant.rs` — `Ruleset` restructure (`CountRules`, `BattingTuning`, `PaceTuning`), `diff_literal`.
- `src/game/scenario.rs` — **new, always compiled**: `Scenario`, `presets()`, `apply_to_world`, `PitchOverride`, `ScenarioAppliedEvent`.
- `src/game/debug.rs` — **new, `#[cfg(feature = "debug")]`**: `DebugPlugin`, `DebugState`, panel tabs, gizmo systems, `ForcedContact`.
- `src/game/juice.rs` — `BaseSpeed` resource; restores compose with it.
- `src/game/flow.rs`, `rules.rs`, `fielding.rs`, `runner.rs`, `ai.rs` — read promoted `pace` values; small public seams (`Play::reset_for_scenario`, `BattingOrder::set_current`, `PitchKind::canonical_aim`).
- `tests/e2e_scenarios.rs` — **new** preset-driven e2e.

---

### Task 1: Restructure `Ruleset` into `counts` / `batting` sub-structs

**Files:**
- Modify: `src/game/variant.rs` (struct defs ~lines 16–66, both literals in `rules()` ~lines 173–211, unit tests)
- Modify: every reader — the compiler lists them; expect `flow.rs`, `batting.rs`, `rules.rs`, `ai.rs`, `tests/`

**Interfaces:**
- Produces: `Ruleset { pub counts: CountRules, pub batting: BattingTuning }`; `CountRules { balls_per_walk, strikes_per_out, outs_per_half, innings, peg_outs, steal_window_secs }`; `BattingTuning { perfect_ms, solid_ms, foul_ms, exit_weak, exit_solid, exit_perfect, pull_yaw_per_ms, cpu_timing_spread_ms, pci_radius_m }`. Field types unchanged. Later tasks reference `ruleset.counts.*` / `ruleset.batting.*`.

- [ ] **Step 1: Restructure the structs in `variant.rs`** — keep every existing doc comment on its field:

```rust
/// Countable-rule knobs read by the rules engine and game flow, grouped so
/// the debug inspector renders each group as its own collapsible section.
#[derive(Resource, Clone, Debug)]
pub struct Ruleset {
    pub counts: CountRules,
    pub batting: BattingTuning,
}

/// Count thresholds and window rules.
#[derive(Clone, Debug)]
pub struct CountRules {
    pub balls_per_walk: u32,
    pub strikes_per_out: u32,
    pub outs_per_half: u32,
    pub innings: u32,
    pub peg_outs: bool,
    pub steal_window_secs: f32,
}

/// Batting-feel timing/contact tuning (see the batting-feel spec §2).
#[derive(Clone, Debug)]
pub struct BattingTuning {
    pub perfect_ms: f32,
    pub solid_ms: f32,
    pub foul_ms: f32,
    pub exit_weak: f32,
    pub exit_solid: f32,
    pub exit_perfect: f32,
    pub pull_yaw_per_ms: f32,
    pub cpu_timing_spread_ms: f32,
    pub pci_radius_m: f32,
}
```

Update both variant literals in `VariantId::rules()` to the nested form (`counts: CountRules { balls_per_walk: 4, ... }, batting: BattingTuning { perfect_ms: 40.0, ... }`), values unchanged.

- [ ] **Step 2: Compiler-driven sweep.** Run `cargo check 2>&1 | head -60`; fix each error by inserting `.counts.` / `.batting.` on the access path (e.g. `rules.perfect_ms` → `rules.batting.perfect_ms`, `ruleset.innings` → `ruleset.counts.innings`). Caution: `GameConfig.innings` (`mod.rs`) also matches a bare `.innings` grep — only touch `Ruleset` accesses, which the compiler alone identifies. Repeat until clean.

- [ ] **Step 3: Full test run.** Run `cargo test`. Expected: all lib + e2e + balance tests pass with zero behavioral change (pure field relocation).

- [ ] **Step 4: Wasm check.** Run `cargo check --target wasm32-unknown-unknown`. Expected: clean.

- [ ] **Step 5: Commit** — `refactor: group Ruleset into counts/batting sub-structs`.

---

### Task 2: Promote pace constants into `Ruleset.pace`

**Files:**
- Modify: `src/game/variant.rs` (add `PaceTuning` + `pace:` in both literals + defaults test)
- Modify: `src/game/rules.rs` (thread `pace: &PaceTuning` into the race/throw functions; consts remain as `PaceTuning::default()` sources)
- Modify: `src/game/flow.rs` (`RESULT_SECS`, `PICKOFF_COOLDOWN_SECS` reads), `src/game/fielding.rs` (`AUTO_THROW_DELAY`, `MoveIntent.speed` from `fielder_speed`, throw call sites), `src/game/runner.rs` (`RUNNER_SPEED` reads), `src/game/ball.rs`/`flow.rs` pitch-launch site (`pitch_speed_scale`)

**Interfaces:**
- Produces: `Ruleset.pace: PaceTuning` with fields and defaults:

```rust
/// Speeds, delays, and race clocks — the game's pace. Defaults are the
/// long-standing module constants; `tests/balance_sim.rs` arbitrates changes.
#[derive(Clone, Debug)]
pub struct PaceTuning {
    /// Scales every `PitchKind::speed()` at release (1.0 = the kind table).
    /// This is how the spec's `PITCH_SPEED` promotion lands: one dial for
    /// all five pitches instead of a fastball-only field.
    pub pitch_speed_scale: f32,      // 1.0
    pub runner_speed: f32,           // 7.5  (rules::RUNNER_SPEED)
    pub fielder_speed: f32,          // 7.0  (rules::FIELDER_SPEED)
    pub reaction_secs: f32,          // 0.35 (rules::REACTION)
    pub throw_speed: f32,            // 27.0 (rules::THROW_FLIGHT_SPEED)
    pub throw_transfer_secs: f32,    // 0.5  (rules::THROW_TRANSFER)
    pub relay_transfer_secs: f32,    // 0.3  (rules::RELAY_TRANSFER)
    pub hit_and_run_jump_secs: f32,  // 1.6  (rules::HIT_AND_RUN_JUMP)
    pub stretch_grace_secs: f32,     // 0.9  (rules::STRETCH_GRACE)
    pub runner_margin_secs: f32,     // 0.35 (rules::RUNNER_MARGIN)
    pub result_secs: f32,            // 1.2  (flow::RESULT_SECS)
    pub pickoff_cooldown_secs: f32,  // 0.9  (flow::PICKOFF_COOLDOWN_SECS)
    pub auto_throw_delay_secs: f32,  // 0.6  (fielding::AUTO_THROW_DELAY)
}

impl Default for PaceTuning { fn default() -> Self { /* the values above */ } }
```

- Rules functions that internally used the promoted consts gain a trailing `pace: &PaceTuning` parameter. Both variants' literals set `pace: PaceTuning::default()`.

- [ ] **Step 1: Write the failing defaults test** in `variant.rs` tests:

```rust
#[test]
fn pace_defaults_match_legacy_constants() {
    let p = PaceTuning::default();
    assert_eq!(p.pitch_speed_scale, 1.0);
    assert_eq!(p.runner_speed, 7.5);
    assert_eq!(p.fielder_speed, 7.0);
    assert_eq!(p.reaction_secs, 0.35);
    assert_eq!(p.throw_speed, 27.0);
    assert_eq!(p.throw_transfer_secs, 0.5);
    assert_eq!(p.relay_transfer_secs, 0.3);
    assert_eq!(p.hit_and_run_jump_secs, 1.6);
    assert_eq!(p.stretch_grace_secs, 0.9);
    assert_eq!(p.runner_margin_secs, 0.35);
    assert_eq!(p.result_secs, 1.2);
    assert_eq!(p.pickoff_cooldown_secs, 0.9);
    assert_eq!(p.auto_throw_delay_secs, 0.6);
}
```

- [ ] **Step 2: Run it to fail** — `cargo test pace_defaults` → FAIL (`PaceTuning` undefined).

- [ ] **Step 3: Add `PaceTuning` + `pace` field**, wire literals, then thread the parameter. In `rules.rs`, find internal users: `grep -n "RUNNER_SPEED\|FIELDER_SPEED\|REACTION\|THROW_FLIGHT_SPEED\|THROW_TRANSFER\|RELAY_TRANSFER\|HIT_AND_RUN_JUMP\|STRETCH_GRACE\|RUNNER_MARGIN" src/game/rules.rs`. For each *function* using one (expect `catch_time`, `resolve_catch`, `resolve_thrown`, `throw_target`, and neighbors the grep reveals), add a trailing `pace: &PaceTuning` param and replace the const read (`RUNNER_SPEED` → `pace.runner_speed`, etc.). Callers in `flow.rs`/`fielding.rs` pass `&ruleset.pace`; `rules.rs` unit tests pass `&PaceTuning::default()`. Module consts stay (they feed `Default`); mark them `pub(crate)` if visibility complains.
  - `flow.rs`: replace `RESULT_SECS`/`PICKOFF_COOLDOWN_SECS` reads at timer-arm sites with `ruleset.pace.result_secs`/`.pickoff_cooldown_secs` (`Play::default()` keeps the consts as bootstrap values — it has no resource access; every in-game arm site has `Res<Ruleset>`).
  - `fielding.rs`: `AUTO_THROW_DELAY` → `ruleset.pace.auto_throw_delay_secs`; where fielder `MoveIntent.speed` is set from `FIELDER_SPEED`, use `ruleset.pace.fielder_speed`; runner speed sites in `runner.rs` likewise.
  - Pitch launch: at the site that reads `PitchKind::speed()` to set the pitch velocity (in `flow.rs`/`ball.rs` — grep `.speed()`), multiply by `ruleset.pace.pitch_speed_scale`.

- [ ] **Step 4: Run the full suite** — `cargo test`. Expected: all pass, `pace_defaults_match_legacy_constants` included; balance sim unchanged (defaults are value-identical).

- [ ] **Step 5: Wasm check** — `cargo check --target wasm32-unknown-unknown`. Expected: clean.

- [ ] **Step 6: Commit** — `feat: promote pace/race constants into Ruleset.pace variant data`.

---

### Task 3: `debug` cargo feature + `DebugPlugin` skeleton (F1, tab bar)

**Files:**
- Modify: `Cargo.toml` (optional dep + feature), `src/game/mod.rs` (cfg module + plugin), `CLAUDE.md` (commands), `.github/workflows/ci.yml` (feature checks)
- Create: `src/game/debug.rs`

**Interfaces:**
- Produces: `debug::DebugPlugin`; `debug::DebugState { open: bool, tab: DebugTab, gizmos: GizmoToggles, last_error: Option<&'static str> }`; `enum DebugTab { Tune, Scenario, State, Gizmos, Time }`; `struct GizmoToggles { zone, trajectory, intercept, pci, runner_targets, colliders: bool }`. Later tasks add per-tab bodies to `debug_panel`.

- [ ] **Step 1: Cargo.toml** — under `[dependencies]` add `bevy-inspector-egui = { version = "0.28", optional = true }`; under `[features]` add `debug = ["dep:bevy-inspector-egui"]`. Run `cargo check --features debug` once; if 0.28 fails resolution against Bevy 0.15, pick the adjacent minor whose changelog states Bevy 0.15 support (do not upgrade Bevy).

- [ ] **Step 2: Create `src/game/debug.rs`:**

```rust
//! Debug mode (`--features debug`): a tabbed egui panel plus gizmo overlays.
//! A privileged reader/writer of existing resources — it must never own
//! gameplay state beyond its own toggles.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_inspector_egui::bevy_egui::{EguiContext, EguiPlugin};
use bevy_inspector_egui::egui;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugTab {
    #[default]
    Tune,
    Scenario,
    State,
    Gizmos,
    Time,
}

#[derive(Clone, Copy, Default)]
pub struct GizmoToggles {
    pub zone: bool,
    pub trajectory: bool,
    pub intercept: bool,
    pub pci: bool,
    pub runner_targets: bool,
    pub colliders: bool,
}

#[derive(Resource, Default)]
pub struct DebugState {
    pub open: bool,
    pub tab: DebugTab,
    pub gizmos: GizmoToggles,
    pub last_error: Option<&'static str>,
}

pub struct DebugPlugin;

impl Plugin for DebugPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebugState>()
            .add_plugins(EguiPlugin)
            .add_systems(Update, toggle_panel)
            .add_systems(Update, debug_panel.run_if(panel_open));
    }
}

fn panel_open(state: Res<DebugState>) -> bool {
    state.open
}

/// F1 opens/closes; number keys 1–5 switch tabs while the panel is open.
fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut state: ResMut<DebugState>) {
    if keys.just_pressed(KeyCode::F1) {
        state.open = !state.open;
    }
    if !state.open {
        return;
    }
    for (key, tab) in [
        (KeyCode::Digit1, DebugTab::Tune),
        (KeyCode::Digit2, DebugTab::Scenario),
        (KeyCode::Digit3, DebugTab::State),
        (KeyCode::Digit4, DebugTab::Gizmos),
        (KeyCode::Digit5, DebugTab::Time),
    ] {
        if keys.just_pressed(key) {
            state.tab = tab;
        }
    }
}

/// Exclusive: the inspector widgets need `&mut World` alongside the egui ctx.
fn debug_panel(world: &mut World) {
    let Ok(ctx) = world
        .query_filtered::<&mut EguiContext, With<PrimaryWindow>>()
        .get_single_mut(world)
        .map(|c| c.get_mut().clone())
    else {
        return;
    };
    egui::Window::new("Debug").default_width(340.0).show(&ctx, |ui| {
        let mut tab = world.resource::<DebugState>().tab;
        ui.horizontal(|ui| {
            for (label, t) in [
                ("Tune", DebugTab::Tune),
                ("Scenario", DebugTab::Scenario),
                ("State", DebugTab::State),
                ("Gizmos", DebugTab::Gizmos),
                ("Time", DebugTab::Time),
            ] {
                ui.selectable_value(&mut tab, t, label);
            }
        });
        world.resource_mut::<DebugState>().tab = tab;
        ui.separator();
        match tab {
            DebugTab::Tune => ui.label("Tune — Task 4"),
            DebugTab::Scenario => ui.label("Scenario — Task 6"),
            DebugTab::State => ui.label("State — Task 8"),
            DebugTab::Gizmos => ui.label("Gizmos — Task 9"),
            DebugTab::Time => ui.label("Time — Task 10"),
        };
    });
}
```

(The placeholder labels are the tab bodies later tasks replace — they ship compiling, not TBD.)

- [ ] **Step 3: Register in `mod.rs`** — with the other `pub mod` lines add `#[cfg(feature = "debug")] pub mod debug;`; in `GamePlugin::build`, after the second `add_plugins` tuple:

```rust
#[cfg(feature = "debug")]
app.add_plugins(debug::DebugPlugin);
```

- [ ] **Step 4: Verify all four build shapes.** `cargo check`, `cargo check --features debug`, `cargo check --target wasm32-unknown-unknown`, `cargo check --target wasm32-unknown-unknown --features debug`. Expected: all clean. Then `cargo test` (feature off — unchanged).

- [ ] **Step 5: CI + docs.** In `.github/workflows/ci.yml`, next to the existing check/test steps add two steps: `cargo check --features debug` and `cargo check --target wasm32-unknown-unknown --features debug` (mirror the existing steps' toolchain/cache setup). In `CLAUDE.md`'s Commands block add: `cargo run --features "dev debug"   # + F1 in-game debug panel`.

- [ ] **Step 6: Commit** — `feat: debug cargo feature with tabbed egui panel skeleton (F1)`.

---

### Task 4: Reflect derives, Tune tab, `diff_literal` export

**Files:**
- Modify: `src/game/variant.rs` (derives, registration-friendly; `diff_literal` + tests), `src/game/mod.rs` (register types), `src/game/debug.rs` (Tune tab body)

**Interfaces:**
- Consumes: Task 1/2 structs.
- Produces: `Ruleset::diff_literal(&self, variant: VariantId) -> String` — empty when nothing differs; else lines like `batting.perfect_ms: 48.0,` under a `// VariantId::Standard overrides:` header.

- [ ] **Step 1: Failing tests** in `variant.rs`:

```rust
#[test]
fn diff_literal_is_empty_at_defaults() {
    assert_eq!(VariantId::Standard.rules().diff_literal(VariantId::Standard), "");
}

#[test]
fn diff_literal_lists_only_changed_fields() {
    let mut r = VariantId::Standard.rules();
    r.batting.perfect_ms = 48.0;
    r.pace.runner_speed = 8.0;
    let s = r.diff_literal(VariantId::Standard);
    assert!(s.contains("batting.perfect_ms: 48.0,"));
    assert!(s.contains("pace.runner_speed: 8.0,"));
    assert!(!s.contains("solid_ms"));
    assert!(s.starts_with("// VariantId::Standard overrides:"));
}
```

- [ ] **Step 2: Run to fail** — `cargo test diff_literal` → FAIL (method missing).

- [ ] **Step 3: Implement** in `variant.rs`:

```rust
impl Ruleset {
    /// Paste-ready Rust lines for every field differing from `variant`'s
    /// defaults — the debug panel's tuning-session export.
    pub fn diff_literal(&self, variant: VariantId) -> String {
        let d = variant.rules();
        let mut out = String::new();
        macro_rules! diff {
            ($($path:ident).+) => {
                if self.$($path).+ != d.$($path).+ {
                    out.push_str(&format!(
                        concat!(stringify!($($path).+), ": {:?},\n"),
                        self.$($path).+
                    ));
                }
            };
        }
        diff!(counts.balls_per_walk);
        diff!(counts.strikes_per_out);
        diff!(counts.outs_per_half);
        diff!(counts.innings);
        diff!(counts.peg_outs);
        diff!(counts.steal_window_secs);
        diff!(batting.perfect_ms);
        diff!(batting.solid_ms);
        diff!(batting.foul_ms);
        diff!(batting.exit_weak);
        diff!(batting.exit_solid);
        diff!(batting.exit_perfect);
        diff!(batting.pull_yaw_per_ms);
        diff!(batting.cpu_timing_spread_ms);
        diff!(batting.pci_radius_m);
        diff!(pace.pitch_speed_scale);
        diff!(pace.runner_speed);
        diff!(pace.fielder_speed);
        diff!(pace.reaction_secs);
        diff!(pace.throw_speed);
        diff!(pace.throw_transfer_secs);
        diff!(pace.relay_transfer_secs);
        diff!(pace.hit_and_run_jump_secs);
        diff!(pace.stretch_grace_secs);
        diff!(pace.runner_margin_secs);
        diff!(pace.result_secs);
        diff!(pace.pickoff_cooldown_secs);
        diff!(pace.auto_throw_delay_secs);
        if out.is_empty() {
            out
        } else {
            format!("// {:?} overrides:\n{}", variant, out)
        }
    }
}
```

(`{:?}` on `VariantId` prints `Standard`; adjust the header to `// VariantId::Standard overrides:` by formatting `"// VariantId::{:?} overrides:\n{}"`.)

- [ ] **Step 4: Reflect derives + registration.** Add `#[derive(Reflect)]` to `Ruleset`, `CountRules`, `BattingTuning`, `PaceTuning`, `FieldSpec`, `Scenery` (plain `bevy::reflect` — no feature gate). In `GamePlugin::build`: `app.register_type::<variant::Ruleset>().register_type::<variant::FieldSpec>();`.

- [ ] **Step 5: Tune tab body** in `debug.rs`, replacing the Task 3 label:

```rust
DebugTab::Tune => {
    bevy_inspector_egui::bevy_inspector::ui_for_resource::<
        crate::game::variant::Ruleset,
    >(world, ui);
    egui::CollapsingHeader::new("Field & Camera").show(ui, |ui| {
        bevy_inspector_egui::bevy_inspector::ui_for_resource::<
            crate::game::variant::FieldSpec,
        >(world, ui);
    });
    if ui.button("Dump diff → stdout + clipboard").clicked() {
        let variant = world.resource::<crate::game::GameConfig>().variant;
        let text = world
            .resource::<crate::game::variant::Ruleset>()
            .diff_literal(variant);
        println!("{text}");
        ui.ctx().copy_text(text);
    }
}
```

(`ui_for_resource` renders each sub-struct as its own collapsible section — the categorization from the spec, for free. If `Context::copy_text` doesn't exist in the resolved egui version, use `ui.output_mut(|o| o.copied_text = text)`.)

- [ ] **Step 6: Tests + manual smoke.** `cargo test diff_literal` → PASS; `cargo test` → all green; `cargo check --features debug` both targets. Manual: `cargo run --features "dev debug"`, F1, drag `batting.perfect_ms`, click Dump diff, confirm the stdout line.

- [ ] **Step 7: Commit** — `feat: reflection-driven Tune tab with paste-ready Ruleset diff export`.

---

### Task 5: Scenario library (`scenario.rs`, presets, apply, pitch override)

**Files:**
- Create: `src/game/scenario.rs`
- Modify: `src/game/mod.rs` (module + `init_resource::<PitchOverride>()` + `add_event::<ScenarioAppliedEvent>()`), `src/game/flow.rs` (`Play::scenario_safe`, `Play::reset_for_scenario`), `src/game/rules.rs` (`BattingOrder::set_current`, `PitchKind::canonical_aim`), `src/game/ai.rs` (`cpu_defense` consumes the override)

**Interfaces:**
- Consumes: `Play` internals (via new methods), `Bases::{reset_for,set}`, `ScoreBoard` pub fields, `steal_window_for` (private fn `flow.rs:283` — used inside `reset_for_scenario`).
- Produces:
  - `scenario::Scenario { pub name: &'static str, pub bases: Vec<bool>, pub outs: u32, pub balls: u32, pub strikes: u32, pub inning: u32, pub top: bool, pub score: (u32, u32), pub batter_slot: Option<u32>, pub next_cpu_pitch: Option<PitchKind> }` (`Clone, Debug`)
  - `scenario::presets() -> Vec<Scenario>` (six named presets below)
  - `scenario::apply_to_world(world: &mut World, s: &Scenario) -> Result<(), &'static str>`
  - `scenario::PitchOverride(pub Option<PitchKind>)` resource; `scenario::ScenarioAppliedEvent { pub name: &'static str }` event
  - `Play::scenario_safe(&self) -> bool`; `Play::reset_for_scenario(&mut self, bases: &Bases, rules: &Ruleset)`
  - `BattingOrder::set_current(&mut self, team: Team, slot: u32)` (1-based, wraps into `LINEUP_SIZE`)
  - `PitchKind::canonical_aim(self) -> Vec2` (round-trips through `from_aim`)

- [ ] **Step 1: Failing unit tests** — create `scenario.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_aim_round_trips_every_pitch() {
        use crate::game::rules::PitchKind::*;
        for kind in [Fastball, Curveball, Changeup, Slider, Sinker] {
            assert_eq!(PitchKind::from_aim(kind.canonical_aim()), kind);
        }
    }

    #[test]
    fn presets_are_legal_for_standard_rules() {
        for s in presets() {
            assert!(s.balls < 4 && s.strikes < 3 && s.outs < 3, "{}", s.name);
            assert!(s.bases.len() <= 4, "{}", s.name);
            assert!(s.inning >= 1, "{}", s.name);
        }
    }

    #[test]
    fn apply_rewrites_the_world_and_fires_the_event() {
        let mut world = test_world(); // helper below
        let s = presets().into_iter().find(|s| s.name == PRESET_LOADED).unwrap();
        apply_to_world(&mut world, &s).unwrap();
        let score = world.resource::<ScoreBoard>();
        assert_eq!((score.balls, score.strikes, score.outs), (3, 2, 2));
        let bases = world.resource::<Bases>();
        assert!(bases.is_occupied(0) && bases.is_occupied(1) && bases.is_occupied(2));
        assert!(!world.resource::<Events<ScenarioAppliedEvent>>().is_empty());
    }

    #[test]
    fn apply_is_refused_while_the_ball_is_live() {
        let mut world = test_world();
        world.resource_mut::<Play>().force_phase_for_test(Phase::InPlay);
        let s = &presets()[0];
        assert!(apply_to_world(&mut world, s).is_err());
    }

    fn test_world() -> World {
        let mut world = World::new();
        world.insert_resource(ScoreBoard { inning: 1, top_of_inning: true, ..Default::default() });
        world.insert_resource(Bases::default());
        world.insert_resource(BattingOrder::default());
        world.insert_resource(Play::default());
        world.insert_resource(VariantId::Standard.rules());
        world.insert_resource(VariantId::Standard.field());
        world.init_resource::<PitchOverride>();
        world.init_resource::<Events<ScenarioAppliedEvent>>();
        world
    }
}
```

`Play.phase` is a pub field, but the test needs to *set* it — add alongside the existing `#[cfg(test)] test_play` in `flow.rs`: `pub fn force_phase_for_test(&mut self, phase: Phase) { self.phase = phase; }` gated `#[cfg(test)]` won't cross crates — since these tests are in the same lib crate, `#[cfg(test)]` works. Use it.

- [ ] **Step 2: Run to fail** — `cargo test scenario` → FAIL (module skeleton missing). Add `pub mod scenario;` to `mod.rs` first so failures are about items, not the module.

- [ ] **Step 3: Implement `scenario.rs`:**

```rust
//! Scenario library — instantly reachable game situations, shared verbatim
//! by the in-game debug panel (`debug.rs`) and the headless e2e tests. A
//! scenario only writes the authoritative resources; runner rigs, HUD, and
//! jerseys all re-mirror through their existing change detection.

use bevy::prelude::*;

use crate::game::flow::Play;
use crate::game::rules::{Bases, BattingOrder, PitchKind};
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::ScoreBoard;

pub const PRESET_LOADED: &str = "Bases loaded, 2 out, full count";

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub bases: Vec<bool>,
    pub outs: u32,
    pub balls: u32,
    pub strikes: u32,
    pub inning: u32,
    pub top: bool,
    pub score: (u32, u32),
    pub batter_slot: Option<u32>,
    pub next_cpu_pitch: Option<PitchKind>,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            name: "Custom",
            bases: vec![false; 3],
            outs: 0,
            balls: 0,
            strikes: 0,
            inning: 1,
            top: true,
            score: (0, 0),
            batter_slot: None,
            next_cpu_pitch: None,
        }
    }
}

/// Forces the CPU's next pitch selection (consumed on use). Scenario data,
/// so it lives here in the lib target; human pitchers are never overridden.
#[derive(Resource, Default)]
pub struct PitchOverride(pub Option<PitchKind>);

#[derive(Event)]
pub struct ScenarioAppliedEvent {
    pub name: &'static str,
}

pub fn presets() -> Vec<Scenario> {
    vec![
        Scenario { name: PRESET_LOADED, bases: vec![true, true, true], outs: 2, balls: 3, strikes: 2, ..Default::default() },
        Scenario { name: "DP setup: R1, 0 out", bases: vec![true, false, false], ..Default::default() },
        Scenario { name: "Steal duel: R1", bases: vec![true, false, false], outs: 1, ..Default::default() },
        Scenario { name: "Tag-up: R3, 1 out", bases: vec![false, false, true], outs: 1, ..Default::default() },
        Scenario { name: "Dropped-third: 2 strikes", strikes: 2, ..Default::default() },
        Scenario { name: "Walk-off: bottom 9, down 1, R2", bases: vec![false, true, false], inning: 9, top: false, score: (3, 4), outs: 2, ..Default::default() },
    ]
}

/// Rewrites the live game to `s`. Refused (Err) while the ball is live —
/// the same deadness gate pausing uses.
pub fn apply_to_world(world: &mut World, s: &Scenario) -> Result<(), &'static str> {
    if !world.resource::<Play>().scenario_safe() {
        return Err("scenario refused: ball is live");
    }
    let base_count = world.resource::<FieldSpec>().base_count();
    {
        let mut score = world.resource_mut::<ScoreBoard>();
        score.home_runs = s.score.0;
        score.away_runs = s.score.1;
        score.inning = s.inning;
        score.top_of_inning = s.top;
        score.balls = s.balls;
        score.strikes = s.strikes;
        score.outs = s.outs;
    }
    {
        let mut bases = world.resource_mut::<Bases>();
        bases.reset_for(base_count);
        for (i, &occ) in s.bases.iter().enumerate() {
            bases.set(i, occ);
        }
    }
    if let Some(slot) = s.batter_slot {
        let team = world.resource::<ScoreBoard>().batting_team();
        world.resource_mut::<BattingOrder>().set_current(team, slot);
    }
    world.resource_scope(|world, mut play: Mut<Play>| {
        play.reset_for_scenario(world.resource::<Bases>(), world.resource::<Ruleset>());
    });
    world.resource_mut::<PitchOverride>().0 = s.next_cpu_pitch;
    world.send_event(ScenarioAppliedEvent { name: s.name });
    Ok(())
}
```

- [ ] **Step 4: The seams.** `flow.rs` (near `test_play`):

```rust
    /// The ball is dead: a scenario may safely rewrite the game state.
    pub fn scenario_safe(&self) -> bool {
        matches!(self.phase, Phase::PrePitch | Phase::Result)
    }

    /// Resets to a fresh at-bat over the given base state — the scenario
    /// library's seam ([`crate::game::scenario::apply_to_world`]).
    pub fn reset_for_scenario(&mut self, bases: &Bases, rules: &Ruleset) {
        *self = Play::default();
        self.hold = steal_window_for(bases, rules);
    }
```

`rules.rs` — on `BattingOrder`:

```rust
    /// Debug/scenario seam: force `team`'s current (1-based) lineup slot.
    pub fn set_current(&mut self, team: Team, slot: u32) {
        let v = slot.saturating_sub(1) % LINEUP_SIZE;
        match team {
            Team::Home => self.home = v,
            Team::Away => self.away = v,
        }
    }
```

`rules.rs` — on `PitchKind` (values chosen to decode through `from_aim`'s 0.35 thresholds):

```rust
    /// The aim whose [`PitchKind::from_aim`] decode is exactly this pitch —
    /// the scenario library's forced-pitch seam.
    pub fn canonical_aim(self) -> Vec2 {
        match self {
            PitchKind::Fastball => Vec2::new(0.0, 0.6),
            PitchKind::Curveball => Vec2::new(0.0, -0.6),
            PitchKind::Slider => Vec2::new(-0.6, 0.0),
            PitchKind::Sinker => Vec2::new(0.6, 0.0),
            PitchKind::Changeup => Vec2::ZERO,
        }
    }
```

`mod.rs`: `pub mod scenario;` plus in `build`: `.init_resource::<scenario::PitchOverride>()` and `.add_event::<scenario::ScenarioAppliedEvent>()`.

`ai.rs` — `cpu_defense` gains `mut pitch_override: ResMut<PitchOverride>` (import from `scenario`); at the point where the pitch aim is computed (inside the `pitch_delay.finished()` branch, before `PitchKind::from_aim` semantics apply), prepend:

```rust
        let aim = if let Some(kind) = pitch_override.0.take() {
            kind.canonical_aim()
        } else {
            /* existing aim computation */
        };
```

- [ ] **Step 5: Run** — `cargo test scenario` → PASS; then `cargo test` full. Expected: green (override resource defaults to `None`, so CPU behavior is unchanged).

- [ ] **Step 6: Commit** — `feat: shared scenario library with named presets and CPU pitch override`.

---

### Task 6: Scenario tab + forced-contact override

**Files:**
- Modify: `src/game/debug.rs` (Scenario tab body, `ForcedContact`), `src/game/flow.rs` (cfg-gated override at the swing grade site, `flow.rs:618-622`)

**Interfaces:**
- Consumes: `scenario::{presets, apply_to_world, Scenario}`, `ContactQuality`.
- Produces: `debug::ForcedContact(pub Option<ContactQuality>)` resource (registered in `DebugPlugin` via `init_resource`).

- [ ] **Step 1: `ForcedContact` + Scenario tab** in `debug.rs`. Add `custom: crate::game::scenario::Scenario` field to `DebugState` (default via `Scenario::default()` — derive `Default` by hand since `DebugState` derives it: implement `Default` for `DebugState` manually now). Tab body replacing the Task 3 label:

```rust
DebugTab::Scenario => {
    for s in crate::game::scenario::presets() {
        if ui.button(s.name).clicked() {
            let r = crate::game::scenario::apply_to_world(world, &s);
            world.resource_mut::<DebugState>().last_error = r.err();
        }
    }
    ui.separator();
    ui.label("Custom");
    let base_count = world.resource::<crate::game::variant::FieldSpec>().base_count();
    let mut state = world.resource_mut::<DebugState>();
    state.custom.bases.resize(base_count, false);
    let mut custom = state.custom.clone();
    ui.horizontal(|ui| {
        for (i, occ) in custom.bases.iter_mut().enumerate() {
            ui.checkbox(occ, format!("{}B", i + 1));
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(&mut custom.balls).range(0..=3).prefix("B "));
        ui.add(egui::DragValue::new(&mut custom.strikes).range(0..=2).prefix("S "));
        ui.add(egui::DragValue::new(&mut custom.outs).range(0..=2).prefix("O "));
        ui.add(egui::DragValue::new(&mut custom.inning).range(1..=99).prefix("Inn "));
        ui.checkbox(&mut custom.top, "top");
    });
    egui::ComboBox::from_label("next CPU pitch")
        .selected_text(format!("{:?}", custom.next_cpu_pitch))
        .show_ui(ui, |ui| {
            use crate::game::rules::PitchKind::*;
            ui.selectable_value(&mut custom.next_cpu_pitch, None, "None");
            for k in [Fastball, Curveball, Changeup, Slider, Sinker] {
                ui.selectable_value(&mut custom.next_cpu_pitch, Some(k), format!("{k:?}"));
            }
        });
    world.resource_mut::<DebugState>().custom = custom.clone();
    if ui.button("Apply custom").clicked() {
        let r = crate::game::scenario::apply_to_world(world, &custom);
        world.resource_mut::<DebugState>().last_error = r.err();
    }
    ui.separator();
    let mut forced = world.resource::<ForcedContact>().0;
    egui::ComboBox::from_label("force contact")
        .selected_text(format!("{forced:?}"))
        .show_ui(ui, |ui| {
            use crate::game::rules::ContactQuality::*;
            ui.selectable_value(&mut forced, None, "Off");
            for q in [Whiff, FoulTip, Weak, Solid, Perfect] {
                ui.selectable_value(&mut forced, Some(q), format!("{q:?}"));
            }
        });
    world.resource_mut::<ForcedContact>().0 = forced;
    if let Some(err) = world.resource::<DebugState>().last_error {
        ui.colored_label(egui::Color32::YELLOW, err);
    }
}
```

With:

```rust
/// Pins every judged swing's grade — deterministic swing-outcome testing.
/// Debug-only: read by `flow`'s swing site through a cfg-gated param.
#[derive(Resource, Default, Clone, Copy)]
pub struct ForcedContact(pub Option<crate::game::rules::ContactQuality>);
```

and `.init_resource::<ForcedContact>()` in `DebugPlugin::build`.

- [ ] **Step 2: Flow override.** In the system containing `flow.rs:618-622`, add a cfg-gated param and apply after grading:

```rust
    #[cfg(feature = "debug")] forced: Res<crate::game::debug::ForcedContact>,
```

```rust
        #[allow(unused_mut)]
        let mut quality = /* existing pci/classic grade expression */;
        #[cfg(feature = "debug")]
        if let Some(f) = forced.0 {
            quality = f;
        }
```

- [ ] **Step 3: Verify.** `cargo test` (feature off — untouched), `cargo check --features debug` both targets. Manual smoke: run with `--features "dev debug"`, apply "Bases loaded, 2 out, full count" → HUD shows 3-2 with 2 outs, three runner rigs walk to their bags; force Perfect → every swing booms.

- [ ] **Step 4: Commit** — `feat: scenario tab with presets, custom builder, forced contact`.

---

### Task 7: Preset-driven e2e (`tests/e2e_scenarios.rs`)

**Files:**
- Create: `tests/e2e_scenarios.rs`

**Interfaces:**
- Consumes: `common::{headless_app, start_game, run_until, DriveGame}` (harness), `scenario::{presets, apply_to_world, PRESET_LOADED}`, `runner::Runner` component, `Play::{scenario_safe, in_steal_window}`.

- [ ] **Step 1: Write the test file:**

```rust
//! Scenario presets applied to the live headless game: the jump-cut template
//! for rule regressions — no inning-scripting to reach a situation.

mod common;

use bevy::prelude::*;
use breakneck_baseball::game::flow::Play;
use breakneck_baseball::game::runner::Runner;
use breakneck_baseball::game::scenario::{apply_to_world, presets, PRESET_LOADED};
use breakneck_baseball::game::ScoreBoard;
use common::{headless_app, run_until, start_game};

#[test]
fn bases_loaded_preset_manifests_runners_and_count() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets().into_iter().find(|s| s.name == PRESET_LOADED).unwrap();
    apply_to_world(app.world_mut(), &s).expect("ball is dead at PrePitch");

    let score = app.world().resource::<ScoreBoard>();
    assert_eq!((score.balls, score.strikes, score.outs), (3, 2, 2));

    // The runner mirror walks rigs onto every occupied bag.
    let settled = run_until(&mut app, 5_000, |app| {
        let mut q = app.world_mut().query::<&Runner>();
        q.iter(app.world()).count() == 3
    });
    assert!(settled.is_some(), "three runner rigs must appear for bases loaded");
}

#[test]
fn steal_preset_opens_the_window() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    let s = presets().into_iter().find(|s| s.name == "Steal duel: R1").unwrap();
    apply_to_world(app.world_mut(), &s).unwrap();
    app.update();
    assert!(
        app.world().resource::<Play>().in_steal_window(),
        "a runner on first must reopen the steal window"
    );
}
```

If `run_until`'s exact signature differs (see `tests/common/mod.rs`), match it — it exists (used by `start_game`).

- [ ] **Step 2: Run** — `cargo test --test e2e_scenarios`. Expected: both pass. If `Runner`/`flow` items aren't `pub` from the lib, make the minimal visibility change (`pub` on the component/module already are — verify).

- [ ] **Step 3: Full suite + commit** — `cargo test`; commit `test: e2e scenario presets drive the live game`.

---

### Task 8: State tab

**Files:**
- Modify: `src/game/flow.rs` (read accessors), `src/game/debug.rs` (tab body + `FrameTimeDiagnosticsPlugin`)

**Interfaces:**
- Produces on `Play`: `pub fn pending_call(&self) -> Option<rules::Outcome>` (returns the copy), `pub fn steal_window_remaining(&self) -> f32` (`self.hold.remaining_secs()`).

- [ ] **Step 1: Accessors** in `flow.rs` (one-liners beside the existing getters). **Step 2: Tab body** replacing the Task 3 label:

```rust
DebugTab::State => {
    let play = world.resource::<crate::game::flow::Play>();
    ui.monospace(format!("phase: {:?}", play.phase));
    ui.monospace(format!("pending_call: {:?}", play.pending_call()));
    ui.monospace(format!("last swing: {:?}", play.last_contact_quality()));
    ui.monospace(format!(
        "steal window: {:.2}s (lead extended: {})",
        play.steal_window_remaining(),
        world.resource::<crate::game::flow::LeadState>().extended
    ));
    ui.monospace(format!(
        "runners settled: {}",
        world.resource::<crate::game::runner::RunnersSettled>().0
    ));
    let mut q = world.query_filtered::<(&Transform, &bevy_rapier3d::prelude::Velocity), With<crate::game::ball::Baseball>>();
    if let Ok((tf, vel)) = q.get_single(world) {
        ui.monospace(format!(
            "ball: h {:.1} m, v {:.1} m/s",
            tf.translation.y,
            vel.linvel.length()
        ));
    }
    if let Some(fps) = world
        .resource::<bevy::diagnostic::DiagnosticsStore>()
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
    {
        ui.monospace(format!("fps: {fps:.0}"));
    }
}
```

Add `app.add_plugins(bevy::diagnostic::FrameTimeDiagnosticsPlugin);` in `DebugPlugin::build`.

- [ ] **Step 3: Verify + commit** — `cargo check --features debug` (both targets), `cargo test`; commit `feat: debug state tab (play machine, ball, fps readouts)`.

---

### Task 9: Gizmos tab + overlays

**Files:**
- Modify: `src/game/debug.rs` (toggle checkboxes + gizmo systems + Rapier debug-render toggle)

**Interfaces:**
- Consumes: `rules::{ZONE_LOW, ZONE_HIGH, ZONE_HALF_WIDTH, predict_landing_from}`, `ball::{Baseball, InFlight, BALL_DRAG_FACTOR, MAGNUS_FACTOR}`, `animation::MoveIntent`, `runner::Runner`, `batting::PciState`, `field::PciCursorMarker`, `variant::FieldSpec`, `bevy_rapier3d::render::{RapierDebugRenderPlugin, DebugRenderContext}`.

- [ ] **Step 1: Tab body** — a checkbox per `GizmoToggles` field; the `colliders` checkbox writes `world.resource_mut::<DebugRenderContext>().enabled`. In `DebugPlugin::build` add `RapierDebugRenderPlugin::default().disabled()` (or set `DebugRenderContext { enabled: false, .. }` after adding).

- [ ] **Step 2: Gizmo systems** (all `.run_if(in_state(GameState::Playing))`, each early-returning on its toggle), added in `DebugPlugin::build`:

```rust
fn zone_gizmo(state: Res<DebugState>, mut gizmos: Gizmos) {
    if !state.gizmos.zone { return; }
    use crate::game::rules::{ZONE_HALF_WIDTH, ZONE_HIGH, ZONE_LOW};
    let center = Vec3::new(0.0, (ZONE_LOW + ZONE_HIGH) / 2.0, 0.0);
    gizmos.rect(
        Isometry3d::new(center, Quat::IDENTITY),
        Vec2::new(ZONE_HALF_WIDTH * 2.0, ZONE_HIGH - ZONE_LOW),
        bevy::color::palettes::css::CYAN,
    );
}

fn trajectory_gizmo(
    state: Res<DebugState>,
    ball: Query<(&Transform, &bevy_rapier3d::prelude::Velocity), With<crate::game::ball::InFlight>>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.trajectory { return; }
    let Ok((tf, vel)) = ball.get_single() else { return; };
    use crate::game::ball::{BALL_DRAG_FACTOR, MAGNUS_FACTOR};
    let (landing, _hang) = crate::game::rules::predict_landing_from(
        tf.translation, vel.linvel, vel.angvel, BALL_DRAG_FACTOR, MAGNUS_FACTOR,
    );
    // A sightline + landing circle reads the play; exact touchdown already
    // lives in fx.rs's landing ring.
    gizmos.line(tf.translation, landing + Vec3::Y * 0.02, bevy::color::palettes::css::ORANGE);
    gizmos.circle(Isometry3d::new(landing + Vec3::Y * 0.02, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), 0.5, bevy::color::palettes::css::ORANGE);
}

fn intercept_gizmo(
    state: Res<DebugState>,
    fielders: Query<(&Transform, &crate::game::animation::MoveIntent)>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.intercept { return; }
    for (tf, intent) in &fielders {
        if let Some(target) = intent.target {
            gizmos.line(tf.translation + Vec3::Y * 0.1, target + Vec3::Y * 0.1, bevy::color::palettes::css::YELLOW);
        }
    }
}

fn throw_target_gizmo(
    state: Res<DebugState>,
    play: Res<crate::game::flow::Play>,
    bases: Res<crate::game::rules::Bases>,
    ruleset: Res<crate::game::variant::Ruleset>,
    field: Res<crate::game::variant::FieldSpec>,
    time: Res<Time>,
    ball: Query<&Transform, With<crate::game::ball::Baseball>>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.intercept || play.phase != crate::game::flow::Phase::InPlay {
        return;
    }
    let Ok(tf) = ball.get_single() else { return; };
    let race = play.since_contact(time.elapsed_secs());
    // Same call fielding.rs makes at the throw (fielding.rs:352) — after
    // Task 2 it also takes `&ruleset.pace`; match the signature the compiler
    // shows. The result indexes `base_positions`; out-of-range means home.
    let target = crate::game::rules::throw_target(
        tf.translation, race, &bases, play.runners_going(), &field, &ruleset.pace,
    );
    let pos = field.base_positions.get(target).copied().unwrap_or(Vec3::ZERO);
    gizmos.circle(
        Isometry3d::new(pos + Vec3::Y * 0.05, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        1.0,
        bevy::color::palettes::css::YELLOW,
    );
}

fn runner_target_gizmo(
    state: Res<DebugState>,
    field: Res<crate::game::variant::FieldSpec>,
    runners: Query<(&Transform, &crate::game::runner::Runner)>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.runner_targets { return; }
    for (tf, runner) in &runners {
        if let Some(bag) = field.base_positions.get(runner.base) {
            gizmos.line(tf.translation, *bag + Vec3::Y * 0.05, bevy::color::palettes::css::LIME);
        }
    }
}

fn pci_gizmo(
    state: Res<DebugState>,
    ruleset: Res<crate::game::variant::Ruleset>,
    cursor: Query<&Transform, With<crate::game::field::PciCursorMarker>>,
    mut gizmos: Gizmos,
) {
    if !state.gizmos.pci { return; }
    let Ok(tf) = cursor.get_single() else { return; };
    gizmos.circle(
        Isometry3d::new(tf.translation, Quat::IDENTITY),
        ruleset.batting.pci_radius_m,
        bevy::color::palettes::css::MAGENTA,
    );
}
```

(Exact `gizmos.rect`/`circle` signatures per Bevy 0.15 — `Isometry3d` argument; adjust if the compiler asks for `(&mut self, position, rotation, ...)` form. `BALL_DRAG_FACTOR`/`MAGNUS_FACTOR` are `pub` in `ball.rs:24-27`; `MoveIntent` is `animation.rs:135` with `target: Option<Vec3>`.)

- [ ] **Step 3: Verify + commit** — `cargo check --features debug` both targets, `cargo test`; manual: toggle each overlay in play. Commit `feat: debug gizmo overlays (zone, trajectory, intercepts, pci, colliders)`.

---

### Task 10: Time controls + `BaseSpeed` composition in juice

**Files:**
- Modify: `src/game/juice.rs` (`BaseSpeed`, compose all speed writes), `src/game/debug.rs` (Time tab, step system)

**Interfaces:**
- Produces: `juice::BaseSpeed(pub f32)` resource (default `1.0`), always compiled. Every `set_relative_speed(X)` in `juice.rs` becomes `set_relative_speed(X * base.0)` (restores use `base.0`).

- [ ] **Step 1: Failing test** in `juice.rs` tests:

```rust
#[test]
fn watchdog_restores_to_base_speed_not_one() {
    let mut app = test_app();
    app.insert_resource(BaseSpeed(0.5));
    send_perfect(&mut app);
    for _ in 0..250 {
        app.update();
    }
    assert_eq!(speed(&app), 0.5, "restore must return to the debug base speed");
}
```

- [ ] **Step 2: Run to fail** — `cargo test watchdog_restores_to_base` → FAIL (`BaseSpeed` undefined).

- [ ] **Step 3: Implement.** In `juice.rs`:

```rust
/// The "normal" speed every juice restore returns to — 1.0 in the shipping
/// game; the debug Time tab dials it for slow-mo/fast-forward sessions.
#[derive(Resource)]
pub struct BaseSpeed(pub f32);

impl Default for BaseSpeed {
    fn default() -> Self {
        BaseSpeed(1.0)
    }
}
```

`init_resource::<BaseSpeed>()` in `JuicePlugin::build`. Add `base: Res<BaseSpeed>` to `reset_juice`, `trigger_juice`, `tick_freeze`, `tick_slowmo`, `restore_on_result`, `tick_watchdog`, `force_restore`; multiply every literal: `FREEZE_SPEED * base.0`, `SLOWMO_SPEED * base.0`, and each `set_relative_speed(1.0)` → `set_relative_speed(base.0)`.

- [ ] **Step 4: Time tab** in `debug.rs`, replacing the Task 3 label. Add `step_pending: bool` to `DebugState` — Task 6 replaced its derived `Default` with a manual impl, so add the field there too (`step_pending: false`):

```rust
DebugTab::Time => {
    ui.horizontal(|ui| {
        for (label, s) in [("¼×", 0.25f32), ("½×", 0.5), ("1×", 1.0), ("2×", 2.0)] {
            if ui.button(label).clicked() {
                world.resource_mut::<crate::game::juice::BaseSpeed>().0 = s;
                world.resource_mut::<Time<Virtual>>().set_relative_speed(s);
            }
        }
    });
    let paused = world.resource::<Time<Virtual>>().is_paused();
    ui.horizontal(|ui| {
        if ui.button(if paused { "resume" } else { "pause" }).clicked() {
            let mut virt = world.resource_mut::<Time<Virtual>>();
            if paused { virt.unpause() } else { virt.pause() }
        }
        if ui.button("step").clicked() {
            world.resource_mut::<Time<Virtual>>().unpause();
            world.resource_mut::<DebugState>().step_pending = true;
        }
    });
}
```

Plus a `Last`-schedule system in `DebugPlugin`:

```rust
fn finish_step(mut state: ResMut<DebugState>, mut virt: ResMut<Time<Virtual>>) {
    if state.step_pending {
        virt.pause();
        state.step_pending = false;
    }
}
```

- [ ] **Step 5: Run** — `cargo test` (juice tests incl. the new one pass; harness unaffected — `BaseSpeed` defaults 1.0). `cargo check --features debug` both targets.

- [ ] **Step 6: Commit** — `feat: debug time controls composing with juice via BaseSpeed`.

---

## Final verification (after Task 10)

- [ ] `cargo test` — full suite green.
- [ ] `cargo check` / `cargo check --features debug` / both wasm variants — clean.
- [ ] `cargo clippy --features debug -- -D warnings` — clean (match repo habit).
- [ ] Manual playthrough with `--features "dev debug"`: F1 → each tab, dump a diff, apply each preset, toggle each gizmo, run at ½× and single-step.
- [ ] TODO.md/TADA.md per user's queue convention if the item is listed there.
