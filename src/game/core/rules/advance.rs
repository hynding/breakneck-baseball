//! Base-running advancement: hits, walks, and the batted-out consequences
//! that move existing runners without a fresh at-bat.

use crate::game::ScoreBoard;

use super::{Bases, TagUp, reset_count};

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
    let mut next = vec![None; n];

    for (base, slot) in bases.runners.iter().enumerate() {
        if let Some(runner) = *slot {
            let dest = base + runner_step;
            if dest >= n {
                runs += 1; // past the last base → scored
            } else {
                next[dest] = Some(runner);
            }
        }
    }
    // The batter reaches base `hit_bases` (1-indexed); one past the last base
    // means they came all the way around.
    if batter_step > n {
        runs += 1;
    } else {
        next[batter_step - 1] = Some(bases.batter);
    }

    bases.runners = next;
    runs
}

/// Advances only forced runners for a walk: the batter takes first and pushes
/// the chain ahead of them. Returns runs scored (a fully-loaded walk forces in
/// one run).
#[must_use]
pub fn advance_walk(bases: &mut Bases) -> u32 {
    // The forced chain is every runner from first up to the first open bag;
    // each moves up one, and with no open bag the lead runner scores.
    let n = bases.count();
    let open = (0..n).find(|&b| !bases.is_occupied(b));
    let runs = u32::from(open.is_none());
    let top = open.unwrap_or(n);
    for base in (0..top).rev() {
        let runner = bases.take(base);
        bases.put(base + 1, runner);
    }
    bases.put(0, Some(bases.batter));
    runs
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
        if let Some(runner) = bases.take(base) {
            let dest = base + n as usize;
            if dest >= count {
                runs += 1;
            } else {
                bases.put(dest, Some(runner));
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
        if let Some(runner) = bases.take(base) {
            if base + 1 >= n {
                runs += 1;
            } else {
                bases.put(base + 1, Some(runner));
            }
        }
    }
    runs
}

/// Tag-ups on a caught fly: the runner on the last base scores or is cut
/// down at the plate per his race (`tag`), and on a `deep` fly the runner one
/// behind moves up into a vacated last base. Trailing runners hold. Returns
/// (runs scored, runners thrown out).
pub(super) fn tag_up(bases: &mut Bases, deep: bool, tag: TagUp) -> (u32, u32) {
    let n = bases.count();
    let mut runs = 0;
    let mut outs = 0;
    if n >= 1 && bases.is_occupied(n - 1) && tag != TagUp::Hold {
        bases.take(n - 1);
        if tag == TagUp::Scores {
            runs += 1;
        } else {
            outs += 1;
        }
    }
    if deep && n >= 2 && bases.is_occupied(n - 2) && !bases.is_occupied(n - 1) {
        let runner = bases.take(n - 2);
        bases.put(n - 1, runner);
    }
    (runs, outs)
}

/// Advances runners for a hit worth `hit_bases` on which the runner from
/// `reader` tried for one base more: he takes it when `safe` (scoring past
/// the last base) and is removed otherwise; everyone else, batter included,
/// takes exactly `hit_bases`. Returns (runs scored, runners thrown out).
pub(super) fn advance_extra_base(
    bases: &mut Bases,
    hit_bases: u32,
    reader: usize,
    safe: bool,
) -> (u32, u32) {
    let n = bases.count();
    let reader_on = bases.take(reader);
    let mut runs = advance_hit_with_jump(bases, hit_bases, false);
    let mut outs = 0;
    if reader_on.is_some() {
        if !safe {
            outs += 1;
        } else if reader + hit_bases as usize + 1 >= n {
            runs += 1;
        } else {
            bases.put(reader + hit_bases as usize + 1, reader_on);
        }
    }
    (runs, outs)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "advance.test.rs"]
mod tests;
