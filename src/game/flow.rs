//! Game flow — the at-bat/play state machine and the rules that drive it.
//!
//! Within [`GameState::Playing`] the game cycles through a small [`Phase`]
//! machine for each pitch:
//!
//! ```text
//! PrePitch --release--> Pitch --contact--> InPlay --resolved--> Result --> PrePitch
//!                          \--take/miss--> (count) --> Result --> PrePitch
//! ```
//!
//! Milestone 2 implements pitching, swinging, contact, and the ball/strike/
//! walk/strikeout count. Ball-in-play outcomes (hits, outs, base running,
//! scoring, and the inning/game-over flow) are layered on in later milestones.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::ball::{Baseball, HitEvent, InFlight, PitchEvent, BALL_RADIUS};
use crate::game::field::PITCH_DISTANCE;
use crate::game::input::Intents;
use crate::game::{GameState, ScoreBoard};

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Z of home plate (the batter stands here).
const PLATE_Z: f32 = 0.0;
/// Horizontal half-width of the called strike zone (metres from plate centre).
const ZONE_HALF_WIDTH: f32 = 0.32;
/// Vertical strike-zone bounds (metres).
const ZONE_LOW: f32 = 0.5;
const ZONE_HIGH: f32 = 1.45;

/// A swing connects while the ball is within this Z band of the plate.
const SWING_LATE_Z: f32 = -1.2; // ball this far past the plate = window closed
const SWING_EARLY_Z: f32 = 3.2; // ball this far in front = earliest contact
/// Maximum horizontal miss the batter can still reach.
const SWING_REACH_X: f32 = 1.7;

/// Nominal pitch speed (m/s) — roughly 85 mph.
const PITCH_SPEED: f32 = 38.0;
/// Seconds the result banner lingers before the next pitch.
const RESULT_SECS: f32 = 1.4;
/// Safety timeout for a ball in play that never resolves (milestone 2 stub).
const INPLAY_TIMEOUT_SECS: f32 = 5.0;

/// Where the ball rests before each pitch (top of the mound).
fn mound_reset_pos() -> Vec3 {
    Vec3::new(0.0, BALL_RADIUS + 0.25, PITCH_DISTANCE)
}

// ── Phase state ───────────────────────────────────────────────────────────────

/// The current step of an at-bat.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Phase {
    /// Waiting for the defense to throw a pitch.
    #[default]
    PrePitch,
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
    /// Drives `Result` (and the milestone-2 `InPlay` stub) timing.
    timer: Timer,
    /// Plate-crossing point (x, y) recorded the first frame the ball reaches
    /// the plate, used to judge a taken pitch as a ball or strike.
    crossing: Option<Vec2>,
    /// True once the current pitch has been decided (prevents double counting).
    resolved: bool,
}

impl Default for Play {
    fn default() -> Self {
        Self {
            phase: Phase::PrePitch,
            timer: Timer::from_seconds(RESULT_SECS, TimerMode::Once),
            crossing: None,
            resolved: false,
        }
    }
}

// ── Events ────────────────────────────────────────────────────────────────────

/// A transient on-screen message (e.g. "STRIKE!", "BALL", "HOME RUN!").
#[derive(Event, Clone)]
pub struct PlayBanner {
    pub text: String,
    pub color: Color,
}

impl PlayBanner {
    fn new(text: impl Into<String>, color: Color) -> Self {
        Self {
            text: text.into(),
            color,
        }
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct FlowPlugin;

impl Plugin for FlowPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Play>()
            .add_event::<PlayBanner>()
            .add_systems(OnEnter(GameState::Playing), reset_play)
            .add_systems(
                Update,
                (pre_pitch, pitch_live, in_play, result_phase)
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// Fresh play state whenever a game (re)starts.
fn reset_play(mut play: ResMut<Play>) {
    *play = Play::default();
}

// ── PrePitch: defense aims and releases ───────────────────────────────────────

fn pre_pitch(
    mut play: ResMut<Play>,
    intents: Res<Intents>,
    score: Res<ScoreBoard>,
    mut pitch_ev: EventWriter<PitchEvent>,
) {
    if play.phase != Phase::PrePitch {
        return;
    }

    let intent = intents.get(score.fielding_team());
    if intent.action {
        // Aim: x steers horizontal location at the plate, y the height.
        let target_x = intent.aim.x * 0.35;
        let target_y = 1.0 + intent.aim.y * 0.5;

        let start = mound_reset_pos();
        let flight = PITCH_DISTANCE / PITCH_SPEED; // ≈ seconds to the plate
                                                   // Solve simple ballistics so the pitch arrives near (target_x, target_y).
        let vx = (target_x - start.x) / flight;
        let vy = (target_y - start.y) / flight + 0.5 * 9.81 * flight;

        pitch_ev.send(PitchEvent {
            velocity: Vec3::new(vx, vy, -PITCH_SPEED),
        });

        play.phase = Phase::Pitch;
        play.crossing = None;
        play.resolved = false;
    }
}

// ── Pitch: batter may swing; otherwise judge the take ─────────────────────────

fn pitch_live(
    mut play: ResMut<Play>,
    intents: Res<Intents>,
    mut score: ResMut<ScoreBoard>,
    ball_q: Query<&Transform, With<Baseball>>,
    mut hit_ev: EventWriter<HitEvent>,
    mut banner: EventWriter<PlayBanner>,
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
            hit_ev.send(compute_hit(pos.z, intent.aim));
            banner.send(PlayBanner::new("IN PLAY", Color::WHITE));
            play.phase = Phase::InPlay;
            play.timer = Timer::from_seconds(INPLAY_TIMEOUT_SECS, TimerMode::Once);
            play.resolved = true;
        } else {
            // Swing and a miss.
            add_strike(&mut score, &mut banner, true);
            end_pitch(&mut play);
        }
        return;
    }

    // No swing: once the ball is well past the plate, judge the take.
    if pos.z < SWING_LATE_Z {
        let cross = play.crossing.unwrap_or(Vec2::new(pos.x, pos.y));
        let in_zone =
            cross.x.abs() <= ZONE_HALF_WIDTH && cross.y >= ZONE_LOW && cross.y <= ZONE_HIGH;
        if in_zone {
            add_strike(&mut score, &mut banner, false);
        } else {
            add_ball(&mut score, &mut banner);
        }
        end_pitch(&mut play);
    }
}

/// Converts contact timing + aim into a batted-ball velocity.
///
/// Timing is derived from the ball's Z at the moment of contact: near the plate
/// (`z ≈ 0`) yields the best power; the aim stick sets launch angle and spray.
fn compute_hit(contact_z: f32, aim: Vec2) -> HitEvent {
    // Quality: 1.0 at the plate, tapering toward the window edges.
    let ideal = 0.4_f32;
    let window = (SWING_EARLY_Z - SWING_LATE_Z) * 0.5;
    let quality = (1.0 - (contact_z - ideal).abs() / window).clamp(0.15, 1.0);

    let speed = 22.0 + 24.0 * quality;
    // Launch angle 8°..38° from the up-aim.
    let launch = (8.0 + 30.0 * (aim.y * 0.5 + 0.5)).to_radians();
    // Spray: aim.x pulls the ball left/right; early contact pulls a bit more.
    let timing_pull = (contact_z - ideal) * 0.06;
    let spray = (aim.x * 0.6 + timing_pull).clamp(-0.9, 0.9);

    let horizontal = speed * launch.cos();
    let velocity = Vec3::new(
        horizontal * spray.sin(),
        speed * launch.sin(),
        horizontal * spray.cos(),
    );
    HitEvent { velocity }
}

// ── InPlay: milestone-2 stub (resolution added in milestone 3) ────────────────

fn in_play(mut play: ResMut<Play>, time: Res<Time>) {
    if play.phase != Phase::InPlay {
        return;
    }
    // Milestone 2 has no fielding/outcome yet — just wait for the ball to settle
    // then move to the result pause. Milestone 3 replaces this with real outcome
    // resolution (hits, outs, base running, scoring).
    if play.timer.tick(time.delta()).finished() {
        play.phase = Phase::Result;
        play.timer = Timer::from_seconds(RESULT_SECS, TimerMode::Once);
    }
}

// ── Result: brief pause, then reset for the next pitch ────────────────────────

fn result_phase(
    mut play: ResMut<Play>,
    time: Res<Time>,
    mut ball_q: Query<(Entity, &mut Transform, &mut Velocity), With<Baseball>>,
    mut commands: Commands,
) {
    if play.phase != Phase::Result {
        return;
    }
    if play.timer.tick(time.delta()).finished() {
        // Park the ball back on the mound, ready for the next pitch.
        if let Ok((entity, mut transform, mut vel)) = ball_q.get_single_mut() {
            transform.translation = mound_reset_pos();
            vel.linvel = Vec3::ZERO;
            vel.angvel = Vec3::ZERO;
            commands.entity(entity).remove::<InFlight>();
        }
        play.phase = Phase::PrePitch;
        play.crossing = None;
        play.resolved = false;
    }
}

// ── Count helpers ─────────────────────────────────────────────────────────────

fn add_ball(score: &mut ScoreBoard, banner: &mut EventWriter<PlayBanner>) {
    score.balls += 1;
    if score.balls >= 4 {
        // Walk. Base running is added in milestone 3; for now, reset the count.
        banner.send(PlayBanner::new("WALK", Color::srgb(0.6, 0.85, 1.0)));
        score.balls = 0;
        score.strikes = 0;
    } else {
        banner.send(PlayBanner::new("BALL", Color::srgb(0.7, 0.9, 0.7)));
    }
}

fn add_strike(score: &mut ScoreBoard, banner: &mut EventWriter<PlayBanner>, swinging: bool) {
    score.strikes += 1;
    if score.strikes >= 3 {
        banner.send(PlayBanner::new("STRIKEOUT!", Color::srgb(1.0, 0.5, 0.35)));
        record_out(score);
    } else if swinging {
        banner.send(PlayBanner::new("SWING & MISS", Color::srgb(1.0, 0.7, 0.4)));
    } else {
        banner.send(PlayBanner::new("STRIKE", Color::srgb(1.0, 0.8, 0.4)));
    }
}

/// Records an out and flips the half-inning after three. (Inning/game-over
/// bounds are finalised in milestone 3; this keeps outs meaningful now.)
fn record_out(score: &mut ScoreBoard) {
    score.balls = 0;
    score.strikes = 0;
    score.outs += 1;
    if score.outs >= 3 {
        score.outs = 0;
        if score.top_of_inning {
            score.top_of_inning = false;
        } else {
            score.top_of_inning = true;
            score.inning += 1;
        }
    }
}

/// Ends the current pitch, routing into the brief result pause.
fn end_pitch(play: &mut Play) {
    play.phase = Phase::Result;
    play.timer = Timer::from_seconds(RESULT_SECS, TimerMode::Once);
    play.resolved = true;
}
