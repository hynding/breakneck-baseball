//! Player-facing options: the persistent [`Settings`] resource, its storage
//! seam, and the settings screen. Batting styles are stored here and
//! consumed by the batting input adapters (spec §3); until those land the
//! values are inert data.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[cfg(not(target_arch = "wasm32"))]
use bevy::log::warn;

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
            // `MinimalPlugins` (used in tests) doesn't register `AudioPlugin`,
            // so `GlobalVolume` wouldn't otherwise exist; initializing it here
            // makes the plugin self-sufficient and harmless alongside the
            // real `AudioPlugin`, which also inserts it (default 1.0) before
            // `apply_volume` ever runs.
            .init_resource::<bevy::audio::GlobalVolume>()
            .add_systems(Update, (apply_volume, persist_settings));
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
        app.add_plugins(MinimalPlugins).add_plugins(SettingsPlugin);
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
}
