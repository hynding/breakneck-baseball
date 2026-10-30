//! A fielded ball stays with its fielder (regression).
//!
//! Observed 2026-10-30: after an outfielder caught a fly, the ball stayed
//! visible on the grass where he caught it while he jogged back to his spot
//! with nothing. The catch only stopped the ball dead, so the dynamic body
//! dropped onto the field and lay there until the mound reset; a received
//! throw did the same at the bag. The fix tracks the possessor in
//! `fielding::ActivePlay` and carries the ball at his glove.
//!
//! This probe runs CPU-vs-CPU and, from every `Caught`/`Settled` report until
//! the ball is thrown again or the next pitch is set up, checks that the
//! visible ball is up off the grass and right with a fielder.

use crate::common;

use bevy::ecs::message::Messages;
use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{LiveBallEvent, Phase, Play};
use breakneck_baseball::game::input::{Controllers, InputSource};
use breakneck_baseball::game::player::Fielder;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{deterministic_headless_app, run_until, start_game};

/// A held ball is this close (m) to its fielder's rig root.
const WITH_FIELDER_M: f32 = 1.5;
/// Below this height (m) a held ball would be lying on the grass.
const ON_GRASS_Y: f32 = 0.3;
/// Possession plays to watch before calling the probe done.
const PLAYS: u32 = 12;

#[test]
fn caught_ball_stays_with_the_fielder() {
    let mut app = deterministic_headless_app();
    start_game(&mut app, KeyCode::Digit1);
    *app.world_mut().resource_mut::<Controllers>() = Controllers {
        home: InputSource::Cpu,
        away: InputSource::Cpu,
        ..Controllers::default()
    };

    let mut cursor = app
        .world()
        .resource::<Messages<LiveBallEvent>>()
        .get_cursor();
    let mut held = false;
    let mut plays = 0u32;
    let mut held_frames = 0u32;
    let mut loose: Vec<(Vec3, f32)> = Vec::new();

    run_until(&mut app, 400_000, |app| {
        let world = app.world_mut();
        for ev in cursor.read(world.resource::<Messages<LiveBallEvent>>()) {
            match ev {
                LiveBallEvent::Caught { .. } | LiveBallEvent::Settled => {
                    if !held {
                        plays += 1;
                    }
                    held = true;
                }
                LiveBallEvent::Thrown { .. } => held = false,
                LiveBallEvent::Landed { .. } => {}
            }
        }
        let phase = world.resource::<Play>().phase;
        if !matches!(phase, Phase::InPlay | Phase::Result) {
            held = false;
        }
        if held {
            let mut ball_q = world.query_filtered::<(&Transform, &Visibility), With<Baseball>>();
            let ball = ball_q
                .iter(world)
                .next()
                .filter(|(_, vis)| **vis != Visibility::Hidden)
                .map(|(tf, _)| tf.translation);
            if let Some(ball) = ball {
                held_frames += 1;
                let mut fielders = world.query_filtered::<&Transform, With<Fielder>>();
                let nearest = fielders
                    .iter(world)
                    .map(|tf| tf.translation.distance(ball))
                    .fold(f32::INFINITY, f32::min);
                if nearest > WITH_FIELDER_M || ball.y < ON_GRASS_Y {
                    loose.push((ball, nearest));
                }
            }
        }
        (plays >= PLAYS && !held)
            || *world.resource::<State<GameState>>().get() == GameState::GameOver
    });

    let inning = app.world().resource::<ScoreBoard>().inning;
    assert!(
        plays >= 3 && held_frames > 0,
        "the probe must see the defense field the ball (plays {plays}, held frames \
         {held_frames}, inning {inning})"
    );
    assert!(
        loose.is_empty(),
        "{} of {held_frames} held frames across {plays} plays showed the ball away from \
         every fielder or lying on the grass; first: ball at {:?}, nearest fielder {:.2} m",
        loose.len(),
        loose[0].0,
        loose[0].1,
    );
}
