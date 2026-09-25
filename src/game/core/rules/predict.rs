//! Fair/foul geometry, fielder catch-time reads, the outfield fence, and
//! numeric flight prediction for a batted ball.

use bevy::math::{Vec2, Vec3};

use crate::game::variant::{FieldSpec, PaceTuning};

use super::{CONTACT_HEIGHT, GRAVITY};

/// Is a ground position in fair territory (the wedge opening toward +Z)?
pub fn is_fair(pos: Vec3, field: &FieldSpec) -> bool {
    pos.z > 1.0 && pos.x.abs() <= pos.z * field.fair_half_angle.tan() + 0.01
}

/// Radial fence distance in the direction of `pos`, interpolated from the
/// foul lines to straightaway centre. The single source of truth for where
/// the wall stands: home-run classification, the spawned wall geometry, and
/// the fielders' don't-run-through-the-wall caps all read it.
pub fn fence_at(pos: Vec3, field: &FieldSpec) -> f32 {
    let dist = Vec2::new(pos.x, pos.z).length();
    let cos_half = field.fair_half_angle.cos();
    let centeredness = (((pos.z / dist.max(0.001)) - cos_half) / (1.0 - cos_half)).clamp(0.0, 1.0);
    field.fence_line + (field.fence_center - field.fence_line) * centeredness
}

/// Time for a fielder at `from` to reach `landing`, first step included.
pub fn catch_time(from: Vec3, landing: Vec3, pace: &PaceTuning) -> f32 {
    pace.reaction_secs
        + Vec2::new(landing.x - from.x, landing.z - from.z).length() / pace.fielder_speed
}

/// The fielder (index into `fielders`) best placed to catch a ball landing at
/// `landing` after `hang` seconds — `None` if nobody can make it.
pub fn best_catcher(
    fielders: &[Vec3],
    landing: Vec3,
    hang: f32,
    pace: &PaceTuning,
) -> Option<usize> {
    fielders
        .iter()
        .enumerate()
        .map(|(i, f)| (i, catch_time(*f, landing, pace)))
        .filter(|(_, t)| *t <= hang)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Numerically integrates a batted ball's flight from contact height with the
/// same gravity + drag + Magnus model the live ball uses (`ball::apply_drag`,
/// `ball::apply_magnus`), returning the landing point (y = 0) and hang time.
/// This is what fielder choreography chases — the *visual* ball's touchdown,
/// not the balance-tuned range in [`classify_batted_ball`].
pub fn predict_landing(vel: Vec3, spin: Vec3, drag_factor: f32, magnus_factor: f32) -> (Vec3, f32) {
    predict_landing_from(
        Vec3::new(0.0, CONTACT_HEIGHT, 0.0),
        vel,
        spin,
        drag_factor,
        magnus_factor,
    )
}

/// [`predict_landing`] from an arbitrary mid-flight state — what a chasing
/// fielder re-plans against every frame as the live ball bends.
pub fn predict_landing_from(
    start: Vec3,
    vel: Vec3,
    spin: Vec3,
    drag_factor: f32,
    magnus_factor: f32,
) -> (Vec3, f32) {
    let mut pos = start;
    let mut v = vel;
    let dt = 1.0 / 120.0;
    let mut t = 0.0;
    while pos.y > 0.0 && t < 15.0 {
        let speed = v.length();
        v += -drag_factor * speed * v * dt;
        v += magnus_factor * spin.cross(v) * dt;
        v.y -= GRAVITY * dt;
        pos += v * dt;
        t += dt;
    }
    (Vec3::new(pos.x, 0.0, pos.z), t)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "predict.test.rs"]
mod tests;
