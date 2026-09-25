//! Unit tests for [`super`] — the screen module.

use super::*;

// Keyboard path only: the harness has no gamepad-input injection
// precedent anywhere in the crate (no test presses a `GamepadButton`),
// so the gamepad half of `toggle_settings`/`edit_settings` isn't
// exercised in unit tests — the `Query<&Gamepad>` is simply empty here
// and both systems fall through to their keyboard branch unchanged.
#[test]
fn s_key_toggles_and_esc_closes() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<SettingsOpen>()
        .init_resource::<SettingsCursorRow>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, toggle_settings);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyS);
    app.update();
    assert!(app.world().resource::<SettingsOpen>().0);

    let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    kb.clear_just_pressed(KeyCode::KeyS);
    kb.press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<SettingsOpen>().0);
}

/// The `ROW_*` consts and the positional `ROW_LABELS` array are two
/// representations of one ordering; a drift between them compiles fine
/// and ships a screen whose labels name one setting while cycling edits
/// another. Pin the pairing.
#[test]
fn row_consts_agree_with_their_labels() {
    for (row, want) in [
        (ROW_P1_STYLE, "P1 BATTING STYLE"),
        (ROW_P2_STYLE, "P2 BATTING STYLE"),
        (ROW_TOUCH, "P1 TOUCH SWING"),
        (ROW_TRAIL, "PITCH TRAIL"),
        (ROW_TRAIL_COLOR, "TRAIL COLOR"),
        (ROW_REDUCE_MOTION, "REDUCE MOTION"),
        (ROW_STRIKE_ZONE, "STRIKE ZONE"),
        (ROW_VOLUME, "VOLUME"),
        (ROW_CLOSE, "CLOSE"),
    ] {
        assert_eq!(ROW_LABELS[row], want, "row {row} label drifted");
    }
    assert_eq!(ROW_LABELS.len(), ROW_CLOSE + 1, "CLOSE must stay last");
}

#[test]
fn touch_row_cycles_and_close_row_closes() {
    use crate::game::settings::{BattingStyle, TouchScheme};
    let mut settings = Settings::default();
    let mut open = SettingsOpen(true);
    // Row 2 is P1 TOUCH SWING: forward cycle walks the schemes.
    cycle_row(ROW_TOUCH, true, false, false, &mut settings, &mut open);
    assert_eq!(settings.touch_scheme, TouchScheme::Tap);
    assert!(open.0, "editing a value row must not close the screen");
    // While the touch scheme owns P1's style (the caller-threaded
    // verdict), the row is dormant: cycling it must not invisibly
    // rewrite the stored (hidden) value.
    cycle_row(ROW_P1_STYLE, true, false, true, &mut settings, &mut open);
    assert_eq!(settings.batting_style[0], BattingStyle::ClassicTiming);
    // Backward from Off wraps to the last scheme.
    settings.touch_scheme = TouchScheme::Off;
    cycle_row(ROW_TOUCH, false, false, false, &mut settings, &mut open);
    assert_eq!(settings.touch_scheme, TouchScheme::ZonePad);
    // With no override in force the style row edits normally again.
    settings.touch_scheme = TouchScheme::Off;
    cycle_row(ROW_P1_STYLE, true, false, false, &mut settings, &mut open);
    assert_eq!(settings.batting_style[0], BattingStyle::SwingMeter);
    // The tap path (wrap_volume) wraps full volume to mute so touch can
    // reach every level; the keyboard path clamps at full instead.
    settings.volume = 1.0;
    cycle_row(ROW_VOLUME, true, true, false, &mut settings, &mut open);
    assert!(settings.volume.abs() < f32::EPSILON, "tap wraps full→mute");
    cycle_row(ROW_VOLUME, false, true, false, &mut settings, &mut open);
    assert!(settings.volume.abs() < f32::EPSILON, "Left floors at zero");
    settings.volume = 1.0;
    cycle_row(ROW_VOLUME, true, false, false, &mut settings, &mut open);
    assert!(
        (settings.volume - 1.0).abs() < f32::EPSILON,
        "keyboard Right clamps at full, never surprise-mutes"
    );
    assert!(open.0, "volume edits must not close the screen");
    // The CLOSE pseudo-row closes instead of editing.
    cycle_row(ROW_CLOSE, true, false, false, &mut settings, &mut open);
    assert!(!open.0);
}

#[test]
fn edit_settings_cycles_style_and_clamps_volume() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(SettingsOpen(true))
        .init_resource::<SettingsCursorRow>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Controllers>()
        .insert_resource(Settings {
            volume: 0.95,
            ..Default::default()
        })
        .add_systems(Update, edit_settings);

    // Row 0 (P1 style): Right cycles forward.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ArrowRight);
    app.update();
    assert_eq!(
        app.world().resource::<Settings>().batting_style[0],
        BattingStyle::SwingMeter
    );

    // Move to the volume row (second-to-last — the CLOSE pseudo-row sits
    // beneath it) and push past the clamp. `press` only re-marks
    // `just_pressed` after a `release` — a held key doesn't repeat — so
    // each tap is release-then-press.
    let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    kb.release(KeyCode::ArrowRight);
    for _ in 0..ROW_LABELS.len() - 2 {
        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.press(KeyCode::ArrowDown);
        app.update();
        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.release(KeyCode::ArrowDown);
    }
    assert_eq!(
        app.world().resource::<SettingsCursorRow>().0,
        ROW_LABELS.len() - 2
    );

    let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    kb.release(KeyCode::ArrowDown);
    kb.press(KeyCode::ArrowRight);
    app.update();
    assert!((app.world().resource::<Settings>().volume - 1.0).abs() < f32::EPSILON);
}
