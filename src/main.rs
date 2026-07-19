//! Breakneck Baseball — a 3-D baseball game built on Bevy (wgpu) + Rapier.
//!
//! This entry-point assembles all plugins and runs the application.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use breakneck_baseball::game::GamePlugin;

fn main() {
    App::new()
        // ── Core Bevy plugins (windowing, rendering via wgpu, asset loading …) ──
        .add_plugins(DefaultPlugins.set(WindowPlugin {
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
        }))
        // ── 3-D physics via Rapier ───────────────────────────────────────────────
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
        // ── All game-specific systems ────────────────────────────────────────────
        .add_plugins(GamePlugin)
        .run();
}
