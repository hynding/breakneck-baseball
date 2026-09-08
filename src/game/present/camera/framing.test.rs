//! Tests for [`super`] — see `framing.rs`.

use super::*;
use crate::game::camera::DuelView;
use crate::game::variant::VariantId;

use super::super::DUEL_FOV;

/// At the 16:9 reference aspect the duel FOV was tuned at, the correction
/// must be an identity (no crop was ever a problem here).
#[test]
fn aspect_safe_duel_vfov_is_identity_at_reference_aspect() {
    let vfov = aspect_safe_duel_vfov(DUEL_FOV, DUEL_REFERENCE_ASPECT);
    assert!(
        (vfov - DUEL_FOV).abs() < 1e-4,
        "16:9 should reproduce DUEL_FOV exactly, got {vfov}"
    );
}

/// A narrower-than-16:9 window (e.g. 4:3) must widen the vertical FOV so
/// the horizontal coverage doesn't shrink and crop the batter.
#[test]
fn aspect_safe_duel_vfov_widens_for_a_narrower_aspect() {
    let vfov = aspect_safe_duel_vfov(DUEL_FOV, 4.0 / 3.0);
    assert!(
        vfov > DUEL_FOV,
        "4:3 should widen the vertical FOV, got {vfov} vs DUEL_FOV {DUEL_FOV}"
    );
}

/// A wider-than-16:9 (ultrawide) window already has FOV to spare — the
/// duel FOV must be left untouched, not narrowed.
#[test]
fn aspect_safe_duel_vfov_unchanged_for_ultrawide() {
    let vfov = aspect_safe_duel_vfov(DUEL_FOV, 21.0 / 9.0);
    assert_eq!(vfov, DUEL_FOV);
}

/// The trot orbit eye stays on a fixed-radius, fixed-height circle around
/// the focus for every azimuth, and actually sweeps (distinct eyes at
/// distinct azimuths) — the "sweeping victory lap" the Result-phase branch
/// lerps toward.
#[test]
fn trot_orbit_eye_rides_a_fixed_circle_and_sweeps() {
    let focus = Vec3::new(2.0, 1.4, 9.0);
    let mut prev: Option<Vec3> = None;
    for step in 0..8 {
        let azim = step as f32 * std::f32::consts::FRAC_PI_4;
        let eye = trot_orbit_eye(focus, azim);
        // Fixed height above the focus.
        assert!((eye.y - (focus.y + TROT_ORBIT_HEIGHT)).abs() < 1e-4);
        // Fixed horizontal radius from the focus.
        let horiz = Vec2::new(eye.x - focus.x, eye.z - focus.z).length();
        assert!(
            (horiz - TROT_ORBIT_DIST).abs() < 1e-3,
            "azim {azim}: radius {horiz} != {TROT_ORBIT_DIST}"
        );
        if let Some(p) = prev {
            assert!(p.distance(eye) > 1e-3, "the orbit must actually move");
        }
        prev = Some(eye);
    }
}

/// The catcher-POV duel framing must show the batter's entire body —
/// spikes to helmet, on his side of the plate — filling 80–90% of the
/// screen height at the 16:9 reference aspect, fully inside the frame,
/// in both parks. The design ask behind the pulled-back duel eye.
#[test]
fn catcher_pov_frames_the_full_batter_at_80_to_90_percent() {
    use crate::game::player::{BATTER_STAND_X, RIG_HEIGHT_M};
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        let (eye, target, vfov) = DuelView::CatcherPov.framing(&f, DUEL_REFERENCE_ASPECT);
        let feet = Vec3::new(BATTER_STAND_X, 0.0, 0.0);
        let head = feet + Vec3::Y * RIG_HEIGHT_M;
        let frac = framed_height_fraction(eye, target, vfov, feet, head);
        assert!(
            (0.80..=0.90).contains(&frac),
            "{id:?}: batter fills {frac:.3} of screen height, want 0.80..=0.90"
        );
        for p in [feet, head] {
            let y = framed_ndc_y(eye, target, vfov, p);
            assert!(
                y.abs() <= 0.98,
                "{id:?}: batter point {p} clipped at ndc y {y:.3}"
            );
        }
    }
}

/// The result pause holds the duel framing only for a pitch the catcher
/// gloved (called strikes/balls, strikeouts into the mitt end tight on
/// the plate); everything the mitt missed — hits, dirt balls, dropped
/// thirds, HBP — releases the camera to the wide shot. The duel phases
/// always want the tight framing; the post-contact plate hold expires
/// with `BALL_FOLLOW_DELAY`.
#[test]
fn duel_framing_holds_result_only_for_gloved_pitches() {
    use crate::game::flow::Play;
    assert!(duel_framing_wanted(
        &Play::test_play(Phase::Result, true),
        10.0
    ));
    assert!(!duel_framing_wanted(
        &Play::test_play(Phase::Result, false),
        10.0
    ));
    assert!(duel_framing_wanted(
        &Play::test_play(Phase::PrePitch, false),
        10.0
    ));
    assert!(duel_framing_wanted(
        &Play::test_play(Phase::Pitch, false),
        10.0
    ));
}

#[test]
fn subject_behind_the_eye_never_occludes() {
    // Same axis as in front, but placed behind the eye (negative along).
    let eye = Vec3::new(0.0, 1.4, -0.9);
    let target = Vec3::new(0.0, 0.85, 15.0);
    let behind = Vec3::new(0.0, 0.6, -3.0);
    assert!(!occludes(
        eye,
        target,
        behind,
        OCCLUSION_NEAR,
        OCCLUSION_RADIUS
    ));
}

#[test]
fn subject_on_axis_within_near_and_radius_occludes() {
    let eye = Vec3::ZERO;
    let target = Vec3::new(0.0, 0.0, 10.0);
    // 2 m down the axis, dead centre: well inside both thresholds.
    let subject = Vec3::new(0.0, 0.0, 2.0);
    assert!(occludes(eye, target, subject, 4.0, 1.6));
}

#[test]
fn subject_beyond_the_near_threshold_does_not_occlude() {
    let eye = Vec3::ZERO;
    let target = Vec3::new(0.0, 0.0, 10.0);
    // On axis, but far past the near cutoff — this is the mechanism
    // that keeps the behind-pitcher/broadcast-plate eyes from ever
    // hiding the catcher, even though he's technically "between" eye
    // and target for those views too.
    let subject = Vec3::new(0.0, 0.0, 8.0);
    assert!(!occludes(eye, target, subject, 4.0, 1.6));
}

#[test]
fn subject_off_axis_beyond_radius_does_not_occlude() {
    let eye = Vec3::ZERO;
    let target = Vec3::new(0.0, 0.0, 10.0);
    // 2 m down the axis (within `near`) but 3 m off to the side.
    let subject = Vec3::new(3.0, 0.0, 2.0);
    assert!(!occludes(eye, target, subject, 4.0, 1.6));
}

#[test]
fn degenerate_axis_never_occludes() {
    let eye = Vec3::new(1.0, 1.0, 1.0);
    assert!(!occludes(eye, eye, eye, 4.0, 1.6));
}

/// The catcher/umpire spawn spots (`FieldSpec::fielder_positions` /
/// `umpire_positions`, offset by the same `Vec3::Y * 0.6` `game::player`
/// adds at spawn) really do sit inside the occlusion cone for
/// `BattingZoom` and really do sit outside it for `BehindPitcher`, for
/// every variant — the concrete regression the e2e test also drives
/// through the real ECS.
#[test]
fn per_variant_occlusion_matches_the_reference_shots() {
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        let catcher = f
            .fielder_positions
            .iter()
            .find(|p| p.z < 0.0)
            .map(|p| *p + Vec3::Y * 0.6);
        let umpire = f.umpire_positions.first().map(|p| *p + Vec3::Y * 0.6);

        let (bz_eye, bz_target, _) = DuelView::BattingZoom.framing(&f, DUEL_REFERENCE_ASPECT);
        if let Some(catcher) = catcher {
            assert!(
                occludes(bz_eye, bz_target, catcher, OCCLUSION_NEAR, OCCLUSION_RADIUS),
                "{id:?}: batting zoom should be blocked by the catcher"
            );
        }
        if let Some(umpire) = umpire {
            assert!(
                occludes(bz_eye, bz_target, umpire, OCCLUSION_NEAR, OCCLUSION_RADIUS),
                "{id:?}: batting zoom should be blocked by the plate umpire"
            );
        }

        let (bp_eye, bp_target, _) = DuelView::BehindPitcher.framing(&f, DUEL_REFERENCE_ASPECT);
        if let Some(catcher) = catcher {
            assert!(
                !occludes(bp_eye, bp_target, catcher, OCCLUSION_NEAR, OCCLUSION_RADIUS),
                "{id:?}: behind-pitcher must keep the catcher visible"
            );
        }
        if let Some(umpire) = umpire {
            assert!(
                !occludes(bp_eye, bp_target, umpire, OCCLUSION_NEAR, OCCLUSION_RADIUS),
                "{id:?}: behind-pitcher must keep the plate umpire visible"
            );
        }
    }
}

/// `duel_framing_wanted`'s first arm is the deliberate inline sibling
/// of [`Phase::pre_contact`] (kept an exhaustive match so a new phase
/// forces a framing decision at compile time). This pins the agreement
/// the hand-edit rule relies on: with the conditional InPlay/Result
/// arms forced false (contact long past, pitch not gloved), the
/// framing must want exactly the pre-contact phases — redefining the
/// shared window without updating the camera's copy fails here instead
/// of silently keeping the old framing.
#[test]
fn duel_framing_pre_contact_arm_matches_the_shared_predicate() {
    let mut play = Play::default();
    let long_after_contact = 1_000.0;
    for phase in [
        Phase::PrePitch,
        Phase::WindUp,
        Phase::Pitch,
        Phase::InPlay,
        Phase::Result,
    ] {
        play.phase = phase;
        assert_eq!(
            duel_framing_wanted(&play, long_after_contact),
            phase.pre_contact(),
            "camera duel arm disagrees with Phase::pre_contact for {phase:?}"
        );
    }
}
