//! Main-menu touch e2e: a raw finger tap on the laid-out "One Player" line
//! starts a game, through the real schedule and the real UI layout.
//!
//! Every menu tap surface hit-tests a Button's `ComputedNode` against its
//! transform. Since Bevy 0.17 UI nodes carry `UiGlobalTransform` and no
//! `GlobalTransform`, so a hit-test query written against the latter matches
//! no entity at all: on the web every menu tap and click did nothing, while
//! the keyboard- and pad-driven suites stayed green. This suite taps.

use crate::common;

use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use breakneck_baseball::game::GameState;
use common::{headless_app, run_until};

#[test]
fn tapping_one_player_starts_a_game() {
    tap_one_player_in_window(1280, 720);
}

/// A phone held sideways: the UI is scaled down to fit (`UiScale` < 1), so
/// the hit-test must convert UI units back to window pixels.
#[test]
fn tapping_one_player_starts_a_game_on_a_landscape_phone() {
    tap_one_player_in_window(844, 390);
}

fn tap_one_player_in_window(width: u32, height: u32) {
    let mut app = headless_app();
    // Windowless harness: a primary window entity gives the UI camera a
    // target, so the menu lays out at this size.
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: bevy::window::WindowResolution::new(width, height),
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    for _ in 0..6 {
        app.update();
    }
    assert_eq!(
        *app.world().resource::<State<GameState>>().get(),
        GameState::MainMenu
    );

    let ui_scale = app.world().resource::<UiScale>().0;
    // The topmost laid-out Button is the menu's first row, One Player.
    let mut buttons = app
        .world_mut()
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Button>>();
    let (size, center) = buttons
        .iter(app.world())
        .filter(|(node, _)| node.size().x > 0.0)
        .map(|(node, t)| {
            // Physical layout pixels back to window pixels: the inverse
            // scale undoes DPI and UI scale together, so the UI scale is
            // multiplied back in.
            let to_window = node.inverse_scale_factor() * ui_scale;
            (node.size() * to_window, t.translation * to_window)
        })
        .min_by(|a, b| a.1.y.total_cmp(&b.1.y))
        .expect("the menu lays out its Buttons");
    assert!(size.y > 0.0, "the One Player line has a height");

    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        app.world_mut().write_message(TouchInput {
            phase,
            position: center,
            window,
            force: None,
            id: 3,
        });
        app.update();
    }
    run_until(&mut app, 30, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Playing
    });
    assert_eq!(
        *app.world().resource::<State<GameState>>().get(),
        GameState::Playing,
        "a tap on One Player starts the game"
    );
}
