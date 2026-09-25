//! Base-running advancement: hits, walks, and the batted-out consequences
//! that move existing runners without a fresh at-bat.

use crate::game::ScoreBoard;

use super::{Bases, reset_count};

/// Advances runners for a clean hit where everyone moves up `hit_bases`.
/// `hit_bases` may exceed the base count by one (a home run clears the field
/// and scores the batter). Returns the number of runs that scored.
#[must_use]
pub fn advance_hit(bases: &mut Bases, hit_bases: u32) -> u32 {
    advance_hit_with_jump(bases, hit_bases, false)
}

/// [`advance_hit`], but `jump` gives every *existing* runner one extra base —
/// the hit-and-run reward for breaking with the pitch (first-to-third on a
/// single). The batter still takes exactly `hit_bases`.
#[must_use]
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
#[must_use]
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
#[must_use]
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
#[path = "advance.test.rs"]
mod tests;
