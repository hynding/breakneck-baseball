//! Ball readability (task 4 of the graphics and camera plan): the ball must
//! stay findable at every tier, on a phone, at the top of a deep fly.
//!
//! - **Blob shadow.** A soft dark disc on the ground under the flying ball,
//!   at every tier. The sun's shadow map (512 texels on Low) loses a 7 cm
//!   ball, and a shadow straight below, rather than offset along the sun,
//!   is the cue that tells a player where the ball will come down. The ball
//!   itself casts no map shadow (`ball::spawn_ball`), so there is one
//!   shadow at every tier, not two.
//! - **Minimum on-screen size.** When the ball shrinks under
//!   [`MIN_BALL_PX`] pixels of radius in the rendered image, a proxy sphere
//!   in the ball's own material sits over it, sized to exactly that, so it
//!   never drops below ~3 px across. The ball's own transform is never
//!   scaled: it carries the regulation collider.
//! - **Flight ghosts.** Fading spheres dropped behind any fast-flying ball,
//!   pitch or batted (they lived in `sim::ball` before; they are cosmetic).
//!   Their lifetime follows the tier's `trail_scale`, so the trail is
//!   shorter on Low.
//!
//! Like every fx system this only reads the ball; nothing here touches
//! physics, the rules or `Time<Virtual>`.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::GameplayEntity;
use crate::game::ball::{BALL_RADIUS, Baseball, InFlight};
use crate::game::camera::MainCamera;
use crate::game::quality::ActiveQuality;
use crate::game::theme::Theme;

/// Smallest on-screen ball radius, in rendered pixels (3 px across).
pub const MIN_BALL_PX: f32 = 1.5;
/// Smallest on-screen blob-shadow radius, in rendered pixels. A flat disc
/// seen from a low camera is foreshortened to a sliver, so this is larger
/// than the ball's floor.
pub const MIN_SHADOW_PX: f32 = 3.0;
/// Shadow height above the turf: over the chalk, under the landing ring.
const SHADOW_Y: f32 = 0.02;
const SHADOW_ALPHA: f32 = 0.45;

/// Ghost lifetime (seconds) at a `trail_scale` of 1, and spawn cadence.
const GHOST_LIFETIME: f32 = 0.35;
const GHOST_INTERVAL: f32 = 0.025;
/// Below this speed (m/s) the ball is rolling, not flying: no ghosts.
const GHOST_MIN_SPEED: f32 = 8.0;

/// Rendered pixels per metre at `dist` metres from a camera with vertical
/// field of view `vfov` (radians) and a viewport `viewport_h` pixels tall.
pub fn px_per_metre(dist: f32, vfov: f32, viewport_h: f32) -> f32 {
    viewport_h / (2.0 * dist.max(0.01) * (vfov * 0.5).tan())
}

/// The world radius that shows as `min_px` pixels at that distance.
pub fn min_world_radius(dist: f32, vfov: f32, viewport_h: f32, min_px: f32) -> f32 {
    min_px / px_per_metre(dist, vfov, viewport_h)
}

/// Blob-shadow radius for a ball `height` metres up: ball-sized on the
/// ground, spreading as it climbs so a high fly's spot is easy to find,
/// capped so a pop-up's shadow never reads as a puddle.
pub fn shadow_radius(height: f32, ball_radius: f32) -> f32 {
    (ball_radius * 1.2 + 0.012 * height.max(0.0)).min(ball_radius * 4.0)
}

#[derive(Component)]
pub(super) struct BallShadow;

#[derive(Component)]
pub(super) struct BallProxy;

/// One fading ghost of the ball's path; shrinks to nothing and despawns.
#[derive(Component)]
pub struct FlightGhost(Timer);

/// Shared ghost mesh and material, built per game from the theme.
#[derive(Resource)]
pub(super) struct GhostAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// The main camera's eye, vertical FOV and rendered height in pixels (the
/// render-scaled target on Low/Medium, not the window).
type MainView<'w, 's> = Query<
    'w,
    's,
    (
        &'static GlobalTransform,
        &'static Camera,
        &'static Projection,
    ),
    With<MainCamera>,
>;

fn main_view(cameras: &MainView) -> Option<(Vec3, f32, f32)> {
    let (eye, camera, projection) = cameras.iter().find(|(_, c, _)| c.is_active)?;
    let Projection::Perspective(p) = projection else {
        return None;
    };
    let h = camera.physical_viewport_size()?.y as f32;
    (h > 0.0).then_some((eye.translation(), p.fov, h))
}

pub(super) fn spawn_ball_readability(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    theme: Res<Theme>,
) {
    let visual_radius = BALL_RADIUS * theme.ball.visual_scale;
    commands.insert_resource(GhostAssets {
        mesh: meshes.add(Sphere::new(visual_radius * 0.8)),
        material: materials.add(StandardMaterial {
            base_color: theme.ball.trail,
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    });
    commands.spawn((
        BallShadow,
        GameplayEntity,
        Mesh3d(meshes.add(Circle::new(1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.0, 0.0, 0.0, SHADOW_ALPHA),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        })),
        // `Circle` faces +Z; lay it flat, facing up.
        Transform::from_xyz(0.0, -5.0, 0.0)
            .with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        NotShadowCaster,
        Visibility::Hidden,
    ));
    commands.spawn((
        BallProxy,
        GameplayEntity,
        Mesh3d(meshes.add(Sphere::new(1.0))),
        // Swapped for the ball's own material on first sight.
        MeshMaterial3d::<StandardMaterial>::default(),
        Transform::from_xyz(0.0, -5.0, 0.0),
        NotShadowCaster,
        Visibility::Hidden,
    ));
}

fn set_visible(mut visibility: Mut<Visibility>, on: bool) {
    visibility.set_if_neq(if on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

/// Parks the blob shadow under the flying ball; hidden otherwise.
#[allow(clippy::type_complexity)]
pub(super) fn place_ball_shadow(
    theme: Res<Theme>,
    ball_q: Query<(&Transform, &InheritedVisibility), (With<Baseball>, With<InFlight>)>,
    cameras: MainView,
    mut shadow_q: Query<(&mut Transform, &mut Visibility), (With<BallShadow>, Without<Baseball>)>,
) {
    let Ok((mut tf, visibility)) = shadow_q.single_mut() else {
        return;
    };
    let ball = ball_q.single().ok().filter(|(_, v)| v.get());
    let Some((ball, _)) = ball else {
        set_visible(visibility, false);
        return;
    };
    let spot = ball.translation.with_y(SHADOW_Y);
    let mut radius = shadow_radius(ball.translation.y, BALL_RADIUS * theme.ball.visual_scale);
    if let Some((eye, fov, h)) = main_view(&cameras) {
        radius = radius.max(min_world_radius(eye.distance(spot), fov, h, MIN_SHADOW_PX));
    }
    tf.translation = spot;
    tf.scale = Vec3::splat(radius);
    set_visible(visibility, true);
}

/// Keeps the ball at least [`MIN_BALL_PX`] pixels in radius on screen.
#[allow(clippy::type_complexity)]
pub(super) fn place_ball_proxy(
    theme: Res<Theme>,
    ball_q: Query<
        (
            &Transform,
            &InheritedVisibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        With<Baseball>,
    >,
    cameras: MainView,
    mut proxy_q: Query<
        (
            &mut Transform,
            &mut Visibility,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        (With<BallProxy>, Without<Baseball>),
    >,
) {
    let Ok((mut tf, visibility, mut material)) = proxy_q.single_mut() else {
        return;
    };
    let (Ok((ball, shown, ball_material)), Some((eye, fov, h))) =
        (ball_q.single(), main_view(&cameras))
    else {
        set_visible(visibility, false);
        return;
    };
    let visual = BALL_RADIUS * theme.ball.visual_scale;
    let floor = min_world_radius(eye.distance(ball.translation), fov, h, MIN_BALL_PX);
    if !shown.get() || floor <= visual {
        set_visible(visibility, false);
        return;
    }
    if material.0 != ball_material.0 {
        material.0 = ball_material.0.clone();
    }
    tf.translation = ball.translation;
    tf.scale = Vec3::splat(floor);
    set_visible(visibility, true);
}

/// Drops a fading ghost behind the fast-flying ball every
/// [`GHOST_INTERVAL`] seconds so its path reads at a glance.
#[allow(clippy::type_complexity)]
pub(super) fn spawn_flight_ghosts(
    time: Res<Time>,
    mut since_last: Local<f32>,
    assets: Option<Res<GhostAssets>>,
    active: Option<Res<ActiveQuality>>,
    ball_q: Query<(&Transform, &Velocity), (With<Baseball>, With<InFlight>)>,
    mut commands: Commands,
) {
    let Some(assets) = assets else {
        return;
    };
    let Ok((transform, vel)) = ball_q.single() else {
        return;
    };
    if vel.linvel.length() < GHOST_MIN_SPEED {
        return;
    }
    *since_last += time.delta_secs();
    if *since_last < GHOST_INTERVAL {
        return;
    }
    *since_last = 0.0;
    let scale = active.map_or(1.0, |a| a.budget().trail_scale);
    commands.spawn((
        FlightGhost(Timer::from_seconds(GHOST_LIFETIME * scale, TimerMode::Once)),
        GameplayEntity,
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.material.clone()),
        Transform::from_translation(transform.translation),
        NotShadowCaster,
    ));
}

/// Shrinks each ghost to nothing over its lifetime, then despawns it.
pub(super) fn fade_flight_ghosts(
    time: Res<Time>,
    mut ghosts: Query<(Entity, &mut FlightGhost, &mut Transform)>,
    mut commands: Commands,
) {
    for (entity, mut ghost, mut transform) in &mut ghosts {
        if ghost.0.tick(time.delta()).is_finished() {
            commands.entity(entity).despawn();
        } else {
            transform.scale = Vec3::splat(1.0 - ghost.0.fraction());
        }
    }
}

#[cfg(test)]
#[path = "ball_read.test.rs"]
mod tests;
