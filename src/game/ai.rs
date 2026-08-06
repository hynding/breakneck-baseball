//! CPU opponent.
//!
//! The AI never bypasses gameplay logic — it writes into the very same
//! [`TeamIntent`] that a controller or keyboard would produce, so the pitching
//! and batting systems in `flow.rs` cannot tell a human from the CPU. These
//! systems run *before* the flow systems (see `FlowPlugin`) so the intent they
//! write is visible the same frame.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::ball::Baseball;
use crate::game::flow::{late_swing_z, swing_dt_ms, LeadState, Phase, Play};
use crate::game::input::{Controllers, InputSource, Intents};
use crate::game::rules::{steal_candidate, Bases, GRAVITY};
use crate::game::variant::Ruleset;
use crate::game::ScoreBoard;

/// A single knob for opponent difficulty (0.0 = easy, 1.0 = tough).
#[derive(Resource)]
pub struct CpuConfig {
    pub skill: f32,
}

impl Default for CpuConfig {
    fn default() -> Self {
        Self { skill: 0.4 }
    }
}

/// Per-pitch bookkeeping for the AI.
#[derive(Resource)]
pub struct CpuState {
    /// Counts down between CPU pitches so it doesn't fire instantly.
    pitch_delay: Timer,
    /// True once the AI batter has already committed a swing decision this pitch.
    decided_swing: bool,
    /// Whether the AI batter has decided to offer at this pitch at all —
    /// decided once, at the old trigger depth (in-zone/chase roll); the
    /// timing of the *press* is separate, gated by `swing_target_dt` below.
    will_swing: Option<bool>,
    /// The swing-timing error (ms, same signed convention as
    /// [`crate::game::flow::swing_dt_ms`]) the CPU is aiming for this pitch —
    /// drawn once from deterministic noise so its swings scatter around
    /// dead-on timing instead of always firing at a fixed ball depth.
    swing_target_dt: Option<f32>,
    /// Whether the AI offense sends the runner this pitch — decided once at
    /// the start of the windup and held for the whole delivery.
    steal_call: Option<bool>,
    /// Whether the AI offense stretches the lead through the pre-pitch steal
    /// window — decided once per window (the early break, pickoff risk).
    window_steal: Option<bool>,
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            pitch_delay: Timer::from_seconds(0.9, TimerMode::Once),
            decided_swing: false,
            will_swing: None,
            swing_target_dt: None,
            steal_call: None,
            window_steal: None,
        }
    }
}

/// Cheap deterministic noise in 0.0..1.0 from a float seed (no `rand` dep, and
/// wasm-safe). Good enough to give pitch location and swing timing some variety.
pub(crate) fn hash01(seed: f32) -> f32 {
    let v = (seed * 12.9898).sin() * 43758.547;
    v - v.floor()
}

/// Noise in −1.0..1.0.
pub(crate) fn noise(seed: f32) -> f32 {
    hash01(seed) * 2.0 - 1.0
}

/// Draws the per-pitch swing-timing target: signed milliseconds (same
/// convention as [`crate::game::flow::swing_dt_ms`] — early negative, late
/// positive), scaled by the configured spread. Pure so the press decision
/// can be pinned by a synthetic `dt` ramp without booting the ECS (Task B3
/// review fix).
///
/// (A CPU-only "Perfect deadband" here — biasing the draw out of the dead-on
/// window to cut barreled CPU home runs — was tried and reverted: because
/// `hit_velocity`'s exit sweet spot is `contact_z ≈ 0.4` (≈ −11 ms of timing),
/// not dt = 0, a symmetric-around-zero deadband excludes *all* hard contact,
/// not just barrels, and collapses the offense. HR is instead held down by the
/// flattened CPU launch aim below plus the balance harness's asserted slack.)
pub(crate) fn draw_target_dt(seed: f32, spread_ms: f32) -> f32 {
    noise(seed) * spread_ms
}

/// Whether *this* frame is the one to press: the live timing error has
/// reached (or passed) the drawn target. [`crate::game::flow::swing_dt_ms`]
/// rises monotonically from negative (early) through zero to positive
/// (late) as the pitch approaches, so this is a plain threshold crossing —
/// the first frame it holds is the press frame.
pub(crate) fn ready_to_press(dt_ms: f32, target_dt: f32) -> bool {
    dt_ms >= target_dt
}

// ── Defense: the AI pitches ───────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn cpu_defense(
    time: Res<Time>,
    controllers: Res<Controllers>,
    cfg: Res<CpuConfig>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    lead: Res<LeadState>,
    mut cpu: ResMut<CpuState>,
    mut intents: ResMut<Intents>,
) {
    let team = score.fielding_team();
    if controllers.source(team) != InputSource::Cpu {
        return;
    }

    // Only act while waiting to pitch; keep the button released otherwise.
    if play.phase != Phase::PrePitch {
        intents.get_mut(team).action = false;
        cpu.pitch_delay.reset();
        return;
    }

    // The steal window: the ball is held, and pressing action would be a
    // pickoff throw. Mostly the CPU just waits it out — but a runner
    // stretching his lead draws the occasional throw over.
    if play.in_steal_window() {
        let intent = intents.get_mut(team);
        intent.action = false;
        if lead.extended {
            // ~0.5 attempts per second of extended lead, deterministic noise.
            let p = (0.3 + 0.5 * cfg.skill) * time.delta_secs();
            intent.action = hash01(time.elapsed_secs() * 3.9) < p;
        }
        cpu.pitch_delay.reset();
        return;
    }

    if cpu.pitch_delay.tick(time.delta()).finished() {
        let t = time.elapsed_secs();
        // Better skill → tighter aim around the strike zone. The aim then
        // gets a pitch-selection bias: held-aim direction is what picks the
        // kind (see `PitchKind::from_aim`), so shifting the aim is how the
        // CPU "calls" its pitch — heaters up, benders down, sweepers wide.
        let spread = 0.55 * (1.0 - cfg.skill) + 0.12;
        let mut aim = Vec2::new(noise(t * 1.7) * spread, noise(t * 2.3) * spread * 0.5);
        let roll = hash01(t * 4.3);
        if roll < 0.40 {
            aim.y += 0.55; // fastball
        } else if roll < 0.65 {
            // changeup: neutral
        } else if roll < 0.85 {
            aim.y -= 0.55; // curveball
        } else if roll < 0.925 {
            aim.x -= 0.6; // slider, sweeping in
        } else {
            aim.x += 0.6; // sinker, running away
        }
        aim = aim.clamp(Vec2::splat(-1.0), Vec2::splat(1.0));

        let intent = intents.get_mut(team);
        intent.action = true;
        intent.aim = aim;

        // Vary the wait before the next pitch a little.
        let wait = 0.7 + hash01(t) * 0.5;
        cpu.pitch_delay = Timer::from_seconds(wait, TimerMode::Once);
    } else {
        intents.get_mut(team).action = false;
    }
}

// ── Offense: the AI bats ──────────────────────────────────────────────────────

// Bevy systems take their dependencies as parameters; the count is inherent.
#[allow(clippy::too_many_arguments)]
pub fn cpu_offense(
    time: Res<Time>,
    controllers: Res<Controllers>,
    cfg: Res<CpuConfig>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    bases: Res<Bases>,
    rules: Res<Ruleset>,
    mut cpu: ResMut<CpuState>,
    ball_q: Query<(&Transform, &Velocity), With<Baseball>>,
    mut intents: ResMut<Intents>,
) {
    let team = score.batting_team();
    if controllers.source(team) != InputSource::Cpu {
        return;
    }

    // Reset the per-pitch decisions before each delivery. In the steal
    // window the AI occasionally stretches the lead — the early break: a
    // guaranteed jump on the pitch, bought at pickoff risk — and, once
    // committed, keeps the lead stretched after the window closes so the
    // gamble actually cashes in at the delivery. A vanished candidate (the
    // runner was picked off) drops the plan.
    if play.phase == Phase::PrePitch {
        cpu.decided_swing = false;
        cpu.will_swing = None;
        cpu.swing_target_dt = None;
        cpu.steal_call = None;
        let extend = if steal_candidate(&bases).is_some() {
            if play.in_steal_window() {
                *cpu.window_steal.get_or_insert_with(|| {
                    hash01(time.elapsed_secs() * 5.3) < 0.1 + 0.2 * cfg.skill
                })
            } else {
                cpu.window_steal == Some(true)
            }
        } else {
            cpu.window_steal = None;
            false
        };
        let intent = intents.get_mut(team);
        intent.action = false;
        intent.aim = if extend {
            Vec2::new(0.0, -1.0)
        } else {
            Vec2::ZERO
        };
        return;
    }
    // During the windup the AI occasionally sends the runner — decided once,
    // then the aim is held down so flow sees the steal armed all delivery.
    // A lead stretched through the window is always sent (that was the plan);
    // with no candidate, nobody goes.
    if play.phase == Phase::WindUp {
        cpu.decided_swing = false;
        cpu.will_swing = None;
        cpu.swing_target_dt = None;
        let elapsed = time.elapsed_secs();
        let committed = cpu.window_steal == Some(true);
        let send = steal_candidate(&bases).is_some()
            && (committed
                || *cpu
                    .steal_call
                    .get_or_insert_with(|| hash01(elapsed * 6.1) < 0.2 + 0.2 * cfg.skill));
        let intent = intents.get_mut(team);
        intent.action = false;
        if send {
            intent.aim = Vec2::new(0.0, -1.0);
        }
        return;
    }
    if play.phase != Phase::Pitch || cpu.decided_swing {
        cpu.window_steal = None;
        intents.get_mut(team).action = false;
        return;
    }
    let Ok((ball, ball_vel)) = ball_q.get_single() else {
        return;
    };
    let pos = ball.translation;
    let t = time.elapsed_secs();

    // Draw the swing's timing target once per pitch, deterministic on the
    // pitch instant — the CPU's human-like timing scatter (Task B3). Same
    // signed convention as `flow::swing_dt_ms`: negative is early, positive
    // is late, drawn from ±`cpu_timing_spread_ms`.
    let target_dt = *cpu
        .swing_target_dt
        .get_or_insert_with(|| draw_target_dt(t * 11.9, rules.batting.cpu_timing_spread_ms));

    // Decide whether to offer at this pitch — once, and **early**, while the
    // ball is still well in front of the plate (past `SWING_EARLY_Z`, the
    // reachable window's front edge). This is what makes the drawn `target_dt`
    // actually govern the press: the old code committed at a ~0.45 m trigger
    // depth, by which point an early target had nothing left to wait for and
    // collapsed onto that near-zero dt — so the CPU always *connected* and
    // never swung through. Committing out here lets an early draw fire while
    // the ball is still unreachable, i.e. a genuine timing whiff. `decision_z`
    // carries a little skill-scaled jitter so the commit depth varies.
    if cpu.will_swing.is_none() {
        let decision_z = 6.5 + noise(t * 3.1) * 1.5;
        if pos.z > decision_z {
            intents.get_mut(team).action = false;
            return;
        }
        // Predicted plate crossing (gravity only; the pitch's drag is the CPU's
        // to misjudge) drives the in-zone/chase offer roll now that the offer
        // is committed before the ball nears the plate.
        let vz = ball_vel.linvel.z.min(-f32::EPSILON);
        let flight = (pos.z / -vz).max(0.0);
        let cross = Vec2::new(
            pos.x + ball_vel.linvel.x * flight,
            pos.y + ball_vel.linvel.y * flight - 0.5 * GRAVITY * flight * flight,
        );
        // The CPU's *judged* zone: the real called zone (rules::ZONE_*)
        // plus a hitter's honest misjudgment fuzz — ~0.1 m wide, ~0.15 m
        // tall. Tracks the rulebook zone so a zone retune doesn't silently
        // turn the CPU into a chaser or a statue.
        let in_zone = cross.x.abs() < 0.35 && (0.4..=1.45).contains(&cross.y);
        let roll = hash01(t * 5.0);
        let swing = if in_zone {
            roll < 0.5 + 0.4 * cfg.skill // usually offers at strikes
        } else {
            roll < 0.28 * (1.0 - cfg.skill) // rarely chases balls
        };
        cpu.will_swing = Some(swing);
        if !swing {
            cpu.decided_swing = true;
            intents.get_mut(team).action = false;
            return;
        }
    }

    // Committed to swinging: press when the live timing error reaches the drawn
    // target, OR at the last frame the swing can still connect (`late_swing_z`,
    // where the error equals `foul_ms`) — whichever comes first. `swing_dt_ms`
    // rises from negative (ball out front) through zero toward positive (past
    // the plate). An *early* target fires while the ball is still beyond
    // `SWING_EARLY_Z` → a real swing-through (Whiff). A *late* target fires at
    // the reachable-late edge → an honest FoulTip/Whiff, instead of the ball
    // reaching the take judgment first and being scored a called strike (which
    // is what used to make the CPU's K take-driven rather than whiff-driven).
    let dt_ms = swing_dt_ms(pos.z, ball_vel.linvel.z);
    let past_late_edge = pos.z <= late_swing_z(ball_vel.linvel.z, rules.batting.foul_ms);
    if !ready_to_press(dt_ms, target_dt) && !past_late_edge {
        intents.get_mut(team).action = false;
        return;
    }

    cpu.decided_swing = true;
    let intent = intents.get_mut(team);
    intent.action = true;
    // Spread the intended launch from grounders through line drives to the
    // occasional fly, but keep the *plane flat* — the launch aim is the CPU's
    // home-run dial (a towering uppercut clears the regulation fence even at a
    // modest exit multiplier), so a mean well below a fly-ball angle keeps the
    // HR rate in the balance band while still putting balls in play (see
    // tests/balance_sim.rs). Horizontal spray is unchanged.
    intent.aim = Vec2::new(noise(t * 7.0) * 0.6, -0.8 + hash01(t * 9.0) * 0.85);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_target_dt_is_deterministic_and_bounded() {
        // Same seed always draws the same target (the CPU's decision has to
        // be reproducible across frames via `get_or_insert_with`).
        let a = draw_target_dt(12.34, 70.0);
        let b = draw_target_dt(12.34, 70.0);
        assert_eq!(a, b);
        // noise() is in −1.0..1.0, so the draw never exceeds the spread.
        assert!(a.abs() <= 70.0 + f32::EPSILON);

        // A different seed generally draws a different target — spot-check
        // a handful so this isn't a degenerate constant function.
        let distinct = (0..8)
            .map(|i| draw_target_dt(i as f32 * 11.9, 70.0))
            .collect::<Vec<_>>();
        assert!(
            distinct.windows(2).any(|w| (w[0] - w[1]).abs() > 1.0),
            "expected varying draws across seeds, got {distinct:?}"
        );
    }

    #[test]
    fn ready_to_press_fires_on_the_first_frame_dt_reaches_target() {
        // A synthetic dt ramp standing in for `swing_dt_ms` sampled once per
        // frame as a pitch approaches: monotonically increasing, early
        // (negative) to late (positive), like the live ball's timing error.
        let ramp: Vec<f32> = (0..50).map(|i| -100.0 + i as f32 * 4.0).collect();
        let target = 37.0;

        let fired = ramp.iter().position(|&dt| ready_to_press(dt, target));

        // First dt >= 37.0 in the ramp (-100, -96, ..., step 4) is 40.0 at
        // index 35 — pin the exact frame, not just "eventually fires".
        assert_eq!(fired, Some(35));
        assert!(ramp[35] >= target);
        assert!(ramp[34] < target);
    }

    #[test]
    fn ready_to_press_fires_immediately_for_an_already_past_target() {
        // A target earlier than the current dt (the commit landed later
        // than the drawn target) presses on the very same frame — a swing
        // can't be un-pressed to wait for a target already behind it.
        assert!(ready_to_press(-10.0, -50.0));
        assert!(ready_to_press(0.0, 0.0));
    }

    #[test]
    fn ready_to_press_holds_for_a_target_not_yet_reached() {
        assert!(!ready_to_press(-20.0, 10.0));
    }
}
