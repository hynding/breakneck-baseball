//! Player entities — pitcher, batter, and fielders.
//!
//! Each player is a small rig (torso + head + cap + brim, the batter also a
//! bat) hanging off one kinematic Rapier body. Looks come from the active
//! [`Theme`]'s per-team [`crate::game::theme::PlayerTemplate`]s: a
//! [`TeamPalette`] of shared material handles is built at spawn, and
//! [`recolor_teams`] reassigns handles whenever the half-inning flips so the
//! defense always wears the fielding team's colours and the batter the
//! batting team's.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::animation::{bat_idle_rotation, AnimClip, LimbKind, MoveIntent, Playing, RigLimb};
use crate::game::ball::PLAYER_GROUP;
use crate::game::flow::{Phase, Play};
use crate::game::input::Intents;
use crate::game::theme::{PlayerTemplate, Theme};
use crate::game::variant::FieldSpec;
use crate::game::{GameState, GameplayEntity, ScoreBoard, Team};

// ── Roles & rig parts ─────────────────────────────────────────────────────────

/// Marker for the player controlling the pitcher.
#[derive(Component)]
pub struct Pitcher;

/// Marker for the player controlling the batter.
#[derive(Component)]
pub struct Batter;

/// Marker for a defensive fielder: the i-th spot in the field spec's
/// `fielder_positions`.
#[derive(Component)]
pub struct Fielder {
    pub index: usize,
}

/// Facing direction for the player model (world-space).
#[allow(dead_code)]
#[derive(Component, Default)]
pub struct FacingDirection(pub Vec3);

/// Whether a rig belongs to the defense (pitcher + fielders) or the batting
/// side (batter, runners) — decides which team's colours it wears as innings
/// flip.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RigUnit {
    Defense,
    Batter,
}

/// Which template material a rig child wears.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PartKind {
    Jersey,
    Cap,
    Skin,
    Bat,
}

/// Tag on every recolourable rig child.
#[derive(Component)]
struct RigPart {
    unit: RigUnit,
    part: PartKind,
}

// ── Team palette ──────────────────────────────────────────────────────────────

/// Shared material handles for one team's template.
pub(crate) struct RigMaterials {
    jersey: Handle<StandardMaterial>,
    cap: Handle<StandardMaterial>,
    skin: Handle<StandardMaterial>,
    bat: Handle<StandardMaterial>,
}

/// Both teams' rig materials, built once per game from the theme.
#[derive(Resource)]
pub(crate) struct TeamPalette {
    home: RigMaterials,
    away: RigMaterials,
}

impl TeamPalette {
    pub(crate) fn for_team(&self, team: Team) -> &RigMaterials {
        match team {
            Team::Home => &self.home,
            Team::Away => &self.away,
        }
    }
}

fn build_materials(
    materials: &mut Assets<StandardMaterial>,
    template: &PlayerTemplate,
) -> RigMaterials {
    let mut solid = |color: Color| {
        materials.add(StandardMaterial {
            base_color: color,
            perceptual_roughness: 0.8,
            ..default()
        })
    };
    RigMaterials {
        jersey: solid(template.jersey),
        cap: solid(template.cap),
        skin: solid(template.skin),
        bat: solid(template.bat),
    }
}

// ── Bat swing ─────────────────────────────────────────────────────────────────

/// Marks the batter's bat pivot — the entity the swing clips rotate.
#[derive(Component)]
pub(crate) struct BatPivot;

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Playing), spawn_players)
            .add_systems(
                Update,
                (recolor_teams, trigger_swing).run_if(in_state(GameState::Playing)),
            );
    }
}

// ── Spawning ──────────────────────────────────────────────────────────────────

/// Shared mesh handles for every rig, kept as a resource so other modules
/// (runners) can spawn rigs too.
#[derive(Resource)]
pub(crate) struct RigMeshes {
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    cap: Handle<Mesh>,
    brim: Handle<Mesh>,
    bat: Handle<Mesh>,
    limb: Handle<Mesh>,
}

/// Builds the team palette and every player rig for the current field.
fn spawn_players(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    field: Res<FieldSpec>,
    theme: Res<Theme>,
) {
    let palette = TeamPalette {
        home: build_materials(&mut materials, &theme.home),
        away: build_materials(&mut materials, &theme.away),
    };
    let rig_meshes = RigMeshes {
        torso: meshes.add(Capsule3d::new(0.3, 0.9)),
        head: meshes.add(Sphere::new(0.18)),
        cap: meshes.add(Cylinder::new(0.19, 0.09)),
        brim: meshes.add(Cuboid::new(0.26, 0.03, 0.16)),
        bat: meshes.add(Cylinder::new(0.032, 0.84)),
        limb: meshes.add(Cylinder::new(0.055, 0.5)),
    };

    // Top of the 1st: Away bats, Home fields.
    let defense = palette.for_team(Team::Home);
    let offense = palette.for_team(Team::Away);

    // Pitcher, standing on the rubber.
    let pitcher = spawn_rig(
        &mut commands,
        &rig_meshes,
        RigUnit::Defense,
        defense,
        Vec3::new(0.0, 0.6 + 0.25, field.pitch_distance),
        -1.0,
    );
    commands.entity(pitcher).insert(Pitcher);

    // Fielders at the spec's spots.
    for (index, spot) in field.fielder_positions.iter().enumerate() {
        let fielder = spawn_rig(
            &mut commands,
            &rig_meshes,
            RigUnit::Defense,
            defense,
            *spot + Vec3::Y * 0.6,
            -1.0,
        );
        commands.entity(fielder).insert(Fielder { index });
    }

    // Batter beside home plate, holding the bat on a swing pivot.
    let batter = spawn_rig(
        &mut commands,
        &rig_meshes,
        RigUnit::Batter,
        offense,
        Vec3::new(0.7, 0.6, 0.0),
        1.0,
    );
    commands.entity(batter).insert(Batter).with_children(|rig| {
        rig.spawn((
            BatPivot,
            Transform::from_translation(Vec3::new(-0.25, 0.45, 0.0))
                .with_rotation(bat_idle_rotation()),
            Visibility::default(),
        ))
        .with_children(|pivot| {
            pivot.spawn((
                RigPart {
                    unit: RigUnit::Batter,
                    part: PartKind::Bat,
                },
                Mesh3d(rig_meshes.bat.clone()),
                MeshMaterial3d(offense.bat.clone()),
                Transform::from_xyz(0.0, 0.46, 0.0),
            ));
        });
    });

    commands.insert_resource(palette);
    commands.insert_resource(rig_meshes);
}

/// Spawns one player rig (body + head + cap + brim + limbs) and returns the
/// parent entity so the caller can attach role markers or extras.
pub(crate) fn spawn_rig(
    commands: &mut Commands,
    meshes: &RigMeshes,
    unit: RigUnit,
    mats: &RigMaterials,
    position: Vec3,
    facing: f32,
) -> Entity {
    commands
        .spawn((
            GameplayEntity,
            FacingDirection(Vec3::Z * facing),
            Transform::from_translation(position).with_rotation(Quat::from_rotation_y(
                if facing < 0.0 {
                    std::f32::consts::PI
                } else {
                    0.0
                },
            )),
            Visibility::default(),
            RigidBody::KinematicPositionBased,
            Collider::capsule_y(0.6, 0.4),
            CollisionGroups::new(PLAYER_GROUP, Group::ALL),
            MoveIntent::default(),
        ))
        .with_children(|rig| {
            rig.spawn((
                RigPart {
                    unit,
                    part: PartKind::Jersey,
                },
                Mesh3d(meshes.torso.clone()),
                MeshMaterial3d(mats.jersey.clone()),
                Transform::default(),
            ));
            rig.spawn((
                RigPart {
                    unit,
                    part: PartKind::Skin,
                },
                Mesh3d(meshes.head.clone()),
                MeshMaterial3d(mats.skin.clone()),
                Transform::from_xyz(0.0, 0.78, 0.0),
            ));
            rig.spawn((
                RigPart {
                    unit,
                    part: PartKind::Cap,
                },
                Mesh3d(meshes.cap.clone()),
                MeshMaterial3d(mats.cap.clone()),
                Transform::from_xyz(0.0, 0.93, 0.0),
            ));
            rig.spawn((
                RigPart {
                    unit,
                    part: PartKind::Cap,
                },
                Mesh3d(meshes.brim.clone()),
                MeshMaterial3d(mats.cap.clone()),
                Transform::from_xyz(0.0, 0.9, 0.19),
            ));
            for (kind, x, y) in [
                (LimbKind::ArmL, 0.36, 0.30),
                (LimbKind::ArmR, -0.36, 0.30),
                (LimbKind::LegL, 0.14, -0.40),
                (LimbKind::LegR, -0.14, -0.40),
            ] {
                rig.spawn((
                    RigLimb { kind },
                    Transform::from_xyz(x, y, 0.0),
                    Visibility::default(),
                ))
                .with_children(|joint| {
                    joint.spawn((
                        RigPart {
                            unit,
                            part: PartKind::Jersey,
                        },
                        Mesh3d(meshes.limb.clone()),
                        MeshMaterial3d(mats.jersey.clone()),
                        Transform::from_xyz(0.0, -0.26, 0.0),
                    ));
                });
            }
        })
        .id()
}

// ── Systems ───────────────────────────────────────────────────────────────────

/// Dresses the defense in the fielding team's template and the batter in the
/// batting team's whenever the scoreboard changes (covers half-inning flips;
/// reassigning shared handles is cheap).
fn recolor_teams(
    score: Res<ScoreBoard>,
    palette: Option<Res<TeamPalette>>,
    mut parts: Query<(&RigPart, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    if !score.is_changed() {
        return;
    }
    let Some(palette) = palette else {
        return;
    };
    for (rig_part, mut material) in &mut parts {
        let team = match rig_part.unit {
            RigUnit::Defense => score.fielding_team(),
            RigUnit::Batter => score.batting_team(),
        };
        let mats = palette.for_team(team);
        material.0 = match rig_part.part {
            PartKind::Jersey => mats.jersey.clone(),
            PartKind::Cap => mats.cap.clone(),
            PartKind::Skin => mats.skin.clone(),
            PartKind::Bat => mats.bat.clone(),
        };
    }
}

/// Starts a swing when the batting side presses action during the duel —
/// humans and the CPU share the same `Intents`, so both animate.
fn trigger_swing(
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    pivots: Query<(Entity, Option<&Playing>), With<BatPivot>>,
    mut commands: Commands,
) {
    if !matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch) {
        return;
    }
    if !intents.get(score.batting_team()).action {
        return;
    }
    for (entity, playing) in &pivots {
        if playing.is_none() {
            commands
                .entity(entity)
                .insert(Playing::then(AnimClip::SwingBat, AnimClip::RecoverSwing));
        }
    }
}
