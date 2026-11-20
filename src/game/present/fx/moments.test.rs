//! Unit tests for [`super`] — the moment effects.

use super::*;
use crate::game::variant::VariantId;

#[test]
fn slides_and_dives_spray_inside_their_clips() {
    for clip in [AnimClip::Slide, AnimClip::Dive] {
        let at = spray_at(clip).expect("sprays");
        assert!(at > 0.0 && at < clip.duration(), "{clip:?}");
    }
    assert_eq!(spray_at(AnimClip::RunCycle), None);
}

#[test]
fn a_spray_fires_once_as_the_clip_passes_contact() {
    let at = 0.09;
    let dt = 1.0 / 60.0;
    let fired = (0..60).filter(|&f| crossed(at, f as f32 * dt, dt)).count();
    assert_eq!(fired, 1);
    assert!(crossed(0.0, 0.0, dt), "a contact at the clip's start fires");
}

#[test]
fn chalk_puffs_only_on_the_lines() {
    let field = VariantId::Standard.field();
    for sign in [-1.0_f32, 1.0] {
        let theta = sign * field.fair_half_angle;
        let along = Vec3::new(theta.sin(), 0.0, theta.cos());
        let outward = Vec3::new(along.z, 0.0, -along.x) * sign;
        assert!(outward.x * sign > 0.0, "outward points away from centre");
        let chalk = along * 40.0 + outward * HALF_BAG_M;
        assert!(on_the_chalk(chalk, &field), "{sign}: on the line");
        assert!(!on_the_chalk(chalk + outward * 0.5, &field), "{sign}: foul");
        assert!(!on_the_chalk(chalk - outward * 0.5, &field), "{sign}: fair");
        assert!(!on_the_chalk(-along * 10.0, &field), "{sign}: behind home");
    }
    assert!(
        !on_the_chalk(Vec3::new(0.0, 0.0, 40.0), &field),
        "dead centre"
    );
}

#[test]
fn only_a_hard_fastball_pops_the_mitt() {
    let mph = GLOVE_POP_MPS / crate::game::rules::MPS_PER_MPH;
    assert!((85.0..95.0).contains(&mph), "{mph}");
}
