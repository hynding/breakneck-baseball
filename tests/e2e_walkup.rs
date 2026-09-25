//! End-to-end: the walk-up between plate appearances (TODO 103). After a
//! strikeout the next batter walks up: the ball is held (a held pitch press
//! throws nothing), the walk-up shot and card are on, and the hold ends
//! either on its own or on the batting side's action — through the curtain,
//! with the duel framing back for the next pitch.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::animation::{AnimClip, Playing};
use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::camera::{BroadcastRig, Shot};
use breakneck_baseball::game::flow::{CURTAIN_SECS, Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::player::{BATTER_STAND_X, Batter, PlateUmpire};
use breakneck_baseball::game::ui::{BannerText, CurtainRoot, WalkUpLine, WalkUpText, WipeLabel};
use breakneck_baseball::game::variant::Ruleset;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{DT, DriveGame, headless_app, run_until, start_game};

const MAX_FRAMES: u64 = 30_000;

/// Whether the script presses the batting side's action during a walk-up.
#[derive(Resource, Default)]
struct Dismiss(bool);

/// Pitches whenever the ball is held, swings far too early at every pitch
/// (three whiffs = a strikeout), and, when told to, presses the batting
/// side's action to dismiss the walk-up.
fn drive(
    state: Res<State<GameState>>,
    dismiss: Res<Dismiss>,
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
    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(fielding).action = true;
            if dismiss.0 && play.walkup_active() {
                intents.get_mut(batting).action = true;
            }
        }
        Phase::Pitch => {
            if let Ok(t) = ball.get_single() {
                if (5.0..=6.0).contains(&t.translation.z) {
                    intents.get_mut(batting).action = true;
                }
            }
        }
        _ => {}
    }
}

fn phase(app: &App) -> Phase {
    app.world().resource::<Play>().phase
}

fn walkup(app: &App) -> bool {
    app.world().resource::<Play>().walkup_active()
}

fn shot(app: &App) -> Option<Shot> {
    app.world().resource::<BroadcastRig>().shot()
}

fn card_line(app: &mut App, line: WalkUpLine) -> String {
    let mut q = app.world_mut().query::<(&WalkUpText, &Text)>();
    q.iter(app.world())
        .find(|(l, _)| l.0 == line)
        .map(|(_, t)| t.0.clone())
        .unwrap_or_default()
}

fn strike_out(app: &mut App) {
    let start_outs = app.world().resource::<ScoreBoard>().outs;
    let k = run_until(app, MAX_FRAMES, |app| {
        app.world().resource::<ScoreBoard>().outs > start_outs
    });
    assert!(k.is_some(), "three early whiffs never produced a strikeout");
    // Strike three's pause is shot from the reaction cam (TODO 104)…
    app.update();
    assert_eq!(phase(app), Phase::Result);
    assert_eq!(shot(app), Some(Shot::ReactionCam));
    // …and the plate umpire rings him up.
    let rung_up = run_until(app, 30, |app| {
        app.world_mut()
            .query_filtered::<&Playing, With<PlateUmpire>>()
            .single(app.world())
            .clip
            == AnimClip::UmpPunchOut
    });
    assert!(
        rung_up.is_some(),
        "the plate umpire must ring up strike three"
    );
    // The banner announces the call, then swaps to the out count a second
    // in ("OUT n" — the reference's out counter, TODO 104).
    let outs = app.world().resource::<ScoreBoard>().outs;
    let banner = |app: &mut App| {
        app.world_mut()
            .query_filtered::<&Text, With<BannerText>>()
            .single(app.world())
            .0
            .clone()
    };
    assert_eq!(banner(app), "STRIKEOUT!");
    let swapped = run_until(app, (1.5 / DT) as u64, |app| {
        app.world_mut()
            .query_filtered::<&Text, With<BannerText>>()
            .single(app.world())
            .0
            .starts_with("OUT ")
    });
    assert!(
        swapped.is_some(),
        "the strikeout banner never swapped to the out count"
    );
    assert_eq!(banner(app), format!("OUT {outs}"));
    // …and the batter walks off before the pause ends (TODO 104).
    let batter_x = |app: &mut App| {
        app.world_mut()
            .query_filtered::<&Transform, With<Batter>>()
            .single(app.world())
            .translation
            .x
    };
    let walked = run_until(app, MAX_FRAMES, |app| {
        phase(app) != Phase::Result || batter_x(app) > BATTER_STAND_X + 0.5
    });
    assert!(walked.is_some());
    assert!(
        batter_x(app) > BATTER_STAND_X + 0.5,
        "the struck-out batter should walk off during the pause (x = {})",
        batter_x(app)
    );
    // The plate appearance is over, so the pause ends with the themed wipe:
    // the panel sweeps in from off the left edge and carries the batting
    // team's name once it is across (TODO 103's wipe).
    let panel_left = |app: &mut App| match app
        .world_mut()
        .query_filtered::<&Node, With<CurtainRoot>>()
        .single(app.world())
        .left
    {
        Val::Percent(p) => p,
        other => panic!("the curtain's left edge should be a percent, was {other:?}"),
    };
    let mut leftmost = 0.0f32;
    let mut label_when_across = String::new();
    let mut frames = 0u64;
    while phase(app) == Phase::Result {
        leftmost = leftmost.min(panel_left(app));
        let label = app
            .world_mut()
            .query_filtered::<&Text, With<WipeLabel>>()
            .single(app.world())
            .0
            .clone();
        if !label.is_empty() {
            label_when_across = label;
        }
        app.update();
        frames += 1;
        assert!(
            frames < MAX_FRAMES,
            "the strikeout's result pause never ended"
        );
    }
    assert!(
        leftmost < -50.0,
        "the wipe should sweep in from off-screen, leftmost {leftmost}"
    );
    let batting = app.world().resource::<ScoreBoard>().batting_team().label();
    assert_eq!(
        label_when_across, batting,
        "the wipe names the team coming up"
    );
    assert!(
        (panel_left(app)).abs() < 1.0,
        "the panel is flush when the phase flips"
    );
    // Back in the box for the next batter's walk-up.
    app.update();
    let x = batter_x(app);
    assert!(
        (x - BATTER_STAND_X).abs() < 0.05,
        "the batter must be back in the box, x = {x}"
    );
}

#[test]
fn a_new_batter_walks_up_and_the_hold_ends_by_timer_or_by_press() {
    let mut app = headless_app();
    app.init_resource::<Dismiss>();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);
    let walkup_secs = app.world().resource::<Ruleset>().pace.walkup_secs;

    // The first plate appearance of the game has no walk-up: nobody just
    // finished one.
    assert!(!walkup(&app));

    // Strikeout → the next batter walks up: shot, card, and a held ball.
    strike_out(&mut app);
    assert!(
        walkup(&app),
        "a strikeout must bring the next batter's walk-up"
    );
    app.update();
    assert_eq!(shot(&app), Some(Shot::WalkUp));
    assert_eq!(card_line(&mut app, WalkUpLine::Title), "UP TO BAT");
    assert!(
        !card_line(&mut app, WalkUpLine::Name).is_empty(),
        "the card names the batter"
    );

    // The script holds the pitch button the whole time: nothing is thrown
    // until the hold expires on its own (plus the curtain), and the duel
    // framing is back once it does.
    let mut frames = 0u64;
    while walkup(&app) {
        assert_eq!(
            phase(&app),
            Phase::PrePitch,
            "the walk-up must hold the pitch"
        );
        app.update();
        frames += 1;
        assert!(frames < MAX_FRAMES, "the walk-up never ended");
    }
    let expected = ((walkup_secs + CURTAIN_SECS) as f64 / DT).round() as i64;
    assert!(
        (frames as i64 - expected).abs() <= 6,
        "walk-up held {frames} frames, expected ~{expected}"
    );
    // The presentation follows flow within a frame.
    app.update();
    assert!(
        card_line(&mut app, WalkUpLine::Name).is_empty(),
        "the card blanks with the hold"
    );
    assert!(
        matches!(shot(&app), Some(Shot::Duel(_))),
        "saw {:?}",
        shot(&app)
    );

    // Second strikeout: this time the batting side presses to dismiss —
    // the hold ends early, still through the curtain.
    app.world_mut().resource_mut::<Dismiss>().0 = true;
    strike_out(&mut app);
    assert!(walkup(&app));
    let mut frames = 0u64;
    while walkup(&app) {
        app.update();
        frames += 1;
        assert!(frames < MAX_FRAMES);
    }
    let curtain_frames = (CURTAIN_SECS as f64 / DT).round() as i64;
    assert!(
        (frames as i64 - curtain_frames).abs() <= 6,
        "a press should end the walk-up after just the curtain ({curtain_frames} frames), took {frames}"
    );
}

/// The browser runs with batter fidgets on (the headless harness disables
/// them): the walk-up's back-to-back fidgets must not stall the hold.
#[test]
fn the_walk_up_ends_with_fidgets_enabled() {
    use breakneck_baseball::game::animation::FidgetsDisabled;
    let mut app = headless_app();
    app.init_resource::<Dismiss>();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);
    app.world_mut().remove_resource::<FidgetsDisabled>();

    strike_out(&mut app);
    assert!(walkup(&app));
    let ended = run_until(&mut app, MAX_FRAMES, |app| !walkup(app));
    assert!(
        ended.is_some(),
        "the walk-up never ended with fidgets enabled"
    );
}
