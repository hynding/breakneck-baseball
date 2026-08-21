//! Impact particles: contact sparks, wall bangs, dirt puffs, home-run
//! fireworks, and the fly-ball landing ring.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{CollisionEvent, Velocity};

use crate::game::GameplayEntity;
use crate::game::ai::{hash01, noise};
use crate::game::ball::{
    BALL_DRAG_FACTOR, Baseball, HitEvent, InFlight, MAGNUS_FACTOR, WallBangEvent,
};
use crate::game::flow::{BallInPlayEvent, Phase, Play};
use crate::game::rules::{self, ContactKind, ContactQuality};
use crate::game::settings::Settings;
use crate::game::theme::Theme;

use super::FxAssets;
use super::trail::build_trail_assets;

/// One transient effect mote: moves, scales, dies.
#[derive(Component)]
pub(super) struct Particle {
    vel: Vec3,
    timer: Timer,
    gravity: f32,
    /// Positive = expands to (1 + grow); negative = shrinks to nothing.
    grow: f32,
}

pub(super) fn build_fx_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    theme: Res<Theme>,
    settings: Res<Settings>,
) {
    build_trail_assets(&mut commands, &mut meshes, &mut materials, &settings);
    let firework = [
        Color::srgb(1.0, 0.85, 0.30),
        Color::srgb(1.0, 0.35, 0.35),
        Color::srgb(0.45, 0.70, 1.0),
        Color::srgb(0.60, 1.0, 0.55),
        Color::srgb(1.0, 0.55, 0.90),
    ]
    .into_iter()
    .map(|base_color| {
        materials.add(StandardMaterial {
            base_color,
            unlit: true,
            ..default()
        })
    })
    .collect();
    commands.insert_resource(FxAssets {
        spark_mesh: meshes.add(Sphere::new(0.07)),
        dust_mesh: meshes.add(Sphere::new(0.14)),
        firework_mesh: meshes.add(Sphere::new(0.18)),
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
        firework,
    });
    // A fresh show state each game (this system runs on `game_start()`).
    commands.insert_resource(Fireworks::default());
}

// ── Landing ring ──────────────────────────────────────────────────────────────

/// The touchdown indicator: a flat ring on the grass under a live fly ball.
#[derive(Component)]
pub(super) struct LandingRing;

/// Ring radius per second of remaining hang time, and its bounds.
const RING_PER_SECOND: f32 = 0.8;
const RING_MIN: f32 = 0.55;
const RING_MAX: f32 = 3.5;
/// Below this ball height the ring retires (the ball is basically down).
const RING_OFF_HEIGHT: f32 = 1.2;

pub(super) fn spawn_landing_ring(
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
pub(super) fn update_landing_ring(
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
pub(super) fn contact_burst(
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
pub(super) fn wall_bang_burst(
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

// ── Home-run fireworks ──────────────────────────────────────────────────────────

/// Seconds a home-run show keeps launching bursts — long enough to ride the
/// trot through the result pause (the play holds the next at-bat until the
/// runners settle, so the trot window is real).
const FIREWORKS_SECS: f32 = 5.0;
/// Delay between bursts; a dead-on Perfect launches them faster and denser.
const FIREWORKS_BURST_SECS: f32 = 0.32;
const FIREWORKS_BURST_SECS_PERFECT: f32 = 0.20;
/// Sparks per burst (more for a Perfect).
const FIREWORKS_SPARKS: usize = 20;
const FIREWORKS_SPARKS_PERFECT: usize = 34;

/// A single firework mote — a [`Particle`] tagged so the home-run show can be
/// told apart from the incidental contact/wall/dust sparks (the e2e test reads
/// this; nothing gameplay does). Purely cosmetic.
#[derive(Component)]
pub struct FireworkSpark;

/// The live state of a home-run fireworks show: bursts keep launching over the
/// outfield until `remaining` runs out, faster and denser after a Perfect
/// swing. Reset fresh each game in [`build_fx_assets`].
#[derive(Resource, Default)]
pub(super) struct Fireworks {
    active: bool,
    remaining: Timer,
    next: Timer,
    perfect: bool,
}

/// The home run is a moment: on a ball over the fence, launch a fireworks show
/// over the outfield — brighter and faster off a dead-on Perfect swing
/// (`Play::last_contact_quality`) — that keeps bursting for the length of the
/// trot. Scales up the same spark burst the wall bang uses; like every fx
/// system it only spawns cosmetic motes and never touches the score.
pub(super) fn home_run_fireworks(
    mut in_play: EventReader<BallInPlayEvent>,
    play: Res<Play>,
    assets: Option<Res<FxAssets>>,
    time: Res<Time>,
    mut show: ResMut<Fireworks>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    for ev in in_play.read() {
        if matches!(ev.kind, ContactKind::HomeRun) {
            show.active = true;
            show.perfect = play.last_contact_quality() == Some(ContactQuality::Perfect);
            show.remaining = Timer::from_seconds(FIREWORKS_SECS, TimerMode::Once);
            show.next = Timer::from_seconds(0.0, TimerMode::Once); // first burst at once
        }
    }
    if !show.active {
        return;
    }
    if show.remaining.tick(time.delta()).finished() {
        show.active = false;
        return;
    }
    if !show.next.tick(time.delta()).finished() {
        return;
    }
    let interval = if show.perfect {
        FIREWORKS_BURST_SECS_PERFECT
    } else {
        FIREWORKS_BURST_SECS
    };
    show.next = Timer::from_seconds(interval, TimerMode::Once);

    // A launch point high over the outfield, spread across the field.
    let s = time.elapsed_secs();
    let center = Vec3::new(
        noise(s * 3.1) * 26.0,
        13.0 + hash01(s * 5.7) * 8.0,
        42.0 + hash01(s * 2.3) * 34.0,
    );
    let palette = &assets.firework;
    let material =
        palette[(hash01(s * 7.9) * palette.len() as f32) as usize % palette.len()].clone();
    let sparks = if show.perfect {
        FIREWORKS_SPARKS_PERFECT
    } else {
        FIREWORKS_SPARKS
    };
    for i in 0..sparks {
        let seed = s * 17.3 + i as f32 * 1.618;
        let dir = Vec3::new(noise(seed), noise(seed * 1.3), noise(seed * 1.7)).normalize_or_zero();
        commands.spawn((
            Particle {
                vel: dir * (5.0 + hash01(seed * 2.1) * 4.0),
                timer: Timer::from_seconds(0.9, TimerMode::Once),
                gravity: 3.0,
                grow: -1.0,
            },
            FireworkSpark,
            GameplayEntity,
            Mesh3d(assets.firework_mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(center),
        ));
    }
}

/// Threshold impact speed for a dust puff (m/s).
const DUST_MIN_SPEED: f32 = 4.0;

/// A puff of dirt wherever the ball thumps the ground.
pub(super) fn bounce_dust(
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
pub(super) fn tick_particles(
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
