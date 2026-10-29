//! CPU opponent.
//!
//! The AI never bypasses gameplay logic — it writes into the very same
//! [`TeamIntent`] that a controller or keyboard would produce, so the pitching
//! and batting systems in `game::flow` cannot tell a human from the CPU. These
//! systems run *before* the flow systems (see `FlowPlugin`) so the intent they
//! write is visible the same frame.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::ScoreBoard;
use crate::game::ball::Baseball;
use crate::game::flow::{
    LeadState, Phase, Play, bat_arrival_z, late_swing_z, swing_dt_at_contact_ms,
};
use crate::game::input::{Controllers, InputSource, Intents};
use crate::game::rules::{Bases, GRAVITY, PitchKind, steal_candidate};
use crate::game::scenario::PitchOverride;
use crate::game::variant::Ruleset;

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
    /// Whether the batter has offered at anything this plate appearance —
    /// the CPU pitcher's read of a hitter who is just standing there (see
    /// [`taker_scatter_scale`]).
    batter_swung_this_pa: bool,
    /// Consecutive finished plate appearances in which the batter never
    /// offered — the pitcher's read of a hitter who is *never* swinging
    /// (see [`grooves_for_taker`]).
    passive_pas: u32,
    /// The pitch sequence number of the current plate appearance's first
    /// wind-up, so the bookkeeping above runs once per plate appearance.
    pa_start_seq: Option<u32>,
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
    /// The game's noise salt: the clock at game start, captured once
    /// (TODO 105). Every draw below mixes this with the pitch sequence
    /// instead of the running clock, so the game's *pacing* — result holds,
    /// walk-ups, the curtain — can never change which pitch gets which draw.
    /// Different games (the harness idles a different number of frames
    /// before each) still get different salts.
    salt: f32,
    /// Pitches delivered so far this game (counted at each WindUp).
    seq: u32,
    /// Frames since the last phase change — the per-frame rolls (a pickoff
    /// throw during the steal window) vary on this, not on the clock.
    phase_frames: u32,
    /// The phase seen last frame, for the edge detection above.
    last_phase: Option<Phase>,
}

impl CpuState {
    /// The seed for a per-pitch draw with salt `k` — one value per (game,
    /// pitch, k), independent of wall time.
    fn seed(&self, k: f32) -> f32 {
        self.salt + self.seq as f32 * 1.618_034 + k * 0.731
    }

    /// A per-pitch roll in 0..1.
    fn roll(&self, k: f32) -> f32 {
        hash01(self.seed(k))
    }

    /// A per-pitch draw in −1..1.
    fn noise(&self, k: f32) -> f32 {
        noise(self.seed(k))
    }

    /// A per-frame roll in 0..1 that changes every frame of the current
    /// phase (for chances expressed per second of a held state).
    fn frame_roll(&self, k: f32) -> f32 {
        hash01(self.seed(k) + self.phase_frames as f32 * 0.013_7)
    }
}

/// Keeps the CPU's noise clock: the phase-change edge, the pitch sequence
/// (one per WindUp), and the frame-within-phase counter. Runs before both
/// CPU systems every frame.
pub fn tick_cpu_clock(play: Res<Play>, mut cpu: ResMut<CpuState>) {
    if cpu.last_phase != Some(play.phase) {
        cpu.last_phase = Some(play.phase);
        cpu.phase_frames = 0;
        if play.phase == Phase::WindUp {
            cpu.seq += 1;
        }
    } else {
        cpu.phase_frames += 1;
    }
}

/// Game start: a fresh sequence under this game's salt.
pub fn reset_cpu_clock(time: Res<Time>, mut cpu: ResMut<CpuState>) {
    *cpu = CpuState {
        salt: time.elapsed_secs(),
        ..CpuState::default()
    };
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            pitch_delay: Timer::from_seconds(0.9, TimerMode::Once),
            batter_swung_this_pa: false,
            passive_pas: 0,
            pa_start_seq: None,
            decided_swing: false,
            will_swing: None,
            swing_target_dt: None,
            steal_call: None,
            window_steal: None,
            salt: 0.0,
            seq: 0,
            phase_frames: 0,
            last_phase: None,
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
/// window to cut barreled CPU home runs — was tried and reverted: it
/// excluded *all* hard contact, not just barrels, and collapsed the offense.
/// HR is instead held down by the flattened CPU launch aim below plus the
/// balance harness's asserted slack.)
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

// ── The behind-in-the-count package (TODO 10) ────────────────────────────────
// A passive batter used to milk the CPU for walk chains (~6-10 BB per half):
// the wide base scatter kept missing the zone at any count. The fix is a
// *paired* package — the pitcher tightens up when behind, and the batter
// compensates in the very same counts, so the CPU-vs-CPU economy
// (tests/balance_sim.rs, the arbiter) stays in its bands instead of
// converting the walk PAs straight into strikeouts (the failure of the
// 2026-08-21 pitcher-only attempt).

/// A pitcher behind in the count (2 balls and up) stops nibbling: his aim
/// scatter shrinks to this fraction of the base spread.
pub(crate) fn behind_scatter_scale(balls: u32) -> f32 {
    // A gentle pull, not the original 0.4 (at which ball three was thrown
    // 14 times in 2,446 two-ball pitches and the CPU walked nobody in 100
    // games, 2026-10 realism study): the base scatter stays honest so ball
    // four can exist, while a pitcher at three balls still gets it over
    // often enough that a batter who never swings doesn't walk every time
    // up (`tests/e2e/passive_walks.rs`).
    // Walks are decided almost entirely at three balls, and this dial is
    // sharp: 0.85/0.7 at two/three balls cut the CPU-vs-CPU walk rate from
    // 9.9% to 4.9% of PAs (MLB 8.4%).
    match balls {
        0..=2 => 1.0,
        _ => 0.85,
    }
}

/// Whether the count calls for the get-it-over arsenal (no benders or
/// sweepers — a heater or a changeup over the plate).
pub(crate) fn wants_get_it_over(balls: u32) -> bool {
    balls >= 2
}

/// A hitter ahead in the count sits on the groove he knows is coming: his
/// swing-timing scatter tightens to this fraction. This is the K%/HR
/// counterweight to the pitcher's pull — grooved strikes get barreled, not
/// whiffed.
pub(crate) fn ahead_timing_scale(balls: u32) -> f32 {
    if balls >= 2 { 0.65 } else { 1.0 }
}

/// ...and he stops chasing: out-of-zone offers scale by this fraction when
/// ahead, so the pitches the pitcher still misses with stay taken balls.
pub(crate) fn ahead_chase_scale(balls: u32) -> f32 {
    if balls >= 2 { 0.5 } else { 1.0 }
}

/// With two strikes the hitter protects the plate: anything close is
/// offered at, so the pitcher's chase pitches (`chase_rate`) get chased.
/// Without it the put-away heater above the letters and the curve in the
/// dirt were simply taken, and the count-aware arsenal cut the CPU K rate
/// from 15% to 12.6% of PAs (MLB 22%).
pub(crate) fn two_strike_chase_scale(strikes: u32) -> f32 {
    if strikes >= 2 { 2.2 } else { 1.0 }
}

/// How much wider the timing draw of a swing at a ball is (see `cpu_offense`).
pub(crate) const CHASE_TIMING_SCALE: f32 = 1.8;

/// The per-pitch chance the CPU sends a runner, from a base rate: nothing
/// in a blowout (the running game is a close-game weapon), a bump with two
/// outs or late and close. MLB teams attempt ~0.9 steals per game — roughly
/// one pitch in forty with a runner able to go — against the 28% per pitch
/// the CPU used to run at (3.3 attempts per team-game, 2026-10 study).
pub(crate) fn steal_rate(base: f32, score: &ScoreBoard) -> f32 {
    let margin = score.home_runs.abs_diff(score.away_runs);
    if margin >= 4 {
        return 0.0;
    }
    let mut p = base;
    if score.outs == 2 {
        p *= 1.5;
    }
    if score.inning >= 7 && margin <= 1 {
        p *= 1.5;
    }
    p
}

/// The scatter a pitcher keeps against a hitter who has taken two called
/// strikes without offering at anything all plate appearance, once the
/// count is full or near it: he challenges him. A hitter who swings keeps
/// the honest scatter — this is what stops a completely passive batter from
/// milking walk after walk (`tests/e2e/passive_walks.rs`) without pulling
/// the CPU-vs-CPU walk rate down with it. (Keyed on balls alone it did: the
/// CPU hitter often takes his way to 2-0 or 3-0 — that is patience, not
/// passivity — and the CPU walk rate fell from 8.6% to 3.4% of PAs.)
pub(crate) fn taker_scatter_scale(swung_this_pa: bool, balls: u32, strikes: u32) -> f32 {
    if challenges_taker(swung_this_pa, balls, strikes) {
        0.5
    } else {
        1.0
    }
}

/// Whether the pitcher is challenging a hitter who has not offered all plate
/// appearance (see [`taker_scatter_scale`]).
pub(crate) fn challenges_taker(swung_this_pa: bool, balls: u32, strikes: u32) -> bool {
    !swung_this_pa && balls >= 2 && strikes >= 2
}

/// Whether the pitcher stops calling edge pitches and chases against a
/// hitter who never swings — none this plate appearance, nor in the last
/// two: from two balls on he just throws a strike, the heater or the
/// changeup. Without it the sinkers and sliders in the two-ball mix walked
/// a never-swinging human four times a half (`tests/e2e/passive_walks.rs`).
/// The streak is what keeps it off a patient hitter: keyed on this plate
/// appearance alone it cut the CPU-vs-CPU walk rate from 7% to 3–5%.
pub(crate) fn grooves_for_taker(swung_this_pa: bool, passive_pas: u32, balls: u32) -> bool {
    !swung_this_pa && passive_pas >= 2 && balls >= 2
}

/// How much of the usual in-zone offer rate the hitter keeps by count:
/// the 3-0 green light is rare (MLB hitters swing at ~10% of 3-0 pitches),
/// 3-1 is selective, and the first pitch is often taken too (MLB hitters
/// offer at ~30% of 0-0 pitches, against ~47% overall); 3-2 and the rest
/// are a normal swing. Walks and pitches-per-PA live or die here — a hitter
/// who hacks at 3-0 never draws one, and one who hacks at every 0-0 strike
/// ends a quarter of his plate appearances on one pitch.
pub(crate) fn count_take_scale(balls: u32, strikes: u32) -> f32 {
    match (balls, strikes) {
        (3, 0) => 0.15,
        (3, 1) => 0.7,
        (0, 0) => 0.7,
        _ => 1.0,
    }
}

// ── Count-aware pitch calling ─────────────────────────────────────────────────
// The CPU calls its pitch from the count the way a big-league catcher does
// (per docs/BASEBALL.md, "Pitch mix by count"): heaters when behind, a
// first-pitch fastball, breaking balls and chase pitches once ahead. It used
// to roll one flat 40/25/20/7.5/7.5 table at every count and switch to a
// 55/45 heater-changeup mix at two balls — and since the aim *is* the pitch
// call (`PitchKind::from_aim`), scatter decoded a third of all pitches as
// changeups (2026-10 realism study: CH 34%, against MLB's ~10%).

/// Call weights in [`ARSENAL`] order for a count.
pub(crate) fn pitch_mix(balls: u32, strikes: u32) -> [f32; 5] {
    match (balls, strikes) {
        // The green-light counts: get a fastball over.
        (3, 0) => [80.0, 10.0, 0.0, 0.0, 10.0],
        (2, 0) | (3, 1) => [65.0, 12.0, 3.0, 5.0, 15.0],
        (2, 1) | (1, 0) => [52.0, 12.0, 8.0, 10.0, 18.0],
        // Full count: challenge, mostly with the heater.
        (3, 2) => [58.0, 12.0, 8.0, 8.0, 14.0],
        // Ahead: the put-away pitches.
        (0, 2) | (1, 2) => [40.0, 8.0, 18.0, 20.0, 14.0],
        (0, 1) | (2, 2) => [40.0, 10.0, 18.0, 20.0, 12.0],
        // 0-0 and 1-1: establish the fastball, mix the rest.
        _ => [48.0, 12.0, 10.0, 15.0, 15.0],
    }
}

/// The kinds [`pitch_mix`]'s weights refer to, in order.
pub(crate) const ARSENAL: [PitchKind; 5] = [
    PitchKind::Fastball,
    PitchKind::Sinker,
    PitchKind::Slider,
    PitchKind::Curveball,
    PitchKind::Changeup,
];

/// The pitch a `roll` in 0..1 calls at this count.
pub(crate) fn call_pitch(roll: f32, balls: u32, strikes: u32) -> PitchKind {
    let mix = pitch_mix(balls, strikes);
    let total: f32 = mix.iter().sum();
    let mut acc = 0.0;
    for (kind, w) in ARSENAL.iter().zip(mix) {
        acc += w / total;
        if roll < acc {
            return *kind;
        }
    }
    PitchKind::Fastball
}

/// How often the pitcher deliberately expands the zone — a heater above the
/// letters, a curve in the dirt — when the hitter is in a hole. MLB zone
/// rate drops from ~50% overall to ~30% at 0-2 — but the CPU's breaking
/// balls already live on the edges, so on top of them it only wastes one
/// 0-2 pitch in five (at 0.5 and 0.3 the CPU K rate fell to 11–13%: a
/// wasted pitch is mostly a taken ball, not a strikeout).
pub(crate) fn chase_rate(balls: u32, strikes: u32) -> f32 {
    match (balls, strikes) {
        (0, 2) => 0.2,
        (1, 2) => 0.1,
        _ => 0.0,
    }
}

/// The held aim that throws `kind` where the count wants it: the kind's
/// location bias plus the skill `scatter`, then fitted into the kind's
/// [`PitchKind::from_aim`] region so the pitch called is the pitch thrown —
/// scatter used to turn wide heaters and short sliders into changeups.
pub(crate) fn pitch_aim(kind: PitchKind, scatter: Vec2, balls: u32, chase: bool) -> Vec2 {
    let mut aim = scatter;
    match kind {
        PitchKind::Fastball => {
            aim.y += if chase {
                0.85 // up the ladder, above the letters
            } else if wants_get_it_over(balls) {
                // A get-it-over heater aims mid-zone, not the letters:
                // the standard bias plus backspin lift parks at the top
                // edge, which is exactly the coin-flip high pitch the pull
                // exists to stop throwing.
                0.3
            } else {
                0.55
            };
        }
        PitchKind::Curveball => aim.y -= if chase { 0.85 } else { 0.55 },
        // Slider, sweeping in. Its Magnus sweep carries it ~0.15 m
        // further in than aimed, so this paints the inside edge
        // (x ≈ +0.40 at the plate); the old 0.6 landed on the
        // batter's hip (x ≈ +0.58 ≥ `BATTER_X_MIN`) — 35% of sliders
        // plunked him and every CPU HBP was a slider.
        PitchKind::Slider => aim.x -= 0.2,
        // Sinker, running away — the outside edge, or off it to chase.
        PitchKind::Sinker => aim.x += if chase { 0.6 } else { 0.35 },
        // Changeup: over the plate, or down at the knees to chase.
        PitchKind::Changeup => aim.y -= if chase { 0.3 } else { 0.0 },
    }
    // Never *intend* a pitch at the batter: full-inside aim reaches his hip
    // (`rules::hits_batter`), so the inside edge is the limit. A sweeping
    // slider can still ride in from there — the honest plunking stays, the
    // deliberate one goes.
    let aim = aim.clamp(Vec2::new(-0.45, -1.0), Vec2::splat(1.0));
    fit_aim_to_kind(aim, kind)
}

/// Just past `from_aim`'s 0.35 decode threshold.
const DECODE_EDGE: f32 = 0.36;

/// Fits `aim` into `kind`'s [`PitchKind::from_aim`] region (assumes `aim`
/// already inside the CPU's −0.45 inside-edge clamp). A miss keeps its
/// size and takes the pitch's character instead of changing the pitch: a
/// heater scattered wide also rides up, a curve scattered wide also
/// buries — still a ball, as the miss was. (Clamping the miss back over the
/// plate instead cut the CPU walk rate from 8% to under 5%.)
pub(crate) fn fit_aim_to_kind(aim: Vec2, kind: PitchKind) -> Vec2 {
    // A vertical pitch wider than the threshold must be taller than wide.
    let vertical = |y: f32, x: f32| {
        let x = x.clamp(-0.98, 0.98);
        if x.abs() > 0.35 {
            Vec2::new(x, y.signum() * y.abs().max(x.abs() + 0.02))
        } else {
            Vec2::new(x, y)
        }
    };
    match kind {
        PitchKind::Fastball => vertical(aim.y.max(DECODE_EDGE), aim.x),
        PitchKind::Curveball => vertical(aim.y.min(-DECODE_EDGE), aim.x),
        PitchKind::Slider => {
            let x = aim.x.min(-DECODE_EDGE);
            Vec2::new(x, aim.y.clamp(x, -x))
        }
        PitchKind::Sinker => {
            let x = aim.x.max(DECODE_EDGE);
            Vec2::new(x, aim.y.clamp(-x, x))
        }
        PitchKind::Changeup => aim.clamp(Vec2::splat(-0.35), Vec2::splat(0.35)),
    }
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
    mut pitch_override: ResMut<PitchOverride>,
) {
    let team = score.fielding_team();
    if controllers.source(team) != InputSource::Cpu {
        return;
    }

    // Read the hitter: a fresh count at the wind-up starts a new plate
    // appearance; any press during the pitch is an offer.
    if play.phase == Phase::WindUp
        && score.balls == 0
        && score.strikes == 0
        && cpu.pa_start_seq != Some(cpu.seq)
    {
        if cpu.pa_start_seq.is_some() {
            cpu.passive_pas = if cpu.batter_swung_this_pa {
                0
            } else {
                cpu.passive_pas + 1
            };
        }
        cpu.pa_start_seq = Some(cpu.seq);
        cpu.batter_swung_this_pa = false;
    }
    if play.phase == Phase::Pitch && intents.get(score.batting_team()).action {
        cpu.batter_swung_this_pa = true;
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
            // ~0.1 throws per second of extended lead, deterministic noise.
            // (Was ~0.5: with the offense stretching on 18% of windows that
            // made 2.1 pickoffs per team-game, ~40× MLB's 0.05; 0.25 still
            // picked off 0.7 a game.)
            let p = (0.06 + 0.1 * cfg.skill) * time.delta_secs();
            intent.action = cpu.frame_roll(3.9) < p;
        }
        // Re-arm only a short post-window beat instead of the full
        // 0.7-1.2 s wait: the old full reset stacked on the 1.5 s window
        // for 2.7-3.2 s of held ball before every pitch with runners on
        // (TODO 91). The window itself — the steal race — is untouched.
        cpu.pitch_delay = Timer::from_seconds(0.35, TimerMode::Once);
        return;
    }

    if cpu.pitch_delay.tick(time.delta()).is_finished() {
        // Better skill → tighter aim around the strike zone. The aim then
        // gets a pitch-selection bias: held-aim direction is what picks the
        // kind (see `PitchKind::from_aim`), so shifting the aim is how the
        // CPU "calls" its pitch — heaters up, benders down, sweepers wide.
        let aim = if let Some(kind) = pitch_override.0.take() {
            kind.canonical_aim()
        } else {
            // Behind in the count the scatter pulls in and the call comes
            // from the count (`pitch_mix`; the TODO 10 package's batter
            // compensation below keeps the CPU-vs-CPU bands honest).
            // Wide enough to miss the zone: MLB pitchers land about half of
            // their pitches in it, and the old 0.55 scatter (±0.27 m at the
            // plate against a 0.25 m zone half-width) was a strike machine
            // that never walked anyone.
            let spread = (0.7 * (1.0 - cfg.skill) + 0.2)
                * behind_scatter_scale(score.balls)
                * taker_scatter_scale(cpu.batter_swung_this_pa, score.balls, score.strikes);
            let scatter = Vec2::new(cpu.noise(1.7) * spread, cpu.noise(2.3) * spread * 0.5);
            let challenge =
                grooves_for_taker(cpu.batter_swung_this_pa, cpu.passive_pas, score.balls);
            let kind = if challenge {
                // Against a statue: a strike, the heater or the changeup.
                if cpu.roll(4.3) < 0.7 {
                    PitchKind::Fastball
                } else {
                    PitchKind::Changeup
                }
            } else {
                call_pitch(cpu.roll(4.3), score.balls, score.strikes)
            };
            let chase = !challenge && cpu.roll(4.9) < chase_rate(score.balls, score.strikes);
            pitch_aim(kind, scatter, score.balls, chase)
        };

        let intent = intents.get_mut(team);
        intent.action = true;
        intent.aim = aim;

        // Vary the wait before the next pitch a little.
        let wait = 0.7 + cpu.roll(1.0) * 0.5;
        cpu.pitch_delay = Timer::from_seconds(wait, TimerMode::Once);
    } else {
        intents.get_mut(team).action = false;
    }
}

// ── Offense: the AI bats ──────────────────────────────────────────────────────

// Bevy systems take their dependencies as parameters; the count is inherent.
#[allow(clippy::too_many_arguments)]
pub fn cpu_offense(
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
                let roll = cpu.roll(5.3);
                let p = steal_rate(0.012 + 0.024 * cfg.skill, &score);
                *cpu.window_steal.get_or_insert(roll < p)
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
        let committed = cpu.window_steal == Some(true);
        let roll = cpu.roll(6.1);
        let p = steal_rate(0.018 + 0.024 * cfg.skill, &score);
        let send = steal_candidate(&bases).is_some()
            && (committed || *cpu.steal_call.get_or_insert(roll < p));
        let intent = intents.get_mut(team);
        intent.action = false;
        if send {
            intent.aim = Vec2::new(0.0, -1.0);
        }
        return;
    }
    if play.phase != Phase::Pitch || cpu.decided_swing {
        cpu.window_steal = None;
        let intent = intents.get_mut(team);
        intent.action = false;
        // Once the ball is in play the held swing aim must not linger: the
        // live-ball runner call reads the same stick
        // (`rules::runner_call_from_aim`), so a low launch aim left in place
        // read as "send the batter" on every hit and the CPU ran itself into
        // OUT STRETCHING on a tenth of its plate appearances. The swing's own
        // aim was captured with the press (`SwingCommands`), so clearing it
        // here changes nothing about the contact.
        if play.phase != Phase::Pitch {
            intent.aim = Vec2::ZERO;
        }
        return;
    }
    let Ok((ball, ball_vel)) = ball_q.single() else {
        return;
    };
    let pos = ball.translation;

    // Draw the swing's timing target once per pitch, deterministic on the
    // pitch instant — the CPU's human-like timing scatter (Task B3). Same
    // signed convention as `flow::swing_dt_ms`: negative is early, positive
    // is late, drawn from ±`cpu_timing_spread_ms`.
    let timing_seed = cpu.seed(11.9);
    let mut target_dt = *cpu.swing_target_dt.get_or_insert_with(|| {
        // Ahead in the count the hitter sits on the groove: a tighter
        // timing draw (the batter arm of the TODO 10 package).
        draw_target_dt(
            timing_seed,
            rules.batting.cpu_timing_spread_ms * ahead_timing_scale(score.balls),
        )
    });

    // Decide whether to offer at this pitch — once, and **early**, while the
    // ball is still well in front of the plate (past `SWING_EARLY_Z`, the
    // reachable window's front edge). This is what makes the drawn `target_dt`
    // actually govern the press: the old code committed at a ~0.45 m trigger
    // depth, by which point an early target had nothing left to wait for and
    // collapsed onto that near-zero dt — so the CPU always *connected* and
    // never swung through. Committing out here lets an early draw fire while
    // the ball is still unreachable, i.e. a genuine timing whiff. `decision_z`
    // carries a little skill-scaled jitter so the commit depth varies.
    // Every depth here is where the *bat* would meet the ball if pressed
    // now (`bat_arrival_z`): the swing has a startup (`flow::
    // SWING_CONTACT_SECS`), so the CPU presses that much ahead, and its
    // realized timing at the bat is exactly the drawn target — the balance
    // economy (`tests/balance_sim.rs`) doesn't move with the startup.
    let arrival_z = bat_arrival_z(pos.z, ball_vel.linvel.z);
    if cpu.will_swing.is_none() {
        let decision_z = 6.5 + cpu.noise(3.1) * 1.5;
        if arrival_z > decision_z {
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
        let roll = cpu.roll(5.0);
        let swing = if in_zone {
            // Usually offers at strikes — except on the 3-0 take.
            roll < (0.5 + 0.4 * cfg.skill) * count_take_scale(score.balls, score.strikes)
        } else {
            // Rarely chases balls — half as often when ahead in the count
            // (the patience arm of the TODO 10 package), more with two
            // strikes, when a taken strike is the strikeout.
            roll < 0.28
                * (1.0 - cfg.skill)
                * ahead_chase_scale(score.balls)
                * two_strike_chase_scale(score.strikes)
        };
        cpu.will_swing = Some(swing);
        if swing && !in_zone {
            // A chase is a fooled swing: the timing draw widens, so most
            // chases miss (MLB whiffs on ~45% of swings at balls against
            // ~15% at strikes) — the put-away pitch has to put hitters away.
            target_dt *= CHASE_TIMING_SCALE;
            cpu.swing_target_dt = Some(target_dt);
        }
        if !swing {
            cpu.decided_swing = true;
            intents.get_mut(team).action = false;
            return;
        }
    }

    // Committed to swinging: press when the timing error *the bat will arrive
    // with* (`swing_dt_at_contact_ms` — the press-time read plus the swing's
    // startup) reaches the drawn target, OR at the last frame the swing can
    // still connect (the bat arriving at `late_swing_z`, where the error
    // equals `foul_ms`) — whichever comes first. The error rises from negative
    // (ball out front) through zero toward positive (past the plate). An
    // *early* target fires while the bat would still arrive beyond
    // `SWING_EARLY_Z` → a real swing-through (Whiff). A *late* target fires at
    // the reachable-late edge → an honest FoulTip/Whiff, instead of the ball
    // reaching the take judgment first and being scored a called strike (which
    // is what used to make the CPU's K take-driven rather than whiff-driven).
    let dt_ms = swing_dt_at_contact_ms(pos.z, ball_vel.linvel.z);
    let past_late_edge = arrival_z <= late_swing_z(ball_vel.linvel.z, rules.batting.foul_ms);
    if !ready_to_press(dt_ms, target_dt) && !past_late_edge {
        intents.get_mut(team).action = false;
        return;
    }

    cpu.decided_swing = true;
    let intent = intents.get_mut(team);
    intent.action = true;
    // Spread the intended launch from choppers through line drives to flies
    // (aim.y −1.0..0.8 → a −4°..28° base launch before the timing tilt tops
    // or lifts it), so the batted-ball mix lands near the real ~40% grounders
    // / 40% liners+flies split instead of a wall of 10°–20° line drives (the
    // 2026-10 realism harness measured a .45 BABIP off the old narrow aim).
    // The launch aim is the CPU's home-run dial — a towering uppercut clears
    // the regulation fence — so the top of the stick is left to humans and
    // HR/9 is checked in tests/balance_sim.rs. Horizontal spray is unchanged.
    intent.aim = Vec2::new(cpu.noise(7.0) * 0.6, cpu.noise(9.0) * 0.9 - 0.1);
}

#[cfg(test)]
#[path = "ai.test.rs"]
mod tests;
