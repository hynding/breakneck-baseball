//! Playing-field geometry, spawned from the chosen [`FieldSpec`].
//!
//! Shared pieces (ground, bases, mound, lighting) are placed wherever the spec
//! says; the surroundings are dressed by the spec's [`Scenery`] routine —
//! a classic ballpark or a suburban front yard.
//!
//! **Standard field dimensions** (metric, matching real MLB proportions scaled
//! to Bevy world units where 1 unit ≈ 1 metre):
//!
//! | Feature                     | Real feet | Metres (≈) |
//! |-----------------------------|-----------|------------|
//! | Base-to-base                | 90 ft     | 27.43 m    |
//! | Home plate → pitcher's mound| 60.5 ft   | 18.44 m    |
//! | Home plate → centre-field   | 400 ft    | 121.9 m    |
//! | Foul lines (1B / 3B)        | 330 ft    | 100.6 m    |

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::variant::{FieldSpec, Scenery};
use crate::game::{GameState, GameplayEntity};

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

/// Marks a base object: `Some(i)` is the (0-indexed) i-th base in running
/// order, `None` is home plate.
#[allow(dead_code)]
#[derive(Component)]
pub struct Base {
    pub index: Option<usize>,
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
    field: Res<FieldSpec>,
) {
    match field.scenery {
        Scenery::Stadium => {
            spawn_stadium_ground(&mut commands, &mut meshes, &mut materials);
            spawn_stadium_mound(&mut commands, &mut meshes, &mut materials, &field);
            spawn_foul_poles(&mut commands, &mut meshes, &mut materials);
            spawn_outfield_wall(&mut commands, &mut meshes, &mut materials);
        }
        Scenery::FrontYard => {
            spawn_front_yard(&mut commands, &mut meshes, &mut materials, &field);
        }
    }
    spawn_bases(&mut commands, &mut meshes, &mut materials, &field);
    spawn_lighting(&mut commands);
}

/// The flat ground slab every scenery stands on (static collider for the ball).
fn spawn_ground_slab(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    color: Color,
) {
    let half_size = 150.0_f32;
    commands.spawn((
        GroundPlane,
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(
            half_size * 2.0,
            GROUND_HALF_DEPTH * 2.0,
            half_size * 2.0,
        ))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.9,
            ..default()
        })),
        Transform::from_xyz(0.0, -GROUND_HALF_DEPTH, 0.0),
        RigidBody::Fixed,
        Collider::cuboid(half_size, GROUND_HALF_DEPTH, half_size),
    ));
}

// ── Stadium scenery ───────────────────────────────────────────────────────────
fn spawn_stadium_ground(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    spawn_ground_slab(
        commands,
        meshes,
        materials,
        Color::srgb(0.18, 0.55, 0.18), // outfield green
    );

    // A lighter infield-dirt square rotated 45° to form the diamond shape.
    let infield_half = BASE_DISTANCE / std::f32::consts::SQRT_2;
    commands.spawn((
        GameplayEntity,
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
/// Home plate at the origin plus one bag per spec base position.
fn spawn_bases(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    field: &FieldSpec,
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

    let mut spawn = |index: Option<usize>, pos: Vec3, mesh: Handle<Mesh>, mat| {
        commands.spawn((
            Base { index },
            GameplayEntity,
            Mesh3d(mesh),
            MeshMaterial3d(mat),
            Transform::from_translation(pos),
            RigidBody::Fixed,
            Collider::cuboid(0.19, 0.025, 0.19),
        ));
    };

    spawn(None, Vec3::ZERO, home_mesh, home_material);
    for (i, pos) in field.base_positions.iter().enumerate() {
        spawn(Some(i), *pos, base_mesh.clone(), base_material.clone());
    }
}

// ── Pitcher's mound ───────────────────────────────────────────────────────────
fn spawn_stadium_mound(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    field: &FieldSpec,
) {
    commands.spawn((
        PitchersMound,
        GameplayEntity,
        Mesh3d(meshes.add(Cylinder::new(2.74, 0.25))), // 9 ft radius mound
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.76, 0.60, 0.42),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.125, field.pitch_distance),
        RigidBody::Fixed,
        Collider::cylinder(0.125, 2.74),
    ));
}

// ── Front-yard scenery ────────────────────────────────────────────────────────
/// Suburban lot scenery. Fleshed out with houses/street/hedges in the
/// front-yard milestone; the lawn and pitching mat are enough to play on.
fn spawn_front_yard(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    field: &FieldSpec,
) {
    spawn_ground_slab(
        commands,
        meshes,
        materials,
        Color::srgb(0.24, 0.52, 0.20), // lawn green
    );

    // A rubber pitching mat instead of a mound.
    commands.spawn((
        PitchersMound,
        GameplayEntity,
        Mesh3d(meshes.add(Cuboid::new(0.8, 0.04, 0.8))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.25, 0.28),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, 0.02, field.pitch_distance),
        RigidBody::Fixed,
        Collider::cuboid(0.4, 0.02, 0.4),
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
            GameplayEntity,
            Mesh3d(meshes.add(Cylinder::new(0.05, 15.0))),
            MeshMaterial3d(pole_material.clone()),
            Transform::from_xyz(sign * foul_line_distance, 7.5, foul_line_distance),
            RigidBody::Fixed,
            Collider::cylinder(7.5, 0.05),
        ));
    }
}

// ── Outfield wall ─────────────────────────────────────────────────────────────
/// A curved wall of flat panels spanning the fair-territory arc (foul pole to
/// foul pole), giving the outfield a visible boundary for home runs. Visual
/// only — no collider, so batted balls fly over it freely.
fn spawn_outfield_wall(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.13, 0.30, 0.55), // padded outfield-wall blue
        perceptual_roughness: 0.9,
        // A little self-illumination so the shadowed side still reads as a wall
        // rather than a black void.
        emissive: LinearRgba::rgb(0.02, 0.05, 0.10),
        ..default()
    });

    let radius = 112.0_f32;
    let height = 3.0_f32;
    let panels = 17;
    // Fair territory spans ±45° from the +Z (centre-field) axis.
    let span = std::f32::consts::FRAC_PI_2; // 90°
    let seg_angle = span / panels as f32;
    // Panel width slightly over the chord so neighbours overlap (no gaps).
    let panel_width = 2.0 * radius * (seg_angle / 2.0).sin() * 1.05;
    let panel_mesh = meshes.add(Cuboid::new(panel_width, height, 0.4));

    for i in 0..panels {
        let theta = -span / 2.0 + seg_angle * (i as f32 + 0.5);
        let pos = Vec3::new(radius * theta.sin(), height / 2.0, radius * theta.cos());
        commands.spawn((
            GameplayEntity,
            Mesh3d(panel_mesh.clone()),
            MeshMaterial3d(wall_material.clone()),
            Transform {
                translation: pos,
                rotation: Quat::from_rotation_y(theta),
                ..default()
            },
        ));
    }
}

// ── Lighting ──────────────────────────────────────────────────────────────────
fn spawn_lighting(commands: &mut Commands) {
    // Sunlight — angled to cast dramatic shadows.
    commands.spawn((
        GameplayEntity,
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
