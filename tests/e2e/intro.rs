//! End-to-end: the game intro and the half-inning cutaway (graphics plan
//! task 15). PLAY BALL flies the camera over the park with the gameplay
//! clock held and the lineup card up for its second half; it ends on its
//! own, on a press, or at once under a Director (scripts, autoplay). A CPU
//! game's later halves open on the scoreboard cutaway inside the walk-up
//! hold, so it adds no game time.

use crate::common;

use bevy::prelude::*;

use breakneck_baseball::game::GameState;
use breakneck_baseball::game::ScoreBoard;
use breakneck_baseball::game::camera::{BroadcastRig, MainCamera, Shot};
use breakneck_baseball::game::director::{Director, Policy};
use breakneck_baseball::game::flow::Play;
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::intro::{INTRO_SECS, Intro, IntroDisabled, LINEUP_FROM, SKIP_GRACE};
use breakneck_baseball::game::ui::{LineupLine, LineupText};

use common::{DT, DriveGame, headless_app, run_until, start_game};

/// Frames for `secs` of the harness's fixed step.
fn frames(secs: f32) -> u64 {
    (f64::from(secs) / DT).ceil() as u64
}

/// A fresh app with the intro on, in the game.
fn game_with_intro() -> App {
    let mut app = headless_app();
    app.world_mut().remove_resource::<IntroDisabled>();
    start_game(&mut app, KeyCode::Digit1);
    app
}

fn intro(app: &App) -> &Intro {
    app.world().resource::<Intro>()
}

fn lineup_text(app: &mut App) -> Vec<(LineupLine, String)> {
    let world = app.world_mut();
    world
        .query::<(&LineupText, &Text)>()
        .iter(world)
        .map(|(l, t)| (l.0, t.0.clone()))
        .collect()
}

fn camera_eye(app: &mut App) -> Vec3 {
    let world = app.world_mut();
    world
        .query_filtered::<&Transform, With<MainCamera>>()
        .single(world)
        .expect("a main camera")
        .translation
}

#[test]
fn the_intro_flies_over_the_park_with_the_clock_held() {
    let mut app = game_with_intro();
    assert!(intro(&app).is_playing(), "PLAY BALL starts the intro");
    let clock = app.world().resource::<Time<Virtual>>().elapsed_secs();
    let walkup = app.world().resource::<Play>().walkup_active();

    app.update();
    let first = camera_eye(&mut app);
    for _ in 0..frames(1.0) {
        app.update();
    }
    let later = camera_eye(&mut app);
    assert!(
        first.distance(later) > 5.0,
        "the camera flies: {first} -> {later}"
    );
    assert!(first.y > 20.0, "it opens high over the park: {first}");
    assert_eq!(
        app.world().resource::<Time<Virtual>>().elapsed_secs(),
        clock,
        "the gameplay clock holds"
    );
    assert!(lineup_text(&mut app).iter().all(|(_, t)| t.is_empty()));

    for _ in 0..frames(LINEUP_FROM) {
        app.update();
    }
    let card = lineup_text(&mut app);
    assert!(
        card.iter()
            .any(|(l, t)| *l == LineupLine::Matchup && t.contains("AWAY AT HOME")),
        "{card:?}"
    );
    let batters = card
        .iter()
        .filter(|(l, t)| matches!(l, LineupLine::Batter(..)) && t.contains('#'))
        .count();
    assert_eq!(batters, 18, "both nines on the card: {card:?}");

    let ended = run_until(&mut app, frames(INTRO_SECS), |app| !intro(app).is_playing());
    assert!(ended.is_some(), "the intro ends on its own");
    assert_eq!(intro(&app).skipped, 0);
    for _ in 0..10 {
        app.update();
    }
    assert!(
        app.world().resource::<Time<Virtual>>().elapsed_secs() > clock,
        "the clock runs again"
    );
    assert!(lineup_text(&mut app).iter().all(|(_, t)| t.is_empty()));
    assert_eq!(
        app.world().resource::<Play>().walkup_active(),
        walkup,
        "the game picks up where it stood"
    );
}

/// Presses `action` from the DriveGame seam once the grace is past.
fn press_after_grace(intro: Res<Intro>, mut intents: ResMut<Intents>) {
    intents.home = default();
    intents.away = default();
    if intro.shown().is_some_and(|t| t > SKIP_GRACE) {
        intents.home.action = true;
    }
}

#[test]
fn a_press_skips_the_intro() {
    let mut app = game_with_intro();
    app.add_systems(DriveGame, press_after_grace);
    let ended = run_until(&mut app, frames(SKIP_GRACE + 0.5), |app| {
        !intro(app).is_playing()
    });
    assert!(ended.is_some(), "a press past the grace skips it");
    assert_eq!(intro(&app).skipped, 1);
}

#[test]
fn a_director_skips_the_intro() {
    let mut app = game_with_intro();
    assert!(intro(&app).is_playing());
    app.insert_resource(Director {
        home: Policy::Cpu,
        away: Policy::Cpu,
    });
    let ended = run_until(&mut app, 3, |app| !intro(app).is_playing());
    assert!(ended.is_some(), "a Director never waits on the intro");
    assert_eq!(intro(&app).skipped, 1);
    assert_eq!(
        *app.world().resource::<State<GameState>>().get(),
        GameState::Playing
    );
}

/// A CPU game into the top of the second: the bottom of the first and the
/// top of the second each open on the cutaway, the top of the first does
/// not (the intro showed the park), and the cutaway never outlasts the
/// walk-up hold it lives in.
#[test]
fn each_later_half_opens_on_the_cutaway() {
    let mut app = headless_app();
    start_game(&mut app, KeyCode::Digit1);
    app.insert_resource(Director {
        home: Policy::Cpu,
        away: Policy::Cpu,
    });
    let mut cut_halves: Vec<(u32, bool)> = Vec::new();
    let mut outside_walkup = false;
    let done = run_until(&mut app, 400_000, |app| {
        let shot = app.world().resource::<BroadcastRig>().shot();
        let score = app.world().resource::<ScoreBoard>();
        let half = (score.inning, score.top_of_inning);
        if shot == Some(Shot::HalfInning) {
            if !app.world().resource::<Play>().walkup_active() {
                outside_walkup = true;
            }
            if cut_halves.last() != Some(&half) {
                cut_halves.push(half);
            }
        }
        // Past the top of the second's first walk-up.
        half == (2, true) && cut_halves.contains(&(2, true)) || score.inning >= 3
    });
    assert!(done.is_some(), "the game never reached the second");
    assert!(!outside_walkup, "the cutaway outlasted the walk-up hold");
    assert_eq!(
        cut_halves,
        [(1, false), (2, true)],
        "one cutaway per later half"
    );
}

/// A long frame (the scene spawning on PLAY BALL, a slow phone) advances
/// the intro by one capped step, not by the whole hitch.
#[test]
fn a_hitch_does_not_eat_the_intro() {
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    let mut app = game_with_intro();
    app.update();
    let before = intro(&app).shown().expect("playing");
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs(3)));
    app.update();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        DT,
    )));
    let after = intro(&app)
        .shown()
        .expect("still playing after a 3 s frame");
    assert!(after - before <= 0.1 + 1e-4, "{before} -> {after}");
}
