//! Unit tests for [`super`] — the trail module.

use super::*;

#[test]
fn fade_step_walks_the_ladder_monotonically() {
    assert_eq!(fade_step(0.0, TRAIL_FADE_STEPS), 0);
    assert_eq!(fade_step(0.999, TRAIL_FADE_STEPS), TRAIL_FADE_STEPS - 1);
    // Out-of-range ages clamp instead of indexing off the ladder.
    assert_eq!(fade_step(1.5, TRAIL_FADE_STEPS), TRAIL_FADE_STEPS - 1);
    let mut prev = 0;
    for i in 0..=20 {
        let s = fade_step(i as f32 / 20.0, TRAIL_FADE_STEPS);
        assert!(s >= prev && s < TRAIL_FADE_STEPS);
        prev = s;
    }
}

#[test]
fn trail_drops_by_distance_not_frame_rate() {
    assert!(
        should_drop(None, Vec3::ZERO, 0.5),
        "first mote drops immediately"
    );
    let last = Some(Vec3::ZERO);
    assert!(!should_drop(last, Vec3::new(0.0, 0.0, -0.3), 0.5));
    assert!(should_drop(last, Vec3::new(0.0, 0.0, -0.6), 0.5));
}

#[test]
fn every_style_has_positive_spacing_and_lifetime() {
    for style in [
        PitchTrailStyle::Comet,
        PitchTrailStyle::Fireball,
        PitchTrailStyle::Frostbite,
        PitchTrailStyle::NeonRings,
        PitchTrailStyle::Stardust,
        PitchTrailStyle::Bubbles,
    ] {
        assert!(trail_spacing(style) > 0.0);
        assert!(trail_lifetime(style) > 0.0);
    }
}
