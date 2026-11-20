//! End-to-end: the runner cam (plan task 8). On an extra-base hit the
//! camera follows the ball, then — once the outfielder's throw is away —
//! trails the runner racing it, and hands over to the base cam for the
//! call. A scripted sweep of drives into the gaps must produce a double and
//! a triple whose shot logs both read ball-follow → runner cam → base cam.

use crate::common;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::camera::{BroadcastRig, Shot};
use breakneck_baseball::game::flow::{Phase, Play, bat_arrival_z};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::Outcome;
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DriveGame, deterministic_headless_app, start_game};

/// Spray aims the sweep cycles through, one per pitch: both gaps and both
/// lines, with enough uppercut for a drive.
const AIMS: [Vec2; 6] = [
    Vec2::new(-0.55, 0.35),
    Vec2::new(0.55, 0.35),
    Vec2::new(-0.9, 0.3),
    Vec2::new(0.9, 0.3),
    Vec2::new(-0.35, 0.45),
    Vec2::new(0.35, 0.45),
];

/// Which aim this pitch uses; advanced at each PrePitch.
#[derive(Resource, Default)]
struct Sweep {
    pitch: usize,
    in_pre_pitch: bool,
}

fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut sweep: ResMut<Sweep>,
    mut intents: ResMut<Intents>,
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
    let batting = score.batting_team();
    match play.phase {
        Phase::PrePitch => {
            if !sweep.in_pre_pitch {
                sweep.in_pre_pitch = true;
                sweep.pitch += 1;
            }
            intents.get_mut(score.fielding_team()).action = true;
        }
        Phase::Pitch => {
            sweep.in_pre_pitch = false;
            intents.get_mut(batting).aim = AIMS[sweep.pitch % AIMS.len()];
            if let Ok((t, v)) = ball.single()
                && (-0.3..=0.3).contains(&bat_arrival_z(t.translation.z, v.linvel.z))
            {
                intents.get_mut(batting).action = true;
            }
        }
        // Send the batter for the extra base on every hit.
        Phase::InPlay => intents.get_mut(batting).aim = Vec2::new(0.0, -1.0),
        _ => {}
    }
}

fn shot(app: &App) -> Option<Shot> {
    app.world().resource::<BroadcastRig>().shot()
}

fn phase(app: &App) -> Phase {
    app.world().resource::<Play>().phase
}

/// The bases a play's batter was credited with, for the outcomes that are
/// hits.
fn hit_bases(outcome: Option<Outcome>) -> Option<u32> {
    match outcome? {
        Outcome::Hit(n) => Some(n),
        Outcome::ExtraBase { hit, .. } => Some(hit),
        _ => None,
    }
}

/// Whether `log` holds ball-follow, then a runner cam, then a base cam, in
/// that order (other shots may sit between).
fn follows_runner_then_bag(log: &[Shot]) -> bool {
    let Some(follow) = log.iter().position(|s| *s == Shot::BallFollow) else {
        return false;
    };
    let Some(runner) = log[follow..]
        .iter()
        .position(|s| matches!(s, Shot::RunnerCam(_)))
        .map(|i| i + follow)
    else {
        return false;
    };
    log[runner..].iter().any(|s| matches!(s, Shot::BaseCam(_)))
}

#[test]
fn extra_base_hits_cut_ball_runner_then_bag() {
    let mut app = deterministic_headless_app();
    app.init_resource::<Sweep>();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);
    // Even exit speeds (the shipped perfect-contact boost puts these
    // drives over the fence): gappers that roll toward the wall.
    {
        let mut r = app.world_mut().resource_mut::<Ruleset>();
        r.batting.exit_perfect = 1.0;
        r.batting.exit_solid = 1.0;
    }

    let mut seen: Vec<(Option<Outcome>, Vec<Shot>)> = Vec::new();
    let (mut double, mut triple) = (false, false);
    let mut frames = 0u64;
    while !(double && triple) && frames < 400_000 && seen.len() < 80 {
        // To the next ball in play.
        while phase(&app) != Phase::InPlay && frames < 400_000 {
            app.update();
            frames += 1;
        }
        let mut log = Vec::new();
        while phase(&app) == Phase::InPlay && frames < 400_000 {
            if let Some(s) = shot(&app)
                && log.last() != Some(&s)
            {
                log.push(s);
            }
            app.update();
            frames += 1;
        }
        if let Some(s) = shot(&app)
            && log.last() != Some(&s)
        {
            log.push(s);
        }
        let outcome = app.world().resource::<Play>().last_outcome();
        let good = follows_runner_then_bag(&log);
        // On every play: the runner cam never comes back once the base cam
        // is up, and when it showed, the call lands on a base cam.
        if let Some(bag) = log.iter().position(|s| matches!(s, Shot::BaseCam(_))) {
            assert!(
                !log[bag..].iter().any(|s| matches!(s, Shot::RunnerCam(_))),
                "runner cam after the base cam: {outcome:?} {log:?}"
            );
        }
        if log.iter().any(|s| matches!(s, Shot::RunnerCam(_))) {
            assert!(
                matches!(log.last(), Some(Shot::BaseCam(_))),
                "the call must land on the base cam: {outcome:?} {log:?}"
            );
        }
        match hit_bases(outcome) {
            Some(2) if good => double = true,
            Some(3) if good => triple = true,
            _ => {}
        }
        seen.push((outcome, log));
    }
    let report = seen
        .iter()
        .map(|(o, l)| format!("{o:?}: {l:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        double && triple,
        "need a double and a triple that cut ball-follow → runner cam → base cam \
         (double {double}, triple {triple}); plays:\n{report}"
    );
}
