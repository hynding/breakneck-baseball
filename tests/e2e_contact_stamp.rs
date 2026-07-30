//! End-to-end coverage for the contact stamp and zone flash (Task B4): a
//! scripted dead-on press grades `ContactQuality::Perfect` (as pinned by
//! `e2e_contact_timing`), and that must:
//!   * stamp `PERFECT!` over the zone-box area, blanking again after its
//!     ~0.8 s display timer, and
//!   * pulse the zone box visible a beat *past* the same-frame flip to
//!     `Phase::InPlay` (its own hide condition) so the flash actually gets a
//!     chance to render, then let it hide again once the shorter flash timer
//!     expires — the fix for the review finding that the box could vanish
//!     the very frame the pulse was set.

mod common;

use bevy::prelude::*;

use breakneck_baseball::game::ball::Baseball;
use breakneck_baseball::game::field::StrikeZoneOverlay;
use breakneck_baseball::game::flow::{Phase, Play};
use breakneck_baseball::game::input::Intents;
use breakneck_baseball::game::ui::ContactStampText;
use breakneck_baseball::game::{GameState, ScoreBoard};

use common::{headless_app, run_until, start_game, DriveGame};

const MAX_FRAMES: u64 = 15_000;

/// Pitches a straightaway changeup, then presses the batting side's action
/// button the instant the ball is dead-on the plate — the exact stage-0
/// script `e2e_contact_timing` uses to grade `ContactQuality::Perfect`.
fn drive(
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

    match play.phase {
        Phase::PrePitch => {
            intents.get_mut(fielding).action = true;
        }
        Phase::Pitch => {
            if let Ok(t) = ball.get_single() {
                let z = t.translation.z;
                if (-0.3..=0.3).contains(&z) {
                    intents.get_mut(batting).action = true;
                }
            }
        }
        _ => {}
    }
}

fn stamp_text(app: &mut App) -> String {
    app.world_mut()
        .query_filtered::<&Text, With<ContactStampText>>()
        .single(app.world())
        .0
        .clone()
}

/// True if every piece of the zone box (fill + 4 frame bars) currently shows
/// `want` — the overlay is always painted uniformly, see `field.rs`.
fn zone_all(app: &mut App, want: Visibility) -> bool {
    let mut query = app
        .world_mut()
        .query_filtered::<&Visibility, With<StrikeZoneOverlay>>();
    let mut seen_any = false;
    for visibility in query.iter(app.world()) {
        seen_any = true;
        if *visibility != want {
            return false;
        }
    }
    seen_any
}

#[test]
fn perfect_contact_stamps_and_flashes_the_zone() {
    let mut app = headless_app();
    app.add_systems(DriveGame, drive);
    start_game(&mut app, KeyCode::Digit2);

    // Starts blank (painted at spawn with an empty string — see ui.rs).
    assert_eq!(stamp_text(&mut app), "", "stamp must start blank");

    // Drive to the dead-on press: contact grades Perfect (pinned by
    // e2e_contact_timing's stage 0) and the ball goes into play.
    let landed = run_until(&mut app, MAX_FRAMES, |app| {
        app.world().resource::<Play>().phase == Phase::InPlay
    });
    assert!(
        landed.is_some(),
        "never reached a live ball off the dead-on swing"
    );

    // The flip to InPlay is the zone box's own hide condition — without the
    // fix, it hides within a frame or two of this point (a same-frame stale
    // read can buy one extra frame by accident, no more). The Task B4 flash
    // pulse must hold the box up for its whole ~0.18 s window, so check well
    // past that accidental margin, not just the instant `landed` returns.
    for frame in 0..10 {
        assert!(
            zone_all(&mut app, Visibility::Inherited),
            "the zone box hid {frame} frame(s) after InPlay started, \
             before the flash pulse's window could run out"
        );
        app.update();
    }

    let stamped = run_until(&mut app, MAX_FRAMES, |app| stamp_text(app) == "PERFECT!");
    assert!(
        stamped.is_some(),
        "a dead-on swing never stamped PERFECT! over the zone box (last seen: {:?})",
        stamp_text(&mut app)
    );

    // The flash pulse is short-lived: the box hides again once it expires,
    // well before the longer-lived stamp blanks on its own timer.
    let zone_hidden = run_until(&mut app, MAX_FRAMES, |app| {
        zone_all(app, Visibility::Hidden)
    });
    assert!(
        zone_hidden.is_some(),
        "the zone box never hid again once the flash pulse expired"
    );

    // The stamp blanks after its own ~0.8 s display timer.
    let blanked = run_until(&mut app, MAX_FRAMES, |app| stamp_text(app).is_empty());
    assert!(
        blanked.is_some(),
        "the PERFECT! stamp never blanked after its display timer"
    );
}
