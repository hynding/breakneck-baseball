//! Game feel: hit-stop on solid contact, and a slow-motion tail on a
//! dead-on Perfect swing. Driven entirely by `ContactEvent` quality and
//! implemented by dialing `Time<Virtual>`'s `relative_speed` — like
//! `fx.rs`'s own hit-stop, this module is purely cosmetic and never touches
//! `ScoreBoard`/`Bases`/rules outcomes.
//!
//! Insert [`JuiceDisabled`] to suppress every effect outright. The test
//! harness (`tests/common/mod.rs`) does this: `relative_speed` scaling is
//! *not* bypassed by `TimeUpdateStrategy::ManualDuration` (checked against
//! `bevy_time`'s `time_system`/`update_virtual_time` — `ManualDuration` only
//! substitutes how `Time<Real>`'s delta is computed; `Time<Virtual>` still
//! multiplies that delta by `relative_speed` exactly as under real-time
//! play), so without `JuiceDisabled` a scripted e2e swing that grades
//! Solid/Perfect would genuinely slow the shared virtual clock every other
//! timing-sensitive system reads — the insert is load-bearing, not just
//! belt-and-braces.
//!
//! A watchdog (real-seconds, so it can't be fooled by the very
//! `relative_speed` it's guarding) restores `relative_speed` to 1.0 no
//! matter what state the effect was in if its budget elapses, and an
//! unconditional `OnExit(Playing)` system does the same immediately on
//! pause or game over — so a slow-mo started just before Esc can never
//! leave the game stuck below normal speed.

use bevy::prelude::*;

use crate::game::flow::{ContactEvent, Phase, Play};
use crate::game::rules::ContactQuality;
use crate::game::{game_start, GameState};

/// Insert to skip every hit-stop/slow-mo effect outright. Nothing in the
/// shipping game inserts this — it exists for headless/test contexts where
/// a slowed virtual clock would corrupt scripted timing (see module doc).
#[derive(Resource)]
pub struct JuiceDisabled;

/// How many Update frames the initial freeze holds, and how slow it runs.
/// Frame-counted rather than time-ticked: at `FREEZE_SPEED` a
/// `Time<Virtual>`-ticked timer would barely advance at all, so the freeze
/// counts real Update frames directly instead.
const FREEZE_FRAMES: u32 = 4;
const FREEZE_SPEED: f32 = 0.05;

/// Perfect's slow-mo tail: 0.5 *virtual* seconds at 0.3x, ticked by
/// `Time<Virtual>`'s own (already-slowed) delta.
const SLOWMO_SECS: f32 = 0.5;
const SLOWMO_SPEED: f32 = 0.3;

/// Real-seconds ceiling on how long any effect may hold `relative_speed`
/// below 1.0. Measured against `Time<Real>`, which `relative_speed` cannot
/// itself slow down, so a stuck effect can never inflate its own budget.
/// Sized comfortably above the longest real-time effect (Perfect's freeze
/// plus its ~1.67 s slow-mo tail at 0.3x).
const WATCHDOG_BUDGET_SECS: f32 = 3.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Counting down `FREEZE_FRAMES` at `FREEZE_SPEED`.
    Freeze,
    /// Ticking `SLOWMO_SECS` (virtual) at `SLOWMO_SPEED` — Perfect only.
    SlowMo,
}

/// What the current effect is doing, if anything. `watchdog` runs
/// independently of `stage`/the gated systems below so it can restore even
/// if those systems stop running mid-effect (state left `Playing`,
/// `JuiceDisabled` inserted mid-flight, etc).
#[derive(Resource, Default)]
struct JuiceState {
    stage: Option<Stage>,
    freeze_frames_left: u32,
    slowmo_timer: Option<Timer>,
    /// Perfect chains a slow-mo tail onto the freeze; Solid does not.
    queue_slowmo: bool,
    watchdog: Option<Timer>,
}

impl JuiceState {
    fn start(&mut self, perfect: bool) {
        self.stage = Some(Stage::Freeze);
        self.freeze_frames_left = FREEZE_FRAMES;
        self.slowmo_timer = None;
        self.queue_slowmo = perfect;
        self.watchdog = Some(Timer::from_seconds(WATCHDOG_BUDGET_SECS, TimerMode::Once));
    }

    fn clear(&mut self) {
        self.stage = None;
        self.freeze_frames_left = 0;
        self.slowmo_timer = None;
        self.queue_slowmo = false;
        self.watchdog = None;
    }
}

pub struct JuicePlugin;

impl Plugin for JuicePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<JuiceState>()
            .add_systems(game_start(), reset_juice)
            .add_systems(
                Update,
                (trigger_juice, tick_freeze, tick_slowmo)
                    .chain()
                    .run_if(juice_enabled)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                restore_on_result.run_if(in_state(GameState::Playing)),
            )
            // Unconditional: must restore even if the gated systems above
            // stopped running (state change, JuiceDisabled inserted
            // mid-flight) — see the module doc.
            .add_systems(Update, tick_watchdog)
            .add_systems(OnExit(GameState::Playing), force_restore);
    }
}

fn juice_enabled(disabled: Option<Res<JuiceDisabled>>) -> bool {
    disabled.is_none()
}

fn reset_juice(mut virt: ResMut<Time<Virtual>>, mut state: ResMut<JuiceState>) {
    virt.set_relative_speed(1.0);
    state.clear();
}

/// Starts (or restarts) the freeze on a graded Solid/Perfect swing. Other
/// qualities (Whiff, FoulTip, and the currently-unreachable Weak) get no
/// hit-stop.
fn trigger_juice(
    mut events: EventReader<ContactEvent>,
    mut virt: ResMut<Time<Virtual>>,
    mut state: ResMut<JuiceState>,
) {
    for ev in events.read() {
        match ev.quality {
            ContactQuality::Solid => state.start(false),
            ContactQuality::Perfect => state.start(true),
            _ => continue,
        }
        virt.set_relative_speed(FREEZE_SPEED);
    }
}

/// Counts down the frame-based freeze; on expiry either chains into the
/// Perfect slow-mo tail or restores full speed (Solid).
fn tick_freeze(mut virt: ResMut<Time<Virtual>>, mut state: ResMut<JuiceState>) {
    if state.stage != Some(Stage::Freeze) {
        return;
    }
    state.freeze_frames_left = state.freeze_frames_left.saturating_sub(1);
    if state.freeze_frames_left > 0 {
        return;
    }
    if state.queue_slowmo {
        state.stage = Some(Stage::SlowMo);
        state.slowmo_timer = Some(Timer::from_seconds(SLOWMO_SECS, TimerMode::Once));
        virt.set_relative_speed(SLOWMO_SPEED);
    } else {
        virt.set_relative_speed(1.0);
        state.clear();
    }
}

/// Ticks Perfect's slow-mo tail in virtual seconds; restores on expiry.
fn tick_slowmo(mut virt: ResMut<Time<Virtual>>, mut state: ResMut<JuiceState>) {
    if state.stage != Some(Stage::SlowMo) {
        return;
    }
    let delta = virt.delta();
    let finished = state
        .slowmo_timer
        .as_mut()
        .is_some_and(|t| t.tick(delta).finished());
    if finished {
        virt.set_relative_speed(1.0);
        state.clear();
    }
}

/// The result pause must never carry a slow-mo hangover into it — the
/// brief calls for a restore the instant `Phase` reaches `Result`.
fn restore_on_result(
    play: Res<Play>,
    mut virt: ResMut<Time<Virtual>>,
    mut state: ResMut<JuiceState>,
) {
    if play.phase == Phase::Result && state.stage.is_some() {
        virt.set_relative_speed(1.0);
        state.clear();
    }
}

/// Belt-and-braces: forces a restore regardless of `GameState`/
/// `JuiceDisabled`/`Phase` once the real-seconds budget elapses, so no
/// combination of state changes can leave `relative_speed` stuck below 1.0.
fn tick_watchdog(
    real: Res<Time<Real>>,
    mut virt: ResMut<Time<Virtual>>,
    mut state: ResMut<JuiceState>,
) {
    let elapsed = state
        .watchdog
        .as_mut()
        .is_some_and(|t| t.tick(real.delta()).finished());
    if elapsed {
        virt.set_relative_speed(1.0);
        state.clear();
    }
}

/// Leaving `Playing` (pause or game over) restores immediately — no reason
/// to wait out the watchdog budget for the common case.
fn force_restore(mut virt: ResMut<Time<Virtual>>, mut state: ResMut<JuiceState>) {
    virt.set_relative_speed(1.0);
    state.clear();
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Team;
    use std::time::Duration;

    /// A minimal app: `MinimalPlugins` (for `Time`) plus `StatesPlugin` (for
    /// `GameState`) plus the plugin under test — no rendering, physics, or
    /// the rest of `GamePlugin`. `ContactEvent` is registered directly since
    /// `FlowPlugin` isn't present.
    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<GameState>()
            .init_resource::<Play>()
            .add_event::<ContactEvent>()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
                Duration::from_secs_f64(1.0 / 60.0),
            ))
            .add_plugins(JuicePlugin);
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Playing);
        app.update(); // Applies the state transition before any assertions.
        app
    }

    fn send_perfect(app: &mut App) {
        app.world_mut().send_event(ContactEvent {
            quality: ContactQuality::Perfect,
            batting_team: Team::Home,
            dt_ms: 0.0,
        });
    }

    fn speed(app: &App) -> f32 {
        app.world().resource::<Time<Virtual>>().relative_speed()
    }

    #[test]
    fn perfect_contact_restores_speed_within_budget() {
        let mut app = test_app();
        assert_eq!(speed(&app), 1.0, "must start at full speed");

        send_perfect(&mut app);
        app.update();
        assert!(
            speed(&app) < 1.0,
            "a Perfect swing must engage the freeze, got {}",
            speed(&app)
        );

        // 250 frames at 1/60 s ≈ 4.17 s real time — comfortably past both
        // the natural freeze+slow-mo completion and the watchdog's 3 s
        // budget, so this holds regardless of which mechanism restored it.
        for _ in 0..250 {
            app.update();
        }
        assert_eq!(
            speed(&app),
            1.0,
            "relative_speed must be back to 1.0 within the budget"
        );
    }

    #[test]
    fn juice_disabled_blocks_every_effect() {
        let mut app = test_app();
        app.insert_resource(JuiceDisabled);

        send_perfect(&mut app);
        for _ in 0..10 {
            app.update();
            assert_eq!(
                speed(&app),
                1.0,
                "JuiceDisabled must keep relative_speed untouched"
            );
        }
    }
}
