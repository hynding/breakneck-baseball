//! Count & scoring mutations: balls, strikes, outs, half-inning flips, and
//! game-end.

use crate::game::ScoreBoard;
use crate::game::variant::Ruleset;

use super::{
    BallCall, Bases, OutKind, StrikeCall, advance_runners_only, advance_trailing, advance_walk,
    double_off_lead_runner, tag_up,
};

/// Records a taken ball. The final ball walks the batter (forcing runners) and
/// ends the at-bat.
pub fn call_ball(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) -> BallCall {
    score.balls += 1;
    if score.balls >= rules.counts.balls_per_walk {
        let runs = advance_walk(bases);
        score.add_runs(runs);
        reset_count(score);
        BallCall::Walk { runs }
    } else {
        BallCall::Ball
    }
}

/// Records a strike (called or swinging). The final strike is an out —
/// unless `dropped_third` (the ball got away and first base was open), in
/// which case the batter reaches and no out is recorded.
pub fn call_strike(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    dropped_third: bool,
) -> StrikeCall {
    score.strikes += 1;
    if score.strikes >= rules.counts.strikes_per_out {
        if dropped_third {
            reset_count(score);
            bases.set(0, true);
            StrikeCall::DroppedThird
        } else {
            record_out(score, bases, rules);
            StrikeCall::Strikeout
        }
    } else {
        StrikeCall::Strike
    }
}

/// Records a foul ball: a strike, unless it would be the last one.
pub fn foul(score: &mut ScoreBoard, rules: &Ruleset) {
    if score.strikes + 1 < rules.counts.strikes_per_out {
        score.strikes += 1;
    }
}

/// Charges one out *without* ending the at-bat (a runner retired on the
/// bases). Flips the half-inning once the side is retired — which also wipes
/// the count, since the interrupted batter starts over next half.
pub fn charge_out(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) {
    score.outs += 1;
    if score.outs >= rules.counts.outs_per_half {
        score.outs = 0;
        reset_count(score);
        bases.clear();
        if score.top_of_inning {
            score.top_of_inning = false;
        } else {
            score.top_of_inning = true;
            score.inning += 1;
        }
    }
}

/// Records an out that ends the at-bat, flipping the half-inning once the
/// side is retired.
pub fn record_out(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) {
    reset_count(score);
    charge_out(score, bases, rules);
}

/// The base-running consequences of a batted-ball out.
pub struct OutPlay {
    /// Outs recorded on the play (1, or 2 for double plays / doubled-off).
    pub outs: u32,
    /// Runs that scored (sacrifice flies, runs crossing on a non-ending play).
    pub runs: u32,
    /// The classic force-and-relay two outs.
    pub double_play: bool,
    /// A sent runner was caught off base when the ball was caught.
    pub doubled_off: bool,
}

/// Applies a batted-ball out with its base-running consequences.
/// `runners_going` is the hit-and-run flag: runners broke with the pitch, so
/// a caught ball doubles the runner off and nobody tags up. Whether a
/// grounder turns two is no longer decided here — [`resolve_thrown`] races
/// the actual relay and reports [`Outcome::DoublePlay`] /
/// [`Outcome::FieldersChoice`] outright (see [`apply_double_play`] and
/// [`apply_fielders_choice`]).
pub fn apply_batted_out(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    kind: OutKind,
    runners_going: bool,
) -> OutPlay {
    let outs_left = rules.counts.outs_per_half.saturating_sub(score.outs);
    let mut play = OutPlay {
        outs: 1,
        runs: 0,
        double_play: false,
        doubled_off: false,
    };
    match kind {
        OutKind::Ground => {
            // The defense took the sure out at first; unless the play ends
            // the inning, everyone else moved up a base.
            if play.outs < outs_left {
                play.runs = advance_trailing(bases);
            }
        }
        OutKind::Fly { deep } => {
            if runners_going {
                play.doubled_off = double_off_lead_runner(bases);
            } else if deep && outs_left > 1 {
                play.runs = tag_up(bases);
            }
        }
        OutKind::Pop | OutKind::FoulPop => {
            if runners_going {
                play.doubled_off = double_off_lead_runner(bases);
            }
        }
        OutKind::Pegged => {}
        // Cut down stretching: the other runners keep the bases they earned
        // (a timing play — any run that crossed counts).
        OutKind::Stretching { advanced } => {
            play.runs = advance_runners_only(bases, advanced);
        }
    }
    if play.doubled_off {
        play.outs += 1;
    }
    // Never charge past the end of the half — a second out on the play can't
    // leak into the next half-inning.
    play.outs = play.outs.min(outs_left);
    score.add_runs(play.runs);
    reset_count(score);
    for _ in 0..play.outs {
        charge_out(score, bases, rules);
    }
    play
}

/// Applies [`Outcome::DoublePlay`]: the forced runner at second and the
/// batter at first, with the trailing advance only when the play doesn't end
/// the inning — identical base math to the old fiat double play. With one
/// out remaining only the force counts (the inning ends on it).
pub fn apply_double_play(score: &mut ScoreBoard, bases: &mut Bases, rules: &Ruleset) -> OutPlay {
    let outs_left = rules.counts.outs_per_half.saturating_sub(score.outs);
    let mut play = OutPlay {
        outs: 2.min(outs_left),
        runs: 0,
        double_play: true,
        doubled_off: false,
    };
    bases.set(0, false); // the forced runner dies at the middle bag
    if play.outs < outs_left {
        play.runs = advance_trailing(bases);
    }
    score.add_runs(play.runs);
    reset_count(score);
    for _ in 0..play.outs {
        charge_out(score, bases, rules);
    }
    play
}

/// Applies [`Outcome::FieldersChoice`]: the forced runner is retired at
/// `out_base` while the batter reaches first; the forced runners behind the
/// out move up with him and everyone ahead holds. Never scores a run.
pub fn apply_fielders_choice(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    out_base: usize,
) -> OutPlay {
    let outs_left = rules.counts.outs_per_half.saturating_sub(score.outs);
    let play = OutPlay {
        outs: 1,
        runs: 0,
        double_play: false,
        doubled_off: false,
    };
    if out_base > 0 {
        bases.set(out_base - 1, false);
    }
    if play.outs < outs_left {
        // Everyone behind the out was forced by the batter: each moves up,
        // and the batter takes first.
        for base in (0..out_base.saturating_sub(1)).rev() {
            if bases.is_occupied(base) {
                bases.set(base, false);
                bases.set(base + 1, true);
            }
        }
        bases.set(0, true);
    }
    reset_count(score);
    charge_out(score, bases, rules);
    play
}

/// Resets balls and strikes for a new at-bat.
pub fn reset_count(score: &mut ScoreBoard) {
    score.balls = 0;
    score.strikes = 0;
}

// ── Game end ──────────────────────────────────────────────────────────────────

/// Returns `true` if the game is over given the current score and inning count.
pub fn is_game_over(score: &ScoreBoard, innings: u32) -> bool {
    // Home has won (or walked off) once regulation is reached and it leads while
    // batting/entering the bottom half.
    if !score.top_of_inning && score.inning >= innings && score.home_runs > score.away_runs {
        return true;
    }
    // A completed bottom half (we've advanced past regulation) that is not tied.
    if score.top_of_inning && score.inning > innings && score.home_runs != score.away_runs {
        return true;
    }
    false
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "count.test.rs"]
mod tests;
