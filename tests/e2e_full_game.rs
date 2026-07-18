//! End-to-end: boots the real app headless and plays a complete 1-inning
//! game — menu key presses, real input/flow/physics/rules systems, virtual
//! time — through to the GAME OVER state.
//!
//! Script: on the menu, **I** cycles the game length from the default 9 to 1
//! and **2** starts a two-player game (so the test owns both teams). In play,
//! the fielding side always pitches a straightaway changeup (a unit-tested
//! called strike), Away never swings (three strikeouts end the top), and Home
//! swings dead-red at the ideal contact point with full uppercut aim — a
//! deterministic home run and an immediate walk-off in the bottom of the 1st.

use std::time::Duration;

use bevy::app::{MainScheduleOrder, PluginsState};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
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

/// Runs after `PreUpdate` (so `gather_intents` has refreshed keyboard-driven
/// intents) and before `Update` (so the flow systems read what we wrote) —
/// the same [`Intents`] seam the CPU AI uses.
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
            // No window and no winit event loop; the GPU adapter still
            // initializes (surface-less) so the render app and its resources
            // exist, but nothing is ever presented.
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins((RapierPhysicsPlugin::<NoUserData>::default(), GamePlugin))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        DT,
    )))
    .init_resource::<Driver>();

    app.init_schedule(DriveGame);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_after(PreUpdate, DriveGame);
    app.add_systems(DriveGame, drive);

    // Driving `app.update()` by hand skips what `App::run` would do: wait out
    // async plugin setup (the wgpu adapter request), then run `finish` /
    // `cleanup`, which insert late resources like `CapturedScreenshots`.
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
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
                // called strikes (unit-tested), so a take always advances
                // the count against the batter.
                Phase::PrePitch => {
                    intents.get_mut(score.fielding_team()).action = true;
                }
                // Away never swings (strikes out); Home swings just before
                // the ideal contact point (contact_z ≈ 0.4) with full
                // uppercut aim — a deterministic home run.
                Phase::Pitch if score.batting_team() == Team::Home => {
                    if let Ok(t) = ball.get_single() {
                        intents.home.aim = Vec2::new(0.0, 1.0);
                        if t.translation.z <= 0.45 && t.translation.z >= 0.0 {
                            intents.home.action = true;
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

    assert_eq!(
        state,
        GameState::GameOver,
        "game never finished ({frames} frames; inning {} top={} {}-{} outs={} balls={} strikes={})",
        score.inning,
        score.top_of_inning,
        score.away_runs,
        score.home_runs,
        score.outs,
        score.balls,
        score.strikes
    );
    assert_eq!(rules.innings, 1, "menu innings setting was not applied");
    // Scripted game: Away takes three strikeouts, Home walks it off in the
    // bottom of the 1st. The walk-off must end the game inside inning 1.
    assert_eq!(score.inning, 1, "a 1-inning game must end in inning 1");
    assert!(!score.top_of_inning, "the game must end in the bottom half");
    assert_eq!(score.away_runs, 0, "Away never swings and cannot score");
    assert!(
        score.home_runs > 0,
        "Home's walk-off run must have scored (home {} - away {})",
        score.home_runs,
        score.away_runs
    );
}
