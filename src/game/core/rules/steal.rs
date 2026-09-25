//! Steals and pickoffs: the pre-pitch leadoff duel.

use crate::game::ScoreBoard;
use crate::game::variant::Ruleset;

use super::{Bases, charge_out};

/// The runner who breaks on a steal or hit-and-run: the lead runner whose
/// next base is open (home can never be stolen here).
pub fn steal_candidate(bases: &Bases) -> Option<usize> {
    let n = bases.count();
    (0..n.saturating_sub(1))
        .rev()
        .find(|&b| bases.is_occupied(b) && !bases.is_occupied(b + 1))
}

/// What sending the runner produced once the pitch reached the catcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum StealResult {
    /// Safe — the runner now stands on `base` (0-indexed).
    Stolen { base: usize },
    /// Thrown out; the out is charged but the batter's count stands.
    Caught,
    /// Nobody was in a position to steal.
    NoRunner,
}

/// Resolves a straight steal on a pitch the batter didn't put in play: the
/// jump beats the throw on off-speed stuff, but a fastball gets there in
/// time — unless the runner broke from an extended lead (`big_jump`), which
/// beats any pitch. The extended lead was the gamble: it exposed the runner
/// to a pickoff during the pre-pitch window (see [`attempt_pickoff`]). One
/// runner (the lead eligible one) goes per pitch.
pub fn attempt_steal(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    off_speed: bool,
    big_jump: bool,
) -> StealResult {
    let Some(runner) = steal_candidate(bases) else {
        return StealResult::NoRunner;
    };
    if off_speed || big_jump {
        bases.set(runner, false);
        bases.set(runner + 1, true);
        StealResult::Stolen { base: runner + 1 }
    } else {
        bases.set(runner, false);
        charge_out(score, bases, rules);
        StealResult::Caught
    }
}

/// What a pickoff throw during the pre-pitch window produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum PickoffResult {
    /// Caught off the bag mid-extension — the runner is out.
    PickedOff { base: usize },
    /// The runner dove back in time (a normal lead is always safe).
    SafeBack,
    /// Nobody was leading off anywhere.
    NoRunner,
}

/// Resolves a pickoff throw at the lead eligible runner. The analytic model
/// keeps runners glued to the bag on a normal lead — only an *extended* lead
/// (the offense arming an early steal) strays far enough to be caught. This
/// is the deterministic counter to the guaranteed [`attempt_steal`] big jump.
pub fn attempt_pickoff(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    extended: bool,
) -> PickoffResult {
    let Some(runner) = steal_candidate(bases) else {
        return PickoffResult::NoRunner;
    };
    if extended {
        bases.set(runner, false);
        charge_out(score, bases, rules);
        PickoffResult::PickedOff { base: runner }
    } else {
        PickoffResult::SafeBack
    }
}

/// Removes the runner who was sent, caught off base when the ball was
/// caught. Returns whether anyone was actually going.
pub(super) fn double_off_lead_runner(bases: &mut Bases) -> bool {
    if let Some(runner) = steal_candidate(bases) {
        bases.set(runner, false);
        true
    } else {
        false
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "steal.test.rs"]
mod tests;
