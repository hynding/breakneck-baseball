//! Unit tests for [`super`] — ball readability.

use super::*;
use crate::game::quality::QualityTier;
use crate::game::theme::ThemeId;

fn visual_radius() -> f32 {
    BALL_RADIUS * ThemeId::DaylightClassic.build().ball.visual_scale
}

/// The deepest fly-ball framing: a phone held landscape (390 logical px
/// tall at 3x) on Low, the play camera's 45° lens, the ball 140 m out
/// (a fly at the wall seen from behind the plate).
fn deepest_fly() -> (f32, f32, f32) {
    let h = 390.0 * 3.0 * QualityTier::Low.render_scale(3.0);
    (140.0, 45.0_f32.to_radians(), h)
}

#[test]
fn the_bare_ball_vanishes_at_the_deepest_fly() {
    // Why the proxy exists: the enlarged visual ball is under a pixel there.
    let (dist, fov, h) = deepest_fly();
    let visual = visual_radius();
    assert!(visual * px_per_metre(dist, fov, h) < 0.5);
}

#[test]
fn the_ball_never_drops_below_three_pixels() {
    let (dist, fov, h) = deepest_fly();
    let floor = min_world_radius(dist, fov, h, MIN_BALL_PX);
    let across = 2.0 * floor * px_per_metre(dist, fov, h);
    assert!((across - 3.0).abs() < 1e-3, "{across} px");
    // Up close the proxy stays out of the way: the real ball is bigger.
    assert!(min_world_radius(8.0, fov, h, MIN_BALL_PX) < visual_radius());
}

#[test]
fn the_shadow_spreads_as_the_ball_climbs_but_stays_a_spot() {
    let r = visual_radius();
    assert!(
        shadow_radius(0.0, r) > r,
        "wider than the ball on the ground"
    );
    assert!(shadow_radius(20.0, r) > shadow_radius(2.0, r));
    assert_eq!(shadow_radius(200.0, r), r * 4.0, "capped");
    assert_eq!(shadow_radius(-1.0, r), shadow_radius(0.0, r));
}

/// The shadow follows the flying ball and goes away the frame flow hides
/// it (a catch), never a frame late; the proxy stays hidden without a
/// rendered view to measure against.
#[test]
fn the_shadow_follows_the_ball_and_hides_with_it() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .insert_resource(ThemeId::DaylightClassic.build())
        .add_systems(Startup, spawn_ball_readability)
        .add_systems(Update, (place_ball_shadow, place_ball_proxy));
    let ball = app
        .world_mut()
        .spawn((
            Baseball,
            InFlight,
            Transform::from_xyz(3.0, 12.0, 60.0),
            Visibility::Inherited,
            MeshMaterial3d::<StandardMaterial>::default(),
        ))
        .id();
    app.update();
    let world = app.world_mut();
    let (tf, vis) = world
        .query_filtered::<(&Transform, &Visibility), With<BallShadow>>()
        .single(world)
        .unwrap();
    assert_eq!(*vis, Visibility::Inherited);
    assert_eq!((tf.translation.x, tf.translation.z), (3.0, 60.0));
    assert!(tf.scale.x > visual_radius(), "spread at 12 m up");

    world.entity_mut(ball).insert(Visibility::Hidden);
    app.update();
    let world = app.world_mut();
    // (The proxy is hidden throughout: there is no rendered view here.)
    let vis = world
        .query_filtered::<&Visibility, With<BallShadow>>()
        .single(world)
        .unwrap();
    assert_eq!(*vis, Visibility::Hidden);
}
