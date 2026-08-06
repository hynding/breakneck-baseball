//! Shared harness for headless end-to-end tests: boots the real app with no
//! window, no winit event loop, and virtual time stepped at 240 Hz, plus a
//! [`DriveGame`] schedule slot for the test's input driver.

use std::time::Duration;

use bevy::app::{MainScheduleOrder, PluginsState};
use bevy::core::{TaskPoolOptions, TaskPoolPlugin};
use bevy::ecs::schedule::{ExecutorKind, ScheduleLabel};
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

/// A queued key tap, applied from the [`DriveGame`] schedule. Pressing the
/// `ButtonInput` resource directly from a test body doesn't work: the input
/// plugin's `PreUpdate` clear wipes `just_pressed` before any `Update`
/// system sees it, so taps must be injected after `PreUpdate`.
#[derive(Resource, Default)]
pub struct TapKey(Option<(KeyCode, u8)>);

fn apply_taps(mut tap: ResMut<TapKey>, mut keyboard: ResMut<ButtonInput<KeyCode>>) {
    if let Some((key, frames_left)) = tap.0 {
        if frames_left > 0 {
            keyboard.press(key);
            tap.0 = Some((key, frames_left - 1));
        } else {
            keyboard.release(key);
            tap.0 = None;
        }
    }
}

/// Presses `key` for one frame (release the next) and steps the app past it.
#[allow(dead_code)]
pub fn tap_key(app: &mut App, key: KeyCode) {
    app.world_mut().resource_mut::<TapKey>().0 = Some((key, 1));
    for _ in 0..4 {
        app.update();
    }
}

/// Starts a game from the main menu by tapping `select_key` (**1** = one
/// player vs CPU, **2** = two players) and waits for `GameState::Playing`.
#[allow(dead_code)]
pub fn start_game(app: &mut App, select_key: KeyCode) {
    use breakneck_baseball::game::GameState;
    tap_key(app, select_key);
    let started = run_until(app, 2_000, |app| {
        *app.world()
            .resource::<bevy::prelude::State<GameState>>()
            .get()
            == GameState::Playing
    });
    assert!(started.is_some(), "menu never started the game");
}

/// Builds the headless app. Add driver systems to the [`DriveGame`] schedule
/// afterwards: `app.add_systems(DriveGame, drive)`.
#[allow(dead_code)]
pub fn headless_app() -> App {
    build_headless_app(false)
}

/// Like [`headless_app`], but pins **single-threaded, run-to-run deterministic**
/// execution: every schedule runs on one thread in a fixed topological order
/// (`ExecutorKind::SingleThreaded`) and the Bevy task pools are capped at one
/// thread. Bevy's default multi-threaded executor resolves ambiguous system
/// orderings by worker-timing — non-reproducible across runs, and (because the
/// tie-break rides type-id hashing) across binary layouts too — which is what
/// made the balance sim's aggregates jitter. This constructor removes that
/// source; the physics step is already single-threaded here (Rapier is built
/// with `simd-stable`, not the rayon `parallel` feature). Opt-in: only the
/// balance sim uses it, so the other e2e harnesses keep the faster default.
#[allow(dead_code)]
pub fn deterministic_headless_app() -> App {
    build_headless_app(true)
}

fn build_headless_app(single_threaded: bool) -> App {
    // Isolate the settings store before `SettingsPlugin` loads it: a
    // headless test must neither read the developer's real settings.json
    // (their volume/batting-style choices would silently steer test
    // behaviour) nor overwrite it when a test mutates `Settings` (the
    // pause board's strike-zone toggle really persists).
    std::env::set_var(
        "BREAKNECK_SETTINGS_PATH",
        std::env::temp_dir().join(format!("bb-e2e-settings-{}.json", std::process::id())),
    );
    let mut app = App::new();
    let default_plugins = DefaultPlugins
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
        .disable::<WinitPlugin>();
    // Deterministic mode caps every Bevy task pool at a single thread, so the
    // (single-threaded) executor below never contends for workers and any
    // schedule that slips through still runs its tasks in a fixed order.
    let default_plugins = if single_threaded {
        default_plugins.set(TaskPoolPlugin {
            task_pool_options: TaskPoolOptions::with_num_threads(1),
        })
    } else {
        default_plugins
    };
    app.add_plugins(default_plugins)
        .add_plugins((RapierPhysicsPlugin::<NoUserData>::default(), GamePlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
            DT,
        )))
        // `juice.rs`'s hit-stop/slow-mo dials `Time<Virtual>`'s relative_speed
        // on every Solid/Perfect ContactEvent. That scaling is NOT bypassed by
        // ManualDuration — bevy_time's time_system still runs
        // update_virtual_time, which multiplies the (manually-fixed) real
        // delta by relative_speed exactly as under automatic real-time — so
        // without this, any scripted swing in these tests that grades
        // Solid/Perfect would genuinely slow the shared virtual clock every
        // other timing-sensitive system reads. This insert is load-bearing,
        // not just belt-and-braces.
        .insert_resource(breakneck_baseball::game::juice::JuiceDisabled)
        // Mute the harness: `DefaultPlugins` keeps `AudioPlugin` (only
        // window/render/winit are stripped), so at a real volume every
        // headless run plays the synthesized crowd/cracks through the
        // machine's speakers — forty balance-sim games of crowd noise.
        // Overwrites the plugin-loaded settings with clean defaults at
        // volume zero (`apply_volume` mirrors it into `GlobalVolume`).
        .insert_resource(breakneck_baseball::game::settings::Settings {
            volume: 0.0,
            ..Default::default()
        });

    app.init_schedule(DriveGame);
    app.world_mut()
        .resource_mut::<MainScheduleOrder>()
        .insert_after(PreUpdate, DriveGame);
    app.init_resource::<TapKey>();
    app.add_systems(DriveGame, apply_taps);

    // Driving `app.update()` by hand skips what `App::run` would do: wait out
    // async plugin setup (the wgpu adapter request), then run `finish` /
    // `cleanup`, which insert late resources like `CapturedScreenshots`.
    while app.plugins_state() == PluginsState::Adding {
        bevy::tasks::tick_global_task_pools_on_main_thread();
    }
    app.finish();
    app.cleanup();

    // Force a fixed, single-threaded run order on every schedule now that the
    // full plugin graph (and its state-transition schedules) exists. The
    // multi-threaded executor is the last remaining non-determinism source once
    // the physics step is scalar/SIMD-serial; `SingleThreaded` runs each
    // schedule's systems in its fixed topological order, identical run-to-run.
    if single_threaded {
        for (_, schedule) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
            schedule.set_executor_kind(ExecutorKind::SingleThreaded);
        }
    }
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
