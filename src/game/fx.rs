//! Game feel — hit-stop and impact particles. Purely cosmetic: nothing here
//! may touch the scoreboard, the bases, or the rules.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{CollisionEvent, Velocity};

use crate::game::ai::{hash01, noise};
use crate::game::ball::{
    Baseball, HitEvent, InFlight, WallBangEvent, BALL_DRAG_FACTOR, MAGNUS_FACTOR,
};
use crate::game::flow::{BallInPlayEvent, Phase, Play};
use crate::game::rules::{self, ContactKind, ContactQuality};
use crate::game::settings::{PitchTrailStyle, Settings};
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
    /// A fatter mote for fireworks, so the show reads from the outfield.
    firework_mesh: Handle<Mesh>,
    spark: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
    /// A small bright palette the fireworks pick from, burst by burst.
    firework: Vec<Handle<StandardMaterial>>,
}

fn build_fx_assets(
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

// ── Pitch trail ───────────────────────────────────────────────────────────────

/// Alpha rungs on the trail's fade ladder: motes step down pre-built
/// materials by age instead of allocating a material per mote per frame.
const TRAIL_FADE_STEPS: usize = 6;

/// One dropped element of the pitch trail. Ages out over the style's
/// lifetime; `seed` feeds the per-style hash-noise animation. Pub so the
/// e2e can count the trail without reaching into fx internals.
#[derive(Component)]
pub struct TrailMote {
    style: PitchTrailStyle,
    timer: Timer,
    seed: f32,
}

/// Meshes plus the fade ladder for the chosen colour — built per game start
/// from [`Settings`] (the settings screen only exists on the menu, so the
/// choice can't change mid-game).
#[derive(Resource)]
struct TrailAssets {
    style: PitchTrailStyle,
    mesh: Handle<Mesh>,
    /// `fade[k]` = the chosen colour at alpha rung `k` (0 = brightest).
    fade: Vec<Handle<StandardMaterial>>,
}

/// Style-tuned drop spacing: metres of ball travel per mote. Rings are
/// sparse gates to thread; the comet path is a dense ribbon.
fn trail_spacing(style: PitchTrailStyle) -> f32 {
    match style {
        PitchTrailStyle::Comet => 0.45,
        PitchTrailStyle::NeonRings => 2.2,
        _ => 0.8,
    }
}

/// Style-tuned mote lifetime (seconds) — how long the fade ladder takes.
fn trail_lifetime(style: PitchTrailStyle) -> f32 {
    match style {
        PitchTrailStyle::Comet => 0.45,
        PitchTrailStyle::NeonRings => 0.7,
        PitchTrailStyle::Bubbles => 0.9,
        _ => 0.6,
    }
}

/// Which fade rung an age fraction (0..1) sits on — clamped, monotonic.
fn fade_step(age_frac: f32, steps: usize) -> usize {
    ((age_frac.max(0.0) * steps as f32) as usize).min(steps - 1)
}

/// Distance-based drop test, so trail density is frame-rate independent
/// (and deterministic in the headless harness).
fn should_drop(last: Option<Vec3>, pos: Vec3, spacing: f32) -> bool {
    last.map_or(true, |l| l.distance(pos) >= spacing)
}

/// The style's mote mesh — all procedural primitives, no asset files (same
/// philosophy as the procedural audio/jerseys/field textures).
fn trail_mesh(style: PitchTrailStyle, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
    match style {
        PitchTrailStyle::Comet => meshes.add(Sphere::new(0.05)),
        PitchTrailStyle::Fireball => meshes.add(Cone {
            radius: 0.07,
            height: 0.16,
        }),
        PitchTrailStyle::Frostbite => meshes.add(Tetrahedron::default()),
        PitchTrailStyle::NeonRings => meshes.add(Torus {
            minor_radius: 0.015,
            major_radius: 0.16,
        }),
        PitchTrailStyle::Stardust => meshes.add(Sphere::new(0.04)),
        PitchTrailStyle::Bubbles => meshes.add(Sphere::new(0.06)),
    }
}

/// Builds the trail assets for the settings' chosen style and colour.
fn build_trail_assets(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    settings: &Settings,
) {
    let base = settings.trail_color.color();
    let fade = (0..TRAIL_FADE_STEPS)
        .map(|k| {
            let alpha = 0.9 * (1.0 - k as f32 / TRAIL_FADE_STEPS as f32);
            materials.add(StandardMaterial {
                base_color: base.with_alpha(alpha),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..default()
            })
        })
        .collect();
    commands.insert_resource(TrailAssets {
        style: settings.pitch_trail,
        mesh: trail_mesh(settings.pitch_trail, meshes),
        fade,
    });
}

/// Drops trail motes behind the pitched ball — the pitch's signature, not
/// the batted ball's, so it runs only while a pitch is on its way to the
/// plate. Cosmetic like every fx system: spawns motes, touches nothing.
fn pitch_trail(
    play: Res<Play>,
    assets: Option<Res<TrailAssets>>,
    ball_q: Query<(&Transform, &Velocity), (With<Baseball>, With<InFlight>)>,
    mut last_drop: Local<Option<Vec3>>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    if play.phase != Phase::Pitch {
        *last_drop = None;
        return;
    }
    let Ok((ball, vel)) = ball_q.get_single() else {
        return;
    };
    let pos = ball.translation;
    if !should_drop(*last_drop, pos, trail_spacing(assets.style)) {
        return;
    }
    *last_drop = Some(pos);
    let dir = vel.linvel.normalize_or_zero();
    // Rings face the flight so the ball threads them; everything else
    // spawns unrotated and lets its tick animation do the talking.
    let rotation = if assets.style == PitchTrailStyle::NeonRings {
        Quat::from_rotation_arc(Vec3::Y, dir)
    } else {
        Quat::IDENTITY
    };
    commands.spawn((
        TrailMote {
            style: assets.style,
            timer: Timer::from_seconds(trail_lifetime(assets.style), TimerMode::Once),
            seed: pos.z * 7.7 + pos.y * 3.1,
        },
        GameplayEntity,
        Mesh3d(assets.mesh.clone()),
        MeshMaterial3d(assets.fade[0].clone()),
        Transform::from_translation(pos).with_rotation(rotation),
    ));
}

/// Ages, animates, fades, and expires trail motes — per-style motion, all
/// deterministic hash noise on the mote's own seed.
fn tick_trail(
    time: Res<Time>,
    assets: Option<Res<TrailAssets>>,
    mut motes: Query<(
        Entity,
        &mut TrailMote,
        &mut Transform,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let dt = time.delta_secs();
    for (entity, mut mote, mut transform, mut material) in &mut motes {
        let frac = mote.timer.tick(time.delta()).fraction();
        let seed = mote.seed;
        // Bubbles pop before their fade completes; everything else rides
        // the ladder to the end.
        let done =
            mote.timer.finished() || (mote.style == PitchTrailStyle::Bubbles && frac >= 0.85);
        if done {
            commands.entity(entity).despawn();
            continue;
        }
        let rung = fade_step(frac, assets.fade.len());
        if material.0 != assets.fade[rung] {
            material.0 = assets.fade[rung].clone();
        }
        match mote.style {
            PitchTrailStyle::Comet => {
                transform.scale = Vec3::splat((1.0 - frac).max(0.05));
            }
            PitchTrailStyle::Fireball => {
                transform.translation.y += 0.8 * dt;
                let flicker = 1.0 + 0.3 * noise(seed + frac * 20.0);
                transform.scale = Vec3::splat(((1.0 - frac) * flicker).max(0.05));
            }
            PitchTrailStyle::Frostbite => {
                transform.translation.y -= 0.4 * dt;
                let spin = dt * (3.0 + 2.0 * hash01(seed));
                transform.rotate_local_y(spin);
                transform.rotate_local_x(spin * 0.7);
                transform.scale = Vec3::splat((0.09 * (1.0 - frac)).max(0.005));
            }
            PitchTrailStyle::NeonRings => {
                transform.scale = Vec3::splat(1.0 + 0.8 * frac);
            }
            PitchTrailStyle::Stardust => {
                transform.translation.y += 0.1 * dt;
                let twinkle = 0.6 + 0.4 * noise(seed * 3.0 + frac * 12.0).abs();
                transform.scale = Vec3::splat((twinkle * (1.0 - frac * 0.5)).max(0.05));
            }
            PitchTrailStyle::Bubbles => {
                transform.translation.y += 0.5 * dt;
                transform.translation.x += 0.3 * noise(seed + frac * 6.0) * dt;
                transform.scale = Vec3::splat(1.0 + 0.5 * frac);
            }
        }
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
struct Fireworks {
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
fn home_run_fireworks(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::settings::PitchTrailStyle;

    #[test]
    fn fade_step_walks_the_ladder_monotonically() {
        assert_eq!(fade_step(0.0, TRAIL_FADE_STEPS), 0);
        assert_eq!(fade_step(0.999, TRAIL_FADE_STEPS), TRAIL_FADE_STEPS - 1);
        // Out-of-range ages clamp instead of indexing off the ladder.
        assert_eq!(fade_step(1.5, TRAIL_FADE_STEPS), TRAIL_FADE_STEPS - 1);
        let mut prev = 0;
        for i in 0..=20 {
            let s = fade_step(i as f32 / 20.0, TRAIL_FADE_STEPS);
            assert!(s >= prev && s < TRAIL_FADE_STEPS);
            prev = s;
        }
    }

    #[test]
    fn trail_drops_by_distance_not_frame_rate() {
        assert!(
            should_drop(None, Vec3::ZERO, 0.5),
            "first mote drops immediately"
        );
        let last = Some(Vec3::ZERO);
        assert!(!should_drop(last, Vec3::new(0.0, 0.0, -0.3), 0.5));
        assert!(should_drop(last, Vec3::new(0.0, 0.0, -0.6), 0.5));
    }

    #[test]
    fn every_style_has_positive_spacing_and_lifetime() {
        for style in [
            PitchTrailStyle::Comet,
            PitchTrailStyle::Fireball,
            PitchTrailStyle::Frostbite,
            PitchTrailStyle::NeonRings,
            PitchTrailStyle::Stardust,
            PitchTrailStyle::Bubbles,
        ] {
            assert!(trail_spacing(style) > 0.0);
            assert!(trail_lifetime(style) > 0.0);
        }
    }
}

impl Plugin for FxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HitStop>()
            .init_resource::<Fireworks>()
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
                    home_run_fireworks,
                    bounce_dust,
                    update_landing_ring,
                    tick_particles,
                    pitch_trail,
                    tick_trail,
                )
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
