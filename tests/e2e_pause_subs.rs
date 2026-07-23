//! End-to-end: pausing between plays opens the substitution board, a bench
//! swap rewrites the roster, and resuming leaves the scene intact — the
//! Playing ⇄ Paused transitions must neither tear down nor respawn the world.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::roster::Rosters;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{headless_app, run_until, DriveGame};

const MAX_FRAMES: u64 = 20_000;

#[derive(Resource, Default)]
struct MenuScript {
    frame: u64,
}

/// A queued key tap, applied from the [`DriveGame`] schedule — pressing the
/// resource directly from the test body would be wiped by the input plugin's
/// PreUpdate clear before any Update system saw `just_pressed`.
#[derive(Resource, Default)]
struct TapKey(Option<(KeyCode, u8)>);

fn apply_taps(mut tap: ResMut<TapKey>, mut keyboard: ResMut<ButtonInput<KeyCode>>) {
    if let Some((key, frames_left)) = tap.0 {
        if frames_left > 0 {
            keyboard.press(key);
            tap.0 = Some((key, frames_left - 1));
        } else {
            keyboard.release(key);
            tap.0 = None;
        }
    }
}

/// Presses **2** on the menu; in play, the fielding side throws straightaway
/// changeups and nobody swings — enough traffic to prove the game runs.
fn drive(
    state: Res<State<GameState>>,
    mut script: ResMut<MenuScript>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
) {
    match state.get() {
        GameState::MainMenu => {
            script.frame += 1;
            match script.frame {
                10 => keyboard.press(KeyCode::Digit2),
                12 => keyboard.release(KeyCode::Digit2),
                _ => {}
            }
        }
        GameState::Playing => {
            let (Some(play), Some(score)) = (play, score) else {
                return;
            };
            intents.home = default();
            intents.away = default();
            if play.phase == Phase::PrePitch {
                intents.get_mut(score.fielding_team()).action = true;
            }
        }
        _ => {}
    }
}

fn tap_key(app: &mut App, key: KeyCode) {
    app.world_mut().resource_mut::<TapKey>().0 = Some((key, 1));
    for _ in 0..4 {
        app.update();
    }
}

fn state(app: &App) -> GameState {
    app.world().resource::<State<GameState>>().get().clone()
}

#[test]
fn pause_swaps_the_bench_and_resumes_cleanly() {
    let mut app = headless_app();
    app.init_resource::<MenuScript>();
    app.init_resource::<TapKey>();
    app.add_systems(DriveGame, (apply_taps, drive).chain());

    let started = run_until(&mut app, MAX_FRAMES, |app| state(app) == GameState::Playing);
    assert!(started.is_some(), "menu never started the game");

    // Wait for a dead ball (waiting on a pitch), then pause.
    let ready = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<Play>().phase == Phase::PrePitch
    });
    assert!(ready.is_some(), "never reached a PrePitch dead ball");
    tap_key(&mut app, KeyCode::Escape);
    assert_eq!(state(&app), GameState::Paused, "Esc between plays pauses");

    // The scene survives the pause: the ball entity is still there.
    let balls = |app: &mut App| {
        app.world_mut()
            .query_filtered::<(), With<Baseball>>()
            .iter(app.world())
            .count()
    };
    assert_eq!(balls(&mut app), 1, "pausing must not tear the scene down");

    // Swap the top of the order (cursor starts at slot 0 / bench 0) for the
    // batting team — Away, in the top of the 1st.
    let before = app.world().resource::<Rosters>().away.clone();
    tap_key(&mut app, KeyCode::Enter);
    let after = app.world().resource::<Rosters>().away.clone();
    assert_eq!(after.lineup[0], before.bench[0], "bench player subbed in");
    assert_eq!(
        after.bench[0], before.lineup[0],
        "starter took the bench seat"
    );

    // Resume: exactly one ball (no duplicate scene spawn), and the game keeps
    // playing — the scripted pitcher works the count against a taking batter.
    tap_key(&mut app, KeyCode::Escape);
    assert_eq!(state(&app), GameState::Playing, "Esc again resumes");
    assert_eq!(balls(&mut app), 1, "resuming must not respawn the scene");

    let progressed = run_until(&mut app, MAX_FRAMES, |app| {
        let s = app.world().resource::<ScoreBoard>();
        s.balls + s.strikes > 0 || s.outs > 0
    });
    assert!(
        progressed.is_some(),
        "the game must keep running after a pause/resume cycle"
    );
}
