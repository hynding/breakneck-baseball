//! Pure baseball rules — no ECS systems, no physics engine, fully unit-tested.
//!
//! Everything in this module is a plain function over plain data: given the
//! current count/bases/score and an input (a batted-ball velocity, a called
//! pitch), it mutates the state and reports what happened. `flow.rs` owns the
//! real-time state machine and translates these results into banners and
//! phase transitions; this module owns the *rules of baseball* and the
//! *balance constants* that make the arcade game fair.

use bevy::math::Vec3;
use bevy::prelude::Resource;

use crate::game::Team;

mod advance;
mod contact;
mod count;
mod pitch;
mod predict;
mod resolve;
mod steal;
#[cfg(test)]
mod test_support;

pub use advance::*;
pub use contact::*;
pub use count::*;
pub use pitch::*;
pub use predict::*;
pub use resolve::*;
pub use steal::*;

// ── Field geometry constants ──────────────────────────────────────────────────
// Pure geometry, hoisted here (rather than read from `present::field`) so
// `core` never reaches upward into a higher layer for a plain number — see
// docs/superpowers/specs/2026-08-19-layered-refactor-design.md, "Sanctioned
// layer back-references". `present::field` and `sim::ball` re-export these
// under their original names via `pub use` so no call site changed.

/// Distance between consecutive bases (90 ft). Kept private here — only
/// [`HALF_DIAGONAL`] needs it — `field::BASE_DISTANCE` is present's own copy
/// for spawning field geometry (not hoisted; nothing in `core` needs it
/// directly).
const BASE_DISTANCE_M: f32 = 27.43;
/// Home plate → pitching rubber (60.5 ft).
pub const PITCH_DISTANCE: f32 = 18.44;
/// Half the base-path diagonal, used to place second base along the Z axis.
pub const HALF_DIAGONAL: f32 = BASE_DISTANCE_M * std::f32::consts::SQRT_2 / 2.0;

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Gravity magnitude used for landing-point prediction (matches Rapier default).
pub const GRAVITY: f32 = 9.81;
/// Approximate batted-ball contact height (metres).
const CONTACT_HEIGHT: f32 = 0.6;

/// Nominal fastball speed (m/s) — roughly 85 mph.
pub const PITCH_SPEED: f32 = 38.0;

/// Home plate half-width (17 in across the front / 2 — docs/BASEBALL.md).
/// The single source of truth: `field.rs` builds the plate slab and the
/// drawn zone from it, and the called zone below adds the ball allowance.
pub const PLATE_HALF_WIDTH_M: f32 = 0.216;
/// Official ball radius — the canonical definition; `ball::BALL_RADIUS` is a
/// `pub use` shim pointing back here (the values were already identical, so
/// Task 15 collapsed the former duplicate into this one const).
pub const BALL_RADIUS_M: f32 = 0.037;
/// Horizontal half-width of the *called* strike zone (metres from plate
/// centre): the plate plus the rulebook's "any part of the ball" allowance
/// (Official Baseball Rules, STRIKE (b) — docs/BASEBALL.md "Strike zone").
/// The drawn zone is the bare plate width; a pitch grazing the drawn frame
/// is still a strike by exactly its own radius, as in real life. Public so
/// the field can draw the zone the umpire actually calls.
pub const ZONE_HALF_WIDTH: f32 = PLATE_HALF_WIDTH_M + BALL_RADIUS_M;
/// Rig landmarks measured off the authored skeleton (tools/build_player.py,
/// 1 unit = 1 m, feet at 0): the spine bone's shoulder line (its tail, and
/// the torso block's top edge), the top of the hip block — where the
/// uniform pants start — and the knee joint (UpperLeg tail / LowerLeg head).
const RIG_SHOULDER_TOP_M: f32 = 1.50;
const RIG_PANTS_TOP_M: f32 = 1.05;
const RIG_KNEE_M: f32 = 0.50;
/// Zone floor: just below the rig's kneecap (docs/BASEBALL.md "Strike
/// zone" — "the hollow beneath the kneecap").
pub const ZONE_LOW: f32 = RIG_KNEE_M - 0.05;
/// Zone ceiling: the rulebook midpoint between the top of the shoulders
/// and the top of the uniform pants, both read off the rig itself rather
/// than generic human proportions (docs/BASEBALL.md).
pub const ZONE_HIGH: f32 = (RIG_SHOULDER_TOP_M + RIG_PANTS_TOP_M) / 2.0;

/// A caught fly at least this far out (scaled by [`FieldSpec::hit_scale`])
/// gives runners time to tag up and advance.
const TAG_UP_MIN_DIST: f32 = 65.0;

// ── Live-play race constants ──────────────────────────────────────────────────
// The outcome of a ball in play is decided *during* the play by kinematic
// races between the live simulation and these speeds — never at contact.

// These are now read live off `Ruleset.pace` (see [`crate::game::variant::PaceTuning`])
// by every function below — the values here only remain as `PaceTuning::default()`'s
// source of truth, so `pub(crate)` (not `pub`) is enough.
/// Base-runner sprint speed (m/s) — shared with the runner rigs so the
/// animation and the umpire agree.
pub(crate) const RUNNER_SPEED: f32 = 7.5;
/// Fielder sprint speed — matches the fielding choreography's chase speed.
pub(crate) const FIELDER_SPEED: f32 = 7.0;
/// First-step reaction delay for fielders and runners alike.
pub(crate) const REACTION: f32 = 0.35;
/// Throw flight speed and glove-to-hand transfer time.
pub(crate) const THROW_FLIGHT_SPEED: f32 = 27.0;
pub(crate) const THROW_TRANSFER: f32 = 0.5;
/// A relay (catch-and-rethrow at a bag) turns faster than a gather.
pub(crate) const RELAY_TRANSFER: f32 = 0.3;
/// Head start a hit-and-run jump gives every forced runner (they broke with
/// the windup, not at contact).
pub(crate) const HIT_AND_RUN_JUMP: f32 = 1.6;
/// Extra grace a sent batter gets stretching for the next base — the throw
/// is usually going somewhere else, so the race is softer than the walk.
pub(crate) const STRETCH_GRACE: f32 = 0.9;
/// Bang-bang margin: ties and near-ties go to the runner.
pub(crate) const RUNNER_MARGIN: f32 = 0.35;
/// Gathers beyond this radius (scaled by hit_scale) concede first base — the
/// out at first is only contested on infield balls.
const INFIELD_GATHER_RADIUS: f32 = 30.0;
/// A catch closer to home than this (scaled) is an infield pop.
const POP_RADIUS: f32 = 30.0;

/// Where the ball rests before each pitch (top of the mound / rubber).
pub fn mound_reset_pos(pitch_distance: f32) -> Vec3 {
    Vec3::new(0.0, BALL_RADIUS_M + 0.25, pitch_distance)
}

// ── Data ──────────────────────────────────────────────────────────────────────

/// Occupancy of the bases, in running order (index 0 = first base). All
/// runners belong to the batting team, so a boolean per base is enough. The
/// base count comes from the field variant. Used by base-running rules and
/// the HUD diamond.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct Bases {
    occupied: Vec<bool>,
}

impl Default for Bases {
    /// A standard three-base diamond.
    fn default() -> Self {
        Bases::new(3)
    }
}

impl Bases {
    /// Empty bases for a field with `count` bases (excluding home).
    pub fn new(count: usize) -> Self {
        Self {
            occupied: vec![false; count],
        }
    }

    /// Number of bases on this field.
    pub fn count(&self) -> usize {
        self.occupied.len()
    }

    /// Is the (0-indexed) base occupied? Out-of-range reads are just empty.
    pub fn is_occupied(&self, base: usize) -> bool {
        self.occupied.get(base).copied().unwrap_or(false)
    }

    /// Sets one base's occupancy. Out-of-range writes are ignored.
    pub fn set(&mut self, base: usize, occupied: bool) {
        if let Some(slot) = self.occupied.get_mut(base) {
            *slot = occupied;
        }
    }

    /// Empties every base, keeping the base count.
    pub fn clear(&mut self) {
        self.occupied.fill(false);
    }

    /// Empties the bases *and* adopts a (possibly different) base count.
    pub fn reset_for(&mut self, count: usize) {
        self.occupied.clear();
        self.occupied.resize(count, false);
    }
}

/// Batters per lineup (regulation nine).
pub const LINEUP_SIZE: u32 = 9;

/// Each team's place in its batting order. The order itself is implicit
/// (slots 1..=9 rotate); what the rules require is that it always rotates —
/// every completed plate appearance brings up the next batter.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct BattingOrder {
    home: u32,
    away: u32,
}

impl BattingOrder {
    /// 1-based lineup slot of the batter currently due up for `team`.
    pub fn current(&self, team: Team) -> u32 {
        let slot = match team {
            Team::Home => self.home,
            Team::Away => self.away,
        };
        slot + 1
    }

    /// The plate appearance ended; the next batter steps in.
    pub fn advance(&mut self, team: Team) {
        let slot = match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        };
        *slot = (*slot + 1) % LINEUP_SIZE;
    }

    /// Debug/scenario seam: force `team`'s current (1-based) lineup slot.
    pub fn set_current(&mut self, team: Team, slot: u32) {
        let v = slot.saturating_sub(1) % LINEUP_SIZE;
        match team {
            Team::Home => self.home = v,
            Team::Away => self.away = v,
        }
    }
}

/// Flavour of an out. `Fly::deep` also drives the tag-up rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKind {
    Ground,
    Fly {
        /// Deep enough for runners to tag up (see [`TAG_UP_MIN_DIST`]).
        deep: bool,
    },
    Pop,
    /// A pop-up caught in foul territory.
    FoulPop,
    /// The runner was hit with the thrown ball (front-yard rules).
    Pegged,
    /// The batter was cut down trying for one base too many; the other
    /// runners keep the `advanced` bases they'd earned.
    Stretching {
        advanced: u32,
    },
}

/// The result of a batted ball.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum Outcome {
    Foul,
    Out(OutKind),
    /// The force-and-relay two outs: the forced runner and the batter.
    DoublePlay,
    /// The force got the runner at `out_base` but the batter beat the relay:
    /// one out, batter on first.
    FieldersChoice {
        out_base: usize,
    },
    /// A clean hit worth this many bases (1 = single … up to the base count).
    Hit(u32),
    HomeRun,
}

/// The batting side's call on a live ball, read at resolution: send the
/// batter for the extra base, hold him a base early, or let the analytic
/// walk decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RunnerCall {
    #[default]
    Neutral,
    Send,
    Hold,
}

/// What a taken ball did to the count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum BallCall {
    /// The count advanced.
    Ball,
    /// Ball four — the batter walked, forcing in `runs` runs.
    Walk { runs: u32 },
}

/// What a strike did to the count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub enum StrikeCall {
    /// The count advanced.
    Strike,
    /// Strike three — the batter is out.
    Strikeout,
    /// Strike three got away from the catcher and the batter beat the play
    /// to first: no out, fresh count for the next batter.
    DroppedThird,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batting_order_rotates_nine_and_wraps() {
        let mut order = BattingOrder::default();
        assert_eq!(order.current(Team::Home), 1);
        for _ in 0..8 {
            order.advance(Team::Home);
        }
        assert_eq!(order.current(Team::Home), 9);
        order.advance(Team::Home);
        assert_eq!(order.current(Team::Home), 1);
        // Teams rotate independently.
        assert_eq!(order.current(Team::Away), 1);
    }
}
