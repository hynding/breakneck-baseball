//! Pitch trail: style-tuned motes dropped behind the pitched ball.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::GameplayEntity;
use crate::game::ai::{hash01, noise};
use crate::game::ball::{Baseball, InFlight};
use crate::game::flow::{Phase, Play};
use crate::game::settings::{PitchTrailStyle, Settings};

use super::TrailAssets;

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
    last.is_none_or(|l| l.distance(pos) >= spacing)
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
pub(super) fn build_trail_assets(
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
#[allow(clippy::type_complexity)]
pub(super) fn pitch_trail(
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
pub(super) fn tick_trail(
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

#[cfg(test)]
mod tests {
    use super::*;

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
