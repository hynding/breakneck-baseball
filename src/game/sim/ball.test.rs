//! Unit tests for [`super`] — ball flight.

use super::*;
use bevy::time::TimeUpdateStrategy;
use std::time::Duration;

/// Launches the flight tests fly: a deep fly to centre (~100 mph at 28°)
/// and a game-typical Perfect drive into the gap (~85 mph at 20°, pulled).
const LAUNCHES: [(f32, f32, f32); 2] = [(44.7, 28.0, 0.0), (38.0, 20.0, -18.0)];

/// A batted ball from the plate at bat height: `speed` m/s, `launch`°
/// up, `yaw`° off dead centre, with `rules::hit_spin`'s spin.
fn batted(speed: f32, launch: f32, yaw: f32) -> (Vec3, Vec3, Vec3) {
    let (l, y) = (launch.to_radians(), yaw.to_radians());
    let launch = Vec3::new(l.cos() * y.sin(), l.sin(), l.cos() * y.cos()) * speed;
    (
        Vec3::new(0.0, 1.0, 0.5),
        launch,
        crate::game::rules::hit_spin(launch),
    )
}

/// Flies the ball with only physics and the aero systems running, stepped
/// at `fps`, and returns where (ground plane, linearly interpolated) and
/// when it first comes down to y = 0.
fn carry_at(fps: f64, (start, vel, spin): (Vec3, Vec3, Vec3)) -> (Vec3, f32) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        bevy::asset::AssetPlugin::default(),
        bevy::mesh::MeshPlugin,
        bevy::scene::ScenePlugin,
        RapierPhysicsPlugin::<NoUserData>::default(),
        crate::game::physics_clock::PhysicsClockPlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        1.0 / fps,
    )))
    .init_resource::<crate::game::rules::Wind>();
    add_aero_systems(&mut app, || true);

    let ball = app
        .world_mut()
        .spawn((
            Baseball,
            InFlight,
            Batted,
            Transform::from_translation(start),
            RigidBody::Dynamic,
            Collider::ball(BALL_RADIUS),
            ColliderMassProperties::Mass(BALL_MASS),
            Velocity {
                linvel: vel,
                angvel: spin,
            },
            Damping {
                linear_damping: 0.0,
                angular_damping: crate::game::rules::SPIN_DAMPING,
            },
        ))
        .id();

    // Two warm-up frames: time's first delta is zero and Rapier builds the body.
    let mut prev = start;
    let mut t = 0.0_f32;
    for _ in 0..(20.0 * fps) as usize {
        app.update();
        let now = app.world().get::<Transform>(ball).unwrap().translation;
        t += app.world().resource::<Time>().delta_secs();
        if now.y <= 0.0 && prev.y > 0.0 {
            let f = prev.y / (prev.y - now.y);
            return (prev.lerp(now, f).with_y(0.0), t);
        }
        prev = now;
    }
    panic!("{fps} fps: the ball never came down");
}

#[test]
fn a_fly_ball_carries_the_same_at_any_frame_rate() {
    for (speed, launch, yaw) in LAUNCHES {
        let ball = batted(speed, launch, yaw);
        let (ref_spot, ref_t) = carry_at(240.0, ball);
        for fps in [20.0, 30.0, 60.0, 144.0] {
            let (spot, t) = carry_at(fps, ball);
            let gap = spot.distance(ref_spot);
            assert!(
                gap < 0.5,
                "{speed} m/s at {launch}°: {fps} fps lands {gap:.2} m from 240 fps"
            );
            assert!(
                (t - ref_t).abs() < 0.06,
                "{speed} m/s at {launch}°: {fps} fps hangs {t:.3} s vs {ref_t:.3} s"
            );
        }
    }
}

/// Fielders, the landing ring and the cameras all chase
/// `rules::predict_landing_from`; the live ball must come down there.
#[test]
fn the_flight_lands_where_fielders_predict_it() {
    for (speed, launch, yaw) in LAUNCHES {
        let ball @ (start, vel, spin) = batted(speed, launch, yaw);
        let (predicted, _) = crate::game::rules::predict_landing_from(
            start,
            vel,
            spin,
            BALL_DRAG_FACTOR,
            MAGNUS_FACTOR,
            Vec3::ZERO,
        );
        for fps in [30.0, 60.0, 240.0] {
            let (spot, _) = carry_at(fps, ball);
            let gap = spot.distance(predicted);
            assert!(
                gap < 0.5,
                "{speed} m/s at {launch}°: {fps} fps lands {gap:.2} m from the prediction"
            );
        }
    }
}
