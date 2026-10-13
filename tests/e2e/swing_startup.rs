//! End-to-end coverage for the swing's startup: a press *starts* the swing,
//! and the bat meets (or misses) the ball only when it actually comes
//! through the zone — the batting experience the reference footage pins in
//! docs/agent/SMB3-REFERENCE-NOTES.md §2.3/§2.5 (the swing is seen, then the
//! ball is followed or the call is made), never the press-frame judgement it
//! replaced.
//!
//!   * A press timed so the bat arrives with the ball on the plate grades
//!     `Perfect`, and the ball leaves the bat a swing's startup after the
//!     press, not on the press frame.
//!   * A press whose bat arrives far too early grades a `Whiff`, and the
//!     strike is called only once the ball has crossed the plate into the
//!     mitt — the miss is *seen* before it is announced.

use crate::common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::flow::{ContactEvent, Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::rules::ContactQuality;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DT, DriveGame, headless_app, run_until, start_game};

const MAX_FRAMES: u64 = 15_000;

/// The ball-`z` window the batting side presses inside (set per test).
#[derive(Resource)]
struct PressWindow(f32, f32);

/// What the drive saw: the frame of the press, the frame and grade of the
/// judged swing, and the lowest ball `z` observed while the pitch was still
/// logically in flight.
#[derive(Resource)]
struct Seen {
    frame: u64,
    press_frame: Option<u64>,
    contact_frame: Option<u64>,
    quality: Option<ContactQuality>,
    min_pitch_z: f32,
}

impl Default for Seen {
    fn default() -> Self {
        Self {
            frame: 0,
            press_frame: None,
            contact_frame: None,
            quality: None,
            min_pitch_z: f32::INFINITY,
        }
    }
}

/// Pitches a straightaway changeup every PrePitch and presses the batting
/// side's action once, inside the window.
#[allow(clippy::too_many_arguments)]
fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
    window: Res<PressWindow>,
    mut seen: ResMut<Seen>,
    mut intents: ResMut<Intents>,
    ball: Query<&Transform, With<Baseball>>,
    mut contacts: MessageReader<ContactEvent>,
) {
    seen.frame += 1;
    if *state.get() != GameState::Playing {
        return;
    }
    let (Some(play), Some(score)) = (play, score) else {
        return;
    };
    for ev in contacts.read() {
        if seen.contact_frame.is_none() {
            seen.contact_frame = Some(seen.frame);
            seen.quality = Some(ev.quality);
        }
    }
    intents.home = default();
    intents.away = default();
    let fielding = score.fielding_team();
    let batting = score.batting_team();

    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(fielding).action = true;
        }
        Phase::Pitch => {
            let Ok(t) = ball.single() else { return };
            let z = t.translation.z;
            seen.min_pitch_z = seen.min_pitch_z.min(z);
            if seen.press_frame.is_none() && (window.0..=window.1).contains(&z) {
                intents.get_mut(batting).action = true;
                seen.press_frame = Some(seen.frame);
            }
        }
        _ => {}
    }
}

fn app_with(window: PressWindow) -> App {
    let mut app = headless_app();
    app.insert_resource(window)
        .init_resource::<Seen>()
        .add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);
    app
}

/// A changeup crosses the plate at ~28 m/s, so a bat that needs ~0.15 s to
/// come through meets a ball that was ~4.2 m out at the press. A press in
/// this window therefore arrives dead-on — `Perfect` — and only *after* the
/// swing's startup has played.
#[test]
fn the_ball_leaves_the_bat_when_the_bat_arrives_not_at_the_press() {
    let mut app = app_with(PressWindow(3.9, 4.5));

    let judged = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<Seen>().contact_frame.is_some()
    });
    assert!(judged.is_some(), "the press never produced a judged swing");

    let seen = app.world().resource::<Seen>();
    assert_eq!(
        seen.quality,
        Some(ContactQuality::Perfect),
        "a press timed for the bat's arrival on the plate should grade Perfect"
    );
    let press = seen.press_frame.expect("the drive pressed");
    let contact = seen.contact_frame.expect("a swing was judged");
    let startup_secs = (contact - press) as f64 * DT;
    assert!(
        (0.10..=0.25).contains(&startup_secs),
        "the ball should leave the bat a swing's startup after the press, saw {startup_secs:.3} s"
    );
}

/// Pressing while the ball is ten metres out puts the bat through the zone
/// with the ball still ~5 m short of the plate: a `Whiff`. The strike must
/// not be called on the press frame — the ball flies on through the zone
/// and into the mitt first, and only then does the count tick.
#[test]
fn a_whiff_is_called_only_after_the_ball_has_crossed_the_plate() {
    let mut app = app_with(PressWindow(9.0, 10.0));

    let judged = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<Play>().phase == Phase::Result
    });
    assert!(
        judged.is_some(),
        "the early swing never reached a result pause"
    );
    // The miss's report lands on the call's frame; the drive reads it next.
    app.update();

    let seen = app.world().resource::<Seen>();
    assert_eq!(seen.quality, Some(ContactQuality::Whiff));
    assert!(
        seen.min_pitch_z < 0.0,
        "the strike was called with the ball still {:.2} m in front of the plate — \
         the miss must be seen before it is announced",
        seen.min_pitch_z
    );
    assert_eq!(
        app.world().resource::<ScoreBoard>().strikes,
        1,
        "the swing-through is a strike once called"
    );
}
