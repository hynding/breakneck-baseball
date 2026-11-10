//! Unit tests for [`super`] — the running-game prompt's wording.

use super::*;

fn cues() -> HintCues {
    HintCues {
        phase: Phase::PrePitch,
        busy: false,
        runner: Some(0),
        defense_key: Some("E"),
        offense_key: None,
        called: false,
    }
}

#[test]
fn the_human_defense_is_shown_its_pickoff() {
    assert_eq!(hint_line(&cues()), "E  PICKOFF 1ST");
}

#[test]
fn the_human_offense_is_shown_its_steal_then_the_call() {
    let mut c = cues();
    c.defense_key = None;
    c.offense_key = Some("B");
    assert_eq!(hint_line(&c), "B  STEAL 2ND");
    c.called = true;
    assert_eq!(hint_line(&c), "STEAL ON!");
    // A steal can still be called in the windup; a pickoff can't be thrown.
    c.called = false;
    c.phase = Phase::WindUp;
    assert_eq!(hint_line(&c), "B  STEAL 2ND");
}

#[test]
fn nothing_to_say_without_a_runner_or_mid_throw() {
    let mut c = cues();
    c.runner = None;
    assert_eq!(hint_line(&c), "");
    let mut c = cues();
    c.busy = true;
    assert_eq!(hint_line(&c), "");
    let mut c = cues();
    c.phase = Phase::Pitch;
    assert_eq!(hint_line(&c), "");
}

#[test]
fn two_humans_both_get_their_button() {
    let mut c = cues();
    c.offense_key = Some("R-SHIFT");
    assert_eq!(hint_line(&c), "E  PICKOFF 1ST      R-SHIFT  STEAL 2ND");
}
