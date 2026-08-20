//! End-to-end proof that runner rigs break off contact *before the umpire's
//! call*, per the real-baseball reads encoded in `rules::runner_break` (see
//! docs/BASEBALL.md "Baserunning after contact"). These tests assert only
//! *choreography* — where the rigs are while the ball is live — never an
//! outcome; the outcome model is pinned by the `rules.rs` unit tests and the
//! other e2e suites.
//!
//! The scripts stay within the harness conventions (CLAUDE.md): scripted
//! grounders are topped late and sprayed at a set fielder's spot, and every
//! pitch waits out the pre-pitch steal window when runners are aboard.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::Bases;
use breakneck_baseball::game::runner::Runner;
use breakneck_baseball::game::variant::FieldSpec;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DriveGame, headless_app, run_until, start_game};

const STAGE_FRAMES: u64 = 15_000;

/// A rig that has broken this far (metres, horizontal) off a bag has clearly
/// left it — comfortably past the normal pre-pitch lead (`LEAD_NORMAL` = 2.2 m
/// in `runner.rs`), so a lead alone can never trip the assertion.
const BROKEN_OFF_BAG: f32 = 2.5;

#[derive(Resource, Default)]
struct Stage(usize);

fn start(app: &mut App) {
    app.init_resource::<Stage>();
    start_game(app, KeyCode::Digit2);
}

fn bases(app: &mut App) -> &Bases {
    app.world().resource::<Bases>()
}

fn score(app: &mut App) -> &ScoreBoard {
    app.world().resource::<ScoreBoard>()
}

fn phase(app: &mut App) -> Phase {
    app.world().resource::<Play>().phase
}

/// Horizontal distance of the runner currently occupying (0-indexed) `base`
/// from that bag — the runner's `base` index stays at its origin while it
/// breaks, so this measures how far off the bag the break has carried it.
fn runner_off_bag(app: &mut App, base: usize) -> f32 {
    let bag = {
        let field = app.world().resource::<FieldSpec>();
        let b = field.base_positions[base];
        Vec2::new(b.x, b.z)
    };
    let mut q = app.world_mut().query::<(&Runner, &Transform)>();
    q.iter(app.world())
        .filter(|(r, _)| r.base == base)
        .map(|(_, t)| Vec2::new(t.translation.x, t.translation.z).distance(bag))
        .fold(0.0_f32, f32::max)
}

/// Plunk the batter (full-inside changeup, no swing) — a dead-ball walk to
/// first that leaves the outs untouched.
fn plunk(intents: &mut Intents, play: &Play, fielding: breakneck_baseball::game::Team) {
    if play.phase == Phase::PrePitch {
        let intent = intents.get_mut(fielding);
        intent.aim = Vec2::new(-1.0, 0.0);
        intent.action = true;
    }
}

/// Take centre strikes without swinging — three of them is a called strikeout
/// (an out that keeps the runners aboard).
fn take_strikes(intents: &mut Intents, play: &Play, fielding: breakneck_baseball::game::Team) {
    if play.phase == Phase::PrePitch {
        let intent = intents.get_mut(fielding);
        intent.aim = Vec2::ZERO; // centre changeup: a called strike
        intent.action = true;
    }
}

/// Pitch a centre changeup and top it late, sprayed at the first baseman — a
/// fair grounder with a set fielder waiting on it.
fn top_a_grounder(
    intents: &mut Intents,
    play: &Play,
    ball: &Query<&Transform, With<Baseball>>,
    fielding: breakneck_baseball::game::Team,
    batting: breakneck_baseball::game::Team,
) {
    match play.phase {
        Phase::PrePitch => {
            let intent = intents.get_mut(fielding);
            intent.aim = Vec2::ZERO;
            intent.action = true;
        }
        Phase::Pitch => {
            if let Ok(t) = ball.get_single() {
                let z = t.translation.z;
                if (-0.1..=0.05).contains(&z) {
                    // A low ball up the middle that gets down and stays live
                    // (fielded and thrown, not lined straight at a fielder for
                    // an instant catch) — the same low-single swing the
                    // hit-and-run scenario uses. Timed dead on the plate
                    // (Perfect, zero pull yaw) so the exit multiplier keeps it
                    // a centred single. The play must stay live long enough for
                    // the runner's break to show; the outcome itself is beside
                    // the point here.
                    let intent = intents.get_mut(batting);
                    intent.aim = Vec2::new(0.0, -1.0);
                    intent.action = true;
                }
            }
        }
        _ => {}
    }
}

// ── Scenario 1: 0 outs, runner on first, grounder → the forced runner goes ───

fn drive_forced(
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
    match stage.0 {
        0 => plunk(&mut intents, &play, fielding),
        _ => top_a_grounder(&mut intents, &play, &ball, fielding, batting),
    }
}

#[test]
fn forced_runner_breaks_before_the_call() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive_forced);
    start(&mut app);

    // Stage 0: put a runner on first with nobody out.
    app.world_mut().resource_mut::<Stage>().0 = 0;
    let on = run_until(&mut app, STAGE_FRAMES, |app| bases(app).is_occupied(0));
    assert!(on.is_some(), "never got a runner to first");
    assert_eq!(score(&mut app).outs, 0, "the plunk must not record an out");

    // Stage 1: a grounder with the runner forced. He must break for second
    // while the play is still live and uncalled — bases and outs unchanged.
    app.world_mut().resource_mut::<Stage>().0 = 1;
    let broke = run_until(&mut app, STAGE_FRAMES, |app| {
        phase(app) == Phase::InPlay
            && score(app).outs == 0
            && bases(app).is_occupied(0)
            && runner_off_bag(app, 0) > BROKEN_OFF_BAG
    });
    assert!(
        broke.is_some(),
        "the forced runner never broke off first before the play resolved"
    );
}

// ── Scenario 2: 2 outs, runners on first and second → everyone runs ──────────

fn drive_two_out(
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
    match stage.0 {
        0 | 1 => take_strikes(&mut intents, &play, fielding),
        2 | 3 => plunk(&mut intents, &play, fielding),
        _ => top_a_grounder(&mut intents, &play, &ball, fielding, batting),
    }
}

#[test]
fn two_outs_break_every_runner_on_contact() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive_two_out);
    start(&mut app);

    // Two called strikeouts with the bases empty: two outs, nobody erased.
    app.world_mut().resource_mut::<Stage>().0 = 0;
    let one = run_until(&mut app, STAGE_FRAMES, |app| score(app).outs == 1);
    assert!(one.is_some(), "first strikeout never landed");
    app.world_mut().resource_mut::<Stage>().0 = 1;
    let two = run_until(&mut app, STAGE_FRAMES, |app| score(app).outs == 2);
    assert!(two.is_some(), "second strikeout never landed");

    // Load first and second with two down (plunks keep the out count).
    app.world_mut().resource_mut::<Stage>().0 = 2;
    let first = run_until(&mut app, STAGE_FRAMES, |app| bases(app).is_occupied(0));
    assert!(first.is_some(), "never restocked first base");
    app.world_mut().resource_mut::<Stage>().0 = 3;
    let second = run_until(&mut app, STAGE_FRAMES, |app| {
        let b = bases(app);
        b.is_occupied(0) && b.is_occupied(1)
    });
    assert!(second.is_some(), "never got runners on first and second");
    assert_eq!(score(&mut app).outs, 2, "the setup must leave two outs");

    // Stage 4: a ball in play with two down — every runner goes on contact.
    // Both must be off their bags while the play is still live and uncalled.
    app.world_mut().resource_mut::<Stage>().0 = 4;
    let broke = run_until(&mut app, STAGE_FRAMES, |app| {
        phase(app) == Phase::InPlay
            && score(app).outs == 2
            && bases(app).is_occupied(0)
            && bases(app).is_occupied(1)
            && runner_off_bag(app, 0) > BROKEN_OFF_BAG
            && runner_off_bag(app, 1) > BROKEN_OFF_BAG
    });
    assert!(
        broke.is_some(),
        "both runners must break on contact with two outs, before the call"
    );
}
