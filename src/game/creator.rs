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
//! The panel (Tune-tab-style egui UI for picking team/slot/tab, editing the
//! selection's appearance, and reverting) lands in this task: [`CreatorState`]
//! grows a `working`/`snapshot` pair of [`RosterFile`] copies the panel edits
//! and can discard, plus a change-application path
//! ([`apply_creator_edits`]) that's deliberately a *separate system* from the
//! panel — the panel only ever mutates `cs.working` (+ `cs.status`), so the
//! headless e2e can drive the exact same apply path with no egui in the loop
//! at all.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_inspector_egui::bevy_egui::EguiContext;
use bevy_inspector_egui::egui;

use crate::game::animation::{AnimClip, Playing};
use crate::game::appearance::{
    Arms, CelebrationId, Eyewear, FidgetId, Headwear, PlayerDef, RosterDefs, RosterFile, SkinTone,
    StanceId,
};
use crate::game::jersey::{self, JerseyAssets};
use crate::game::model_assets::{GltfJerseyMesh, GltfPart, GltfTeamMaterials};
use crate::game::player::{self, RigUnit};
use crate::game::roster::{PlayerIdentity, Rosters};
use crate::game::rules::LINEUP_SIZE;
use crate::game::settings::settings_closed;
use crate::game::theme::Theme;
use crate::game::{GameState, Team};

/// Which tab of the panel is showing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CreatorTab {
    #[default]
    Identity,
    Gear,
    Colors,
    Animations,
}

/// Which team/roster-slot the Creator is currently previewing, which tab is
/// open, and the working/snapshot copies the panel edits live in. `working`
/// starts equal to `snapshot` on every Creator entry (`enter_creator_stage`)
/// and to each other again after a revert; only [`apply_creator_edits`] ever
/// turns `working` into a live `RosterDefs`/`Rosters`/preview-identity
/// change — the panel itself touches nothing else.
#[derive(Resource, Debug, Clone)]
pub struct CreatorState {
    pub team: Team,
    pub index: usize,
    pub tab: CreatorTab,
    pub working: RosterFile,
    pub snapshot: RosterFile,
    pub status: String,
}

impl Default for CreatorState {
    fn default() -> Self {
        // Real content up front (not an empty/placeholder file) so a
        // `CreatorState` built before `RosterDefs` has ever been touched —
        // e.g. `init_resource` order at plugin build — is still valid to
        // read from; `enter_creator_stage` overwrites both copies from the
        // live `RosterDefs` on every actual Creator entry regardless.
        let file = crate::game::appearance::embedded_roster_file();
        Self {
            team: Team::Home,
            index: 0,
            tab: CreatorTab::default(),
            working: file.clone(),
            snapshot: file,
            status: String::new(),
        }
    }
}

/// The selected player's def — index 0..(pool len) spans lineup then bench.
/// Clamped like [`crate::game::roster::TeamRoster::card`] so a stale index
/// (e.g. left over from a bigger roster file) still resolves to something
/// instead of panicking.
pub fn selected_def(file: &mut RosterFile, team: Team, index: usize) -> &mut PlayerDef {
    let pool = match team {
        Team::Home => &mut file.home,
        Team::Away => &mut file.away,
    };
    let clamped = index.min(pool.len() - 1);
    &mut pool[clamped]
}

/// Preview `Rosters` + `PlayerIdentity` for a selection, built fresh from
/// `working` every call. Bench players (`index >= LINEUP_SIZE`) can't be
/// addressed by [`crate::game::roster::TeamRoster::card`] — it only reaches
/// into the lineup — so a bench selection swaps that player into the
/// selected team's lineup slot 0 in the returned `Rosters` and reports
/// identity index 0; the dress pipeline never needs to know "bench" is a
/// concept. A lineup selection (`index < LINEUP_SIZE`) passes the ordering
/// through unmodified.
pub fn preview_rosters_and_identity(
    working: &RosterFile,
    team: Team,
    index: usize,
) -> (Rosters, PlayerIdentity) {
    let mut rosters = Rosters::from_defs(&RosterDefs(working.clone()));
    let lineup_size = LINEUP_SIZE as usize;
    if index >= lineup_size {
        let bench_index = index - lineup_size;
        let team_roster = rosters.team_mut(team);
        if let Some(card) = team_roster.bench.get(bench_index).cloned() {
            team_roster.lineup[0] = card;
        }
        (rosters, PlayerIdentity { team, index: 0 })
    } else {
        (rosters, PlayerIdentity { team, index })
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
/// expects to find. Also loads `cs.working`/`cs.snapshot` fresh from the live
/// `RosterDefs` — every Creator entry starts with no unsaved edits, even if a
/// previous session left `working` mid-edit (reverted separately on exit,
/// but redundant safety here costs nothing).
#[allow(clippy::too_many_arguments)]
fn enter_creator_stage(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    theme: Res<Theme>,
    mut cs: ResMut<CreatorState>,
    defs: Res<RosterDefs>,
    mut live_rosters: ResMut<Rosters>,
    jersey_assets: Option<Res<JerseyAssets>>,
    mut main_cameras: Query<&mut Camera, (With<Camera3d>, Without<CreatorStage>)>,
) {
    cs.working = defs.0.clone();
    cs.snapshot = defs.0.clone();
    cs.status.clear();

    // The persistent main camera (`camera.rs::spawn_camera`, active from
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
fn exit_creator_stage(
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

/// The panel-independent apply path: whenever [`CreatorState`] changes
/// (panel edit, selection change, or a test writing `cs.working`/`cs.team`/
/// `cs.index` directly with no panel in the loop at all), rebuilds the live
/// `RosterDefs` and `Rosters` from `cs.working` and re-stamps the preview
/// rig's [`PlayerIdentity`] — the dress pipeline (`gear::dress_rigs`,
/// `jersey::dress_jerseys`) reacts through the normal `Changed<PlayerIdentity>`
/// path exactly as it would for a real rig. Note: `player::sync_identities`
/// (the gameplay identity stamper) only queries `RosterRole` rigs, and the
/// preview rig carries none, so the two stampers never fight over this
/// entity. Deliberately a system separate from the egui panel (`creator_panel`
/// only ever mutates `cs.working`/`cs.status`) so this same path is what
/// both the panel and the headless e2e (`tests/e2e_creator.rs`) drive.
fn apply_creator_edits(
    cs: Res<CreatorState>,
    mut defs: ResMut<RosterDefs>,
    mut live_rosters: ResMut<Rosters>,
    mut commands: Commands,
    rig: Query<Entity, With<PreviewRig>>,
) {
    if !cs.is_changed() {
        return;
    }
    *defs = RosterDefs(cs.working.clone());
    let (rosters, id) = preview_rosters_and_identity(&cs.working, cs.team, cs.index);
    *live_rosters = rosters;
    for entity in &rig {
        // Insert `PlayerIdentity` unconditionally on every apply — do NOT
        // gate it on team/index differing from what's already stamped.
        // Insertion (not value-equality) is the change signal
        // `dress_rigs`/`sync_identities` react to (see
        // `player::sync_identities`'s "Inserting is the change signal" doc
        // comment) — a same-value re-insert on a pure appearance edit IS the
        // retrigger mechanism, not waste.
        commands.entity(entity).insert(id);
    }
}

/// Discards unsaved edits on **every** exit path (Esc today; future menu
/// buttons land the same way) by resetting `cs.working` to `cs.snapshot` and
/// reapplying — the same two steps the panel's Revert button performs, just
/// run directly instead of relying on `apply_creator_edits` picking the
/// change up next frame (it won't: by the time `OnExit` fires, the state has
/// already left `Creator`, and that system is gated on being in it).
fn revert_creator_edits(
    mut cs: ResMut<CreatorState>,
    mut defs: ResMut<RosterDefs>,
    mut live_rosters: ResMut<Rosters>,
) {
    cs.working = cs.snapshot.clone();
    *defs = RosterDefs(cs.working.clone());
    let (rosters, _id) = preview_rosters_and_identity(&cs.working, cs.team, cs.index);
    *live_rosters = rosters;
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

/// The selector + tabs + revert egui side panel. Exclusive (needs `&mut
/// World`, same as `debug::debug_panel`) purely to reach the `EguiContext`
/// the same way that system does; every actual read/write is a single
/// `Mut<CreatorState>` borrow handed to [`render_creator_panel`], which is
/// plain `egui` + `CreatorState` with no other ECS access at all.
///
/// Tolerates a missing egui context — a headless test app has no
/// `PrimaryWindow` entity, so this no-ops instead of panicking; the apply
/// path it feeds (`apply_creator_edits`) is a separate system precisely so
/// the headless e2e can drive it with no panel, and therefore no egui
/// context, in the loop at all.
fn creator_panel(world: &mut World) {
    let Ok(ctx) = world
        .query_filtered::<&mut EguiContext, With<PrimaryWindow>>()
        .get_single_mut(world)
        .map(|mut c| c.get_mut().clone())
    else {
        return;
    };
    let mut cs = world.resource_mut::<CreatorState>();
    egui::SidePanel::left("creator_panel")
        .default_width(320.0)
        .resizable(true)
        .show(&ctx, |ui| render_creator_panel(ui, &mut cs));
}

/// Team toggle + scrollable 13-name roster list, tab strip, the active
/// tab's fields, and Revert — all against `cs.working` (the panel never
/// touches `RosterDefs`/`Rosters`/the preview rig directly; that's
/// [`apply_creator_edits`]'s job, driven by `cs` simply having changed).
fn render_creator_panel(ui: &mut egui::Ui, cs: &mut CreatorState) {
    ui.heading("Player Creator");
    ui.separator();

    ui.horizontal(|ui| {
        ui.selectable_value(&mut cs.team, Team::Home, "Home");
        ui.selectable_value(&mut cs.team, Team::Away, "Away");
    });
    let team = cs.team;
    let pool_len = match team {
        Team::Home => cs.working.home.len(),
        Team::Away => cs.working.away.len(),
    };
    egui::ScrollArea::vertical()
        .id_salt("creator_roster_list")
        .max_height(180.0)
        .show(ui, |ui| {
            let lineup_size = LINEUP_SIZE as usize;
            for i in 0..pool_len {
                let def = match team {
                    Team::Home => &cs.working.home[i],
                    Team::Away => &cs.working.away[i],
                };
                let label = if i < lineup_size {
                    format!("{:>2}. #{:<2} {}", i + 1, def.number, def.name)
                } else {
                    format!("B{}. #{:<2} {}", i - lineup_size + 1, def.number, def.name)
                };
                if ui.selectable_label(cs.index == i, label).clicked() {
                    cs.index = i;
                }
            }
        });
    ui.separator();

    ui.horizontal(|ui| {
        for (label, t) in [
            ("Identity", CreatorTab::Identity),
            ("Gear", CreatorTab::Gear),
            ("Colors", CreatorTab::Colors),
            ("Animations", CreatorTab::Animations),
        ] {
            ui.selectable_value(&mut cs.tab, t, label);
        }
    });
    ui.separator();

    let tab = cs.tab;
    let (team, index) = (cs.team, cs.index);
    {
        let def = selected_def(&mut cs.working, team, index);
        match tab {
            CreatorTab::Identity => render_identity_tab(ui, def),
            CreatorTab::Gear => render_gear_tab(ui, def),
            CreatorTab::Colors => render_colors_tab(ui, def),
            CreatorTab::Animations => render_animations_tab(ui, def),
        }
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Revert").clicked() {
            // `apply_creator_edits` picks this up next frame — it reapplies
            // on any `CreatorState` change, a revert included.
            cs.working = cs.snapshot.clone();
            cs.status = "reverted to last entry".to_string();
        }
        ui.add_enabled(false, egui::Button::new("Save"))
            .on_disabled_hover_text("lands in a later task");
        ui.add_enabled(false, egui::Button::new("Randomize"))
            .on_disabled_hover_text("lands in a later task");
        ui.add_enabled(false, egui::Button::new("Portraits"))
            .on_disabled_hover_text("lands in a later task");
    });
    ui.label(cs.status.as_str());
}

fn render_identity_tab(ui: &mut egui::Ui, def: &mut PlayerDef) {
    ui.label("Name (A-Z, up to 8 characters — jersey font alphabet)");
    let mut name = def.name.clone();
    if ui.text_edit_singleline(&mut name).changed() {
        def.name = name
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .take(8)
            .collect();
    }
    ui.horizontal(|ui| {
        ui.label("Number");
        ui.add(egui::DragValue::new(&mut def.number).range(0..=99));
    });
}

/// One horizontally-wrapped row of selectable buttons, one per `T::VARIANTS`
/// entry labelled from `T::NAMES` (declaration order pinned equal by
/// `appearance_enum!`, see `tests::variants_len_matches_names_for_every_appearance_enum`).
fn radio_grid<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    variants: &[T],
    names: &[&str],
    value: &mut T,
) {
    ui.horizontal_wrapped(|ui| {
        for (variant, name) in variants.iter().zip(names.iter()) {
            ui.selectable_value(value, *variant, *name);
        }
    });
}

fn render_gear_tab(ui: &mut egui::Ui, def: &mut PlayerDef) {
    ui.label("Headwear");
    radio_grid(
        ui,
        Headwear::VARIANTS,
        Headwear::NAMES,
        &mut def.appearance.headwear,
    );
    ui.separator();
    ui.label("Eyewear");
    radio_grid(
        ui,
        Eyewear::VARIANTS,
        Eyewear::NAMES,
        &mut def.appearance.eyewear,
    );
    ui.separator();
    ui.label("Arms");
    radio_grid(ui, Arms::VARIANTS, Arms::NAMES, &mut def.appearance.arms);
    ui.separator();
    ui.checkbox(&mut def.appearance.chain, "Chain");
}

fn render_colors_tab(ui: &mut egui::Ui, def: &mut PlayerDef) {
    ui.label("Skin tone");
    ui.horizontal_wrapped(|ui| {
        for (variant, name) in SkinTone::VARIANTS.iter().zip(SkinTone::NAMES.iter()) {
            let rgba = variant.color().to_srgba().to_f32_array();
            let swatch = egui::Color32::from_rgb(
                (rgba[0] * 255.0) as u8,
                (rgba[1] * 255.0) as u8,
                (rgba[2] * 255.0) as u8,
            );
            let selected = def.appearance.skin == *variant;
            let button = egui::Button::new("")
                .fill(swatch)
                .min_size(egui::vec2(28.0, 28.0))
                .stroke(if selected {
                    egui::Stroke::new(2.0_f32, egui::Color32::WHITE)
                } else {
                    egui::Stroke::NONE
                });
            if ui.add(button).on_hover_text(*name).clicked() {
                def.appearance.skin = *variant;
            }
        }
    });
}

fn render_animations_tab(ui: &mut egui::Ui, def: &mut PlayerDef) {
    ui.label("Stance");
    radio_grid(
        ui,
        StanceId::VARIANTS,
        StanceId::NAMES,
        &mut def.appearance.style.stance,
    );
    ui.separator();
    ui.label("Fidget");
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut def.appearance.style.fidget, None, "None");
        for (variant, name) in FidgetId::VARIANTS.iter().zip(FidgetId::NAMES.iter()) {
            ui.selectable_value(&mut def.appearance.style.fidget, Some(*variant), *name);
        }
    });
    ui.separator();
    ui.label("Celebration");
    radio_grid(
        ui,
        CelebrationId::VARIANTS,
        CelebrationId::NAMES,
        &mut def.appearance.style.celebration,
    );
}

pub struct CreatorPlugin;

impl Plugin for CreatorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatorState>()
            .add_systems(OnEnter(GameState::Creator), enter_creator_stage)
            .add_systems(
                OnExit(GameState::Creator),
                (revert_creator_edits, exit_creator_stage),
            )
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
                    apply_creator_edits,
                    preview_idle,
                    retint_preview,
                    creator_panel,
                )
                    .chain()
                    .run_if(in_state(GameState::Creator)),
            );
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::appearance::embedded_roster_file;
    use crate::game::rules::LINEUP_SIZE;

    #[test]
    fn selected_def_spans_lineup_then_bench() {
        let mut file = embedded_roster_file();
        let lineup_name = file.home[3].name.clone();
        assert_eq!(selected_def(&mut file, Team::Home, 3).name, lineup_name);

        // Bench index 10 is pool index 10 (lineup 0..9, bench from 9).
        let bench_name = file.home[10].name.clone();
        assert_eq!(selected_def(&mut file, Team::Home, 10).name, bench_name);

        // Out-of-range index clamps to the last player rather than panicking.
        let last_name = file.away.last().unwrap().name.clone();
        let far_index = file.away.len() + 50;
        assert_eq!(
            selected_def(&mut file, Team::Away, far_index).name,
            last_name
        );
    }

    #[test]
    fn selected_def_edits_are_visible_through_the_same_file() {
        let mut file = embedded_roster_file();
        selected_def(&mut file, Team::Home, 0).appearance.headwear =
            crate::game::appearance::Headwear::Bare;
        assert_eq!(
            file.home[0].appearance.headwear,
            crate::game::appearance::Headwear::Bare
        );
    }

    #[test]
    fn bench_selection_remaps_to_lineup_slot_zero() {
        let file = embedded_roster_file();
        let lineup_size = LINEUP_SIZE as usize;
        let bench_index = lineup_size + 1; // second bench player (0-based within bench)
        let bench_name = file.home[bench_index].name.clone();

        let (rosters, id) = preview_rosters_and_identity(&file, Team::Home, bench_index);

        assert_eq!(
            id,
            PlayerIdentity {
                team: Team::Home,
                index: 0
            }
        );
        assert_eq!(rosters.home.lineup[0].name, bench_name);
    }

    #[test]
    fn lineup_selection_keeps_index_and_ordering_unmodified() {
        let file = embedded_roster_file();
        let (rosters, id) = preview_rosters_and_identity(&file, Team::Home, 3);

        assert_eq!(
            id,
            PlayerIdentity {
                team: Team::Home,
                index: 3
            }
        );
        let expected = Rosters::from_defs(&RosterDefs(file.clone()));
        assert_eq!(
            rosters
                .home
                .lineup
                .iter()
                .map(|c| &c.name)
                .collect::<Vec<_>>(),
            expected
                .home
                .lineup
                .iter()
                .map(|c| &c.name)
                .collect::<Vec<_>>(),
        );
    }
}
