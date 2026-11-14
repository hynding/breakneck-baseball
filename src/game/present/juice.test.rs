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
        .add_message::<ContactEvent>()
        .add_message::<LiveBallEvent>()
        .insert_resource(crate::game::variant::VariantId::Standard.field())
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
    app.world_mut().write_message(ContactEvent {
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

fn throw_to(app: &mut App, base: usize) {
    app.world_mut().write_message(LiveBallEvent::Thrown {
        pos: Vec3::new(0.0, 0.0, 60.0),
        base,
        race_time: 3.0,
        landed_secs: 0.0,
    });
}

#[test]
fn a_throw_home_slows_until_it_reaches_the_catcher() {
    let mut app = test_app();
    let home = app.world().resource::<FieldSpec>().base_count();
    // A throw to first is no play at the plate.
    throw_to(&mut app, 0);
    app.update();
    assert_eq!(speed(&app), 1.0);

    throw_to(&mut app, home);
    app.update();
    assert!((speed(&app) - PLATE_SLOWMO_SPEED).abs() < 1e-6);
    for _ in 0..30 {
        app.update();
    }
    assert!(speed(&app) < 1.0, "still slow while the throw flies");
    app.world_mut().write_message(LiveBallEvent::Settled);
    app.update();
    assert_eq!(speed(&app), 1.0, "full speed once it's in the mitt");
}

#[test]
fn a_plate_slowmo_never_outlives_its_watchdog() {
    let mut app = test_app();
    let home = app.world().resource::<FieldSpec>().base_count();
    throw_to(&mut app, home);
    app.update();
    // No Settled ever arrives: 5 s of real frames clears the 4.5 s budget.
    for _ in 0..300 {
        app.update();
    }
    assert_eq!(speed(&app), 1.0);
}
