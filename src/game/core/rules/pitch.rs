//! Pitch kinematics: arsenal selection, ballistic release, the strike zone,
//! and hit-by-pitch.

use bevy::math::{Vec2, Vec3};

use crate::game::ScoreBoard;

use super::{
    Bases, GRAVITY, PITCH_SPEED, ZONE_HALF_WIDTH, ZONE_HIGH, ZONE_LOW, advance_walk,
    mound_reset_pos, reset_count,
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
    let target_x = -aim.x * 0.6;
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
    let spray = (-aim.x * 0.6 + timing * 0.05).clamp(-0.95, 0.95);

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
/// `player.rs`).
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
mod tests {
    use super::super::test_support::*;
    use super::super::{BALL_RADIUS_M, PLATE_HALF_WIDTH_M};
    use super::*;
    use crate::game::ball::BALL_DRAG_FACTOR;

    /// Simulates a full pitch flight with the same gravity + drag + Magnus
    /// model the live ball uses (`ball::apply_drag` / `ball::apply_magnus`),
    /// returning the plate-crossing point. Locks the balance constants to
    /// observable behaviour: if a model change makes centre pitches become
    /// balls, these fail instead of the gameplay quietly degrading.
    fn simulate_pitch(kind: PitchKind, aim: Vec2) -> Vec2 {
        let pitch_distance = std_field().pitch_distance;
        let mut pos = mound_reset_pos(pitch_distance);
        let mut vel = pitch_velocity_kind(kind, aim, pitch_distance, 1.0);
        let spin = kind.spin();
        let dt = 1.0 / 240.0;

        while pos.z > 0.0 {
            let speed = vel.length();
            vel += -BALL_DRAG_FACTOR * speed * vel * dt;
            vel += crate::game::ball::MAGNUS_FACTOR * spin.cross(vel) * dt;
            vel.y -= GRAVITY * dt;
            pos += vel * dt;
            assert!(pos.y > 0.0, "pitch hit the ground before the plate");
        }
        Vec2::new(pos.x, pos.y)
    }

    /// The called zone follows the MLB rulebook (docs/BASEBALL.md, "Strike
    /// zone"): plate width plus the any-part-of-the-ball allowance each
    /// side, knee hollow to the stance midpoint for the 1.85 m rig.
    #[test]
    fn zone_is_plate_width_plus_ball_allowance() {
        assert!((ZONE_HALF_WIDTH - (PLATE_HALF_WIDTH_M + BALL_RADIUS_M)).abs() < 1e-6);
        // Just below the rig's kneecap, and the rulebook shoulders/pants
        // midpoint read off the rig skeleton (0.45 and 1.275 for the
        // authored 1.85 m rig — see the consts' derivation).
        assert!((ZONE_LOW - 0.45).abs() < 1e-6);
        assert!((ZONE_HIGH - 1.275).abs() < 1e-6);
        // `ball::BALL_RADIUS` is a `pub use` shim back to this const (Task
        // 15 collapsed the former duplicate) — this pins that it still
        // resolves to the same value if that ever changes.
        assert!((BALL_RADIUS_M - crate::game::ball::BALL_RADIUS).abs() < 1e-9);
    }

    /// Neutral aim throws to the middle of the *current* zone — the aim map
    /// may never drift off the zone the umpire calls. `pitch_velocity_kind`
    /// is a gravity-only solve, so the check is exact (spin/drag bend is the
    /// kinds' character on top, covered by the flight sims below).
    #[test]
    fn neutral_aim_targets_zone_middle() {
        let kind = PitchKind::Changeup;
        let v = pitch_velocity_kind(kind, Vec2::ZERO, 18.44, 1.0);
        let flight = 18.44 / kind.speed();
        let start = mound_reset_pos(18.44);
        let y_at_plate = start.y + v.y * flight - 0.5 * GRAVITY * flight * flight;
        assert!(
            (y_at_plate - (ZONE_LOW + ZONE_HIGH) / 2.0).abs() < 0.02,
            "neutral aim crosses at y {y_at_plate}, zone middle is {}",
            (ZONE_LOW + ZONE_HIGH) / 2.0
        );
        assert!(v.x.abs() < 0.05);
    }

    #[test]
    fn every_kind_centre_aimed_is_a_strike() {
        for kind in [
            PitchKind::Fastball,
            PitchKind::Curveball,
            PitchKind::Changeup,
            PitchKind::Slider,
            PitchKind::Sinker,
        ] {
            let cross = simulate_pitch(kind, Vec2::ZERO);
            assert!(
                is_in_zone(cross),
                "{kind:?} crossed at ({:.2}, {:.2}) — outside the zone",
                cross.x,
                cross.y
            );
        }
    }

    #[test]
    fn backspin_rides_and_topspin_dives() {
        let fast = simulate_pitch(PitchKind::Fastball, Vec2::ZERO);
        let curve = simulate_pitch(PitchKind::Curveball, Vec2::ZERO);
        assert!(
            fast.y > curve.y + 0.15,
            "fastball {fast:?} vs curveball {curve:?}"
        );
    }

    #[test]
    fn aim_maps_to_kinds_per_spec() {
        assert_eq!(
            PitchKind::from_aim(Vec2::new(0.0, 1.0)),
            PitchKind::Fastball
        );
        assert_eq!(
            PitchKind::from_aim(Vec2::new(0.0, -1.0)),
            PitchKind::Curveball
        );
        assert_eq!(PitchKind::from_aim(Vec2::ZERO), PitchKind::Changeup);
        assert_eq!(PitchKind::from_aim(Vec2::new(-1.0, 0.0)), PitchKind::Slider);
        assert_eq!(PitchKind::from_aim(Vec2::new(1.0, 0.0)), PitchKind::Sinker);
        // The dominant axis wins a diagonal.
        assert_eq!(
            PitchKind::from_aim(Vec2::new(0.4, 0.9)),
            PitchKind::Fastball
        );
        assert_eq!(PitchKind::from_aim(Vec2::new(-0.9, 0.4)), PitchKind::Slider);
    }

    #[test]
    fn slider_sweeps_in_and_sinker_runs_away() {
        let neutral = simulate_pitch(PitchKind::Changeup, Vec2::ZERO);
        let slider = simulate_pitch(PitchKind::Slider, Vec2::ZERO);
        let sinker = simulate_pitch(PitchKind::Sinker, Vec2::ZERO);
        // The batter stands at +X: the slider breaks toward him, the sinker
        // runs away, and the sinker also finishes below the slider.
        assert!(
            slider.x > neutral.x + 0.08,
            "slider {slider:?} vs {neutral:?}"
        );
        assert!(
            sinker.x < neutral.x - 0.08,
            "sinker {sinker:?} vs {neutral:?}"
        );
    }

    #[test]
    fn full_inside_fastball_plunks_the_batter() {
        // Max inside aim (stick-left: the batter's box is on the +X /
        // screen-left side) crosses inside the batter's body window.
        let cross = simulate_pitch(PitchKind::Fastball, Vec2::new(-1.0, 0.0));
        assert!(
            hits_batter(cross),
            "crossing ({:.2}, {:.2}) should hit the batter",
            cross.x,
            cross.y
        );
        assert!(!is_in_zone(cross));
    }

    #[test]
    fn batter_window_boundaries() {
        assert!(hits_batter(Vec2::new(0.6, 1.0)));
        assert!(!hits_batter(Vec2::new(0.4, 1.0))); // inside pitch, no contact
        assert!(!hits_batter(Vec2::new(-0.6, 1.0))); // away side — no batter there
        assert!(!hits_batter(Vec2::new(0.6, 2.2))); // sails over his head
    }

    #[test]
    fn hit_by_pitch_forces_like_a_walk() {
        let mut score = ScoreBoard {
            balls: 1,
            strikes: 2,
            top_of_inning: true,
            ..Default::default()
        };
        let mut bases = loaded();
        assert_eq!(hit_by_pitch(&mut score, &mut bases), 1);
        assert_eq!(score.away_runs, 1);
        assert_eq!((score.balls, score.strikes), (0, 0));
        assert_eq!(bases, loaded());
    }

    #[test]
    fn hit_spin_pulls_toward_the_spray_side() {
        let pulled = hit_spin(Vec3::new(10.0, 8.0, 20.0));
        let oppo = hit_spin(Vec3::new(-10.0, 8.0, 20.0));
        assert!(pulled.y * oppo.y < 0.0, "sidespin should flip with spray");
    }
}
