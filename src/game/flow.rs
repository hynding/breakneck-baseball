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
use crate::game::ball::{Baseball, HitEvent, InFlight, PitchEvent};
use crate::game::input::Intents;
use crate::game::player::Pitcher;
use crate::game::rules::{
    self, BallCall, Bases, BattingOrder, OutKind, Outcome, StealResult, StrikeCall,
};
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
    /// `Time::elapsed_secs` at contact — the live-play race clock's zero.
    contact_at: f32,
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
            contact_at: 0.0,
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
    /// Picked up off the ground at `pos`; the throw races begin.
    Gathered { pos: Vec3 },
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

pub struct FlowPlugin;

impl Plugin for FlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Play>()
            .init_resource::<Bases>()
            .init_resource::<BattingOrder>()
            .init_resource::<CpuConfig>()
            .init_resource::<CpuState>()
            .add_event::<BallInPlayEvent>()
            .add_event::<LiveBallEvent>()
            .add_event::<PlayBanner>()
            .add_systems(OnEnter(GameState::Playing), reset_flow)
            .add_systems(
                Update,
                // CPU intent is written first so pitching/batting see it this frame.
                (
                    cpu_defense,
                    cpu_offense,
                    pre_pitch,
                    wind_up,
                    pitch_live,
                    in_play,
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
    field: Res<FieldSpec>,
) {
    *play = Play::default();
    bases.reset_for(field.base_count());
    *order = BattingOrder::default();
}

// ── PrePitch: defense aims and releases ───────────────────────────────────────

fn pre_pitch(
    mut play: ResMut<Play>,
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    pitcher_q: Query<Entity, With<Pitcher>>,
    mut commands: Commands,
) {
    if play.phase != Phase::PrePitch {
        return;
    }

    let intent = intents.get(score.fielding_team());
    if intent.action {
        play.pending_pitch = Some((intent.aim, rules::PitchKind::from_aim(intent.aim)));
        play.phase = Phase::WindUp;
        play.timer = Timer::from_seconds(AnimClip::WindUp.duration(), TimerMode::Once);
        play.crossing = None;
        play.resolved = false;
        for pitcher in &pitcher_q {
            commands
                .entity(pitcher)
                .insert(Playing::then(AnimClip::WindUp, AnimClip::ThrowRelease));
        }
    }
}

// ── WindUp: the delivery plays out, then the ball leaves the hand ─────────────

fn wind_up(
    time: Res<Time>,
    mut play: ResMut<Play>,
    field: Res<FieldSpec>,
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    mut pitch_ev: EventWriter<PitchEvent>,
) {
    if play.phase != Phase::WindUp {
        return;
    }
    // Holding the stick down through the delivery sends the lead runner.
    if intents.get(score.batting_team()).aim.y < -0.7 {
        play.steal_armed = true;
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
            LiveBallEvent::Gathered { pos } => {
                let t = time.elapsed_secs() - play.contact_at;
                Some(Some(rules::resolve_gathered(pos, t, &field, &rules_res)))
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

fn result_phase(
    mut play: ResMut<Play>,
    time: Res<Time>,
    field: Res<FieldSpec>,
    mut ball_q: Query<(Entity, &mut Transform, &mut Velocity), With<Baseball>>,
    mut commands: Commands,
) {
    if play.phase != Phase::Result {
        return;
    }
    if play.timer.tick(time.delta()).finished() {
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
            let base_text = if play.double_play {
                "DOUBLE PLAY!"
            } else if play.doubled_off {
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
                }
            };
            let text = if play.runs > 0 {
                format!("{base_text}  +{}", play.runs)
            } else {
                base_text.to_string()
            };
            banner.send(PlayBanner::new(text, BannerTone::Bad));
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
    match rules::attempt_steal(score, bases, ruleset, off_speed) {
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
