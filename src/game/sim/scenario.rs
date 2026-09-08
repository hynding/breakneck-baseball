//! Scenario library — instantly reachable game situations, shared verbatim
//! by the in-game debug panel (`debug.rs`) and the headless e2e tests. A
//! scenario only writes the authoritative resources; runner rigs, HUD, and
//! jerseys all re-mirror through their existing change detection.

use bevy::prelude::*;

use crate::game::ScoreBoard;
use crate::game::flow::Play;
use crate::game::rules::{Bases, BattingOrder, PitchKind};
use crate::game::variant::{FieldSpec, Ruleset};

pub const PRESET_LOADED: &str = "Bases loaded, 2 out, full count";

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub bases: Vec<bool>,
    pub outs: u32,
    pub balls: u32,
    pub strikes: u32,
    pub inning: u32,
    pub top: bool,
    pub score: (u32, u32),
    pub batter_slot: Option<u32>,
    pub next_cpu_pitch: Option<PitchKind>,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            name: "Custom",
            bases: vec![false; 3],
            outs: 0,
            balls: 0,
            strikes: 0,
            inning: 1,
            top: true,
            score: (0, 0),
            batter_slot: None,
            next_cpu_pitch: None,
        }
    }
}

/// Forces the CPU's next pitch selection (consumed on use). Scenario data,
/// so it lives here in the lib target; human pitchers are never overridden.
#[derive(Resource, Default)]
pub struct PitchOverride(pub Option<PitchKind>);

/// Fired once `apply_to_world` finishes rewriting the world. No system
/// consumes it today — by design, not an oversight: `ScoreBoard`, `Bases`,
/// and `Play` are all `Resource`s, so runner rigs, HUD, and jerseys already
/// re-mirror them through their own change detection without needing to
/// react to this event directly. It's kept as the documented seam for a
/// future consumer that wants to react to a scenario jump specifically
/// (e.g. a debug-panel toast, or an e2e test asserting a jump happened)
/// rather than to the resource changes it causes.
#[derive(Event)]
pub struct ScenarioAppliedEvent {
    pub name: &'static str,
}

pub fn presets() -> Vec<Scenario> {
    vec![
        Scenario {
            name: PRESET_LOADED,
            bases: vec![true, true, true],
            outs: 2,
            balls: 3,
            strikes: 2,
            ..Default::default()
        },
        Scenario {
            name: "DP setup: R1, 0 out",
            bases: vec![true, false, false],
            ..Default::default()
        },
        Scenario {
            name: "Steal duel: R1",
            bases: vec![true, false, false],
            outs: 1,
            ..Default::default()
        },
        Scenario {
            name: "Tag-up: R3, 1 out",
            bases: vec![false, false, true],
            outs: 1,
            ..Default::default()
        },
        Scenario {
            name: "Dropped-third: 2 strikes",
            strikes: 2,
            ..Default::default()
        },
        Scenario {
            name: "Walk-off: bottom 9, down 1, R2",
            bases: vec![false, true, false],
            inning: 9,
            top: false,
            score: (3, 4),
            outs: 2,
            ..Default::default()
        },
    ]
}

/// Rewrites the live game to `s`. Refused (Err) while the ball is live —
/// the same deadness gate pausing uses.
pub fn apply_to_world(world: &mut World, s: &Scenario) -> Result<(), &'static str> {
    if !world.resource::<Play>().scenario_safe() {
        return Err("scenario refused: ball is live");
    }
    let base_count = world.resource::<FieldSpec>().base_count();
    {
        let mut score = world.resource_mut::<ScoreBoard>();
        score.home_runs = s.score.0;
        score.away_runs = s.score.1;
        score.inning = s.inning;
        score.top_of_inning = s.top;
        score.balls = s.balls;
        score.strikes = s.strikes;
        score.outs = s.outs;
    }
    {
        let mut bases = world.resource_mut::<Bases>();
        bases.reset_for(base_count);
        for (i, &occ) in s.bases.iter().enumerate() {
            bases.set(i, occ);
        }
    }
    if let Some(slot) = s.batter_slot {
        let team = world.resource::<ScoreBoard>().batting_team();
        world.resource_mut::<BattingOrder>().set_current(team, slot);
    }
    world.resource_scope(|world, mut play: Mut<Play>| {
        play.reset_for_scenario(world.resource::<Bases>(), world.resource::<Ruleset>());
    });
    world.resource_mut::<PitchOverride>().0 = s.next_cpu_pitch;
    world.send_event(ScenarioAppliedEvent { name: s.name });
    Ok(())
}

#[cfg(test)]
#[path = "scenario.test.rs"]
mod tests;
