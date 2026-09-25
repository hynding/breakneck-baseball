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
/// adds at spawn) really are cleared out of the default batting view —
/// the catcher by the look-ahead cone, the plate umpire either by the
/// cone, by the lens-brush rule (front yard: he stands at the eye), or
/// by standing behind the lens (Standard: a metre behind it) — and really
/// do sit outside the cone for `BehindPitcher`, for every variant. The
/// concrete regression the e2e test also drives through the real ECS.
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
                "{id:?}: the batting view should be blocked by the catcher"
            );
        }
        if let Some(umpire) = umpire {
            let hidden = occludes(bz_eye, bz_target, umpire, OCCLUSION_NEAR, OCCLUSION_RADIUS)
                || brushes_lens(bz_eye, umpire, LENS_BRUSH_RADIUS);
            let behind_lens = (umpire - bz_eye).dot(bz_target - bz_eye) < 0.0;
            assert!(
                hidden || behind_lens,
                "{id:?}: the plate umpire must be hidden or behind the batting view's lens"
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

/// The default batting view (TODO 100) must reproduce the reference
/// composition (docs/agent/SMB3-REFERENCE-NOTES.md §2.1) in both parks at
/// the 16:9 reference aspect: the whole batter in frame filling 75–90% of
/// the screen height on the screen-left side (his +x box renders left of
/// centre), the zone box centred horizontally and sitting in the lower-middle
/// of the frame, and the pitcher's release point above the zone — so the
/// ball grows toward the lens with the full bat arc visible beside it.
#[test]
fn batting_zoom_frames_the_batter_and_centres_the_zone() {
    use crate::game::player::{BATTER_STAND_X, RIG_HEIGHT_M};
    use crate::game::rules::{ZONE_HIGH, ZONE_LOW};
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let f = id.field();
        let (eye, target, vfov) = DuelView::BattingZoom.framing(&f, DUEL_REFERENCE_ASPECT);
        let feet = Vec3::new(BATTER_STAND_X, 0.0, 0.0);
        let head = feet + Vec3::Y * RIG_HEIGHT_M;
        let frac = framed_height_fraction(eye, target, vfov, feet, head);
        assert!(
            (0.75..=0.90).contains(&frac),
            "{id:?}: batter fills {frac:.3} of screen height, want 0.75..=0.90"
        );
        for p in [feet, head] {
            let y = framed_ndc_y(eye, target, vfov, p);
            assert!(
                y.abs() <= 0.97,
                "{id:?}: batter point {p} clipped at ndc y {y:.3}"
            );
        }
        // The batter's box (+x) must render off-centre so the zone is clear.
        let head_x = framed_ndc_x(eye, target, vfov, DUEL_REFERENCE_ASPECT, head);
        assert!(
            head_x < -0.15,
            "{id:?}: batter's head at ndc x {head_x:.3}, want screen-left"
        );

        let zone = Vec3::new(0.0, (ZONE_HIGH + ZONE_LOW) / 2.0, 0.0);
        let zx = framed_ndc_x(eye, target, vfov, DUEL_REFERENCE_ASPECT, zone);
        let zy = framed_ndc_y(eye, target, vfov, zone);
        assert!(
            zx.abs() <= 0.2,
            "{id:?}: zone centre at ndc x {zx:.3}, want centred"
        );
        assert!(
            (-0.4..=0.05).contains(&zy),
            "{id:?}: zone centre at ndc y {zy:.3}, want the lower-middle of the frame"
        );

        let release = Vec3::new(0.0, 1.8, f.pitch_distance);
        let ry = framed_ndc_y(eye, target, vfov, release);
        assert!(
            ry > zy + 0.2,
            "{id:?}: release point (ndc y {ry:.3}) must sit above the zone ({zy:.3})"
        );
    }
}

/// The lens-brush rule hides a body the eye is parked against even when it
/// stands beside or behind the eye (invisible to the look-ahead cone), and
/// leaves alone anyone a stride further off — the front-yard plate umpire
/// (z=-2.2) under the default batting eye (z=-2.0) versus Standard's umpire
/// a metre behind it (z=-3.0).
#[test]
fn lens_brush_hides_only_a_body_at_the_eye() {
    let eye = Vec3::new(0.2, 1.25, -2.0);
    let front_yard_ump = Vec3::new(0.0, 0.0, -2.2);
    let standard_ump = Vec3::new(0.0, 0.0, -3.0);
    assert!(brushes_lens(eye, front_yard_ump, LENS_BRUSH_RADIUS));
    assert!(!brushes_lens(eye, standard_ump, LENS_BRUSH_RADIUS));
    // Height never matters: the roots sit at the feet.
    assert!(brushes_lens(
        eye,
        front_yard_ump + Vec3::Y * 5.0,
        LENS_BRUSH_RADIUS
    ));
}
