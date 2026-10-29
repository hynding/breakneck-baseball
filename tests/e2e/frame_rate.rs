//! The game plays the same at 30 fps as at 240 fps.
//!
//! Every other suite steps at 240 Hz, where bevy_rapier's default 1/60 s
//! step cap never bites. Below 60 fps that cap made physics cover only part
//! of each frame while drag, locomotion and timers took all of it, so a
//! pitch reached the plate late and slow (`game::physics_clock`). This suite
//! throws the same pitch at both frame rates and compares its flight.

use crate::common;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::GameState;
use breakneck_baseball::game::ScoreBoard;
use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;

use common::{DriveGame, headless_app, run_until, start_game};

/// Pitches as soon as the fielding side may; nobody swings.
fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
) {
    intents.home = default();
    intents.away = default();
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    if play.phase == Phase::PrePitch {
        intents.get_mut(score.fielding_team()).action = true;
    }
}

/// The first pitch's flight: release instant, and the instant and speed at
/// which it crossed the plate (z = 0).
#[derive(Resource, Default)]
struct Flight {
    released_at: Option<f32>,
    last: Option<(f32, f32)>,
    crossed: Option<(f32, f32)>,
}

/// Samples after the physics step, so each sample is the frame's final state.
fn sample(
    time: Res<Time>,
    ball: Query<(&Transform, &Velocity), With<Baseball>>,
    mut flight: ResMut<Flight>,
) {
    if flight.crossed.is_some() {
        return;
    }
    let Ok((t, v)) = ball.single() else {
        return;
    };
    let now = time.elapsed_secs();
    let z = t.translation.z;
    if flight.released_at.is_none() {
        if v.linvel.z < -1.0 {
            // The release frame's physics step covered the whole frame, so
            // the ball left the hand at that frame's start.
            flight.released_at = Some(now - time.delta_secs());
        } else {
            flight.last = Some((now, z));
            return;
        }
    }
    if z <= 0.0 {
        let (t0, z0) = flight.last.expect("a sample precedes the crossing");
        let at = t0 + (now - t0) * z0 / (z0 - z);
        flight.crossed = Some((at, v.linvel.length()));
    }
    flight.last = Some((now, z));
}

/// (seconds from release to the plate, speed at the plate) at `fps`.
fn pitch_flight(fps: f64) -> (f32, f32) {
    let mut app = headless_app();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_secs_f64(1.0 / fps),
    ));
    app.init_resource::<Flight>();
    app.add_systems(DriveGame, drive);
    app.add_systems(Last, sample);
    start_game(&mut app, KeyCode::Digit2);
    // ~60 s of game time at either rate.
    let frames = (60.0 * fps) as u64;
    let crossed = run_until(&mut app, frames, |app| {
        app.world().resource::<Flight>().crossed.is_some()
    });
    assert!(
        crossed.is_some(),
        "{fps} fps: the pitch never crossed the plate"
    );
    let flight = app.world().resource::<Flight>();
    let (at, speed) = flight.crossed.unwrap();
    (at - flight.released_at.unwrap(), speed)
}

#[test]
fn a_pitch_flies_the_same_at_30_fps_as_at_240() {
    let (secs_240, speed_240) = pitch_flight(240.0);
    let (secs_30, speed_30) = pitch_flight(30.0);
    assert!(
        (secs_30 - secs_240).abs() < 0.02,
        "pitch took {secs_30:.3} s to reach the plate at 30 fps vs {secs_240:.3} s at 240 fps"
    );
    assert!(
        (speed_30 - speed_240).abs() / speed_240 < 0.02,
        "pitch crossed the plate at {speed_30:.2} m/s at 30 fps vs {speed_240:.2} m/s at 240 fps"
    );
}
