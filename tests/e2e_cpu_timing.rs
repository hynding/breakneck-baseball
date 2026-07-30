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

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::flow::{ContactEvent, Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::ContactQuality;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{headless_app, run_until, start_game, DriveGame};

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
