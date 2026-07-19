//! End-to-end scenarios for the advanced rules, driven through the real app:
//! hit-by-pitch, stolen bases, caught stealing, double plays, hit-and-run,
//! and the dropped third strike.
//!
//! Each scenario is a staged script: the outer loop watches `Bases` /
//! `ScoreBoard` for the current milestone and bumps the `Stage` resource; the
//! in-app driver reads the stage and writes `Intents` (the CPU AI's seam).
//! Everything is deterministic — outcomes follow from pitch kind, aim, and
//! swing timing exactly as the unit-tested rules dictate.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::Bases;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{headless_app, run_until, DriveGame};

/// Generous per-milestone budget (~40 sim-seconds).
const STAGE_FRAMES: u64 = 10_000;

#[derive(Resource, Default)]
struct Stage(usize);

#[derive(Resource, Default)]
struct MenuScript {
    frame: u64,
}

/// Presses **2** on the menu to start a two-player game.
fn drive_menu(
    state: Res<State<GameState>>,
    mut script: ResMut<MenuScript>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
) {
    if *state.get() != GameState::MainMenu {
        return;
    }
    script.frame += 1;
    match script.frame {
        10 => keyboard.press(KeyCode::Digit2),
        12 => keyboard.release(KeyCode::Digit2),
        _ => {}
    }
}

fn start_two_player_game(app: &mut App) {
    app.init_resource::<Stage>();
    app.init_resource::<MenuScript>();
    app.add_systems(DriveGame, drive_menu);
    let started = run_until(app, STAGE_FRAMES, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Playing
    });
    assert!(started.is_some(), "menu never started the game");
}

fn bases(app: &mut App) -> &Bases {
    app.world().resource::<Bases>()
}

fn score(app: &mut App) -> &ScoreBoard {
    app.world().resource::<ScoreBoard>()
}

/// Advances the stage counter and runs until `milestone` holds.
fn expect_stage(app: &mut App, stage: usize, what: &str, milestone: impl FnMut(&mut App) -> bool) {
    app.world_mut().resource_mut::<Stage>().0 = stage;
    let reached = run_until(app, STAGE_FRAMES, milestone);
    let s = app.world().resource::<ScoreBoard>();
    assert!(
        reached.is_some(),
        "stage {stage} ({what}) never reached its milestone \
         (inning {} top={} outs={} balls={} strikes={})",
        s.inning,
        s.top_of_inning,
        s.outs,
        s.balls,
        s.strikes
    );
}

// ── Scenario 1: HBP → SB → CS → HBP → DP → HBP → hit-and-run ─────────────────

/// Per-stage intents. The pitching side always initiates from PrePitch; the
/// batting side arms steals in the windup and times its swings off the live
/// ball position.
fn drive_scenario(
    stage: Res<Stage>,
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
    ball: Query<&Transform, With<Baseball>>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    let fielding = score.fielding_team();
    let batting = score.batting_team();

    // (pitch aim, batter arms a steal?, batter swing window on ball z)
    let (pitch_aim, arm_steal, swing) = match stage.0 {
        // Top of the 1st: Home pitches to Away.
        // S0: full-inside changeup plunks the batter.
        0 => (Vec2::new(1.0, 0.0), false, None),
        // S1: centre changeup; runner sent — off-speed → stolen base.
        1 => (Vec2::ZERO, true, None),
        // S2: high fastball; runner sent — caught stealing.
        2 => (Vec2::new(0.0, 0.6), true, None),
        // S3: plunk the next batter to restock first base.
        3 => (Vec2::new(1.0, 0.0), false, None),
        // S4: centre changeup, batter tops a very late weak grounder with a
        // runner on first → inning-ending 6-4-3.
        4 => (Vec2::ZERO, false, Some((-1.2, -0.95, Vec2::ZERO))),
        // Bottom of the 1st: Away pitches to Home.
        // S5: plunk the Home batter.
        5 => (Vec2::new(1.0, 0.0), false, None),
        // S6: hit-and-run — runner goes, batter slaps a low single;
        // the jump sends him first-to-third.
        6 => (Vec2::ZERO, true, Some((0.3, 0.8, Vec2::new(0.0, -1.0)))),
        _ => (Vec2::ZERO, false, None),
    };

    match play.phase {
        Phase::PrePitch => {
            let intent = intents.get_mut(fielding);
            intent.aim = pitch_aim;
            intent.action = true;
        }
        Phase::WindUp if arm_steal => {
            intents.get_mut(batting).aim = Vec2::new(0.0, -1.0);
        }
        Phase::Pitch => {
            if let (Some((z_min, z_max, aim)), Ok(t)) = (swing, ball.get_single()) {
                let intent = intents.get_mut(batting);
                intent.aim = aim;
                let z = t.translation.z;
                if z >= z_min && z <= z_max {
                    intent.action = true;
                }
            }
        }
        _ => {}
    }
}

#[test]
fn hbp_steals_double_play_and_hit_and_run() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive_scenario);
    start_two_player_game(&mut app);

    expect_stage(&mut app, 0, "hit-by-pitch", |app| bases(app).is_occupied(0));
    expect_stage(&mut app, 1, "stolen base", |app| {
        let b = bases(app);
        b.is_occupied(1) && !b.is_occupied(0)
    });
    expect_stage(&mut app, 2, "caught stealing", |app| {
        let no_runners = !(0..3).any(|i| bases(app).is_occupied(i));
        no_runners && score(app).outs == 1
    });
    // The count survived the caught stealing: two pitches were taken (the
    // centre changeup and the high fastball) and neither reset the at-bat.
    let s = score(&mut app);
    assert_eq!(
        s.balls + s.strikes,
        2,
        "the batter's count must carry through a caught stealing \
         (balls={} strikes={})",
        s.balls,
        s.strikes
    );

    expect_stage(&mut app, 3, "second hit-by-pitch", |app| {
        bases(app).is_occupied(0) && score(app).outs == 1
    });
    expect_stage(&mut app, 4, "inning-ending double play", |app| {
        !score(app).top_of_inning
    });
    let s = score(&mut app);
    assert_eq!(s.inning, 1, "the DP ends the half, not the game");
    assert_eq!(s.outs, 0, "outs reset after the flip");
    assert_eq!((s.home_runs, s.away_runs), (0, 0));

    expect_stage(&mut app, 5, "hit-by-pitch on the Home batter", |app| {
        bases(app).is_occupied(0)
    });
    expect_stage(&mut app, 6, "hit-and-run first-to-third", |app| {
        let b = bases(app);
        b.is_occupied(0) && b.is_occupied(2)
    });
    let b = bases(&mut app);
    assert!(
        !b.is_occupied(1),
        "the hit-and-run jump must clear second (runner went first-to-third)"
    );
}

// ── Scenario 2: dropped third strike ─────────────────────────────────────────

/// The pitcher spins curveballs; the batter flails at each one while it is
/// far out of reach (a swinging strike). Strike three on a curve with first
/// base open gets away — the batter reaches.
fn drive_whiffs(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut intents: ResMut<Intents>,
    ball: Query<&Transform, With<Baseball>>,
) {
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    match play.phase {
        Phase::PrePitch => {
            let intent = intents.get_mut(score.fielding_team());
            intent.aim = Vec2::new(0.0, -1.0); // curveball
            intent.action = true;
        }
        Phase::Pitch => {
            if let Ok(t) = ball.get_single() {
                if t.translation.z > 5.0 {
                    intents.get_mut(score.batting_team()).action = true;
                }
            }
        }
        _ => {}
    }
}

#[test]
fn dropped_third_strike_lets_the_batter_reach() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive_whiffs);
    start_two_player_game(&mut app);

    let reached = run_until(&mut app, STAGE_FRAMES, |app| bases(app).is_occupied(0));
    let s = score(&mut app);
    assert!(
        reached.is_some(),
        "the batter never reached on the dropped third \
         (outs={} balls={} strikes={})",
        s.outs,
        s.balls,
        s.strikes
    );
    assert_eq!(s.outs, 0, "a dropped third strike is not an out");
    assert_eq!(
        (s.balls, s.strikes),
        (0, 0),
        "the next batter starts with a fresh count"
    );
}
