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

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{headless_app, run_until, start_game, tap_key, DriveGame};

/// Hard cap ≈ 7 sim-minutes; the scripted game needs ~10 pitches (~40 s) plus
/// the full walk-off trot (the play must end before the game can).
const MAX_FRAMES: u64 = 100_000;

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
    // Neutral by default; the phases below opt in.
    intents.home = default();
    intents.away = default();
    match play.phase {
        // The fielding side throws straightaway changeups: known called
        // strikes (unit-tested), so a take always advances the count
        // against the batter.
        Phase::PrePitch => {
            intents.get_mut(score.fielding_team()).action = true;
        }
        // Away never swings (strikes out); Home swings just before the
        // ideal contact point (contact_z ≈ 0.4) with full uppercut aim —
        // a deterministic home run.
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

#[test]
fn one_inning_game_plays_to_completion() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);

    // One press of I cycles the default 9 innings to 1, then 2 starts a
    // two-player game so the test scripts both teams.
    tap_key(&mut app, KeyCode::KeyI);
    start_game(&mut app, KeyCode::Digit2);

    let finished = run_until(&mut app, MAX_FRAMES, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::GameOver
    });

    let score = app.world().resource::<ScoreBoard>();
    let rules = app.world().resource::<Ruleset>();

    assert!(
        finished.is_some(),
        "game never finished (inning {} top={} {}-{} outs={} balls={} strikes={})",
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
