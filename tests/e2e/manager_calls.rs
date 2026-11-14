//! End-to-end: the manager's-call button. The defense's press puts the
//! batter on (an intentional walk, no pitches); the offense's squares the
//! batter around, and a hittable pitch is bunted into live play — a soft
//! roller the defense has to field, with the runner moving up on the out.

use crate::common;

use bevy::prelude::*;

use breakneck_baseball::game::flow::{Phase, Play, PlayBanner};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::Bases;
use breakneck_baseball::game::scenario::{apply_to_world, presets};
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DriveGame, headless_app, run_until, start_game};

const BUDGET: u64 = 6_000;

#[derive(Resource, Default, Clone, Copy)]
struct Thumbs {
    /// Defense: press the manager's call (intentional walk).
    walk: bool,
    /// Offense: press the manager's call (square to bunt), once.
    bunt: bool,
}

#[derive(Resource, Default)]
struct Pressed {
    /// The scenario is staged; the thumbs may act.
    armed: bool,
    tactic: bool,
    pitch: bool,
}

#[derive(Resource, Default)]
struct Banners(Vec<String>);

#[allow(clippy::too_many_arguments)]
fn drive(
    state: Res<State<GameState>>,
    thumbs: Res<Thumbs>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    mut pressed: ResMut<Pressed>,
    mut intents: ResMut<Intents>,
    mut banners: MessageReader<PlayBanner>,
    mut log: ResMut<Banners>,
) {
    log.0.extend(banners.read().map(|b| b.text.clone()));
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    if !pressed.armed
        || play.phase != Phase::PrePitch
        || play.walkup_active()
        || play.duel_throw().is_some()
    {
        return;
    }
    let (fielding, batting) = (score.fielding_team(), score.batting_team());
    if !pressed.tactic {
        pressed.tactic = true;
        if thumbs.walk {
            intents.get_mut(fielding).tactic = true;
        }
        if thumbs.bunt {
            intents.get_mut(batting).tactic = true;
        }
        return;
    }
    // Then one pitch down the middle (a changeup, the neutral aim).
    if thumbs.bunt && !pressed.pitch && play.bunting() {
        pressed.pitch = true;
        intents.get_mut(fielding).action = true;
    }
}

fn setup(thumbs: Thumbs) -> App {
    let mut app = headless_app();
    app.insert_resource(thumbs);
    app.init_resource::<Pressed>();
    app.init_resource::<Banners>();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);
    let s = presets()
        .into_iter()
        .find(|s| s.name == "Steal duel: R1")
        .unwrap();
    apply_to_world(app.world_mut(), &s).unwrap();
    app.world_mut().resource_mut::<Pressed>().armed = true;
    app
}

fn banners(app: &mut App) -> Vec<String> {
    for _ in 0..3 {
        app.update();
    }
    app.world().resource::<Banners>().0.clone()
}

#[test]
fn the_defense_puts_the_batter_on_without_a_pitch() {
    let mut app = setup(Thumbs {
        walk: true,
        ..default()
    });
    let walked = run_until(&mut app, BUDGET, |app| {
        app.world().resource::<Bases>().is_occupied(1)
    });
    assert!(walked.is_some(), "the intentional walk never happened");
    let bases = app.world().resource::<Bases>();
    assert!(bases.is_occupied(0), "the batter takes first");
    assert!(bases.is_occupied(1), "the forced runner moves to second");
    let score = app.world().resource::<ScoreBoard>();
    assert_eq!((score.balls, score.strikes, score.outs), (0, 0, 1));
    let log = banners(&mut app);
    assert!(
        log.iter().any(|b| b == "INTENTIONAL WALK"),
        "banners: {log:?}"
    );
}

#[test]
fn a_squared_batter_bunts_a_hittable_pitch_into_play() {
    let mut app = setup(Thumbs {
        bunt: true,
        ..default()
    });
    let in_play = run_until(&mut app, BUDGET, |app| {
        app.world().resource::<Play>().phase == Phase::InPlay
    });
    assert!(in_play.is_some(), "the bunt never went into play");
    let ended = run_until(&mut app, BUDGET, |app| {
        app.world().resource::<Play>().phase == Phase::Result
    });
    assert!(ended.is_some(), "the bunt's play never ended");
    let log = banners(&mut app);
    assert!(
        log.iter().any(|b| b == "SQUARES TO BUNT"),
        "banners: {log:?}"
    );
    // The runner from first moved up whatever the call on the batter.
    assert!(
        app.world().resource::<Bases>().is_occupied(1),
        "the runner should be on second, banners: {log:?}"
    );
}
