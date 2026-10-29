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

/// Fraction of the horizontal launch speed a ground ball keeps once it is
/// down and skipping (the first bounces eat the rest), and the per-second
/// decay of that speed as it rolls — the analytic twin of the live ball's
/// restitution + rolling resistance (`ball::apply_drag`). Calibrated against
/// the 2026-10 realism harness: too fast and infielders give up on balls
/// they would field; too slow and they wait at a spot the ball never reaches.
const GROUND_SPEED_RETAIN: f32 = 0.6;
const GROUND_SPEED_DECAY: f32 = 0.9;
/// A fielder may arrive this much after the ball and still cut it off — he
/// is ranging to a spot on its path, not diving at a point in time.
const GROUND_WAIT_MARGIN: f32 = 0.35;
/// How far ahead the roll is projected when looking for a cut-off.
const GROUND_HORIZON_SECS: f32 = 6.0;

/// Where the infield cuts off a ground ball: a point on the ball's roll path
/// (straight on from its first bounce `landing`, along the `launch`
/// direction, slowing as it goes) that some fielder reaches no later than
/// the ball, chosen as early on the path as possible so the throw is short.
/// Returns that fielder (index into `fielders`) and the spot; `None` with
/// the roll's end when nobody can cut it off and the ball will be run down.
///
/// Before this, a grounder's chaser was whoever could reach its *first
/// bounce* soonest — three metres in front of the plate, i.e. the catcher
/// or pitcher — and every other infielder went to cover a bag, so a 60 mph
/// grounder skipped untouched through the infield for a double.
pub fn ground_intercept(
    fielders: &[Vec3],
    landing: Vec3,
    launch: Vec3,
    pace: &PaceTuning,
) -> (Option<usize>, Vec3) {
    let hvel = Vec2::new(launch.x, launch.z) * GROUND_SPEED_RETAIN;
    roll_intercept(fielders, landing, hvel, pace)
}

/// [`ground_intercept`] for a ball already on the ground: the chaser
/// re-plans every frame from the live position and horizontal velocity, so
/// a hot grounder is met on its path rather than chased from behind (a
/// 40 m/s grounder outruns a 7 m/s fielder until it has rolled past the
/// outfield grass — the 2026-10 harness measured a .60 BABIP on grounders
/// off the chase-the-ball targeting). Returns the cut-off spot, or `None`
/// when this fielder cannot get in front of it before it stops.
pub fn ground_intercept_from(
    fielder: Vec3,
    pos: Vec3,
    vel: Vec3,
    pace: &PaceTuning,
) -> Option<Vec3> {
    let hvel = Vec2::new(vel.x, vel.z);
    let (who, spot) = roll_intercept(&[fielder], pos, hvel, pace);
    who.map(|_| spot)
}

fn roll_intercept(
    fielders: &[Vec3],
    start: Vec3,
    hvel: Vec2,
    pace: &PaceTuning,
) -> (Option<usize>, Vec3) {
    let dir = hvel.normalize_or_zero();
    let v0 = hvel.length();
    let along = |t: f32| v0 * (1.0 - (-GROUND_SPEED_DECAY * t).exp()) / GROUND_SPEED_DECAY;
    let point = |t: f32| {
        let s = along(t);
        Vec3::new(start.x + dir.x * s, 0.0, start.z + dir.y * s)
    };
    let mut t = 0.0;
    while t <= GROUND_HORIZON_SECS {
        let p = point(t);
        if let Some((i, _)) = fielders
            .iter()
            .enumerate()
            .map(|(i, f)| (i, catch_time(*f, p, pace)))
            .filter(|(_, ft)| *ft <= t + GROUND_WAIT_MARGIN)
            .min_by(|a, b| a.1.total_cmp(&b.1))
        {
            return (Some(i), p);
        }
        t += 0.1;
    }
    (None, point(GROUND_HORIZON_SECS))
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
