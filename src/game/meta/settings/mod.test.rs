//! Unit tests for [`super`] — the settings module.

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
    // The unparseable blob is preserved for recovery, not just dropped.
    let bak = std::fs::read(path.with_extension("json.bak")).unwrap();
    assert_eq!(bak, b"{ not json");
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
    assert!(!s.reduce_motion, "full motion defaults on");
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
    let on_disk: Settings = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert!((on_disk.volume - 0.25).abs() < 1e-5);

    // SAFETY: still under `ENV_LOCK` via `_guard`; see the set_var above.
    unsafe { std::env::remove_var("BREAKNECK_SETTINGS_PATH") };
    let _ = std::fs::remove_dir_all(dir);
}
