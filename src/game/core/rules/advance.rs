//! Base-running advancement: hits, walks, and the batted-out consequences
//! that move existing runners without a fresh at-bat.

use crate::game::ScoreBoard;

use super::{Bases, reset_count};

/// Advances runners for a clean hit where everyone moves up `hit_bases`.
/// `hit_bases` may exceed the base count by one (a home run clears the field
/// and scores the batter). Returns the number of runs that scored.
pub fn advance_hit(bases: &mut Bases, hit_bases: u32) -> u32 {
    advance_hit_with_jump(bases, hit_bases, false)
}

/// [`advance_hit`], but `jump` gives every *existing* runner one extra base —
/// the hit-and-run reward for breaking with the pitch (first-to-third on a
/// single). The batter still takes exactly `hit_bases`.
pub fn advance_hit_with_jump(bases: &mut Bases, hit_bases: u32, jump: bool) -> u32 {
    debug_assert!(hit_bases >= 1, "a hit is worth at least one base");
    let n = bases.count();
    let runner_step = hit_bases as usize + jump as usize;
    let batter_step = hit_bases as usize;
    let mut runs = 0;
    let mut next = vec![false; n];

    for base in 0..n {
        if bases.is_occupied(base) {
            let dest = base + runner_step;
            if dest >= n {
                runs += 1; // past the last base → scored
            } else {
                next[dest] = true;
            }
        }
    }
    // The batter reaches base `hit_bases` (1-indexed); one past the last base
    // means they came all the way around.
    if batter_step > n {
        runs += 1;
    } else {
        next[batter_step - 1] = true;
    }

    bases.occupied = next;
    runs
}

/// Advances only forced runners for a walk: the batter takes first and pushes
/// the chain ahead of them. Returns runs scored (a fully-loaded walk forces in
/// one run).
pub fn advance_walk(bases: &mut Bases) -> u32 {
    for base in 0..bases.count() {
        if !bases.is_occupied(base) {
            bases.set(base, true);
            return 0;
        }
    }
    1 // every base occupied: the lead runner is forced home
}

/// Applies a hit worth `hit_bases` bases: advances runners (with the
/// hit-and-run `jump` when runners were going), credits runs to the batting
/// team, and ends the at-bat. Returns the runs scored.
pub fn apply_hit(score: &mut ScoreBoard, bases: &mut Bases, hit_bases: u32, jump: bool) -> u32 {
    let runs = advance_hit_with_jump(bases, hit_bases, jump);
    score.add_runs(runs);
    reset_count(score);
    runs
}

/// Advances every *existing* runner `n` bases without placing the batter —
/// the base state after the batter is cut down stretching. Returns runs.
pub(super) fn advance_runners_only(bases: &mut Bases, n: u32) -> u32 {
    let count = bases.count();
    let mut runs = 0;
    for base in (0..count).rev() {
        if bases.is_occupied(base) {
            bases.set(base, false);
            let dest = base + n as usize;
            if dest >= count {
                runs += 1;
            } else {
                bases.set(dest, true);
            }
        }
    }
    runs
}

/// After the batter is retired on the ground, every runner advances one base
/// (the defense takes the sure out at first). Returns runs forced across.
pub(super) fn advance_trailing(bases: &mut Bases) -> u32 {
    let n = bases.count();
    let mut runs = 0;
    // Walk from the lead base down so nobody leapfrogs.
    for base in (0..n).rev() {
        if bases.is_occupied(base) {
            bases.set(base, false);
            if base + 1 >= n {
                runs += 1;
            } else {
                bases.set(base + 1, true);
            }
        }
    }
    runs
}

/// Tag-up on a deep fly: the runner on the last base scores and the runner
/// one behind moves up. Trailing runners hold. Returns runs scored.
pub(super) fn tag_up(bases: &mut Bases) -> u32 {
    let n = bases.count();
    let mut runs = 0;
    if n >= 1 && bases.is_occupied(n - 1) {
        bases.set(n - 1, false);
        runs += 1;
    }
    if n >= 2 && bases.is_occupied(n - 2) {
        bases.set(n - 2, false);
        bases.set(n - 1, true);
    }
    runs
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
}
