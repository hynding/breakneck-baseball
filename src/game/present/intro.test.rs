//! Unit tests for [`super`] — the game intro.

use super::*;
use crate::game::variant::VariantId;

/// The flyover opens high beyond centre field looking in at the plate and
/// settles low behind home looking out, coming down the whole way, and in
/// every park.
#[test]
fn the_flyover_swings_from_centre_field_to_behind_the_plate() {
    for id in [VariantId::Standard, VariantId::FrontYard] {
        let field = id.field();
        let (start, start_target, _) = flyover(0.0, &field, false);
        let (end, end_target, fov) = flyover(INTRO_SECS, &field, false);
        assert!(start.z > field.fence_line, "{id:?}: opens past the fence");
        assert!(start.y > 30.0, "{id:?}: opens high");
        assert!(start_target.z.abs() < 1.0, "{id:?}: looks in at the plate");
        assert!(end.z < -15.0 && end.y < 10.0, "{id:?}: settles behind home");
        assert!(end_target.z > 5.0, "{id:?}: looking out at the mound");
        assert!(fov > 0.5 && fov < 1.2);
        let mut prev = start.y;
        for i in 1..=50 {
            let (eye, _, _) = flyover(INTRO_SECS * i as f32 / 50.0, &field, false);
            assert!(eye.y <= prev + 1e-4, "{id:?}: climbs at step {i}");
            assert!(eye.x >= -1e-3, "{id:?}: swings round the first-base side");
            prev = eye.y;
        }
    }
}

/// Reduce motion holds the closing frame from the first instant.
#[test]
fn reduce_motion_holds_the_flyover_still() {
    let field = VariantId::Standard.field();
    let end = flyover(INTRO_SECS, &field, false);
    for t in [0.0, 1.0, 3.0, INTRO_SECS] {
        assert_eq!(flyover(t, &field, true), end, "{t} s");
    }
}

#[test]
fn the_lineup_card_comes_up_partway_through() {
    let mut intro = Intro::default();
    assert!(!intro.lineup_up());
    intro.active = Some(Run {
        shown: LINEUP_FROM - 0.1,
        camera: None,
    });
    assert!(intro.is_playing() && !intro.lineup_up());
    intro.active.as_mut().unwrap().shown = LINEUP_FROM;
    assert!(intro.lineup_up());
}
