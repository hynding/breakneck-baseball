//! Fielders return to their set spots between at-bats (TODO 30 regression).
//!
//! Observed in a browser game 2026-08-20: after a play that pulled the
//! defense off their spots to cover bases, fielders stayed parked at the
//! covered bases (e.g. standing at home plate) through several subsequent
//! at-bats. Root cause: on an instantly-resolved play (a liner caught the
//! same frame the cover orders went out), `fielding::return_to_spots` fired
//! while every fielder was still within its "already set" tolerance — so it
//! sent nobody home, consumed its one-shot state, and left the cover
//! `MoveIntent`s live. The defense then ran to the bases *after* the play
//! and parked there until the next ball in play. The fix voids every
//! outstanding fielding order when the play ends.
//!
//! This probe runs a whole CPU-vs-CPU game and, at every delivery (the frame
//! `Phase::WindUp` begins), checks each fielder against his
//! `FieldSpec::fielder_positions` spot. A fielder counts toward the parked
//! streak only when he is off his spot **and not ordered back to it** — an
//! outfielder still jogging home from a deep backup station across a few
//! quick deliveries is legal baseball (instrumented 2026-08-24: every
//! off-spot fielder across 222 deliveries carried the correct return
//! order, and pure distance flagged a jogger 2.7 m from home). A live
//! order pointing anywhere *else* — a cover bag, a stale intercept — is
//! precisely the TODO 30 bug and still counts.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::{Controllers, InputSource};
use breakneck_baseball::game::player::Fielder;
use breakneck_baseball::game::variant::FieldSpec;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{deterministic_headless_app, run_until, start_game};

/// A fielder this far (m) from his spot at delivery is "off his spot".
const OFF_SPOT_M: f32 = 2.0;
/// Consecutive off-spot, not-returning deliveries that count as parked.
const PARKED_STREAK: u32 = 3;
/// An order within this (m) of the spot counts as "heading home".
const HOMEBOUND_M: f32 = 0.5;

#[test]
fn fielders_are_set_before_every_delivery() {
    let mut app = deterministic_headless_app();
    start_game(&mut app, KeyCode::Digit1);
    *app.world_mut().resource_mut::<Controllers>() = Controllers {
        home: InputSource::Cpu,
        away: InputSource::Cpu,
    };

    let mut last_phase = Phase::PrePitch;
    let mut deliveries: u32 = 0;
    // Per fielder index: (current consecutive off-spot streak, worst streak,
    // sample position from the worst streak).
    let mut streaks: Vec<(u32, u32, Vec3)> = Vec::new();

    run_until(&mut app, 400_000, |app| {
        let phase = app.world().resource::<Play>().phase;
        let windup_started = phase == Phase::WindUp && last_phase != Phase::WindUp;
        last_phase = phase;
        if windup_started {
            deliveries += 1;
            let world = app.world_mut();
            let spots = world.resource::<FieldSpec>().fielder_positions.clone();
            let mut q = world.query::<(
                &Fielder,
                &Transform,
                &breakneck_baseball::game::animation::MoveIntent,
            )>();
            for (fielder, tf, intent) in q.iter(world) {
                if streaks.len() <= fielder.index {
                    streaks.resize(fielder.index + 1, (0, 0, Vec3::ZERO));
                }
                let Some(spot) = spots.get(fielder.index) else {
                    continue;
                };
                let d = Vec2::new(tf.translation.x - spot.x, tf.translation.z - spot.z).length();
                // Jogging home is legal; parked (no order) or ordered
                // anywhere else (a stale cover/intercept) is the bug.
                let heading_home = intent
                    .target
                    .is_some_and(|t| Vec2::new(t.x - spot.x, t.z - spot.z).length() < HOMEBOUND_M);
                let entry = &mut streaks[fielder.index];
                if d > OFF_SPOT_M && !heading_home {
                    entry.0 += 1;
                    if entry.0 > entry.1 {
                        entry.1 = entry.0;
                        entry.2 = tf.translation;
                    }
                } else {
                    entry.0 = 0;
                }
            }
        }
        *app.world().resource::<State<GameState>>().get() == GameState::GameOver
    });

    let s = app.world().resource::<ScoreBoard>();
    println!(
        "probe: {deliveries} deliveries, final inning {} ({}-{})",
        s.inning, s.home_runs, s.away_runs
    );

    assert!(
        deliveries > 20,
        "probe never saw a meaningful number of deliveries ({deliveries})"
    );
    let parked: Vec<String> = streaks
        .iter()
        .enumerate()
        .filter(|(_, (_, worst, _))| *worst >= PARKED_STREAK)
        .map(|(i, (_, worst, at))| {
            format!("fielder {i} off-spot {worst} deliveries running (at {at:?})")
        })
        .collect();
    assert!(
        parked.is_empty(),
        "parked fielders detected:\n{}",
        parked.join("\n")
    );
}
