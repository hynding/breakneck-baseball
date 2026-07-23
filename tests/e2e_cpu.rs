//! End-to-end: the CPU offense (including its steal calls) plays a complete
//! half-inning against a scripted human pitcher without stalling the game.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{headless_app, run_until, start_game, DriveGame};

/// ≈ 10 sim-minutes — dozens of pitches, far beyond a normal half-inning even
/// with steal windows whenever the CPU puts runners aboard.
const MAX_FRAMES: u64 = 150_000;

/// Pitches straightaway changeups whenever the human (Home) is in the field.
/// The CPU bats the top half entirely on its own — swing decisions, steal
/// calls, everything.
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
    // Only the human's intent is scripted; the CPU systems run after this
    // schedule and write the Away side themselves.
    intents.home = default();
    if play.phase == Phase::PrePitch && score.fielding_team() == Team::Home {
        intents.home.action = true;
    }
}

#[test]
fn cpu_offense_completes_a_half_inning() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit1);

    let flipped = run_until(&mut app, MAX_FRAMES, |app| {
        let s = app.world().resource::<ScoreBoard>();
        !s.top_of_inning
    });

    let s = app.world().resource::<ScoreBoard>();
    assert!(
        flipped.is_some(),
        "the CPU never finished the top half (outs={} balls={} strikes={})",
        s.outs,
        s.balls,
        s.strikes
    );
    assert_eq!(s.inning, 1);
    assert_eq!(s.outs, 0, "outs reset entering the bottom half");
}
