//! The settings screen end-to-end: S opens it on the menu, edits route to
//! the resource and the store, Esc closes, and a game still starts.

mod common;

use bevy::input::keyboard::KeyCode;
use breakneck_baseball::game::settings::{BattingStyle, Settings, SettingsOpen};

#[test]
fn settings_edit_persists_and_game_starts() {
    let dir = std::env::temp_dir().join(format!("bb-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::env::set_var("BREAKNECK_SETTINGS_PATH", &path);

    let mut app = common::headless_app();
    app.update();

    // S opens the screen.
    common::tap_key(&mut app, KeyCode::KeyS);
    for _ in 0..3 {
        app.update();
    }
    assert!(
        app.world().resource::<SettingsOpen>().0,
        "S must open settings"
    );

    // Right on row 0 cycles P1's style Classic -> SwingMeter.
    common::tap_key(&mut app, KeyCode::ArrowRight);
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Settings>().batting_style[0],
        BattingStyle::SwingMeter
    );

    // Persisted.
    let on_disk: Settings = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(on_disk.batting_style[0], BattingStyle::SwingMeter);

    // Esc closes; the game still starts (menu keys un-suppressed).
    common::tap_key(&mut app, KeyCode::Escape);
    for _ in 0..3 {
        app.update();
    }
    assert!(!app.world().resource::<SettingsOpen>().0);
    common::start_game(&mut app, KeyCode::Digit1);

    std::env::remove_var("BREAKNECK_SETTINGS_PATH");
    let _ = std::fs::remove_dir_all(dir);
}
