//! Pure camera framing math: no ECS, no resources — just the geometry the
//! rigs in [`super::rigs`] and the duel-view picker in [`super`] read.

use bevy::prelude::*;

use crate::game::flow::{Phase, Play};

use super::DUEL_REFERENCE_ASPECT;

// ── Framing math ──────────────────────────────────────────────────────────────

/// The duel-phase vertical FOV to actually apply for a camera whose viewport
/// has the given `aspect` (width / height), so the *horizontal* field of view
/// never shrinks below what `target_vfov` gives at the 16:9 reference the
/// duel framing was tuned at. `PerspectiveProjection::fov` is vertical, so a
/// narrower-than-16:9 viewport (a portrait-ish window, or a narrow wasm
/// canvas under `fit_canvas_to_parent`) crops horizontally at a fixed
/// vertical FOV — exactly what put the batter at risk of clipping out of
/// frame in the tight catcher-POV shot. Converts `target_vfov` to the
/// horizontal FOV it gives at the 16:9 reference, then re-derives the
/// vertical FOV that reproduces *that* horizontal FOV at the real `aspect`;
/// identity at 16:9, wider (more vertical coverage) below it, and left at
/// `target_vfov` above it (ultrawide already has FOV to spare, so it's left
/// untouched rather than narrowed).
pub fn aspect_safe_duel_vfov(target_vfov: f32, aspect: f32) -> f32 {
    if aspect >= DUEL_REFERENCE_ASPECT {
        return target_vfov;
    }
    let target_hfov = 2.0 * ((target_vfov / 2.0).tan() * DUEL_REFERENCE_ASPECT).atan();
    2.0 * ((target_hfov / 2.0).tan() / aspect).atan()
}

/// Signed vertical NDC coordinate (−1 = bottom edge, +1 = top edge) of world
/// point `p` as seen by a look-at camera at `eye` toward `target` with
/// vertical FOV `vfov`. Pure — the framing tests use it to prove the duel
/// shot really contains the batter, instead of eyeballing screenshots.
pub fn framed_ndc_y(eye: Vec3, target: Vec3, vfov: f32, p: Vec3) -> f32 {
    let fwd = (target - eye).normalize();
    let right = fwd.cross(Vec3::Y).normalize();
    let up = right.cross(fwd);
    let v = p - eye;
    let depth = v.dot(fwd).max(f32::EPSILON);
    (v.dot(up) / depth) / (vfov / 2.0).tan()
}

/// Fraction of the viewport height the segment `bottom`→`top` spans through
/// the same camera.
pub fn framed_height_fraction(eye: Vec3, target: Vec3, vfov: f32, bottom: Vec3, top: Vec3) -> f32 {
    ((framed_ndc_y(eye, target, vfov, top) - framed_ndc_y(eye, target, vfov, bottom)) / 2.0).abs()
}

/// Home-run trot orbit: during the result pause of a home run the broadcast
/// rig sweeps around the diamond instead of holding the static wide plate, so
/// the trot is shot from a moving camera. Distance/height of the orbiting eye
/// and the radians-per-second it sweeps.
const TROT_ORBIT_DIST: f32 = 26.0;
const TROT_ORBIT_HEIGHT: f32 = 11.0;
pub(super) const TROT_ORBIT_RATE: f32 = 0.7;

/// The broadcast eye for the home-run trot orbit: a point on a circle of
/// radius [`TROT_ORBIT_DIST`] at height [`TROT_ORBIT_HEIGHT`] around `focus`,
/// swept to `azimuth` radians. Same sin/cos parameterization as
/// [`super::rigs::orbit_transform`], so the trot shot reuses the free
/// camera's orbit math.
pub(super) fn trot_orbit_eye(focus: Vec3, azimuth: f32) -> Vec3 {
    focus
        + Vec3::new(
            TROT_ORBIT_DIST * azimuth.sin(),
            TROT_ORBIT_HEIGHT,
            TROT_ORBIT_DIST * azimuth.cos(),
        )
}

// ── Occlusion ─────────────────────────────────────────────────────────────────

/// How close to the eye (metres, measured along the eye→target axis) a
/// subject must be to count as blocking the shot. Small on purpose: this is
/// a body brushing the lens, not a general raycast, which is why views whose
/// eye sits far from the catcher/umpire (behind-pitcher, broadcast plate)
/// never trigger it even though those two are technically "in between" eye
/// and target in the literal geometric sense.
pub(super) const OCCLUSION_NEAR: f32 = 4.0;

/// How far off the eye→target axis (metres) a subject may sit and still
/// count as blocking the shot.
pub(super) const OCCLUSION_RADIUS: f32 = 1.6;

/// Pure predicate: does `subject` sit close enough to `eye`, and close
/// enough to the `eye`→`target` sightline, to block the shot? `near` caps
/// how far down the axis (from the eye) counts as "in the way"; `radius`
/// caps how far off the axis. A subject behind the eye (negative distance
/// along the axis) never occludes.
pub fn occludes(eye: Vec3, target: Vec3, subject: Vec3, near: f32, radius: f32) -> bool {
    let axis = target - eye;
    let axis_len = axis.length();
    if axis_len < f32::EPSILON {
        return false;
    }
    let axis_dir = axis / axis_len;
    let to_subject = subject - eye;
    let along = to_subject.dot(axis_dir);
    if along <= 0.0 || along > near.min(axis_len) {
        return false;
    }
    let perp = to_subject - axis_dir * along;
    perp.length() <= radius
}

/// The phases during which the broadcast rig wants (or is still holding)
/// the tight duel framing: the duel itself, the post-contact plate hold,
/// and the result pause of a pitch the catcher gloved — a called strike or
/// ball doesn't deserve a zoom-out; only balls the mitt missed (hits, dirt
/// balls, dropped thirds, HBP) release the camera. Shared between
/// [`super::rigs::broadcast_camera`]'s framing choice and
/// [`super::rigs::hide_occluders`]'s catcher-POV arm so the catcher can
/// never pop into a lens that is still parked inside his silhouette.
pub(super) fn duel_framing_wanted(play: &Play, now: f32) -> bool {
    match play.phase {
        Phase::PrePitch | Phase::WindUp | Phase::Pitch => true,
        Phase::InPlay => play.since_contact(now) < super::BALL_FOLLOW_DELAY,
        Phase::Result => play.pitch_gloved() && !play.is_home_run(),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "framing.test.rs"]
mod tests;
