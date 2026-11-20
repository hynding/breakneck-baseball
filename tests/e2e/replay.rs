//! End-to-end: instant replay (graphics plan task 9). The scripted home run
//! from `home_run_moment` must get a replay a beat into its result
//! pause; while it plays the gameplay clock stands still, the rendered rigs
//! show the recorded play while their live transforms never change, and
//! when it ends the game picks up exactly where it stopped. A confirm press
//! skips it, and the Off setting means none.

use crate::common;

use bevy::animation::AnimationTargetId;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::animation::RigLimb;
use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::camera::CameraMode;
use breakneck_baseball::game::flow::{Phase, Play, bat_arrival_z};
use breakneck_baseball::game::input::{Controllers, InputSource, Intents};
use breakneck_baseball::game::player::{Batter, RigUnitTag};
use breakneck_baseball::game::quality::{ActiveQuality, QualityTier};
use breakneck_baseball::game::replay::{Replay, SKIP_GRACE};
use breakneck_baseball::game::settings::{ReplayMode, Settings};
use breakneck_baseball::game::ui::{ReplayTagLine, ReplayTagText, SKIP_PROMPT};
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{DT, DriveGame, headless_app, run_until, start_game, tap_key};

const MAX_FRAMES: u64 = 100_000;

/// Whether the drive should mash the confirm button during a replay.
#[derive(Resource, Default)]
struct SkipReplays(bool);

fn pin_classic_contact_windows(app: &mut App) {
    let mut r = app.world_mut().resource_mut::<Ruleset>();
    r.batting.perfect_ms = 40.0;
    r.batting.solid_ms = 90.0;
    r.batting.foul_ms = 140.0;
    r.batting.exit_solid = 1.0;
    r.batting.exit_perfect = 1.25;
}

/// The `home_run_moment` script: Away strikes out looking, Home swings a
/// dead-red uppercut for a home run.
fn drive(
    state: Res<State<GameState>>,
    skip: Res<SkipReplays>,
    replay: Res<Replay>,
    mut intents: ResMut<Intents>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    ball: Query<(&Transform, &Velocity), With<Baseball>>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    if replay.is_playing() {
        intents.away.action = skip.0;
        return;
    }
    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(score.fielding_team()).action = true;
        }
        Phase::Pitch if score.batting_team() == Team::Home => {
            if let Ok((t, v)) = ball.single() {
                intents.home.aim = Vec2::new(0.0, 1.0);
                let z = bat_arrival_z(t.translation.z, v.linvel.z);
                if (0.0..=0.45).contains(&z) {
                    intents.home.action = true;
                }
            }
        }
        _ => {}
    }
}

fn app_with(mode: ReplayMode, skip: bool) -> App {
    let mut app = headless_app();
    app.insert_resource(SkipReplays(skip));
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);
    pin_classic_contact_windows(&mut app);
    app.world_mut().resource_mut::<Settings>().replays = mode;
    app
}

fn playing(app: &App) -> bool {
    app.world().resource::<Replay>().is_playing()
}

fn virtual_secs(app: &App) -> f64 {
    app.world().resource::<Time<Virtual>>().elapsed_secs_f64()
}

fn rigs(app: &mut App) -> Vec<(Entity, Transform, GlobalTransform)> {
    let mut q = app
        .world_mut()
        .query_filtered::<(Entity, &Transform, &GlobalTransform), Or<(
            With<RigUnitTag>,
            With<RigLimb>,
            With<AnimationTargetId>,
        )>>();
    let mut v: Vec<_> = q.iter(app.world()).map(|(e, t, g)| (e, *t, *g)).collect();
    v.sort_by_key(|(e, _, _)| *e);
    v
}

/// The replay tag's line, as painted this frame.
fn tag_line(app: &mut App, line: ReplayTagLine) -> String {
    let mut q = app.world_mut().query::<(&ReplayTagText, &Text)>();
    q.iter(app.world())
        .find(|(t, _)| t.0 == line)
        .map(|(_, text)| text.0.clone())
        .expect("the replay tag spawns with the game")
}

fn to_home_run_call(app: &mut App) {
    let called = run_until(app, MAX_FRAMES, |app| {
        let play = app.world().resource::<Play>();
        play.phase == Phase::Result && play.is_home_run()
    });
    assert!(called.is_some(), "the scripted home run never came");
}

#[test]
fn a_home_run_replays_with_play_frozen_and_untouched() {
    let mut app = app_with(ReplayMode::BigPlays, false);
    to_home_run_call(&mut app);
    let started = run_until(&mut app, (2.0 / DT) as u64, |app| playing(app));
    assert!(started.is_some(), "no replay after the home run call");

    let clock = virtual_secs(&app);
    let live = rigs(&mut app);
    // Live, the batter is hidden once his run-out stand-in takes over; the
    // replay opens before contact, so it shows him in the box.
    let batter_drawn = |app: &mut App| {
        let mut q = app
            .world_mut()
            .query_filtered::<(&Visibility, &InheritedVisibility), With<Batter>>();
        q.iter(app.world())
            .map(|(v, inherited)| (*v, inherited.get()))
            .collect::<Vec<_>>()
    };
    let at_start = batter_drawn(&mut app);
    assert!(
        at_start
            .iter()
            .any(|(live, drawn)| *live == Visibility::Hidden && *drawn),
        "the replay must draw the batter the live play has hidden: {at_start:?}"
    );
    let roots: Vec<Entity> = {
        let mut q = app
            .world_mut()
            .query_filtered::<Entity, (With<RigUnitTag>, Without<ChildOf>)>();
        q.iter(app.world()).collect()
    };
    assert!(!roots.is_empty());
    let mut rendered_differs = false;
    let mut cameras = Vec::new();
    let mut frames = 0u64;
    let (mut tagged, mut slow, mut prompted) = (false, false, false);
    while playing(&app) {
        app.update();
        frames += 1;
        assert!(frames < (30.0 / DT) as u64, "the replay never ended");
        if !playing(&app) {
            break;
        }
        assert_eq!(
            virtual_secs(&app),
            clock,
            "the gameplay clock ran during the replay"
        );
        assert_eq!(app.world().resource::<Play>().phase, Phase::Result);
        // Live transforms are put back every frame; what was rendered (the
        // propagated global transform) is the recording's.
        let now = rigs(&mut app);
        assert_eq!(
            now.len(),
            live.len(),
            "rigs spawned or despawned during the replay"
        );
        for ((e, t, g), (_, t0, _)) in now.iter().zip(&live) {
            // (Locomotion re-derives a mover's facing each frame, even with
            // the clock stopped: equal up to float noise.)
            assert!(
                t.translation == t0.translation && t.rotation.dot(t0.rotation).abs() > 1.0 - 1e-6,
                "rig {e} moved for real during the replay: {t:?} vs {t0:?}"
            );
            // Roots are top-level, so their world pose is their local one:
            // drawn somewhere other than where they live stand.
            if roots.contains(e) && g.translation().distance(t0.translation) > 0.5 {
                rendered_differs = true;
            }
        }
        tagged |= tag_line(&mut app, ReplayTagLine::Title) == "REPLAY";
        slow |= tag_line(&mut app, ReplayTagLine::Speed) == "SLOW MOTION";
        prompted |= tag_line(&mut app, ReplayTagLine::Skip) == SKIP_PROMPT;
        let mut cam = app
            .world_mut()
            .query_filtered::<&Transform, With<Camera3d>>();
        if let Ok(tf) = cam.single(app.world()) {
            cameras.push(*tf);
        }
    }
    assert!(
        rendered_differs,
        "the replay never showed a rig anywhere but where it stands"
    );
    assert!(
        cameras.windows(2).any(|w| w[0] != w[1]),
        "the replay camera never moved"
    );
    let secs = frames as f64 * DT;
    assert!(secs > 2.0, "replay only {secs:.2} s");
    assert!(tagged, "the REPLAY tag never showed");
    assert!(prompted, "the skip prompt never showed");
    // Slow motion is High's second angle alone.
    let high = app
        .world()
        .get_resource::<ActiveQuality>()
        .is_some_and(|q| q.tier == QualityTier::High);
    assert_eq!(slow, high, "slow motion shown on High only (High: {high})");

    // Play resumes and goes on to the next pitch.
    let resumed = run_until(&mut app, 10, |app| virtual_secs(app) > clock);
    assert!(resumed.is_some(), "the clock never resumed");
    let next = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<Play>().phase == Phase::PrePitch
    });
    assert!(next.is_some(), "play never went on to the next pitch");
    assert_eq!(
        tag_line(&mut app, ReplayTagLine::Title),
        "",
        "the tag stayed up after the replay"
    );
    assert_eq!(app.world().resource::<Replay>().played, 1);
}

#[test]
fn a_confirm_press_skips_the_replay() {
    let mut app = app_with(ReplayMode::BigPlays, true);
    to_home_run_call(&mut app);
    assert!(run_until(&mut app, (2.0 / DT) as u64, |app| playing(app)).is_some());
    // The grace holds: a press during it doesn't skip.
    let early = run_until(&mut app, ((SKIP_GRACE as f64 - 0.1) / DT) as u64, |app| {
        !playing(app)
    });
    assert!(
        early.is_none(),
        "a press skipped the replay inside the grace"
    );
    let ended = run_until(&mut app, (0.3 / DT) as u64, |app| !playing(app));
    assert!(ended.is_some(), "a press never skipped the replay");
}

#[test]
fn replays_off_means_none() {
    let mut app = app_with(ReplayMode::Off, false);
    to_home_run_call(&mut app);
    let none = run_until(&mut app, (3.0 / DT) as u64, |app| playing(app));
    assert!(none.is_none(), "a replay played with replays off");
}

/// The task's done-when: a whole CPU-vs-CPU game with a replay after every
/// ball in play (`Always`, the heaviest load) finishes without a stall, and
/// leaves the gameplay clock running.
#[test]
fn a_cpu_game_with_every_replay_finishes() {
    let mut app = headless_app();
    tap_key(&mut app, KeyCode::KeyI); // 9 innings -> 1
    start_game(&mut app, KeyCode::Digit1);
    *app.world_mut().resource_mut::<Controllers>() = Controllers {
        home: InputSource::Cpu,
        away: InputSource::Cpu,
        ..Controllers::default()
    };
    app.world_mut().resource_mut::<Settings>().replays = ReplayMode::Always;
    let finished = run_until(&mut app, 600_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::GameOver
    });
    assert!(finished.is_some(), "the game never finished");
    let replay = app.world().resource::<Replay>();
    assert!(
        replay.played >= 2,
        "only {} replays in a game",
        replay.played
    );
    assert!(!replay.is_playing());
    assert!(!app.world().resource::<Time<Virtual>>().is_paused());
}

/// The orbit camera never sets its own lens and eases from wherever the
/// camera is, so a replay must hand the lens back as it found it.
#[test]
fn the_orbit_camera_gets_its_lens_back() {
    let mut app = app_with(ReplayMode::BigPlays, false);
    *app.world_mut().resource_mut::<CameraMode>() = CameraMode::Orbit;
    to_home_run_call(&mut app);
    let lens = |app: &mut App| {
        let mut q = app
            .world_mut()
            .query_filtered::<(&Transform, &Projection), With<Camera3d>>();
        let (tf, p) = q.single(app.world()).expect("one camera");
        let fov = match p {
            Projection::Perspective(p) => p.fov,
            _ => f32::NAN,
        };
        (tf.translation, fov)
    };
    // The frame before the replay starts.
    let mut before = lens(&mut app);
    let started = run_until(&mut app, (2.0 / DT) as u64, |app| {
        if playing(app) {
            return true;
        }
        before = lens(app);
        false
    });
    assert!(started.is_some(), "no replay");
    let during = lens(&mut app);
    assert_ne!(during.1, before.1, "the replay set its own lens");
    let ended = run_until(&mut app, (30.0 / DT) as u64, |app| !playing(app));
    assert!(ended.is_some(), "the replay never ended");
    let after = lens(&mut app);
    assert_eq!(after.1, before.1, "the lens came back");
    assert!(
        after.0.distance(before.0) < 1.0,
        "the orbit camera carries on from where it was: {before:?} vs {after:?}"
    );
}
