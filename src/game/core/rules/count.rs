//! Count & scoring mutations: balls, strikes, outs, half-inning flips, and
//! game-end.

use crate::game::ScoreBoard;
use crate::game::variant::Ruleset;

use super::{
    BallCall, Bases, OutKind, StrikeCall, advance_extra_base, advance_runners_only,
    advance_trailing, advance_walk, double_off_lead_runner, tag_up,
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

/// The defense puts the batter on intentionally: he takes first, forced
/// runners move up, and the count is wiped — no pitches thrown (the MLB
/// rule since 2017, per docs/BASEBALL.md). Returns runs forced in.
pub fn intentional_walk(score: &mut ScoreBoard, bases: &mut Bases) -> u32 {
    let runs = advance_walk(bases);
    score.add_runs(runs);
    reset_count(score);
    runs
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
            bases.put(0, Some(bases.batter()));
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
    /// The runner tagging from third was thrown out at the plate.
    pub thrown_out_home: bool,
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
        thrown_out_home: false,
    };
    match kind {
        OutKind::Ground => {
            // The defense took the sure out at first; unless the play ends
            // the inning, everyone else moved up a base.
            if play.outs < outs_left {
                play.runs = advance_trailing(bases);
            }
        }
        OutKind::Fly { deep, tag } => {
            if runners_going {
                play.doubled_off = double_off_lead_runner(bases);
            } else if outs_left > 1 {
                let (runs, cut_down) = tag_up(bases, deep, tag);
                play.runs = runs;
                play.outs += cut_down;
                play.thrown_out_home = cut_down > 0;
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

/// Applies [`Outcome::DoublePlay`]: the runner forced to `out_base`
/// (0-indexed; `base_count()` = home plate) and the batter at first. Unless
/// the play ends the inning, every other runner moves up a base — the ones
/// behind the force were forced too, and a runner ahead of a force at second
/// scores from third, as on the 6-4-3 with the corners full. A force at the
/// plate therefore never scores a run. With one out remaining only the force
/// counts (the inning ends on it).
pub fn apply_double_play(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    out_base: usize,
) -> OutPlay {
    let outs_left = rules.counts.outs_per_half.saturating_sub(score.outs);
    let mut play = OutPlay {
        outs: 2.min(outs_left),
        runs: 0,
        double_play: true,
        doubled_off: false,
        thrown_out_home: false,
    };
    // The forced runner dies at the bag he was running to.
    if out_base > 0 {
        bases.take(out_base - 1);
    }
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

/// Applies [`Outcome::FieldersChoice`]: the runner forced to `out_base` is
/// retired while the batter reaches first. Every other runner in the force
/// chain (the consecutively occupied bags from first, all forced by the
/// batter at contact) takes the next bag — the lead one scoring from third
/// with the bases loaded and the force at second or third. Runners beyond the
/// chain were not forced and hold. Nothing moves when the out ends the
/// inning. Per docs/BASEBALL.md ("Force play").
pub fn apply_fielders_choice(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    out_base: usize,
) -> OutPlay {
    let outs_left = rules.counts.outs_per_half.saturating_sub(score.outs);
    let mut play = OutPlay {
        outs: 1,
        runs: 0,
        double_play: false,
        doubled_off: false,
        thrown_out_home: false,
    };
    let n = bases.count();
    let forced = (0..n).take_while(|&b| bases.is_occupied(b)).count();
    if out_base > 0 {
        bases.take(out_base - 1);
    }
    if play.outs < outs_left {
        // Lead runner first, so nobody lands on a bag still occupied.
        for base in (0..forced).rev() {
            if let Some(runner) = bases.take(base) {
                if base + 1 >= n {
                    play.runs += 1;
                } else {
                    bases.put(base + 1, Some(runner));
                }
            }
        }
        bases.put(0, Some(bases.batter()));
    }
    score.add_runs(play.runs);
    reset_count(score);
    charge_out(score, bases, rules);
    play
}

/// Applies [`Outcome::ExtraBase`]: a `hit` worth that many bases on which
/// the runner from `runner` went for one more — taking it when `safe`, else
/// thrown out there. Runs that crossed ahead of him count (they were home
/// before the tag). Returns (runs scored, outs recorded).
pub fn apply_extra_base(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    hit: u32,
    runner: usize,
    safe: bool,
) -> (u32, u32) {
    let (runs, outs) = advance_extra_base(bases, hit, runner, safe);
    score.add_runs(runs);
    reset_count(score);
    for _ in 0..outs {
        charge_out(score, bases, rules);
    }
    (runs, outs)
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
