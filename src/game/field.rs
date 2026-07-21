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

use crate::game::rules;
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

/// Marks an outfield-wall panel — collision partners for wall-carom effects.
#[derive(Component)]
pub struct OutfieldWall;

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
    theme: Res<crate::game::theme::Theme>,
) {
    // The sky is theme data like everything else — a bright day or a night
    // game. Matters most from the catcher's-eye duel camera, which looks up
    // past the wall into nothing but clear colour.
    commands.insert_resource(ClearColor(theme.sky));
    match field.scenery {
        Scenery::Stadium => {
            spawn_stadium_ground(&mut commands, &mut meshes, &mut materials);
            spawn_stadium_mound(&mut commands, &mut meshes, &mut materials, &field);
            spawn_foul_poles(&mut commands, &mut meshes, &mut materials);
            spawn_outfield_wall(&mut commands, &mut meshes, &mut materials, &field);
        }
        Scenery::FrontYard => {
            spawn_front_yard(&mut commands, &mut meshes, &mut materials, &field);
        }
    }
    spawn_bases(&mut commands, &mut meshes, &mut materials, &field);
    // The sun sits behind home plate in both parks so everything the
    // broadcast and duel cameras look at — players' backs, house fronts, the
    // outfield — is lit rather than silhouetted; ambient keeps shadow sides
    // readable up close.
    match field.scenery {
        Scenery::Stadium => spawn_lighting(
            &mut commands,
            std::f32::consts::PI - std::f32::consts::FRAC_PI_6,
            0.25,
        ),
        Scenery::FrontYard => spawn_lighting(
            &mut commands,
            std::f32::consts::PI + std::f32::consts::FRAC_PI_6,
            0.35,
        ),
    }
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
/// Suburban lot: the batter hits from the lawn out across the street. All
/// dressing is visual-only (no colliders) so the analytic outcomes and the
/// ball's flight are never blocked; only the ground and the pitching mat are
/// physical.
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

    let mut flat = |size: Vec2, pos: Vec3, color: Color| {
        commands.spawn((
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(size.x, 0.004, size.y))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 1.0,
                ..default()
            })),
            Transform::from_translation(pos),
        ));
    };

    // The street runs the whole block, flanked by sidewalks.
    let asphalt = Color::srgb(0.32, 0.32, 0.34);
    let concrete = Color::srgb(0.62, 0.62, 0.60);
    flat(Vec2::new(300.0, 8.0), Vec3::new(0.0, 0.002, 26.0), asphalt);
    flat(
        Vec2::new(300.0, 0.3),
        Vec3::new(0.0, 0.004, 26.0),
        Color::srgb(0.85, 0.75, 0.2), // painted centre line
    );
    flat(Vec2::new(300.0, 2.0), Vec3::new(0.0, 0.002, 21.0), concrete);
    flat(Vec2::new(300.0, 2.0), Vec3::new(0.0, 0.002, 31.0), concrete);

    let mut block = |size: Vec3, pos: Vec3, color: Color| {
        commands.spawn((
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(size.x, size.y, size.z))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                perceptual_roughness: 0.9,
                ..default()
            })),
            Transform::from_translation(pos),
        ));
    };

    // Our house behind home plate, with a door and windows facing the yard.
    // Kept behind z = -12.5 so the broadcast camera (eye z = -12) never looks
    // through it.
    let siding = Color::srgb(0.78, 0.72, 0.58);
    let roof = Color::srgb(0.35, 0.22, 0.18);
    let trim = Color::srgb(0.30, 0.34, 0.42);
    block(
        Vec3::new(14.0, 5.0, 7.0),
        Vec3::new(0.0, 2.5, -16.5),
        siding,
    );
    block(Vec3::new(15.0, 1.2, 8.0), Vec3::new(0.0, 5.6, -16.5), roof);
    block(Vec3::new(1.4, 2.4, 0.2), Vec3::new(0.0, 1.2, -12.9), trim); // door
    block(Vec3::new(2.0, 1.4, 0.2), Vec3::new(-4.0, 2.6, -12.9), trim); // window
    block(Vec3::new(2.0, 1.4, 0.2), Vec3::new(4.0, 2.6, -12.9), trim); // window

    // The neighbours' houses across the street — clear those for a home run.
    let neighbour = Color::srgb(0.62, 0.68, 0.74);
    for x in [-22.0, 0.0, 22.0] {
        block(
            Vec3::new(14.0, 5.5, 7.0),
            Vec3::new(x, 2.75, 44.0),
            neighbour,
        );
        block(Vec3::new(15.0, 1.2, 8.0), Vec3::new(x, 6.2, 44.0), roof);
    }

    // Hedges along the lot lines.
    let hedge = Color::srgb(0.13, 0.35, 0.13);
    block(Vec3::new(0.8, 1.0, 16.0), Vec3::new(16.0, 0.5, 10.0), hedge);
    block(
        Vec3::new(0.8, 1.0, 16.0),
        Vec3::new(-16.0, 0.5, 10.0),
        hedge,
    );
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
/// Height of the outfield wall (m).
pub const WALL_HEIGHT: f32 = 3.0;

/// A curved wall of flat panels spanning the fair-territory arc, standing
/// exactly on the spec's home-run fence ([`rules::fence_at`] interpolates the
/// foul-line distance out to straightaway centre). Each panel is a fixed
/// collider, so live balls carom off it — a ball ruled a home run at contact
/// clears it in the air, and everything else plays off the wall.
fn spawn_outfield_wall(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    field: &FieldSpec,
) {
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.13, 0.30, 0.55), // padded outfield-wall blue
        perceptual_roughness: 0.9,
        // A little self-illumination so the shadowed side still reads as a wall
        // rather than a black void.
        emissive: LinearRgba::rgb(0.02, 0.05, 0.10),
        ..default()
    });

    let thickness = 0.4_f32;
    let panels = 24;
    let span = field.fair_half_angle * 2.0;
    // A point on the rules fence at angle `theta` off the centre-field axis.
    let fence_point = |theta: f32| {
        let dir = Vec3::new(theta.sin(), 0.0, theta.cos());
        dir * rules::fence_at(dir, field)
    };

    // The wall is the polyline through the fence points: each panel is the
    // chord between neighbours, so visual, physical, and home-run fences are
    // one and the same surface.
    for i in 0..panels {
        let t0 = -span / 2.0 + span * i as f32 / panels as f32;
        let t1 = -span / 2.0 + span * (i + 1) as f32 / panels as f32;
        let (p0, p1) = (fence_point(t0), fence_point(t1));
        let mid = (p0 + p1) / 2.0 + Vec3::Y * (WALL_HEIGHT / 2.0);
        let chord = p1 - p0;
        // Slightly over the chord so neighbouring panels overlap (no gaps).
        let width = chord.length() * 1.02;
        // Yaw that maps the panel's local +X onto the chord direction.
        let yaw = (-chord.z).atan2(chord.x);

        commands.spawn((
            OutfieldWall,
            GameplayEntity,
            Mesh3d(meshes.add(Cuboid::new(width, WALL_HEIGHT, thickness))),
            MeshMaterial3d(wall_material.clone()),
            Transform {
                translation: mid,
                rotation: Quat::from_rotation_y(yaw),
                ..default()
            },
            RigidBody::Fixed,
            Collider::cuboid(width / 2.0, WALL_HEIGHT / 2.0, thickness / 2.0),
            // Matches the ball's own restitution so its Min combine rule
            // keeps a lively carom instead of a dead drop.
            Restitution {
                coefficient: 0.55,
                combine_rule: CoefficientCombineRule::Min,
            },
        ));
    }
}

// ── Lighting ──────────────────────────────────────────────────────────────────
/// Sunlight angled to cast shadows, with the azimuth (`yaw`) chosen per
/// scenery, plus an ambient fill so shadows aren't pitch-black.
fn spawn_lighting(commands: &mut Commands, yaw: f32, ambient: f32) {
    commands.spawn((
        GameplayEntity,
        DirectionalLight {
            illuminance: 50_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(
            EulerRot::YXZ,
            yaw,
            -std::f32::consts::FRAC_PI_4,
            0.0,
        )),
    ));

    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: ambient,
    });
}
