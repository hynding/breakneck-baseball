//! Mojo — each player's confidence, rising when he succeeds and falling when
//! he fails, so a hot hitter or a dealing pitcher reads (and plays) like one.
//!
//! Pure bookkeeping and arithmetic, like [`crate::game::roster`]: no RNG and
//! no systems (`flow` records plate appearances into it). A batter's mojo is
//! keyed by team and lineup index, a pitcher's by team (the pitcher holds
//! lineup index 0, but his pitching confidence is separate from his
//! batting). Every score starts at 0 each game, lives in −1..=1, and decays
//! toward 0 a little with every plate appearance, so a streak fades unless
//! it keeps going.
//!
//! The one effect is in the duel: the batter's mojo against the pitcher's
//! scales the swing's timing error ([`Mojo::timing_scale`]) before it is
//! graded — a confident hitter facing a rattled pitcher squares up a ball
//! he would have just missed, and the other way round. It applies to human
//! and CPU swings alike. The deltas are sized so an average hitter's
//! successes and failures balance (≈30% reach base), keeping the league
//! economy where the balance sim tuned it.

use bevy::prelude::Resource;

use crate::game::Team;
use crate::game::rules::LINEUP_SIZE;

/// How a plate appearance ended, as mojo sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaResult {
    /// The batter reached base (hit, walk, hit by pitch) without an out
    /// being made on the play.
    pub reached: bool,
    /// Runs the play scored.
    pub runs: u32,
}

/// Each PA keeps this much of the score before adding the new result.
pub const MOJO_RETAIN: f32 = 0.9;
/// A batter reaching base.
pub const MOJO_REACHED: f32 = 0.25;
/// Each run the play drove in, on top (at most [`MOJO_MAX_RUNS`] counted).
pub const MOJO_PER_RUN: f32 = 0.1;
const MOJO_MAX_RUNS: u32 = 4;
/// A batter making an out. With ~30% of PAs reaching base, 0.3 × 0.25 is
/// about 0.7 × 0.11 — an average hitter drifts nowhere.
pub const MOJO_OUT: f32 = -0.11;
/// The pitcher moves the opposite way at this fraction of the batter's
/// swing — he faces every batter, so each PA counts for less.
pub const MOJO_PITCHER_SHARE: f32 = 0.5;
/// The swing's timing error shrinks by up to this fraction at the biggest
/// edge (batter fully confident, pitcher fully rattled) and grows by it at
/// the other end.
pub const MOJO_TIMING_EDGE: f32 = 0.12;

/// The five steps the HUD names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MojoLevel {
    IceCold,
    Cold,
    Even,
    Hot,
    OnFire,
}

impl MojoLevel {
    pub fn of(score: f32) -> Self {
        match score {
            s if s >= 0.5 => MojoLevel::OnFire,
            s if s >= 0.2 => MojoLevel::Hot,
            s if s > -0.2 => MojoLevel::Even,
            s if s > -0.5 => MojoLevel::Cold,
            _ => MojoLevel::IceCold,
        }
    }

    /// Plain ASCII for the bundled font.
    pub fn label(self) -> &'static str {
        match self {
            MojoLevel::IceCold => "ICE COLD",
            MojoLevel::Cold => "COLD",
            MojoLevel::Even => "EVEN",
            MojoLevel::Hot => "HOT",
            MojoLevel::OnFire => "ON FIRE",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TeamMojo {
    batters: [f32; LINEUP_SIZE as usize],
    pitcher: f32,
}

impl Default for TeamMojo {
    fn default() -> Self {
        Self {
            batters: [0.0; LINEUP_SIZE as usize],
            pitcher: 0.0,
        }
    }
}

/// Both teams' mojo, reset when a game starts.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct Mojo {
    home: TeamMojo,
    away: TeamMojo,
}

impl Mojo {
    fn team(&self, team: Team) -> &TeamMojo {
        match team {
            Team::Home => &self.home,
            Team::Away => &self.away,
        }
    }

    fn team_mut(&mut self, team: Team) -> &mut TeamMojo {
        match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        }
    }

    /// The batting mojo of `team`'s 1-based lineup `slot` (the value
    /// [`crate::game::rules::BattingOrder::current`] reports).
    pub fn batter(&self, team: Team, slot: u32) -> f32 {
        self.team(team).batters[slot_index(slot)]
    }

    /// `team`'s pitcher's mojo.
    pub fn pitcher(&self, team: Team) -> f32 {
        self.team(team).pitcher
    }

    /// Records a finished plate appearance: `batting` team's lineup `slot`
    /// against the `batting.other()` pitcher.
    pub fn record(&mut self, batting: Team, slot: u32, result: PaResult) {
        let delta = batter_delta(result);
        let b = &mut self.team_mut(batting).batters[slot_index(slot)];
        *b = step(*b, delta);
        let p = &mut self.team_mut(batting.other()).pitcher;
        *p = step(*p, -delta * MOJO_PITCHER_SHARE);
    }

    /// The factor the swing's timing error is multiplied by for `batting`
    /// team's lineup `slot` against the other team's pitcher: below 1 when
    /// the batter has the edge, above 1 when the pitcher does.
    pub fn timing_scale(&self, batting: Team, slot: u32) -> f32 {
        let edge = (self.batter(batting, slot) - self.pitcher(batting.other())).clamp(-1.0, 1.0);
        1.0 - MOJO_TIMING_EDGE * edge
    }
}

fn slot_index(slot: u32) -> usize {
    (slot.max(1) - 1).min(LINEUP_SIZE - 1) as usize
}

fn batter_delta(result: PaResult) -> f32 {
    if result.reached || result.runs > 0 {
        let base = if result.reached { MOJO_REACHED } else { 0.0 };
        base + MOJO_PER_RUN * result.runs.min(MOJO_MAX_RUNS) as f32
    } else {
        MOJO_OUT
    }
}

fn step(score: f32, delta: f32) -> f32 {
    (score * MOJO_RETAIN + delta).clamp(-1.0, 1.0)
}

#[cfg(test)]
#[path = "mojo.test.rs"]
mod tests;
