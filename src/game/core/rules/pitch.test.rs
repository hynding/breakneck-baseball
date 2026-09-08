//! Unit tests for [`super`] — the pitch module.

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
