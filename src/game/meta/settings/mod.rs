//! Player-facing options: the persistent [`Settings`] resource, its storage
//! seam, and the settings screen (`screen.rs`). Batting styles are stored
//! here and consumed by the batting input adapters (spec §3); until those
//! land the values are inert data.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
use bevy::log::warn;

use crate::game::GameState;

mod screen;

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

/// Which look the pitch trail wears (consumed by `game::fx`'s trail systems):
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
            .init_resource::<screen::SettingsCursorRow>()
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
            .add_systems(Startup, screen::spawn_settings_screen)
            .add_systems(
                Update,
                (
                    screen::toggle_settings,
                    screen::edit_settings,
                    screen::paint_settings_screen,
                )
                    .chain()
                    .run_if(in_state(GameState::MainMenu)),
            )
            // Defensive reset: `menu_select` is suppressed while the screen
            // is open (see `settings_closed`), so `MainMenu` can't normally
            // be left with the screen up — kept as a safety net regardless.
            .add_systems(OnExit(GameState::MainMenu), screen::close_settings_on_exit);
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

/// Menu hotkeys stand down while the settings screen is up — registered as
/// `.run_if(settings_closed)` on `cycle_options` and `menu_select` in
/// `menu.rs` so **S** doesn't fight T/F/I or the mode-select keys.
pub fn settings_closed(open: Res<SettingsOpen>) -> bool {
    !open.0
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
        // SAFETY: `ENV_LOCK` (held by `_guard` for this whole test) serializes
        // every test in this module that reads or writes
        // `BREAKNECK_SETTINGS_PATH`, and `store_path()` — the only reader —
        // is only ever called from inside that same critical section, so no
        // other thread can observe the environment mid-mutation.
        unsafe { std::env::set_var("BREAKNECK_SETTINGS_PATH", &path) };
        let mut s = Settings::default();
        s.batting_style[1] = BattingStyle::PciCursor;
        s.volume = 0.4;
        save_settings(&s);
        assert_eq!(load_settings(), s);
        // SAFETY: still under `ENV_LOCK` via `_guard`; see the set_var above.
        unsafe { std::env::remove_var("BREAKNECK_SETTINGS_PATH") };
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_store_falls_back_to_default() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = std::env::temp_dir().join(format!("bb-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, b"{ not json").unwrap();
        // SAFETY: `ENV_LOCK` (held by `_guard` for this whole test) serializes
        // every test in this module that reads or writes
        // `BREAKNECK_SETTINGS_PATH`, and `store_path()` — the only reader —
        // is only ever called from inside that same critical section, so no
        // other thread can observe the environment mid-mutation.
        unsafe { std::env::set_var("BREAKNECK_SETTINGS_PATH", &path) };
        assert_eq!(load_settings(), Settings::default());
        // SAFETY: still under `ENV_LOCK` via `_guard`; see the set_var above.
        unsafe { std::env::remove_var("BREAKNECK_SETTINGS_PATH") };
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn style_cycles_through_all_three_and_wraps() {
        let s = BattingStyle::ClassicTiming;
        assert_eq!(s.next(), BattingStyle::SwingMeter);
        assert_eq!(s.next().next(), BattingStyle::PciCursor);
        assert_eq!(s.next().next().next(), BattingStyle::ClassicTiming);
        assert_eq!(s.prev(), BattingStyle::PciCursor);
        assert!(
            BattingStyle::PciCursor
                .label()
                .contains("gamepad recommended")
        );
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
        // SAFETY: `ENV_LOCK` (held by `_guard` for this whole test) serializes
        // every test in this module that reads or writes
        // `BREAKNECK_SETTINGS_PATH`, and `store_path()` — the only reader —
        // is only ever called from inside that same critical section, so no
        // other thread can observe the environment mid-mutation.
        unsafe { std::env::set_var("BREAKNECK_SETTINGS_PATH", &path) };

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

        // SAFETY: still under `ENV_LOCK` via `_guard`; see the set_var above.
        unsafe { std::env::remove_var("BREAKNECK_SETTINGS_PATH") };
        let _ = std::fs::remove_dir_all(dir);
    }
}
