//! End-to-end: the running game on its own button. The defense's secondary
//! press throws a pickoff and the offense's calls a steal; both throws are
//! *staged* — the ball flies to the bag and the call (and the bases) change
//! only when it lands — and a pitch press inside the steal window is held
//! for the window's end rather than swallowed.

use crate::common;

use bevy::prelude::*;

use breakneck_baseball::game::flow::{DuelThrowKind, LeadState, Phase, Play, PlayBanner};
use breakneck_baseball::game::fx::DuelThrowBall;
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::Bases;
use breakneck_baseball::game::scenario::{apply_to_world, presets};
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DriveGame, headless_app, run_until, start_game};

const BUDGET: u64 = 6_000;

/// What the scripted thumbs do this frame.
#[derive(Resource, Default, Clone, Copy)]
struct Thumbs {
    /// Defense: pickoff once the runner's lead passes this (metres).
    pickoff_past_lead: Option<f32>,
    /// Defense: press pitch on every PrePitch frame / once.
    pitch: PitchPress,
    /// Offense: press the steal button on every PrePitch frame.
    call_steal: bool,
    /// The pitch aim.
    aim: Vec2,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum PitchPress {
    #[default]
    Never,
    /// One press, on the first PrePitch frame inside the steal window.
    OnceInWindow,
}

#[derive(Resource, Default)]
struct Pressed(bool);

#[derive(Resource, Default)]
struct Banners(Vec<String>);

#[allow(clippy::too_many_arguments)]
fn drive(
    state: Res<State<GameState>>,
    thumbs: Res<Thumbs>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    lead: Option<Res<LeadState>>,
    mut pressed: ResMut<Pressed>,
    mut intents: ResMut<Intents>,
    mut banners: MessageReader<PlayBanner>,
    mut log: ResMut<Banners>,
) {
    log.0.extend(banners.read().map(|b| b.text.clone()));
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score), Some(lead)) = (play, score, lead) else {
        return;
    };
    intents.home = default();
    intents.away = default();
    let fielding = score.fielding_team();
    let batting = score.batting_team();
    if play.phase != Phase::PrePitch || play.walkup_active() {
        return;
    }
    if thumbs.call_steal {
        intents.get_mut(batting).secondary = true;
    }
    let defense = intents.get_mut(fielding);
    defense.aim = thumbs.aim;
    if let Some(past) = thumbs.pickoff_past_lead {
        if lead.lead_m > past && play.duel_throw().is_none() {
            defense.secondary = true;
        }
    }
    if thumbs.pitch == PitchPress::OnceInWindow && play.in_steal_window() && !pressed.0 {
        pressed.0 = true;
        defense.action = true;
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
    app
}

/// The banners raised so far — a couple of frames on, so a call made on the
/// frame a milestone tripped has reached the driver's reader.
fn banners(app: &mut App) -> Vec<String> {
    for _ in 0..3 {
        app.update();
    }
    app.world().resource::<Banners>().0.clone()
}

fn throw_kind(app: &App) -> Option<DuelThrowKind> {
    app.world().resource::<Play>().duel_throw().map(|t| t.kind)
}

fn ghost_balls(app: &mut App) -> usize {
    let world = app.world_mut();
    world.query::<&DuelThrowBall>().iter(world).count()
}

/// A stretched lead draws the throw: the ball is seen flying, the runner is
/// still on first while it does, and only the tag retires him.
#[test]
fn a_pickoff_is_thrown_seen_and_called_at_the_tag() {
    let mut app = setup(Thumbs {
        pickoff_past_lead: Some(4.2),
        call_steal: true,
        ..default()
    });
    let started = run_until(&mut app, BUDGET, |app| {
        throw_kind(app) == Some(DuelThrowKind::Pickoff)
    });
    assert!(started.is_some(), "the defense's button never threw over");
    let thrown_at = app.world().resource::<Time>().elapsed_secs();
    app.update();
    assert_eq!(ghost_balls(&mut app), 1, "the pickoff must be seen flying");
    assert!(
        app.world().resource::<Bases>().is_occupied(0),
        "the call waits for the ball: the runner is still on first mid-throw"
    );
    let outs_before = app.world().resource::<ScoreBoard>().outs;

    let tagged = run_until(&mut app, BUDGET, |app| {
        !app.world().resource::<Bases>().is_occupied(0)
    });
    assert!(tagged.is_some(), "a 4.2 m lead must be picked off");
    let flight = app.world().resource::<Time>().elapsed_secs() - thrown_at;
    assert!(
        (0.7..=1.0).contains(&flight),
        "the call lands with the ball (~0.8 s after the throw), took {flight:.2}s"
    );
    assert_eq!(app.world().resource::<ScoreBoard>().outs, outs_before + 1);
    let log = banners(&mut app);
    assert!(
        log.iter().any(|b| b == "PICKED OFF AT 1ST!"),
        "banners: {log:?}"
    );
}

/// A throw at a normal lead: the runner dives back in, the call says so in
/// words, and the duel picks up again with him still on first.
#[test]
fn a_safe_pickoff_says_where_and_resumes_the_duel() {
    let mut app = setup(Thumbs {
        pickoff_past_lead: Some(0.0),
        ..default()
    });
    let landed = run_until(&mut app, BUDGET, |app| {
        app.world()
            .resource::<Play>()
            .duel_throw()
            .is_some_and(|t| t.landed)
    });
    assert!(landed.is_some(), "the pickoff never landed");
    assert!(app.world().resource::<Bases>().is_occupied(0));
    let log = banners(&mut app);
    assert!(log.iter().any(|b| b == "SAFE AT 1ST"), "banners: {log:?}");
    let resumed = run_until(&mut app, BUDGET, |app| throw_kind(app).is_none());
    assert!(resumed.is_some(), "the duel never resumed after the tag");
    assert_eq!(app.world().resource::<Play>().phase, Phase::PrePitch);
}

/// A pitch pressed while the window holds the ball is thrown when the
/// window closes — one press, no second one needed, no pickoff.
#[test]
fn a_pitch_press_in_the_window_is_held_not_swallowed() {
    let mut app = setup(Thumbs {
        pitch: PitchPress::OnceInWindow,
        ..default()
    });
    let pitched = run_until(&mut app, BUDGET, |app| {
        app.world().resource::<Play>().phase == Phase::WindUp
    });
    assert!(pitched.is_some(), "the queued pitch was never thrown");
    assert!(app.world().resource::<Pressed>().0);
    assert!(
        !app.world().resource::<Play>().in_steal_window(),
        "the queued pitch must still wait out the window"
    );
    assert!(
        throw_kind(&app).is_none(),
        "a pitch press is never a pickoff"
    );
}

/// A called steal: the runner goes on first movement, the catcher's throw
/// is staged after the catch, and the bases only change at the tag.
#[test]
fn a_called_steal_shows_the_catchers_throw_before_the_call() {
    let mut app = setup(Thumbs {
        pitch: PitchPress::OnceInWindow,
        call_steal: true,
        ..default()
    });
    let thrown = run_until(&mut app, BUDGET, |app| {
        throw_kind(app) == Some(DuelThrowKind::Steal)
    });
    assert!(
        thrown.is_some(),
        "the called steal never drew the catcher's throw"
    );
    assert_eq!(app.world().resource::<Play>().phase, Phase::Result);
    assert!(app.world().resource::<LeadState>().called);
    assert!(
        app.world().resource::<Bases>().is_occupied(0),
        "mid-throw the runner hasn't been called yet"
    );
    app.update();
    assert_eq!(ghost_balls(&mut app), 1, "the catcher's throw must be seen");

    let called = run_until(&mut app, BUDGET, |app| {
        !app.world().resource::<Bases>().is_occupied(0)
    });
    assert!(called.is_some(), "the steal was never called");
    let log = banners(&mut app);
    assert!(
        log.iter()
            .any(|b| b == "STOLEN BASE!" || b == "CAUGHT STEALING 2ND"),
        "banners: {log:?}"
    );
}
