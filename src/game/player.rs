//! Player entities — pitcher, batter, and fielders.
//!
//! Each player is a simple capsule mesh with a kinematic Rapier body.
//! The actual animation and AI can be layered on top of these components later.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::ball::PLAYER_GROUP;
use crate::game::variant::FieldSpec;
use crate::game::{GameState, GameplayEntity};

// ── Player roles ──────────────────────────────────────────────────────────────

/// Marker for the human/AI player controlling the pitcher.
#[derive(Component)]
pub struct Pitcher;

/// Marker for the human/AI player controlling the batter.
#[derive(Component)]
pub struct Batter;

/// Marker for a defensive fielder: the i-th spot in the field spec's
/// `fielder_positions` (how many there are — and where — is the variant's
/// call).
#[allow(dead_code)]
#[derive(Component)]
pub struct Fielder {
    pub index: usize,
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
    field: Res<FieldSpec>,
) {
    let mesh = player_capsule(&mut meshes);
    let mat = team_material(&mut materials, Color::srgb(0.9, 0.2, 0.2)); // red team

    commands.spawn((
        Pitcher,
        GameplayEntity,
        FacingDirection(Vec3::NEG_Z),
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        // Pitcher stands 0.6 m above the mound surface (capsule half-height)
        Transform::from_xyz(0.0, 0.6 + 0.25, field.pitch_distance),
        RigidBody::KinematicPositionBased,
        Collider::capsule_y(0.6, 0.4),
        CollisionGroups::new(PLAYER_GROUP, Group::ALL),
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
        GameplayEntity,
        FacingDirection(Vec3::Z),
        Mesh3d(mesh),
        MeshMaterial3d(mat),
        Transform::from_xyz(0.7, 0.6, 0.0),
        RigidBody::KinematicPositionBased,
        Collider::capsule_y(0.6, 0.4),
        CollisionGroups::new(PLAYER_GROUP, Group::ALL),
    ));
}

fn spawn_fielders(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    field: Res<FieldSpec>,
) {
    for (index, translation) in field.fielder_positions.iter().enumerate() {
        let mesh = player_capsule(&mut meshes);
        let mat = team_material(&mut materials, Color::srgb(0.9, 0.2, 0.2));

        commands.spawn((
            Fielder { index },
            GameplayEntity,
            FacingDirection(Vec3::NEG_Z),
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(*translation + Vec3::Y * 0.6),
            RigidBody::KinematicPositionBased,
            Collider::capsule_y(0.6, 0.4),
            CollisionGroups::new(PLAYER_GROUP, Group::ALL),
        ));
    }
}
