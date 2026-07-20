//! Shared harness for headless end-to-end tests: boots the real app with no
//! window, no winit event loop, and virtual time stepped at 240 Hz, plus a
//! [`DriveGame`] schedule slot for the test's input driver.

use std::time::Duration;

use bevy::app::{MainScheduleOrder, PluginsState};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::render::RenderPlugin;
use bevy::time::TimeUpdateStrategy;
use bevy::winit::WinitPlugin;
use bevy_rapier3d::prelude::{NoUserData, RapierPhysicsPlugin};

use breakneck_baseball::game::GamePlugin;

/// Simulation step: 240 Hz keeps swing-timing windows (~0.12 m of ball travel
/// per frame) tight enough for deterministic scripted contact.
pub const DT: f64 = 1.0 / 240.0;

/// Runs after `PreUpdate` (so `gather_intents` has refreshed keyboard-driven
/// intents) and before `Update` (so the flow systems read what a test driver
/// wrote) — the same [`breakneck_baseball::game::input::Intents`] seam the
/// CPU AI uses.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DriveGame;

/// Builds the headless app. Add driver systems to the [`DriveGame`] schedule
/// afterwards: `app.add_systems(DriveGame, drive)`.
pub fn headless_app() -> App {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            // No window, no winit event loop, and no GPU at all: CI runners
            // have no adapter, so rendering is disabled outright. The
            // finish()/cleanup() below still runs every plugin's late setup
            // (e.g. CapturedScreenshots), which is what the main-app render
            // systems need to no-op safely.
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(WgpuSettings {
                    backends: None,
                    ..default()
                }),
                ..default()
            })
            .disable::<WinitPlugin>(),
    )
    .add_plugins((RapierPhysicsPlugin::<NoUserData>::default(), GamePlugin))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        DT,
    )));

    app.init_schedule(DriveGame);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_after(PreUpdate, DriveGame);

    // Driving `app.update()` by hand skips what `App::run` would do: wait out
    // async plugin setup (the wgpu adapter request), then run `finish` /
    // `cleanup`, which insert late resources like `CapturedScreenshots`.
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();
    app
}

/// Steps the app until `done` returns true, up to `max_frames`. Returns the
/// frames consumed, or `None` if the predicate never held.
pub fn run_until(
    app: &mut App,
    max_frames: u64,
    mut done: impl FnMut(&mut App) -> bool,
) -> Option<u64> {
    for frame in 1..=max_frames {
        app.update();
        if done(app) {
            return Some(frame);
        }
    }
    None
}
