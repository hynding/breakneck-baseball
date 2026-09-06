//! The full touch input pipeline, end to end in the REAL schedule: raw
//! `TouchInput` window events → Bevy's `InputSystem` → `detect`/`resolve`
//! ownership → `read_touch` → `gather_intents` → `Intents`.
//!
//! The translator's unit tests hand-wire a minimal app (their own ordering,
//! their own resources), and the zone-pad matrix cell deliberately pins
//! that a Director-driven slot is NOT touch-owned — so neither would catch
//! a schedule-level regression (a reordered PreUpdate set, a broken
//! `resolve_touch_owner`-before-`gather_intents` edge). This test would:
//! raw window events survive to `InputSystem`, so the DriveGame injection
//! rule (which exists because the input plugin's PreUpdate clear wipes
//! *presses* made outside it) doesn't apply to them.

mod common;

use bevy::input::touch::{TouchInput, TouchPhase};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use breakneck_baseball::game::Team;
use breakneck_baseball::game::input::{Controllers, Intents};

use common::{headless_app, start_game};

#[test]
fn touch_drag_reaches_intents_through_the_real_schedule() {
    let mut app = headless_app();
    // The harness runs windowless (no winit, no GPU); the translator only
    // needs an entity with window geometry to hit-test against.
    let window = app
        .world_mut()
        .spawn((
            Window {
                // Typed: bare float literals here fall back to f32 through
                // a bound that is being phased out (rust-lang#154024) — the
                // workspace's only warning, and a future hard error.
                resolution: (1280.0_f32, 720.0_f32).into(),
                ..Default::default()
            },
            PrimaryWindow,
        ))
        .id();
    start_game(&mut app, KeyCode::Digit1);

    let send = |app: &mut App, phase: TouchPhase, pos: Vec2| {
        app.world_mut().send_event(TouchInput {
            phase,
            position: pos,
            window,
            force: None,
            id: 7,
        });
    };
    // A finger lands and drags straight down past the stick radius — on
    // defense (top of the 1st, Home pitches) the generic mapping makes it
    // the virtual-stick aim.
    send(&mut app, TouchPhase::Started, Vec2::new(600.0, 300.0));
    app.update();
    assert_eq!(
        app.world().resource::<Controllers>().touch_team,
        Some(Team::Home),
        "a real touch grants Home ownership the same frame, through the schedule"
    );
    send(&mut app, TouchPhase::Moved, Vec2::new(600.0, 450.0));
    app.update();
    let aim = app.world().resource::<Intents>().get(Team::Home).aim;
    assert!(
        aim.y < -0.9,
        "the down-drag must reach Intents through resolve→read→gather, got {aim:?}"
    );
    // Lift: the merged intent returns to neutral (nothing latches).
    send(&mut app, TouchPhase::Ended, Vec2::new(600.0, 450.0));
    app.update();
    app.update();
    let aim = app.world().resource::<Intents>().get(Team::Home).aim;
    assert!(
        aim.length() < 1e-5,
        "a lifted finger leaves no residual aim, got {aim:?}"
    );
}
