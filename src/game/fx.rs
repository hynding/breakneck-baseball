//! Game feel — hit-stop and impact particles. Purely cosmetic: nothing here
//! may touch the scoreboard, the bases, or the rules.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{CollisionEvent, Velocity};

use crate::game::ai::{hash01, noise};
use crate::game::ball::{
    Baseball, HitEvent, InFlight, WallBangEvent, BALL_DRAG_FACTOR, MAGNUS_FACTOR,
};
use crate::game::flow::{Phase, Play};
use crate::game::rules;
use crate::game::theme::Theme;
use crate::game::{GameState, GameplayEntity};

/// How hard time slows on contact, and for how long (real seconds).
const HIT_STOP_SCALE: f32 = 0.05;
const HIT_STOP_SECS: f32 = 0.06;

#[derive(Resource, Default)]
struct HitStop(Option<Timer>);

/// Freezes the world for a beat when bat meets ball.
fn start_hit_stop(
    mut hits: EventReader<HitEvent>,
    mut virt: ResMut<Time<Virtual>>,
    mut stop: ResMut<HitStop>,
) {
    if hits.read().next().is_some() {
        virt.set_relative_speed(HIT_STOP_SCALE);
        stop.0 = Some(Timer::from_seconds(HIT_STOP_SECS, TimerMode::Once));
    }
}

/// Restores full speed once the (real-time) freeze window elapses.
fn end_hit_stop(real: Res<Time<Real>>, mut virt: ResMut<Time<Virtual>>, mut stop: ResMut<HitStop>) {
    let finished = stop
        .0
        .as_mut()
        .is_some_and(|t| t.tick(real.delta()).finished());
    if finished {
        virt.set_relative_speed(1.0);
        stop.0 = None;
    }
}

// ── Particles ─────────────────────────────────────────────────────────────────

/// One transient effect mote: moves, scales, dies.
#[derive(Component)]
struct Particle {
    vel: Vec3,
    timer: Timer,
    gravity: f32,
    /// Positive = expands to (1 + grow); negative = shrinks to nothing.
    grow: f32,
}

/// Shared meshes/materials for effects, built once per game from the theme.
#[derive(Resource)]
struct FxAssets {
    spark_mesh: Handle<Mesh>,
    dust_mesh: Handle<Mesh>,
    spark: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
}

fn build_fx_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    theme: Res<Theme>,
) {
    commands.insert_resource(FxAssets {
        spark_mesh: meshes.add(Sphere::new(0.07)),
        dust_mesh: meshes.add(Sphere::new(0.14)),
        spark: materials.add(StandardMaterial {
            base_color: theme.ball.trail,
            unlit: true,
            ..default()
        }),
        dust: materials.add(StandardMaterial {
            base_color: Color::srgba(0.75, 0.7, 0.6, 1.0),
            unlit: true,
            ..default()
        }),
    });
}

// ── Landing ring ──────────────────────────────────────────────────────────────

/// The touchdown indicator: a flat ring on the grass under a live fly ball.
#[derive(Component)]
struct LandingRing;

/// Ring radius per second of remaining hang time, and its bounds.
const RING_PER_SECOND: f32 = 0.8;
const RING_MIN: f32 = 0.55;
const RING_MAX: f32 = 3.5;
/// Below this ball height the ring retires (the ball is basically down).
const RING_OFF_HEIGHT: f32 = 1.2;

fn spawn_landing_ring(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    theme: Res<Theme>,
) {
    commands.spawn((
        LandingRing,
        GameplayEntity,
        Mesh3d(meshes.add(Torus {
            minor_radius: 0.07,
            major_radius: 1.0,
        })),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: theme.ui.accent.with_alpha(0.85),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.06, 0.0),
        Visibility::Hidden,
    ));
}

/// While an uncalled fly ball is up, the ring sits on its predicted landing
/// spot and shrinks with the remaining hang time — where *and when* it will
/// come down, at a glance.
#[allow(clippy::type_complexity)]
fn update_landing_ring(
    play: Res<Play>,
    ball_q: Query<(&Transform, &Velocity), (With<Baseball>, With<InFlight>, Without<LandingRing>)>,
    mut ring_q: Query<(&mut Transform, &mut Visibility), With<LandingRing>>,
) {
    let Ok((mut ring_tf, mut visibility)) = ring_q.get_single_mut() else {
        return;
    };
    let live = play.phase == Phase::InPlay && !play.is_resolved();
    let flying = ball_q
        .get_single()
        .ok()
        .filter(|(ball, _)| ball.translation.y > RING_OFF_HEIGHT);
    let Some((ball, vel)) = (if live { flying } else { None }) else {
        if *visibility != Visibility::Hidden {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let (landing, hang) = rules::predict_landing_from(
        ball.translation,
        vel.linvel,
        vel.angvel,
        BALL_DRAG_FACTOR,
        MAGNUS_FACTOR,
    );
    ring_tf.translation = Vec3::new(landing.x, 0.06, landing.z);
    let radius = (RING_MIN + RING_PER_SECOND * hang).clamp(RING_MIN, RING_MAX);
    ring_tf.scale = Vec3::new(radius, 1.0, radius);
    if *visibility != Visibility::Inherited {
        *visibility = Visibility::Inherited;
    }
}

/// Sparks fly off the bat at contact.
fn contact_burst(
    mut hits: EventReader<HitEvent>,
    ball_q: Query<&Transform, With<Baseball>>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    for _ in hits.read() {
        let Ok(ball) = ball_q.get_single() else {
            continue;
        };
        for i in 0..10 {
            let seed = time.elapsed_secs() * 13.7 + i as f32 * 1.618;
            let dir = Vec3::new(
                noise(seed),
                hash01(seed * 1.3) * 0.8 + 0.2,
                noise(seed * 1.7),
            )
            .normalize_or_zero();
            commands.spawn((
                Particle {
                    vel: dir * (4.0 + hash01(seed * 2.1) * 5.0),
                    timer: Timer::from_seconds(0.35, TimerMode::Once),
                    gravity: 4.0,
                    grow: -1.0,
                },
                GameplayEntity,
                Mesh3d(assets.spark_mesh.clone()),
                MeshMaterial3d(assets.spark.clone()),
                Transform::from_translation(ball.translation),
            ));
        }
    }
}

/// Sparks spray back off the padding when the ball bangs the wall.
fn wall_bang_burst(
    mut bangs: EventReader<WallBangEvent>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    for bang in bangs.read() {
        // Spray hemisphere back toward the infield (the wall is behind).
        let inward = -Vec3::new(bang.pos.x, 0.0, bang.pos.z).normalize_or_zero();
        for i in 0..8 {
            let seed = time.elapsed_secs() * 11.3 + i as f32 * 1.618;
            let dir = (inward * (0.6 + hash01(seed))
                + Vec3::new(noise(seed * 1.3), hash01(seed * 1.7), noise(seed * 2.1)) * 0.7)
                .normalize_or_zero();
            commands.spawn((
                Particle {
                    vel: dir * (3.0 + hash01(seed * 2.9) * 4.0),
                    timer: Timer::from_seconds(0.4, TimerMode::Once),
                    gravity: 5.0,
                    grow: -1.0,
                },
                GameplayEntity,
                Mesh3d(assets.spark_mesh.clone()),
                MeshMaterial3d(assets.spark.clone()),
                Transform::from_translation(bang.pos),
            ));
        }
    }
}

/// Threshold impact speed for a dust puff (m/s).
const DUST_MIN_SPEED: f32 = 4.0;

/// A puff of dirt wherever the ball thumps the ground.
fn bounce_dust(
    mut collisions: EventReader<CollisionEvent>,
    ball_q: Query<(Entity, &Transform, &Velocity), With<Baseball>>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let Ok((ball_entity, ball_tf, vel)) = ball_q.get_single() else {
        return;
    };
    for event in collisions.read() {
        let CollisionEvent::Started(a, b, _) = event else {
            continue;
        };
        if *a != ball_entity && *b != ball_entity {
            continue;
        }
        if vel.linvel.length() < DUST_MIN_SPEED {
            continue;
        }
        for i in 0..6 {
            let seed = time.elapsed_secs() * 9.1 + i as f32 * 2.399;
            commands.spawn((
                Particle {
                    vel: Vec3::new(
                        noise(seed) * 1.6,
                        0.6 + hash01(seed * 1.9),
                        noise(seed * 2.3) * 1.6,
                    ),
                    timer: Timer::from_seconds(0.4, TimerMode::Once),
                    gravity: 0.8,
                    grow: 1.6,
                },
                GameplayEntity,
                Mesh3d(assets.dust_mesh.clone()),
                MeshMaterial3d(assets.dust.clone()),
                Transform::from_translation(Vec3::new(
                    ball_tf.translation.x,
                    0.08,
                    ball_tf.translation.z,
                )),
            ));
        }
    }
}

/// Moves, scales, and expires every live particle.
fn tick_particles(
    time: Res<Time>,
    mut particles: Query<(Entity, &mut Particle, &mut Transform)>,
    mut commands: Commands,
) {
    let dt = time.delta_secs();
    for (entity, mut particle, mut transform) in &mut particles {
        let gravity = particle.gravity;
        particle.vel.y -= gravity * dt;
        let step = particle.vel * dt;
        transform.translation += step;
        let f = particle.timer.tick(time.delta()).fraction();
        transform.scale = Vec3::splat(if particle.grow >= 0.0 {
            1.0 + particle.grow * f
        } else {
            (1.0 - f).max(0.01)
        });
        if particle.timer.finished() {
            commands.entity(entity).despawn();
        }
    }
}

pub struct FxPlugin;

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HitStop>()
            .add_systems(
                crate::game::game_start(),
                (build_fx_assets, spawn_landing_ring),
            )
            .add_systems(
                Update,
                (
                    start_hit_stop,
                    end_hit_stop,
                    contact_burst,
                    wall_bang_burst,
                    bounce_dust,
                    update_landing_ring,
                    tick_particles,
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
