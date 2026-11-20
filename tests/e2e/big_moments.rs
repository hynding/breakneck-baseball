//! End-to-end: big-moment sequences (graphics plan task 10). A home run's
//! result pause cuts to the crowd, then plays its replay, then the trot
//! orbit; a walk-off holds the plate pile-up; a dive gets its low angle on
//! the fielder who laid out. A press during a sequence hands straight back
//! to the ordinary shot list, that same frame, with no replay.

use crate::common;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::animation::{AnimClip, Playing};
use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::camera::sequence::{
    BigMoment, CROWD_SECS, DIVE_SECS, PILE_UP_REPLAY_SECS,
};
use breakneck_baseball::game::camera::{BroadcastRig, Shot};
use breakneck_baseball::game::flow::{Phase, Play, bat_arrival_z};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::player::Fielder;
use breakneck_baseball::game::replay::{BigMomentCue, Replay, SKIP_GRACE};
use breakneck_baseball::game::scenario::{self, Scenario};
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard, Team};

use common::{DriveGame, headless_app, run_until, start_game, tap_key};

const MAX_FRAMES: u64 = 200_000;

/// How the drive behaves.
#[derive(Resource, Default)]
struct Script {
    /// Press action once, this far into a big moment's result pause.
    skip_at: Option<f32>,
    /// Spray aim for the home side's swings (straight up the middle with
    /// an uppercut when unset: the scripted home run).
    aim: Option<Vec2>,
    pressed: bool,
}

#[allow(clippy::too_many_arguments)]
fn drive(
    time: Res<Time>,
    state: Res<State<GameState>>,
    mut script: ResMut<Script>,
    replay: Res<Replay>,
    cue: Res<BigMomentCue>,
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
        return;
    }
    let now = time.elapsed_secs();
    match play.phase {
        Phase::PrePitch => {
            script.pressed = false;
            intents.get_mut(score.fielding_team()).action = true;
        }
        Phase::Pitch if score.batting_team() == Team::Home => {
            if let Ok((t, v)) = ball.single() {
                intents.home.aim = script.aim.unwrap_or(Vec2::new(0.0, 1.0));
                let z = bat_arrival_z(t.translation.z, v.linvel.z);
                if (0.0..=0.45).contains(&z) {
                    intents.home.action = true;
                }
            }
        }
        Phase::Result => {
            if let Some(at) = script.skip_at
                && cue.moment.is_some()
                && !script.pressed
                && cue.since_call(now) >= at
            {
                script.pressed = true;
                intents.home.action = true;
            }
        }
        _ => {}
    }
}

fn pin_classic_contact_windows(app: &mut App) {
    let mut r = app.world_mut().resource_mut::<Ruleset>();
    r.batting.perfect_ms = 40.0;
    r.batting.solid_ms = 90.0;
    r.batting.foul_ms = 140.0;
    r.batting.exit_solid = 1.0;
    r.batting.exit_perfect = 1.25;
}

fn app_with(script: Script) -> App {
    let mut app = headless_app();
    app.insert_resource(script);
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);
    pin_classic_contact_windows(&mut app);
    app
}

fn shot(app: &App) -> Option<Shot> {
    app.world().resource::<BroadcastRig>().shot()
}

fn phase(app: &App) -> Phase {
    app.world().resource::<Play>().phase
}

fn playing(app: &App) -> bool {
    app.world().resource::<Replay>().is_playing()
}

/// Runs to the scripted home run's result pause and returns the shots seen
/// through it (deduplicated, replay frames left out) and the result-pause
/// time the replay started at, if it did.
fn home_run_pause(app: &mut App) -> (Vec<Shot>, Option<f32>) {
    let called = run_until(app, MAX_FRAMES, |app| {
        let play = app.world().resource::<Play>();
        play.phase == Phase::Result && play.is_home_run()
    });
    assert!(called.is_some(), "the scripted home run never came");
    let mut log: Vec<Shot> = Vec::new();
    let mut replay_at = None;
    let mut frames = 0;
    while phase(app) == Phase::Result && frames < 60_000 {
        if playing(app) {
            let now = app.world().resource::<Time>().elapsed_secs();
            replay_at.get_or_insert(app.world().resource::<BigMomentCue>().since_call(now));
        } else if let Some(s) = shot(app)
            && log.last() != Some(&s)
        {
            log.push(s);
        }
        app.update();
        frames += 1;
    }
    (log, replay_at)
}

#[test]
fn a_home_run_cuts_to_the_crowd_then_replays_then_orbits() {
    let mut app = app_with(Script::default());
    let (log, replay_at) = home_run_pause(&mut app);
    let crowd = log.iter().position(|s| *s == Shot::CrowdCam);
    let orbit = log.iter().position(|s| *s == Shot::TrotOrbit);
    assert!(
        matches!((crowd, orbit), (Some(c), Some(o)) if c < o),
        "crowd shot, then the trot orbit: {log:?}"
    );
    let at = replay_at.expect("the home run replayed");
    assert!(
        (at - CROWD_SECS).abs() < 0.1,
        "the replay comes after the crowd shot: {at:.2} s"
    );
}

#[test]
fn a_press_skips_the_sequence_and_its_replay() {
    let skip_at = SKIP_GRACE + 0.2;
    let mut app = app_with(Script {
        skip_at: Some(skip_at),
        ..default()
    });
    let called = run_until(&mut app, MAX_FRAMES, |app| {
        let play = app.world().resource::<Play>();
        play.phase == Phase::Result && play.is_home_run()
    });
    assert!(called.is_some(), "the scripted home run never came");
    assert_eq!(shot(&app), Some(Shot::CrowdCam));
    // The frame the press lands on, the shot list takes back over.
    let skipped = run_until(&mut app, 600, |app| {
        app.world().resource::<BigMomentCue>().skipped
    });
    assert!(skipped.is_some(), "the press never skipped");
    assert_eq!(shot(&app), Some(Shot::TrotOrbit), "back on the press frame");
    let replayed = run_until(&mut app, 600, |app| playing(app));
    assert!(replayed.is_none(), "a skipped sequence still replayed");
}

#[test]
fn a_walk_off_holds_the_pile_up() {
    let mut app = headless_app();
    app.insert_resource(Script {
        // To right-centre, short of the wall: the ball falls in.
        aim: Some(Vec2::new(0.5, 0.3)),
        ..default()
    });
    app.add_systems(DriveGame, drive);
    tap_key(&mut app, KeyCode::KeyI); // 9 innings -> 1
    start_game(&mut app, KeyCode::Digit2);
    pin_classic_contact_windows(&mut app);
    {
        let mut r = app.world_mut().resource_mut::<Ruleset>();
        r.batting.exit_perfect = 1.0;
    }
    // Bottom of the last, tied, the winning run on third.
    let stage = Scenario {
        name: "walk-off",
        bases: vec![false, false, true],
        top: false,
        ..default()
    };
    let mut seen = Vec::new();
    let mut frames = 0u64;
    let mut prev = None;
    while frames < MAX_FRAMES {
        // Re-stage at every new plate appearance's first pitch until the
        // run scores (once per PrePitch: re-staging restarts the walk-up).
        let entered = prev != Some(phase(&app));
        prev = Some(phase(&app));
        if entered
            && phase(&app) == Phase::PrePitch
            && !app.world().resource::<Play>().is_walk_off()
            && *app.world().resource::<State<GameState>>().get() == GameState::Playing
        {
            let _ = scenario::apply_to_world(app.world_mut(), &stage);
        }
        app.update();
        frames += 1;
        let play = app.world().resource::<Play>();
        if play.phase == Phase::Result && play.is_walk_off() {
            if play.is_home_run() {
                panic!("the staged walk-off was a home run: re-aim the script");
            }
            break;
        }
    }
    assert!(frames < MAX_FRAMES, "no walk-off in {frames} frames");
    assert_eq!(
        app.world().resource::<BigMomentCue>().moment,
        Some(BigMoment::WalkOff)
    );
    let mut replay_at = None;
    while phase(&app) == Phase::Result
        && *app.world().resource::<State<GameState>>().get() == GameState::Playing
        && seen.len() < 10
    {
        if playing(&app) {
            let now = app.world().resource::<Time>().elapsed_secs();
            replay_at.get_or_insert(app.world().resource::<BigMomentCue>().since_call(now));
        } else if let Some(s) = shot(&app)
            && seen.last() != Some(&s)
        {
            seen.push(s);
        }
        app.update();
    }
    let at = replay_at.expect("the walk-off replayed");
    assert!(
        (at - PILE_UP_REPLAY_SECS).abs() < 0.1,
        "the replay plays inside the pile-up: {at:.2} s"
    );
    assert_eq!(seen.first(), Some(&Shot::PileUp), "{seen:?}");
    assert!(
        seen.iter().all(|s| *s == Shot::PileUp),
        "the pile-up holds the pause: {seen:?}"
    );
}

#[test]
fn a_dive_gets_its_low_angle() {
    let mut app = app_with(Script {
        aim: Some(Vec2::new(-0.5, 0.3)),
        ..default()
    });
    {
        let mut r = app.world_mut().resource_mut::<Ruleset>();
        r.batting.exit_perfect = 1.0;
    }
    // The dive itself is the fielding sim's call; the sequence keys off its
    // clip, so the test plays it on the nearest fielder once a ball is hit.
    let mut dove = None;
    let mut frames = 0u64;
    while frames < MAX_FRAMES {
        app.update();
        frames += 1;
        let play = app.world().resource::<Play>();
        if play.phase == Phase::InPlay && dove.is_none() {
            let now = app.world().resource::<Time>().elapsed_secs();
            if play.since_contact(now) > 0.4 && !play.is_home_run() {
                let ball = app
                    .world_mut()
                    .query_filtered::<&Transform, With<Baseball>>()
                    .single(app.world())
                    .map(|t| t.translation)
                    .unwrap_or_default();
                let mut q = app
                    .world_mut()
                    .query_filtered::<(Entity, &Transform), With<Fielder>>();
                let nearest = q
                    .iter(app.world())
                    .min_by(|a, b| {
                        a.1.translation
                            .distance(ball)
                            .total_cmp(&b.1.translation.distance(ball))
                    })
                    .map(|(e, _)| e);
                if let Some(f) = nearest {
                    app.world_mut()
                        .entity_mut(f)
                        .insert(Playing::new(AnimClip::Dive));
                    dove = Some(f);
                }
            }
        }
        let play = app.world().resource::<Play>();
        if play.phase == Phase::Result && dove.is_some() {
            break;
        }
        if play.phase == Phase::PrePitch {
            dove = None;
        }
    }
    let fielder = dove.expect("a ball in play to dive for");
    let play = app.world().resource::<Play>();
    if play.is_walk_off() || play.is_home_run() {
        panic!("the dive play ended the game or left the park");
    }
    assert_eq!(
        app.world().resource::<BigMomentCue>().moment,
        Some(BigMoment::Dive(fielder))
    );
    assert_eq!(shot(&app), Some(Shot::DiveCam(fielder)));
    // The low angle holds until its replay starts, after it.
    let mut held = true;
    let replayed = run_until(&mut app, 600, |app| {
        if playing(app) {
            return true;
        }
        held &= shot(app) == Some(Shot::DiveCam(fielder));
        false
    });
    assert!(replayed.is_some(), "the dive replayed");
    assert!(held, "the dive cam held until the replay");
    let now = app.world().resource::<Time>().elapsed_secs();
    let at = app.world().resource::<BigMomentCue>().since_call(now);
    assert!(
        (at - DIVE_SECS).abs() < 0.1,
        "the replay comes after the dive shot: {at:.2} s"
    );
}
