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
    // Uneven frames, a hitch included: the clip's own clock decides.
    let clock = [0.0, 0.016, 0.033, 0.05, 0.08, 0.15, 0.166, 0.2, 0.3];
    let fired = clock.windows(2).filter(|w| crossed(at, w[0], w[1])).count();
    assert_eq!(fired, 1);
    assert!(
        crossed(at, -1.0, 0.2),
        "a clip first seen past contact fires"
    );
    assert!(!crossed(at, 0.1, 0.2), "already past");
}

#[test]
fn chalk_puffs_only_on_the_lines() {
    for variant in [VariantId::Standard, VariantId::FrontYard] {
        let field = variant.field();
        let spans = crate::game::field::foul_line_spans(&field);
        assert_eq!(spans.len(), 2, "{variant:?}");
        for (a, b) in spans {
            let along = (b - a).normalize();
            let outward = Vec3::new(along.z, 0.0, -along.x) * along.x.signum();
            assert!(outward.x * along.x > 0.0, "outward points away from centre");
            let chalk = a + along * 20.0;
            assert!(on_the_chalk(chalk, &field), "{variant:?}: on the line");
            assert!(
                !on_the_chalk(chalk + outward * 0.5, &field),
                "{variant:?}: foul"
            );
            assert!(
                !on_the_chalk(chalk - outward * 0.5, &field),
                "{variant:?}: fair"
            );
            assert!(
                !on_the_chalk(-along * 10.0, &field),
                "{variant:?}: behind home"
            );
        }
        assert!(
            !on_the_chalk(Vec3::new(0.0, 0.0, 40.0), &field),
            "{variant:?}: centre"
        );
    }
}

#[test]
fn only_a_hard_fastball_pops_the_mitt() {
    let mph = GLOVE_POP_MPS / crate::game::rules::MPS_PER_MPH;
    assert!((85.0..95.0).contains(&mph), "{mph}");
}
