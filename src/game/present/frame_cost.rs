//! The frame-cost probe: with `BREAKNECK_PERF=1` natively or `?perf` on the
//! web (`0` or `false` turns it off), logs one `bb-perf` line every
//! [`PROBE_WINDOW_SECS`] of real time with the tier in force, the mean and
//! worst frame, the entity count and the meshes visible to any view (the
//! main camera's or a shadow cascade's: a draw-cost proxy, not a draw count). `tools/frame_cost.mjs` reads
//! those lines to compare tiers and to show what each presentation PR costs
//! (task 1 of the graphics and camera plan); nothing else reads them. Off,
//! it costs one early return per frame.

use bevy::ecs::entity::Entities;
use bevy::prelude::*;

use crate::game::quality::{ActiveQuality, launch_param};

/// Seconds of real time per `bb-perf` line.
pub const PROBE_WINDOW_SECS: f32 = 5.0;

/// The probe's running window.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct FrameCostProbe {
    /// Off unless the launch asked for it.
    pub enabled: bool,
    window: FrameWindow,
}

/// Frame times accumulated over one window.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct FrameWindow {
    pub elapsed: f32,
    pub frames: u32,
    pub worst: f32,
}

/// One finished window, as logged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameSummary {
    pub frames: u32,
    pub mean_ms: f32,
    pub worst_ms: f32,
}

impl FrameWindow {
    /// Adds one frame of `dt` seconds; returns the summary and starts a new
    /// window once [`PROBE_WINDOW_SECS`] have passed.
    pub fn observe(&mut self, dt: f32) -> Option<FrameSummary> {
        self.elapsed += dt;
        self.frames += 1;
        self.worst = self.worst.max(dt);
        if self.elapsed < PROBE_WINDOW_SECS {
            return None;
        }
        let summary = FrameSummary {
            frames: self.frames,
            mean_ms: self.elapsed / self.frames as f32 * 1000.0,
            worst_ms: self.worst * 1000.0,
        };
        *self = FrameWindow::default();
        Some(summary)
    }
}

pub struct FrameCostPlugin;

impl Plugin for FrameCostPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(FrameCostProbe {
            enabled: launch_param("BREAKNECK_PERF", "perf")
                .is_some_and(|v| !matches!(v.as_str(), "0" | "false")),
            ..default()
        })
        .add_systems(Last, probe_frame_cost);
    }
}

fn probe_frame_cost(
    time: Res<Time<Real>>,
    mut probe: ResMut<FrameCostProbe>,
    active: Option<Res<ActiveQuality>>,
    entities: &Entities,
    meshes: Query<&ViewVisibility, With<Mesh3d>>,
) {
    if !probe.enabled {
        return;
    }
    let Some(summary) = probe.window.observe(time.delta_secs()) else {
        return;
    };
    let tier = active.map_or("?", |a| a.tier.label());
    let visible = meshes.iter().filter(|v| v.get()).count();
    info!(
        "bb-perf tier={tier} frames={} mean_ms={:.2} worst_ms={:.2} entities={} meshes_visible={visible}",
        summary.frames,
        summary.mean_ms,
        summary.worst_ms,
        entities.len(),
    );
}

#[cfg(test)]
#[path = "frame_cost.test.rs"]
mod tests;
