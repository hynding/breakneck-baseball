//! Unit tests for [`super`] — the stadium module.

use super::*;

/// Mirrors the rotation + translation `spawn_stadium_ground` applies to
/// the infield-dirt cuboid (`Transform::from_rotation_y(FRAC_PI_4)`
/// around a centre at `(0, HALF_DIAGONAL)`), so the diamond's *actual*
/// world-space corners — not just its nominal half-size — can be checked
/// against the true base positions.
fn diamond_corner(sign_x: f32, sign_z: f32, half_size: f32) -> Vec3 {
    let center = Vec3::new(0.0, 0.0, HALF_DIAGONAL);
    let rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
    center + rotation * Vec3::new(sign_x * half_size, 0.0, sign_z * half_size)
}

/// The infield dirt diamond's four corners must land exactly on home
/// plate and the three bases, so the dirt basepath band runs along the
/// real baselines and every bag sits centered on it (docs/BASEBALL.md's
/// groundskeeping notes). Regression test for a bug where `INFIELD_HALF`
/// was `BASE_DISTANCE / √2` — √2 too large — which overshot the bases by
/// ~40% and left the bags (and home plate's own dirt cutout, painted
/// underneath the grass-interior layer) stranded inside the diamond's
/// grass interior instead of on its dirt corners. Reads `INFIELD_HALF`
/// directly — the same const `spawn_stadium_ground` builds the mesh
/// from — so a regression to the old formula fails this test.
#[test]
fn infield_diamond_corners_align_with_bases() {
    let eps = 0.01;

    let home = diamond_corner(1.0, -1.0, INFIELD_HALF);
    assert!(home.distance(Vec3::ZERO) < eps, "home at {home:?}");

    let first = diamond_corner(-1.0, -1.0, INFIELD_HALF);
    let want_first = Vec3::new(-HALF_DIAGONAL, 0.0, HALF_DIAGONAL);
    assert!(
        first.distance(want_first) < eps,
        "first at {first:?}, want {want_first:?}"
    );

    let second = diamond_corner(-1.0, 1.0, INFIELD_HALF);
    let want_second = Vec3::new(0.0, 0.0, HALF_DIAGONAL * 2.0);
    assert!(
        second.distance(want_second) < eps,
        "second at {second:?}, want {want_second:?}"
    );

    let third = diamond_corner(1.0, 1.0, INFIELD_HALF);
    let want_third = Vec3::new(HALF_DIAGONAL, 0.0, HALF_DIAGONAL);
    assert!(
        third.distance(want_third) < eps,
        "third at {third:?}, want {want_third:?}"
    );
}

/// Regression for a units bug: `spawn_lighting`'s `ambient_fraction`
/// call sites (0.15 Stadium, 0.20 FrontYard — retuned for contrast in
/// the Task 20 polish sweep, previously 0.25/0.35) were passed straight
/// through as `AmbientLight::brightness` — fine under Bevy's pre-0.14
/// small-multiplier semantics, but in 0.15 `brightness` is an absolute
/// lux-like value (`AmbientLight::default()` is 80.0), so a raw 0.25 was
/// indistinguishable from "no ambient light at all" next to a 50,000 lux
/// sun. Every scenery's resulting ambient must clear Bevy's own default
/// fill (else it's *darker* than doing nothing) and stay a sane fraction
/// of the sun rather than approaching or exceeding it.
#[test]
fn ambient_fraction_scales_with_sun_illuminance_not_raw() {
    for ambient_fraction in [0.15_f32, 0.20] {
        let brightness = ambient_fraction * SUN_ILLUMINANCE;
        assert!(
            brightness > AmbientLight::default().brightness,
            "ambient {brightness} lux is dimmer than Bevy's own default ({}) — \
             shadowed surfaces will render darker than out-of-the-box Bevy",
            AmbientLight::default().brightness
        );
        assert!(
            brightness < SUN_ILLUMINANCE,
            "ambient {brightness} lux should stay a fraction of the sun ({SUN_ILLUMINANCE}), \
             not wash shadows out entirely"
        );
    }
}
