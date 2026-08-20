//! End-to-end: the home-run moment (Task B8). Scripts the same deterministic
//! walk-off home run as `e2e_full_game` (Away takes three Ks in the top of the
//! 1st, Home swings a dead-red uppercut for a Perfect walk-off in the bottom),
//! but instead of just asserting the game reaches GAME OVER it watches the
//! moment play out on screen:
//!
//!   * the fireworks show spawns motes (`fx::FireworkSpark`),
//!   * the camera enters the home-run trot orbit (`Play::is_home_run()` while
//!     `Phase::Result`), and
//!   * the walk-off finishes on screen: the winning run sits on the board with
//!     the game *still* `Playing` (the trot running) before it ends — the B5
//!     review fix, so the slow-mo/juice and trot are no longer truncated by a
//!     GameOver fired at contact.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::fx::FireworkSpark;
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{DriveGame, headless_app, run_until, start_game, tap_key};

/// Hard cap: the scripted game needs ~10 pitches plus the full walk-off trot
/// (which must finish before the game can end, now that it's deferred).
const MAX_FRAMES: u64 = 100_000;

/// Pins the "classic" windows a dead-red uppercut grades a Perfect home run
/// against (the shipped Standard windows are the balance harness's to tune —
/// same decoupling as `e2e_full_game`).
fn pin_classic_contact_windows(app: &mut App) {
    let mut r = app.world_mut().resource_mut::<Ruleset>();
    r.batting.perfect_ms = 40.0;
    r.batting.solid_ms = 90.0;
    r.batting.foul_ms = 140.0;
    r.batting.exit_solid = 1.0;
    r.batting.exit_perfect = 1.25;
}

fn drive(
    state: Res<State<GameState>>,
    mut intents: ResMut<Intents>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    ball: Query<&Transform, With<Baseball>>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(score.fielding_team()).action = true;
        }
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

fn firework_motes(app: &mut App) -> usize {
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, With<FireworkSpark>>();
    q.iter(app.world()).count()
}

#[test]
fn walk_off_home_run_is_a_moment_that_finishes_on_screen() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);

    tap_key(&mut app, KeyCode::KeyI);
    start_game(&mut app, KeyCode::Digit2);
    pin_classic_contact_windows(&mut app);

    let mut saw_fireworks = false;
    let mut saw_trot_orbit = false;
    let mut winning_run_while_playing = false;

    let finished = run_until(&mut app, MAX_FRAMES, |app| {
        if firework_motes(app) > 0 {
            saw_fireworks = true;
        }
        if *app.world().resource::<State<GameState>>().get() == GameState::Playing {
            let play = app.world().resource::<Play>();
            let home_run_result = play.is_home_run() && play.phase == Phase::Result;
            let score = app.world().resource::<ScoreBoard>();
            let home_ahead_bottom = !score.top_of_inning && score.home_runs > score.away_runs;
            if home_run_result {
                saw_trot_orbit = true;
            }
            // The walk-off run is on the board but the game is still live — the
            // trot is finishing on screen instead of the game ending at contact.
            if home_ahead_bottom {
                winning_run_while_playing = true;
            }
        }
        *app.world().resource::<State<GameState>>().get() == GameState::GameOver
    });

    assert!(
        finished.is_some(),
        "the walk-off game never reached GAME OVER"
    );
    assert!(
        saw_fireworks,
        "the home-run fireworks never spawned any motes"
    );
    assert!(
        saw_trot_orbit,
        "the camera never entered the trot orbit (is_home_run() during Phase::Result)"
    );
    assert!(
        winning_run_while_playing,
        "the walk-off ended at contact instead of finishing the trot on screen \
         (winning run was never observed while still Playing)"
    );
}
