//! Breakneck Baseball — a 3-D baseball game built on Bevy (wgpu) + Rapier.
//!
//! This entry-point assembles all plugins and runs the application.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

mod game;

use game::GamePlugin;

fn main() {
    App::new()
        // ── Core Bevy plugins (windowing, rendering via wgpu, asset loading …) ──
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Breakneck Baseball".into(),
                resolution: (1280.0, 720.0).into(),
                ..default()
            }),
            ..default()
        }))
        // ── 3-D physics via Rapier ───────────────────────────────────────────────
        .add_plugins(RapierPhysicsPlugin::<NoUserData>::default())
        // ── Debug collider outlines (disable in release if desired) ──────────────
        .add_plugins(RapierDebugRenderPlugin::default())
        // ── All game-specific systems ────────────────────────────────────────────
        .add_plugins(GamePlugin)
        .run();
}
