//! Flags that show the wind (graphics plan task 12, built with task 13's
//! effects): one atop each foul pole and one over the scoreboard. They
//! stream downwind, flutter faster the harder it blows, and hang limp on a
//! calm day, so a Windy game reads before the first fly ball. Shown on the
//! tiers that budget `weather_fx` (Medium and up). Scenery only: the wind
//! they read is `rules::Wind`, which they never touch.

use bevy::prelude::*;

use crate::game::GameplayEntity;
use crate::game::quality::{ActiveQuality, QualityTier};
use crate::game::rules::{self, Wind};

/// Flag size (m): fly (length) and hoist (height).
pub(super) const FLAG_FLY: f32 = 4.2;
pub(super) const FLAG_HOIST: f32 = 2.6;
/// A wind this strong (mph) streams a flag straight out.
const FULL_STREAM_MPH: f32 = 20.0;
/// How far a flag droops (radians below level) in no wind at all.
const LIMP_DROOP: f32 = 1.25;

/// A flag, hoisted at `hoist` (the top of its pole); `phase` staggers the
/// flutter so the flags don't wave in step.
#[derive(Component)]
pub struct WindFlag {
    pub hoist: Vec3,
    phase: f32,
}

/// The shared look of every flag: a mesh whose hoist edge is its local
/// origin, and a team-neutral cloth.
pub(super) struct FlagKit {
    mesh: Handle<Mesh>,
    cloth: Handle<StandardMaterial>,
    pole: Handle<StandardMaterial>,
}

impl FlagKit {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        // A rectangle shifted so its top-left (hoist) corner is the origin:
        // the pose below rotates it about the halyard.
        let mesh = Rectangle::new(FLAG_FLY, FLAG_HOIST)
            .mesh()
            .build()
            .translated_by(Vec3::new(FLAG_FLY / 2.0, -FLAG_HOIST / 2.0, 0.0));
        Self {
            mesh: meshes.add(mesh),
            cloth: materials.add(StandardMaterial {
                base_color: Color::srgb(0.85, 0.12, 0.14),
                perceptual_roughness: 0.9,
                double_sided: true,
                cull_mode: None,
                ..default()
            }),
            pole: materials.add(StandardMaterial {
                base_color: Color::srgb(0.75, 0.76, 0.78),
                ..default()
            }),
        }
    }

    /// A flag at `hoist`, hidden until [`flags_follow_budget`] shows it.
    pub fn spawn_flag(&self, commands: &mut Commands, hoist: Vec3, phase: f32) {
        commands.spawn((
            WindFlag { hoist, phase },
            GameplayEntity,
            Mesh3d(self.mesh.clone()),
            MeshMaterial3d(self.cloth.clone()),
            Transform::from_translation(hoist),
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
        ));
    }

    /// A short pole standing on `foot` with a flag at its top.
    pub fn spawn_flagpole(
        &self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        foot: Vec3,
        height: f32,
        phase: f32,
    ) {
        commands.spawn((
            GameplayEntity,
            Mesh3d(meshes.add(Cylinder::new(0.08, height))),
            MeshMaterial3d(self.pole.clone()),
            Transform::from_translation(foot + Vec3::Y * (height / 2.0)),
            bevy::light::NotShadowCaster,
        ));
        self.spawn_flag(commands, foot + Vec3::Y * height, phase);
    }
}

/// How hard the wind streams a flag: 0 in still air, 1 at
/// [`FULL_STREAM_MPH`] and above.
pub(super) fn stream(wind: Vec3) -> f32 {
    let mph = Vec2::new(wind.x, wind.z).length() / rules::MPS_PER_MPH;
    (mph / FULL_STREAM_MPH).clamp(0.0, 1.0)
}

/// A flag's rotation at time `t` (s) in `wind` (the air's velocity, m/s):
/// its fly points downwind, drooping toward the pole as the wind drops,
/// with a flutter that quickens and widens in a stronger wind.
pub(super) fn flag_pose(wind: Vec3, t: f32, phase: f32) -> Quat {
    let s = stream(wind);
    let flat = Vec2::new(wind.x, wind.z);
    // Local +X (the fly) turned onto the wind's heading; a still day keeps
    // the authored heading.
    let heading = if flat.length_squared() > 1e-6 {
        (-flat.y).atan2(flat.x)
    } else {
        0.0
    };
    let flutter = (t * (2.0 + 6.0 * s) + phase).sin() * (0.05 + 0.25 * s);
    let droop = (1.0 - s) * LIMP_DROOP + (t * 1.3 + phase).sin() * 0.04;
    Quat::from_rotation_y(heading + flutter) * Quat::from_rotation_z(-droop)
}

/// Every frame, each shown flag takes its pose in the game's wind.
pub(super) fn wave_flags(
    wind: Option<Res<Wind>>,
    time: Res<Time>,
    mut flags: Query<(&WindFlag, &mut Transform, &Visibility)>,
) {
    let wind = wind.map_or(Vec3::ZERO, |w| w.0);
    let t = time.elapsed_secs();
    for (flag, mut tf, visibility) in &mut flags {
        if *visibility == Visibility::Hidden {
            continue;
        }
        tf.translation = flag.hoist;
        tf.rotation = flag_pose(wind, t, flag.phase);
    }
}

/// Flags show on tiers that budget weather effects and hide on Low, and
/// follow a tier change mid-game. With no tier resolved (a bare harness)
/// they show, as the effects budget falls back to High.
pub(super) fn flags_follow_budget(
    active: Option<Res<ActiveQuality>>,
    mut flags: Query<(&mut Visibility, Ref<WindFlag>)>,
) {
    let changed = active.as_ref().is_some_and(|a| a.is_changed());
    let shown = active
        .as_ref()
        .map_or(QualityTier::High.budget(), |a| a.budget())
        .weather_fx;
    let want = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for (mut visibility, marker) in &mut flags {
        if (changed || marker.is_added()) && *visibility != want {
            *visibility = want;
        }
    }
}

#[cfg(test)]
#[path = "flags.test.rs"]
mod tests;
