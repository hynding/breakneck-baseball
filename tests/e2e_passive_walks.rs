//! The walk-chain probe (TODO 10): a completely passive human batter must
//! not draw endless CPU walks. Observed in browser play at ~7 BB/inning and
//! reproduced headlessly at ~6 BB per passive half before the fix; the
//! behind-in-the-count package (`ai::cpu_defense` zone pull paired with the
//! `ai::cpu_offense` ahead-count compensation) holds it down.
//!
//! The probe stages the bottom of the 1st via the scenario seam (human Home
//! bats, CPU Away pitches), injects nothing at all for the human, and
//! counts WALK banners until the half ends. `tests/balance_sim.rs` remains
//! the arbiter of the CPU-vs-CPU economy; this test guards the
//! human-facing symptom the sim cannot see.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::flow::PlayBanner;
use breakneck_baseball::game::scenario::{self, Scenario};
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DriveGame, deterministic_headless_app, run_until, start_game};

/// ≈ 20 sim-minutes at 240 Hz — a passive half is long (deep counts every
/// plate appearance) but must end by strikeouts well inside this.
const MAX_FRAMES: u64 = 300_000;

/// Walks a passive batter may draw in one half-inning. The pre-fix
/// baseline measured ~6; the accepted post-fix level leaves room for the
/// occasional genuine miss cluster without letting chains regress.
const MAX_WALKS: u32 = 3;

#[derive(Resource, Default)]
struct WalkCount(u32);

#[derive(Resource, Default)]
struct BannerTally(std::collections::BTreeMap<String, u32>);

/// Counts WALK banners from the injection schedule (events persist across
/// the frame boundary, so a 1-per-frame reader never misses one).
fn count_walks(
    mut banners: EventReader<PlayBanner>,
    mut walks: ResMut<WalkCount>,
    mut tally: ResMut<BannerTally>,
) {
    for banner in banners.read() {
        if banner.text.starts_with("WALK") {
            walks.0 += 1;
        }
        *tally.0.entry(banner.text.clone()).or_insert(0) += 1;
    }
}

#[test]
fn passive_batter_does_not_draw_walk_chains() {
    let mut app = deterministic_headless_app();
    app.init_resource::<WalkCount>();
    app.init_resource::<BannerTally>();
    app.add_systems(DriveGame, count_walks);
    start_game(&mut app, KeyCode::Digit1); // human Home vs CPU Away

    // Jump straight to the bottom of the 1st: Home (the silent human) bats,
    // the CPU pitches. Fresh count, nobody aboard.
    let staged = Scenario {
        name: "passive bottom 1",
        top: false,
        ..Default::default()
    };
    scenario::apply_to_world(app.world_mut(), &staged).expect("scenario applies");

    // Run until the half flips back to the top of the 2nd (three passive
    // outs) or the game somehow ends.
    let flipped = run_until(&mut app, MAX_FRAMES, |app| {
        let s = app.world().resource::<ScoreBoard>();
        let over = *app.world().resource::<State<GameState>>().get() == GameState::GameOver;
        over || s.top_of_inning || s.inning > 1
    });
    assert!(flipped.is_some(), "the passive half never ended");

    let walks = app.world().resource::<WalkCount>().0;
    eprintln!("passive half: {walks} walk(s)");
    for (text, n) in &app.world().resource::<BannerTally>().0 {
        eprintln!("  {n:>3}  {text}");
    }
    assert!(
        walks <= MAX_WALKS,
        "a passive batter drew {walks} walks in one half (max {MAX_WALKS}) — \
         the CPU walk chains are back (TODO 10)"
    );
}
