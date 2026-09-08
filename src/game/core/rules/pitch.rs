//! Pitch kinematics: arsenal selection, ballistic release, the strike zone,
//! and hit-by-pitch.

use bevy::math::{Vec2, Vec3};

use crate::game::ScoreBoard;

use super::{
    Bases, GRAVITY, PITCH_SPEED, ZONE_HALF_WIDTH, ZONE_HIGH, ZONE_LOW, advance_walk,
    aim_to_world_x, mound_reset_pos, reset_count,
};

// ── Pitch & contact kinematics ────────────────────────────────────────────────

/// The pitcher's arsenal. Speeds in m/s; spin in rad/s about world axes for a
/// −Z pitch: +X is backspin (Magnus lift), −X topspin (dive), ±Y sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchKind {
    Fastball,
    Curveball,
    Changeup,
    /// Hard breaking ball that sweeps toward the batter's side (+X).
    Slider,
    /// Two-seamer that dives and runs away from the batter (−X).
    Sinker,
}

impl PitchKind {
    pub fn speed(self) -> f32 {
        match self {
            PitchKind::Fastball => PITCH_SPEED,
            PitchKind::Curveball => 31.0,
            PitchKind::Changeup => 29.0,
            PitchKind::Slider => 33.0,
            PitchKind::Sinker => 35.0,
        }
    }

    pub fn spin(self) -> Vec3 {
        match self {
            PitchKind::Fastball => Vec3::new(20.0, 0.0, 0.0),
            PitchKind::Curveball => Vec3::new(-18.0, 6.0, 0.0),
            PitchKind::Changeup => Vec3::new(6.0, 0.0, 0.0),
            // −Y spin accelerates a −Z pitch toward +X (the batter's side);
            // +Y sweeps it away toward −X. Mild ±X components add ride/dive.
            PitchKind::Slider => Vec3::new(-4.0, -14.0, 0.0),
            PitchKind::Sinker => Vec3::new(-10.0, 10.0, 0.0),
        }
    }

    /// Held aim at release selects the pitch by its dominant axis: up =
    /// fastball, down = curveball, left = slider, right = sinker, neutral =
    /// changeup. Aim keeps steering location too — aiming high *means*
    /// throwing the heater upstairs, and aiming inside means the sweeper in.
    pub fn from_aim(aim: Vec2) -> PitchKind {
        if aim.x.abs() > 0.35 && aim.x.abs() >= aim.y.abs() {
            if aim.x < 0.0 {
                PitchKind::Slider
            } else {
                PitchKind::Sinker
            }
        } else if aim.y > 0.35 {
            PitchKind::Fastball
        } else if aim.y < -0.35 {
            PitchKind::Curveball
        } else {
            PitchKind::Changeup
        }
    }

    /// The aim whose [`PitchKind::from_aim`] decode is exactly this pitch —
    /// the scenario library's forced-pitch seam.
    pub fn canonical_aim(self) -> Vec2 {
        match self {
            PitchKind::Fastball => Vec2::new(0.0, 0.6),
            PitchKind::Curveball => Vec2::new(0.0, -0.6),
            PitchKind::Slider => Vec2::new(-0.6, 0.0),
            PitchKind::Sinker => Vec2::new(0.6, 0.0),
            PitchKind::Changeup => Vec2::ZERO,
        }
    }
}

/// Full-deflection pitch aim in meters of lateral plate target. Named (like
/// its spray sibling below) so an aim-authority retune edits a dial, not a
/// bare literal — the two are coincidentally equal, NOT one convention: this
/// is meters at the plate, [`SPRAY_AIM_FRAC`] is a sine-of-spray fraction.
const PITCH_AIM_X_M: f32 = 0.6;
/// Full-deflection hit-spray pull, as a fraction of the sprayable arc.
const SPRAY_AIM_FRAC: f32 = 0.6;

/// Solves the ballistic release velocity for a pitch of `kind` from
/// `pitch_distance` aimed at plate location `(aim.x, aim.y)` (both in
/// −1.0..=1.0, zero = middle of the zone). Deliberately gravity-only: the
/// kind's spin then bends the flight (fastballs ride, curveballs dive), so a
/// pitch's character *is* its physics. `pitch_speed_scale`
/// (`PaceTuning::pitch_speed_scale`) scales `kind.speed()` at release — the
/// one dial that speeds up or slows down every pitch in the arsenal — before
/// the ballistic solve, so the scaled flight time keeps the aim accurate at
/// any scale, not just 1.0.
pub fn pitch_velocity_kind(
    kind: PitchKind,
    aim: Vec2,
    pitch_distance: f32,
    pitch_speed_scale: f32,
) -> Vec3 {
    // Wide enough that a full-inside aim reaches the batter's body — painting
    // the inside corner risks a hit-by-pitch. Negated: stick-right means
    // screen-right, which the behind-home camera renders as world −X.
    let target_x = aim_to_world_x(aim.x) * PITCH_AIM_X_M;
    // Centred on the *current* zone's middle (so "zero = middle of the
    // zone" stays true whatever the rulebook heights are); ±0.45 spans the
    // zone edge to just outside it — full-up still paints above the
    // letters, full-down still bounces the curve in the dirt.
    let target_y = (ZONE_LOW + ZONE_HIGH) / 2.0 + aim.y * 0.45;
    let speed = kind.speed() * pitch_speed_scale;

    let start = mound_reset_pos(pitch_distance);
    let flight = pitch_distance / speed;
    let vx = (target_x - start.x) / flight;
    let vy = (target_y - start.y) / flight + 0.5 * GRAVITY * flight;

    Vec3::new(vx, vy, -speed)
}

/// Spin imparted by the bat: sidespin toward the spray side plus mild
/// backspin (−X lifts a +Z batted ball). Single source of truth — the live
/// ball and the landing predictor both use it.
pub fn hit_spin(vel: Vec3) -> Vec3 {
    Vec3::new(-6.0, vel.x.signum() * vel.length() * 0.25, 0.0)
}

/// Converts contact timing + aim into a batted-ball velocity.
///
/// Timing is everything: `contact_z ≈ 0.4` (ball on the plate) is squared-up
/// for a hard line drive, while early contact (ball still out front) skies the
/// ball for a pop-up and late contact tops it for a weak grounder. A tight
/// window means mistimed swings produce catchable balls, keeping the out-rate
/// and inning pace in line with arcade baseball.
pub fn hit_velocity(contact_z: f32, aim: Vec2) -> Vec3 {
    let ideal = 0.4_f32;
    let timing = contact_z - ideal; // >0 early, <0 late
    let quality = (1.0 - timing.abs() / 1.1).clamp(0.08, 1.0);

    let speed = 16.0 + 30.0 * quality;
    // Aim sets the intended launch; mistiming skews it toward pop-up / grounder.
    // A neutral swing (aim.y = 0) is a ~19° line drive — the base hit angle;
    // aiming up trades hittability for home-run power. Spray is negated so
    // stick-right pulls toward screen-right (world −X).
    let launch_deg = (6.0 + 26.0 * (aim.y * 0.5 + 0.5) + timing * 8.0).clamp(-8.0, 72.0);
    let launch = launch_deg.to_radians();
    let spray = (aim_to_world_x(aim.x) * SPRAY_AIM_FRAC + timing * 0.05).clamp(-0.95, 0.95);

    let horizontal = speed * launch.cos();
    Vec3::new(
        horizontal * spray.sin(),
        speed * launch.sin(),
        horizontal * spray.cos(),
    )
}

/// Is a plate-crossing point (x = horizontal, y = height) a called strike?
pub fn is_in_zone(crossing: Vec2) -> bool {
    crossing.x.abs() <= ZONE_HALF_WIDTH && crossing.y >= ZONE_LOW && crossing.y <= ZONE_HIGH
}

/// Inner edge of the batter's body window; he stands at x ≈ +0.7 (see
/// `game::player`).
const BATTER_X_MIN: f32 = 0.52;
/// Above this the pitch sails over the batter's head.
const BATTER_Y_MAX: f32 = 1.7;

/// Does a plate-crossing point plunk the batter? Only meaningful on a take —
/// swinging at the pitch negates a hit-by-pitch, as in the rulebook.
pub fn hits_batter(crossing: Vec2) -> bool {
    crossing.x >= BATTER_X_MIN && crossing.y > 0.0 && crossing.y <= BATTER_Y_MAX
}

/// Awards first base after a hit-by-pitch: dead ball, forced runners only.
/// Returns runs forced in.
#[must_use]
pub fn hit_by_pitch(score: &mut ScoreBoard, bases: &mut Bases) -> u32 {
    let runs = advance_walk(bases);
    score.add_runs(runs);
    reset_count(score);
    runs
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "pitch.test.rs"]
mod tests;
