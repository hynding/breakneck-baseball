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

use bevy::animation::graph::{AnimationGraph, AnimationNodeIndex};
use bevy::asset::embedded_asset;
use bevy::gltf::Gltf;
use bevy::prelude::*;

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

/// Polls until the Gltf is in, then builds the graph by CLIP_TABLE name
/// lookup (never by animation index — export order is not part of the
/// contract). Runs behind a run_if so it costs nothing once built.
fn build_rig_animations(
    mut commands: Commands,
    handle: Res<PlayerModelHandle>,
    gltfs: Res<Assets<Gltf>>,
    clips: Res<Assets<AnimationClip>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
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
    commands.insert_resource(RigAnimations {
        graph: graphs.add(graph),
        scene: gltf.scenes[0].clone(),
        jersey_material: gltf
            .named_materials
            .get(JERSEY_MATERIAL)
            .cloned()
            .unwrap_or_default(),
        cap_material: gltf
            .named_materials
            .get(CAP_MATERIAL)
            .cloned()
            .unwrap_or_default(),
        nodes,
        speeds,
    });
}

pub struct ModelAssetsPlugin;

impl Plugin for ModelAssetsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "models/player.glb");
        app.add_systems(Startup, load_player_model).add_systems(
            Update,
            build_rig_animations.run_if(|r: Option<Res<RigAnimations>>| r.is_none()),
        );
    }
}
