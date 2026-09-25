//! Unit tests for [`super`] — the diamond module.

use super::*;
use crate::game::variant::VariantId;

/// Basic sanity check on `distance_point_to_segment`'s geometry,
/// independent of any field const: zero exactly on the segment, the
/// expected perpendicular distance off it (while still between the
/// endpoints), and clamped-to-endpoint distance for a point beyond it —
/// this is a *segment* distance, not an infinite line's.
#[test]
fn distance_point_to_segment_matches_geometry() {
    let a = Vec2::ZERO;
    let b = Vec2::new(10.0, 10.0);
    assert!(distance_point_to_segment(Vec2::new(5.0, 5.0), a, b) < 1e-5);
    let want = 5.0 / std::f32::consts::SQRT_2;
    assert!((distance_point_to_segment(Vec2::new(0.0, 5.0), a, b) - want).abs() < 1e-4);
    // Beyond `b`: the infinite line's perpendicular distance would still
    // be small, but the segment must clamp to `b` itself.
    let beyond = Vec2::new(20.0, 20.0);
    let want_clamped = beyond.distance(b);
    assert!((distance_point_to_segment(beyond, a, b) - want_clamped).abs() < 1e-4);
}

/// `on_box_outline` must flag a point on either the inner (plate-side) or
/// outer edge as "on" the outline, but reject both the box's own deep
/// interior and home plate itself — the chalk paints a hollow rectangle,
/// not a filled one, per `spawn_batters_box`.
#[test]
fn box_outline_flags_edges_not_interior_or_plate() {
    let center = Vec2::new(BOX_CENTER_X, 0.0);
    let half = Vec2::new(BOX_HALF_WIDTH, BOX_HALF_LENGTH);

    let inner_edge = Vec2::new(BOX_CENTER_X - BOX_HALF_WIDTH, 0.0);
    assert!(on_box_outline(inner_edge, center, half, CHALK_WIDTH));
    let outer_edge = Vec2::new(BOX_CENTER_X + BOX_HALF_WIDTH, 0.0);
    assert!(on_box_outline(outer_edge, center, half, CHALK_WIDTH));
    let front_edge = Vec2::new(BOX_CENTER_X, BOX_HALF_LENGTH);
    assert!(on_box_outline(front_edge, center, half, CHALK_WIDTH));

    assert!(
        !on_box_outline(center, center, half, CHALK_WIDTH),
        "the box's own centre should read as interior, not outline"
    );
    assert!(
        !on_box_outline(Vec2::ZERO, center, half, CHALK_WIDTH),
        "home plate itself must sit clear of the box outline"
    );
}

/// Regression guard: the batter's box must actually clear the plate by
/// the regulation 6 in gap (docs/BASEBALL.md), reading `PLATE_HALF_WIDTH`
/// and `BOX_PLATE_GAP` directly so a future edit that shrinks either
/// can't silently overlap the box onto the plate.
#[test]
fn batters_box_clears_the_plate() {
    let inner_edge = BOX_CENTER_X - BOX_HALF_WIDTH;
    assert!((inner_edge - (PLATE_HALF_WIDTH + BOX_PLATE_GAP)).abs() < 1e-6);
    assert!(inner_edge > PLATE_HALF_WIDTH);
}

/// The foul lines must run along the real bases' *outer* edge on their
/// way from home to the fence — offset from `FieldSpec::base_positions`
/// by exactly the bag's half-width, not through the centre (MLB Rule
/// 2.03 / groundskeeperu.com, see `foul_line_span`'s doc comment and
/// docs/BASEBALL.md) — derived from the base positions themselves, not a
/// hardcoded 45°, so this must hold for both variants (Standard's
/// diamond and FrontYard's lawn, which uses a different `fair_half_angle`
/// and base layout entirely).
///
/// Exercises `foul_line_span` — the exact function `spawn_foul_line`
/// calls to place the chalk — rather than re-deriving the direction/fence
/// formula inline, so a regression in the real spawn path actually fails
/// this test. Verified: temporarily changing `foul_line_span` to always
/// read `base_positions[0]` (ignoring `base_index`) made the third-base
/// assertions fail for both variants with a many-metres-off distance, as
/// expected; reverted after confirming the failure.
#[test]
fn foul_lines_pass_through_first_and_third_base() {
    for variant in [VariantId::Standard, VariantId::FrontYard] {
        let field = variant.field();
        for &base_index in &[0, field.base_count() - 1] {
            let base = field.base_positions[base_index];
            let (start, end) = foul_line_span(&field, base_index)
                .unwrap_or_else(|| panic!("{variant:?} base {base_index} at origin"));
            let dist = distance_point_to_segment(
                Vec2::new(base.x, base.z),
                Vec2::new(start.x, start.z),
                Vec2::new(end.x, end.z),
            );
            assert!(
                (dist - BASE_HALF_WIDTH).abs() < 1e-4,
                "{variant:?} base {base_index} at {base:?} is {dist} m off its foul line, \
                 want exactly the bag half-width ({BASE_HALF_WIDTH} m) — not zero (through \
                 the centre) and not the old chalk-half-width tolerance",
            );
        }
    }
}

/// Regression guard for the FrontYard z-fighting bug: `CHALK_Y` was
/// originally derived only against the stadium's own ground layers
/// (topmost `STADIUM_GRASS_INTERIOR_TOP` ≈ 0.0027), but `spawn_chalk_lines`
/// runs for *both* sceneries and the front yard's street/sidewalk/
/// centre-line decals sit higher (topmost `FRONTYARD_CENTERLINE_TOP` =
/// 0.006) — so a front-yard foul line crossing the street (z ≈ 20–32)
/// shared a z-plane with the centre line and z-fought. Checks the chalk
/// quads' actual *bottom* face (`CHALK_Y - CHALK_MESH_HEIGHT / 2`) clears
/// every named ground-decal top in both variants, reading the same consts
/// the spawn functions build their meshes from.
#[test]
fn chalk_clears_every_ground_decal_in_both_variants() {
    let chalk_bottom = CHALK_Y - CHALK_MESH_HEIGHT / 2.0;
    for (label, top) in [
        (
            "stadium dirt basepath",
            super::super::stadium::STADIUM_DIRT_TOP,
        ),
        ("stadium cutouts", super::super::stadium::STADIUM_CUTOUT_TOP),
        (
            "stadium grass interior",
            super::super::stadium::STADIUM_GRASS_INTERIOR_TOP,
        ),
        (
            "front yard street/sidewalks",
            super::super::stadium::FRONTYARD_STREET_TOP,
        ),
        (
            "front yard centre line",
            super::super::stadium::FRONTYARD_CENTERLINE_TOP,
        ),
    ] {
        assert!(
            chalk_bottom > top,
            "chalk bottom {chalk_bottom} does not clear {label}'s top face {top}"
        );
    }
}
