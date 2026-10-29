//! Steals and pickoffs: the pre-pitch leadoff duel.

use bevy::math::Vec2;

use crate::game::ScoreBoard;
use crate::game::variant::Ruleset;

use super::{
    BASE_DISTANCE_M, Bases, HALF_DIAGONAL, PITCH_DISTANCE, PitchKind, ZONE_HALF_WIDTH, ZONE_LOW,
    charge_out,
};

/// The runner who breaks on a steal or hit-and-run: the lead runner whose
/// next base is open (home can never be stolen here).
pub fn steal_candidate(bases: &Bases) -> Option<usize> {
    let n = bases.count();
    (0..n.saturating_sub(1))
        .rev()
        .find(|&b| bases.is_occupied(b) && !bases.is_occupied(b + 1))
}

// ── The steal race ────────────────────────────────────────────────────────────
// A steal is a race between two clocks that both start at the pitcher's first
// movement: the runner's (jump + sprint to the bag) and the defense's (delivery
// to the catcher + pop time + the tag). Per docs/BASEBALL.md ("Stolen-base
// timing"), an MLB delivery takes ~1.3–1.5 s, an average pop time to second
// is ~2.0 s, and a runner from a normal lead gets there in ~3.3–3.5 s — the
// "3.3 rule": a battery under 3.3 s throws out most runners. Everything below
// is regulation-diamond seconds (see [`steal_race`]).

/// First movement → release from the stretch (the pitch's flight is added on
/// top from the pitch's own speed, so a changeup is slower to the plate).
pub const DELIVERY_MOVE_SECS: f32 = 0.92;
/// Release-point extension: the ball leaves the hand this far in front of the
/// rubber, so it flies `pitch_distance − this` to the mitt.
const RELEASE_EXTENSION_M: f32 = 1.8;
/// The catcher's catch-and-transfer before the ball leaves his hand.
pub const CATCHER_EXCHANGE_SECS: f32 = 0.7;
/// Catcher's throw speed averaged over the throw (arc and tag included):
/// 0.7 s exchange + 38.8 m at this pace is the ~2.0 s average pop to second,
/// and the same arm makes the ~1.6 s pop to third.
pub const CATCHER_THROW_SPEED: f32 = 29.8;
/// A runner's top sprint speed (MLB average sprint speed ~27 ft/s).
pub const RUNNER_SPRINT_SPEED: f32 = 8.2;
/// Reaction, acceleration and slide on top of the flat-out sprint: with a
/// normal lead this lands an average runner at second ~3.38 s after first
/// movement — a step behind a fastball (1.36 s + 2.0 s pop = 3.36 s) and
/// ahead of every off-speed pitch.
pub const RUNNER_OVERHEAD_SECS: f32 = 0.30;
/// The normal lead off first (regulation metres): what every runner takes.
pub const LEAD_NORMAL_M: f32 = 2.2;
/// The furthest a runner stretches his lead while the offense holds Down.
pub const LEAD_MAX_M: f32 = 4.5;
/// How fast a stretched lead grows — and shrinks back — while the pitcher
/// holds the ball (the runner's shuffle).
pub const LEAD_SHUFFLE_SPEED: f32 = 2.6;
/// A runner on second isn't held: his walking secondary lead is this much
/// longer than the lead a runner on first gets.
pub const SECOND_BASE_LEAD_BONUS_M: f32 = 2.5;
/// A send at the very end of the wind-up costs this much jump — a runner who
/// breaks late against the delivery is this far behind one who broke on
/// first movement.
pub const LATE_BREAK_SECS: f32 = 0.5;
/// Extra transfer when the catcher has to dig a pitch out low: added in full
/// once the pitch crosses `LOW_PITCH_SPAN_M` under the zone (a ball in the
/// dirt).
pub const LOW_PITCH_PENALTY_SECS: f32 = 0.25;
const LOW_PITCH_SPAN_M: f32 = 0.5;
/// Extra transfer when the catcher reaches across for a pitch off the plate.
pub const WIDE_PITCH_PENALTY_SECS: f32 = 0.1;
const WIDE_PITCH_SPAN_M: f32 = 0.5;
/// A pickoff throw: the pitcher's spin move and the throw to the bag, glove
/// to tag.
pub const PICKOFF_THROW_SECS: f32 = 0.8;
/// Reaction before the runner dives back.
pub const PICKOFF_REACTION_SECS: f32 = 0.2;
/// The runner's dive back to the bag.
pub const DIVE_BACK_SPEED: f32 = 6.0;

/// Everything the steal race needs to know about one sent runner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StealAttempt {
    /// The pitch the runner went on — its speed sets the delivery time.
    pub kind: PitchKind,
    /// Where it crossed the plate (`None`: never recorded — no penalty).
    pub crossing: Option<Vec2>,
    /// How far into the wind-up the runner broke, 0 (on first movement, or
    /// already going from a stretched lead) to 1 (as the ball was released).
    pub break_fraction: f32,
    /// The lead he broke from (regulation metres, [`LEAD_NORMAL_M`] for a
    /// normal lead).
    pub lead_m: f32,
}

impl StealAttempt {
    /// A send on first movement from a normal lead.
    pub fn on_first_move(kind: PitchKind) -> Self {
        Self {
            kind,
            crossing: None,
            break_fraction: 0.0,
            lead_m: LEAD_NORMAL_M,
        }
    }
}

/// Both clocks of a steal race, in seconds from the pitcher's first movement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StealRace {
    /// The runner's arrival at the bag.
    pub runner_secs: f32,
    /// The tag: delivery + catcher's transfer + throw.
    pub defense_secs: f32,
}

impl StealRace {
    /// The throw has to *beat* the runner — a dead heat is safe.
    pub fn safe(&self) -> bool {
        self.runner_secs <= self.defense_secs
    }
}

/// First movement → the catcher's mitt for `kind` (game pitch speeds, so the
/// arcade changeup really is slow to the plate).
pub fn delivery_secs(kind: PitchKind) -> f32 {
    DELIVERY_MOVE_SECS + (PITCH_DISTANCE - RELEASE_EXTENSION_M) / kind.speed()
}

/// The catcher's extra transfer for where the pitch crossed: nothing for a
/// pitch in the zone or above it, up to [`LOW_PITCH_PENALTY_SECS`] for one in
/// the dirt and [`WIDE_PITCH_PENALTY_SECS`] for one he reaches across for.
pub fn receive_penalty_secs(crossing: Option<Vec2>) -> f32 {
    let Some(c) = crossing else {
        return 0.0;
    };
    let low = ((ZONE_LOW - c.y) / LOW_PITCH_SPAN_M).clamp(0.0, 1.0);
    let wide = ((c.x.abs() - ZONE_HALF_WIDTH) / WIDE_PITCH_SPAN_M).clamp(0.0, 1.0);
    low * LOW_PITCH_PENALTY_SECS + wide * WIDE_PITCH_PENALTY_SECS
}

/// Times the race for the runner on `runner` (0-indexed) stealing the next
/// bag. The race is run on the regulation diamond whatever field the variant
/// draws — a backyard's short base paths would otherwise make every steal
/// (or none) succeed — so only *which* bag matters: the first steal is the
/// long throw to second, every later one the short throw to third.
pub fn steal_race(runner: usize, attempt: &StealAttempt) -> StealRace {
    let (lead_bonus, throw_m) = if runner == 0 {
        (0.0, 2.0 * HALF_DIAGONAL)
    } else {
        (SECOND_BASE_LEAD_BONUS_M, BASE_DISTANCE_M)
    };
    let lead = attempt.lead_m + lead_bonus;
    let runner_secs = attempt.break_fraction.clamp(0.0, 1.0) * LATE_BREAK_SECS
        + RUNNER_OVERHEAD_SECS
        + (BASE_DISTANCE_M - lead).max(0.0) / RUNNER_SPRINT_SPEED;
    let defense_secs = delivery_secs(attempt.kind)
        + receive_penalty_secs(attempt.crossing)
        + CATCHER_EXCHANGE_SECS
        + throw_m / CATCHER_THROW_SPEED;
    StealRace {
        runner_secs,
        defense_secs,
    }
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

/// Resolves a straight steal on a pitch the batter didn't put in play by
/// running the [`steal_race`]: a good jump beats an average battery on
/// off-speed stuff, a fastball cuts down a runner who broke on first
/// movement, a pitch in the dirt gives the runner the transfer, and a lead
/// stretched through the pre-pitch duel (bought at pickoff risk, see
/// [`attempt_pickoff`]) outruns anything. One runner (the lead eligible one)
/// goes per pitch.
pub fn attempt_steal(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    attempt: &StealAttempt,
) -> StealResult {
    let Some(runner) = steal_candidate(bases) else {
        return StealResult::NoRunner;
    };
    bases.set(runner, false);
    if steal_race(runner, attempt).safe() {
        bases.set(runner + 1, true);
        StealResult::Stolen { base: runner + 1 }
    } else {
        charge_out(score, bases, rules);
        StealResult::Caught
    }
}

/// The runner's next lead after `dt` seconds of the pre-pitch duel: a
/// stretch (`extending`) shuffles out toward [`LEAD_MAX_M`], letting go
/// shuffles back toward [`LEAD_NORMAL_M`] — at the same pace, so a bluff
/// takes as long to undo as it took to make.
pub fn shuffle_lead(lead_m: f32, extending: bool, dt: f32) -> f32 {
    let step = LEAD_SHUFFLE_SPEED * dt;
    if extending {
        (lead_m + step).min(LEAD_MAX_M)
    } else {
        (lead_m - step).max(LEAD_NORMAL_M)
    }
}

/// Seconds the runner needs to get back to the bag from `lead_m` on a
/// pickoff throw.
pub fn dive_back_secs(lead_m: f32) -> f32 {
    PICKOFF_REACTION_SECS + lead_m / DIVE_BACK_SPEED
}

/// What a pickoff throw during the pre-pitch window produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum PickoffResult {
    /// Caught off the bag mid-extension — the runner is out.
    PickedOff { base: usize },
    /// The runner dove back in time.
    SafeBack,
    /// Nobody was leading off anywhere.
    NoRunner,
}

/// Resolves a pickoff throw at the lead eligible runner: a race between the
/// throw ([`PICKOFF_THROW_SECS`]) and his dive back from `lead_m`. A normal
/// lead is always back in time; a stretch only gets caught once it has
/// shuffled out past ~3.6 m — so a quick throw at a fresh stretch wastes the
/// move, and a runner who has been hanging out there is a dead man.
pub fn attempt_pickoff(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    rules: &Ruleset,
    lead_m: f32,
) -> PickoffResult {
    let Some(runner) = steal_candidate(bases) else {
        return PickoffResult::NoRunner;
    };
    if dive_back_secs(lead_m) > PICKOFF_THROW_SECS {
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
