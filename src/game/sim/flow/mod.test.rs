//! Unit tests for [`super`] — the flow module.

use super::*;

/// A gloved pitch is remembered through the result pause (the camera
/// holds the duel framing on it — `camera::duel_framing_wanted`) and a
/// fresh `Play` starts unglooved.
#[test]
fn pitch_gloved_defaults_false_and_reads_back() {
    let play = Play::default();
    assert!(!play.pitch_gloved());
    let play = Play::test_play(Phase::Result, true);
    assert!(play.pitch_gloved());
}

/// A fresh play has no strike call on record — the observer seam only
/// ever reports a decision the umpire actually made this play.
#[test]
fn last_strike_call_defaults_none() {
    assert_eq!(Play::default().last_strike_call(), None);
}

/// A taken ball is the one beat that ends without the curtain; every other
/// result dips to black before the reset (TODO 101).
#[test]
fn only_a_taken_ball_skips_the_curtain() {
    assert!(!ResultBeat::Ball.dips());
    for beat in [
        ResultBeat::Strike,
        ResultBeat::Foul,
        ResultBeat::Strikeout,
        ResultBeat::Walk,
        ResultBeat::Pickoff,
        ResultBeat::InPlay,
    ] {
        assert!(beat.dips(), "{beat:?} should dip");
    }
}

/// Strike calls map onto their beats: the third strike is its own beat,
/// and a dropped third the batter runs out ends the plate appearance like
/// a walk does.
#[test]
fn strike_calls_land_in_their_beats() {
    assert_eq!(
        ResultBeat::for_strike(rules::StrikeCall::Strike),
        ResultBeat::Strike
    );
    assert_eq!(
        ResultBeat::for_strike(rules::StrikeCall::Strikeout),
        ResultBeat::Strikeout
    );
    assert_eq!(
        ResultBeat::for_strike(rules::StrikeCall::DroppedThird),
        ResultBeat::Walk
    );
}

/// The pause a read-out should live for is the hold plus the curtain for a
/// dipping beat and the bare hold for a ball; the curtain's progress is
/// exposed only while it is closing.
#[test]
fn result_pause_includes_the_curtain_only_when_the_beat_dips() {
    let strike = Play::test_result(ResultBeat::Strike, None);
    let ball = Play::test_result(ResultBeat::Ball, None);
    let strikeout = Play::test_result(ResultBeat::Strikeout, None);
    let hold = RESULT_SECS; // `test_result` keeps the default timer length
    assert!((strike.result_pause_secs() - (hold + CURTAIN_SECS)).abs() < 1e-5);
    assert!((ball.result_pause_secs() - hold).abs() < 1e-5);
    // A beat that ends the plate appearance closes with the slower wipe.
    assert!((strikeout.result_pause_secs() - (hold + WIPE_SECS)).abs() < 1e-5);
    assert!(strikeout.curtain_is_wipe() && !strike.curtain_is_wipe());
    assert_eq!(strike.curtain_progress(), None);
    let closing = Play::test_result(ResultBeat::Strike, Some(0.5));
    let progress = closing.curtain_progress().expect("closing");
    assert!((progress - 0.5).abs() < 1e-3, "got {progress}");
}

/// Only the beats that end a plate appearance bring the next batter's
/// walk-up; a pickoff, a foul, a ball, and a strike keep the box occupied.
#[test]
fn plate_appearance_ends_on_strikeout_walk_and_batted_ball() {
    for beat in [ResultBeat::Strikeout, ResultBeat::Walk, ResultBeat::InPlay] {
        assert!(beat.ends_plate_appearance(), "{beat:?}");
    }
    for beat in [
        ResultBeat::Ball,
        ResultBeat::Strike,
        ResultBeat::Foul,
        ResultBeat::Pickoff,
    ] {
        assert!(!beat.ends_plate_appearance(), "{beat:?}");
    }
    assert!(Play::test_walkup().walkup_active());
    assert!(!Play::default().walkup_active());
}
