//! Unit tests for [`super`] — the juice module.

use super::*;
use crate::game::Team;
use std::time::Duration;

/// A minimal app: `MinimalPlugins` (for `Time`) plus `StatesPlugin` (for
/// `GameState`) plus the plugin under test — no rendering, physics, or
/// the rest of `GamePlugin`. `ContactEvent` is registered directly since
/// `FlowPlugin` isn't present.
fn test_app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(bevy::state::app::StatesPlugin)
        .init_state::<GameState>()
        .init_resource::<Play>()
        .add_event::<ContactEvent>()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 60.0),
        ))
        .add_plugins(JuicePlugin);
    app.world_mut()
        .resource_mut::<NextState<GameState>>()
        .set(GameState::Playing);
    app.update(); // Applies the state transition before any assertions.
    app
}

fn send_perfect(app: &mut App) {
    app.world_mut().send_event(ContactEvent {
        quality: ContactQuality::Perfect,
        batting_team: Team::Home,
        dt_ms: 0.0,
    });
}

fn speed(app: &App) -> f32 {
    app.world().resource::<Time<Virtual>>().relative_speed()
}

#[test]
fn perfect_contact_restores_speed_within_budget() {
    let mut app = test_app();
    assert_eq!(speed(&app), 1.0, "must start at full speed");

    send_perfect(&mut app);
    app.update();
    assert!(
        speed(&app) < 1.0,
        "a Perfect swing must engage the freeze, got {}",
        speed(&app)
    );

    // 250 frames at 1/60 s ≈ 4.17 s real time — comfortably past both
    // the natural freeze+slow-mo completion and the watchdog's 3 s
    // budget, so this holds regardless of which mechanism restored it.
    for _ in 0..250 {
        app.update();
    }
    assert_eq!(
        speed(&app),
        1.0,
        "relative_speed must be back to 1.0 within the budget"
    );
}

#[test]
fn watchdog_restores_to_base_speed_not_one() {
    let mut app = test_app();
    app.insert_resource(BaseSpeed(0.5));
    send_perfect(&mut app);
    for _ in 0..250 {
        app.update();
    }
    assert_eq!(
        speed(&app),
        0.5,
        "restore must return to the debug base speed"
    );
}

#[test]
fn juice_disabled_blocks_every_effect() {
    let mut app = test_app();
    app.insert_resource(JuiceDisabled);

    send_perfect(&mut app);
    for _ in 0..10 {
        app.update();
        assert_eq!(
            speed(&app),
            1.0,
            "JuiceDisabled must keep relative_speed untouched"
        );
    }
}
