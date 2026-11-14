//! Unit tests for [`super`] — the predict module.

use super::super::hit_spin;
use super::super::test_support::*;
use super::*;
use crate::game::ball::BALL_DRAG_FACTOR;

// ── Aimed-base selection ──────────────────────────────────────────────────

#[test]
fn fence_interpolates_line_to_center() {
    let f = std_field();
    // Straightaway centre field.
    assert!((fence_at(Vec3::new(0.0, 0.0, 100.0), &f) - f.fence_center).abs() < 0.01);
    // Down the line the fence sits at the line distance.
    let line = Vec3::new(100.0, 0.0, 100.0); // 45° = the foul line
    assert!((fence_at(line, &f) - f.fence_line).abs() < 0.01);
}

// ── Landing prediction ────────────────────────────────────────────────────

#[test]
fn dragless_landing_matches_closed_form() {
    let vel = vel_at(30.0, 30.0);
    let (land, t) = predict_landing(vel, Vec3::ZERO, 0.0, 0.0, Vec3::ZERO);
    let disc = vel.y * vel.y + 2.0 * GRAVITY * 0.6; // CONTACT_HEIGHT
    let t_expect = (vel.y + disc.sqrt()) / GRAVITY;
    assert!((t - t_expect).abs() < 0.05, "hang time {t} vs {t_expect}");
    let range_expect = Vec2::new(vel.x, vel.z).length() * t_expect;
    let range = Vec2::new(land.x, land.z).length();
    assert!(
        (range - range_expect).abs() < 1.5,
        "range {range} vs {range_expect}"
    );
}

#[test]
fn drag_shortens_flight() {
    let vel = vel_at(30.0, 40.0);
    let (with_drag, t_drag) = predict_landing(vel, Vec3::ZERO, BALL_DRAG_FACTOR, 0.0, Vec3::ZERO);
    let (no_drag, _) = predict_landing(vel, Vec3::ZERO, 0.0, 0.0, Vec3::ZERO);
    assert!(
        Vec2::new(with_drag.x, with_drag.z).length() < Vec2::new(no_drag.x, no_drag.z).length()
    );
    assert!(t_drag > 0.5);
}

#[test]
fn sidespin_bends_the_landing_point() {
    let vel = vel_at(25.0, 35.0);
    let (straight, _) = predict_landing(vel, Vec3::ZERO, BALL_DRAG_FACTOR, 0.0, Vec3::ZERO);
    let (bent, _) = predict_landing(
        vel,
        hit_spin(Vec3::new(10.0, 8.0, 20.0)),
        BALL_DRAG_FACTOR,
        crate::game::ball::MAGNUS_FACTOR,
        Vec3::ZERO,
    );
    assert!(
        (bent.x - straight.x).abs() > 0.5,
        "Magnus should bend the carry"
    );
}

#[test]
fn ground_intercept_picks_the_infielder_on_the_roll_path_not_the_battery() {
    let pace = PaceTuning::default();
    // Catcher behind the plate, pitcher on the mound, shortstop up the
    // middle at regulation depth.
    let catcher = Vec3::new(0.0, 0.0, -1.5);
    let pitcher = Vec3::new(0.0, 0.0, 18.0);
    let shortstop = Vec3::new(7.0, 0.0, 35.8);
    let fielders = [catcher, pitcher, shortstop];
    // A 60 mph grounder up the middle, bouncing 3 m in front of the plate.
    let launch = Vec3::new(2.0, -2.0, 26.0);
    let landing = Vec3::new(0.2, 0.0, 3.0);
    let (who, spot) = ground_intercept(&fielders, landing, launch, &pace);
    assert_ne!(
        who,
        Some(0),
        "the catcher never chases a grounder up the middle"
    );
    assert!(who.is_some(), "somebody on the path cuts it off");
    assert!(
        spot.z > landing.z,
        "the cut-off is down the path, past the bounce"
    );
    // Nobody anywhere near the path: it runs out, and the roll end is returned.
    let far = [Vec3::new(-60.0, 0.0, 90.0)];
    let (nobody, end) = ground_intercept(&far, landing, launch, &pace);
    assert_eq!(nobody, None);
    assert!(
        end.z > 15.0,
        "the roll end is well into the infield, got {end:?}"
    );
}

// ── Wind ──────────────────────────────────────────────────────────────────

/// A ~400 ft fly to straightaway centre (the live ball's own spin).
fn deep_fly(wind: Vec3) -> Vec3 {
    let v = Vec3::new(
        0.0,
        39.0 * 25f32.to_radians().sin(),
        39.0 * 25f32.to_radians().cos(),
    );
    predict_landing(v, hit_spin(v), 0.0003, 0.0028, wind).0
}

#[test]
fn a_tail_wind_carries_a_fly_about_three_feet_a_mph() {
    // Per docs/BASEBALL.md ("Wind"): ~3 ft of carry per mph blowing out,
    // a little less taken off by the same wind blowing in.
    let calm = deep_fly(Vec3::ZERO).z;
    assert!((110.0..135.0).contains(&calm), "calm fly {calm} m");
    let ten = 10.0 * MPS_PER_MPH;
    let out_ft = (deep_fly(Vec3::new(0.0, 0.0, ten)).z - calm) * 3.281;
    let in_ft = (calm - deep_fly(Vec3::new(0.0, 0.0, -ten)).z) * 3.281;
    assert!((25.0..40.0).contains(&out_ft), "out {out_ft} ft");
    assert!((20.0..out_ft).contains(&in_ft), "in {in_ft} ft");
    // A cross wind drifts it sideways, the way it blows.
    let drift = deep_fly(Vec3::new(ten, 0.0, 0.0)).x;
    assert!(drift > 2.0, "drift {drift} m");
}

#[test]
fn no_wind_means_no_push() {
    assert_eq!(
        wind_accel(Vec3::new(0.0, 10.0, 30.0), Vec3::ZERO),
        Vec3::ZERO
    );
    assert_eq!(wind_accel(Vec3::ZERO, Vec3::X), Vec3::ZERO);
}

#[test]
fn the_wind_report_reads_from_the_plate() {
    let mph = |m: f32| m * MPS_PER_MPH;
    assert_eq!(wind_report(Vec3::ZERO), "WIND CALM");
    assert_eq!(wind_report(Vec3::new(0.0, 0.0, mph(1.0))), "WIND CALM");
    assert_eq!(
        wind_report(Vec3::new(0.0, 0.0, mph(8.0))),
        "WIND 8 OUT TO CF"
    );
    // +X is left field (screen left from behind the plate).
    assert_eq!(
        wind_report(Vec3::new(mph(7.0), 0.0, mph(7.0))),
        "WIND 10 OUT TO LF"
    );
    assert_eq!(
        wind_report(Vec3::new(-mph(12.0), 0.0, 0.0)),
        "WIND 12 L TO R"
    );
    assert_eq!(
        wind_report(Vec3::new(0.0, 0.0, -mph(5.0))),
        "WIND 5 IN FROM CF"
    );
}
