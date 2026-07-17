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
//! All baseball *rules* (batted-ball classification, base running, the count,
//! game-over) live in [`crate::game::rules`] as pure, unit-tested functions;
//! this module reads input, drives the phases and timers, and translates rule
//! results into banners and state transitions. Ball-in-play outcomes are
//! decided **analytically** at contact (see [`rules::classify_batted_ball`])
//! rather than by simulating fielders — the arcade convention. The physics
//! ball still flies for feel, but the box score is deterministic.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::ai::{cpu_defense, cpu_offense, CpuConfig, CpuState};
use crate::game::animation::{AnimClip, Playing};
use crate::game::ball::{Baseball, HitEvent, InFlight, PitchEvent};
use crate::game::input::Intents;
use crate::game::player::Pitcher;
use crate::game::rules::{self, BallCall, Bases, OutKind, Outcome, StrikeCall};
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
/// The live-ball window: hang time plus room for the fielding choreography,
/// clamped so grounders don't dawdle and moonshots don't stall the game.
const INPLAY_BUFFER: f32 = 1.2;
const INPLAY_MIN: f32 = 2.2;
const INPLAY_MAX: f32 = 6.5;

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
    /// Aim stored at windup start, released as the pitch when the delivery ends.
    pending_pitch: Option<Vec2>,
}

impl Default for Play {
    fn default() -> Self {
        Self {
            phase: Phase::PrePitch,
            timer: Timer::from_seconds(RESULT_SECS, TimerMode::Once),
            crossing: None,
            resolved: false,
            pending_pitch: None,
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

/// Fired once per fair contact: the already-decided outcome plus where the
/// live ball will come down. Fielder and runner choreography key off this.
#[derive(Event, Clone, Copy)]
pub struct BallInPlayEvent {
    pub outcome: Outcome,
    pub landing: Vec3,
    pub hang_time: f32,
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
            .init_resource::<CpuConfig>()
            .init_resource::<CpuState>()
            .add_event::<BallInPlayEvent>()
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
                    result_phase,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// Fresh play + base state whenever a game (re)starts. The base count follows
/// the chosen field.
fn reset_flow(mut play: ResMut<Play>, mut bases: ResMut<Bases>, field: Res<FieldSpec>) {
    *play = Play::default();
    bases.reset_for(field.base_count());
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
        play.pending_pitch = Some(intent.aim);
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
    mut pitch_ev: EventWriter<PitchEvent>,
) {
    if play.phase != Phase::WindUp {
        return;
    }
    if play.timer.tick(time.delta()).finished() {
        let aim = play.pending_pitch.take().unwrap_or(Vec2::ZERO);
        pitch_ev.send(PitchEvent {
            velocity: rules::pitch_velocity(aim, field.pitch_distance),
        });
        play.phase = Phase::Pitch;
    }
}

// ── Pitch: batter may swing; otherwise judge the take ─────────────────────────

#[allow(clippy::too_many_arguments)]
fn pitch_live(
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

    let intent = intents.get(score.batting_team());

    if intent.action {
        let reachable =
            pos.z >= SWING_LATE_Z && pos.z <= SWING_EARLY_Z && pos.x.abs() <= SWING_REACH_X;
        if reachable {
            let velocity = rules::hit_velocity(pos.z, intent.aim);
            let outcome = rules::classify_batted_ball(velocity, &field, &rules);
            hit_ev.send(HitEvent { velocity });
            resolve_contact(outcome, &mut score, &mut bases, &rules, &mut banner);
            if outcome == Outcome::Foul {
                end_pitch(&mut play);
            } else {
                let (landing, hang_time) =
                    rules::predict_landing(velocity, crate::game::ball::BALL_DRAG_FACTOR);
                in_play_ev.send(BallInPlayEvent {
                    outcome,
                    landing,
                    hang_time,
                });
                play.phase = Phase::InPlay;
                play.timer = Timer::from_seconds(
                    (hang_time + INPLAY_BUFFER).clamp(INPLAY_MIN, INPLAY_MAX),
                    TimerMode::Once,
                );
                play.resolved = true;
            }
        } else {
            add_strike(&mut score, &mut bases, &rules, &mut banner, true);
            end_pitch(&mut play);
        }
        maybe_end_game(&score, &rules, &mut next_state);
        return;
    }

    // No swing: once the ball is well past the plate, judge the take.
    if pos.z < SWING_LATE_Z {
        let cross = play.crossing.unwrap_or(Vec2::new(pos.x, pos.y));
        if rules::is_in_zone(cross) {
            add_strike(&mut score, &mut bases, &rules, &mut banner, false);
        } else {
            add_ball(&mut score, &mut bases, &rules, &mut banner);
        }
        end_pitch(&mut play);
        maybe_end_game(&score, &rules, &mut next_state);
    }
}

// ── InPlay: ball flies for feel; outcome was already resolved at contact ──────

fn in_play(mut play: ResMut<Play>, time: Res<Time>) {
    if play.phase != Phase::InPlay {
        return;
    }
    if play.timer.tick(time.delta()).finished() {
        play.phase = Phase::Result;
        play.timer = Timer::from_seconds(RESULT_SECS, TimerMode::Once);
    }
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
    }
}

// ── Rule results → banners ────────────────────────────────────────────────────

fn resolve_contact(
    outcome: Outcome,
    score: &mut ScoreBoard,
    bases: &mut Bases,
    ruleset: &Ruleset,
    banner: &mut EventWriter<PlayBanner>,
) {
    match outcome {
        Outcome::Foul => {
            rules::foul(score, ruleset);
            banner.send(PlayBanner::new("FOUL", BannerTone::Info));
        }
        Outcome::Out(kind) => {
            let text = match kind {
                OutKind::Ground => "GROUND OUT",
                OutKind::Fly => "FLY OUT",
                OutKind::Pop => "POP OUT",
                OutKind::Pegged => "PEGGED!",
            };
            banner.send(PlayBanner::new(text, BannerTone::Bad));
            rules::record_out(score, bases, ruleset);
        }
        Outcome::Hit(n) => {
            let label = match n {
                1 => "SINGLE".to_string(),
                2 => "DOUBLE".to_string(),
                3 => "TRIPLE".to_string(),
                n => format!("{n} BASES!"),
            };
            hit(score, bases, banner, n, &label, BannerTone::Good);
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
            );
        }
    }
}

fn hit(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    banner: &mut EventWriter<PlayBanner>,
    hit_bases: u32,
    label: &str,
    tone: BannerTone,
) {
    let runs = rules::apply_hit(score, bases, hit_bases);
    let text = if runs > 0 {
        format!("{label}  +{runs}")
    } else {
        label.to_string()
    };
    banner.send(PlayBanner::new(text, tone));
}

fn add_ball(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    ruleset: &Ruleset,
    banner: &mut EventWriter<PlayBanner>,
) {
    match rules::call_ball(score, bases, ruleset) {
        BallCall::Walk { .. } => {
            banner.send(PlayBanner::new("WALK", BannerTone::Epic));
        }
        BallCall::Ball => {
            banner.send(PlayBanner::new("BALL", BannerTone::Info));
        }
    }
}

fn add_strike(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    ruleset: &Ruleset,
    banner: &mut EventWriter<PlayBanner>,
    swinging: bool,
) {
    match rules::call_strike(score, bases, ruleset) {
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
