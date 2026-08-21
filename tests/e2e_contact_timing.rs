//! End-to-end coverage for swing-timing → contact quality (Task B2).
//!
//! Three scripted presses off identical straightaway changeups prove the
//! `ContactEvent` spine and the Classic quality→physics mapping:
//!   * a dead-on press (ball on the plate) grades `ContactQuality::Perfect`
//!     and leaves the bat *faster* than
//!   * a mistimed (early) press that grades `ContactQuality::Solid`, while
//!   * a wildly early press (ball still way out front) grades
//!     `ContactQuality::Whiff` and puts no ball in play.
//!
//! Only the *press timing* is scripted; the outcomes fall out of the pure
//! rules (`contact_quality` + the exit multipliers) exactly as the unit tests
//! dictate.

mod common;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{BallInPlayEvent, ContactEvent, Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::ContactQuality;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DriveGame, headless_app, run_until, start_game};

/// Generous per-stage budget (steal windows, `Ruleset::steal_window_secs`,
/// gate the pitch once a runner reaches base).
const STAGE_FRAMES: u64 = 15_000;

#[derive(Resource, Default)]
struct Stage(usize);

/// Per-stage capture: the graded quality of the swing, the peak post-contact
/// ball speed, and whether a ball actually went into play. `live_slot` is the
/// stage that owns the ball currently in flight — pinned at the contact so a
/// still-live ball keeps filling its own slot even after the outer loop has
/// advanced the stage counter.
#[derive(Resource)]
struct Captured {
    quality: [Option<ContactQuality>; 3],
    speed: [f32; 3],
    in_play: [bool; 3],
    live_slot: usize,
}

impl Default for Captured {
    fn default() -> Self {
        Self {
            quality: [None; 3],
            speed: [0.0; 3],
            in_play: [false; 3],
            live_slot: usize::MAX,
        }
    }
}

/// Pitches a straightaway changeup every PrePitch (neutral aim → changeup),
/// and swings the batting side on a stage-specific ball-`z` window so each
/// press lands in a known timing band.
fn drive(
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

    // Ball-z window the batter presses inside, per stage.
    let window = match stage.0 {
        0 => Some((-0.3_f32, 0.3_f32)), // ball on the plate → Perfect
        1 => Some((1.3, 2.2)),          // ball still out front → Solid (early)
        2 => Some((4.0, 6.0)),          // way out front, unreachable → Whiff
        _ => None,
    };

    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(fielding).action = true;
        }
        Phase::Pitch => {
            if let (Some((zmin, zmax)), Ok(t)) = (window, ball.get_single()) {
                let z = t.translation.z;
                if z >= zmin && z <= zmax {
                    intents.get_mut(batting).action = true;
                }
            }
        }
        _ => {}
    }
}

/// Records the first `ContactEvent` and the peak in-play ball speed for the
/// current stage.
fn capture(
    stage: Res<Stage>,
    play: Option<Res<Play>>,
    mut contact_ev: EventReader<ContactEvent>,
    mut in_play_ev: EventReader<BallInPlayEvent>,
    ball: Query<&Velocity, With<Baseball>>,
    mut cap: ResMut<Captured>,
) {
    let s = stage.0.min(2);
    for ev in contact_ev.read() {
        // Pin the owning slot at the contact instant, so a ball that stays
        // live after the outer loop advances the stage still fills its slot.
        cap.live_slot = s;
        if cap.quality[s].is_none() {
            cap.quality[s] = Some(ev.quality);
        }
    }
    for _ in in_play_ev.read() {
        cap.in_play[s] = true;
    }
    if matches!(play.as_deref().map(|p| p.phase), Some(Phase::InPlay)) {
        let slot = cap.live_slot;
        if slot < 3 {
            if let Ok(v) = ball.get_single() {
                cap.speed[slot] = cap.speed[slot].max(v.linvel.length());
            }
        }
    }
}

fn advance(app: &mut App, stage: usize, what: &str, milestone: impl FnMut(&mut App) -> bool) {
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

fn quality(app: &App, stage: usize) -> Option<ContactQuality> {
    app.world().resource::<Captured>().quality[stage]
}

#[test]
fn perfect_swing_beats_solid_and_early_whiffs() {
    let mut app = headless_app();
    app.init_resource::<Stage>();
    app.init_resource::<Captured>();
    app.add_systems(DriveGame, (drive, capture));
    start_game(&mut app, KeyCode::Digit2);

    // Stage 0: dead-on press → Perfect, ball leaves the bat.
    advance(&mut app, 0, "perfect contact", |app| {
        quality(app, 0) == Some(ContactQuality::Perfect)
            && app.world().resource::<Captured>().speed[0] > 0.0
    });

    // Stage 1: early press → Solid, ball leaves the bat.
    advance(&mut app, 1, "solid contact", |app| {
        quality(app, 1) == Some(ContactQuality::Solid)
            && app.world().resource::<Captured>().speed[1] > 0.0
    });

    // Stage 2: wildly early press → Whiff, no ball in play.
    advance(&mut app, 2, "early whiff", |app| {
        quality(app, 2) == Some(ContactQuality::Whiff)
    });

    let cap = app.world().resource::<Captured>();
    assert!(
        cap.speed[0] > cap.speed[1],
        "a dead-on (Perfect) swing must exit faster than a mistimed (Solid) one \
         (perfect={:.2} m/s, solid={:.2} m/s)",
        cap.speed[0],
        cap.speed[1],
    );
    assert!(!cap.in_play[2], "a whiff must not put a ball in play",);
}
