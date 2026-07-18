# Regulation Rules, Innings Setting & E2E Test Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Classic Stadium variant match regulation baseball layout, add a menu setting for game length (1/3/6/9 innings), and validate with a headless end-to-end test that plays a full 1-inning game.

**Architecture:** All fixes flow through the existing data-driven seams: `FieldSpec`/`Ruleset` resources (variant.rs) for layout and thresholds, `GameConfig` (mod.rs) for menu-chosen options, pure functions in rules.rs for rule logic. The e2e test boots the real `GamePlugin` headless (no window, no GPU backend), steps virtual time at 240 Hz, and drives both teams through the documented `Intents` seam plus real keyboard-resource presses for the menu.

**Tech Stack:** Rust, Bevy 0.15, bevy_rapier3d, cargo integration tests.

## Global Constraints

- Prefix all cargo commands: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- After physics/rendering changes verify both targets: `cargo check` AND `cargo check --target wasm32-unknown-unknown`
- Bevy 0.15 idioms; home plate at world origin, +Z toward the field
- Don't hardcode baseball facts in systems — variants are data (variant.rs)
- Commit after each task; messages end with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Rules Audit (2026-07-18)

Verified correct already: 4 balls/3 strikes/3 outs, foul is a strike but never the third,
walks advance forced runners only, bases-loaded walk forces in one run, half-inning flip
clears bases, walk-off ends the game immediately, home team skips the bottom of the final
inning when leading, ties go to extra innings, pitch distance 18.44 m, fences 100/122 m
(≈330/400 ft), 90° fair wedge, strike zone ≈ regulation.

Defects: (1) base bags placed √2 too far (base paths 127 ft instead of 90 ft, bags off the
dirt); (2) game length hardcoded per variant, no player setting.

Backlog — since implemented (see `2026-07-18-advanced-baseball-rules.md`): tag-ups/
sacrifice flies, double plays, hit-by-pitch, caught foul pops, dropped third strike,
steals (pickoffs N/A — the analytic model has no leadoffs), batting lineups.

---

### Task 1: Regulation diamond geometry

**Files:**
- Modify: `src/game/variant.rs:129-160` (Standard base + infielder positions, tests)

**Interfaces:**
- Consumes: `field::{BASE_DISTANCE, HALF_DIAGONAL, PITCH_DISTANCE}` (HALF_DIAGONAL = 27.43·√2/2 ≈ 19.396)
- Produces: `VariantId::Standard.field()` with first base 27.43 m from home, second at (0, 0, 38.79)

- [ ] **Step 1: Update the variant test to regulation expectations (failing)**

In `standard_matches_regulation_baseball` (variant.rs tests) replace the second-base assertion with:

```rust
// First base is 90 ft (27.43 m) from home, and every base path is 90 ft.
assert!((f.base_positions[0].length() - BASE_DISTANCE).abs() < 0.01);
for pair in f.base_positions.windows(2) {
    assert!(((pair[1] - pair[0]).length() - BASE_DISTANCE).abs() < 0.01);
}
// Second base straight out along +Z at the full diamond diagonal (127 ft 3⅜ in).
assert!((f.base_positions[1] - Vec3::new(0.0, 0.0, 38.79)).length() < 0.01);
```

Add `BASE_DISTANCE` to the test imports if needed (`use crate::game::field::BASE_DISTANCE;`).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib variant` — expect `standard_matches_regulation_baseball` FAIL (first base currently 38.79 m from home).

- [ ] **Step 3: Fix the Standard field data**

In `VariantId::Standard.field()` use coordinate offsets of `HALF_DIAGONAL` (import it):

```rust
base_positions: vec![
    Vec3::new(HALF_DIAGONAL, 0.0, HALF_DIAGONAL),
    Vec3::new(0.0, 0.0, HALF_DIAGONAL * 2.0),
    Vec3::new(-HALF_DIAGONAL, 0.0, HALF_DIAGONAL),
],
```

and move the infielders onto the (now smaller) diamond:

```rust
fielder_positions: vec![
    Vec3::new(0.0, 0.0, -1.5), // catcher
    Vec3::new(HALF_DIAGONAL, 0.0, HALF_DIAGONAL - 3.0),
    Vec3::new(7.0, 0.0, HALF_DIAGONAL * 2.0 - 3.0),
    Vec3::new(-7.0, 0.0, HALF_DIAGONAL * 2.0 - 3.0),
    Vec3::new(-HALF_DIAGONAL, 0.0, HALF_DIAGONAL - 3.0),
    Vec3::new(-40.0, 0.0, 85.0), // left field
    Vec3::new(0.0, 0.0, 110.0),  // centre field
    Vec3::new(40.0, 0.0, 85.0),  // right field
],
```

- [ ] **Step 4: Run the full unit suite**

Run: `cargo test` — expect all PASS (hit-distance bands are absolute, unaffected).

- [ ] **Step 5: Commit**

```bash
git add src/game/variant.rs
git commit -m "fix: regulation 90 ft base paths — bags now sit on the dirt diamond"
```

---

### Task 2: Innings setting (1 / 3 / 6 / 9)

**Files:**
- Modify: `src/game/variant.rs` (add `INNINGS_OPTIONS`, `next_innings`, tests)
- Modify: `src/game/mod.rs:71-76` (`GameConfig` gains `innings`, manual `Default`)
- Modify: `src/game/menu.rs` (menu line, **I** key / gamepad East cycling, apply on start)

**Interfaces:**
- Produces: `variant::next_innings(u32) -> u32` cycling 1→3→6→9→1 (unknown → 1); `GameConfig.innings: u32`; `menu_select` writes `rules.innings = config.innings`.

- [ ] **Step 1: Write failing tests for the cycle helper (variant.rs tests)**

```rust
#[test]
fn innings_options_cycle_and_wrap() {
    assert_eq!(next_innings(1), 3);
    assert_eq!(next_innings(3), 6);
    assert_eq!(next_innings(6), 9);
    assert_eq!(next_innings(9), 1);
}

#[test]
fn unknown_innings_value_restarts_the_cycle() {
    assert_eq!(next_innings(2), 1);
}
```

- [ ] **Step 2: Run to verify compile failure** — `cargo test --lib variant` fails: `next_innings` not found.

- [ ] **Step 3: Implement in variant.rs (near `Ruleset`)**

```rust
/// Menu-selectable regulation game lengths.
pub const INNINGS_OPTIONS: [u32; 4] = [1, 3, 6, 9];

/// The next game-length option in the menu cycle (wraps; values not in the
/// list restart it).
pub fn next_innings(current: u32) -> u32 {
    match INNINGS_OPTIONS.iter().position(|&n| n == current) {
        Some(i) => INNINGS_OPTIONS[(i + 1) % INNINGS_OPTIONS.len()],
        None => INNINGS_OPTIONS[0],
    }
}
```

- [ ] **Step 4: `GameConfig` carries the choice (mod.rs)** — drop `Default` from the derive, add the field + manual impl:

```rust
#[derive(Resource, Debug)]
pub struct GameConfig {
    pub mode: GameMode,
    pub variant: VariantId,
    pub theme: ThemeId,
    /// Regulation innings for the next game; menu-cycled, seeded from the
    /// variant's default whenever the variant changes.
    pub innings: u32,
}

impl Default for GameConfig {
    fn default() -> Self {
        let variant = VariantId::default();
        Self {
            mode: GameMode::default(),
            innings: variant.rules().innings,
            variant,
            theme: ThemeId::default(),
        }
    }
}
```

Add a mod.rs test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_innings_follow_the_default_variant() {
        assert_eq!(GameConfig::default().innings, 9);
    }
}
```

- [ ] **Step 5: Menu wiring (menu.rs)**

In `cycle_options` add the key and behaviour (F resets innings to the new variant's default):

```rust
let innings_pressed = keyboard.just_pressed(KeyCode::KeyI)
    || pads.iter().any(|p| p.just_pressed(GamepadButton::East));

if !field_pressed && !theme_pressed && !innings_pressed {
    return;
}
if field_pressed {
    config.variant = config.variant.next();
    config.innings = config.variant.rules().innings;
}
if innings_pressed {
    config.innings = variant::next_innings(config.innings);
}
```

(import `crate::game::variant` accordingly). In `build_menu` the option loop becomes owned strings and gains the line:

```rust
for (label, value) in [
    ("F   Field", config.variant.label().to_string()),
    ("I   Innings", config.innings.to_string()),
    ("T   Theme", config.theme.label().to_string()),
] {
```

In `menu_select`, after `*rules = config.variant.rules();` add:

```rust
rules.innings = config.innings;
```

- [ ] **Step 6: Run tests** — `cargo test` all PASS.

- [ ] **Step 7: Commit**

```bash
git add src/game/variant.rs src/game/mod.rs src/game/menu.rs
git commit -m "feat: menu innings setting (1/3/6/9) applied to the ruleset at game start"
```

---

### Task 3: Short-game rule coverage

**Files:**
- Modify: `src/game/rules.rs` (tests only, in the `Game end` section)

- [ ] **Step 1: Add the tests**

```rust
#[test]
fn one_inning_walkoff_ends_immediately() {
    let score = ScoreBoard {
        home_runs: 1,
        away_runs: 0,
        inning: 1,
        top_of_inning: false,
        ..Default::default()
    };
    assert!(is_game_over(&score, 1));
}

#[test]
fn one_inning_tie_goes_to_extras() {
    // Still tied in the bottom of the 1st: play on.
    let bottom = ScoreBoard {
        inning: 1,
        top_of_inning: false,
        ..Default::default()
    };
    assert!(!is_game_over(&bottom, 1));
    // Tied after a full inning: extras.
    let extras = ScoreBoard {
        inning: 2,
        top_of_inning: true,
        ..Default::default()
    };
    assert!(!is_game_over(&extras, 1));
}

#[test]
fn home_lead_entering_bottom_of_final_skips_the_half() {
    // Home led 2-0 when the top of the 6th ended: the bottom is never played.
    let score = ScoreBoard {
        home_runs: 2,
        away_runs: 0,
        inning: 6,
        top_of_inning: false,
        ..Default::default()
    };
    assert!(is_game_over(&score, 6));
}
```

- [ ] **Step 2: Run** — `cargo test --lib rules` all PASS (these lock existing behaviour).

- [ ] **Step 3: Commit**

```bash
git add src/game/rules.rs
git commit -m "test: lock game-end rules for short (1-inning) games"
```

---

### Task 4: Headless e2e — a full 1-inning game

**Files:**
- Create: `src/lib.rs` (expose `pub mod game;` so integration tests can link)
- Modify: `src/main.rs` (use the lib crate)
- Create: `tests/e2e_full_game.rs`

**Interfaces:**
- Consumes: `GamePlugin`, `GameState`, `ScoreBoard`, `Team`, `flow::{Play, Phase}`, `input::Intents`, `ball::Baseball`, `variant::Ruleset`.
- Test driver runs in a custom schedule inserted after `PreUpdate` (after `gather_intents`, before the Update flow systems) so writing `Intents` is deterministic.

- [ ] **Step 1: Split out the lib target**

`src/lib.rs`:

```rust
//! Library target so integration tests (tests/) can build the real game app.
pub mod game;
```

`src/main.rs`: delete `mod game;` and import from the lib instead — `use breakneck_baseball::game::GamePlugin;` (keep everything else identical).

Run: `cargo check` — PASS.

- [ ] **Step 2: Write the e2e test**

`tests/e2e_full_game.rs` — boots the app headless, uses the menu (I → 1 inning, 2 → two players), then scripts: the fielding team always pitches straightaway (changeup down the middle), Away never swings (strikeouts), Home swings dead-red at the contact point (home runs). Expected: top 1 = three strikeouts, bottom 1 = walk-off homer, final 1-0 Home in 1 inning.

```rust
//! End-to-end: boots the real app headless and plays a complete 1-inning
//! game — menu key presses, real input/flow/physics/rules systems, virtual
//! time — through to the GAME OVER state.

use std::time::Duration;

use bevy::app::MainScheduleOrder;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::RenderPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy::winit::WinitPlugin;
use bevy_rapier3d::prelude::{NoUserData, RapierPhysicsPlugin};

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GamePlugin, GameState, ScoreBoard, Team};

/// Simulation step: 240 Hz keeps the swing-timing window (~0.12 m of ball
/// travel per frame) tight enough for a deterministic home-run swing.
const DT: f64 = 1.0 / 240.0;
/// Hard cap ≈ 5 sim-minutes; the scripted game needs ~10 pitches (~40 s).
const MAX_FRAMES: u64 = 72_000;

/// Runs after PreUpdate (so `gather_intents` has run) and before Update (so
/// the flow systems read what we wrote) — the same seam the CPU AI uses.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct DriveGame;

#[derive(Resource, Default)]
struct Driver {
    frame: u64,
}

fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(WgpuSettings {
                    backends: None,
                    ..default()
                }),
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins((
        RapierPhysicsPlugin::<NoUserData>::default(),
        GamePlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        DT,
    )))
    .init_resource::<Driver>();

    app.init_schedule(DriveGame);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_after(PreUpdate, DriveGame);
    app.add_systems(DriveGame, drive);
    app
}

fn drive(
    state: Res<State<GameState>>,
    mut driver: ResMut<Driver>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut intents: ResMut<Intents>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    ball: Query<&Transform, With<Baseball>>,
) {
    driver.frame += 1;
    match state.get() {
        GameState::MainMenu => match driver.frame {
            // One press of I cycles the default 9 innings to 1.
            10 => keyboard.press(KeyCode::KeyI),
            12 => keyboard.release(KeyCode::KeyI),
            // Start a two-player game so the test scripts both teams.
            30 => keyboard.press(KeyCode::Digit2),
            32 => keyboard.release(KeyCode::Digit2),
            _ => {}
        },
        GameState::Playing => {
            let (Some(play), Some(score)) = (play, score) else {
                return;
            };
            // Neutral by default; the phases below opt in.
            intents.home = default();
            intents.away = default();
            match play.phase {
                // The fielding side throws straightaway changeups: known
                // strikes (unit-tested), so a take is always a called strike.
                Phase::PrePitch => {
                    intents.get_mut(score.fielding_team()).action = true;
                }
                // Away never swings (strikes out); Home swings dead-red just
                // before the ideal contact point (contact_z ≈ 0.4) with full
                // uppercut aim — a deterministic home run.
                Phase::Pitch => {
                    if score.batting_team() == Team::Home {
                        if let Ok(t) = ball.get_single() {
                            intents.home.aim = Vec2::new(0.0, 1.0);
                            if t.translation.z <= 0.45 && t.translation.z >= 0.0 {
                                intents.home.action = true;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

#[test]
fn one_inning_game_plays_to_completion() {
    let mut app = headless_app();

    let mut frames: u64 = 0;
    while frames < MAX_FRAMES {
        app.update();
        frames += 1;
        if *app.world().resource::<State<GameState>>().get() == GameState::GameOver {
            break;
        }
    }

    let state = app.world().resource::<State<GameState>>().get().clone();
    let score = app.world().resource::<ScoreBoard>();
    let rules = app.world().resource::<Ruleset>();

    assert_eq!(state, GameState::GameOver, "game never finished ({frames} frames)");
    assert_eq!(rules.innings, 1, "menu innings setting was not applied");
    // Scripted game: Away takes three strikeouts, Home walks it off in the
    // bottom of the 1st. The walk-off must end the game inside inning 1.
    assert_eq!(score.inning, 1, "a 1-inning game must end in inning 1");
    assert!(!score.top_of_inning, "the game must end in the bottom half");
    assert_eq!(score.away_runs, 0, "Away never swings and cannot score");
    assert!(
        score.home_runs > 0,
        "Home's walk-off run must have scored (home {} - away {})",
        score.home_runs, score.away_runs
    );
}
```

- [ ] **Step 3: Run it** — `cargo test --test e2e_full_game -- --nocapture`
Expected: PASS in well under the frame cap. If the headless render setup fails on this
machine, the fallback is replacing the `RenderPlugin`/`WinitPlugin` config with
`DefaultPlugins.build().disable::<WinitPlugin>()` plus `bevy::app::ScheduleRunnerPlugin` —
diagnose from the panic before changing strategy.

- [ ] **Step 4: Full suite** — `cargo test` all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/main.rs tests/e2e_full_game.rs
git commit -m "test: headless e2e playing a complete 1-inning game to GAME OVER"
```

---

### Task 5: Dual-target verification & docs

**Files:**
- Modify: `CLAUDE.md` (test-suite line)

- [ ] **Step 1: Verify both targets**

```bash
cargo check
cargo check --target wasm32-unknown-unknown
cargo clippy --all-targets -- -D warnings
cargo test
```

All must pass (wasm check ensures the lib split didn't break the web build).

- [ ] **Step 2: Update CLAUDE.md** — replace "There is no test suite yet." with:

```
Tests: `cargo test` runs the rules/variant/input unit tests plus a headless e2e
(`tests/e2e_full_game.rs`) that plays a scripted 1-inning game through the real
app. The menu cycles innings (1/3/6/9) with **I**.
```

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: test suite and innings setting in CLAUDE.md"
```
