//! The settings screen: spawn/paint of its UI and the input systems that
//! drive it (opened with **S** on the main menu; see `super`'s
//! [`super::SettingsPlugin`]).

use bevy::prelude::*;

use bevy::color::Alpha;

#[cfg(test)]
use super::BattingStyle;
use super::{Settings, SettingsOpen};
use crate::game::theme::Theme;
use crate::game::ui::hidden_tint;

// ── Settings screen ──────────────────────────────────────────────────────────

/// Root of the settings screen (full-screen centered column overlay).
#[derive(Component)]
pub(super) struct SettingsUi;

/// The screen's inner card — the opaque panel that occludes the menu behind
/// it while open (mirrors `subs.rs`'s `SubsUi`/`SubsCard` split: the menu
/// isn't a 3D backdrop like gameplay, so the overlay needs its own solid
/// panel rather than relying on world geometry to read as "in front").
#[derive(Component)]
pub(super) struct SettingsCard;

/// The screen's title line ("SETTINGS" while open).
#[derive(Component)]
pub(super) struct SettingsTitle;

/// Label text for each row — tinted to show the cursor row.
#[derive(Component)]
pub(super) struct SettingsRowLabel(usize);

/// Value text for each row (styles, trail, colour, volume).
#[derive(Component)]
pub(super) struct SettingsRowText(usize);

/// Which row the cursor is on (0..ROW_LABELS.len()).
#[derive(Resource, Default)]
pub(super) struct SettingsCursorRow(usize);

const ROW_LABELS: [&str; 5] = [
    "P1 BATTING STYLE",
    "P2 BATTING STYLE",
    "PITCH TRAIL",
    "TRAIL COLOR",
    "VOLUME",
];

/// Builds the settings screen once at startup, painted behind
/// [`hidden_tint`] per the wasm UI rule (see `subs.rs`): spawned once, shown
/// and hidden only by mutating the children of this root — never despawned
/// or respawned mid-session.
pub(super) fn spawn_settings_screen(mut commands: Commands, theme: Res<Theme>) {
    let ui = &theme.ui;
    commands
        .spawn((
            SettingsUi,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(hidden_tint(ui.panel_bg)),
        ))
        .with_children(|root| {
            root.spawn((
                SettingsCard,
                Node {
                    padding: UiRect::axes(Val::Px(40.0), Val::Px(28.0)),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(10.0),
                    border: UiRect::all(Val::Px(1.5)),
                    ..default()
                },
                BackgroundColor(hidden_tint(ui.panel_bg)),
                BorderColor(hidden_tint(ui.panel_border)),
                BorderRadius::all(Val::Px(16.0)),
            ))
            .with_children(|card| {
                card.spawn((
                    SettingsTitle,
                    Text::new(""),
                    TextFont {
                        font_size: 34.0,
                        ..default()
                    },
                    TextColor(ui.accent),
                ));
                for i in 0..ROW_LABELS.len() {
                    card.spawn((
                        SettingsRowLabel(i),
                        Text::new(""),
                        TextFont {
                            font_size: 20.0,
                            ..default()
                        },
                        TextColor(ui.text_primary),
                    ));
                    card.spawn((
                        SettingsRowText(i),
                        Text::new(""),
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(ui.text_dim),
                    ));
                }
            });
        });
}

/// Paints the screen every frame from state: blank/hidden when closed, full
/// content when open (child mutation only — wasm rule, see `subs.rs`). The
/// root dims to a translucent scrim and the card goes opaque while open so
/// the settings content fully occludes the menu behind it (the menu has no
/// 3D backdrop the way gameplay does, so the overlay must paint its own).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn paint_settings_screen(
    open: Res<SettingsOpen>,
    cursor: Res<SettingsCursorRow>,
    settings: Res<Settings>,
    theme: Res<Theme>,
    mut roots: Query<&mut BackgroundColor, (With<SettingsUi>, Without<SettingsCard>)>,
    mut cards: Query<(&mut BackgroundColor, &mut BorderColor), With<SettingsCard>>,
    mut title: Query<
        (&mut Text, &mut TextColor),
        (
            With<SettingsTitle>,
            Without<SettingsRowLabel>,
            Without<SettingsRowText>,
        ),
    >,
    mut labels: Query<
        (&SettingsRowLabel, &mut Text, &mut TextColor),
        (Without<SettingsTitle>, Without<SettingsRowText>),
    >,
    mut values: Query<
        (&SettingsRowText, &mut Text),
        (Without<SettingsTitle>, Without<SettingsRowLabel>),
    >,
) {
    let Ok((mut title_text, mut title_color)) = title.get_single_mut() else {
        return;
    };
    let ui = &theme.ui;
    if !open.0 {
        for mut bg in &mut roots {
            bg.0 = hidden_tint(ui.panel_bg);
        }
        for (mut bg, mut border) in &mut cards {
            bg.0 = hidden_tint(ui.panel_bg);
            border.0 = hidden_tint(ui.panel_border);
        }
        **title_text = String::new();
        for (_, mut text, _) in &mut labels {
            **text = String::new();
        }
        for (_, mut text) in &mut values {
            **text = String::new();
        }
        return;
    }
    for mut bg in &mut roots {
        bg.0 = ui.panel_bg.with_alpha(0.9);
    }
    for (mut bg, mut border) in &mut cards {
        bg.0 = ui.panel_bg;
        border.0 = ui.panel_border;
    }
    **title_text = "SETTINGS".to_string();
    title_color.0 = ui.accent;
    for (label, mut text, mut color) in &mut labels {
        let marker = if cursor.0 == label.0 { "> " } else { "  " };
        **text = format!("{marker}{}", ROW_LABELS[label.0]);
        color.0 = if cursor.0 == label.0 {
            ui.accent
        } else {
            ui.text_primary
        };
    }
    for (row, mut text) in &mut values {
        **text = match row.0 {
            0 => settings.batting_style[0].label().to_string(),
            1 => settings.batting_style[1].label().to_string(),
            2 => settings.pitch_trail.label().to_string(),
            3 => settings.trail_color.label().to_string(),
            _ => format!("{:.0}%", settings.volume * 100.0),
        };
    }
}

/// **S** / gamepad **Select** toggles the screen open/closed; **Esc** /
/// gamepad **East** (B) closes it while open. MainMenu only. `Select` is
/// also `camera.rs`'s duel-view toggle, and `East` is `menu.rs`'s
/// innings-cycle key — no clash: those systems run only in `Playing`
/// (`Select`) or are gated `.run_if(settings_closed)` (`East`), so they
/// never fire alongside this system's `MainMenu`-only, open-state-gated
/// handling of the same buttons.
pub(super) fn toggle_settings(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut open: ResMut<SettingsOpen>,
) {
    let toggle_pressed = keyboard.just_pressed(KeyCode::KeyS)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::Select));
    let close_pressed = keyboard.just_pressed(KeyCode::Escape)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::East));
    if toggle_pressed {
        open.0 = !open.0;
    } else if open.0 && close_pressed {
        open.0 = false;
    }
}

/// Up/Down (or gamepad DPad Up/Down) move the row cursor; Left/Right (or
/// DPad Left/Right) cycle the batting style or nudge the volume (±0.1,
/// clamped). No-op while the screen is closed.
pub(super) fn edit_settings(
    keyboard: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    open: Res<SettingsOpen>,
    mut cursor: ResMut<SettingsCursorRow>,
    mut settings: ResMut<Settings>,
) {
    if !open.0 {
        return;
    }
    let up = keyboard.just_pressed(KeyCode::ArrowUp)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::DPadUp));
    let down = keyboard.just_pressed(KeyCode::ArrowDown)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::DPadDown));
    if up {
        cursor.0 = cursor.0.checked_sub(1).unwrap_or(ROW_LABELS.len() - 1);
    }
    if down {
        cursor.0 = (cursor.0 + 1) % ROW_LABELS.len();
    }
    let left = keyboard.just_pressed(KeyCode::ArrowLeft)
        || pads.iter().any(|p| p.just_pressed(GamepadButton::DPadLeft));
    let right = keyboard.just_pressed(KeyCode::ArrowRight)
        || pads
            .iter()
            .any(|p| p.just_pressed(GamepadButton::DPadRight));
    if !(left || right) {
        return;
    }
    match cursor.0 {
        0 | 1 => {
            let s = settings.batting_style[cursor.0];
            settings.batting_style[cursor.0] = if right { s.next() } else { s.prev() };
        }
        2 => {
            let s = settings.pitch_trail;
            settings.pitch_trail = if right { s.next() } else { s.prev() };
        }
        3 => {
            let c = settings.trail_color;
            settings.trail_color = if right { c.next() } else { c.prev() };
        }
        _ => {
            let dv = if right { 0.1 } else { -0.1 };
            settings.volume = (settings.volume + dv).clamp(0.0, 1.0);
        }
    }
}

/// Safety-net reset for leaving `MainMenu`: see the `OnExit` registration.
pub(super) fn close_settings_on_exit(
    mut open: ResMut<SettingsOpen>,
    mut cursor: ResMut<SettingsCursorRow>,
) {
    open.0 = false;
    cursor.0 = 0;
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn edit_settings_cycles_style_and_clamps_volume() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(SettingsOpen(true))
            .init_resource::<SettingsCursorRow>()
            .init_resource::<ButtonInput<KeyCode>>()
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

        // Move to the volume row (now the last of ROW_LABELS, past the two
        // trail rows) and push past the clamp. `press` only re-marks
        // `just_pressed` after a `release` — a held key doesn't repeat — so
        // each tap is release-then-press.
        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.release(KeyCode::ArrowRight);
        for _ in 0..ROW_LABELS.len() - 1 {
            let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            kb.press(KeyCode::ArrowDown);
            app.update();
            let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            kb.release(KeyCode::ArrowDown);
        }
        assert_eq!(
            app.world().resource::<SettingsCursorRow>().0,
            ROW_LABELS.len() - 1
        );

        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.release(KeyCode::ArrowDown);
        kb.press(KeyCode::ArrowRight);
        app.update();
        assert!((app.world().resource::<Settings>().volume - 1.0).abs() < f32::EPSILON);
    }
}
