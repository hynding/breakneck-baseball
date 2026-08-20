//! Stage spawn/teardown (ground, lights, camera, the one preview rig),
//! camera framing, preview clip selection, and team retinting.

use bevy::prelude::*;

use crate::game::Team;
use crate::game::animation::{self, Playing};
use crate::game::appearance::{CelebrationId, FidgetId, RosterDefs, StanceId};
use crate::game::jersey::{self, JerseyAssets};
use crate::game::model_assets::{GltfJerseyMesh, GltfPart, GltfTeamMaterials};
use crate::game::player::{self, RigUnit};
use crate::game::roster::Rosters;
use crate::game::theme::Theme;

use super::{
    CreatorState, CreatorTab, LastAppliedRoster, preview_rosters_and_identity, selected_def_ref,
};

/// Marks the one rig the Creator dresses/animates for preview. Carries
/// `Batter` too (see the module doc) so `wire_rigs` shows the bat submesh —
/// every tab the panel will grow implies a held bat.
#[derive(Component)]
pub struct PreviewRig;

/// Every entity spawned for the Creator stage (ground, lights, camera, the
/// preview rig) — despawned wholesale on exit and rebuilt fresh next entry,
/// mirroring `GameplayEntity`'s role for real games.
#[derive(Component)]
pub(super) struct CreatorStage;

/// Ground, lights, camera, and the one preview rig — built the same way
/// `player::spawn_players` builds a real one (`player::build_rig_model`,
/// `player::spawn_rig`), so the shared pipeline has exactly the components it
/// expects to find. Also loads `cs.working`/`cs.snapshot` fresh from the live
/// `RosterDefs` — every Creator entry starts with no unsaved edits, even if a
/// previous session left `working` mid-edit (reverted separately on exit,
/// but redundant safety here costs nothing).
#[allow(clippy::too_many_arguments)]
pub(super) fn enter_creator_stage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    theme: Res<Theme>,
    mut cs: ResMut<CreatorState>,
    defs: Res<RosterDefs>,
    mut last_applied: ResMut<LastAppliedRoster>,
    mut live_rosters: ResMut<Rosters>,
    jersey_assets: Option<Res<JerseyAssets>>,
    mut main_cameras: Query<&mut Camera, (With<Camera3d>, Without<CreatorStage>)>,
) {
    cs.working = defs.0.clone();
    cs.snapshot = defs.0.clone();
    cs.status.clear();
    // `apply_creator_edits` hasn't written anything yet this session — seed
    // its yardstick to match `defs.0` (== working == snapshot right now) so
    // `sync_creator_from_external_reload`'s first poll sees "nothing to do"
    // instead of mistaking a stale value left over from a previous Creator
    // visit for a fresh external reload.
    last_applied.0 = defs.0.clone();

    // The persistent main camera (`game::camera::spawn_camera`, active from
    // `Startup`) is still around while the Creator's own camera spawns
    // below. Two active `Camera3d`s at the same default order targeting the
    // primary window trip Bevy's `sort_cameras` order-ambiguity warning
    // ("Camera order ambiguities detected") and render unpredictably — stand
    // the main one down for the duration of the stage; `exit_creator_stage`
    // restores it. Filtered by `Without<CreatorStage>` (rather than by
    // spawn-order) so it's correct regardless of when our own camera below
    // is actually materialized.
    for mut camera in &mut main_cameras {
        camera.is_active = false;
    }

    // A small neutral stage — not the field's mown-stripe texture (that's
    // gameplay dressing this module has no business duplicating), just
    // somewhere flat to stand the preview rig.
    commands.spawn((
        CreatorStage,
        Mesh3d(meshes.add(Cuboid::new(12.0, 0.05, 12.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.25, 0.45, 0.25),
            perceptual_roughness: 0.95,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.025, 0.0),
    ));

    // Key / fill / rim, intensities to taste.
    commands.spawn((
        CreatorStage,
        PointLight {
            intensity: 1_500_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(2.0, 3.0, 2.2),
    ));
    commands.spawn((
        CreatorStage,
        PointLight {
            intensity: 500_000.0,
            ..default()
        },
        Transform::from_xyz(-2.4, 1.8, 1.6),
    ));
    commands.spawn((
        CreatorStage,
        PointLight {
            intensity: 350_000.0,
            ..default()
        },
        Transform::from_xyz(0.0, 1.6, -2.4),
    ));

    commands.spawn((
        CreatorStage,
        Camera3d::default(),
        Transform::from_xyz(1.8, 1.6, 2.6).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
    ));

    let rig_model = player::build_rig_model(&mut meshes, &asset_server, theme.player_model);
    let mats = match cs.team {
        Team::Home => player::build_materials(&mut materials, &theme.home),
        Team::Away => player::build_materials(&mut materials, &theme.away),
    };
    let rig = player::spawn_rig(
        &mut commands,
        &rig_model,
        RigUnit::Batter,
        &mats,
        Vec3::new(0.0, 0.6, 0.0),
        1.0,
    );
    // Same helper `apply_creator_edits` uses at runtime — the initial spawn
    // and every later edit go through one identity/roster computation, so a
    // selection that starts on the bench dresses correctly from frame one.
    let (rosters, id) = preview_rosters_and_identity(&cs.working, cs.team, cs.index);
    *live_rosters = rosters;
    commands
        .entity(rig)
        .insert((CreatorStage, PreviewRig, player::Batter, id));

    // `attach_jerseys` only needs the struct's mesh/material handles, not the
    // resource itself — reuse the game's if one already exists (a game was
    // played this session), otherwise build and install a fresh one so a
    // later real game start finds it ready.
    match jersey_assets {
        Some(assets) => jersey::attach_jerseys(&mut commands, rig, &assets),
        None => {
            let assets = jersey::make_assets(&mut meshes, &mut materials);
            jersey::attach_jerseys(&mut commands, rig, &assets);
            commands.insert_resource(assets);
        }
    }
}

/// Despawns the whole stage — the preview rig included, so the next entry
/// rebuilds it fresh rather than trying to reuse a stale one — and
/// reactivates the main camera `enter_creator_stage` stood down (see the
/// comment there on the order-ambiguity warning this pairing prevents).
pub(super) fn exit_creator_stage(
    mut commands: Commands,
    stage: Query<Entity, With<CreatorStage>>,
    mut main_cameras: Query<&mut Camera, (With<Camera3d>, Without<CreatorStage>)>,
) {
    for entity in &stage {
        commands.entity(entity).despawn_recursive();
    }
    for mut camera in &mut main_cameras {
        camera.is_active = true;
    }
}

/// Camera position + look-at target for a tab, per the brief's tuned framing:
/// Identity gets a full-body shot, Gear/Colors share a head close-up (both
/// tabs edit things worn on/near the head), Animations backs off to a
/// batter's-box-ish three-quarter so a stance/swing preview reads.
fn camera_target(tab: CreatorTab) -> (Vec3, Vec3) {
    match tab {
        CreatorTab::Identity => (Vec3::new(0.0, 1.1, 3.2), Vec3::new(0.0, 1.0, 0.0)),
        CreatorTab::Gear | CreatorTab::Colors => {
            (Vec3::new(0.35, 1.55, 1.1), Vec3::new(0.0, 1.5, 0.0))
        }
        CreatorTab::Animations => (Vec3::new(2.2, 1.4, 2.2), Vec3::new(0.0, 1.0, 0.0)),
    }
}

/// Eases the Creator camera toward the active tab's framing every frame
/// rather than cutting — an exponential approach
/// (`1 - (-8.0 * dt).exp()`, tuned by eye) so faster machines and slower
/// ones converge to the same target in the same wall-clock time regardless
/// of frame rate. Targets only translation + look-at rotation; the camera
/// never rolls.
pub(super) fn lerp_creator_camera(
    cs: Res<CreatorState>,
    time: Res<Time>,
    mut camera: Query<&mut Transform, (With<Camera3d>, With<CreatorStage>)>,
) {
    let Ok(mut transform) = camera.get_single_mut() else {
        return;
    };
    let (target_pos, look_at) = camera_target(cs.tab);
    let t = 1.0 - (-8.0 * time.delta_secs()).exp();
    let new_translation = transform.translation.lerp(target_pos, t);
    let target_rotation = Transform::from_translation(new_translation)
        .looking_at(look_at, Vec3::Y)
        .rotation;
    transform.translation = new_translation;
    transform.rotation = transform.rotation.slerp(target_rotation, t);
}

/// Which specific selection state a preview clip choice was last computed
/// from — compared each frame so [`preview_idle`] only (re)inserts `Playing`
/// on an actual change (tab, player, or a style field), never every frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct PreviewKey {
    team: Team,
    index: usize,
    tab: CreatorTab,
    stance: StanceId,
    fidget: Option<FidgetId>,
    celebration: CelebrationId,
}

/// Tab-aware preview clip: on Identity/Gear/Colors the rig holds the
/// player's resolved stance loop (livelier than a bare `Idle` and shows the
/// bat); on Animations the *selected* style element previews directly —
/// picking a stance loops it immediately, picking a fidget or celebration
/// plays it once (`Playing::then`) and returns to the stance loop, matching
/// how that element actually surfaces in a real at-bat. Re-triggers only on
/// a genuine selection change (tracked via [`PreviewKey`]), and only treats a
/// fidget/celebration change as "just selected" (one-shot preview) when the
/// player/tab stayed put — switching players or tabs always lands back on a
/// plain stance loop instead of replaying whatever that player's last-picked
/// fidget happened to be.
pub(super) fn preview_idle(
    cs: Res<CreatorState>,
    mut commands: Commands,
    mut last: Local<Option<PreviewKey>>,
    rig: Query<(Entity, Option<&Playing>), With<PreviewRig>>,
) {
    let Ok((entity, playing)) = rig.get_single() else {
        return;
    };
    let def = selected_def_ref(&cs.working, cs.team, cs.index);
    let key = PreviewKey {
        team: cs.team,
        index: cs.index,
        tab: cs.tab,
        stance: def.appearance.style.stance,
        fidget: def.appearance.style.fidget,
        celebration: def.appearance.style.celebration,
    };

    if Some(key) == *last && playing.is_some() {
        return;
    }

    let stance_clip = animation::stance_clip(key.stance);
    let steady_selection = last.is_some_and(|p| p.team == key.team && p.index == key.index);
    let new_playing = if key.tab == CreatorTab::Animations && steady_selection {
        let prev = last.expect("steady_selection implies last.is_some()");
        if let Some(fidget) = key.fidget.filter(|_| prev.fidget != key.fidget) {
            Playing::then(animation::fidget_clip(fidget), stance_clip)
        } else if prev.celebration != key.celebration {
            match animation::celebration_clip(key.celebration) {
                Some(clip) => Playing::then(clip, stance_clip),
                None => Playing::new(stance_clip),
            }
        } else {
            Playing::new(stance_clip)
        }
    } else {
        Playing::new(stance_clip)
    };

    *last = Some(key);
    commands.entity(entity).insert(new_playing);
}

/// Keeps the preview rig's jersey/cap uniform matching `CreatorState::team`.
/// `recolor_gltf` (the gameplay twin) keys off `ScoreBoard` and stays
/// `Playing`-gated, so without this the panel's team toggle would silently
/// fail to retint the uniform — only gear props would follow the team (they
/// take `team_mats.cap(team)` directly from `gear::dress_rigs`, which *does*
/// run here via `dressing_active`). Walks the preview rig's subtree every
/// frame rather than gating on `is_changed`: the glTF scene wires up
/// asynchronously, so `GltfJerseyMesh` tags can appear well after a
/// selection change already flipped `is_changed` back to false — a one-rig
/// walk is cheap enough that gating isn't worth the race.
pub(super) fn retint_preview(
    cs: Res<CreatorState>,
    mats: Option<Res<GltfTeamMaterials>>,
    rig: Query<Entity, With<PreviewRig>>,
    children_q: Query<&Children>,
    mut jerseys: Query<(&GltfJerseyMesh, &mut MeshMaterial3d<StandardMaterial>)>,
) {
    let Some(mats) = mats else { return };
    let Ok(root) = rig.get_single() else {
        return;
    };
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let Ok((tag, mut material)) = jerseys.get_mut(entity) {
            material.0 = match tag.part {
                GltfPart::Jersey => mats.jersey(cs.team),
                GltfPart::Cap => mats.cap(cs.team),
            };
        }
        if let Ok(children) = children_q.get(entity) {
            stack.extend(children.iter().copied());
        }
    }
}
