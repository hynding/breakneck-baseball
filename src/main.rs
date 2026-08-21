//! Breakneck Baseball — a 3-D baseball game built on Bevy (wgpu) + Rapier.
//!
//! This entry-point assembles all plugins and runs the application.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use breakneck_baseball::game::GamePlugin;

fn main() {
    // On the web a panic aborts the wasm instance and freezes the canvas
    // with nothing but a cryptic "unreachable" in the console. Surface it:
    // log the real panic message, and post it to the page so index.html can
    // swap the dead canvas for an honest "reload" card (TODO 12). Installed
    // before the App builds so even a plugin-construction panic is caught.
    #[cfg(target_arch = "wasm32")]
    std::panic::set_hook(Box::new(|info| {
        let msg = info.to_string();
        web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&msg));
        if let Some(win) = web_sys::window() {
            let payload = wasm_bindgen::JsValue::from_str(&format!("bb-panic:{msg}"));
            // Same-window mail drop; the page listens for the "bb-panic:"
            // prefix. A failed post changes nothing — the console line above
            // already happened.
            let _ = win.post_message(&payload, "*");
        }
    }));

    let default_plugins = DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Breakneck Baseball".into(),
            resolution: (1280.0_f32, 720.0_f32).into(),
            // On the web, resize the render target to fill the browser
            // window instead of staying locked at 1280×720 (which would
            // otherwise overflow and clip centred UI).
            fit_canvas_to_parent: true,
            ..default()
        }),
        ..default()
    });
    // Dev iteration loop: read the model as a plain watched file so a
    // Blender re-export hot-reloads it. Release/wasm embed it instead.
    #[cfg(feature = "dev")]
    let default_plugins = default_plugins.set(bevy::asset::AssetPlugin {
        file_path: "src".into(),
        watch_for_changes_override: Some(true),
        ..Default::default()
    });
    let mut app = App::new();
    app
        // ── Core Bevy plugins (windowing, rendering via wgpu, asset loading …) ──
        .add_plugins(default_plugins)
        // ── 3-D physics via Rapier ───────────────────────────────────────────────
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
        // ── All game-specific systems ────────────────────────────────────────────
        .add_plugins(GamePlugin);

    // The portrait harness (Phase 4, Task 4): `--portraits <dir>` boots
    // windowed, force-enters the dev Creator stage, and walks every player
    // to a PNG for AI visual QA — see `game::portraits`. Native + debug only:
    // wasm has neither a CLI to parse this from nor a filesystem to write
    // PNGs to. Inserted after `add_plugins(GamePlugin)` (which registers
    // `portraits::PortraitsPlugin`) — resource insertion doesn't depend on
    // plugin build order, only on landing before `app.run()`.
    #[cfg(all(feature = "debug", not(target_arch = "wasm32")))]
    if let Some(dir) = parse_portraits_arg() {
        std::fs::create_dir_all(&dir)
            .unwrap_or_else(|e| panic!("--portraits {}: {e}", dir.display()));
        app.insert_resource(breakneck_baseball::game::portraits::PortraitRun::new(dir));
    }

    app.run();
}

/// Pulls `--portraits <dir>` out of `std::env::args`, if present. Not a
/// general-purpose CLI parser — one flag, one value, first occurrence wins —
/// this harness has no other arguments to worry about conflicting with.
#[cfg(all(feature = "debug", not(target_arch = "wasm32")))]
fn parse_portraits_arg() -> Option<std::path::PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--portraits")
        .and_then(|i| args.get(i + 1))
        .map(std::path::PathBuf::from)
}
