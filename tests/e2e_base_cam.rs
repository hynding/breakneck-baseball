//! End-to-end: the cut-based shot list (TODO 102). A scripted grounder must
//! cut from the plate to the ball within the post-contact hold, land on the
//! **base cam** the moment the fielder's throw is away (the shot the call is
//! announced in), keep that shot through the result pause, and only return
//! to the duel framing for the next pitch.

mod common;

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use breakneck_baseball::game::animation::{AnimClip, Playing};
use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::camera::{BroadcastRig, Shot};
use breakneck_baseball::game::flow::{Phase, Play, bat_arrival_z};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::player::Umpire;
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DT, DriveGame, headless_app, run_until, start_game};

const MAX_FRAMES: u64 = 20_000;

/// Pitch a centre changeup and top it late up the middle — the same low
/// single `e2e_baserunning_breaks` uses: a fair grounder a set fielder
/// gathers and throws. The press lands where the bat *meets* the ball
/// (`bat_arrival_z`), not where the ball is on the press frame.
fn drive(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
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
    let fielding = score.fielding_team();
    let batting = score.batting_team();
    match play.phase {
        Phase::PrePitch => {
            let intent = intents.get_mut(fielding);
            intent.aim = Vec2::ZERO;
            intent.action = true;
        }
        Phase::Pitch => {
            if let Ok((t, v)) = ball.single() {
                if (-0.1..=0.05).contains(&bat_arrival_z(t.translation.z, v.linvel.z)) {
                    let intent = intents.get_mut(batting);
                    intent.aim = Vec2::new(0.0, -1.0);
                    intent.action = true;
                }
            }
        }
        _ => {}
    }
}

fn shot(app: &App) -> Option<Shot> {
    app.world().resource::<BroadcastRig>().shot()
}

fn phase(app: &App) -> Phase {
    app.world().resource::<Play>().phase
}

#[test]
fn a_grounder_cuts_to_the_ball_then_the_base_cam_and_holds_the_call_there() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);

    let contact = run_until(&mut app, MAX_FRAMES, |app| phase(app) == Phase::InPlay);
    assert!(contact.is_some(), "never put the ball in play");
    assert!(
        matches!(shot(&app), Some(Shot::Duel(_))),
        "contact must hold the plate framing, saw {:?}",
        shot(&app)
    );

    // The cut away from the plate comes within the post-contact hold.
    let cut = run_until(&mut app, (0.5 / DT) as u64, |app| {
        !matches!(shot(app), Some(Shot::Duel(_)))
    });
    assert!(
        cut.is_some(),
        "the camera never cut away from the plate after contact"
    );
    assert_eq!(
        shot(&app),
        Some(Shot::BallFollow),
        "the first cut after contact is to the ball"
    );

    // The throw brings the base cam, still during the live play.
    let thrown = run_until(&mut app, MAX_FRAMES, |app| {
        matches!(shot(app), Some(Shot::BaseCam(_))) || phase(app) != Phase::InPlay
    });
    assert!(thrown.is_some());
    let Some(Shot::BaseCam(base)) = shot(&app) else {
        panic!(
            "the throw never cut to a base cam (shot {:?}, phase {:?})",
            shot(&app),
            phase(&app)
        );
    };
    assert_eq!(
        phase(&app),
        Phase::InPlay,
        "the base cam must arrive while the play is live"
    );

    // The call lands in that shot and the result pause keeps it — and an
    // umpire signals it (safe or out, whichever the race decided).
    let called = run_until(&mut app, MAX_FRAMES, |app| phase(app) == Phase::Result);
    assert!(called.is_some(), "the play never resolved");
    let signalled = run_until(&mut app, 30, |app| {
        app.world_mut()
            .query_filtered::<&Playing, With<Umpire>>()
            .iter(app.world())
            .any(|p| matches!(p.clip, AnimClip::UmpSafe | AnimClip::UmpPunchOut))
    });
    assert!(
        signalled.is_some(),
        "an umpire must signal the call at the bag"
    );
    for _ in 0..8 {
        app.update();
        assert_eq!(
            shot(&app),
            Some(Shot::BaseCam(base)),
            "the result pause must hold the base cam the call landed in"
        );
    }

    // The play ended the plate appearance, so PrePitch opens on the next
    // batter's walk-up shot (TODO 103); the duel framing follows once the
    // walk-up hold ends.
    let next = run_until(&mut app, MAX_FRAMES, |app| phase(app) == Phase::PrePitch);
    assert!(next.is_some());
    app.update();
    assert_eq!(
        shot(&app),
        Some(Shot::WalkUp),
        "PrePitch must open on the walk-up"
    );
    let duel = run_until(&mut app, MAX_FRAMES, |app| {
        !app.world().resource::<Play>().walkup_active()
    });
    assert!(duel.is_some(), "the walk-up never ended");
    app.update();
    assert!(
        matches!(shot(&app), Some(Shot::Duel(_))),
        "the walk-up must cut back to the duel, saw {:?}",
        shot(&app)
    );
}

/// A catchable fly with a chaser under it takes the fielder cam while the
/// ball is still coming down (the dead-on press `e2e_contact_stamp` uses
/// puts a fly in the air).
fn drive_fly(
    state: Res<State<GameState>>,
    play: Option<Res<Play>>,
    score: Option<Res<ScoreBoard>>,
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
    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(score.fielding_team()).action = true;
        }
        Phase::Pitch => {
            if let Ok((t, v)) = ball.single() {
                if (-0.3..=0.3).contains(&bat_arrival_z(t.translation.z, v.linvel.z)) {
                    intents.get_mut(score.batting_team()).action = true;
                }
            }
        }
        _ => {}
    }
}

#[test]
fn a_fly_ball_takes_the_fielder_cam_on_the_way_down() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive_fly);
    start_game(&mut app, KeyCode::Digit2);
    // Keep the dead-on fly in the park: at the shipped exit multipliers it
    // clears the fence, and a home run is (rightly) followed, not
    // fielder-cammed.
    {
        let mut r = app.world_mut().resource_mut::<Ruleset>();
        r.batting.exit_perfect = 0.7;
        r.batting.exit_solid = 0.7;
    }

    let contact = run_until(&mut app, MAX_FRAMES, |app| phase(app) == Phase::InPlay);
    assert!(contact.is_some(), "never put the ball in play");
    let mut seen = Vec::new();
    let mut frames = 0u64;
    let mut peak_y = 0.0f32;
    while phase(&app) == Phase::InPlay {
        if let Some(s) = shot(&app) {
            if seen.last() != Some(&s) {
                seen.push(s);
            }
        }
        if let Ok(t) = app
            .world_mut()
            .query_filtered::<&Transform, With<Baseball>>()
            .single(app.world())
        {
            peak_y = peak_y.max(t.translation.y);
        }
        app.update();
        frames += 1;
        assert!(frames < MAX_FRAMES, "the play never resolved");
    }
    assert!(
        seen.iter().any(|s| matches!(s, Shot::FielderCam(_))),
        "a fly ball should cut to the fielder cam on the way down; shots seen: {seen:?}, peak ball height {peak_y:.2} m"
    );
}
