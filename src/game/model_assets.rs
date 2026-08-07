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
/// Named bat material — the exporter splits mesh primitives per material, so
/// the bat is its own child entity `wire_rigs` finds by handle equality.
pub const BAT_MATERIAL: &str = "Bat";
/// Named skin material — the per-player tint seam ([`RigSkinMeshes`]).
pub const SKIN_MATERIAL: &str = "Skin";

/// Bones gameplay attaches to (jersey lettering, the bat, future props).
pub const ATTACH_BONES: &[&str] = &[
    "Hips",
    "Spine",
    "Head",
    "UpperArm.L",
    "UpperArm.R",
    "LowerArm.L",
    "LowerArm.R",
    "Bat",
];

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
    (AnimClip::BattingStance, "BattingStance"),
    (AnimClip::StanceOpen, "StanceOpen"),
    (AnimClip::StanceClosed, "StanceClosed"),
    (AnimClip::StanceWaggle, "StanceWaggle"),
    (AnimClip::FidgetBatTap, "FidgetBatTap"),
    (AnimClip::FidgetHalfSwing, "FidgetHalfSwing"),
    (AnimClip::CelebrateBatFlip, "CelebrateBatFlip"),
];

/// Clips without their own baked action fold onto the one that covers them.
/// `SwingBat`/`RecoverSwing` keep aliasing `BatterSwing` here — the bat is a
/// bone under the same skeleton as the arms now, so the one baked action
/// still covers body and bat together; `BattingStance` gets its own row
/// instead of folding onto `BatterSwing` because it's a genuinely distinct
/// held pose (looping, no swing motion), not a body/bat split of one clip.
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

use crate::game::player::{Batter, GltfRig, RigUnit, RigUnitTag};
use crate::game::theme::Theme;

/// Asset path the runtime loads. Task 8 adds a `dev`-feature arm that reads
/// the plain file for Blender hot-reload; release/wasm always embed.
pub fn player_model_path() -> &'static str {
    if cfg!(feature = "dev") {
        // Served from the file-watched "src" asset root (see main.rs).
        "game/models/player.glb"
    } else {
        "embedded://breakneck_baseball/game/models/player.glb"
    }
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
    /// The bat submesh's material handle — `wire_rigs` compares every
    /// descendant against it to find and show/hide the batter's prop.
    pub bat_material: Handle<StandardMaterial>,
    /// The skin submesh's material handle — `wire_rigs` compares every
    /// descendant against it to collect [`RigSkinMeshes`] for per-player tint.
    pub skin_material: Handle<StandardMaterial>,
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

/// Keeps the baked [`GltfTeamMaterials`] in step with the active [`Theme`]:
/// [`build_rig_animations`] only ever bakes the four team materials once
/// (the moment the Gltf loads, typically on the main menu, against whatever
/// theme happened to be active then), so without this system cycling themes
/// with **T** — or simply loading with a non-default theme already
/// selected — left the glTF rigs permanently wearing the theme baked at
/// boot. Re-tinting the existing handles in place (rather than rebuilding
/// and reassigning them) means tagged meshes pick up the change for free —
/// no need to touch `GltfJerseyMesh`/[`crate::game::player::recolor_gltf`]
/// at all. Umpire blacks are theme-independent and are left alone.
fn retint_gltf_team_materials(
    theme: Res<Theme>,
    mats: Option<Res<GltfTeamMaterials>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !theme.is_changed() {
        return;
    }
    let Some(mats) = mats else {
        return;
    };
    let mut set = |handle: &Handle<StandardMaterial>, color: Color| {
        if let Some(m) = materials.get_mut(handle) {
            m.base_color = color;
        }
    };
    set(&mats.home_jersey, theme.home.jersey);
    set(&mats.home_cap, theme.home.cap);
    set(&mats.away_jersey, theme.away.jersey);
    set(&mats.away_cap, theme.away.cap);
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

/// Marker inserted once [`build_rig_animations`] hits a permanently missing
/// clip, so the `run_if` gate below stops retrying it: the Gltf asset is
/// already loaded and named_animations won't grow a clip on a later frame,
/// so without this the system would `error!` every single frame forever
/// instead of once.
#[derive(Resource)]
struct RigAnimationsFailed;

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
            commands.insert_resource(RigAnimationsFailed);
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
    let bat_material = gltf
        .named_materials
        .get(BAT_MATERIAL)
        .cloned()
        .unwrap_or_default();
    let skin_material = gltf
        .named_materials
        .get(SKIN_MATERIAL)
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
        bat_material,
        skin_material,
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
/// Note: Hips is in ATTACH_BONES (the model contract) but deliberately NOT
/// resolved here — no Phase 2 gear mounts to hips, and an unused resolved bone
/// is dead weight (YAGNI). Add hips when a prop actually needs it.
#[derive(Component)]
pub struct RigBones {
    pub spine: Entity,
    pub upper_arm_l: Entity,
    pub upper_arm_r: Entity,
    pub head: Entity,
    pub lower_arm_l: Entity,
    pub lower_arm_r: Entity,
    pub bat: Entity,
}

/// Skinned-mesh entities wearing the model's Skin material, per rig — the
/// per-player tint seam. Umpire rigs get one too but are never dressed (no
/// PlayerIdentity).
#[derive(Component)]
pub struct RigSkinMeshes(pub Vec<Entity>);

/// This rig's cap submeshes — headwear dressing shows/hides them per player
/// while team recolouring keeps owning their material.
#[derive(Component)]
pub struct RigCapMeshes(pub Vec<Entity>);

/// Finishes glTF rigs once their scene has instantiated: attaches the shared
/// graph + transitions to the skeleton's AnimationPlayer, resolves the
/// contract's named bones, tags every skinned mesh wearing the model's
/// jersey/cap material with [`GltfJerseyMesh`] so [`recolor_gltf`] can dress
/// it, collects the skin/cap submeshes onto the root as [`RigSkinMeshes`]/
/// [`RigCapMeshes`] for per-player dressing (`gear::dress_rigs`), and shows
/// the bat submesh only on the plate batter (the `Batter`
/// marker) — every other rig, including run-out rigs which also carry
/// `RigUnit::Batter`, hides it, since the bat is skinned into the shared mesh
/// and every instance would otherwise carry one. Retries each frame until
/// the async scene lands (cheap: only unwired rigs are visited, and only for
/// a frame or two).
#[allow(clippy::type_complexity)]
fn wire_rigs(
    mut commands: Commands,
    anims: Option<Res<RigAnimations>>,
    unwired: Query<(Entity, &RigUnitTag, Has<Batter>), (With<GltfRig>, Without<RigPlayer>)>,
    children_q: Query<&Children>,
    players: Query<(), With<AnimationPlayer>>,
    names: Query<&Name>,
    mats_q: Query<&MeshMaterial3d<StandardMaterial>>,
) {
    let Some(anims) = anims else {
        return;
    };
    for (root, unit_tag, is_batter) in &unwired {
        let mut player = None;
        let (mut spine, mut ual, mut uar, mut head, mut lal, mut lar, mut bat) =
            (None, None, None, None, None, None, None);
        let mut jersey_meshes = Vec::new();
        let mut cap_meshes = Vec::new();
        let mut bat_meshes = Vec::new();
        let mut skin_meshes = Vec::new();
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
                    "Head" => head = Some(e),
                    "LowerArm.L" => lal = Some(e),
                    "LowerArm.R" => lar = Some(e),
                    "Bat" => bat = Some(e),
                    _ => {}
                }
            }
            if let Ok(mat) = mats_q.get(e) {
                if mat.0 == anims.jersey_material {
                    jersey_meshes.push(e);
                } else if mat.0 == anims.cap_material {
                    cap_meshes.push(e);
                } else if mat.0 == anims.bat_material {
                    bat_meshes.push(e);
                } else if mat.0 == anims.skin_material {
                    skin_meshes.push(e);
                }
            }
            if let Ok(children) = children_q.get(e) {
                stack.extend(children.iter().copied());
            }
        }
        let (
            Some(player),
            Some(spine),
            Some(ual),
            Some(uar),
            Some(head),
            Some(lal),
            Some(lar),
            Some(bat),
        ) = (player, spine, ual, uar, head, lal, lar, bat)
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
                head,
                lower_arm_l: lal,
                lower_arm_r: lar,
                bat,
            },
            RigSkinMeshes(skin_meshes),
            RigCapMeshes(cap_meshes.clone()),
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
        let bat_visibility = if is_batter {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        for e in bat_meshes {
            commands.entity(e).insert(bat_visibility);
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
                build_rig_animations.run_if(
                    |built: Option<Res<RigAnimations>>,
                     failed: Option<Res<RigAnimationsFailed>>| {
                        built.is_none() && failed.is_none()
                    },
                ),
            )
            .add_systems(Update, retint_gltf_team_materials)
            .add_systems(
                Update,
                wire_rigs.run_if(in_state(crate::game::GameState::Playing)),
            );
    }
}
