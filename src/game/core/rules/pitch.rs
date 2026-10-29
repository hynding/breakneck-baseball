//! Pitch kinematics: arsenal selection, ballistic release, the strike zone,
//! and hit-by-pitch.

use bevy::math::{Vec2, Vec3};

use crate::game::ScoreBoard;
use crate::game::variant::BattingTuning;

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

/// Exit speed (m/s) of a dead-on swing before the quality multiplier — the
/// top of the contact continuum. With `exit_perfect` 1.28 on top this is a
/// ~100 mph barrel; with `exit_solid` 0.95 at the Solid edge it is a ~60 mph
/// blooper. `tests/balance_sim.rs` arbitrates this number, not feel.
const EXIT_PEAK: f32 = 31.0;
/// How much of [`EXIT_PEAK`] a swing at the very edge of the foul window has
/// lost (0.36 → a 130 ms-late swing still leaves the bat at 64% of peak).
/// Linear in the timing error so Solid contact is a real *continuum* from
/// blooper to liner instead of one fixed dribbler: the old model graded
/// power by the ball's distance from a fixed point in *metres*, which at
/// pitch speed bottomed out inside ~30 ms — every Solid swing left the bat
/// at the same 39 mph.
const EXIT_FALLOFF: f32 = 0.36;
/// Degrees of launch tilt at one Solid-window of timing error: an early
/// swing gets under the ball (pop-up), a late one tops it (grounder). The
/// tilt grows with the *square* of the error, so Perfect contact keeps its
/// aimed launch while Solid-edge contact is pushed to the extremes — the
/// shape of real mishits (Statcast: soft contact is pop-ups and grounders,
/// not 60 mph bloopers at a base-hit angle, which is where a linear tilt
/// parked every Solid swing and ran up a .450 BABIP in the 2026-10 study).
const TIMING_LAUNCH_DEG: f32 = 30.0;

/// Converts swing timing + aim into a batted-ball velocity.
///
/// Timing is everything: `dt_ms` is the signed swing error at the bat
/// (`flow::swing_dt_ms`, early = negative). Dead-on is the hardest contact;
/// exit speed falls linearly with |dt| across the foul window
/// ([`EXIT_PEAK`], [`EXIT_FALLOFF`]), and the launch tilts up when early and
/// down when late ([`TIMING_LAUNCH_DEG`]) — a mistimed swing still produces a
/// catchable ball, keeping the out rate and inning pace in line with arcade
/// baseball. Graded in milliseconds, never metres, so the feel is the same
/// off a changeup and a heater. The pull-side yaw of mistimed contact is
/// [`apply_contact_quality`]'s, not this function's.
pub fn hit_velocity(dt_ms: f32, aim: Vec2, batting: &BattingTuning) -> Vec3 {
    let miss = (dt_ms.abs() / batting.foul_ms.max(f32::EPSILON)).clamp(0.0, 1.0);
    let speed = EXIT_PEAK * (1.0 - EXIT_FALLOFF * miss);
    // Early (negative) timing tilts the launch up, late tilts it down, by
    // up to 1.5 Solid-windows' worth so the foul-window fringe keeps tilting.
    let err = (dt_ms / batting.solid_ms.max(f32::EPSILON)).clamp(-1.5, 1.5);
    let tilt = -err.signum() * err * err * TIMING_LAUNCH_DEG;
    // Aim sets the intended launch: stick-down (aim.y = −1) is a −4° chopper,
    // neutral a 14° line drive — the base hit angle — and stick-up a 32° fly;
    // aiming up trades hittability for home-run power. Spray is negated so stick-right pulls toward
    // screen-right (world −X).
    let launch_deg = (-4.0 + 36.0 * (aim.y * 0.5 + 0.5) + tilt).clamp(-8.0, 72.0);
    let launch = launch_deg.to_radians();
    let spray = (aim_to_world_x(aim.x) * SPRAY_AIM_FRAC).clamp(-0.95, 0.95);

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
