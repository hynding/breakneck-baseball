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
