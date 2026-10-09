//! End-to-end coverage for the swing-and-miss beat (TODO 101; the shape the
//! reference footage pins in docs/agent/SMB3-REFERENCE-NOTES.md §2.3): a
//! scripted early swing grades a `Whiff`, and that must
//!   * stamp the swing's timing (`EARLY`) — the miss is *explained*, not just
//!     announced,
//!   * show the pitch's speed by the plate for the beat,
//!   * hold for the strike beat's length, then close the curtain fully black
//!     on the frame the phase flips to `PrePitch` (the reset lands unseen),
//!   * and open the curtain again on its own once the next duel is up.

mod common;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{CURTAIN_SECS, Phase, Play, ResultBeat, bat_arrival_z};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::ui::{ContactStampText, CurtainRoot, PitchSpeedText};
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DT, DriveGame, headless_app, run_until, start_game};

const MAX_FRAMES: u64 = 15_000;

/// Pitches straight away, then presses the batting side's action button so
/// the bat comes through while the ball is still five metres out — far too
/// early for any contact window, so the swing grades a `Whiff` (an EARLY one).
fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
    ball: Query<(&Transform, &Velocity), With<Baseball>>,
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
            if let Ok((t, v)) = ball.single() {
                if (5.0..=6.0).contains(&bat_arrival_z(t.translation.z, v.linvel.z)) {
                    intents.get_mut(batting).action = true;
                }
            }
        }
        _ => {}
    }
}

fn text_of<M: Component>(app: &mut App) -> String {
    app.world_mut()
        .query_filtered::<&Text, With<M>>()
        .single(app.world())
        .unwrap()
        .0
        .clone()
}

fn curtain_alpha(app: &mut App) -> f32 {
    app.world_mut()
        .query_filtered::<&BackgroundColor, With<CurtainRoot>>()
        .single(app.world())
        .unwrap()
        .0
        .alpha()
}

fn phase(app: &App) -> Phase {
    app.world().resource::<Play>().phase
}

#[test]
fn a_whiff_is_explained_held_and_curtained() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);

    // The game fades in from black: the curtain root is painted opaque at
    // spawn (the wasm rule wants a real colour there) and opens on its own.
    let opened = run_until(&mut app, MAX_FRAMES, |app| curtain_alpha(app) < 0.05);
    assert!(
        opened.is_some(),
        "the curtain never opened after game start"
    );

    // Drive to the whiff's result pause.
    let judged = run_until(&mut app, MAX_FRAMES, |app| phase(app) == Phase::Result);
    assert!(
        judged.is_some(),
        "the early swing never reached a result pause"
    );
    let play = app.world().resource::<Play>();
    assert_eq!(
        play.result_beat(),
        Some(ResultBeat::Strike),
        "an early whiff on a fresh count is a strike beat"
    );
    let hold = app
        .world()
        .resource::<Ruleset>()
        .pace
        .result_secs_for(ResultBeat::Strike);

    // The miss is explained: timing under the call, speed by the plate.
    let stamped = run_until(&mut app, 30, |app| {
        text_of::<ContactStampText>(app) == "EARLY"
    });
    assert!(
        stamped.is_some(),
        "a whiff must stamp its timing (saw {:?})",
        text_of::<ContactStampText>(&mut app)
    );
    let speed = text_of::<PitchSpeedText>(&mut app);
    assert!(
        speed.ends_with(" MPH") && speed.trim_end_matches(" MPH").parse::<u32>().is_ok(),
        "the pitch speed read-out should show a whole-number MPH, saw {speed:?}"
    );

    // The pause holds for the strike beat plus the curtain, the curtain is
    // fully black on the frame the phase flips, and the read-outs are still
    // up until then (they vanish under the curtain, never before).
    let mut frames = 0u64;
    let mut peak_alpha = 0.0f32;
    let mut stamp_seen_at_flip = false;
    while phase(&app) == Phase::Result {
        frames += 1;
        assert!(frames < MAX_FRAMES, "stuck in the result pause");
        peak_alpha = peak_alpha.max(curtain_alpha(&mut app));
        stamp_seen_at_flip = !text_of::<ContactStampText>(&mut app).is_empty();
        app.update();
    }
    assert_eq!(phase(&app), Phase::PrePitch);
    let expected = ((hold + CURTAIN_SECS) as f64 / DT).round() as i64;
    assert!(
        (frames as i64 - expected).abs() <= 6,
        "result pause ran {frames} frames, expected ~{expected} (hold {hold} s + curtain {CURTAIN_SECS} s)"
    );
    assert!(
        peak_alpha >= 0.95,
        "the curtain should be fully black by the flip, peaked at {peak_alpha}"
    );
    assert!(
        stamp_seen_at_flip,
        "the timing stamp must stay up through the beat, not blank early"
    );

    // And it opens again on its own for the next duel.
    let reopened = run_until(&mut app, 600, |app| curtain_alpha(app) < 0.05);
    assert!(
        reopened.is_some(),
        "the curtain never reopened after the flip"
    );
    assert!(
        text_of::<ContactStampText>(&mut app).is_empty(),
        "the stamp must be gone once the beat is over"
    );
}
