//! Game flow — the real-time at-bat/play state machine.
//!
//! Within [`GameState::Playing`] the game cycles through a small [`Phase`]
//! machine for each pitch:
//!
//! ```text
//! PrePitch --release--> Pitch --contact--> InPlay --resolved--> Result --> PrePitch
//!                          \--take/miss--> (count) --> Result --> PrePitch
//! ```
//!
//! All baseball *rules* (base running, the count, game-over, the live-play
//! races) live in [`crate::game::rules`] as pure, unit-tested functions; this
//! module reads input, drives the phases and timers, and translates rule
//! results into banners and state transitions. Contact settles only what
//! physics settles (a ball over the fence — see [`rules::classify_contact`]);
//! everything else stays **live**: the fielding simulation reports what
//! happens on the grass ([`LiveBallEvent`]) and [`resolve_live_play`] turns
//! those reports into the call via kinematic runner-vs-throw races.
//!
//! The phase machine is split by where each phase's work happens: [`pitch`]
//! owns everything up to and including the swing/take judgment (PrePitch,
//! WindUp, Pitch), [`live`] owns the live-ball chase once contact puts a ball
//! in play (InPlay), and [`result`] owns the result pause and the shared
//! rule-result → banner translation. This module keeps the shared state
//! ([`Play`], [`Phase`], the events) and the plugin wiring.

use bevy::prelude::*;

use crate::game::ai::{CpuConfig, CpuState, cpu_defense, cpu_offense};
use crate::game::rules::{self, Bases, BattingOrder, Outcome};
use crate::game::variant::Ruleset;
use crate::game::{GameState, Team};

mod live;
mod pitch;
mod result;

pub(crate) use pitch::{late_swing_z, swing_dt_ms};

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Seconds the result banner lingers before the next pitch. Live gameplay
/// reads this off `Ruleset.pace.result_secs`; `Play::default()` keeps this
/// const as its bootstrap value (no resource access at construction), and
/// it's `PaceTuning::default()`'s source of truth.
pub(crate) const RESULT_SECS: f32 = 1.2;
/// Minimum seconds between pickoff throws — the arm has to reload, so a held
/// button can't machine-gun the bag. Live gameplay reads this off
/// `Ruleset.pace.pickoff_cooldown_secs`; this const only remains as
/// `PaceTuning::default()`'s source of truth.
pub(crate) const PICKOFF_COOLDOWN_SECS: f32 = 0.9;

// ── Phase state ───────────────────────────────────────────────────────────────

/// The current step of an at-bat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    /// Waiting for the defense to throw a pitch.
    #[default]
    PrePitch,
    /// The pitcher's delivery is playing out; the ball hasn't left the hand.
    WindUp,
    /// Ball is travelling to the plate; the batter may swing.
    Pitch,
    /// The ball has been hit and is live.
    InPlay,
    /// A short pause showing the result before the next pitch.
    Result,
}

/// Runtime state for the play machine.
#[derive(Resource)]
pub struct Play {
    pub phase: Phase,
    timer: Timer,
    /// Plate-crossing point (x, y), recorded once as the pitch passes the plate.
    crossing: Option<Vec2>,
    resolved: bool,
    /// Aim + selected kind stored at windup start, released as the pitch when
    /// the delivery ends.
    pending_pitch: Option<(Vec2, rules::PitchKind)>,
    /// The kind of the pitch currently in flight (set at release). Drives the
    /// dropped-third-strike and steal resolutions.
    live_kind: Option<rules::PitchKind>,
    /// The batting side sent the lead runner as the delivery started
    /// (aim held down through the windup).
    steal_armed: bool,
    /// The armed steal broke from an extended pre-pitch lead — a jump no
    /// throw beats (the pickoff was the defense's counter).
    big_jump: bool,
    /// The lead was stretched *during* the steal window — the only extension
    /// that earns the guaranteed jump, because it was the only one exposed
    /// to the pickoff. Stretching after the window is a plain late break.
    window_lead: bool,
    /// The pre-pitch steal window: while running, the pitch is gated and the
    /// leadoff/pickoff duel is live. Zero-length when nobody can steal.
    hold: Timer,
    /// Reload time between pickoff throws.
    pickoff_cooldown: Timer,
    /// The last pitch ended untouched (take / swing-through): the ball is on
    /// its way to the catcher's mitt, and [`catcher_receives`] may stop it.
    pitch_taken: bool,
    /// A pitch reached the catcher's glove before the take/swing was
    /// logically judged (the timing window can stay open well past the
    /// catcher now — see `late_swing_z`) and was hidden there for
    /// presentation, ahead of the official freeze. Cleared whenever the ball
    /// turns out *not* to be caught after all (a legitimately late hit, a
    /// dropped third, HBP) so it can't linger invisible.
    presentational_catch: bool,
    /// The catcher received this at-bat's last pitch in the mitt (either
    /// the presentational glove-hide or the official freeze) — the camera
    /// holds the tight at-bat framing through the result pause instead of
    /// zooming out. Never set by dirt balls, sailed pitches, HBP, or
    /// anything hit; cleared at the PrePitch reset.
    pitch_gloved: bool,
    /// A call decided by the throw race but not yet announced: the ball is
    /// still in the air, and the play stays visually alive (the throw flies,
    /// the batter rounds the bases) until fielding reports it settled.
    pending_call: Option<Outcome>,
    /// `Time::elapsed_secs` at contact — the live-play race clock's zero.
    contact_at: f32,
    /// A wall carom has already been called this play (one banner per play).
    wall_called: bool,
    /// The quality of the most recent judged swing this at-bat (any swing,
    /// whiff included), stashed for presentation — the HR fireworks scale up
    /// off a dead-on Perfect. Cleared at reset (per at-bat).
    last_contact_quality: Option<rules::ContactQuality>,
    /// This play is a home run: set at contact, held through the trot and the
    /// result pause (so the camera can orbit the trot), cleared at reset.
    home_run: bool,
}

impl Play {
    /// Whether the current play's call has already been made (home runs at
    /// contact; live balls once [`resolve_live_play`] rules). The camera uses
    /// this to pick between play-framing and trot-following shots.
    pub fn is_resolved(&self) -> bool {
        self.resolved
    }

    /// Seconds since contact, given the current `Time::elapsed_secs` — the
    /// live-play race clock the fielding choreography and rules share.
    pub fn since_contact(&self, now: f32) -> f32 {
        now - self.contact_at
    }

    /// Whether the batting side sent the runners with the windup (the
    /// hit-and-run jump); read by the throw races.
    pub fn runners_going(&self) -> bool {
        self.steal_armed
    }

    /// Whether the pre-pitch steal window is still open: the pitch is held,
    /// leads may stretch, and a defensive action is a pickoff throw.
    pub fn in_steal_window(&self) -> bool {
        !self.hold.finished()
    }

    /// The hit the umpire has already decided but not yet announced (the
    /// throw is still in the air): `Some(bases)` lets the runner rigs break
    /// for the bases they've earned while the play finishes.
    pub fn pending_hit(&self) -> Option<u32> {
        match self.pending_call {
            Some(Outcome::Hit(n)) => Some(n),
            _ => None,
        }
    }

    /// The most recent judged swing's quality this at-bat (any swing), or
    /// `None` before the first swing. Read by the home-run fireworks.
    pub fn last_contact_quality(&self) -> Option<rules::ContactQuality> {
        self.last_contact_quality
    }

    /// The hit/out already decided but not yet announced (the throw is still
    /// in the air), for debug readouts.
    pub fn pending_call(&self) -> Option<Outcome> {
        self.pending_call
    }

    /// Seconds left in the pre-pitch steal window, for debug readouts.
    pub fn steal_window_remaining(&self) -> f32 {
        self.hold.remaining_secs()
    }

    /// Whether the live/just-finished play is a home run — held from contact
    /// through the trot and the result pause so the camera can orbit it.
    pub fn is_home_run(&self) -> bool {
        self.home_run
    }

    /// Whether the catcher gloved this at-bat's last pitch — read by the
    /// camera to hold the at-bat framing through the result pause.
    pub fn pitch_gloved(&self) -> bool {
        self.pitch_gloved
    }

    /// Test-only constructor for camera/flow unit tests that need a `Play`
    /// in a given phase without driving the whole machine there.
    #[cfg(test)]
    pub fn test_play(phase: Phase, pitch_gloved: bool) -> Self {
        Self {
            phase,
            pitch_gloved,
            ..Self::default()
        }
    }

    /// Test-only: force the phase directly, without driving the machine
    /// there — used to exercise `scenario_safe`'s refusal while live.
    #[cfg(test)]
    pub fn force_phase_for_test(&mut self, phase: Phase) {
        self.phase = phase;
    }

    /// The ball is dead: a scenario may safely rewrite the game state.
    pub fn scenario_safe(&self) -> bool {
        matches!(self.phase, Phase::PrePitch | Phase::Result)
    }

    /// Resets to a fresh at-bat over the given base state — the scenario
    /// library's seam ([`crate::game::scenario::apply_to_world`]).
    pub fn reset_for_scenario(&mut self, bases: &Bases, rules: &Ruleset) {
        *self = Play::default();
        self.hold = pitch::steal_window_for(bases, rules);
    }
}

/// The live leadoff state, shared with the runner visuals and the CPU: the
/// offense holding Down stretches the lead runner off the bag — arming the
/// guaranteed steal jump, and offering the pickoff.
#[derive(Resource, Default)]
pub struct LeadState {
    pub extended: bool,
}

impl Default for Play {
    fn default() -> Self {
        Self {
            phase: Phase::PrePitch,
            timer: Timer::from_seconds(RESULT_SECS, TimerMode::Once),
            crossing: None,
            resolved: false,
            pending_pitch: None,
            live_kind: None,
            steal_armed: false,
            big_jump: false,
            window_lead: false,
            hold: Timer::from_seconds(0.0, TimerMode::Once),
            pickoff_cooldown: Timer::from_seconds(0.0, TimerMode::Once),
            pitch_taken: false,
            presentational_catch: false,
            pitch_gloved: false,
            pending_call: None,
            contact_at: 0.0,
            wall_called: false,
            last_contact_quality: None,
            home_run: false,
        }
    }
}

// ── Events ────────────────────────────────────────────────────────────────────

/// The emotional register of a banner. Flow decides *what happened*; the UI
/// maps the tone onto the active theme's palette — flow knows no colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerTone {
    /// The batter came out ahead (hits).
    Good,
    /// The batter was retired (outs, strikeouts).
    Bad,
    /// Routine count traffic (balls, strikes, fouls).
    Info,
    /// The big moments (home runs, walks forced in).
    Epic,
}

/// Fired once per contact put in play: what physics alone settled (home run
/// or live ball) plus the predicted landing point. Fielder and runner
/// choreography key off this — the *call* comes later, from the live play.
#[derive(Event, Clone, Copy)]
pub struct BallInPlayEvent {
    pub kind: rules::ContactKind,
    pub landing: Vec3,
    /// The baserunning shape of the ball off the bat (grounder / catchable fly
    /// / deep fly), for the runner rigs' break reads. Cosmetic only — the call
    /// still comes from the live-play races. Meaningful for a fair live ball.
    pub contact_class: rules::ContactClass,
}

/// Physical reports from the fielding simulation. Fielding never touches the
/// score or bases — it says what happened on the grass, and the rules decide
/// what it means.
#[derive(Event, Clone, Copy)]
pub enum LiveBallEvent {
    /// Gloved on the fly at `pos` (before the first bounce).
    Caught { pos: Vec3 },
    /// First bounce at `pos` — the fair/foul call point.
    Landed { pos: Vec3 },
    /// The gathered ball was thrown from `pos` at `base` (`base_count()` =
    /// home), `race_time` seconds after contact on the shared race clock.
    /// Auto-throws backdate `race_time` to the gather instant (the analytic
    /// defense throws promptly); manual throws pay for every held moment.
    Thrown {
        pos: Vec3,
        base: usize,
        race_time: f32,
    },
    /// The thrown ball (relay leg included) has been received and the play
    /// is physically dead — the cue [`resolve_live_play`] waits for before
    /// announcing a call decided at the throw.
    Settled,
}

/// The pitch ended untouched and the catcher gloved it — cosmetic (the call
/// was already made from the crossing), fired for the glove-pop sound.
#[derive(Event, Clone, Copy)]
pub struct PitchCaughtEvent;

/// A judged swing: fired on *every* swing the batter offers at a pitch,
/// whiffs included. Carries the graded [`rules::ContactQuality`], who was
/// batting, and the signed swing timing (`dt_ms`, early = negative) so
/// presentation systems (fx/audio/camera, later tasks) can react without
/// re-deriving the timing. The rules/physics consequence is applied at the
/// swing site in [`pitch::pitch_live`]; this event is a read-only report.
#[derive(Event, Clone, Copy)]
pub struct ContactEvent {
    pub quality: rules::ContactQuality,
    pub batting_team: Team,
    pub dt_ms: f32,
}

/// A transient on-screen message (e.g. "STRIKE!", "BALL", "HOME RUN!").
#[derive(Event, Clone)]
pub struct PlayBanner {
    pub text: String,
    pub tone: BannerTone,
}

impl PlayBanner {
    fn new(text: impl Into<String>, tone: BannerTone) -> Self {
        Self {
            text: text.into(),
            tone,
        }
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

/// Marks the flow chain below — `pre_pitch`, `wind_up`, `pitch_live`,
/// `in_play`, `resolve_live_play`, and `result_phase` all mutate
/// `Play::phase` somewhere in this tuple. Consumers that read `Play::phase`
/// and must see this frame's flip (not last frame's) order
/// `.after(PhaseSet)` — the `player::IdentitySet` pattern. In particular
/// `player::PlayerPlugin`'s batter chain needs this: `batter_stance`'s
/// continuation-cut arm (and `batter_fidgets`, and `trigger_swing`'s
/// stance-only swing gate) must see the *same-frame* `PrePitch -> WindUp`
/// flip `pre_pitch` writes, or the multi-threaded executor's ambiguous
/// worker-timing tie-break can run the batter chain first and leave a
/// fidget clip in place for a frame right at the windup.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct PhaseSet;

pub struct FlowPlugin;

impl Plugin for FlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Play>()
            .init_resource::<Bases>()
            .init_resource::<BattingOrder>()
            .init_resource::<LeadState>()
            .init_resource::<CpuConfig>()
            .init_resource::<CpuState>()
            .add_event::<BallInPlayEvent>()
            .add_event::<LiveBallEvent>()
            .add_event::<PitchCaughtEvent>()
            .add_event::<ContactEvent>()
            .add_event::<PlayBanner>()
            .add_systems(crate::game::game_start(), pitch::reset_flow)
            .add_systems(
                Update,
                // CPU intent is written first so pitching/batting see it this
                // frame. `adapt_swings` sits after `wind_up` (not right after
                // `cpu_offense`) so it reads the *post-flip* phase on the
                // exact frame `wind_up` moves WindUp -> Pitch — otherwise a
                // human's one-frame `action` edge lands while the adapter
                // still sees the stale WindUp phase and gets silently
                // dropped (pitch_live used to run after wind_up too, so this
                // restores that same-frame delivery). It still reads
                // `cpu_offense`'s intent write from earlier this frame.
                (
                    cpu_defense,
                    cpu_offense,
                    pitch::pre_pitch,
                    pitch::wind_up,
                    crate::game::batting::adapt_swings,
                    pitch::pitch_live,
                    pitch::catcher_receives,
                    live::in_play,
                    live::announce_wall_bang,
                    live::resolve_live_play,
                    result::result_phase,
                )
                    .chain()
                    .in_set(PhaseSet)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gloved pitch is remembered through the result pause (the camera
    /// holds the duel framing on it — `camera::duel_framing_wanted`) and a
    /// fresh `Play` starts unglooved.
    #[test]
    fn pitch_gloved_defaults_false_and_reads_back() {
        let play = Play::default();
        assert!(!play.pitch_gloved());
        let play = Play::test_play(Phase::Result, true);
        assert!(play.pitch_gloved());
    }
}
