//! End-to-end coverage for the contact stamp (Task B4): a scripted dead-on
//! press grades `ContactQuality::Perfect` (as pinned by `e2e_contact_timing`)
//! and the HUD must stamp `PERFECT!` over the zone-box area — then blank it
//! again once its ~0.8 s display timer runs out.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::ui::ContactStampText;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{headless_app, run_until, start_game, DriveGame};

const MAX_FRAMES: u64 = 15_000;

/// Pitches a straightaway changeup, then presses the batting side's action
/// button the instant the ball is dead-on the plate — the exact stage-0
/// script `e2e_contact_timing` uses to grade `ContactQuality::Perfect`.
fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
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
    let fielding = score.fielding_team();
    let batting = score.batting_team();

    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(fielding).action = true;
        }
        Phase::Pitch => {
            if let Ok(t) = ball.get_single() {
                let z = t.translation.z;
                if (-0.3..=0.3).contains(&z) {
                    intents.get_mut(batting).action = true;
                }
            }
        }
        _ => {}
    }
}

fn stamp_text(app: &mut App) -> String {
    app.world_mut()
        .query_filtered::<&Text, With<ContactStampText>>()
        .single(app.world())
        .0
        .clone()
}

#[test]
fn perfect_contact_stamps_and_then_blanks() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);

    // Starts blank (painted at spawn with an empty string — see ui.rs).
    assert_eq!(stamp_text(&mut app), "", "stamp must start blank");

    // Drive to the dead-on press: the ball goes into play, grading Perfect
    // (pinned by e2e_contact_timing's stage 0), and the HUD stamps it —
    // driven off the `ContactEvent` fired the same frame, so give the UI
    // system a frame or two to catch up rather than snapshotting instantly.
    let stamped = run_until(&mut app, MAX_FRAMES, |app| stamp_text(app) == "PERFECT!");
    assert!(
        stamped.is_some(),
        "a dead-on swing never stamped PERFECT! over the zone box (last seen: {:?})",
        stamp_text(&mut app)
    );

    // The stamp blanks again after its ~0.8 s display timer — well within
    // the live-play buffer that holds the next pitch off, so nothing here
    // needs a fresh swing to observe the fade.
    let blanked = run_until(&mut app, MAX_FRAMES, |app| stamp_text(app).is_empty());
    assert!(
        blanked.is_some(),
        "the PERFECT! stamp never blanked after its display timer"
    );
}
