//! Dev-only player-creation stage (`--features debug`).
//!
//! The whole point of the Creator is honesty: the preview rig it dresses is
//! wired, dressed, lettered, and animated by the *exact same* systems
//! gameplay uses — [`crate::game::dressing_active`] widens each of those
//! systems' run condition from `in_state(Playing)` to also cover
//! `GameState::Creator`, so nothing in the pipeline needs to know a preview
//! rig is not a real player. This module owns only what's specific to the
//! stage itself: entering/leaving it, the ground/lights/camera, spawning the
//! one preview rig, and the two things the shared pipeline doesn't already
//! cover for a rig with no `RosterRole` — the `Batter` marker (bat
//! visibility) and team-uniform retinting (`recolor_gltf` is
//! `ScoreBoard`-keyed and stays `Playing`-gated, so it never sees this rig).
//!
//! The panel itself (Tune-tab-style egui UI for picking team/slot/tab) lands
//! in Task 2; this task only proves the pipeline gating and the preview rig.

use bevy::prelude::*;

use crate::game::animation::{AnimClip, Playing};
use crate::game::jersey::{self, JerseyAssets};
use crate::game::model_assets::{GltfJerseyMesh, GltfPart, GltfTeamMaterials};
use crate::game::player::{self, RigUnit};
use crate::game::roster::PlayerIdentity;
use crate::game::settings::settings_closed;
use crate::game::theme::Theme;
use crate::game::{GameState, Team};

/// Which team/roster-slot the Creator is currently previewing. Grows in
/// Task 2 (tab, edit buffers, …) — this is the skeleton the shared dress
/// pipeline reacts to via [`PlayerIdentity`].
#[derive(Resource, Debug, Clone, Copy)]
pub struct CreatorState {
    pub team: Team,
    pub index: usize,
}

impl Default for CreatorState {
    fn default() -> Self {
        Self {
            team: Team::Home,
            index: 0,
        }
    }
}

/// Marks the one rig the Creator dresses/animates for preview. Carries
/// `Batter` too (see the module doc) so `wire_rigs` shows the bat submesh —
/// every tab the panel will grow implies a held bat.
#[derive(Component)]
pub struct PreviewRig;

/// Every entity spawned for the Creator stage (ground, lights, camera, the
/// preview rig) — despawned wholesale on exit and rebuilt fresh next entry,
/// mirroring `GameplayEntity`'s role for real games.
#[derive(Component)]
struct CreatorStage;

/// **C** on the main menu opens the Creator — gated on
/// [`settings_closed`] the same way `menu::cycle_options`/`menu::menu_select`
/// are, so C can't fire behind an open Settings screen. `KeyC` is also
/// `camera::toggle_camera_mode`'s duel-view toggle, but that system only
/// runs in `Playing` — no conflict with this MainMenu-only handler.
fn enter_creator(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if keyboard.just_pressed(KeyCode::KeyC) {
        next_state.set(GameState::Creator);
    }
}

/// Esc leaves the Creator. Keypress only — the actual teardown lives in
/// [`exit_creator_stage`] (`OnExit(Creator)`), decoupled from the keypress so
/// it fires on every exit path, not just this one.
fn exit_creator(keyboard: Res<ButtonInput<KeyCode>>, mut next_state: ResMut<NextState<GameState>>) {
    if keyboard.just_pressed(KeyCode::Escape) {
        next_state.set(GameState::MainMenu);
    }
}

/// Ground, lights, camera, and the one preview rig — built the same way
/// `player::spawn_players` builds a real one (`player::build_rig_model`,
/// `player::spawn_rig`), so the shared pipeline has exactly the components it
/// expects to find.
#[allow(clippy::too_many_arguments)]
fn enter_creator_stage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    theme: Res<Theme>,
    cs: Res<CreatorState>,
    jersey_assets: Option<Res<JerseyAssets>>,
) {
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
    commands.entity(rig).insert((
        CreatorStage,
        PreviewRig,
        player::Batter,
        PlayerIdentity {
            team: cs.team,
            index: cs.index,
        },
    ));

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
/// rebuilds it fresh rather than trying to reuse a stale one.
fn exit_creator_stage(mut commands: Commands, stage: Query<Entity, With<CreatorStage>>) {
    for entity in &stage {
        commands.entity(entity).despawn_recursive();
    }
}

/// Re-stamps the preview rig's [`PlayerIdentity`] whenever [`CreatorState`]'s
/// selection changes — the dress pipeline (`gear::dress_rigs`,
/// `jersey::dress_jerseys`) reacts through the normal `Changed<PlayerIdentity>`
/// path exactly as it would for a real rig. Note: `player::sync_identities`
/// (the gameplay identity stamper) only queries `RosterRole` rigs, and the
/// preview rig carries none, so the two stampers never fight over this
/// entity.
fn sync_preview_identity(
    cs: Res<CreatorState>,
    mut commands: Commands,
    rig: Query<Entity, With<PreviewRig>>,
) {
    if !cs.is_changed() {
        return;
    }
    for entity in &rig {
        commands.entity(entity).insert(PlayerIdentity {
            team: cs.team,
            index: cs.index,
        });
    }
}

/// A preview rig with nothing playing settles into `Idle` — Task 3 makes
/// this tab-aware (stance/fidget/celebration previews).
fn preview_idle(mut commands: Commands, rig: Query<Entity, (With<PreviewRig>, Without<Playing>)>) {
    for entity in &rig {
        commands.entity(entity).insert(Playing::new(AnimClip::Idle));
    }
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
fn retint_preview(
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

pub struct CreatorPlugin;

impl Plugin for CreatorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatorState>()
            .add_systems(OnEnter(GameState::Creator), enter_creator_stage)
            .add_systems(OnExit(GameState::Creator), exit_creator_stage)
            .add_systems(
                Update,
                enter_creator
                    .run_if(in_state(GameState::MainMenu))
                    .run_if(settings_closed),
            )
            .add_systems(
                Update,
                (
                    exit_creator,
                    sync_preview_identity,
                    preview_idle,
                    retint_preview,
                )
                    .run_if(in_state(GameState::Creator)),
            );
    }
}
