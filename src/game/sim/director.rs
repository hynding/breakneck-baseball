//! The Director — scripted or CPU control of every player slot.
//!
//! This is the automation seam: it writes [`Intents`] at the exact same
//! schedule point the e2e tests always have (the [`DriveGame`] schedule,
//! after the input plugin's `PreUpdate` clear and before `Update`), so it
//! works identically headless, native windowed, and wasm — and because
//! every control mechanism converges on `Intents` → the batting adapters'
//! `SwingCommands`, a control mechanism added in the future is covered
//! automatically.
//!
//! **Completely inert unless enabled**: without the [`Director`] resource
//! nothing here writes a single intent; the default policy for any slot is
//! [`Policy::Human`] (pass through real input). Scripts are *data* — an
//! enum vocabulary of conditions and actions, RON-loadable from
//! `tests/scripts/` and reusable by name — so the same script drives
//! Classic, Swing Meter, and PCI batting: the script says *swing now* and
//! the per-style translation stays in `batting::adapt_swings`, exactly like
//! a human thumb.

use bevy::app::MainScheduleOrder;
// Anonymous: our script `Condition` enum shadows the bevy trait's name,
// but the trait must stay in scope for `.and(...)` on run conditions.
use bevy::ecs::schedule::Condition as _;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;
use serde::{Deserialize, Serialize};

use crate::game::ball::Baseball;
use crate::game::batting::style_for;
use crate::game::fielding::ActivePlay;
use crate::game::flow::{Phase, Play, swing_dt_ms};
use crate::game::input::{Controllers, InputSource, Intents, KeyScheme, TeamIntent};
use crate::game::settings::{BattingStyle, Settings};
use crate::game::{GameState, ScoreBoard, Team};

// ── The injection schedule ────────────────────────────────────────────────────

/// Runs after `PreUpdate` (the input plugin has refreshed keyboard/gamepad
/// intents) and before `Update` (flow reads what was written) — the one
/// injection point for synthetic input. Registered by [`DirectorPlugin`];
/// the e2e harness adds its own driver systems to it.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct DriveGame;

// ── Policies ──────────────────────────────────────────────────────────────────

/// Who controls one team slot.
#[derive(Clone, Debug, Default)]
pub enum Policy {
    /// Real input passes through untouched (the default; fully inert).
    #[default]
    Human,
    /// The existing CPU AI drives the slot (attract mode on a human slot).
    /// The director routes the slot's [`InputSource`] to `Cpu` so the AI
    /// systems adopt it; the CPU always bats Classic, as everywhere.
    Cpu,
    /// A data script drives the slot through `Intents`.
    Scripted(Script),
}

/// Per-slot control policies. Insert to enable the director; absent, the
/// game is untouched.
#[derive(Resource, Clone, Debug, Default)]
pub struct Director {
    pub home: Policy,
    pub away: Policy,
}

impl Director {
    pub fn policy(&self, team: Team) -> &Policy {
        match team {
            Team::Home => &self.home,
            Team::Away => &self.away,
        }
    }
}

// ── Script data ───────────────────────────────────────────────────────────────

/// Coach-free mirror of [`Phase`] so scripts serialize without depending on
/// flow internals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptPhase {
    PrePitch,
    WindUp,
    Pitch,
    InPlay,
    Result,
}

impl ScriptPhase {
    fn matches(self, phase: Phase) -> bool {
        matches!(
            (self, phase),
            (ScriptPhase::PrePitch, Phase::PrePitch)
                | (ScriptPhase::WindUp, Phase::WindUp)
                | (ScriptPhase::Pitch, Phase::Pitch)
                | (ScriptPhase::InPlay, Phase::InPlay)
                | (ScriptPhase::Result, Phase::Result)
        )
    }
}

/// A throw destination, mapped to the same stick convention a human uses
/// (screen right = first, up = second, left = third, down = home).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaseSel {
    First,
    Second,
    Third,
    Home,
}

impl BaseSel {
    fn aim(self) -> Vec2 {
        match self {
            BaseSel::First => Vec2::new(1.0, 0.0),
            BaseSel::Second => Vec2::new(0.0, 1.0),
            BaseSel::Third => Vec2::new(-1.0, 0.0),
            BaseSel::Home => Vec2::new(0.0, -1.0),
        }
    }
}

/// When a reactive rule applies. Timed-only scripts can't play baseball —
/// these conditions are how a script reacts to the live game.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Condition {
    Always,
    Phase(ScriptPhase),
    /// This slot is batting.
    OnOffense,
    /// This slot is fielding.
    OnDefense,
    /// The pre-pitch steal window is open.
    InStealWindow,
    /// The live pitch is within `early_ms` of the plate: the signed swing
    /// timing error has risen to `-early_ms` (0 = dead-on, rising late).
    PlateEta {
        early_ms: f32,
    },
    /// A fielder has gathered the ball and is holding it.
    BallGathered,
    /// The current phase has run at least this long (pacing).
    PhaseElapsed {
        at_least: f32,
    },
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
}

/// What a matched rule does to this frame's intent.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ScriptAction {
    /// Hold the stick at (x, y) — pitch aim, swing aim, lead stretch.
    Aim { x: f32, y: f32 },
    /// Raw action edge (pitch release, pickoff, PCI/Classic press).
    Press,
    /// Raw action level (the Swing Meter's load, without the edge).
    HoldPress,
    /// Style-aware swing commit: the director synthesizes whatever input
    /// pattern the slot's batting style needs (press for Classic/PCI, a
    /// load-and-release for Swing Meter); the adapter still grades it.
    Swing,
    /// Aim at a base and press — the manual defensive throw.
    ThrowTo(BaseSel),
}

/// A timed intent: applied while game time (since game start) is inside
/// `[at, at + hold)`. Reactive rules do the real playing; timed steps exist
/// for boot choreography.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimedStep {
    pub at: f32,
    pub hold: f32,
    #[serde(default)]
    pub aim: Option<(f32, f32)>,
    #[serde(default)]
    pub press: bool,
}

/// A reactive rule: while `when` holds, `then` applies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    pub when: Condition,
    pub then: ScriptAction,
}

/// One slot's play-book: timed steps plus reactive rules, expressed as data
/// so scripts live in `.ron` files (`tests/scripts/`) and load by name.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Script {
    pub name: String,
    #[serde(default)]
    pub steps: Vec<TimedStep>,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Script {
    /// Whether any rule can commit a swing — the Swing Meter arm holds its
    /// load only for scripts that will actually release it.
    fn can_swing(&self) -> bool {
        self.rules
            .iter()
            .any(|r| matches!(r.then, ScriptAction::Swing))
    }
}

// ── Named script registry ─────────────────────────────────────────────────────

/// The named scripts, embedded from `tests/scripts/*.ron` at compile time
/// so the same registry works headless, native, and wasm (no filesystem).
const BUILTIN_SCRIPTS: &[(&str, &str)] = &[
    (
        "balanced",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/scripts/balanced.ron"
        )),
    ),
    (
        "take-all",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/scripts/take-all.ron"
        )),
    ),
    (
        "steal-artist",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/scripts/steal-artist.ron"
        )),
    ),
];

/// Loads a named script from the embedded registry. Panics on a malformed
/// embedded script — that is a build error, not a runtime condition.
pub fn script(name: &str) -> Option<Script> {
    BUILTIN_SCRIPTS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(n, text)| {
            ron::from_str(text).unwrap_or_else(|e| panic!("embedded script {n} is malformed: {e}"))
        })
}

// ── Runtime ───────────────────────────────────────────────────────────────────

/// The director's own bookkeeping — per-slot swing state and phase timing.
/// Always present; meaningless while no [`Director`] is inserted.
#[derive(Resource, Default)]
pub struct DirectorRuntime {
    game_started_at: f32,
    phase_started_at: f32,
    last_phase: Option<Phase>,
    home: SlotState,
    away: SlotState,
}

#[derive(Default)]
struct SlotState {
    /// A Swing action already fired this pitch (one commit per pitch).
    swing_fired: bool,
    /// A manual throw was already ordered this play.
    throw_fired: bool,
}

impl DirectorRuntime {
    fn slot_mut(&mut self, team: Team) -> &mut SlotState {
        match team {
            Team::Home => &mut self.home,
            Team::Away => &mut self.away,
        }
    }
}

fn mark_game_start(time: Res<Time>, mut runtime: ResMut<DirectorRuntime>) {
    *runtime = DirectorRuntime {
        game_started_at: time.elapsed_secs(),
        phase_started_at: time.elapsed_secs(),
        ..Default::default()
    };
}

/// Everything a condition may ask about the live game.
struct Ctx {
    phase: Phase,
    on_offense: bool,
    in_steal_window: bool,
    phase_elapsed: f32,
    /// Signed swing-timing error of the live pitch (early = negative).
    dt_ms: Option<f32>,
    gathered: bool,
}

fn eval(cond: &Condition, ctx: &Ctx) -> bool {
    match cond {
        Condition::Always => true,
        Condition::Phase(p) => p.matches(ctx.phase),
        Condition::OnOffense => ctx.on_offense,
        Condition::OnDefense => !ctx.on_offense,
        Condition::InStealWindow => ctx.in_steal_window,
        Condition::PlateEta { early_ms } => ctx.dt_ms.is_some_and(|dt| dt >= -early_ms),
        Condition::BallGathered => ctx.gathered,
        Condition::PhaseElapsed { at_least } => ctx.phase_elapsed >= *at_least,
        Condition::All(cs) => cs.iter().all(|c| eval(c, ctx)),
        Condition::Any(cs) => cs.iter().any(|c| eval(c, ctx)),
        Condition::Not(c) => !eval(c, ctx),
    }
}

/// Routes each slot's [`InputSource`] to match its policy: `Cpu` slots to
/// the AI, `Scripted` slots to a keyboard scheme (so the CPU systems ignore
/// them and `batting::style_for` applies the slot's configured style —
/// scripted input is pseudo-human by design). `Human` slots are untouched.
fn enforce_routing(director: Res<Director>, mut controllers: ResMut<Controllers>) {
    for (team, scheme) in [
        (Team::Home, KeyScheme::Primary),
        (Team::Away, KeyScheme::Secondary),
    ] {
        let slot = match team {
            Team::Home => &mut controllers.home,
            Team::Away => &mut controllers.away,
        };
        match director.policy(team) {
            Policy::Human => {}
            Policy::Cpu => {
                if *slot != InputSource::Cpu {
                    *slot = InputSource::Cpu;
                }
            }
            Policy::Scripted(_) => {
                if *slot == InputSource::Cpu {
                    *slot = InputSource::Keyboard(scheme);
                }
            }
        }
    }
}

/// The director's frame: evaluate each scripted slot's play-book and write
/// its [`TeamIntent`] — overwriting whatever the real input layer produced
/// for that slot, exactly as a test driver would.
#[allow(clippy::too_many_arguments)]
fn direct(
    time: Res<Time>,
    director: Res<Director>,
    mut runtime: ResMut<DirectorRuntime>,
    controllers: Res<Controllers>,
    settings: Res<Settings>,
    play: Res<Play>,
    score: Res<ScoreBoard>,
    active: Res<ActivePlay>,
    ball_q: Query<(&Transform, &Velocity), With<Baseball>>,
    mut intents: ResMut<Intents>,
) {
    let now = time.elapsed_secs();
    if runtime.last_phase != Some(play.phase) {
        runtime.last_phase = Some(play.phase);
        runtime.phase_started_at = now;
        if play.phase == Phase::PrePitch {
            runtime.home = SlotState::default();
            runtime.away = SlotState::default();
        }
    }

    let dt_ms = (play.phase == Phase::Pitch)
        .then(|| {
            ball_q
                .get_single()
                .ok()
                .map(|(tf, vel)| swing_dt_ms(tf.translation.z, vel.linvel.z))
        })
        .flatten();

    for team in [Team::Home, Team::Away] {
        let Policy::Scripted(script) = director.policy(team) else {
            continue;
        };
        let ctx = Ctx {
            phase: play.phase,
            on_offense: score.batting_team() == team,
            in_steal_window: play.in_steal_window(),
            phase_elapsed: now - runtime.phase_started_at,
            dt_ms,
            gathered: active.holding_since().is_some(),
        };
        let style = style_for(team, &controllers, &settings);
        let game_t = now - runtime.game_started_at;

        let mut intent = TeamIntent::default();
        for step in &script.steps {
            if (step.at..step.at + step.hold).contains(&game_t) {
                if let Some((x, y)) = step.aim {
                    intent.aim = Vec2::new(x, y);
                }
                if step.press {
                    intent.action = true;
                    intent.action_held = true;
                }
            }
        }

        let mut swing_now = false;
        for rule in &script.rules {
            if !eval(&rule.when, &ctx) {
                continue;
            }
            match rule.then {
                ScriptAction::Aim { x, y } => intent.aim = Vec2::new(x, y),
                ScriptAction::Press => {
                    intent.action = true;
                    intent.action_held = true;
                }
                ScriptAction::HoldPress => intent.action_held = true,
                ScriptAction::Swing => swing_now = true,
                ScriptAction::ThrowTo(base) => {
                    let state = runtime.slot_mut(team);
                    if ctx.gathered && !state.throw_fired {
                        state.throw_fired = true;
                        intent.aim = base.aim();
                        intent.action = true;
                        intent.action_held = true;
                    }
                }
            }
        }

        // The style-aware swing: the script says *swing now*; the director
        // is the thumb, the adapter still translates per style.
        let state = runtime.slot_mut(team);
        match style {
            BattingStyle::SwingMeter => {
                // Load through the pitch, release at the commit. Scripts
                // that never swing never load (a take stays a take).
                if ctx.on_offense
                    && ctx.phase == Phase::Pitch
                    && script.can_swing()
                    && !state.swing_fired
                {
                    if swing_now {
                        state.swing_fired = true;
                        intent.action_held = false; // release fires the meter
                    } else {
                        intent.action_held = true;
                    }
                }
            }
            BattingStyle::ClassicTiming | BattingStyle::PciCursor => {
                if swing_now && ctx.on_offense && !state.swing_fired {
                    state.swing_fired = true;
                    intent.action = true;
                    intent.action_held = true;
                }
            }
        }

        *intents.get_mut(team) = intent;
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct DirectorPlugin;

impl Plugin for DirectorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DirectorRuntime>();
        app.init_schedule(DriveGame);
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .insert_after(PreUpdate, DriveGame);
        app.add_systems(crate::game::game_start(), mark_game_start)
            .add_systems(
                DriveGame,
                (enforce_routing, direct)
                    .chain()
                    .run_if(in_state(GameState::Playing).and(resource_exists::<Director>)),
            );
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx {
            phase: Phase::Pitch,
            on_offense: true,
            in_steal_window: false,
            phase_elapsed: 0.5,
            dt_ms: Some(-20.0),
            gathered: false,
        }
    }

    #[test]
    fn plate_eta_fires_once_the_error_reaches_the_threshold() {
        let cond = Condition::PlateEta { early_ms: 10.0 };
        // dt −20 ms: the ball is still 20 ms out — a 10 ms trigger waits.
        assert!(!eval(&cond, &ctx()));
        let mut c = ctx();
        c.dt_ms = Some(-10.0);
        assert!(eval(&cond, &c));
        c.dt_ms = Some(5.0);
        assert!(eval(&cond, &c));
        // No live ball: never fires.
        c.dt_ms = None;
        assert!(!eval(&cond, &c));
    }

    #[test]
    fn boolean_combinators_compose() {
        let c = ctx();
        let both = Condition::All(vec![
            Condition::OnOffense,
            Condition::Phase(ScriptPhase::Pitch),
        ]);
        assert!(eval(&both, &c));
        let neither = Condition::All(vec![
            Condition::OnDefense,
            Condition::Phase(ScriptPhase::Pitch),
        ]);
        assert!(!eval(&neither, &c));
        assert!(eval(&Condition::Not(Box::new(neither.clone())), &c));
        assert!(eval(&Condition::Any(vec![neither, both]), &c));
    }

    #[test]
    fn every_builtin_script_parses() {
        for (name, _) in BUILTIN_SCRIPTS {
            let s = script(name).expect("registered");
            assert!(
                !s.rules.is_empty() || !s.steps.is_empty(),
                "{name} is empty"
            );
        }
    }

    #[test]
    fn base_aims_match_the_stick_convention() {
        // Same mapping rules::aimed_base reads back (screen right = first).
        use crate::game::rules::aimed_base;
        use crate::game::variant::VariantId;
        let f = VariantId::Standard.field();
        assert_eq!(aimed_base(BaseSel::First.aim(), &f), Some(0));
        assert_eq!(aimed_base(BaseSel::Second.aim(), &f), Some(1));
        assert_eq!(aimed_base(BaseSel::Third.aim(), &f), Some(2));
        assert_eq!(aimed_base(BaseSel::Home.aim(), &f), Some(f.base_count()));
    }
}
