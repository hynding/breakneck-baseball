//! Player entities — pitcher, batter, and fielders.
//!
//! Each player is a simple capsule mesh with a kinematic Rapier body.
//! The actual animation and AI can be layered on top of these components later.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::field::{BASE_DISTANCE, PITCH_DISTANCE};
use crate::game::GameState;

// ── Player roles ──────────────────────────────────────────────────────────────

/// Marker for the human/AI player controlling the pitcher.
#[derive(Component)]
pub struct Pitcher;

/// Marker for the human/AI player controlling the batter.
#[derive(Component)]
pub struct Batter;

/// Marker for any defensive fielder (1B, 2B, SS, 3B, LF, CF, RF).
#[allow(dead_code)]
#[derive(Component)]
pub struct Fielder {
    pub position: FieldPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldPosition {
    FirstBase,
    SecondBase,
    ShortStop,
    ThirdBase,
    LeftField,
    CenterField,
    RightField,
    Catcher,
}

/// Facing direction for the player model (world-space).
#[allow(dead_code)]
#[derive(Component, Default)]
pub struct FacingDirection(pub Vec3);

// ── Plugin ────────────────────────────────────────────────────────────────────
pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            OnEnter(GameState::Playing),
            (spawn_pitcher, spawn_batter, spawn_fielders),
        );
    }
}

// ── Helper: shared player body mesh / material ────────────────────────────────
fn player_capsule(meshes: &mut ResMut<Assets<Mesh>>) -> Handle<Mesh> {
    meshes.add(Capsule3d::new(0.4, 1.2))
}

fn team_material(
    materials: &mut ResMut<Assets<StandardMaterial>>,
    color: Color,
) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: color,
        perceptual_roughness: 0.8,
        ..default()
    })
}

// ── Spawn systems ─────────────────────────────────────────────────────────────
fn spawn_pitcher(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = player_capsule(&mut meshes);
    let mat = team_material(&mut materials, Color::srgb(0.9, 0.2, 0.2)); // red team

    commands.spawn((
        Pitcher,
        FacingDirection(Vec3::NEG_Z),
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        // Pitcher stands 0.6 m above the mound surface (capsule half-height)
        Transform::from_xyz(0.0, 0.6 + 0.25, PITCH_DISTANCE),
        RigidBody::KinematicPositionBased,
        Collider::capsule_y(0.6, 0.4),
    ));
}

fn spawn_batter(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mesh = player_capsule(&mut meshes);
    let mat = team_material(&mut materials, Color::srgb(0.2, 0.3, 0.8)); // blue team

    // Batter stands just beside home plate (offset slightly to the right).
    commands.spawn((
        Batter,
        FacingDirection(Vec3::Z),
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        Transform::from_xyz(0.7, 0.6, 0.0),
        RigidBody::KinematicPositionBased,
        Collider::capsule_y(0.6, 0.4),
    ));
}

fn spawn_fielders(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let positions: &[(FieldPosition, Vec3)] = &[
        (FieldPosition::Catcher, Vec3::new(0.0, 0.0, -1.5)),
        (
            FieldPosition::FirstBase,
            Vec3::new(BASE_DISTANCE, 0.0, BASE_DISTANCE - 3.0),
        ),
        (
            FieldPosition::SecondBase,
            Vec3::new(7.0, 0.0, BASE_DISTANCE * 2.0 - 3.0),
        ),
        (
            FieldPosition::ShortStop,
            Vec3::new(-7.0, 0.0, BASE_DISTANCE * 2.0 - 3.0),
        ),
        (
            FieldPosition::ThirdBase,
            Vec3::new(-BASE_DISTANCE, 0.0, BASE_DISTANCE - 3.0),
        ),
        (FieldPosition::LeftField, Vec3::new(-40.0, 0.0, 85.0)),
        (FieldPosition::CenterField, Vec3::new(0.0, 0.0, 110.0)),
        (FieldPosition::RightField, Vec3::new(40.0, 0.0, 85.0)),
    ];

    for (pos, translation) in positions {
        let mesh = player_capsule(&mut meshes);
        let mat = team_material(&mut materials, Color::srgb(0.9, 0.2, 0.2));

        commands.spawn((
            Fielder { position: *pos },
            FacingDirection(Vec3::NEG_Z),
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(*translation + Vec3::Y * 0.6),
            RigidBody::KinematicPositionBased,
            Collider::capsule_y(0.6, 0.4),
        ));
    }
}
