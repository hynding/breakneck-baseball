//! End-to-end: pausing between plays opens the substitution board, a bench
//! swap rewrites the roster, and resuming leaves the scene intact — the
//! Playing ⇄ Paused transitions must neither tear down nor respawn the world.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::roster::Rosters;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{headless_app, run_until, start_game, tap_key, DriveGame};

const MAX_FRAMES: u64 = 20_000;

/// In play, the fielding side throws straightaway changeups and nobody
/// swings — enough traffic to prove the game runs.
fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    if play.phase == Phase::PrePitch {
        intents.get_mut(score.fielding_team()).action = true;
    }
}

fn state(app: &App) -> GameState {
    app.world().resource::<State<GameState>>().get().clone()
}

#[test]
fn pause_swaps_the_bench_and_resumes_cleanly() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);

    // Wait for a dead ball (waiting on a pitch), then pause.
    let ready = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<Play>().phase == Phase::PrePitch
    });
    assert!(ready.is_some(), "never reached a PrePitch dead ball");
    tap_key(&mut app, KeyCode::Escape);
    assert_eq!(state(&app), GameState::Paused, "Esc between plays pauses");

    // The scene survives the pause: the ball entity is still there.
    let balls = |app: &mut App| {
        app.world_mut()
            .query_filtered::<(), With<Baseball>>()
            .iter(app.world())
            .count()
    };
    assert_eq!(balls(&mut app), 1, "pausing must not tear the scene down");

    // Swap the top of the order (cursor starts at slot 0 / bench 0) for the
    // batting team — Away, in the top of the 1st.
    let before = app.world().resource::<Rosters>().away.clone();
    tap_key(&mut app, KeyCode::Enter);
    let after = app.world().resource::<Rosters>().away.clone();
    assert_eq!(after.lineup[0], before.bench[0], "bench player subbed in");
    assert_eq!(
        after.bench[0], before.lineup[0],
        "starter took the bench seat"
    );

    // Resume: exactly one ball (no duplicate scene spawn), and the game keeps
    // playing — the scripted pitcher works the count against a taking batter.
    tap_key(&mut app, KeyCode::Escape);
    assert_eq!(state(&app), GameState::Playing, "Esc again resumes");
    assert_eq!(balls(&mut app), 1, "resuming must not respawn the scene");

    let progressed = run_until(&mut app, MAX_FRAMES, |app| {
        let s = app.world().resource::<ScoreBoard>();
        s.balls + s.strikes > 0 || s.outs > 0
    });
    assert!(
        progressed.is_some(),
        "the game must keep running after a pause/resume cycle"
    );
}
