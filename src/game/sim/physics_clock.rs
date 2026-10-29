//! Keeps Rapier's clock in step with the frame clock.
//!
//! bevy_rapier's default `TimestepMode::Variable` advances physics by
//! `min(delta, 1/60 s)` per frame. Below 60 fps that cap throws away part of
//! every frame — while ball drag/Magnus (`ball.rs`), rig locomotion and every
//! phase timer advance by the full `Time` delta. At 30 fps the ball covered
//! half the ground it should in real time but still bled a full frame of
//! drag: pitches reached the plate late, hits fell short, runners beat
//! throws. The headless suites step at 240 Hz or 100 Hz, where the cap never
//! bites (`tests/e2e/frame_rate.rs` is the one that runs at 30 fps).
//!
//! Instead, every frame steps physics by the whole (virtual) delta, split
//! into enough substeps that none is longer than [`MAX_SUBSTEP_SECS`] — so a
//! slow frame keeps 60 fps-sized integration steps, and a fast one still
//! takes a single step exactly as before.

use bevy::prelude::*;
use bevy_rapier3d::plugin::{PhysicsSet, TimestepMode};

/// Longest single Rapier step. Slightly over 1/60 s so ordinary vsync jitter
/// around 60 fps (16.4–17.0 ms frames) stays one step per frame.
pub const MAX_SUBSTEP_SECS: f32 = 1.0 / 50.0;

/// Substep ceiling for one frame. Bevy already clamps a `Time<Virtual>`
/// delta to its `max_delta` (250 ms); this bounds the work a hitch (or a
/// debug speed-up) can ask of one frame. Past it substeps simply lengthen —
/// physics still covers the whole delta.
pub const MAX_SUBSTEPS: usize = 16;

/// The timestep that advances physics by exactly `delta` seconds.
pub fn timestep_for(delta: f32) -> TimestepMode {
    let substeps = ((delta / MAX_SUBSTEP_SECS).ceil() as usize).clamp(1, MAX_SUBSTEPS);
    TimestepMode::Variable {
        // `delta` itself is the cap: the step is never truncated.
        max_dt: f32::INFINITY,
        time_scale: 1.0,
        substeps,
    }
}

/// Rewrites [`TimestepMode`] for this frame before Rapier steps.
pub struct PhysicsClockPlugin;

impl Plugin for PhysicsClockPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(timestep_for(0.0))
            .add_systems(PostUpdate, sync_timestep.before(PhysicsSet::SyncBackend));
    }
}

fn sync_timestep(time: Res<Time>, mut mode: ResMut<TimestepMode>) {
    // `Time` here is `Time<Virtual>`, the clock `StepSimulation` reads, so
    // hit-stop/slow-mo (`juice`) and pause carry through unchanged.
    mode.set_if_neq(timestep_for(time.delta_secs()));
}

#[cfg(test)]
#[path = "physics_clock.test.rs"]
mod tests;
