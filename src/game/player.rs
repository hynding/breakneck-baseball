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

use crate::game::animation;
use crate::game::animation::{
    bat_idle_rotation, AnimClip, LimbKind, MoveIntent, Playing, RigBaseY, RigLimb,
};
use crate::game::ball::PLAYER_GROUP;
use crate::game::flow::{Phase, Play};
use crate::game::input::Intents;
use crate::game::jersey::attach_jerseys;
use crate::game::model_assets::{GltfJerseyMesh, GltfPart, GltfTeamMaterials};
use crate::game::roster::{PlayerIdentity, RosterRole, Rosters};
use crate::game::rules::BattingOrder;
use crate::game::theme::{PlayerModelId, PlayerTemplate, Theme};
use crate::game::variant::FieldSpec;
use crate::game::{GameState, GameplayEntity, ScoreBoard, Team};

// ── Roles & rig parts ─────────────────────────────────────────────────────────

/// Marker for the player controlling the pitcher.
#[derive(Component)]
pub struct Pitcher;

/// Marker for the player controlling the batter.
#[derive(Component)]
pub struct Batter;

/// Where the batter stands: the centre of his side of the right-handed
/// batter's box (+X of the plate, docs/BASEBALL.md). Public so the camera
/// framing tests can prove the duel shot really contains him.
pub const BATTER_STAND_X: f32 = 0.7;

/// The rig's authored height (tools/build_player.py) — spikes to helmet.
/// Public for the same framing tests.
pub const RIG_HEIGHT_M: f32 = 1.85;

/// Marker for a defensive fielder: the i-th spot in the field spec's
/// `fielder_positions`.
#[derive(Component)]
pub struct Fielder {
    pub index: usize,
}

/// The fielder stationed behind the plate (spawn spot at z < 0): crouches
/// through the pitch duel. Parks without one (the front yard) have none.
#[derive(Component)]
pub struct CatcherRole;

/// The umpire behind the plate, who crouches through the duel like the
/// catcher he's peering over.
#[derive(Component)]
pub struct PlateUmpire;

/// Facing direction for the player model (world-space).
#[allow(dead_code)]
#[derive(Component, Default)]
pub struct FacingDirection(pub Vec3);

/// Whether a rig belongs to the defense (pitcher + fielders) or the batting
/// side (batter, runners) — decides which team's colours it wears as innings
/// flip. Umpires wear their own blacks and never recolour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RigUnit {
    Defense,
    Batter,
    Umpire,
}

/// A rig root's team unit, readable by wiring/recolour systems.
#[derive(Component, Clone, Copy)]
pub struct RigUnitTag(pub RigUnit);

/// Marks a glTF rig root awaiting (or holding) its wired skeleton.
#[derive(Component)]
pub struct GltfRig;

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

/// The umpires' blacks — a fixed look, deliberately outside the theme's
/// team templates so the crew reads as neutral in every palette.
fn umpire_materials(materials: &mut Assets<StandardMaterial>) -> RigMaterials {
    build_materials(
        materials,
        &PlayerTemplate {
            jersey: Color::srgb(0.15, 0.16, 0.19),
            cap: Color::srgb(0.05, 0.05, 0.06),
            skin: Color::srgb(0.85, 0.66, 0.5),
            bat: Color::srgb(0.3, 0.3, 0.3),
        },
    )
}

// ── Bat swing ─────────────────────────────────────────────────────────────────

/// Marks the batter's bat pivot — the entity the swing clips rotate.
#[derive(Component)]
pub(crate) struct BatPivot;

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::game::game_start(), spawn_players)
            .add_systems(
                Update,
                (
                    recolor_teams,
                    recolor_gltf,
                    // Reads `PlayerIdentity` to resolve the batter's personal
                    // stance — must see the same-frame stamp on the exact
                    // frame a new batter steps up (the `dress_jerseys`
                    // pattern in jersey.rs).
                    batter_stance.after(IdentitySet),
                    // Also reads `PlayerIdentity` for the fidget lookup;
                    // chained after `batter_stance` so it always sees this
                    // frame's freshly (re)resolved stance/cut before
                    // deciding whether to break it.
                    batter_fidgets.after(IdentitySet),
                    trigger_swing,
                    catcher_crouch,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// Holds the plate batter (the `Batter`-marker rig) in his personal batting
/// stance through the duel — resolved from `PlayerIdentity` via
/// `Rosters::team(..).card(..).appearance.style.stance` and
/// `animation::stance_clip`, falling back to the shared `AnimClip::BattingStance`
/// when the batter has no identity yet — the mirror of [`catcher_crouch`] on
/// the other side of the plate — and releases it the moment the ball is in
/// play, so the swap to the run-out rig (or a return to `Idle` between
/// at-bats) can take over. Runs before [`trigger_swing`] (via `.chain()`) so
/// a swing pressed on the very first duel frame still lands: even if this
/// system's insert reaches the batter first, `trigger_swing`'s widened gate
/// immediately replaces the held stance with `BatterSwing` the same frame.
/// Registered `.after(IdentitySet)` so it never reads a stale identity on the
/// exact frame a new batter steps up (the established `dress_jerseys`
/// pattern in jersey.rs).
///
/// Also owns cutting a fidget short: `batter_fidgets` only *starts* one in
/// `Phase::PrePitch`, but an 0.8–0.9 s fidget clip started late in PrePitch
/// can still be mid-flight when the duel moves to `WindUp`/`Pitch` — and a
/// fidget clip left in place there would both violate "fidgets exist only in
/// PrePitch" and (via `trigger_swing`'s stance-only gate) eat a real swing
/// press for up to 0.9 s. So whenever the batter is dueling past PrePitch, or
/// not dueling at all, any fidget in flight is force-replaced with the
/// resolved stance (dueling) or dropped outright (`!dueling`), mirroring the
/// stance-removal arm below.
fn batter_stance(
    play: Res<Play>,
    identities: Query<&PlayerIdentity>,
    rosters: Res<Rosters>,
    batters: Query<(Entity, Option<&Playing>), With<Batter>>,
    mut commands: Commands,
) {
    let dueling = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    let past_pre_pitch = matches!(play.phase, Phase::WindUp | Phase::Pitch);
    for (entity, playing) in &batters {
        let resolved = identities
            .get(entity)
            .map(|id| rosters.team(id.team).card(id.index).appearance.style.stance)
            .map(animation::stance_clip)
            .unwrap_or(AnimClip::BattingStance);
        match playing {
            None if dueling => {
                commands.entity(entity).insert(Playing::new(resolved));
            }
            // Identity changed mid-duel (a new at-bat that never passed
            // through a non-dueling frame): swap the stale stance for the
            // new batter's own.
            Some(playing)
                if dueling && animation::is_stance(playing.clip) && playing.clip != resolved =>
            {
                commands.entity(entity).insert(Playing::new(resolved));
            }
            Some(playing) if !dueling && animation::is_stance(playing.clip) => {
                commands.entity(entity).remove::<Playing>();
            }
            // Continuation cut: a fidget still playing once the windup
            // starts settles straight into the stance it was chained back
            // to anyway.
            Some(playing) if past_pre_pitch && animation::is_fidget(playing.clip) => {
                commands.entity(entity).insert(Playing::new(resolved));
            }
            // Continuation cut, dead-ball side: a fidget surviving into a
            // non-dueling frame (e.g. the ball going live mid-fidget) is
            // just dropped, same as a held stance would be.
            Some(playing) if !dueling && animation::is_fidget(playing.clip) => {
                commands.entity(entity).remove::<Playing>();
            }
            _ => {}
        }
    }
}

/// Between pitches a batter with an authored fidget occasionally breaks his
/// stance hold — helmet tap, practice half-swing — then settles back into it
/// (`Playing::then`). Dead-ball only: `Phase::PrePitch`, never during the
/// steal window (the duel there is gameplay-legible timing), and only while
/// he's actually holding a stance — `batter_stance`'s continuation-cut arm
/// owns getting any in-flight fidget back out before the windup, so this
/// system never has to worry about one surviving past `PrePitch`. Cadence is
/// deterministic hash noise (`ai::hash01`, the ai.rs convention): the seed
/// mixes the inning, the batter's 1-based lineup slot, and his roster index,
/// so the drawn interval — 4..9 s per at-bat-slot — varies between at-bats
/// but never between runs (replays and tests stay reproducible).
/// `FidgetsDisabled` (the scripted e2e harness default) suppresses it
/// outright. Registered `.after(IdentitySet)` — it reads `PlayerIdentity`.
#[allow(clippy::too_many_arguments)]
fn batter_fidgets(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    time: Res<Time>,
    disabled: Option<Res<animation::FidgetsDisabled>>,
    mut since_stance: Local<f32>,
    batters: Query<(Entity, &PlayerIdentity, Option<&Playing>), With<Batter>>,
    mut commands: Commands,
) {
    if disabled.is_some() || play.phase != Phase::PrePitch || play.in_steal_window() {
        *since_stance = 0.0;
        return;
    }
    let Ok((entity, id, playing)) = batters.get_single() else {
        return;
    };
    let Some(playing) = playing else {
        *since_stance = 0.0;
        return;
    };
    if !animation::is_stance(playing.clip) {
        *since_stance = 0.0;
        return;
    }
    let card = rosters.team(id.team).card(id.index);
    let Some(fidget) = card.appearance.style.fidget else {
        return;
    };
    *since_stance += time.delta_secs();
    // Deterministic per-at-bat interval in [4, 9): hash the inning, the
    // batter's lineup slot, and his roster index so it varies between
    // at-bats but never between runs.
    let seed =
        score.inning as f32 * 31.0 + order.current(id.team) as f32 * 7.0 + id.index as f32 * 13.0;
    let interval = 4.0 + 5.0 * crate::game::ai::hash01(seed);
    if *since_stance >= interval {
        *since_stance = 0.0;
        commands.entity(entity).insert(Playing::then(
            animation::fidget_clip(fidget),
            animation::stance_clip(card.appearance.style.stance),
        ));
    }
}

/// Holds the catcher (and the plate umpire peering over him) in the
/// receiving crouch through the duel, and releases the stance the moment the
/// ball is in play so coverage can take over.
#[allow(clippy::type_complexity)]
fn catcher_crouch(
    play: Res<Play>,
    catchers: Query<(Entity, Option<&Playing>), Or<(With<CatcherRole>, With<PlateUmpire>)>>,
    mut commands: Commands,
) {
    let dueling = matches!(play.phase, Phase::PrePitch | Phase::WindUp | Phase::Pitch);
    for (entity, playing) in &catchers {
        match playing {
            None if dueling => {
                commands
                    .entity(entity)
                    .insert(Playing::new(AnimClip::CatcherCrouch));
            }
            Some(playing) if !dueling && playing.clip == AnimClip::CatcherCrouch => {
                commands.entity(entity).remove::<Playing>();
            }
            _ => {}
        }
    }
}

// ── Spawning ──────────────────────────────────────────────────────────────────

/// Shared mesh handles for the Blocky rig, held inside [`RigModel::Blocky`].
pub struct RigMeshes {
    torso: Handle<Mesh>,
    head: Handle<Mesh>,
    cap: Handle<Mesh>,
    brim: Handle<Mesh>,
    bat: Handle<Mesh>,
    limb: Handle<Mesh>,
}

/// How rigs are constructed this game — replaces `RigMeshes` as the
/// resource; Blocky keeps its meshes inside the arm.
#[derive(Resource)]
pub enum RigModel {
    Blocky(RigMeshes),
    Gltf { scene: Handle<Scene> },
}

/// Builds the team palette and every player rig for the current field.
fn spawn_players(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    field: Res<FieldSpec>,
    theme: Res<Theme>,
    asset_server: Res<AssetServer>,
) {
    let palette = TeamPalette {
        home: build_materials(&mut materials, &theme.home),
        away: build_materials(&mut materials, &theme.away),
    };
    // The model seam: a future humanoid model is a new arm here (its own
    // meshes — and richer poses behind the same AnimClip names).
    let rig_model = match theme.player_model {
        PlayerModelId::Blocky => RigModel::Blocky(RigMeshes {
            torso: meshes.add(Capsule3d::new(0.3, 0.9)),
            head: meshes.add(Sphere::new(0.18)),
            cap: meshes.add(Cylinder::new(0.19, 0.09)),
            brim: meshes.add(Cuboid::new(0.26, 0.03, 0.16)),
            bat: meshes.add(Cylinder::new(0.032, 0.84)),
            limb: meshes.add(Cylinder::new(0.055, 0.5)),
        }),
        PlayerModelId::Gltf(_) => RigModel::Gltf {
            // Handle now, bytes async: the scene fills in when loaded, so
            // there is no menu-vs-load race to gate.
            scene: asset_server.load(
                bevy::gltf::GltfAssetLabel::Scene(0)
                    .from_asset(crate::game::model_assets::player_model_path()),
            ),
        },
    };
    let jersey_assets = crate::game::jersey::make_assets(&mut meshes, &mut materials);

    // Top of the 1st: Away bats, Home fields.
    let defense = palette.for_team(Team::Home);
    let offense = palette.for_team(Team::Away);

    // Pitcher, standing on the rubber.
    let pitcher = spawn_rig(
        &mut commands,
        &rig_model,
        RigUnit::Defense,
        defense,
        Vec3::new(0.0, 0.6 + 0.25, field.pitch_distance),
        -1.0,
    );
    commands
        .entity(pitcher)
        .insert(Pitcher)
        .insert(RosterRole::Pitcher);
    attach_jerseys(&mut commands, pitcher, &jersey_assets);

    // Fielders at the spec's spots.
    for (index, spot) in field.fielder_positions.iter().enumerate() {
        let fielder = spawn_rig(
            &mut commands,
            &rig_model,
            RigUnit::Defense,
            defense,
            *spot + Vec3::Y * 0.6,
            -1.0,
        );
        commands
            .entity(fielder)
            .insert(Fielder { index })
            .insert(RosterRole::Fielder(index));
        attach_jerseys(&mut commands, fielder, &jersey_assets);
        if spot.z < 0.0 {
            commands.entity(fielder).insert(CatcherRole);
        }
    }

    // The umpiring crew, in their own blacks (never recoloured). The plate
    // umpire faces the field from behind home; the rest face the plate.
    let umpire_mats = umpire_materials(&mut materials);
    for (i, spot) in field.umpire_positions.iter().enumerate() {
        let facing = if spot.z < 0.0 { 1.0 } else { -1.0 };
        let umpire = spawn_rig(
            &mut commands,
            &rig_model,
            RigUnit::Umpire,
            &umpire_mats,
            *spot + Vec3::Y * 0.6,
            facing,
        );
        if i == 0 && spot.z < 0.0 {
            commands.entity(umpire).insert(PlateUmpire);
        }
    }

    // Batter in the right-handed box beside the plate, holding the bat on a
    // swing pivot. He stands side-on to the pitcher, facing the plate (−X),
    // as a real batter does in the box (per docs/BASEBALL.md).
    let batter = spawn_rig(
        &mut commands,
        &rig_model,
        RigUnit::Batter,
        offense,
        Vec3::new(BATTER_STAND_X, 0.6, 0.0),
        1.0,
    );
    commands.entity(batter).insert(
        Transform::from_xyz(BATTER_STAND_X, 0.6, 0.0)
            .with_rotation(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2)),
    );
    attach_jerseys(&mut commands, batter, &jersey_assets);
    commands.insert_resource(jersey_assets);
    commands
        .entity(batter)
        .insert(Batter)
        .insert(RosterRole::Batter);
    // The bat is a mesh child only for Blocky — in glTF it's a bone under
    // the skeleton, wired up alongside the rest of the rig (Task 5).
    if let RigModel::Blocky(meshes) = &rig_model {
        commands.entity(batter).with_children(|rig| {
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
                    Mesh3d(meshes.bat.clone()),
                    MeshMaterial3d(offense.bat.clone()),
                    Transform::from_xyz(0.0, 0.46, 0.0),
                ));
            });
        });
    }

    commands.insert_resource(palette);
    commands.insert_resource(rig_model);
}

/// Spawns one player rig root and returns the entity so the caller can
/// attach role markers or extras. The [`RigModel`] decides the children: the
/// Blocky arm builds the body/head/cap/limb meshes verbatim, the Gltf arm
/// hangs a `SceneRoot` off the root (the skeleton wires up async — Task 5).
pub(crate) fn spawn_rig(
    commands: &mut Commands,
    model: &RigModel,
    unit: RigUnit,
    mats: &RigMaterials,
    position: Vec3,
    facing: f32,
) -> Entity {
    let root = commands
        .spawn((
            GameplayEntity,
            FacingDirection(Vec3::Z * facing),
            RigUnitTag(unit),
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
            RigBaseY(position.y),
        ))
        .id();
    match model {
        RigModel::Blocky(meshes) => {
            commands.entity(root).with_children(|rig| {
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
            });
        }
        RigModel::Gltf { scene } => {
            commands.entity(root).insert(GltfRig).with_children(|rig| {
                rig.spawn((
                    SceneRoot(scene.clone()),
                    // Model origin is at the feet; the root floats at
                    // collider-center height, so drop the scene to ground.
                    Transform::from_xyz(0.0, -position.y, 0.0),
                ));
            });
        }
    }
    root
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
            // Umpires keep their blacks through every half-inning.
            RigUnit::Umpire => continue,
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

/// glTF twin of [`recolor_teams`]: dresses tagged skinned meshes in the
/// current sides' tints; umpires get their blacks once and are skipped
/// after. Also fires for freshly-tagged meshes so a rig wired mid-inning
/// starts dressed instead of waiting for the next scoreboard change.
fn recolor_gltf(
    score: Res<ScoreBoard>,
    mats: Option<Res<GltfTeamMaterials>>,
    added: Query<(), Added<GltfJerseyMesh>>,
    mut meshes: Query<(&GltfJerseyMesh, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(mats) = mats else { return };
    if !score.is_changed() && added.is_empty() {
        return;
    }
    for (tag, mut material) in &mut meshes {
        let handle = match (tag.unit, tag.part) {
            (RigUnit::Umpire, GltfPart::Jersey) => mats.umpire_jersey.clone(),
            (RigUnit::Umpire, GltfPart::Cap) => mats.umpire_cap.clone(),
            (RigUnit::Defense, GltfPart::Jersey) => mats.jersey(score.fielding_team()),
            (RigUnit::Defense, GltfPart::Cap) => mats.cap(score.fielding_team()),
            (RigUnit::Batter, GltfPart::Jersey) => mats.jersey(score.batting_team()),
            (RigUnit::Batter, GltfPart::Cap) => mats.cap(score.batting_team()),
        };
        material.0 = handle;
    }
}

/// Starts a swing when the batting side presses action during the duel —
/// humans and the CPU share the same `Intents`, so both animate. The bat
/// pivot sweeps and the batter's arms drive through with it, so the swing
/// reads on the whole body, not just the bat.
#[allow(clippy::type_complexity)]
fn trigger_swing(
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    pivots: Query<(Entity, Option<&Playing>), With<BatPivot>>,
    batters: Query<(Entity, Option<&Playing>), (With<Batter>, Without<BatPivot>)>,
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
    for (entity, playing) in &batters {
        // A stance loop (shared or personal) always plays through the duel
        // now, so "nothing playing" is no longer the only swingable state:
        // replacing any held stance is fine (the driver's current-mismatch
        // restart cross-fades it away), but a `BatterSwing` already in
        // flight must never be interrupted by a second press.
        let swingable = match playing.map(|p| p.clip) {
            None => true,
            Some(c) => animation::is_stance(c),
        };
        if swingable {
            commands
                .entity(entity)
                .insert(Playing::new(AnimClip::BatterSwing));
        }
    }
}

/// Systems that stamp [`crate::game::roster::PlayerIdentity`] run in this
/// set; identity *consumers* order `.after(IdentitySet)` and get Bevy's
/// auto-inserted sync point, seeing the same-frame stamps. Promoted from a
/// bare `.chain()` per the Phase 1 review so new consumers join
/// declaratively.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentitySet;

/// Keeps every seated rig's [`crate::game::roster::PlayerIdentity`] matching
/// the live game: re-stamps on scoreboard flips (the defense becomes the
/// other team's nine), batting-order advances (the batter rig becomes the
/// next hitter), and roster rewrites (substitutions, dev file reloads).
/// Inserting is the change signal — `dress_jerseys` chains after this and
/// watches `Changed<PlayerIdentity>`. `pub(crate)` and registered from
/// `JerseyPlugin` (not here) so the two chain together with Bevy's
/// auto-inserted sync point.
pub(crate) fn sync_identities(
    score: Res<ScoreBoard>,
    order: Res<BattingOrder>,
    rosters: Res<Rosters>,
    mut commands: Commands,
    rigs: Query<(Entity, &RosterRole)>,
    added: Query<(), Added<RosterRole>>,
) {
    let refresh =
        score.is_changed() || order.is_changed() || rosters.is_changed() || !added.is_empty();
    if !refresh {
        return;
    }
    for (entity, role) in &rigs {
        commands
            .entity(entity)
            .insert(role.identity(&score, &order, &rosters));
    }
}
