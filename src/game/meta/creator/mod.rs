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
//!
//! Split across submodules (this file is the facade + entry/apply/revert
//! systems): [`panel`] (the egui side panel), [`preview`] (stage
//! spawn/teardown, camera, preview clip selection, retinting), [`randomize`]
//! (the curated Randomize button), and [`persist`] (save-to-disk).

use bevy::prelude::*;

use crate::game::appearance::{PlayerDef, RosterDefs, RosterFile};
use crate::game::roster::{PlayerIdentity, Rosters};
use crate::game::rules::LINEUP_SIZE;
use crate::game::settings::settings_closed;
use crate::game::{GameState, Team};

mod panel;
mod persist;
mod preview;
mod randomize;

pub use persist::{save_working, save_working_to};
pub use preview::PreviewRig;
pub use randomize::randomize_player;

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
    /// Bumped once per Randomize click and fed to [`randomize::randomize_player`] as its
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
/// [`preview::exit_creator_stage`] (`OnExit(Creator)`), decoupled from the
/// keypress so it fires on every exit path, not just this one.
fn exit_creator(keyboard: Res<ButtonInput<KeyCode>>, mut next_state: ResMut<NextState<GameState>>) {
    if keyboard.just_pressed(KeyCode::Escape) {
        next_state.set(GameState::MainMenu);
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
/// entity. Deliberately a system separate from the egui panel (`panel::creator_panel`
/// only ever mutates `cs.working`/`cs.status`) so this same path is what
/// both the panel and the headless e2e (`tests/e2e_creator.rs`) drive.
///
/// Also records exactly what it wrote into [`LastAppliedRoster`] — the
/// yardstick [`sync_creator_from_external_reload`] compares `defs.0`
/// against to tell a genuine external reload apart from this system's own
/// (one-frame-lagged, relative to the panel) write. See that system's doc
/// comment for why an inferred check (comparing against `cs.working`
/// instead) misfires mid-drag.
fn apply_creator_edits(
    cs: Res<CreatorState>,
    mut defs: ResMut<RosterDefs>,
    mut last_applied: ResMut<LastAppliedRoster>,
    mut live_rosters: ResMut<Rosters>,
    mut commands: Commands,
    rig: Query<Entity, With<PreviewRig>>,
) {
    if !cs.is_changed() {
        return;
    }
    *defs = RosterDefs(cs.working.clone());
    last_applied.0 = defs.0.clone();
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
    mut last_applied: ResMut<LastAppliedRoster>,
    mut live_rosters: ResMut<Rosters>,
) {
    if cs.working == cs.snapshot {
        return;
    }
    cs.working = cs.snapshot.clone();
    *defs = RosterDefs(cs.working.clone());
    last_applied.0 = defs.0.clone();
    let (rosters, _id) = preview_rosters_and_identity(&cs.working, cs.team, cs.index);
    *live_rosters = rosters;
}

/// What [`apply_creator_edits`] most recently wrote into the live
/// `RosterDefs` — the exact yardstick [`sync_creator_from_external_reload`]
/// uses to tell a genuine external reload apart from that system's own
/// one-frame-lagged write (see its doc comment for the frame trace this
/// closes). Deliberately its own resource rather than a `CreatorState`
/// field: `apply_creator_edits` only ever *reads* `CreatorState`
/// (`cs: Res<CreatorState>`) today, and keeping it that way means writing
/// this yardstick can never itself flag `CreatorState` changed and confuse
/// a system gated on `cs.is_changed()`.
///
/// Seeded to match `RosterDefs` on every Creator entry
/// ([`preview::enter_creator_stage`]) — nothing has been applied yet that
/// session, so there is nothing to compare against until the first real
/// write.
#[derive(Resource, Debug, Clone)]
struct LastAppliedRoster(RosterFile);

impl Default for LastAppliedRoster {
    fn default() -> Self {
        Self(crate::game::appearance::embedded_roster_file())
    }
}

/// Folds an *external* reload of `RosterDefs` (the dev watcher hot-swapping
/// `data/players.ron` after an AI/editor edit, per `appearance::dev_watch`)
/// into the Creator's own `working`/`snapshot` copies while the Creator is
/// open — otherwise the Creator has no idea `defs.0` moved out from under it,
/// and either the unconditional exit-time revert (guarded above) or the next
/// panel edit (`apply_creator_edits` always writes `defs.0 =
/// cs.working.clone()`) would silently stomp the reload back out.
///
/// The invariant is `defs.0 != last_applied.0`, **not** `defs.0 !=
/// cs.working` — an earlier version compared against `cs.working` (and
/// `cs.snapshot`) directly and had a false-positive hole: this system runs
/// *before* `apply_creator_edits` in the `Creator` chain, so on any frame
/// `defs.0` is one frame behind whatever the panel just wrote (e.g. mid-drag
/// on a numeric field, which fires `.changed()` every frame the drag moves).
/// Concretely, frame 3 of a 3-frame drag: `defs.0` still holds frame 2's
/// applied value while `cs.working` already holds frame 3's — `defs.0 !=
/// cs.working` and `defs.0 != cs.snapshot`, misclassifying the panel's own
/// lag as an external reload and stomping `cs.working` back a step *and*
/// corrupting `cs.snapshot` (breaking Revert until save or re-entry).
///
/// [`LastAppliedRoster`] closes that hole by recording exactly what
/// `apply_creator_edits` wrote, not inferring it: `defs.0` can only differ
/// from `last_applied.0` if something *other* than `apply_creator_edits`
/// wrote `RosterDefs` — the dev watcher — since every apply keeps them
/// identical by construction, lag included (the comparison needs no
/// `cs.working`/`cs.snapshot` involvement at all). On a genuine mismatch
/// there is nothing meaningful left to revert *to* (the on-disk edit
/// already is the new baseline), so `working`, `snapshot`, and
/// `last_applied` all adopt the reload together; touching `cs` also flags
/// it changed, so `apply_creator_edits` (ordered right after this system in
/// the `Creator` chain) re-stamps the preview rig's `PlayerIdentity` and
/// redresses it from the reload on the very same frame.
fn sync_creator_from_external_reload(
    defs: Res<RosterDefs>,
    mut last_applied: ResMut<LastAppliedRoster>,
    mut cs: ResMut<CreatorState>,
) {
    if defs.0 == last_applied.0 {
        return;
    }
    cs.working = defs.0.clone();
    cs.snapshot = defs.0.clone();
    last_applied.0 = defs.0.clone();
}

pub struct CreatorPlugin;

impl Plugin for CreatorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CreatorState>()
            .init_resource::<LastAppliedRoster>()
            .add_systems(OnEnter(GameState::Creator), preview::enter_creator_stage)
            .add_systems(
                OnExit(GameState::Creator),
                (revert_creator_edits, preview::exit_creator_stage),
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
                    preview::lerp_creator_camera,
                    preview::preview_idle,
                    preview::retint_preview,
                    panel::creator_panel,
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
