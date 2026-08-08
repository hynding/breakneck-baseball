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

use crate::game::ai::hash01;
use crate::game::animation::{self, Playing};
use crate::game::appearance::{
    Arms, CelebrationId, Eyewear, FidgetId, Headwear, PlayerAppearance, PlayerDef, RosterDefs,
    RosterFile, SkinTone, StanceId, StyleSet, TrotId,
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
    /// Bumped once per Randomize click and fed to [`randomize_player`] as its
    /// seed — a plain counter (not a clock/hash of real time) so repeated
    /// clicks visibly cycle through different curated looks while staying
    /// fully deterministic for anything driving the panel headlessly.
    pub randomize_seed: u32,
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
            randomize_seed: 0,
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

/// Read-only twin of [`selected_def`] — same clamp, no `&mut` needed. Used by
/// systems (camera framing, preview clip selection, `portraits.rs`'s
/// filename-from-selection lookup) that only need to look at the selection,
/// not edit it.
pub(crate) fn selected_def_ref(file: &RosterFile, team: Team, index: usize) -> &PlayerDef {
    let pool = match team {
        Team::Home => &file.home,
        Team::Away => &file.away,
    };
    let clamped = index.min(pool.len() - 1);
    &pool[clamped]
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
///
/// Guarded on `cs.working == cs.snapshot`: with no user edits there is
/// nothing to revert, and writing `defs` unconditionally here would clobber
/// a live `RosterDefs` the dev watcher hot-swapped while the Creator was
/// open (see [`sync_creator_from_external_reload`]) — the old
/// `working`/`snapshot` pair would stomp the fresh disk content right back
/// out on the way to the menu. `working != snapshot` still reverts exactly
/// as before.
fn revert_creator_edits(
    mut cs: ResMut<CreatorState>,
    mut defs: ResMut<RosterDefs>,
    mut live_rosters: ResMut<Rosters>,
) {
    if cs.working == cs.snapshot {
        return;
    }
    cs.working = cs.snapshot.clone();
    *defs = RosterDefs(cs.working.clone());
    let (rosters, _id) = preview_rosters_and_identity(&cs.working, cs.team, cs.index);
    *live_rosters = rosters;
}

/// Folds an *external* reload of `RosterDefs` (the dev watcher hot-swapping
/// `data/players.ron` after an AI/editor edit, per `appearance::dev_watch`)
/// into the Creator's own `working`/`snapshot` copies while the Creator is
/// open — otherwise the Creator has no idea `defs.0` moved out from under it,
/// and either the unconditional exit-time revert (guarded above) or the next
/// panel edit (`apply_creator_edits` always writes `defs.0 =
/// cs.working.clone()`) would silently stomp the reload back out.
///
/// The invariant that tells "external reload" apart from the Creator's own
/// writes: [`apply_creator_edits`] is the *only* other system that touches
/// `RosterDefs` while Creator is open, and it always sets `defs.0` to
/// exactly `cs.working` — so `defs.0 == cs.working` covers both "nothing
/// happened" and "our own apply/panel path just wrote this" and must no-op.
/// A genuine external reload instead lands content the Creator has not seen
/// from either side yet, so it differs from BOTH `cs.working` and
/// `cs.snapshot` at once — that's the only case this system reacts to. On
/// that case there is nothing meaningful left to revert *to* (the on-disk
/// edit already is the new baseline), so both copies adopt the reload
/// together; touching `cs` also flags it changed, so `apply_creator_edits`
/// (ordered right after this system in the `Creator` chain) re-stamps the
/// preview rig's `PlayerIdentity` and redresses it from the reload on the
/// very same frame.
fn sync_creator_from_external_reload(defs: Res<RosterDefs>, mut cs: ResMut<CreatorState>) {
    if defs.0 == cs.working || defs.0 == cs.snapshot {
        return;
    }
    cs.working = defs.0.clone();
    cs.snapshot = defs.0.clone();
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
fn lerp_creator_camera(
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
struct PreviewKey {
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
fn preview_idle(
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
/// the same way that system does.
///
/// egui redraws every frame the panel is visible, so a plain `&mut
/// CreatorState` handed to [`render_creator_panel`] would flag
/// `Changed<CreatorState>` on *every* frame via `Mut::deref_mut` — not just
/// frames with a real edit — defeating `apply_creator_edits`'s
/// `is_changed()` gate (it would rebuild `RosterDefs`/`Rosters` and
/// re-insert `PlayerIdentity` continuously while the Creator is simply
/// sitting open). Fixed the same way `debug.rs`'s Tune tab avoids the same
/// trap: render against a `bypass_change_detection()` borrow (no implicit
/// flag), have the render fns report back whether a widget actually
/// changed something, and only call `set_changed()` when that's true — a
/// real edit still flags exactly like a normal `DerefMut` would.
///
/// Tolerates a missing egui context — a headless test app has no
/// `PrimaryWindow` entity, so this no-ops instead of panicking; the apply
/// path it feeds (`apply_creator_edits`) is a separate system precisely so
/// the headless e2e can drive it with no panel, and therefore no egui
/// context, in the loop at all — and that path mutates `CreatorState` via
/// ordinary `ResMut`/field-assignment, whose normal change detection is
/// untouched by this bypass (it only ever applies to the panel's own
/// render-time borrow).
fn creator_panel(world: &mut World) {
    let Ok(ctx) = world
        .query_filtered::<&mut EguiContext, With<PrimaryWindow>>()
        .get_single_mut(world)
        .map(|mut c| c.get_mut().clone())
    else {
        return;
    };
    let mut cs = world.resource_mut::<CreatorState>();
    let changed = {
        let cs = cs.bypass_change_detection();
        egui::SidePanel::left("creator_panel")
            .default_width(320.0)
            .resizable(true)
            .show(&ctx, |ui| render_creator_panel(ui, cs))
            .inner
    };
    if changed {
        cs.set_changed();
    }
}

/// Team toggle + scrollable 13-name roster list, tab strip, the active
/// tab's fields, and Revert — all against `cs.working` (the panel never
/// touches `RosterDefs`/`Rosters`/the preview rig directly; that's
/// [`apply_creator_edits`]'s job). Returns whether any widget actually
/// changed a value this frame — `creator_panel` uses that to decide
/// whether to flag `CreatorState` changed at all (see its doc comment).
fn render_creator_panel(ui: &mut egui::Ui, cs: &mut CreatorState) -> bool {
    let mut changed = false;
    ui.heading("Player Creator");
    ui.separator();

    ui.horizontal(|ui| {
        changed |= ui
            .selectable_value(&mut cs.team, Team::Home, "Home")
            .changed();
        changed |= ui
            .selectable_value(&mut cs.team, Team::Away, "Away")
            .changed();
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
                if ui.selectable_label(cs.index == i, label).clicked() && cs.index != i {
                    cs.index = i;
                    changed = true;
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
            changed |= ui.selectable_value(&mut cs.tab, t, label).changed();
        }
    });
    ui.separator();

    let tab = cs.tab;
    let (team, index) = (cs.team, cs.index);
    {
        let def = selected_def(&mut cs.working, team, index);
        changed |= match tab {
            CreatorTab::Identity => render_identity_tab(ui, def),
            CreatorTab::Gear => render_gear_tab(ui, def),
            CreatorTab::Colors => render_colors_tab(ui, def),
            CreatorTab::Animations => render_animations_tab(ui, def),
        };
    }

    ui.separator();
    ui.horizontal(|ui| {
        if ui.button("Revert").clicked() {
            // `apply_creator_edits` picks this up next frame — it reapplies
            // on any `CreatorState` change, a revert included.
            cs.working = cs.snapshot.clone();
            cs.status = "reverted to last entry".to_string();
            changed = true;
        }
        if ui.button("Save").clicked() {
            // Not `apply_creator_edits`'s job (that path never touches
            // disk) — the panel calls the pure save fn directly and routes
            // the result into `cs.status`/`cs.snapshot` itself.
            match save_working(&cs.working) {
                Ok(()) => {
                    cs.snapshot = cs.working.clone();
                    cs.status = "saved to data/players.ron".to_string();
                }
                Err(e) => cs.status = format!("save failed: {e}"),
            }
            changed = true;
        }
        if ui.button("Randomize").clicked() {
            cs.randomize_seed = cs.randomize_seed.wrapping_add(1);
            let seed = cs.randomize_seed;
            randomize_player(selected_def(&mut cs.working, team, index), seed);
            changed = true;
        }
        // Disabled rather than wired up: the portrait harness
        // (`portraits.rs`) is a `PortraitRun` phase machine that starts at
        // `Phase::WaitForMenu` and drives the *menu* into the Creator itself
        // (`start_next_shot`/`advance_after_capture`) — invoking it from
        // inside an already-open Creator would need a phase tweak to skip
        // that leg. Not required for this task; the real entry point is the
        // CLI flag below, which the tooltip now points at directly.
        ui.add_enabled(false, egui::Button::new("Portraits"))
            .on_disabled_hover_text(
            "run from the command line:\ncargo run --features \"dev debug\" -- --portraits <dir>",
        );
    });
    ui.label(cs.status.as_str());
    changed
}

fn render_identity_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> bool {
    let mut changed = false;
    ui.label("Name (A-Z, up to 8 characters — jersey font alphabet)");
    let mut name = def.name.clone();
    if ui.text_edit_singleline(&mut name).changed() {
        def.name = name
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .take(8)
            .collect();
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.label("Number");
        changed |= ui
            .add(egui::DragValue::new(&mut def.number).range(0..=99))
            .changed();
    });
    changed
}

/// One horizontally-wrapped row of selectable buttons, one per `T::VARIANTS`
/// entry labelled from `T::NAMES` (declaration order pinned equal by
/// `appearance_enum!`, see `tests::variants_len_matches_names_for_every_appearance_enum`).
/// Returns whether the click actually changed `value` (a re-click on the
/// already-selected variant reports `false`, same as every other widget
/// here).
fn radio_grid<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    variants: &[T],
    names: &[&str],
    value: &mut T,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        for (variant, name) in variants.iter().zip(names.iter()) {
            changed |= ui.selectable_value(value, *variant, *name).changed();
        }
    });
    changed
}

fn render_gear_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> bool {
    let mut changed = false;
    ui.label("Headwear");
    changed |= radio_grid(
        ui,
        Headwear::VARIANTS,
        Headwear::NAMES,
        &mut def.appearance.headwear,
    );
    ui.separator();
    ui.label("Eyewear");
    changed |= radio_grid(
        ui,
        Eyewear::VARIANTS,
        Eyewear::NAMES,
        &mut def.appearance.eyewear,
    );
    ui.separator();
    ui.label("Arms");
    changed |= radio_grid(ui, Arms::VARIANTS, Arms::NAMES, &mut def.appearance.arms);
    ui.separator();
    changed |= ui.checkbox(&mut def.appearance.chain, "Chain").changed();
    changed
}

fn render_colors_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> bool {
    let mut changed = false;
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
            if ui.add(button).on_hover_text(*name).clicked() && def.appearance.skin != *variant {
                def.appearance.skin = *variant;
                changed = true;
            }
        }
    });
    changed
}

fn render_animations_tab(ui: &mut egui::Ui, def: &mut PlayerDef) -> bool {
    let mut changed = false;
    ui.label("Stance");
    changed |= radio_grid(
        ui,
        StanceId::VARIANTS,
        StanceId::NAMES,
        &mut def.appearance.style.stance,
    );
    ui.separator();
    ui.label("Fidget");
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .selectable_value(&mut def.appearance.style.fidget, None, "None")
            .changed();
        for (variant, name) in FidgetId::VARIANTS.iter().zip(FidgetId::NAMES.iter()) {
            changed |= ui
                .selectable_value(&mut def.appearance.style.fidget, Some(*variant), *name)
                .changed();
        }
    });
    ui.separator();
    ui.label("Celebration");
    changed |= radio_grid(
        ui,
        CelebrationId::VARIANTS,
        CelebrationId::NAMES,
        &mut def.appearance.style.celebration,
    );
    changed
}

// ── Randomize ────────────────────────────────────────────────────────────────

/// Deterministic 0..1 roll for one randomize "channel" (skin, headwear, ...)
/// off a shared `seed` — large, distinct per-channel offsets keep channels
/// decorrelated since [`hash01`] is a `sin`-based hash where nearby inputs
/// produce nearby outputs.
fn roll(seed: u32, channel: u32) -> f32 {
    hash01(seed as f32 * 7.0 + channel as f32 * 101.0)
}

/// Uniform pick across every variant of a slice (used where the brief calls
/// a field "uniform" — skin, arms, stance).
fn pick_uniform<T: Copy>(roll: f32, variants: &[T]) -> T {
    let n = variants.len();
    let idx = ((roll * n as f32) as usize).min(n - 1);
    variants[idx]
}

/// Cap 40% / Helmet 25% / CapBackwards 20% / Bare 15%.
fn pick_headwear(roll: f32) -> Headwear {
    if roll < 0.40 {
        Headwear::Cap
    } else if roll < 0.65 {
        Headwear::Helmet
    } else if roll < 0.85 {
        Headwear::CapBackwards
    } else {
        Headwear::Bare
    }
}

/// Bare 60%, the other three variants split evenly over the remaining 40%.
fn pick_eyewear(roll: f32) -> Eyewear {
    const REST: f32 = (1.0 - 0.60) / 3.0;
    if roll < 0.60 {
        Eyewear::Bare
    } else if roll < 0.60 + REST {
        Eyewear::Glasses
    } else if roll < 0.60 + 2.0 * REST {
        Eyewear::Shades
    } else {
        Eyewear::EyeBlack
    }
}

/// None 40%, the two real fidgets split evenly over the remaining 60%.
fn pick_fidget(roll: f32) -> Option<FidgetId> {
    if roll < 0.40 {
        None
    } else if roll < 0.70 {
        Some(FidgetId::HalfSwing)
    } else {
        Some(FidgetId::BatTap)
    }
}

/// Standard 70% / BatFlip 30%.
fn pick_celebration(roll: f32) -> CelebrationId {
    if roll < 0.70 {
        CelebrationId::Standard
    } else {
        CelebrationId::BatFlip
    }
}

/// Curated randomize: coherent combinations, not uniform RGB clown output.
/// Deterministic in `seed` ([`hash01`] mixes) so the same seed always
/// reproduces the same look — the panel's bumping `randomize_seed` counter
/// gets a fresh look per click while staying pinnable in tests. Builds a
/// whole fresh [`PlayerAppearance`] literal (every field named explicitly,
/// no `..PlayerAppearance::default()` spread) so a field can never be
/// silently left un-rolled; `name`/`number` are untouched — randomize only
/// covers appearance, per the brief's curation table.
pub fn randomize_player(def: &mut PlayerDef, seed: u32) {
    let skin = pick_uniform(roll(seed, 0), SkinTone::VARIANTS);
    let headwear = pick_headwear(roll(seed, 1));
    let eyewear = pick_eyewear(roll(seed, 2));
    let chain = roll(seed, 3) < 0.25;
    let arms = pick_uniform(roll(seed, 4), Arms::VARIANTS);
    let stance = pick_uniform(roll(seed, 5), StanceId::VARIANTS);
    let fidget = pick_fidget(roll(seed, 6));
    let celebration = pick_celebration(roll(seed, 7));

    def.appearance = PlayerAppearance {
        skin,
        headwear,
        eyewear,
        arms,
        chain,
        style: StyleSet {
            stance,
            fidget,
            // The only `TrotId` variant today — written explicitly (not via
            // a `..default()` spread) so a future second trot is forced
            // through this same curated seam instead of silently defaulting.
            trot: TrotId::Standard,
            celebration,
        },
    };
}

// ── Save ─────────────────────────────────────────────────────────────────────

/// Where [`save_working`] writes by default — the repo's authored roster
/// file. Anchored by `CARGO_MANIFEST_DIR` (not the process's cwd, which
/// nothing guarantees points at the workspace).
const PLAYERS_RON_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/players.ron");

/// Validates then writes `working` as pretty RON to `path`. Pulled the path
/// out into a parameter (rather than hardcoding [`PLAYERS_RON_PATH`] here) so
/// tests can point this at a temp file instead of the repo's real data —
/// [`save_working`] is the thin wrapper that supplies the real path. Validates
/// *before* writing so a bad working copy (e.g. an unauthored `!` in a name)
/// never touches disk. NOTE: a save always re-serializes the *whole* file in
/// this pretty-RON formatting, so the first save after this lands produces a
/// one-time diff of formatting-only churn against the hand-authored
/// `data/players.ron` — accepted; the Creator hub owns the file's format from
/// here on.
pub fn save_working_to(path: &str, working: &RosterFile) -> Result<(), String> {
    RosterDefs::validate(working)?;
    let text = ron::ser::to_string_pretty(working, ron::ser::PrettyConfig::new())
        .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Saves `working` to the repo's real `data/players.ron`. The panel's Save
/// button calls this directly; on `Ok` it also refreshes `cs.snapshot` (the
/// saved state becomes the new revert point) and on `Err` routes the message
/// into `cs.status` — both at the call site, since this fn only ever touches
/// the file, never `CreatorState`.
pub fn save_working(working: &RosterFile) -> Result<(), String> {
    save_working_to(PLAYERS_RON_PATH, working)
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
                    sync_creator_from_external_reload,
                    apply_creator_edits,
                    lerp_creator_camera,
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

    fn blank_def() -> PlayerDef {
        PlayerDef {
            name: "TEST".to_string(),
            number: 0,
            appearance: PlayerAppearance::default(),
        }
    }

    #[test]
    fn randomize_is_deterministic_for_the_same_seed() {
        let mut a = blank_def();
        let mut b = blank_def();
        randomize_player(&mut a, 42);
        randomize_player(&mut b, 42);
        assert_eq!(a.appearance, b.appearance);
    }

    #[test]
    fn randomize_leaves_name_and_number_untouched() {
        let mut def = blank_def();
        randomize_player(&mut def, 7);
        assert_eq!(def.name, "TEST");
        assert_eq!(def.number, 0);
    }

    /// Every curated field must vary across enough seeds to prove it's
    /// actually driven by the roll, not silently left at a default via a
    /// `..PlayerAppearance::default()` spread — "every field written" from
    /// the brief. Headwear additionally must hit every one of its four
    /// variants (the brief's explicit coverage requirement). None of the
    /// appearance enums derive `Hash`, so coverage is tracked with plain
    /// `Vec::contains` rather than a `HashSet`.
    #[test]
    fn randomize_covers_every_field_over_many_seeds() {
        let mut skins = Vec::new();
        let mut headwears = Vec::new();
        let mut eyewears = Vec::new();
        let mut chains = Vec::new();
        let mut arms = Vec::new();
        let mut stances = Vec::new();
        let mut fidgets = Vec::new();
        let mut celebrations = Vec::new();

        for seed in 0..100u32 {
            let mut def = blank_def();
            randomize_player(&mut def, seed);
            let a = &def.appearance;
            if !skins.contains(&a.skin) {
                skins.push(a.skin);
            }
            if !headwears.contains(&a.headwear) {
                headwears.push(a.headwear);
            }
            if !eyewears.contains(&a.eyewear) {
                eyewears.push(a.eyewear);
            }
            if !chains.contains(&a.chain) {
                chains.push(a.chain);
            }
            if !arms.contains(&a.arms) {
                arms.push(a.arms);
            }
            if !stances.contains(&a.style.stance) {
                stances.push(a.style.stance);
            }
            if !fidgets.contains(&a.style.fidget) {
                fidgets.push(a.style.fidget);
            }
            if !celebrations.contains(&a.style.celebration) {
                celebrations.push(a.style.celebration);
            }
            assert_eq!(a.style.trot, TrotId::Standard);
        }

        assert_eq!(
            headwears.len(),
            Headwear::VARIANTS.len(),
            "every headwear variant must appear over 100 seeds"
        );
        assert!(skins.len() > 1, "skin must vary across seeds");
        assert!(eyewears.len() > 1, "eyewear must vary across seeds");
        assert!(
            chains.contains(&true) && chains.contains(&false),
            "chain must land both true and false across seeds"
        );
        assert!(arms.len() > 1, "arms must vary across seeds");
        assert!(stances.len() > 1, "stance must vary across seeds");
        assert!(
            fidgets.contains(&None)
                && fidgets.contains(&Some(FidgetId::HalfSwing))
                && fidgets.contains(&Some(FidgetId::BatTap)),
            "fidget must hit None and both real variants across seeds"
        );
        assert!(
            celebrations.contains(&CelebrationId::Standard)
                && celebrations.contains(&CelebrationId::BatFlip),
            "celebration must hit both variants across seeds"
        );
    }

    /// Loose statistical check on the curated weights (not a strict RNG —
    /// `ai::hash01` is a sin-based hash, so give it a generous band) over a
    /// much bigger sample: headwear's `Cap` is the plurality pick (~40%),
    /// never a minority sliver, and never a de-facto uniform 25% either.
    #[test]
    fn randomize_headwear_weights_favor_cap() {
        let mut cap_count = 0u32;
        let n = 2000u32;
        for seed in 0..n {
            let mut def = blank_def();
            randomize_player(&mut def, seed);
            if def.appearance.headwear == Headwear::Cap {
                cap_count += 1;
            }
        }
        let frac = cap_count as f32 / n as f32;
        assert!(
            (0.30..=0.50).contains(&frac),
            "Cap should land near its curated 40% weight, got {frac}"
        );
    }

    #[test]
    fn save_working_to_round_trips_through_ron() {
        let dir = std::env::temp_dir().join(format!("bb-creator-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("valid.ron");
        let path_str = path.to_str().unwrap();

        let working = embedded_roster_file();
        let result = save_working_to(path_str, &working);
        assert!(result.is_ok(), "valid roster file must save: {result:?}");

        let text = std::fs::read_to_string(&path).unwrap();
        let reparsed: RosterFile = ron::from_str(&text).unwrap();
        assert_eq!(reparsed, working);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn save_working_to_rejects_an_invalid_name_and_writes_nothing() {
        let dir = std::env::temp_dir().join(format!("bb-creator-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("invalid.ron");
        let path_str = path.to_str().unwrap();
        std::fs::remove_file(&path).ok();

        let mut working = embedded_roster_file();
        working.home[0].name = "bad!".to_string();
        let result = save_working_to(path_str, &working);
        assert!(result.is_err(), "an invalid name must be rejected");
        assert!(
            !path.exists(),
            "a rejected save must not write anything to disk"
        );
    }
}
