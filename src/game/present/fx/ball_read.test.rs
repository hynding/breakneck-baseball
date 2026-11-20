//! Unit tests for [`super`] — ball readability.

use super::*;
use crate::game::quality::QualityTier;

/// The deepest fly-ball framing: a phone held landscape (390 logical px
/// tall at 3x) on Low, the play camera's 45° lens, the ball 140 m out
/// (a fly at the wall seen from behind the plate).
fn deepest_fly() -> (f32, f32, f32) {
    let h = 390.0 * 3.0 * QualityTier::Low.render_scale(3.0);
    (140.0, 45.0_f32.to_radians(), h)
}

#[test]
fn the_bare_ball_vanishes_at_the_deepest_fly() {
    // Why the proxy exists: the 2.7x visual ball is under a pixel there.
    let (dist, fov, h) = deepest_fly();
    let visual = BALL_RADIUS * 2.7;
    assert!(visual * px_per_metre(dist, fov, h) < 0.5);
}

#[test]
fn the_ball_never_drops_below_three_pixels() {
    let (dist, fov, h) = deepest_fly();
    let floor = min_world_radius(dist, fov, h, MIN_BALL_PX);
    let across = 2.0 * floor * px_per_metre(dist, fov, h);
    assert!((across - 3.0).abs() < 1e-3, "{across} px");
    // Up close the proxy stays out of the way: the real ball is bigger.
    assert!(min_world_radius(8.0, fov, h, MIN_BALL_PX) < BALL_RADIUS * 2.7);
}

#[test]
fn the_shadow_spreads_as_the_ball_climbs_but_stays_a_spot() {
    let r = BALL_RADIUS * 2.7;
    assert!(
        shadow_radius(0.0, r) > r,
        "wider than the ball on the ground"
    );
    assert!(shadow_radius(20.0, r) > shadow_radius(2.0, r));
    assert_eq!(shadow_radius(200.0, r), r * 4.0, "capped");
    assert_eq!(shadow_radius(-1.0, r), shadow_radius(0.0, r));
}

#[test]
fn the_trail_is_shorter_on_low() {
    assert!(QualityTier::Low.budget().trail_scale < QualityTier::High.budget().trail_scale);
}
