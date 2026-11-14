//! Rain: thin falling streaks in a box that travels with the main camera,
//! spawned at game start when the chosen weather rains
//! ([`crate::game::weather::Weather::Rain`]). Purely cosmetic, like the
//! rest of `fx` — the game's wet-day effect on play is the cool air
//! (`weather::Conditions::carry`), not the drops.

use bevy::prelude::*;

use crate::game::GameplayEntity;
use crate::game::camera::MainCamera;
use crate::game::settings::Settings;

/// Streaks in the box. Few enough to stay cheap on a phone's Low tier (one
/// shared mesh and material, no shadows), enough to read as rain.
pub(super) const DROPS: usize = 260;
/// The box around the camera the drops fall through (metres): half-width,
/// height, and how far ahead of the camera it reaches.
const HALF_WIDTH: f32 = 14.0;
const HEIGHT: f32 = 16.0;
const DEPTH: f32 = 30.0;
/// Fall speed (m/s): real raindrops fall at ~4–9 m/s; streaks read best fast.
const FALL_SPEED: f32 = 14.0;

/// One drop: its offset in the camera's box (camera-relative world axes,
/// so the streaks stay vertical whatever the camera does).
#[derive(Component)]
pub struct RainDrop {
    offset: Vec3,
}

/// A deterministic spread in 0..1 for drop `i`, axis `k` (no RNG needed for
/// scenery).
fn spread(i: usize, k: u32) -> f32 {
    let x = (i as u32)
        .wrapping_mul(2_654_435_761)
        .wrapping_add(k.wrapping_mul(40_503));
    let x = (x ^ (x >> 15)).wrapping_mul(2_246_822_519);
    (x ^ (x >> 13)) as f32 / u32::MAX as f32
}

/// Spawns the drops when the game's weather rains.
pub(super) fn spawn_rain(
    mut commands: Commands,
    settings: Option<Res<Settings>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let rains = settings.is_some_and(|s| s.weather.spec().rain);
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else {
        return;
    };
    if !rains {
        return;
    }
    let mesh = meshes.add(Cuboid::new(0.012, 0.55, 0.012));
    let material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.78, 0.84, 0.95, 0.45),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });
    for i in 0..DROPS {
        let offset = Vec3::new(
            (spread(i, 0) * 2.0 - 1.0) * HALF_WIDTH,
            spread(i, 1) * HEIGHT,
            spread(i, 2) * DEPTH,
        );
        commands.spawn((
            GameplayEntity,
            RainDrop { offset },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(offset),
            bevy::light::NotShadowCaster,
        ));
    }
}

/// Drops fall and wrap to the top of the box, which rides along in front of
/// the main camera.
pub(super) fn fall_rain(
    time: Res<Time>,
    camera: Query<&GlobalTransform, With<MainCamera>>,
    mut drops: Query<(&mut RainDrop, &mut Transform)>,
) {
    let Ok(cam) = camera.single() else {
        return;
    };
    let eye = cam.translation();
    // Flat forward, so the box sits ahead of the lens even when it pitches.
    let fwd = Vec3::new(cam.forward().x, 0.0, cam.forward().z).normalize_or(Vec3::Z);
    let right = Vec3::Y.cross(fwd).normalize_or(Vec3::X);
    let fall = FALL_SPEED * time.delta_secs();
    for (mut drop, mut tf) in &mut drops {
        drop.offset.y -= fall;
        if drop.offset.y < 0.0 {
            drop.offset.y += HEIGHT;
        }
        let base = eye - Vec3::Y * (HEIGHT * 0.5);
        tf.translation =
            base + right * drop.offset.x + Vec3::Y * drop.offset.y + fwd * (drop.offset.z + 3.0);
    }
}

#[cfg(test)]
#[path = "rain.test.rs"]
mod tests;
