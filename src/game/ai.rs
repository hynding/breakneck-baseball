//! CPU opponent.
//!
//! The AI never bypasses gameplay logic — it writes into the very same
//! [`TeamIntent`] that a controller or keyboard would produce, so the pitching
//! and batting systems in `flow.rs` cannot tell a human from the CPU. These
//! systems run *before* the flow systems (see `FlowPlugin`) so the intent they
//! write is visible the same frame.

use bevy::prelude::*;

use crate::game::ball::Baseball;
use crate::game::flow::Phase;
use crate::game::flow::Play;
use crate::game::input::{Controllers, InputSource, Intents};
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
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            pitch_delay: Timer::from_seconds(0.9, TimerMode::Once),
            decided_swing: false,
        }
    }
}

/// Cheap deterministic noise in 0.0..1.0 from a float seed (no `rand` dep, and
/// wasm-safe). Good enough to give pitch location and swing timing some variety.
fn hash01(seed: f32) -> f32 {
    let v = (seed * 12.9898).sin() * 43758.547;
    v - v.floor()
}

/// Noise in −1.0..1.0.
fn noise(seed: f32) -> f32 {
    hash01(seed) * 2.0 - 1.0
}

// ── Defense: the AI pitches ───────────────────────────────────────────────────

pub fn cpu_defense(
    time: Res<Time>,
    controllers: Res<Controllers>,
    cfg: Res<CpuConfig>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
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

    if cpu.pitch_delay.tick(time.delta()).finished() {
        let t = time.elapsed_secs();
        // Better skill → tighter aim around the strike zone.
        let spread = 0.55 * (1.0 - cfg.skill) + 0.12;
        let aim = Vec2::new(noise(t * 1.7) * spread, noise(t * 2.3) * spread);

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

pub fn cpu_offense(
    time: Res<Time>,
    controllers: Res<Controllers>,
    cfg: Res<CpuConfig>,
    score: Res<ScoreBoard>,
    play: Res<Play>,
    mut cpu: ResMut<CpuState>,
    ball_q: Query<&Transform, With<Baseball>>,
    mut intents: ResMut<Intents>,
) {
    let team = score.batting_team();
    if controllers.source(team) != InputSource::Cpu {
        return;
    }

    // Reset the per-pitch decision at the start of each pitch.
    if play.phase == Phase::PrePitch {
        cpu.decided_swing = false;
        intents.get_mut(team).action = false;
        return;
    }
    if play.phase != Phase::Pitch || cpu.decided_swing {
        intents.get_mut(team).action = false;
        return;
    }
    let Ok(ball) = ball_q.get_single() else {
        return;
    };
    let pos = ball.translation;

    let t = time.elapsed_secs();
    // The AI commits when the ball reaches its (noisy) trigger depth. A wider
    // spread at lower skill means the CPU often mistimes — producing the weak
    // pop-ups and grounders that make outs, so innings actually end.
    let trigger_z = 0.45 + noise(t * 3.1) * 1.6 * (1.0 - cfg.skill);
    if pos.z > trigger_z {
        intents.get_mut(team).action = false;
        return;
    }

    cpu.decided_swing = true;

    // Is the pitch a strike as it nears the plate?
    let in_zone = pos.x.abs() < 0.5 && (0.4..=1.6).contains(&pos.y);
    let roll = hash01(t * 5.0);
    let swing = if in_zone {
        roll < 0.5 + 0.4 * cfg.skill // usually offers at strikes
    } else {
        roll < 0.28 * (1.0 - cfg.skill) // rarely chases balls
    };

    let intent = intents.get_mut(team);
    if swing {
        intent.action = true;
        // Spread the intended launch from low grounders to high flies so batted
        // balls vary instead of all being squared-up line drives.
        intent.aim = Vec2::new(noise(t * 7.0) * 0.6, -0.25 + hash01(t * 9.0) * 1.0);
    } else {
        intent.action = false;
    }
}
