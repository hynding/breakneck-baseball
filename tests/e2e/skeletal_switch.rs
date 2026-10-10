//! `HeadlessConfig::skip_skeletal_sampling` (TODO 29 lever 2) rests on the
//! premise that bone poses are cosmetic: the rules and the Coach read rig
//! root transforms, contact is judged by timing, catches by fielder
//! position. Within one process the single-threaded sim is bit-deterministic
//! (the second test here pins that), so the same CPU-vs-CPU inning played
//! twice, with and without the switch, should end on the same frame with the
//! same score.
//!
//! **It doesn't (2026-10-10).** Same score, 8161 vs 8116 frames. The lockstep
//! diagnostic below finds the first difference at frame 444: the ball at the
//! plate, physics identical, and every fielder root moved by `locomote` one
//! frame earlier with sampling — their `MoveIntent` targets were set a frame
//! apart. No `sim/` system reads a bone; the rig roots carry no animation
//! components; Rapier's body pose matches the root transform in both runs;
//! and moving the stripping system between schedules changes nothing. The
//! remaining suspect is iteration order: stripping creates extra bone
//! archetypes at spawn, which shifts archetype ids and so the order a query
//! yields fielders, and a decision that depends on that order lands a frame
//! apart. That is a determinism hazard in its own right (archetype creation
//! order is not something gameplay should depend on). The premise test stays
//! `#[ignore]`d as the ready-made gate for whoever attributes and fixes it;
//! the balance sim keeps sampling until it passes.

use crate::common;

use std::time::Duration;

use bevy::prelude::*;
use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::Play;
use breakneck_baseball::game::input::{Controllers, InputSource};
use breakneck_baseball::game::rules::Bases;
use breakneck_baseball::game::{GameState, ScoreBoard};
use common::{HeadlessConfig, headless_app_with, run_until, start_game, tap_key};

/// The balance sim's step.
const SIM_DT: f64 = 1.0 / 100.0;
const MAX_FRAMES: u64 = 400_000;

#[derive(Debug, PartialEq)]
struct Outcome {
    frames: u64,
    inning: u32,
    top: bool,
    outs: u32,
    home: u32,
    away: u32,
    occupied: [bool; 3],
}

fn play_one_cpu_inning(skip_skeletal_sampling: bool) -> Outcome {
    let mut app = headless_app_with(HeadlessConfig {
        single_threaded: true,
        skip_skeletal_sampling,
        ..Default::default()
    });
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(SIM_DT),
    ));
    // 9 -> 1 inning, one-player mode, both slots to the CPU: `balance_sim`.
    tap_key(&mut app, KeyCode::KeyI);
    start_game(&mut app, KeyCode::Digit1);
    *app.world_mut().resource_mut::<Controllers>() = Controllers {
        home: InputSource::Cpu,
        away: InputSource::Cpu,
        ..Controllers::default()
    };
    let frames = run_until(&mut app, MAX_FRAMES, |app| {
        let over = *app.world().resource::<State<GameState>>().get() == GameState::GameOver;
        over || app.world().resource::<ScoreBoard>().inning >= 2
    })
    .expect("one CPU-vs-CPU inning never finished");
    let score = app.world().resource::<ScoreBoard>();
    let bases = app.world().resource::<Bases>();
    Outcome {
        frames,
        inning: score.inning,
        top: score.top_of_inning,
        outs: score.outs,
        home: score.home_runs,
        away: score.away_runs,
        occupied: [
            bases.is_occupied(0),
            bases.is_occupied(1),
            bases.is_occupied(2),
        ],
    }
}

#[test]
#[ignore = "fails today (see the module doc): the gate for adopting skip_skeletal_sampling in balance_sim"]
fn skipping_skeletal_sampling_does_not_change_the_inning() {
    let sampled = play_one_cpu_inning(false);
    let skipped = play_one_cpu_inning(true);
    assert_eq!(
        sampled, skipped,
        "the same inning diverged with skeletal sampling skipped — some sim \
         system now depends on a bone pose; drop the switch from balance_sim"
    );
}

/// The control for the test above, and the premise `balance_sim` rests on:
/// two apps in one process play the same inning identically.
#[test]
fn the_same_inning_replays_identically_in_one_process() {
    let first = play_one_cpu_inning(false);
    let second = play_one_cpu_inning(false);
    assert_eq!(
        first, second,
        "in-process replay is no longer deterministic"
    );
}

/// Diagnostic, not a gate: steps the two configurations in lockstep and
/// prints the first frame where they differ — play phase, ball, and every
/// moving rig root with its clip — so a divergence can be attributed to a
/// mechanism rather than guessed at.
#[test]
#[ignore = "diagnostic for skipping_skeletal_sampling_does_not_change_the_inning (prints, never fails)"]
fn where_the_skipped_inning_first_diverges() {
    use breakneck_baseball::game::animation::{MoveIntent, Playing};

    fn boot(skip: bool) -> App {
        let mut app = headless_app_with(HeadlessConfig {
            single_threaded: true,
            skip_skeletal_sampling: skip,
            ..Default::default()
        });
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(SIM_DT),
        ));
        tap_key(&mut app, KeyCode::KeyI);
        start_game(&mut app, KeyCode::Digit1);
        *app.world_mut().resource_mut::<Controllers>() = Controllers {
            home: InputSource::Cpu,
            away: InputSource::Cpu,
            ..Controllers::default()
        };
        app
    }

    fn fingerprint(app: &mut App) -> Vec<String> {
        let world = app.world_mut();
        let ball = world
            .query_filtered::<&Transform, With<Baseball>>()
            .single(world)
            .map(|t| t.translation)
            .unwrap_or(Vec3::NAN);
        let play = world.resource::<Play>();
        let s = world.resource::<ScoreBoard>();
        let mut tokens = vec![
            format!("phase={:?}", play.phase),
            format!("ball=({:.3},{:.3},{:.3})", ball.x, ball.y, ball.z),
            format!(
                "inn={}/{}/outs{}/b{}/s{}/{}-{}",
                s.inning, s.top_of_inning, s.outs, s.balls, s.strikes, s.away_runs, s.home_runs
            ),
        ];
        let mut rigs: Vec<String> = world
            .query::<(Entity, &Transform, &MoveIntent, Option<&Playing>)>()
            .iter(world)
            .map(|(e, t, _, p)| {
                format!(
                    "rig{}=({:.3},{:.3},{:.3}){}",
                    e.index(),
                    t.translation.x,
                    t.translation.y,
                    t.translation.z,
                    p.map(|p| format!("[{:?}]", p.clip)).unwrap_or_default()
                )
            })
            .collect();
        rigs.sort();
        tokens.extend(rigs);
        tokens
    }

    /// Change-tick and physics state of one rig root this frame.
    fn watch(app: &mut App, index: u32) -> String {
        use bevy::animation::{AnimationPlayer, AnimationTarget};
        use bevy_rapier3d::prelude::{RapierRigidBodyHandle, RapierRigidBodySet, RigidBody};
        let world = app.world_mut();
        let Some(entity) = world
            .query_filtered::<Entity, With<MoveIntent>>()
            .iter(world)
            .find(|e| e.index() == index)
        else {
            return "no such rig".into();
        };
        let handle = world
            .entity(entity)
            .get::<RapierRigidBodyHandle>()
            .map(|h| h.0);
        let body_pos = handle
            .and_then(|h| {
                world
                    .query::<&RapierRigidBodySet>()
                    .iter(world)
                    .next()
                    .and_then(|set| set.bodies.get(h))
                    .map(|rb| {
                        let p = rb.position().translation;
                        format!("({:.3},{:.3},{:.3})", p.x, p.y, p.z)
                    })
            })
            .unwrap_or_else(|| "none".into());
        let e = world.entity(entity);
        let t = e.get_ref::<Transform>().expect("rig Transform");
        let g = e.get_ref::<GlobalTransform>().expect("rig GlobalTransform");
        format!(
            "T=({:.3},{:.3},{:.3}) T.changed={} G.changed={} body={} rb={:?} target={} player={}",
            t.translation.x,
            t.translation.y,
            t.translation.z,
            t.is_changed(),
            g.is_changed(),
            body_pos,
            e.get::<RigidBody>(),
            e.contains::<AnimationTarget>(),
            e.contains::<AnimationPlayer>(),
        )
    }

    let mut a = boot(false);
    let mut b = boot(true);
    let mut prev: Vec<String> = Vec::new();
    for frame in 1..=MAX_FRAMES {
        a.update();
        b.update();
        if (441..=445).contains(&frame) {
            println!("watch frame {frame} rig116 sampled: {}", watch(&mut a, 116));
            println!("watch frame {frame} rig116 skipped: {}", watch(&mut b, 116));
        }
        let fa = fingerprint(&mut a);
        let fb = fingerprint(&mut b);
        if fa != fb {
            println!("first divergence at frame {frame}:");
            println!("  previous frame (both): {}", prev.join(" "));
            let diff: Vec<String> = fa
                .iter()
                .zip(fb.iter())
                .filter(|(x, y)| x != y)
                .map(|(x, y)| format!("{x} != {y}"))
                .collect();
            println!("  differs: {}", diff.join(" | "));
            if fa.len() != fb.len() {
                println!("  (token counts differ: {} vs {})", fa.len(), fb.len());
            }
            return;
        }
        prev = fa;
        let over = *a.world().resource::<State<GameState>>().get() == GameState::GameOver;
        if over || a.world().resource::<ScoreBoard>().inning >= 2 {
            println!("no divergence over {frame} frames");
            return;
        }
    }
    panic!("inning never finished");
}
