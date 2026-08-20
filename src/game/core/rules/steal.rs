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
mod tests {
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
}
