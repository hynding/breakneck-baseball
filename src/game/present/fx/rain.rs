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

/// The mesh and material rain splashes share, present only in a rain game.
#[derive(Resource)]
pub(super) struct SplashAssets {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// Splashes per second at High, on tiers that budget `weather_fx`.
pub(super) const SPLASHES_PER_SEC: f32 = 70.0;
/// How long a splash ring lives (s).
const SPLASH_SECS: f32 = 0.28;
/// The patch of ground ahead of the camera the splashes land in (m):
/// half-width and depth.
const SPLASH_HALF_WIDTH: f32 = 9.0;
const SPLASH_DEPTH: f32 = 22.0;

/// Spawns the drops when the game's weather rains.
pub(super) fn spawn_rain(
    mut commands: Commands,
    settings: Option<Res<Settings>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    materials: Option<ResMut<Assets<StandardMaterial>>>,
) {
    commands.remove_resource::<SplashAssets>();
    let rains = settings.is_some_and(|s| s.weather.spec().rain);
    let (Some(mut meshes), Some(mut materials)) = (meshes, materials) else {
        return;
    };
    if !rains {
        return;
    }
    commands.insert_resource(SplashAssets {
        mesh: meshes.add(Cylinder::new(0.07, 0.004)),
        material: materials.add(StandardMaterial {
            base_color: Color::srgba(0.85, 0.90, 1.0, 0.4),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        }),
    });
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

/// A splash ring, tagged for the e2e check.
#[derive(Component)]
pub struct RainSplash;

/// How many splashes are due this frame: `per_sec` scaled by the tier,
/// carrying the fraction left over in `owed`.
pub(super) fn splashes_due(owed: &mut f32, per_sec: f32, scale: f32, dt: f32) -> usize {
    *owed += per_sec * scale * dt;
    let due = owed.floor();
    *owed -= due;
    due as usize
}

/// Raindrops splash on the ground ahead of the camera: small rings that
/// spread and fade, on tiers that budget weather effects (Medium and up).
pub(super) fn rain_splashes(
    assets: Option<Res<SplashAssets>>,
    camera: Query<&GlobalTransform, With<MainCamera>>,
    time: Res<Time>,
    mut owed: Local<f32>,
    mut count: Local<usize>,
    mut budget: super::budget::FxBudget,
    mut commands: Commands,
) {
    let Some(assets) = assets else { return };
    let b = budget.budget();
    if !b.weather_fx {
        return;
    }
    let Ok(cam) = camera.single() else { return };
    let due = splashes_due(
        &mut owed,
        SPLASHES_PER_SEC,
        b.particle_scale,
        time.delta_secs(),
    );
    if due == 0 {
        return;
    }
    // Already scaled by the tier: claim them at face value.
    let n = budget.allow_unscaled(due);
    let eye = cam.translation();
    let fwd = Vec3::new(cam.forward().x, 0.0, cam.forward().z).normalize_or(Vec3::Z);
    let right = Vec3::Y.cross(fwd).normalize_or(Vec3::X);
    for _ in 0..n {
        *count = count.wrapping_add(1);
        let i = *count;
        let at = eye.with_y(0.03)
            + right * ((spread(i, 7) * 2.0 - 1.0) * SPLASH_HALF_WIDTH)
            + fwd * (2.0 + spread(i, 8) * SPLASH_DEPTH);
        commands.spawn((
            super::particles::Particle::new(Vec3::ZERO, SPLASH_SECS, 0.0, 1.8),
            RainSplash,
            GameplayEntity,
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.material.clone()),
            Transform::from_translation(at),
            bevy::light::NotShadowCaster,
        ));
    }
}

#[cfg(test)]
#[path = "rain.test.rs"]
mod tests;
