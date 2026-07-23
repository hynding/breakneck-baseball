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

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::ai::{cpu_defense, cpu_offense, CpuConfig, CpuState};
use crate::game::animation::{AnimClip, Playing};
use crate::game::ball::{Baseball, HitEvent, InFlight, PitchEvent, WallBangEvent};
use crate::game::input::Intents;
use crate::game::player::{CatcherRole, Pitcher};
use crate::game::rules::{
    self, BallCall, Bases, BattingOrder, OutKind, Outcome, StealResult, StrikeCall,
};
use crate::game::runner::RunnersSettled;
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::{GameState, ScoreBoard};

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Z of home plate (the batter stands here).
const PLATE_Z: f32 = 0.0;

/// A swing connects while the ball is within this Z band of the plate.
const SWING_LATE_Z: f32 = -1.2; // ball this far past the plate = window closed
const SWING_EARLY_Z: f32 = 3.2; // ball this far in front = earliest contact
/// Maximum horizontal miss the batter can still reach.
const SWING_REACH_X: f32 = 1.8;

/// Seconds the result banner lingers before the next pitch.
const RESULT_SECS: f32 = 1.2;
/// Extra seconds the result pause will wait for runner rigs to finish their
/// paths (the home-run trot, a first-to-third sprint) before the next batter
/// steps in — a hard cap so a stray path can never stall the game.
const RESULT_SETTLE_CAP: f32 = 20.0;
/// Minimum seconds between pickoff throws — the arm has to reload, so a held
/// button can't machine-gun the bag.
const PICKOFF_COOLDOWN_SECS: f32 = 0.9;
/// Home-run trot window: hang time plus a little air, clamped.
const INPLAY_BUFFER: f32 = 1.2;
const INPLAY_MIN: f32 = 2.2;
const INPLAY_MAX: f32 = 6.5;
/// Hard cap on an unresolved live play: hang time plus chase-and-throw room.
/// If nothing has resolved by then, the play is called from the current ball
/// state so the game can never stall.
const LIVE_PLAY_BUFFER: f32 = 5.0;
const LIVE_PLAY_MIN: f32 = 4.0;
const LIVE_PLAY_MAX: f32 = 11.0;

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
    /// `Time::elapsed_secs` at contact — the live-play race clock's zero.
    contact_at: f32,
    /// A wall carom has already been called this play (one banner per play).
    wall_called: bool,
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
            contact_at: 0.0,
            wall_called: false,
        }
    }
}

/// The steal window a fresh at-bat opens: the ruleset's duel length whenever
/// a runner is actually in a position to steal ([`rules::steal_candidate`]),
/// nothing otherwise — a runner parked on third alone gates no pitch.
fn steal_window_for(bases: &Bases, rules: &Ruleset) -> Timer {
    let secs = if rules::steal_candidate(bases).is_some() {
        rules.steal_window_secs
    } else {
        0.0
    };
    Timer::from_seconds(secs, TimerMode::Once)
}

/// The offense's send-the-runner gesture: the same held-Down read the live
/// runner call uses ([`rules::runner_call_from_aim`]), so leads, steals, and
/// send-the-batter share one stick convention.
fn wants_send(aim: Vec2) -> bool {
    rules::runner_call_from_aim(aim) == rules::RunnerCall::Send
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
}

/// The pitch ended untouched and the catcher gloved it — cosmetic (the call
/// was already made from the crossing), fired for the glove-pop sound.
#[derive(Event, Clone, Copy)]
pub struct PitchCaughtEvent;

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
            .add_event::<PlayBanner>()
            .add_systems(crate::game::game_start(), reset_flow)
            .add_systems(
                Update,
                // CPU intent is written first so pitching/batting see it this frame.
                (
                    cpu_defense,
                    cpu_offense,
                    pre_pitch,
                    wind_up,
                    pitch_live,
                    catcher_receives,
                    in_play,
                    announce_wall_bang,
                    resolve_live_play,
                    result_phase,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// Fresh play + base state whenever a game (re)starts. The base count follows
/// the chosen field.
fn reset_flow(
    mut play: ResMut<Play>,
    mut bases: ResMut<Bases>,
    mut order: ResMut<BattingOrder>,
    mut lead: ResMut<LeadState>,
    field: Res<FieldSpec>,
) {
    *play = Play::default();
    bases.reset_for(field.base_count());
    *order = BattingOrder::default();
    lead.extended = false;
}

// ── PrePitch: the leadoff duel, then the defense aims and releases ────────────

#[allow(clippy::too_many_arguments)]
fn pre_pitch(
    time: Res<Time>,
    mut play: ResMut<Play>,
    intents: Res<Intents>,
    mut score: ResMut<ScoreBoard>,
    mut bases: ResMut<Bases>,
    rules_res: Res<Ruleset>,
    mut lead: ResMut<LeadState>,
    mut banner: EventWriter<PlayBanner>,
    pitcher_q: Query<Entity, With<Pitcher>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut commands: Commands,
) {
    if play.phase != Phase::PrePitch {
        return;
    }
    play.hold.tick(time.delta());
    play.pickoff_cooldown.tick(time.delta());

    // The offense works the lead: holding Down stretches the lead runner off
    // the bag — the guaranteed steal jump, bought at pickoff risk.
    let offense = intents.get(score.batting_team());
    lead.extended = wants_send(offense.aim) && rules::steal_candidate(&bases).is_some();

    let intent = intents.get(score.fielding_team());
    if play.in_steal_window() {
        // Only a stretch *held through* the window (while the pickoff threat
        // is live) earns the guaranteed jump at delivery — retreating to the
        // bag forfeits it, so a one-frame pulse can't bank a risk-free jump.
        play.window_lead = lead.extended;
        // The duel window: the ball is held. A defensive action here is a
        // pickoff throw at the leading runner, not a pitch — one throw per
        // reload, so a held button can't spam the bag.
        if intent.action && play.pickoff_cooldown.finished() {
            play.pickoff_cooldown = Timer::from_seconds(PICKOFF_COOLDOWN_SECS, TimerMode::Once);
            match rules::attempt_pickoff(&mut score, &mut bases, &rules_res, lead.extended) {
                rules::PickoffResult::PickedOff { .. } => {
                    banner.send(PlayBanner::new("PICKED OFF!", BannerTone::Bad));
                    // A pickoff out is a play: it takes the same result
                    // pause as any other out (banner linger + runners
                    // settling) before the next window can open.
                    end_pitch(&mut play);
                    maybe_end_game(&score, &rules_res, &mut next_state);
                }
                rules::PickoffResult::SafeBack => {
                    banner.send(PlayBanner::new("BACK IN TIME", BannerTone::Info));
                }
                rules::PickoffResult::NoRunner => {}
            }
        }
        return;
    }

    if intent.action {
        play.pending_pitch = Some((intent.aim, rules::PitchKind::from_aim(intent.aim)));
        play.phase = Phase::WindUp;
        play.timer = Timer::from_seconds(AnimClip::WindUp.duration(), TimerMode::Once);
        play.crossing = None;
        play.resolved = false;
        play.pitch_taken = false;
        // A lead still stretched at first movement sends the runner with the
        // delivery. It's only the no-throw-beats-it jump when the stretch
        // was made during the window — that's the extension that paid the
        // pickoff risk; stretching only after the window is a late break.
        if lead.extended {
            play.steal_armed = true;
            play.big_jump = play.window_lead;
        }
        for pitcher in &pitcher_q {
            commands
                .entity(pitcher)
                .insert(Playing::then(AnimClip::WindUp, AnimClip::ThrowRelease));
        }
    }
}

// ── WindUp: the delivery plays out, then the ball leaves the hand ─────────────

#[allow(clippy::too_many_arguments)]
fn wind_up(
    time: Res<Time>,
    mut play: ResMut<Play>,
    field: Res<FieldSpec>,
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    bases: Res<Bases>,
    mut lead: ResMut<LeadState>,
    mut pitch_ev: EventWriter<PitchEvent>,
) {
    if play.phase != Phase::WindUp {
        return;
    }
    // Holding the stick down through the delivery sends the lead runner (the
    // late break: a classic race against the catcher, no guaranteed jump).
    // Nobody in a position to steal means nobody is going.
    if wants_send(intents.get(score.batting_team()).aim) && rules::steal_candidate(&bases).is_some()
    {
        play.steal_armed = true;
        lead.extended = true;
    }
    if play.timer.tick(time.delta()).finished() {
        let (aim, kind) = play
            .pending_pitch
            .take()
            .unwrap_or((Vec2::ZERO, rules::PitchKind::Changeup));
        pitch_ev.send(PitchEvent {
            velocity: rules::pitch_velocity_kind(kind, aim, field.pitch_distance),
            spin: kind.spin(),
        });
        play.live_kind = Some(kind);
        play.phase = Phase::Pitch;
    }
}

// ── Pitch: batter may swing; otherwise judge the take ─────────────────────────

#[allow(clippy::too_many_arguments)]
fn pitch_live(
    time: Res<Time>,
    mut play: ResMut<Play>,
    intents: Res<Intents>,
    rules: Res<Ruleset>,
    field: Res<FieldSpec>,
    mut score: ResMut<ScoreBoard>,
    mut bases: ResMut<Bases>,
    ball_q: Query<&Transform, With<Baseball>>,
    mut hit_ev: EventWriter<HitEvent>,
    mut in_play_ev: EventWriter<BallInPlayEvent>,
    mut banner: EventWriter<PlayBanner>,
    mut order: ResMut<BattingOrder>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if play.phase != Phase::Pitch || play.resolved {
        return;
    }
    let Ok(ball) = ball_q.get_single() else {
        return;
    };
    let pos = ball.translation;

    // Record the plate-crossing location once.
    if play.crossing.is_none() && pos.z <= PLATE_Z + 0.1 {
        play.crossing = Some(Vec2::new(pos.x, pos.y));
    }

    // Captured before any resolution can flip the half-inning: the batting
    // order advances for the team whose batter just finished, not whoever
    // bats next.
    let batter = score.batting_team();
    let intent = intents.get(batter);

    if intent.action {
        let reachable =
            pos.z >= SWING_LATE_Z && pos.z <= SWING_EARLY_Z && pos.x.abs() <= SWING_REACH_X;
        if reachable {
            let velocity = rules::hit_velocity(pos.z, intent.aim);
            hit_ev.send(HitEvent { velocity });
            let (landing, hang_time) = rules::predict_landing(
                velocity,
                rules::hit_spin(velocity),
                crate::game::ball::BALL_DRAG_FACTOR,
                crate::game::ball::MAGNUS_FACTOR,
            );
            let kind = rules::classify_contact(landing, &field);
            in_play_ev.send(BallInPlayEvent { kind, landing });
            play.contact_at = time.elapsed_secs();
            play.phase = Phase::InPlay;
            match kind {
                // Only a ball over the fence is settled at contact.
                rules::ContactKind::HomeRun => {
                    let going = play.steal_armed;
                    resolve_contact(
                        Outcome::HomeRun,
                        &mut score,
                        &mut bases,
                        &rules,
                        &mut banner,
                        going,
                    );
                    order.advance(batter);
                    play.timer = Timer::from_seconds(
                        (hang_time + INPLAY_BUFFER).clamp(INPLAY_MIN, INPLAY_MAX),
                        TimerMode::Once,
                    );
                    play.resolved = true;
                }
                // Everything else stays live: the fielders' chase and the
                // runner races decide the call in `resolve_live_play`.
                rules::ContactKind::Live { .. } => {
                    play.timer = Timer::from_seconds(
                        (hang_time + LIVE_PLAY_BUFFER).clamp(LIVE_PLAY_MIN, LIVE_PLAY_MAX),
                        TimerMode::Once,
                    );
                    play.resolved = false;
                }
            }
        } else {
            // Swinging through a curveball in the dirt with first base open:
            // the catcher can't hold strike three and the batter runs.
            let dropped =
                play.live_kind == Some(rules::PitchKind::Curveball) && !bases.is_occupied(0);
            let call = add_strike(&mut score, &mut bases, &rules, &mut banner, true, dropped);
            // The catcher gloves everything except the strike three that got
            // away (that one is in the dirt by definition).
            play.pitch_taken = call != StrikeCall::DroppedThird;
            if call != StrikeCall::Strike {
                order.advance(batter);
            }
            // The catcher has the ball: a sent runner must survive the throw.
            if play.steal_armed {
                resolve_steal(&play, &mut score, &mut bases, &rules, &mut banner);
            }
            end_pitch(&mut play);
        }
        maybe_end_game(&score, &rules, &mut next_state);
        return;
    }

    // No swing: once the ball is well past the plate, judge the take.
    if pos.z < SWING_LATE_Z {
        let cross = play.crossing.unwrap_or(Vec2::new(pos.x, pos.y));
        if rules::hits_batter(cross) {
            // Dead ball: the batter takes first, forced runners move.
            let runs = rules::hit_by_pitch(&mut score, &mut bases);
            let tone = if runs > 0 {
                BannerTone::Epic
            } else {
                BannerTone::Good
            };
            banner.send(PlayBanner::new("HIT BY PITCH", tone));
            order.advance(batter);
        } else {
            play.pitch_taken = true;
            let (pa_over, walked) = if rules::is_in_zone(cross) {
                let call = add_strike(&mut score, &mut bases, &rules, &mut banner, false, false);
                (call != StrikeCall::Strike, false)
            } else {
                let walked = add_ball(&mut score, &mut bases, &rules, &mut banner);
                (walked, walked)
            };
            if pa_over {
                order.advance(batter);
            }
            // A walk is a dead ball (runners advance freely); otherwise a
            // sent runner has to beat the catcher's throw.
            if play.steal_armed && !walked {
                resolve_steal(&play, &mut score, &mut bases, &rules, &mut banner);
            }
        }
        end_pitch(&mut play);
        maybe_end_game(&score, &rules, &mut next_state);
    }
}

// ── The catcher receives ──────────────────────────────────────────────────────

/// Stops an untouched pitch in the catcher's mitt instead of letting it sail
/// to the backstop. Cosmetic — the call was already made at the crossing —
/// but the ball really ends up in the glove, with the pop to prove it. Parks
/// without a catcher (the front yard) let the ball fly as before.
#[allow(clippy::type_complexity)]
fn catcher_receives(
    mut play: ResMut<Play>,
    catchers: Query<(Entity, &Transform), (With<CatcherRole>, Without<Baseball>)>,
    mut ball_q: Query<(Entity, &mut Transform, &mut Velocity), (With<Baseball>, With<InFlight>)>,
    mut caught: EventWriter<PitchCaughtEvent>,
    mut commands: Commands,
) {
    if play.phase != Phase::Result || !play.pitch_taken {
        return;
    }
    let Some((catcher, catcher_tf)) = catchers.iter().next() else {
        return;
    };
    let Ok((ball, mut ball_tf, mut vel)) = ball_q.get_single_mut() else {
        return;
    };
    let pos = ball_tf.translation;
    if pos.z > catcher_tf.translation.z + 0.6 || vel.linvel.z >= 0.0 {
        return; // still on its way in
    }
    play.pitch_taken = false;
    if pos.y < 0.12 || pos.y > 2.4 {
        return; // in the dirt or over everything: play it off the backstop
    }
    ball_tf.translation = catcher_tf.translation + Vec3::new(0.0, 0.5, 0.45);
    vel.linvel = Vec3::ZERO;
    vel.angvel = Vec3::ZERO;
    commands.entity(ball).remove::<InFlight>();
    commands
        .entity(catcher)
        .insert(Playing::new(AnimClip::GloveUp));
    caught.send(PitchCaughtEvent);
}

// ── InPlay: the ball is live ──────────────────────────────────────────────────

/// Ticks the play clock. Resolved plays (home runs, or anything already
/// called by [`resolve_live_play`]) move on to the result pause when the
/// timer runs out; unresolved plays are force-called by `resolve_live_play`.
fn in_play(mut play: ResMut<Play>, time: Res<Time>) {
    if play.phase != Phase::InPlay {
        return;
    }
    play.timer.tick(time.delta());
    if play.resolved && play.timer.finished() {
        play.phase = Phase::Result;
        play.timer = Timer::from_seconds(RESULT_SECS, TimerMode::Once);
    }
}

/// Turns the fielding simulation's physical reports into the umpire's call.
/// This is where a live ball actually becomes an out, a hit, or a foul —
/// seconds after contact, from what really happened on the grass.
#[allow(clippy::too_many_arguments)]
fn resolve_live_play(
    time: Res<Time>,
    mut events: EventReader<LiveBallEvent>,
    mut play: ResMut<Play>,
    rules_res: Res<Ruleset>,
    field: Res<FieldSpec>,
    intents: Res<Intents>,
    mut score: ResMut<ScoreBoard>,
    mut bases: ResMut<Bases>,
    mut order: ResMut<BattingOrder>,
    mut banner: EventWriter<PlayBanner>,
    ball_q: Query<&Transform, With<Baseball>>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if play.phase != Phase::InPlay || play.resolved {
        events.clear();
        return;
    }

    // `None` = foul ball; `Some(outcome)` = a completed play.
    let mut resolution: Option<Option<Outcome>> = None;
    for ev in events.read() {
        resolution = match *ev {
            LiveBallEvent::Caught { pos } => {
                Some(Some(Outcome::Out(rules::resolve_catch(pos, &field))))
            }
            LiveBallEvent::Landed { pos } if !rules::is_fair(pos, &field) => Some(None),
            // A fair bounce just keeps the play alive.
            LiveBallEvent::Landed { .. } => continue,
            LiveBallEvent::Thrown {
                pos,
                base,
                race_time,
            } => {
                let call = rules::runner_call_from_aim(intents.get(score.batting_team()).aim);
                Some(Some(rules::resolve_thrown(
                    pos,
                    race_time,
                    base,
                    &bases,
                    play.runners_going(),
                    call,
                    &field,
                    &rules_res,
                )))
            }
        };
        break;
    }
    // Play clock expired with the ball still loose: call it from where the
    // ball is right now.
    if resolution.is_none() && play.timer.finished() {
        let pos = ball_q
            .get_single()
            .map(|t| t.translation)
            .unwrap_or(Vec3::ZERO);
        let t = time.elapsed_secs() - play.contact_at;
        resolution = Some(if rules::is_fair(pos, &field) {
            Some(rules::resolve_gathered(pos, t, &field, &rules_res))
        } else {
            None
        });
    }
    let Some(resolved) = resolution else {
        return;
    };

    let batter = score.batting_team();
    let going = play.steal_armed;
    let outcome = resolved.unwrap_or(Outcome::Foul);
    resolve_contact(
        outcome,
        &mut score,
        &mut bases,
        &rules_res,
        &mut banner,
        going,
    );
    if outcome != Outcome::Foul {
        order.advance(batter);
    }
    play.resolved = true;
    play.phase = Phase::Result;
    play.timer = Timer::from_seconds(RESULT_SECS, TimerMode::Once);
    maybe_end_game(&score, &rules_res, &mut next_state);
}

// ── Result: brief pause, then reset for the next pitch ────────────────────────

#[allow(clippy::too_many_arguments)]
fn result_phase(
    mut play: ResMut<Play>,
    time: Res<Time>,
    field: Res<FieldSpec>,
    rules_res: Res<Ruleset>,
    bases: Res<Bases>,
    settled: Res<RunnersSettled>,
    mut overtime: Local<f32>,
    mut lead: ResMut<LeadState>,
    mut ball_q: Query<(Entity, &mut Transform, &mut Velocity), With<Baseball>>,
    mut commands: Commands,
) {
    if play.phase != Phase::Result {
        return;
    }
    if !play.timer.tick(time.delta()).finished() {
        return;
    }
    // The play isn't over while runner rigs are still moving (the home-run
    // trot, a first-to-third sprint): the next batter waits for the bases to
    // settle, with a hard cap so a stray path can never stall the game.
    if !settled.0 && *overtime < RESULT_SETTLE_CAP {
        *overtime += time.delta_secs();
        return;
    }
    *overtime = 0.0;
    if let Ok((entity, mut transform, mut vel)) = ball_q.get_single_mut() {
        transform.translation = rules::mound_reset_pos(field.pitch_distance);
        vel.linvel = Vec3::ZERO;
        vel.angvel = Vec3::ZERO;
        commands.entity(entity).remove::<InFlight>();
    }
    play.phase = Phase::PrePitch;
    play.crossing = None;
    play.resolved = false;
    play.pending_pitch = None;
    play.live_kind = None;
    play.steal_armed = false;
    play.big_jump = false;
    play.window_lead = false;
    play.pitch_taken = false;
    play.wall_called = false;
    // A runner in stealing position opens the duel window for the next at-bat.
    play.hold = steal_window_for(&bases, &rules_res);
    lead.extended = false;
}

/// A live ball caroms off the wall: one excited call per play. Resolved
/// plays (a rare home run clipping the top of the wall) stay silent — the
/// call was already made.
fn announce_wall_bang(
    mut bangs: EventReader<WallBangEvent>,
    mut play: ResMut<Play>,
    mut banner: EventWriter<PlayBanner>,
) {
    let banged = bangs.read().next().is_some();
    if banged && play.phase == Phase::InPlay && !play.resolved && !play.wall_called {
        play.wall_called = true;
        banner.send(PlayBanner::new("OFF THE WALL!", BannerTone::Good));
    }
}

// ── Rule results → banners ────────────────────────────────────────────────────

fn resolve_contact(
    outcome: Outcome,
    score: &mut ScoreBoard,
    bases: &mut Bases,
    ruleset: &Ruleset,
    banner: &mut EventWriter<PlayBanner>,
    runners_going: bool,
) {
    match outcome {
        Outcome::Foul => {
            rules::foul(score, ruleset);
            banner.send(PlayBanner::new("FOUL", BannerTone::Info));
        }
        Outcome::Out(kind) => {
            let play = rules::apply_batted_out(score, bases, ruleset, kind, runners_going);
            let base_text = if play.doubled_off {
                "DOUBLED OFF!"
            } else if play.runs > 0 && matches!(kind, OutKind::Fly { .. }) {
                "SAC FLY"
            } else {
                match kind {
                    OutKind::Ground => "GROUND OUT",
                    OutKind::Fly { .. } => "FLY OUT",
                    OutKind::Pop => "POP OUT",
                    OutKind::FoulPop => "FOUL POP OUT",
                    OutKind::Pegged => "PEGGED!",
                    OutKind::Stretching { .. } => "OUT STRETCHING!",
                }
            };
            let text = if play.runs > 0 {
                format!("{base_text}  +{}", play.runs)
            } else {
                base_text.to_string()
            };
            banner.send(PlayBanner::new(text, BannerTone::Bad));
        }
        Outcome::DoublePlay => {
            let play = rules::apply_double_play(score, bases, ruleset);
            let text = if play.runs > 0 {
                format!("DOUBLE PLAY!  +{}", play.runs)
            } else {
                "DOUBLE PLAY!".to_string()
            };
            banner.send(PlayBanner::new(text, BannerTone::Bad));
        }
        Outcome::FieldersChoice { out_base } => {
            rules::apply_fielders_choice(score, bases, ruleset, out_base);
            banner.send(PlayBanner::new("FIELDER'S CHOICE", BannerTone::Bad));
        }
        Outcome::Hit(n) => {
            let label = match n {
                1 => "SINGLE".to_string(),
                2 => "DOUBLE".to_string(),
                3 => "TRIPLE".to_string(),
                n => format!("{n} BASES!"),
            };
            hit(
                score,
                bases,
                banner,
                n,
                &label,
                BannerTone::Good,
                runners_going,
            );
        }
        // A home run is worth one more base than the field has.
        Outcome::HomeRun => {
            let bases_worth = bases.count() as u32 + 1;
            hit(
                score,
                bases,
                banner,
                bases_worth,
                "HOME RUN!",
                BannerTone::Epic,
                runners_going,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn hit(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    banner: &mut EventWriter<PlayBanner>,
    hit_bases: u32,
    label: &str,
    tone: BannerTone,
    jump: bool,
) {
    let runs = rules::apply_hit(score, bases, hit_bases, jump);
    let text = if runs > 0 {
        format!("{label}  +{runs}")
    } else {
        label.to_string()
    };
    banner.send(PlayBanner::new(text, tone));
}

/// Records a taken ball. Returns whether it was ball four (a dead-ball walk,
/// which pre-empts any steal attempt).
fn add_ball(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    ruleset: &Ruleset,
    banner: &mut EventWriter<PlayBanner>,
) -> bool {
    match rules::call_ball(score, bases, ruleset) {
        BallCall::Walk { .. } => {
            banner.send(PlayBanner::new("WALK", BannerTone::Epic));
            true
        }
        BallCall::Ball => {
            banner.send(PlayBanner::new("BALL", BannerTone::Info));
            false
        }
    }
}

/// Resolves a sent runner once the catcher has the ball: the jump beats the
/// throw on off-speed pitches, a fastball cuts the runner down.
fn resolve_steal(
    play: &Play,
    score: &mut ScoreBoard,
    bases: &mut Bases,
    ruleset: &Ruleset,
    banner: &mut EventWriter<PlayBanner>,
) {
    let off_speed = play.live_kind != Some(rules::PitchKind::Fastball);
    match rules::attempt_steal(score, bases, ruleset, off_speed, play.big_jump) {
        StealResult::Stolen { .. } => {
            banner.send(PlayBanner::new("STOLEN BASE!", BannerTone::Good));
        }
        StealResult::Caught => {
            banner.send(PlayBanner::new("CAUGHT STEALING", BannerTone::Bad));
        }
        StealResult::NoRunner => {}
    }
}

fn add_strike(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    ruleset: &Ruleset,
    banner: &mut EventWriter<PlayBanner>,
    swinging: bool,
    dropped_third: bool,
) -> StrikeCall {
    let call = rules::call_strike(score, bases, ruleset, dropped_third);
    match call {
        StrikeCall::DroppedThird => {
            banner.send(PlayBanner::new("DROPPED 3RD STRIKE!", BannerTone::Good));
        }
        StrikeCall::Strikeout => {
            banner.send(PlayBanner::new("STRIKEOUT!", BannerTone::Bad));
        }
        StrikeCall::Strike if swinging => {
            banner.send(PlayBanner::new("SWING & MISS", BannerTone::Info));
        }
        StrikeCall::Strike => {
            banner.send(PlayBanner::new("STRIKE", BannerTone::Info));
        }
    }
    call
}

fn maybe_end_game(score: &ScoreBoard, rules: &Ruleset, next_state: &mut NextState<GameState>) {
    if rules::is_game_over(score, rules.innings) {
        next_state.set(GameState::GameOver);
    }
}

fn end_pitch(play: &mut Play) {
    play.phase = Phase::Result;
    play.timer = Timer::from_seconds(RESULT_SECS, TimerMode::Once);
    play.resolved = true;
}
