//! Unit tests for [`super`] — the batting module.

use super::*;
use crate::game::GameMode;
use crate::game::input::assign_controllers;

#[test]
fn meter_release_fires_the_swing() {
    assert_eq!(meter_step(false, true, false), (false, true));
}

#[test]
fn meter_holding_past_the_window_is_a_swinging_whiff() {
    assert_eq!(meter_step(true, true, true), (false, true));
}

#[test]
fn meter_press_starts_loading_without_swinging() {
    assert_eq!(meter_step(true, false, false), (true, false));
}

#[test]
fn meter_idle_does_nothing() {
    // No hold, nothing loaded: neither loads nor fires.
    assert_eq!(meter_step(false, false, false), (false, false));
}

#[test]
fn meter_load_frac_ramps_and_clamps() {
    let mut meter = MeterState::default();
    assert_eq!(meter.load_frac(Team::Away, 5.0), 0.0);
    meter.start(Team::Away, 5.0);
    assert!(meter.loading(Team::Away));
    assert!((meter.load_frac(Team::Away, 5.5) - 0.5).abs() < 1e-6);
    assert_eq!(meter.load_frac(Team::Away, 7.0), 1.0); // clamped at full
    meter.clear(Team::Away);
    assert!(!meter.loading(Team::Away));
    assert_eq!(meter.load_frac(Team::Away, 7.0), 0.0);
}

#[test]
fn touch_scheme_owns_p1_style_and_never_touches_cpu_or_p2() {
    use crate::game::settings::TouchScheme;
    let settings = Settings {
        batting_style: [BattingStyle::ClassicTiming, BattingStyle::ClassicTiming],
        touch_scheme: TouchScheme::ZonePad,
        ..Settings::default()
    };
    // 1P: the scheme decides the touch-owned P1 (Home); the CPU stays
    // Classic.
    let mut one = assign_controllers(GameMode::OnePlayer, &[]);
    one.touch_team = Some(Team::Home);
    assert_eq!(
        style_for(Team::Home, &one, &settings),
        BattingStyle::PciCursor
    );
    assert_eq!(
        style_for(Team::Away, &one, &settings),
        BattingStyle::ClassicTiming
    );
    // 2P: P2 keeps their configured style.
    let mut two = assign_controllers(GameMode::TwoPlayers, &[]);
    two.touch_team = Some(Team::Home);
    assert_eq!(
        style_for(Team::Away, &two, &settings),
        BattingStyle::ClassicTiming
    );
    // No touch owner (a Director-driven slot, per `resolve_touch_owner`):
    // the scheme never overrides — configured style holds.
    let undirected = assign_controllers(GameMode::OnePlayer, &[]);
    assert_eq!(
        style_for(Team::Home, &undirected, &settings),
        BattingStyle::ClassicTiming
    );
    // Off: P1's configured style applies unchanged.
    let off = Settings::default();
    assert_eq!(
        style_for(Team::Home, &one, &off),
        BattingStyle::ClassicTiming
    );
}

/// Pins the Zone Pad's absolute-cursor path end to end at the adapter:
/// a `TeamIntent::cursor` set from the `Intents` seam (exactly what the
/// touch translator — or a Director `Cursor` action — produces) must
/// snap the PCI cursor, clamped to the zone, and a press must grade at
/// the snapped spot via `pci_offset`.
#[test]
fn pci_adapter_snaps_to_an_absolute_cursor_from_intents() {
    use crate::game::flow::{Phase, Play};
    use crate::game::input::Intents;
    use crate::game::settings::TouchScheme;
    use crate::game::variant::VariantId;
    use crate::game::{GameMode, ScoreBoard};

    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut play = Play::default();
    play.phase = Phase::Pitch;
    // Home (the touch team) bats.
    let score = ScoreBoard {
        top_of_inning: false,
        ..Default::default()
    };
    let mut controllers = assign_controllers(GameMode::OnePlayer, &[]);
    controllers.touch_team = Some(Team::Home);
    app.insert_resource(play)
        .insert_resource(score)
        .insert_resource(controllers)
        .insert_resource(Settings {
            touch_scheme: TouchScheme::ZonePad,
            ..Settings::default()
        })
        .insert_resource(VariantId::Standard.rules())
        .init_resource::<Intents>()
        .init_resource::<SwingCommands>()
        .init_resource::<MeterState>()
        .init_resource::<MeterLoad>()
        .init_resource::<PciState>()
        .add_systems(Update, adapt_swings);

    // An off-zone cursor must clamp; the press this frame swings there.
    let cursor = Vec2::new(-10.0, 10.0);
    {
        let mut intents = app.world_mut().resource_mut::<Intents>();
        intents.home.cursor = Some(cursor);
        intents.home.action = true;
    }
    app.update();
    let snapped = app.world().resource::<PciState>().cursor(Team::Home);
    assert_eq!(
        snapped,
        Vec2::new(-rules::ZONE_HALF_WIDTH, rules::ZONE_HIGH),
        "absolute cursor should snap then clamp to the zone"
    );
    let world = app.world_mut();
    let cmd = world
        .resource_mut::<SwingCommands>()
        .take(Team::Home)
        .expect("press with ZonePad style should fire a PCI swing");
    assert_eq!(cmd.pci_offset, Some(snapped));
}

#[test]
fn cpu_always_routes_classic() {
    let mut controllers = assign_controllers(GameMode::OnePlayer, &[]);
    controllers.touch_team = Some(Team::Home);
    let settings = Settings {
        batting_style: [BattingStyle::PciCursor, BattingStyle::SwingMeter],
        ..Settings::default()
    };
    assert_eq!(
        style_for(Team::Away, &controllers, &settings),
        BattingStyle::ClassicTiming
    );
    assert_eq!(
        style_for(Team::Home, &controllers, &settings),
        BattingStyle::PciCursor
    );
}
