# Settings Infrastructure (Batting-Feel Plan A) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A persistent, per-player settings system (batting styles + master volume) with a keyboard/gamepad settings screen opened with **S** from the main menu.

**Architecture:** New `src/game/settings.rs` owns the `Settings` resource, a dual-target persistence seam (native JSON file / wasm localStorage), the settings-screen UI (painted at startup, shown by child mutation per the wasm UI rule), and volume application via Bevy's `GlobalVolume`. `menu.rs` gains the S toggle and suppresses its own hotkeys while the screen is open. `BattingStyle` ships here as stored data; Plan B consumes it.

**Tech Stack:** Bevy 0.15, serde + serde_json (both targets), `dirs` (native-only), `web-sys` with `Window`+`Storage` features (wasm-only).

**Spec:** docs/superpowers/specs/2026-07-30-batting-feel-design.md §1 (+ §8 Plan A scope)

## Global Constraints

- Prefix every cargo command: `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`
- Verify BOTH targets after changes: `cargo check` and `cargo check --target wasm32-unknown-unknown`; keep `Cargo.lock` committed
- wasm UI rule (CLAUDE.md): roots painted at spawn with nonzero alpha; show/hide only by mutating children; no UI roots spawned mid-state
- Settings rows at launch exactly: **P1 Batting Style**, **P2 Batting Style**, **Volume**; PCI row value renders as `PCI cursor (gamepad recommended)`
- Persistence: native → JSON at platform config dir, overridable via env `BREAKNECK_SETTINGS_PATH` (the test seam); wasm → `localStorage` key `breakneck-baseball.settings`
- Save on every change; load-or-default at startup; a corrupt/missing file must never panic (fall back to `Settings::default()`)
- Volume clamped to `0.0..=1.0`, default `0.7`, applied via `bevy::audio::GlobalVolume`

---

### Task A1: `Settings` data model + serde round-trip

**Files:**
- Modify: `Cargo.toml` (add `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"` to `[dependencies]`)
- Create: `src/game/settings.rs` (data model only)
- Modify: `src/game/mod.rs` (add `pub mod settings;` beside the other modules)

**Interfaces:**
- Consumes: nothing.
- Produces (later tasks + Plan B rely on these exact names):
  - `pub enum BattingStyle { ClassicTiming, SwingMeter, PciCursor }` with `pub fn label(self) -> &'static str` ("Classic timing" / "Swing meter" / "PCI cursor (gamepad recommended)"), `pub fn next(self)`, `pub fn prev(self)`
  - `pub struct Settings { pub batting_style: [BattingStyle; 2], pub volume: f32 }` (`Resource`, `Serialize`, `Deserialize`, `Clone`, `PartialEq`), `impl Default` → `[ClassicTiming; 2], volume 0.7`
  - `impl Settings { pub fn clamped(self) -> Self }` — volume clamped to `0.0..=1.0`

- [ ] **Step 1: Write the failing unit tests** (in `src/game/settings.rs`'s `#[cfg(test)] mod tests`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_cycles_through_all_three_and_wraps() {
        let s = BattingStyle::ClassicTiming;
        assert_eq!(s.next(), BattingStyle::SwingMeter);
        assert_eq!(s.next().next(), BattingStyle::PciCursor);
        assert_eq!(s.next().next().next(), BattingStyle::ClassicTiming);
        assert_eq!(s.prev(), BattingStyle::PciCursor);
        assert!(BattingStyle::PciCursor.label().contains("gamepad recommended"));
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
        let mut s = Settings::default();
        s.volume = 1.7;
        assert!((s.clamped().volume - 1.0).abs() < f32::EPSILON);
        s.volume = -0.3;
        assert!(s.clamped().volume.abs() < f32::EPSILON);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib settings`
Expected: compile FAIL (module/type not found).

- [ ] **Step 3: Implement the model**

```rust
//! Player-facing options: the persistent [`Settings`] resource, its storage
//! seam, and the settings screen. Batting styles are stored here and
//! consumed by the batting input adapters (spec §3); until those land the
//! values are inert data.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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
    pub fn clamped(mut self) -> Self {
        self.volume = self.volume.clamp(0.0, 1.0);
        self
    }
}
```

Add `pub mod settings;` to `src/game/mod.rs`. Add the two deps to `Cargo.toml` `[dependencies]`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib settings`
Expected: 3 tests PASS.

- [ ] **Step 5: Both targets compile**

Run: `cargo check && cargo check --target wasm32-unknown-unknown`
Expected: clean (serde/serde_json build on both).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/game/settings.rs src/game/mod.rs
git commit -m "feat: Settings data model with per-player batting styles and volume"
```

---

### Task A2: Persistence seam (native file / wasm localStorage)

**Files:**
- Modify: `Cargo.toml` (`[target.'cfg(not(target_arch = "wasm32"))'.dependencies] dirs = "5"`; add `web-sys = { version = "0.3", features = ["Window", "Storage"] }` to the existing wasm-only section)
- Modify: `src/game/settings.rs`
- Test: same file's test module (native paths only; wasm covered by `cargo check --target`)

**Interfaces:**
- Consumes: `Settings` (A1).
- Produces:
  - `pub fn load_settings() -> Settings` — reads the store, falls back to default on any error, always returns `.clamped()`
  - `pub fn save_settings(s: &Settings)` — best-effort write; errors are logged (`warn!`), never panic
  - Native store path: `$BREAKNECK_SETTINGS_PATH` if set, else `<config_dir>/breakneck-baseball/settings.json`; wasm store: `localStorage["breakneck-baseball.settings"]`

- [ ] **Step 1: Write the failing tests** (append to the test module)

```rust
    #[test]
    fn save_load_round_trips_through_env_path() {
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
        let dir = std::env::temp_dir().join(format!("bb-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, b"{ not json").unwrap();
        std::env::set_var("BREAKNECK_SETTINGS_PATH", &path);
        assert_eq!(load_settings(), Settings::default());
        std::env::remove_var("BREAKNECK_SETTINGS_PATH");
        let _ = std::fs::remove_dir_all(dir);
    }
```

Note: these two tests mutate the same process-wide env var; Rust runs tests in threads. Guard with a shared lock so they serialize:

```rust
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
```

Take `let _guard = ENV_LOCK.lock().unwrap();` as the first line of BOTH tests.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib settings`
Expected: compile FAIL (`load_settings` unknown).

- [ ] **Step 3: Implement the seam**

```rust
/// localStorage key / file name shared by both stores.
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
```

(Native `read_store` doesn't use `STORE_KEY` in its path — the file name in `store_path` stays `settings.json`; `STORE_KEY` is the wasm key. Keep the const beside both uses with this comment.)

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib settings`
Expected: 5 tests PASS.

- [ ] **Step 5: Both targets compile**

Run: `cargo check && cargo check --target wasm32-unknown-unknown`
Expected: clean — this validates the web-sys feature set actually covers `window()`/`local_storage()`.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/game/settings.rs
git commit -m "feat: dual-target settings persistence (config file / localStorage)"
```

---

### Task A3: `SettingsPlugin` — load at startup, volume application, save-on-change

**Files:**
- Modify: `src/game/settings.rs` (plugin + systems)
- Modify: `src/game/mod.rs` (register `SettingsPlugin` in `GamePlugin`, before `MenuPlugin` so the menu can read the resource; note `GamePlugin` already splits `.add_plugins` calls because of Bevy's 15-tuple limit — add this one to the trailing group)

**Interfaces:**
- Consumes: A1/A2 items.
- Produces:
  - `pub struct SettingsPlugin` — inserts `Settings` via `load_settings()` at build time, plus systems:
    - `apply_volume` — on `Settings` change (and once at startup), writes `GlobalVolume::new(settings.volume)`
    - `persist_settings` — on `Settings` change, calls `save_settings`
  - `pub struct SettingsOpen(pub bool)` resource (default false) — the settings screen's visibility flag; menu key suppression (A4) and tests read it

- [ ] **Step 1: Write the failing test** (append to the module's tests; a minimal App, no windowing needed for resource logic)

```rust
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
```

Note: `GlobalVolume`'s field is `volume: bevy::audio::Volume`; `Volume::get()` returns the f32. If the 0.15 API differs (e.g. tuple newtype), adapt the assertions — the invariant under test is "GlobalVolume mirrors Settings.volume", not the accessor spelling.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib settings plugin_loads`
Expected: compile FAIL (`SettingsPlugin` unknown).

- [ ] **Step 3: Implement plugin + systems**

```rust
/// Whether the settings screen is currently shown (toggled with **S** on
/// the main menu). A resource so menu systems can suppress their own
/// hotkeys while the screen is open.
#[derive(Resource, Default)]
pub struct SettingsOpen(pub bool);

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(load_settings())
            .init_resource::<SettingsOpen>()
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
```

(`GlobalVolume` is registered by Bevy's `AudioPlugin`; under `MinimalPlugins` in the test it's absent — `init_resource::<bevy::audio::GlobalVolume>()` in the plugin `build` makes the plugin self-sufficient and harmless alongside the real `AudioPlugin`. Add that line.)

Register `SettingsPlugin` in `src/game/mod.rs`'s `GamePlugin` (trailing `.add_plugins` group, before `MenuPlugin`'s tuple if ordering allows — the menu only reads the resource at runtime, so exact position within the same schedule is not load-bearing; keep it out of the full 15-tuple).

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --lib settings`
Expected: 6 tests PASS.

- [ ] **Step 5: Full suite + both targets**

Run: `cargo test && cargo check --target wasm32-unknown-unknown`
Expected: green (the new plugin must not disturb any e2e).

- [ ] **Step 6: Commit**

```bash
git add src/game/settings.rs src/game/mod.rs
git commit -m "feat: SettingsPlugin loads, applies volume, persists on change"
```

---

### Task A4: Settings screen UI + S toggle + menu-key suppression

**Files:**
- Modify: `src/game/settings.rs` (UI spawn + input systems)
- Modify: `src/game/menu.rs` (footer hint line gains `S  Settings`; `cycle_options` and `menu_select` gain `.run_if(settings_closed)`)

**Interfaces:**
- Consumes: `Settings`, `SettingsOpen`, `BattingStyle` (A1-A3); `Theme`/`UiTheme` (existing); `GameState::MainMenu`.
- Produces:
  - `pub fn settings_closed(open: Res<SettingsOpen>) -> bool` — run-condition for menu systems
  - Component `SettingsUi` (screen root, painted at startup), `SettingsRowText(usize)` (row value text), `SettingsCursor` marker rows
  - Row order (fixed): 0 = P1 Batting Style, 1 = P2 Batting Style, 2 = Volume
  - Controls: **S** toggles (MainMenu only), Up/Down move the row cursor, Left/Right cycle/adjust (volume ±0.1 clamped), **Esc** closes

- [ ] **Step 1: Spawn the screen (painted at startup, hidden by content)**

Follow the `subs.rs` board idiom exactly (spawned once, `hidden_tint` when closed, real colors when open — never alpha-0, never spawned mid-state). Structure:

```rust
/// Root of the settings screen (full-screen centered column overlay).
#[derive(Component)]
struct SettingsUi;

/// Value text for row `0..=2` (styles, styles, volume).
#[derive(Component)]
struct SettingsRowText(usize);

/// Label text for row `0..=2` — tinted to show the cursor row.
#[derive(Component)]
struct SettingsRowLabel(usize);

/// Which row the cursor is on (0..=2).
#[derive(Resource, Default)]
struct SettingsCursorRow(usize);

const ROW_LABELS: [&str; 3] = ["P1 BATTING STYLE", "P2 BATTING STYLE", "VOLUME"];

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
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(10.0),
                ..default()
            },
            // Painted at spawn per the wasm UI rule; content carries the
            // visible pixels, the root stays a barely-tinted catcher.
            BackgroundColor(crate::game::ui::hidden_tint()),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(""),
                TextFont { font_size: 34.0, ..default() },
                TextColor(ui.accent),
                SettingsTitle,
            ));
            for i in 0..3 {
                root.spawn((
                    Text::new(""),
                    TextFont { font_size: 20.0, ..default() },
                    TextColor(ui.text_primary),
                    SettingsRowLabel(i),
                ));
                root.spawn((
                    Text::new(""),
                    TextFont { font_size: 18.0, ..default() },
                    TextColor(ui.text_dim),
                    SettingsRowText(i),
                ));
            }
        });
}

#[derive(Component)]
struct SettingsTitle;
```

Check `ui::hidden_tint`'s actual visibility (`pub` vs `pub(crate)`) — widen to `pub(crate)` if needed. Register `spawn_settings_screen` at `Startup` (the menu exists from startup; MainMenu is the initial state).

- [ ] **Step 2: The paint/update system**

```rust
/// Paints the screen every frame from state: blank/hidden when closed,
/// full content when open (child mutation only — wasm rule).
fn paint_settings_screen(
    open: Res<SettingsOpen>,
    cursor: Res<SettingsCursorRow>,
    settings: Res<Settings>,
    theme: Res<Theme>,
    mut title: Query<(&mut Text, &mut TextColor), (With<SettingsTitle>, Without<SettingsRowLabel>, Without<SettingsRowText>)>,
    mut labels: Query<(&SettingsRowLabel, &mut Text, &mut TextColor), (Without<SettingsTitle>, Without<SettingsRowText>)>,
    mut values: Query<(&SettingsRowText, &mut Text), (Without<SettingsTitle>, Without<SettingsRowLabel>)>,
) {
    let ui = &theme.ui;
    let (mut title_text, mut title_color) = title.single_mut();
    if !open.0 {
        *title_text = Text::new("");
        for (_, mut t, _) in &mut labels {
            *t = Text::new("");
        }
        for (_, mut t) in &mut values {
            *t = Text::new("");
        }
        return;
    }
    *title_text = Text::new("SETTINGS");
    title_color.0 = ui.accent;
    for (label, mut text, mut color) in &mut labels {
        let marker = if cursor.0 == label.0 { "> " } else { "  " };
        *text = Text::new(format!("{marker}{}", ROW_LABELS[label.0]));
        color.0 = if cursor.0 == label.0 { ui.accent } else { ui.text_primary };
    }
    for (row, mut text) in &mut values {
        *text = Text::new(match row.0 {
            0 => settings.batting_style[0].label().to_string(),
            1 => settings.batting_style[1].label().to_string(),
            _ => format!("{:.0}%", settings.volume * 100.0),
        });
    }
}
```

- [ ] **Step 3: Input systems + run-condition**

```rust
/// Menu hotkeys stand down while the settings screen is up.
pub fn settings_closed(open: Res<SettingsOpen>) -> bool {
    !open.0
}

/// S toggles the screen; Esc closes it. MainMenu only.
fn toggle_settings(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut open: ResMut<SettingsOpen>,
) {
    if keyboard.just_pressed(KeyCode::KeyS) {
        open.0 = !open.0;
    } else if open.0 && keyboard.just_pressed(KeyCode::Escape) {
        open.0 = false;
    }
}

/// Up/Down move the cursor; Left/Right edit the focused row.
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
```

Register in `SettingsPlugin`:

```rust
            .init_resource::<SettingsCursorRow>()
            .add_systems(Startup, spawn_settings_screen)
            .add_systems(
                Update,
                (toggle_settings, edit_settings, paint_settings_screen)
                    .chain()
                    .run_if(in_state(crate::game::GameState::MainMenu)),
            )
```

(Also: when leaving MainMenu the paint system stops running — add an `OnExit(GameState::MainMenu)` system that sets `SettingsOpen(false)` and blanks via one extra `paint` run, or simply set `open.0 = false` in an `OnExit` closure and let the same frame's paint clear it. Implement the `OnExit` reset.)

In `src/game/menu.rs`:
- add `.run_if(crate::game::settings::settings_closed)` to `cycle_options` and `menu_select`'s registrations (S must not fight T/F/I or start-keys while editing);
- find the footer/controls hint text in `spawn_menu` and add an `S  Settings` line matching its formatting.

- [ ] **Step 4: Manual-logic check via unit-style app test** (append to settings tests)

```rust
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
```

Run: `cargo test --lib settings`
Expected: PASS (adapt `ButtonInput` mutation to the harness idiom if clear_just_pressed misbehaves — pressing then `app.update()` twice also works).

- [ ] **Step 5: Full suite + both targets**

Run: `cargo test && cargo check --target wasm32-unknown-unknown && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/game/settings.rs src/game/menu.rs
git commit -m "feat: settings screen with S toggle, row editing, and menu-key suppression"
```

---

### Task A5: Staged e2e — open, edit, persist, close, play

**Files:**
- Create: `tests/e2e_settings.rs`

**Interfaces:**
- Consumes: everything above; harness `common::{headless_app, run_until, tap_key, start_game}` (note `tap_key` injects via the DriveGame schedule — required, since `PreUpdate` clears direct presses).

- [ ] **Step 1: Write the test**

```rust
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
    assert!(app.world().resource::<SettingsOpen>().0, "S must open settings");

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
    let on_disk: Settings =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
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
```

(Check `common::start_game`'s select-key convention in the harness — existing tests use `Digit1`/`Digit2`.)

- [ ] **Step 2: Run to verify it fails only if wiring is wrong, then passes**

Run: `cargo test --test e2e_settings`
Expected: PASS if A4 is correct; if it fails, the failure names the broken hop (open flag, resource edit, disk write, or start).

- [ ] **Step 3: Full suite**

Run: `cargo test`
Expected: all green — especially `e2e_pause_subs` (S is also a jersey letter — confirm no key collisions on the menu: T/F/I/S are distinct) and the four gates.

- [ ] **Step 4: Commit**

```bash
git add tests/e2e_settings.rs
git commit -m "test: settings e2e — open, edit, persist, close, play"
```

---

### Task A6: Docs + final sweep

**Files:**
- Modify: `CLAUDE.md` (menu paragraph: add S/settings + persistence note; one sentence in the architecture section naming `settings.rs` and the store locations)
- Modify: `TADA.md` if the user's TODO listed settings (it did not — skip unless present)

- [ ] **Step 1: Update CLAUDE.md**

In the menu-keys sentence (currently documents T theme cycling and I innings), add: **S** opens the settings screen (per-player batting styles — consumed by the batting adapters when they land — and master volume), persisted natively to the platform config dir (`BREAKNECK_SETTINGS_PATH` overrides; the test seam) and to `localStorage` on wasm via `settings.rs`.

- [ ] **Step 2: Final gate**

Run: `cargo test && cargo check --target wasm32-unknown-unknown && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all clean.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: settings screen and persistence recorded"
```

---

## Out of scope (later plans)

Plan B consumes `Settings::batting_style` (spine + Classic + juice + CPU dial + balance harness); Plan C adds the SwingMeter/PciCursor adapters. Nothing in this plan may reference contact quality, ContactEvent, or adapters.
