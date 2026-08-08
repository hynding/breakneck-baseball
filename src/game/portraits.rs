//! The portrait harness (`--features debug`, `--portraits <dir>`) — the AI's
//! eyes on every player the roster defines.
//!
//! `main.rs` parses `--portraits <dir>` (native + debug only — wasm has no
//! filesystem to write PNGs to and no CLI to parse them from) and, if
//! present, inserts a bare [`PortraitRun`] resource before `app.run()`. This
//! module owns the rest: force the app past the main menu into
//! [`crate::game::creator::CreatorState`], walk every (team, roster-slot)
//! pair through both framings the brief cares about (full-body on the
//! Identity tab, head close-up on Gear), and capture each via Bevy 0.15's
//! `Screenshot::primary_window()` + `save_to_disk` — the same one-shot API
//! `creator.rs`'s doc comment already cites as verified. When the queue
//! drains, it fires `AppExit`.
//!
//! No headless e2e test covers this module: `Screenshot::primary_window()`
//! needs a real window and a real GPU surface to read back from, neither of
//! which exist in the windowless harness `tests/common/mod.rs` builds — the
//! **run itself** (`cargo run --features "dev debug" -- --portraits <dir>`,
//! eyeballing the resulting PNGs) is the verification, per the task brief's
//! documented spec §6 deviation.

use std::collections::VecDeque;
use std::path::PathBuf;

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};

use crate::game::creator::{selected_def_ref, CreatorState, CreatorTab};
use crate::game::{GameState, Team};

/// Which shot of a player is being framed. Maps onto the two
/// [`CreatorTab`]s the brief calls out — Identity for the full body, Gear
/// for the head/gear close-up — kept as its own type (rather than driving
/// straight off `CreatorTab`) so the filename suffix and the queue's
/// two-shots-per-player structure don't have to know Colors/Animations
/// exist at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Framing {
    Full,
    Close,
}

impl Framing {
    fn tab(self) -> CreatorTab {
        match self {
            Framing::Full => CreatorTab::Identity,
            Framing::Close => CreatorTab::Gear,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Framing::Full => "full",
            Framing::Close => "close",
        }
    }
}

/// The driver's state machine. `queue`/`current` are populated lazily —
/// [`PortraitRun::new`] only knows the output directory; the actual (team,
/// index) pairs come from the live roster once the Creator stage is up and
/// `CreatorState::working` is populated (`enter_creator_stage` fills it from
/// `RosterDefs` the same frame the state transition lands), so this harness
/// automatically covers however many players the roster file defines rather
/// than hardcoding "26".
enum Phase {
    /// Boot has not yet reached `MainMenu` (should be true from frame one,
    /// but wait honestly rather than assume).
    WaitForMenu,
    /// `Creator` was requested; waiting for the state transition (and
    /// `enter_creator_stage`'s `OnEnter` spawn) to actually land.
    WaitForCreatorEnter,
    /// Letting wiring/dressing/the camera lerp catch up to the current
    /// (team, index, framing) before capturing it.
    Settle(Timer),
    /// Just spawned a `Screenshot` entity for the current (team, index,
    /// framing) and waiting a beat before mutating `CreatorState` again.
    /// Load-bearing, not cosmetic: `Screenshot::primary_window()`'s doc
    /// comment says it captures "this frame", but Update (where dressing
    /// reacts to a `PlayerIdentity` change) always finishes before that
    /// frame's render extraction — so advancing to the next player in the
    /// *same* Update tick that spawned the screenshot re-dresses the rig to
    /// the next player before the frame the screenshot targets ever
    /// renders, and the PNG ends up showing the next player instead of the
    /// one named in its filename. A short pause here guarantees at least
    /// one full frame renders (and gets captured) with nothing left to
    /// mutate before advancing.
    PostCapture(Timer),
    /// After the last capture request, waiting for the async GPU
    /// readback/PNG write to finish (the `Screenshot` entity despawns once
    /// its observer fires) before exiting — capped so a stuck readback
    /// can't hang the process forever.
    Draining(Timer),
    /// `AppExit` already sent; nothing left to do while the app winds down.
    Exited,
}

/// Dev-only driver resource. Absent unless `--portraits <dir>` was passed;
/// [`drive_portraits`] treats it as `Option<ResMut<PortraitRun>>` and no-ops
/// when it's `None`, so the harness costs nothing when unused.
#[derive(Resource)]
pub struct PortraitRun {
    dir: PathBuf,
    queue: VecDeque<(Team, usize)>,
    current: Option<(Team, usize)>,
    framing: Framing,
    phase: Phase,
}

/// How long to let wiring/dressing/the camera lerp settle before a capture —
/// per the brief, ~0.6 s covers glTF scene wiring, jersey lettering, and
/// `lerp_creator_camera`'s exponential approach landing close enough to its
/// target framing.
const SETTLE_SECS: f32 = 0.6;

/// How long [`Phase::PostCapture`] waits after spawning a `Screenshot`
/// before the driver is allowed to mutate `CreatorState` again — see that
/// variant's doc comment for why this has to be nonzero. A couple of frames'
/// worth at a conservative 30 fps floor, not a single `Timer::from_seconds`
/// tick, precisely because the risk is exactly one frame of staleness.
const POST_CAPTURE_SECS: f32 = 0.1;

/// Upper bound on how long [`Phase::Draining`] will wait for the last
/// screenshot's async save before exiting anyway — generous, but not
/// infinite, so a stuck GPU readback degrades to "harness exited a beat
/// early" instead of "harness never exits."
const DRAIN_TIMEOUT_SECS: f32 = 5.0;

impl PortraitRun {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            queue: VecDeque::new(),
            current: None,
            framing: Framing::Full,
            phase: Phase::WaitForMenu,
        }
    }
}

fn team_slug(team: Team) -> &'static str {
    match team {
        Team::Home => "home",
        Team::Away => "away",
    }
}

/// Drives the whole run. Deliberately not gated on any `GameState` run
/// condition — it has to observe both `MainMenu` (to request Creator) and
/// `Creator` (to know the request landed), so it runs every frame and reads
/// `State<GameState>` itself.
#[allow(clippy::too_many_arguments)]
fn drive_portraits(
    mut commands: Commands,
    run: Option<ResMut<PortraitRun>>,
    state: Res<State<GameState>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut cs: ResMut<CreatorState>,
    time: Res<Time>,
    shots: Query<Entity, With<Screenshot>>,
    mut exit: EventWriter<AppExit>,
) {
    let Some(mut run) = run else { return };

    match &mut run.phase {
        Phase::WaitForMenu => {
            if *state.get() == GameState::MainMenu {
                next_state.set(GameState::Creator);
                run.phase = Phase::WaitForCreatorEnter;
            }
        }

        Phase::WaitForCreatorEnter => {
            if *state.get() == GameState::Creator {
                // The whole (team, slot) queue, built once we can actually
                // see the working roster — see the struct doc.
                if run.queue.is_empty() && run.current.is_none() {
                    for team in [Team::Home, Team::Away] {
                        let len = match team {
                            Team::Home => cs.working.home.len(),
                            Team::Away => cs.working.away.len(),
                        };
                        for index in 0..len {
                            run.queue.push_back((team, index));
                        }
                    }
                }
                start_next_shot(&mut run, &mut cs);
            }
        }

        Phase::Settle(timer) => {
            timer.tick(time.delta());
            if timer.finished() {
                capture_current(&mut commands, &run, &cs);
                run.phase =
                    Phase::PostCapture(Timer::from_seconds(POST_CAPTURE_SECS, TimerMode::Once));
            }
        }

        Phase::PostCapture(timer) => {
            timer.tick(time.delta());
            if timer.finished() {
                advance_after_capture(&mut run, &mut cs);
            }
        }

        Phase::Draining(timer) => {
            timer.tick(time.delta());
            if shots.is_empty() || timer.finished() {
                exit.send(AppExit::Success);
                run.phase = Phase::Exited;
            }
        }

        Phase::Exited => {}
    }
}

/// Pops the next (team, index) off the queue and arms a fresh
/// `Phase::Settle` for its full-body framing, or — queue empty — starts
/// draining toward `AppExit`. Shared by the first entry into `Creator` and
/// by [`advance_after_capture`] once a player's close-up has been captured.
fn start_next_shot(run: &mut PortraitRun, cs: &mut CreatorState) {
    match run.queue.pop_front() {
        Some((team, index)) => {
            run.current = Some((team, index));
            run.framing = Framing::Full;
            cs.team = team;
            cs.index = index;
            cs.tab = run.framing.tab();
            run.phase = Phase::Settle(Timer::from_seconds(SETTLE_SECS, TimerMode::Once));
        }
        None => {
            run.current = None;
            run.phase = Phase::Draining(Timer::from_seconds(DRAIN_TIMEOUT_SECS, TimerMode::Once));
        }
    }
}

/// After a settled capture: on the full-body shot, switch to the close-up
/// framing for the *same* player and settle again; on the close-up shot,
/// move to the next player (or start draining, queue permitting).
fn advance_after_capture(run: &mut PortraitRun, cs: &mut CreatorState) {
    match run.framing {
        Framing::Full => {
            run.framing = Framing::Close;
            cs.tab = run.framing.tab();
            run.phase = Phase::Settle(Timer::from_seconds(SETTLE_SECS, TimerMode::Once));
        }
        Framing::Close => start_next_shot(run, cs),
    }
}

/// Spawns the actual `Screenshot::primary_window()` + `save_to_disk`
/// observer for `run.current` at `run.framing`, named
/// `<team>-<index>-<name>-<framing>.png`. Reads the player's name off
/// `cs.working` (the working copy, not `RosterDefs`) since that's what's
/// actually driving the preview rig's jersey lettering right now — the
/// working copy always equals a fresh clone of `RosterDefs` while the
/// Creator is open (`enter_creator_stage`), so this only differs from the
/// authored file mid-edit, which the harness never does.
fn capture_current(commands: &mut Commands, run: &PortraitRun, cs: &CreatorState) {
    let Some((team, index)) = run.current else {
        return;
    };
    let def = selected_def_ref(&cs.working, team, index);
    let filename = format!(
        "{}-{:02}-{}-{}.png",
        team_slug(team),
        index,
        def.name,
        run.framing.suffix()
    );
    let path = run.dir.join(filename);
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}

pub struct PortraitsPlugin;

impl Plugin for PortraitsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, drive_portraits);
    }
}
