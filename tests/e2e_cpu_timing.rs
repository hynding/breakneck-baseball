//! End-to-end coverage for the CPU batter's timing dial (Task B3).
//!
//! `ai::cpu_offense` used to decide *and* press a swing at the same instant
//! (a noisy trigger depth). Since B3 it draws a per-pitch `target_dt` from
//! `cpu_timing_spread_ms` and presses only once the live `swing_dt_ms` (see
//! `flow.rs`) reaches it — the CPU should now scatter its swings across more
//! than one `ContactQuality`, not always land in the same band.
//!
//! Only the *pitch* is scripted (a human pitcher lobs straightaway
//! changeups); every swing decision and its timing is the CPU's own, exactly
//! like `e2e_cpu.rs`.
//!
//! **Margin, not marginal (review fix):** Rapier/Bevy's multithreaded
//! physics isn't bit-reproducible run-to-run, so a variety assertion at the
//! narrow `cpu_timing_spread_ms` is knife-edge — most presses land
//! `Perfect` and only a sliver reaches `Solid`, so a few ms of
//! nondeterministic jitter flips the result. This test instead forces
//! `Ruleset::cpu_timing_spread_ms` to 200 ms right after kickoff — wide
//! enough, now that `flow::late_swing_z` lets the swing window stay open the
//! whole `foul_ms` (forced to 140 ms below), that `Perfect`/`Solid`/`FoulTip` are all
//! comfortably reachable regardless of scheduler ordering. The exact
//! press-frame *logic* is pinned bit-for-bit by the synthetic-ramp unit
//! tests in `ai.rs` (`ready_to_press_*`); this e2e only needs to prove the
//! wiring reaches the ECS/physics live, with a fat margin against jitter.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::flow::{ContactEvent, Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::ContactQuality;
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{headless_app, run_until, start_game, DriveGame};

/// A forced spread decoupled from the tuned default: with `foul_ms` forced
/// to 140 ms below, this comfortably spans Perfect/Solid/FoulTip so the
/// variety assertion has fat margin against run-to-run physics jitter
/// instead of hinging on a handful of milliseconds.
const FORCED_SPREAD_MS: f32 = 200.0;

/// Generous headroom: 20 CPU-faced pitches, across possibly several
/// half-innings if the CPU's side keeps making outs and the game keeps
/// cycling back to its turn at bat.
const MAX_FRAMES: u64 = 600_000;

/// How many pitches the CPU batter must face before judging variety.
const TARGET_PITCHES: u32 = 20;

/// Counts pitches actually thrown to the CPU batter (fresh entries into
/// `Phase::Pitch` while Home fields, i.e. Away — the CPU in one-player mode —
/// bats) so the test can stop once it has seen enough of them.
#[derive(Resource, Default)]
struct PitchCount {
    seen: u32,
    was_pitch: bool,
}

/// Every graded swing (whiffs included — see `ContactEvent`'s doc comment in
/// `flow.rs`) the CPU actually makes contact-decisions on.
#[derive(Resource, Default)]
struct Qualities(Vec<ContactQuality>);

/// Pitches straightaway whenever the human (Home) fields, exactly like
/// `e2e_cpu.rs`; the CPU bats — including its swing timing — on its own.
fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
    mut count: ResMut<PitchCount>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    if score.fielding_team() == Team::Home {
        if play.phase == Phase::PrePitch {
            intents.home.action = true;
        }
        let is_pitch = play.phase == Phase::Pitch;
        if is_pitch && !count.was_pitch {
            count.seen += 1;
        }
        count.was_pitch = is_pitch;
    } else {
        count.was_pitch = false;
    }
}

fn capture(mut ev: EventReader<ContactEvent>, mut qualities: ResMut<Qualities>) {
    for e in ev.read() {
        qualities.0.push(e.quality);
    }
}

#[test]
fn cpu_batters_swing_with_varied_timing() {
    let mut app = headless_app();
    app.init_resource::<PitchCount>();
    app.init_resource::<Qualities>();
    app.add_systems(DriveGame, (drive, capture));
    start_game(&mut app, KeyCode::Digit1);
    {
        // Force a wide spread and pin the "classic" contact windows this test's
        // margin reasoning (below) assumes. The shipped Standard windows are the
        // B7 balance harness's to tune (`tests/balance_sim.rs` / `variant.rs`);
        // this test proves the *timing-dial wiring* reaches the ECS, not the
        // balance economy, so it owns the windows it depends on.
        let mut r = app.world_mut().resource_mut::<Ruleset>();
        r.batting.cpu_timing_spread_ms = FORCED_SPREAD_MS;
        r.batting.perfect_ms = 40.0;
        r.batting.solid_ms = 90.0;
        r.batting.foul_ms = 140.0;
    }

    run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<PitchCount>().seen >= TARGET_PITCHES
    });

    let seen = app.world().resource::<PitchCount>().seen;
    assert!(
        seen >= TARGET_PITCHES,
        "only saw {seen} CPU pitches inside the frame budget (wanted {TARGET_PITCHES})"
    );

    let qualities = app.world().resource::<Qualities>().0.clone();
    let mut distinct: Vec<ContactQuality> = Vec::new();
    for q in &qualities {
        if !distinct.contains(q) {
            distinct.push(*q);
        }
    }
    assert!(
        distinct.len() >= 2,
        "expected the CPU's deterministic timing spread to produce at least 2 \
         distinct contact qualities across {seen} pitches, got {qualities:?}"
    );
}
