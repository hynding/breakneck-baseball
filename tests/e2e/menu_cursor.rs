//! Main-menu cursor e2e: a controller drives the menu the console way
//! (D-pad moves, A chooses, Left/Right change, B is not an option key), and
//! the on-screen hints follow the device in use.

use crate::common;

use bevy::prelude::*;
use breakneck_baseball::game::ai::CpuConfig;
use breakneck_baseball::game::input::ActiveDevice;
use breakneck_baseball::game::settings::{Difficulty, Settings};
use breakneck_baseball::game::{GameConfig, GameMode, GameState};
use common::{DriveGame, headless_app, run_until, tap_key};

/// A queued pad-button tap, pressed from [`DriveGame`] (after the input
/// plugin's `PreUpdate` clear, like `common::TapKey`) on every pad.
#[derive(Resource, Default)]
struct PadTap(Option<GamepadButton>);

fn apply_pad_taps(mut tap: ResMut<PadTap>, mut pads: Query<&mut Gamepad>) {
    let Some(button) = tap.0.take() else {
        return;
    };
    for mut pad in &mut pads {
        pad.digital_mut().press(button);
    }
}

fn pad_app() -> App {
    let mut app = headless_app();
    app.init_resource::<PadTap>()
        .add_systems(DriveGame, apply_pad_taps);
    app.world_mut().spawn(Gamepad::default());
    for _ in 0..4 {
        app.update();
    }
    app
}

/// Presses `button` for one frame (the pad's own `PreUpdate` clear ends the
/// edge), releases it, and steps past it.
fn tap_pad(app: &mut App, button: GamepadButton) {
    app.world_mut().resource_mut::<PadTap>().0 = Some(button);
    app.update();
    let world = app.world_mut();
    for mut pad in world.query::<&mut Gamepad>().iter_mut(world) {
        pad.digital_mut().release(button);
    }
    for _ in 0..3 {
        app.update();
    }
}

fn menu_text(app: &mut App) -> String {
    let world = app.world_mut();
    let mut all = String::new();
    for text in world.query::<&Text>().iter(world) {
        all.push_str(&text.0);
        all.push('\n');
    }
    all
}

#[test]
fn pad_cursor_changes_options_and_starts_the_focused_mode() {
    let mut app = pad_app();
    assert_eq!(
        *app.world().resource::<ActiveDevice>(),
        ActiveDevice::Gamepad,
        "a connected pad should switch the hints to the pad"
    );
    let innings = app.world().resource::<GameConfig>().innings;

    // B is "back" now, not the innings key.
    tap_pad(&mut app, GamepadButton::East);
    assert_eq!(app.world().resource::<GameConfig>().innings, innings);
    assert_eq!(
        *app.world().resource::<State<GameState>>().get(),
        GameState::MainMenu
    );

    // Rows: One Player, Two Players, Season, Field, Innings, ... — down to
    // Innings and step it with Right.
    for _ in 0..4 {
        tap_pad(&mut app, GamepadButton::DPadDown);
    }
    tap_pad(&mut app, GamepadButton::DPadRight);
    let stepped = app.world().resource::<GameConfig>().innings;
    assert_ne!(
        stepped, innings,
        "Right on the Innings row should change it"
    );

    // Back up to Two Players and choose it with A.
    for _ in 0..3 {
        tap_pad(&mut app, GamepadButton::DPadUp);
    }
    tap_pad(&mut app, GamepadButton::South);
    let started = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Playing
    });
    assert!(started.is_some(), "A on Two Players never started the game");
    let config = app.world().resource::<GameConfig>();
    assert_eq!(config.mode, GameMode::TwoPlayers);
    assert_eq!(config.innings, stepped, "the stepped innings must carry in");
}

/// The CPU difficulty row is a cursor stop like every option: Right steps
/// it, Start plays One Player from it, and the chosen step is the skill
/// the CPU actually plays the game at.
#[test]
fn pad_steps_cpu_difficulty_into_the_game() {
    let mut app = pad_app();
    assert_eq!(
        app.world().resource::<Settings>().difficulty,
        Difficulty::Normal
    );
    // Rows: One Player, Two Players, Field, Innings, Theme, CPU difficulty.
    for _ in 0..5 {
        tap_pad(&mut app, GamepadButton::DPadDown);
    }
    assert!(
        menu_text(&mut app).contains("> CPU difficulty"),
        "cursor should rest on the difficulty row:\n{}",
        menu_text(&mut app)
    );
    tap_pad(&mut app, GamepadButton::DPadRight);
    assert_eq!(
        app.world().resource::<Settings>().difficulty,
        Difficulty::Hard
    );
    // Option values are accent-tinted `TextSpan` children of the row.
    let world = app.world_mut();
    assert!(
        world
            .query::<&TextSpan>()
            .iter(world)
            .any(|span| span.0 == "Hard"),
        "menu shows the new step"
    );

    tap_pad(&mut app, GamepadButton::Start);
    let started = run_until(&mut app, 2_000, |app| {
        *app.world().resource::<State<GameState>>().get() == GameState::Playing
    });
    assert!(started.is_some(), "Start never launched the game");
    assert_eq!(
        app.world().resource::<GameConfig>().mode,
        GameMode::OnePlayer
    );
    let skill = app.world().resource::<CpuConfig>().skill;
    assert!(
        (skill - Difficulty::Hard.skill()).abs() < f32::EPSILON,
        "the CPU plays at {skill}, not the chosen Hard step"
    );
}

#[test]
fn hints_follow_the_active_device() {
    let mut app = pad_app();
    let pad_text = menu_text(&mut app);
    assert!(
        pad_text.contains("A: choose"),
        "pad hints missing:\n{pad_text}"
    );
    assert!(
        !pad_text.contains("WASD"),
        "keyboard hints leaked:\n{pad_text}"
    );
    assert!(
        pad_text.contains("> One Player"),
        "cursor missing:\n{pad_text}"
    );
    assert!(
        !pad_text.contains("1   One Player"),
        "hotkey shown to pad:\n{pad_text}"
    );

    // A keypress hands the menu back to the keyboard's hotkey hints.
    tap_key(&mut app, KeyCode::ArrowDown);
    assert_eq!(
        *app.world().resource::<ActiveDevice>(),
        ActiveDevice::Keyboard
    );
    let key_text = menu_text(&mut app);
    assert!(
        key_text.contains("WASD"),
        "keyboard hints missing:\n{key_text}"
    );
    assert!(
        !key_text.contains("A: choose"),
        "pad hints leaked:\n{key_text}"
    );
    assert!(
        key_text.contains("> 2   Two Players"),
        "cursor missing:\n{key_text}"
    );
}
