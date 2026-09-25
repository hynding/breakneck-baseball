//! Unit tests for [`super`] — the steal module.

use super::super::test_support::*;
use super::*;

// ── Steals ────────────────────────────────────────────────────────────────

#[test]
fn steal_succeeds_against_offspeed() {
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_steal(&mut score, &mut bases, &std_rules(), true, false),
        StealResult::Stolen { base: 1 }
    );
    assert_eq!(bases, with(&[1]));
    assert_eq!(score.outs, 0);
}

#[test]
fn steal_is_caught_against_a_fastball() {
    let mut score = ScoreBoard {
        balls: 2,
        strikes: 1,
        ..Default::default()
    };
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_steal(&mut score, &mut bases, &std_rules(), false, false),
        StealResult::Caught
    );
    assert_eq!(bases, empty());
    assert_eq!(score.outs, 1);
    // The at-bat continues with the count intact.
    assert_eq!((score.balls, score.strikes), (2, 1));
}

#[test]
fn only_the_lead_eligible_runner_steals() {
    // Runners on first and second: second steals third; first stays put
    // (his target is now... still second — one steal per pitch).
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0, 1]);
    assert_eq!(
        attempt_steal(&mut score, &mut bases, &std_rules(), true, false),
        StealResult::Stolen { base: 2 }
    );
    assert_eq!(bases, with(&[0, 2]));
}

#[test]
fn home_cannot_be_stolen() {
    let mut score = ScoreBoard::default();
    let mut bases = with(&[2]);
    assert_eq!(
        attempt_steal(&mut score, &mut bases, &std_rules(), true, false),
        StealResult::NoRunner
    );
    assert_eq!(bases, with(&[2]));
}

#[test]
fn empty_bases_cannot_steal() {
    let mut score = ScoreBoard::default();
    let mut bases = empty();
    assert_eq!(
        attempt_steal(&mut score, &mut bases, &std_rules(), true, false),
        StealResult::NoRunner
    );
}

#[test]
fn big_jump_beats_even_a_fastball() {
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_steal(&mut score, &mut bases, &std_rules(), false, true),
        StealResult::Stolen { base: 1 }
    );
    assert_eq!(bases, with(&[1]));
    assert_eq!(score.outs, 0);
}

// ── Pickoffs ──────────────────────────────────────────────────────────────

#[test]
fn pickoff_catches_an_extended_lead() {
    let mut score = ScoreBoard {
        balls: 1,
        strikes: 2,
        ..Default::default()
    };
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), true),
        PickoffResult::PickedOff { base: 0 }
    );
    assert_eq!(bases, empty());
    assert_eq!(score.outs, 1);
    // The batter's count survives — no pitch was thrown.
    assert_eq!((score.balls, score.strikes), (1, 2));
}

#[test]
fn pickoff_on_a_normal_lead_is_safe() {
    let mut score = ScoreBoard::default();
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), false),
        PickoffResult::SafeBack
    );
    assert_eq!(bases, with(&[0]));
    assert_eq!(score.outs, 0);
}

#[test]
fn pickoff_with_nobody_leading_is_no_play() {
    let mut score = ScoreBoard::default();
    let mut bases = empty();
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), true),
        PickoffResult::NoRunner
    );
}

#[test]
fn pickoff_third_out_retires_the_side() {
    let mut score = ScoreBoard {
        outs: 2,
        top_of_inning: true,
        inning: 1,
        ..Default::default()
    };
    let mut bases = with(&[0]);
    assert_eq!(
        attempt_pickoff(&mut score, &mut bases, &std_rules(), true),
        PickoffResult::PickedOff { base: 0 }
    );
    assert_eq!(score.outs, 0, "side retired: outs reset");
    assert!(!score.top_of_inning, "half-inning flips on the third out");
    assert_eq!(bases, empty());
}
