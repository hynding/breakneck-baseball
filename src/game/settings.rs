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

/// Which look the pitch trail wears (consumed by `fx.rs`'s trail systems):
/// the classic fading path, or one of five interchangeable 3D styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PitchTrailStyle {
    /// The baseline: a fading path of motes tracing the pitch.
    #[default]
    Comet,
    /// Flickering flame cones drifting up off the seam.
    Fireball,
    /// Spinning ice shards falling away behind the ball.
    Frostbite,
    /// Glowing hoops the ball threads, expanding as they fade.
    NeonRings,
    /// Twinkling star motes that hang and shimmer.
    Stardust,
    /// Wobbling bubbles that rise, swell, and pop.
    Bubbles,
}

impl PitchTrailStyle {
    pub fn label(self) -> &'static str {
        match self {
            PitchTrailStyle::Comet => "Comet (fading path)",
            PitchTrailStyle::Fireball => "Fireball",
            PitchTrailStyle::Frostbite => "Frostbite",
            PitchTrailStyle::NeonRings => "Neon rings",
            PitchTrailStyle::Stardust => "Stardust",
            PitchTrailStyle::Bubbles => "Bubble stream",
        }
    }

    pub fn next(self) -> Self {
        match self {
            PitchTrailStyle::Comet => PitchTrailStyle::Fireball,
            PitchTrailStyle::Fireball => PitchTrailStyle::Frostbite,
            PitchTrailStyle::Frostbite => PitchTrailStyle::NeonRings,
            PitchTrailStyle::NeonRings => PitchTrailStyle::Stardust,
            PitchTrailStyle::Stardust => PitchTrailStyle::Bubbles,
            PitchTrailStyle::Bubbles => PitchTrailStyle::Comet,
        }
    }

    pub fn prev(self) -> Self {
        // 5 nexts = 1 prev in a 6-cycle.
        self.next().next().next().next().next()
    }
}

/// The adjustable trail colour — a named preset palette (cycled on the
/// settings screen) applied to the fading path and tinting every 3D style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrailColor {
    #[default]
    Ember,
    Gold,
    Venom,
    Ice,
    Royal,
    Rose,
    Frost,
}

impl TrailColor {
    pub fn label(self) -> &'static str {
        match self {
            TrailColor::Ember => "Ember",
            TrailColor::Gold => "Gold",
            TrailColor::Venom => "Venom",
            TrailColor::Ice => "Ice",
            TrailColor::Royal => "Royal",
            TrailColor::Rose => "Rose",
            TrailColor::Frost => "Frost",
        }
    }

    /// The preset's base colour (alpha is the trail systems' business).
    pub fn color(self) -> Color {
        match self {
            TrailColor::Ember => Color::srgb(1.0, 0.45, 0.15),
            TrailColor::Gold => Color::srgb(1.0, 0.85, 0.30),
            TrailColor::Venom => Color::srgb(0.45, 1.0, 0.35),
            TrailColor::Ice => Color::srgb(0.40, 0.85, 1.0),
            TrailColor::Royal => Color::srgb(0.65, 0.45, 1.0),
            TrailColor::Rose => Color::srgb(1.0, 0.45, 0.75),
            TrailColor::Frost => Color::srgb(0.95, 0.97, 1.0),
        }
    }

    pub fn next(self) -> Self {
        match self {
            TrailColor::Ember => TrailColor::Gold,
            TrailColor::Gold => TrailColor::Venom,
            TrailColor::Venom => TrailColor::Ice,
            TrailColor::Ice => TrailColor::Royal,
            TrailColor::Royal => TrailColor::Rose,
            TrailColor::Rose => TrailColor::Frost,
            TrailColor::Frost => TrailColor::Ember,
        }
    }

    pub fn prev(self) -> Self {
        // 6 nexts = 1 prev in a 7-cycle.
        self.next().next().next().next().next().next()
    }
}

/// Everything the player can configure. Persisted on every change.
#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Batting style per player slot (index 0 = P1, 1 = P2).
    pub batting_style: [BattingStyle; 2],
    /// The pitch trail's look — serde-defaulted so stores written before
    /// trails existed still load instead of resetting every option.
    #[serde(default)]
    pub pitch_trail: PitchTrailStyle,
    /// The trail's colour preset (same back-compat default).
    #[serde(default)]
    pub trail_color: TrailColor,
    /// Whether the floating strike-zone wireframe is drawn during the duel.
    /// Toggled from the pause board (**Z**) so either player can switch it
    /// mid-game; defaults on, serde-defaulted for old stores.
    #[serde(default = "default_true")]
    pub show_strike_zone: bool,
    /// Master volume, 0.0..=1.0, applied via [`bevy::audio::GlobalVolume`].
    pub volume: f32,
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            batting_style: [BattingStyle::ClassicTiming; 2],
            pitch_trail: PitchTrailStyle::default(),
            trail_color: TrailColor::default(),
            show_strike_zone: true,
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

/// Label text for each row — tinted to show the cursor row.
#[derive(Component)]
struct SettingsRowLabel(usize);

/// Value text for each row (styles, trail, colour, volume).
#[derive(Component)]
struct SettingsRowText(usize);

/// Which row the cursor is on (0..ROW_LABELS.len()).
#[derive(Resource, Default)]
struct SettingsCursorRow(usize);

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
            2 => settings.pitch_trail.label().to_string(),
            3 => settings.trail_color.label().to_string(),
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

/// **S** / gamepad **Select** toggles the screen open/closed; **Esc** /
/// gamepad **East** (B) closes it while open. MainMenu only. `Select` is
/// also `camera.rs`'s duel-view toggle, and `East` is `menu.rs`'s
/// innings-cycle key — no clash: those systems run only in `Playing`
/// (`Select`) or are gated `.run_if(settings_closed)` (`East`), so they
/// never fire alongside this system's `MainMenu`-only, open-state-gated
/// handling of the same buttons.
fn toggle_settings(
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
fn edit_settings(
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
    fn trail_style_and_color_cycle_and_wrap() {
        let mut s = PitchTrailStyle::Comet;
        for _ in 0..6 {
            s = s.next();
        }
        assert_eq!(s, PitchTrailStyle::Comet);
        assert_eq!(PitchTrailStyle::Comet.prev(), PitchTrailStyle::Bubbles);
        let mut c = TrailColor::Ember;
        for _ in 0..7 {
            c = c.next();
        }
        assert_eq!(c, TrailColor::Ember);
        assert_eq!(TrailColor::Ember.prev(), TrailColor::Frost);
    }

    /// A pre-trail settings store (no trail fields) must still load — the
    /// new fields are serde-defaulted, not a breaking schema change that
    /// would silently reset every player's existing choices.
    #[test]
    fn legacy_store_without_trail_fields_loads_with_defaults() {
        let legacy = r#"{"batting_style":["SwingMeter","ClassicTiming"],"volume":0.5}"#;
        let s: Settings = serde_json::from_str(legacy).unwrap();
        assert_eq!(s.pitch_trail, PitchTrailStyle::Comet);
        assert_eq!(s.trail_color, TrailColor::Ember);
        assert!(s.show_strike_zone, "zone overlay defaults on");
        assert_eq!(s.batting_style[0], BattingStyle::SwingMeter);
        assert!((s.volume - 0.5).abs() < 1e-6);
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
