//! Player-facing options: the persistent [`Settings`] resource, its storage
//! seam, and the settings screen. Batting styles are stored here and
//! consumed by the batting input adapters (spec §3); until those land the
//! values are inert data.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
use bevy::log::warn;

use bevy::color::Alpha;

use crate::game::theme::Theme;
use crate::game::ui::hidden_tint;
use crate::game::GameState;

/// Which batting input front-end a player uses (spec §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BattingStyle {
    ClassicTiming,
    SwingMeter,
    PciCursor,
}

impl BattingStyle {
    /// Settings-row value text. PCI carries its input recommendation.
    pub fn label(self) -> &'static str {
        match self {
            BattingStyle::ClassicTiming => "Classic timing",
            BattingStyle::SwingMeter => "Swing meter",
            BattingStyle::PciCursor => "PCI cursor (gamepad recommended)",
        }
    }

    pub fn next(self) -> Self {
        match self {
            BattingStyle::ClassicTiming => BattingStyle::SwingMeter,
            BattingStyle::SwingMeter => BattingStyle::PciCursor,
            BattingStyle::PciCursor => BattingStyle::ClassicTiming,
        }
    }

    pub fn prev(self) -> Self {
        self.next().next()
    }
}

/// Everything the player can configure. Persisted on every change.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Batting style per player slot (index 0 = P1, 1 = P2).
    pub batting_style: [BattingStyle; 2],
    /// Master volume, 0.0..=1.0, applied via [`bevy::audio::GlobalVolume`].
    pub volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            batting_style: [BattingStyle::ClassicTiming; 2],
            volume: 0.7,
        }
    }
}

impl Settings {
    /// Returns a copy with every field forced into its legal range —
    /// applied after deserializing untrusted stored data.
    pub fn clamped(&self) -> Self {
        let mut result = self.clone();
        result.volume = result.volume.clamp(0.0, 1.0);
        result
    }
}

/// localStorage key / file name shared by both stores.
#[allow(dead_code)]
const STORE_KEY: &str = "breakneck-baseball.settings";

#[cfg(not(target_arch = "wasm32"))]
fn store_path() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("BREAKNECK_SETTINGS_PATH") {
        return Some(p.into());
    }
    dirs::config_dir().map(|d| d.join("breakneck-baseball").join("settings.json"))
}

/// Reads persisted settings; any failure (missing, unreadable, corrupt)
/// falls back to defaults so a bad store can never brick startup.
pub fn load_settings() -> Settings {
    read_store()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default()
        .clamped()
}

/// Best-effort persist; failures are logged, never fatal (a read-only FS or
/// blocked localStorage just means options reset next launch).
pub fn save_settings(s: &Settings) {
    let Ok(text) = serde_json::to_string_pretty(s) else {
        return;
    };
    if let Err(e) = write_store(&text) {
        warn!("settings not saved: {e}");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_store() -> Option<String> {
    std::fs::read_to_string(store_path()?).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn write_store(text: &str) -> Result<(), String> {
    let path = store_path().ok_or("no config directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

#[cfg(target_arch = "wasm32")]
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

#[cfg(target_arch = "wasm32")]
fn read_store() -> Option<String> {
    local_storage()?.get_item(STORE_KEY).ok().flatten()
}

#[cfg(target_arch = "wasm32")]
fn write_store(text: &str) -> Result<(), String> {
    local_storage()
        .ok_or("no localStorage")?
        .set_item(STORE_KEY, text)
        .map_err(|_| "localStorage set_item failed".into())
}

/// Whether the settings screen is currently shown (toggled with **S** on
/// the main menu). A resource so menu systems can suppress their own
/// hotkeys while the screen is open.
#[derive(Resource, Default)]
pub struct SettingsOpen(pub bool);

/// Loads persisted settings at startup, mirrors [`Settings::volume`] into
/// the audio mixer, and persists every change back to the store.
pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_settings())
            .init_resource::<SettingsOpen>()
            .init_resource::<SettingsCursorRow>()
            // `MinimalPlugins` (used in tests) doesn't register `AudioPlugin`,
            // so `GlobalVolume` wouldn't otherwise exist; initializing it here
            // makes the plugin self-sufficient and harmless alongside the
            // real `AudioPlugin`, which also inserts it (default 1.0) before
            // `apply_volume` ever runs.
            .init_resource::<bevy::audio::GlobalVolume>()
            .add_systems(Update, (apply_volume, persist_settings))
            // The menu exists from startup and `MainMenu` is the initial
            // state, so the screen can be painted once here per the wasm UI
            // rule (see `subs.rs`) and shown/hidden by mutating children —
            // never despawned or respawned mid-session.
            .add_systems(Startup, spawn_settings_screen)
            .add_systems(
                Update,
                (toggle_settings, edit_settings, paint_settings_screen)
                    .chain()
                    .run_if(in_state(GameState::MainMenu)),
            )
            // Defensive reset: `menu_select` is suppressed while the screen
            // is open (see `settings_closed`), so `MainMenu` can't normally
            // be left with the screen up — kept as a safety net regardless.
            .add_systems(OnExit(GameState::MainMenu), close_settings_on_exit);
    }
}

/// Mirrors [`Settings::volume`] into the audio mixer. `is_changed` is true
/// on insertion, so the loaded value applies on the first frame too.
fn apply_volume(settings: Res<Settings>, mut volume: ResMut<bevy::audio::GlobalVolume>) {
    if settings.is_changed() {
        *volume = bevy::audio::GlobalVolume::new(settings.volume.clamp(0.0, 1.0));
    }
}

/// Writes every change to the store — cheap (tiny JSON) and forgetting to
/// save is worse than the extra writes.
fn persist_settings(settings: Res<Settings>) {
    if settings.is_changed() && !settings.is_added() {
        save_settings(&settings);
    }
}

// ── Settings screen ──────────────────────────────────────────────────────────

/// Root of the settings screen (full-screen centered column overlay).
#[derive(Component)]
struct SettingsUi;

/// The screen's inner card — the opaque panel that occludes the menu behind
/// it while open (mirrors `subs.rs`'s `SubsUi`/`SubsCard` split: the menu
/// isn't a 3D backdrop like gameplay, so the overlay needs its own solid
/// panel rather than relying on world geometry to read as "in front").
#[derive(Component)]
struct SettingsCard;

/// The screen's title line ("SETTINGS" while open).
#[derive(Component)]
struct SettingsTitle;

/// Label text for row `0..=2` — tinted to show the cursor row.
#[derive(Component)]
struct SettingsRowLabel(usize);

/// Value text for row `0..=2` (styles, styles, volume).
#[derive(Component)]
struct SettingsRowText(usize);

/// Which row the cursor is on (0..=2).
#[derive(Resource, Default)]
struct SettingsCursorRow(usize);

const ROW_LABELS: [&str; 3] = ["P1 BATTING STYLE", "P2 BATTING STYLE", "VOLUME"];

/// Builds the settings screen once at startup, painted behind
/// [`hidden_tint`] per the wasm UI rule (see `subs.rs`): spawned once, shown
/// and hidden only by mutating the children of this root — never despawned
/// or respawned mid-session.
fn spawn_settings_screen(mut commands: Commands, theme: Res<Theme>) {
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
                for i in 0..3 {
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
fn paint_settings_screen(
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
            _ => format!("{:.0}%", settings.volume * 100.0),
        };
    }
}

/// Menu hotkeys stand down while the settings screen is up — registered as
/// `.run_if(settings_closed)` on `cycle_options` and `menu_select` in
/// `menu.rs` so **S** doesn't fight T/F/I or the mode-select keys.
pub fn settings_closed(open: Res<SettingsOpen>) -> bool {
    !open.0
}

/// **S** toggles the screen open/closed; **Esc** closes it. MainMenu only.
fn toggle_settings(keyboard: Res<ButtonInput<KeyCode>>, mut open: ResMut<SettingsOpen>) {
    if keyboard.just_pressed(KeyCode::KeyS) {
        open.0 = !open.0;
    } else if open.0 && keyboard.just_pressed(KeyCode::Escape) {
        open.0 = false;
    }
}

/// Up/Down move the row cursor; Left/Right cycle the batting style or nudge
/// the volume (±0.1, clamped). No-op while the screen is closed.
fn edit_settings(
    keyboard: Res<ButtonInput<KeyCode>>,
    open: Res<SettingsOpen>,
    mut cursor: ResMut<SettingsCursorRow>,
    mut settings: ResMut<Settings>,
) {
    if !open.0 {
        return;
    }
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        cursor.0 = cursor.0.checked_sub(1).unwrap_or(2);
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        cursor.0 = (cursor.0 + 1) % 3;
    }
    let left = keyboard.just_pressed(KeyCode::ArrowLeft);
    let right = keyboard.just_pressed(KeyCode::ArrowRight);
    if !(left || right) {
        return;
    }
    match cursor.0 {
        0 | 1 => {
            let s = settings.batting_style[cursor.0];
            settings.batting_style[cursor.0] = if right { s.next() } else { s.prev() };
        }
        _ => {
            let dv = if right { 0.1 } else { -0.1 };
            settings.volume = (settings.volume + dv).clamp(0.0, 1.0);
        }
    }
}

/// Safety-net reset for leaving `MainMenu`: see the `OnExit` registration.
fn close_settings_on_exit(mut open: ResMut<SettingsOpen>, mut cursor: ResMut<SettingsCursorRow>) {
    open.0 = false;
    cursor.0 = 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn save_load_round_trips_through_env_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("bb-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        // Env var is the documented test seam for the native store.
        std::env::set_var("BREAKNECK_SETTINGS_PATH", &path);
        let mut s = Settings::default();
        s.batting_style[1] = BattingStyle::PciCursor;
        s.volume = 0.4;
        save_settings(&s);
        assert_eq!(load_settings(), s);
        std::env::remove_var("BREAKNECK_SETTINGS_PATH");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_store_falls_back_to_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("bb-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, b"{ not json").unwrap();
        std::env::set_var("BREAKNECK_SETTINGS_PATH", &path);
        assert_eq!(load_settings(), Settings::default());
        std::env::remove_var("BREAKNECK_SETTINGS_PATH");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn style_cycles_through_all_three_and_wraps() {
        let s = BattingStyle::ClassicTiming;
        assert_eq!(s.next(), BattingStyle::SwingMeter);
        assert_eq!(s.next().next(), BattingStyle::PciCursor);
        assert_eq!(s.next().next().next(), BattingStyle::ClassicTiming);
        assert_eq!(s.prev(), BattingStyle::PciCursor);
        assert!(BattingStyle::PciCursor
            .label()
            .contains("gamepad recommended"));
    }

    #[test]
    fn settings_round_trip_and_defaults() {
        let s = Settings::default();
        assert_eq!(s.batting_style, [BattingStyle::ClassicTiming; 2]);
        assert!((s.volume - 0.7).abs() < f32::EPSILON);
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn clamped_bounds_volume() {
        let mut s = Settings {
            volume: 1.7,
            ..Default::default()
        };
        assert!((s.clamped().volume - 1.0).abs() < f32::EPSILON);
        s.volume = -0.3;
        assert!(s.clamped().volume.abs() < f32::EPSILON);
    }

    #[test]
    fn plugin_loads_applies_and_persists() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("bb-plugin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::env::set_var("BREAKNECK_SETTINGS_PATH", &path);

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            // `spawn_settings_screen` (Startup) needs a `Theme` to build the
            // screen from; the real app always has one by the time Startup
            // runs (`GamePlugin` inserts it ahead of `SettingsPlugin`).
            .insert_resource(crate::game::theme::ThemeId::DaylightClassic.build())
            .add_plugins(SettingsPlugin);
        app.update();
        // Loaded default volume applied to GlobalVolume.
        let gv = app.world().resource::<bevy::audio::GlobalVolume>();
        assert!((gv.volume.get() - 0.7).abs() < 1e-5);

        // Mutate → persisted + volume follows.
        app.world_mut().resource_mut::<Settings>().volume = 0.25;
        app.update();
        let gv = app.world().resource::<bevy::audio::GlobalVolume>();
        assert!((gv.volume.get() - 0.25).abs() < 1e-5);
        let on_disk: Settings =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!((on_disk.volume - 0.25).abs() < 1e-5);

        std::env::remove_var("BREAKNECK_SETTINGS_PATH");
        let _ = std::fs::remove_dir_all(dir);
    }

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

        // Move to the volume row and push past the clamp. `press` only
        // re-marks `just_pressed` after a `release` — a held key doesn't
        // repeat — so each tap is release-then-press.
        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.release(KeyCode::ArrowRight);
        kb.press(KeyCode::ArrowDown);
        app.update();
        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.release(KeyCode::ArrowDown);
        kb.press(KeyCode::ArrowDown);
        app.update();
        assert_eq!(app.world().resource::<SettingsCursorRow>().0, 2);

        let mut kb = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        kb.release(KeyCode::ArrowDown);
        kb.press(KeyCode::ArrowRight);
        app.update();
        assert!((app.world().resource::<Settings>().volume - 1.0).abs() < f32::EPSILON);
    }
}
