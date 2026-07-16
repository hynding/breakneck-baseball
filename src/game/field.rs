//! Baseball field geometry.
//!
//! Spawns the playing surface — infield diamond, bases, pitcher's mound, foul
//! lines, outfield grass — all as static Rapier colliders so the ball can roll
//! and bounce on them correctly.
//!
//! **Field dimensions** (metric, matching real MLB proportions scaled to Bevy
//! world units where 1 unit ≈ 1 metre):
//!
//! | Feature                     | Real feet | Metres (≈) |
//! |-----------------------------|-----------|------------|
//! | Base-to-base                | 90 ft     | 27.43 m    |
//! | Home plate → pitcher's mound| 60.5 ft   | 18.44 m    |
//! | Home plate → centre-field   | 400 ft    | 121.9 m    |
//! | Foul lines (1B / 3B)        | 330 ft    | 100.6 m    |

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::GameState;

// ── Distances in metres ───────────────────────────────────────────────────────
/// Distance between consecutive bases (90 ft).
pub const BASE_DISTANCE: f32 = 27.43;
/// Home plate → pitching rubber (60.5 ft).
pub const PITCH_DISTANCE: f32 = 18.44;
/// Half the base-path diagonal, used to place second base along the Z axis.
pub const HALF_DIAGONAL: f32 = BASE_DISTANCE * std::f32::consts::SQRT_2 / 2.0;
/// Ground-plane thickness for the static field collider.
const GROUND_HALF_DEPTH: f32 = 0.1;

// ── Field-object marker components ───────────────────────────────────────────
/// Marks the entire playing-surface ground plane.
#[derive(Component)]
pub struct GroundPlane;

/// Marks a base object (first, second, third, or home plate).
#[allow(dead_code)]
#[derive(Component)]
pub struct Base {
    pub label: BaseLabel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseLabel {
    Home,
    First,
    Second,
    Third,
}

/// Marks the pitcher's mound.
#[derive(Component)]
pub struct PitchersMound;

/// Marks one of the four foul-line poles.
#[derive(Component)]
pub struct FoulPole;

// ── Plugin ────────────────────────────────────────────────────────────────────
pub struct FieldPlugin;

impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_field);
    }
}

// ── Systems ───────────────────────────────────────────────────────────────────
fn spawn_field(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    spawn_ground(&mut commands, &mut meshes, &mut materials);
    spawn_infield(&mut commands, &mut meshes, &mut materials);
    spawn_pitchers_mound(&mut commands, &mut meshes, &mut materials);
    spawn_foul_poles(&mut commands, &mut meshes, &mut materials);
    spawn_lighting(&mut commands);
}

// ── Ground plane ─────────────────────────────────────────────────────────────
fn spawn_ground(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    // A large flat quad that covers the whole playing area.
    let half_size = 150.0_f32;

    commands.spawn((
        GroundPlane,
        Mesh3d(meshes.add(Cuboid::new(
            half_size * 2.0,
            GROUND_HALF_DEPTH * 2.0,
            half_size * 2.0,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.55, 0.18), // outfield green
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -GROUND_HALF_DEPTH, 0.0),
        // Static physics body so the ball collides with the ground.
        RigidBody::Fixed,
        Collider::cuboid(half_size, GROUND_HALF_DEPTH, half_size),
    ));

    // A lighter infield-dirt square rotated 45° to form the diamond shape.
    let infield_half = BASE_DISTANCE / std::f32::consts::SQRT_2;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(infield_half * 2.0, 0.001, infield_half * 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.76, 0.60, 0.42), // dirt brown
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform {
            translation: Vec3::new(0.0, 0.001, HALF_DIAGONAL),
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
            ..default()
        },
    ));
}

// ── Bases ─────────────────────────────────────────────────────────────────────
fn spawn_infield(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let base_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        ..default()
    });
    let home_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.9, 0.9),
        perceptual_roughness: 0.8,
        ..default()
    });
    let base_mesh = meshes.add(Cuboid::new(0.38, 0.05, 0.38));
    let home_mesh = meshes.add(Cuboid::new(0.43, 0.03, 0.43));

    // Bases are arranged around the diamond, with home plate at the origin.
    // Z-positive points toward the outfield (centre field).
    let bases = [
        (
            BaseLabel::Home,
            Vec3::new(0.0, 0.0, 0.0),
            home_mesh.clone(),
            home_material.clone(),
        ),
        (
            BaseLabel::First,
            Vec3::new(BASE_DISTANCE, 0.0, BASE_DISTANCE),
            base_mesh.clone(),
            base_material.clone(),
        ),
        (
            BaseLabel::Second,
            Vec3::new(0.0, 0.0, BASE_DISTANCE * 2.0),
            base_mesh.clone(),
            base_material.clone(),
        ),
        (
            BaseLabel::Third,
            Vec3::new(-BASE_DISTANCE, 0.0, BASE_DISTANCE),
            base_mesh.clone(),
            base_material.clone(),
        ),
    ];

    for (label, pos, mesh, mat) in bases {
        commands.spawn((
            Base { label },
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(pos),
            RigidBody::Fixed,
            Collider::cuboid(0.19, 0.025, 0.19),
        ));
    }
}

// ── Pitcher's mound ───────────────────────────────────────────────────────────
fn spawn_pitchers_mound(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        PitchersMound,
        Mesh3d(meshes.add(Cylinder::new(2.74, 0.25))), // 9 ft radius mound
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.76, 0.60, 0.42),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.125, PITCH_DISTANCE),
        RigidBody::Fixed,
        Collider::cylinder(0.125, 2.74),
    ));
}

// ── Foul poles ────────────────────────────────────────────────────────────────
fn spawn_foul_poles(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let pole_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.84, 0.0), // gold
        metallic: 0.8,
        perceptual_roughness: 0.2,
        ..default()
    });

    let foul_line_distance = 100.6_f32; // ≈ 330 ft

    for sign in [-1.0_f32, 1.0_f32] {
        commands.spawn((
            FoulPole,
            Mesh3d(meshes.add(Cylinder::new(0.05, 15.0))),
            MeshMaterial3d(pole_material.clone()),
            Transform::from_xyz(sign * foul_line_distance, 7.5, foul_line_distance),
            RigidBody::Fixed,
            Collider::cylinder(7.5, 0.05),
        ));
    }
}

// ── Lighting ──────────────────────────────────────────────────────────────────
fn spawn_lighting(commands: &mut Commands) {
    // Sunlight — angled to cast dramatic shadows.
    commands.spawn((
        DirectionalLight {
            illuminance: 50_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(
            EulerRot::XYZ,
            -std::f32::consts::FRAC_PI_4,
            std::f32::consts::FRAC_PI_6,
            0.0,
        )),
    ));

    // Ambient fill so shadows aren't pitch-black.
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 0.15,
    });
}
