//! The glTF player model: contract constants shared by the runtime loader
//! and the model-contract test. The loader/wiring systems land here too
//! (later tasks) — this module owns everything about the embedded `.glb`.

use crate::game::animation::AnimClip;

/// Repo-relative path of the committed model (used by the contract test and
/// the export script's output).
pub const PLAYER_GLB: &str = "src/game/models/player.glb";

/// Named material the recolour system re-tints per team.
pub const JERSEY_MATERIAL: &str = "JerseyBody";
/// Named cap material, also team-tinted.
pub const CAP_MATERIAL: &str = "Cap";

/// Bones gameplay attaches to (jersey lettering, the bat, future props).
pub const ATTACH_BONES: &[&str] = &["Hips", "Spine", "Head", "UpperArm.L", "UpperArm.R", "Bat"];

/// Budgets per docs/superpowers/specs/2026-07-24-gltf-player-models-design.md
/// §7 — ~18 skinned rigs at once on a WebGL2 floor.
pub const MAX_BONES: usize = 48;
pub const MAX_TRIANGLES: usize = 5_000;
pub const MAX_GLB_BYTES: usize = 512 * 1024;

/// AnimClip → baked clip name: the single source of truth for the runtime
/// graph AND the contract test, so the Rust enum and the Blender file can
/// only drift in ways that fail CI loudly. `SwingBat`/`RecoverSwing` are
/// absent by design — they alias `BatterSwing` via [`node_for`] (the bat is
/// a bone, so one clip covers body and bat).
pub const CLIP_TABLE: &[(AnimClip, &str)] = &[
    (AnimClip::Idle, "Idle"),
    (AnimClip::WindUp, "WindUp"),
    (AnimClip::ThrowRelease, "ThrowRelease"),
    (AnimClip::RunCycle, "RunCycle"),
    (AnimClip::ScoopBall, "ScoopBall"),
    (AnimClip::GloveUp, "GloveUp"),
    (AnimClip::CatcherCrouch, "CatcherCrouch"),
    (AnimClip::Dive, "Dive"),
    (AnimClip::Slide, "Slide"),
    (AnimClip::BatterSwing, "BatterSwing"),
];

/// Clips without their own baked action fold onto the one that covers them.
pub fn node_for(clip: AnimClip) -> AnimClip {
    match clip {
        AnimClip::SwingBat | AnimClip::RecoverSwing => AnimClip::BatterSwing,
        c => c,
    }
}

use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use bevy::asset::embedded_asset;
use bevy::gltf::Gltf;
use bevy::prelude::*;

use crate::game::player::{GltfRig, RigUnit, RigUnitTag};
use crate::game::theme::Theme;

/// Asset path the runtime loads. Task 8 adds a `dev`-feature arm that reads
/// the plain file for Blender hot-reload; release/wasm always embed.
pub fn player_model_path() -> &'static str {
    "embedded://breakneck_baseball/game/models/player.glb"
}

/// The whole-model Gltf handle, held so [`build_rig_animations`] can poll it.
#[derive(Resource)]
struct PlayerModelHandle(Handle<Gltf>);

/// Built once the Gltf loads: the shared graph, per-clip node indices and
/// speed factors, plus the handles spawn/recolour need.
#[derive(Resource)]
pub struct RigAnimations {
    pub graph: Handle<AnimationGraph>,
    pub scene: Handle<Scene>,
    pub jersey_material: Handle<StandardMaterial>,
    pub cap_material: Handle<StandardMaterial>,
    nodes: Vec<AnimationNodeIndex>, // parallel to CLIP_TABLE
    speeds: Vec<f32>,               // authored duration / AnimClip::duration()
}

impl RigAnimations {
    /// Graph node + playback speed for a clip (aliases resolve via
    /// [`node_for`]).
    pub fn node(&self, clip: AnimClip) -> (AnimationNodeIndex, f32) {
        let target = node_for(clip);
        let i = CLIP_TABLE
            .iter()
            .position(|(c, _)| *c == target)
            .expect("every AnimClip resolves into CLIP_TABLE via node_for");
        (self.nodes[i], self.speeds[i])
    }
}

fn load_player_model(asset_server: Res<AssetServer>, mut commands: Commands) {
    commands.insert_resource(PlayerModelHandle(asset_server.load(player_model_path())));
}

/// Team-tinted clones of the model's named materials, plus the umpires'
/// fixed blacks — mirrors player.rs's `TeamPalette` caching pattern, but for
/// the glTF rigs' shared skinned-mesh materials.
#[derive(Resource)]
pub struct GltfTeamMaterials {
    home_jersey: Handle<StandardMaterial>,
    home_cap: Handle<StandardMaterial>,
    away_jersey: Handle<StandardMaterial>,
    away_cap: Handle<StandardMaterial>,
    pub umpire_jersey: Handle<StandardMaterial>,
    pub umpire_cap: Handle<StandardMaterial>,
}

impl GltfTeamMaterials {
    pub fn jersey(&self, team: crate::game::Team) -> Handle<StandardMaterial> {
        match team {
            crate::game::Team::Home => self.home_jersey.clone(),
            crate::game::Team::Away => self.away_jersey.clone(),
        }
    }
    pub fn cap(&self, team: crate::game::Team) -> Handle<StandardMaterial> {
        match team {
            crate::game::Team::Home => self.home_cap.clone(),
            crate::game::Team::Away => self.away_cap.clone(),
        }
    }
}

/// Which recolourable model part a skinned-mesh entity is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GltfPart {
    Jersey,
    Cap,
}

/// Tag on skinned-mesh entities wearing a team-tintable material, resolved
/// during [`wire_rigs`] by comparing against the model's named material
/// handles (the scene spawner does not clone material handles per
/// instance — every rig's jersey mesh really does carry the same
/// `Handle<StandardMaterial>` the `Gltf` asset reports).
#[derive(Component)]
pub struct GltfJerseyMesh {
    pub unit: RigUnit,
    pub part: GltfPart,
}

/// Polls until the Gltf is in, then builds the graph by CLIP_TABLE name
/// lookup (never by animation index — export order is not part of the
/// contract). Runs behind a run_if so it costs nothing once built.
fn build_rig_animations(
    mut commands: Commands,
    handle: Res<PlayerModelHandle>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    theme: Res<Theme>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(gltf) = gltfs.get(&handle.0) else {
        return;
    };
    let mut handles = Vec::with_capacity(CLIP_TABLE.len());
    for (_, name) in CLIP_TABLE {
        let Some(h) = gltf.named_animations.get(*name) else {
            error!("player.glb is missing clip {name} — model contract violated");
            return;
        };
        handles.push(h.clone());
    }
    let (graph, nodes) = AnimationGraph::from_clips(handles.iter().cloned());
    let speeds = CLIP_TABLE
        .iter()
        .zip(&handles)
        .map(|((clip, _), h)| {
            let authored = clips
                .get(h)
                .map(|c| c.duration())
                .unwrap_or(clip.duration());
            authored / clip.duration()
        })
        .collect();
    let jersey_base = gltf
        .named_materials
        .get(JERSEY_MATERIAL)
        .cloned()
        .unwrap_or_default();
    let cap_base = gltf
        .named_materials
        .get(CAP_MATERIAL)
        .cloned()
        .unwrap_or_default();

    let tint = |materials: &mut Assets<StandardMaterial>,
                base: &Handle<StandardMaterial>,
                color: Color| {
        let mut m = materials.get(base).cloned().unwrap_or_default();
        m.base_color = color;
        materials.add(m)
    };
    let team_mats = GltfTeamMaterials {
        home_jersey: tint(&mut materials, &jersey_base, theme.home.jersey),
        home_cap: tint(&mut materials, &cap_base, theme.home.cap),
        away_jersey: tint(&mut materials, &jersey_base, theme.away.jersey),
        away_cap: tint(&mut materials, &cap_base, theme.away.cap),
        umpire_jersey: tint(&mut materials, &jersey_base, Color::srgb(0.15, 0.16, 0.19)),
        umpire_cap: tint(&mut materials, &cap_base, Color::srgb(0.05, 0.05, 0.06)),
    };
    commands.insert_resource(team_mats);

    commands.insert_resource(RigAnimations {
        graph: graphs.add(graph),
        scene: gltf.scenes[0].clone(),
        jersey_material: jersey_base,
        cap_material: cap_base,
        nodes,
        speeds,
    });
}

/// Handle from a rig root to its skeleton's [`AnimationPlayer`] entity, plus
/// which clip that player was last told to run. Presence = "wired".
#[derive(Component)]
pub struct RigPlayer {
    pub player: Entity,
    pub current: Option<AnimClip>,
}

/// Named bone entities gameplay attaches to (jersey quads, future props).
#[derive(Component)]
pub struct RigBones {
    pub spine: Entity,
    pub upper_arm_l: Entity,
    pub upper_arm_r: Entity,
    pub bat: Entity,
}

/// Finishes glTF rigs once their scene has instantiated: attaches the shared
/// graph + transitions to the skeleton's AnimationPlayer, resolves the
/// contract's named bones, and tags every skinned mesh wearing the model's
/// jersey/cap material with [`GltfJerseyMesh`] so [`recolor_gltf`] can dress
/// it. Retries each frame until the async scene lands (cheap: only unwired
/// rigs are visited, and only for a frame or two).
#[allow(clippy::type_complexity)]
fn wire_rigs(
    mut commands: Commands,
    anims: Option<Res<RigAnimations>>,
    unwired: Query<(Entity, &RigUnitTag), (With<GltfRig>, Without<RigPlayer>)>,
    children_q: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    names: Query<&Name>,
    mats_q: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    let Some(anims) = anims else {
        return;
    };
    for (root, unit_tag) in &unwired {
        let mut player = None;
        let (mut spine, mut ual, mut uar, mut bat) = (None, None, None, None);
        let mut jersey_meshes = Vec::new();
        let mut cap_meshes = Vec::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            if players.get(e).is_ok() {
                player = Some(e);
            }
            if let Ok(name) = names.get(e) {
                match name.as_str() {
                    "Spine" => spine = Some(e),
                    "UpperArm.L" => ual = Some(e),
                    "UpperArm.R" => uar = Some(e),
                    "Bat" => bat = Some(e),
                    _ => {}
                }
            }
            if let Ok(mat) = mats_q.get(e) {
                if mat.0 == anims.jersey_material {
                    jersey_meshes.push(e);
                } else if mat.0 == anims.cap_material {
                    cap_meshes.push(e);
                }
            }
            if let Ok(children) = children_q.get(e) {
                stack.extend(children.iter().copied());
            }
        }
        let (Some(player), Some(spine), Some(ual), Some(uar), Some(bat)) =
            (player, spine, ual, uar, bat)
        else {
            continue; // scene still instantiating — retry next frame
        };
        commands.entity(player).insert((
            AnimationGraphHandle(anims.graph.clone()),
            AnimationTransitions::new(),
        ));
        commands.entity(root).insert((
            RigPlayer {
                player,
                current: None,
            },
            RigBones {
                spine,
                upper_arm_l: ual,
                upper_arm_r: uar,
                bat,
            },
        ));
        let unit = unit_tag.0;
        for e in jersey_meshes {
            commands.entity(e).insert(GltfJerseyMesh {
                unit,
                part: GltfPart::Jersey,
            });
        }
        for e in cap_meshes {
            commands.entity(e).insert(GltfJerseyMesh {
                unit,
                part: GltfPart::Cap,
            });
        }
    }
}

pub struct ModelAssetsPlugin;

impl Plugin for ModelAssetsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "models/player.glb");
        app.add_systems(Startup, load_player_model)
            .add_systems(
                Update,
                build_rig_animations.run_if(|r: Option<Res<RigAnimations>>| r.is_none()),
            )
            .add_systems(
                Update,
                wire_rigs.run_if(in_state(crate::game::GameState::Playing)),
            );
    }
}
