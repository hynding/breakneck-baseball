//! Unit tests for [`super`] — the advance module.

use super::super::test_support::*;
use super::*;

// ── Base running ──────────────────────────────────────────────────────────

#[test]
fn single_puts_batter_on_first() {
    let mut b = empty();
    assert_eq!(advance_hit(&mut b, 1), 0);
    assert_eq!(b, with(&[0]));
}

#[test]
fn single_scores_runner_from_third() {
    let mut b = with(&[2]);
    // Everyone advances one: third scores, batter to first.
    assert_eq!(advance_hit(&mut b, 1), 1);
    assert_eq!(b, with(&[0]));
}

#[test]
fn grand_slam_clears_bases_and_scores_four() {
    let mut b = loaded();
    assert_eq!(advance_hit(&mut b, 4), 4);
    assert_eq!(b, empty());
}

#[test]
fn double_with_runner_on_first() {
    let mut b = with(&[0]);
    // Batter to second, runner from first to third.
    assert_eq!(advance_hit(&mut b, 2), 0);
    assert_eq!(b, with(&[1, 2]));
}

#[test]
fn walk_forces_only_when_bases_ahead_are_occupied() {
    let mut b = empty();
    assert_eq!(advance_walk(&mut b), 0);
    assert_eq!(b, with(&[0]));

    // Runner on first: batter forces them to second.
    let mut b = with(&[0]);
    assert_eq!(advance_walk(&mut b), 0);
    assert_eq!(b, with(&[0, 1]));

    // Bases loaded: forces in a run, still loaded.
    let mut b = loaded();
    assert_eq!(advance_walk(&mut b), 1);
    assert_eq!(b, loaded());
}

#[test]
fn four_base_walk_chain_only_scores_when_all_full() {
    let mut b = Bases::new(4);
    for expected in [0, 0, 0, 0, 1] {
        assert_eq!(advance_walk(&mut b), expected);
    }
}

#[test]
fn four_base_hit_advancement() {
    let mut b = Bases::new(4);
    // Batter reaches the fourth base without scoring.
    assert_eq!(advance_hit(&mut b, 4), 0);
    assert!(b.is_occupied(3));
    // A five-base homer scores that runner and the batter.
    assert_eq!(advance_hit(&mut b, 5), 2);
    assert_eq!(b, Bases::new(4));
}

#[test]
fn hit_and_run_sends_first_to_third_on_a_single() {
    let mut b = with(&[0]);
    // Runner takes two (the jump), batter takes one.
    assert_eq!(advance_hit_with_jump(&mut b, 1, true), 0);
    assert_eq!(b, with(&[0, 2]));
}

#[test]
fn apply_hit_credits_runs_and_resets_the_count() {
    let mut score = ScoreBoard {
        balls: 2,
        strikes: 1,
        top_of_inning: false, // Home bats
        ..Default::default()
    };
    let mut bases = with(&[1]);
    // Double: runner on second scores, batter to second.
    assert_eq!(apply_hit(&mut score, &mut bases, 2, false), 1);
    assert_eq!(score.home_runs, 1);
    assert_eq!((score.balls, score.strikes), (0, 0));
    assert_eq!(bases, with(&[1]));
}
