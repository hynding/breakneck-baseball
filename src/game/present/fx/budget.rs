//! The per-tier particle allowance every effect spawns through: each burst
//! is authored at its High count, scaled by the tier's `particle_scale`,
//! and clipped so the motes alive at once never pass the tier's
//! `particle_cap` (`quality::RenderBudget`).

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::game::quality::{ActiveQuality, QualityTier, RenderBudget};

/// Effect particles alive right now: counted up as effects spawn them
/// (through [`FxBudget::allow`]) and down as `tick_particles` retires them.
/// Reset each game, when `cleanup_gameplay` has despawned the last game's.
#[derive(Resource, Default, Debug)]
pub struct LiveParticles(pub usize);

/// How many motes a burst authored at `authored` gets at `scale` with
/// `live` already up against `cap`: at least one when the burst is wanted
/// at all and there is room, never more than the room left.
pub(super) fn allowance(authored: usize, scale: f32, live: usize, cap: usize) -> usize {
    if authored == 0 {
        return 0;
    }
    let want = ((authored as f32 * scale).round() as usize).max(1);
    want.min(cap.saturating_sub(live))
}

/// The budget a spawning effect reads: the active tier's allowance plus
/// the live count it charges.
#[derive(SystemParam)]
pub(super) struct FxBudget<'w> {
    active: Option<Res<'w, ActiveQuality>>,
    live: Option<ResMut<'w, LiveParticles>>,
}

impl FxBudget<'_> {
    /// The tier's budget (High's when no tier has resolved, e.g. a harness).
    pub fn budget(&self) -> RenderBudget {
        self.active
            .as_ref()
            .map_or(QualityTier::High.budget(), |a| a.budget())
    }

    /// Claims room for a burst authored at `authored` motes and returns how
    /// many to spawn.
    pub fn allow(&mut self, authored: usize) -> usize {
        let budget = self.budget();
        let Some(live) = self.live.as_mut() else {
            return allowance(authored, budget.particle_scale, 0, budget.particle_cap);
        };
        let n = allowance(authored, budget.particle_scale, live.0, budget.particle_cap);
        live.0 += n;
        n
    }

    /// Claims room for `n` motes the caller has already scaled to the tier
    /// (a steady stream such as rain splashes), clipped only by the cap.
    pub fn allow_unscaled(&mut self, n: usize) -> usize {
        let cap = self.budget().particle_cap;
        let Some(live) = self.live.as_mut() else {
            return n.min(cap);
        };
        let n = n.min(cap.saturating_sub(live.0));
        live.0 += n;
        n
    }
}

#[cfg(test)]
#[path = "budget.test.rs"]
mod tests;
