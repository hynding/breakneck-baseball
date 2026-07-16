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
//! Ball-in-play outcomes are decided **analytically** from the batted-ball
//! launch vector (see [`classify_batted_ball`]) rather than by simulating
//! fielders — the arcade convention. The physics ball still flies for feel, but
//! the box score is deterministic from contact quality and aim.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::ball::{Baseball, HitEvent, InFlight, PitchEvent, BALL_RADIUS};
use crate::game::field::PITCH_DISTANCE;
use crate::game::input::Intents;
use crate::game::{GameConfig, GameState, ScoreBoard};

// ── Tuning constants ──────────────────────────────────────────────────────────

/// Z of home plate (the batter stands here).
const PLATE_Z: f32 = 0.0;
/// Horizontal half-width of the called strike zone (metres from plate centre).
const ZONE_HALF_WIDTH: f32 = 0.34;
/// Vertical strike-zone bounds (metres).
const ZONE_LOW: f32 = 0.5;
const ZONE_HIGH: f32 = 1.45;

/// A swing connects while the ball is within this Z band of the plate.
const SWING_LATE_Z: f32 = -1.2; // ball this far past the plate = window closed
const SWING_EARLY_Z: f32 = 3.2; // ball this far in front = earliest contact
/// Maximum horizontal miss the batter can still reach.
const SWING_REACH_X: f32 = 1.8;

/// Nominal pitch speed (m/s) — roughly 85 mph.
const PITCH_SPEED: f32 = 38.0;
/// Seconds the result banner lingers before the next pitch.
const RESULT_SECS: f32 = 1.5;
/// How long a batted ball stays live (visual) before the field resets.
const INPLAY_SECS: f32 = 3.0;

/// Gravity magnitude used for landing-point prediction (matches Rapier default).
const GRAVITY: f32 = 9.81;
/// Approximate batted-ball contact height (metres).
const CONTACT_HEIGHT: f32 = 0.6;
/// Fraction of the drag-free range a real (draggy) ball actually travels.
const DRAG_RANGE_FACTOR: f32 = 0.73;

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
    timer: Timer,
    crossing: Option<Vec2>,
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

/// Occupancy of the three bases. All runners belong to the batting team, so a
/// boolean is enough. Used by base-running rules and the HUD diamond.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bases {
    pub first: bool,
    pub second: bool,
    pub third: bool,
}

impl Bases {
    fn clear(&mut self) {
        *self = Bases::default();
    }
}

// ── Batted-ball outcomes ──────────────────────────────────────────────────────

/// Flavour of an out, used only for the on-screen banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutKind {
    Ground,
    Fly,
    Pop,
}

/// The result of a batted ball.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Foul,
    Out(OutKind),
    Single,
    Double,
    Triple,
    HomeRun,
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
            .init_resource::<Bases>()
            .add_event::<PlayBanner>()
            .add_systems(OnEnter(GameState::Playing), reset_flow)
            .add_systems(
                Update,
                (pre_pitch, pitch_live, in_play, result_phase)
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// Fresh play + base state whenever a game (re)starts.
fn reset_flow(mut play: ResMut<Play>, mut bases: ResMut<Bases>) {
    *play = Play::default();
    bases.clear();
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
        let target_x = intent.aim.x * 0.35;
        let target_y = 1.0 + intent.aim.y * 0.5;

        let start = mound_reset_pos();
        let flight = PITCH_DISTANCE / PITCH_SPEED;
        let vx = (target_x - start.x) / flight;
        let vy = (target_y - start.y) / flight + 0.5 * GRAVITY * flight;

        pitch_ev.send(PitchEvent {
            velocity: Vec3::new(vx, vy, -PITCH_SPEED),
        });

        play.phase = Phase::Pitch;
        play.crossing = None;
        play.resolved = false;
    }
}

// ── Pitch: batter may swing; otherwise judge the take ─────────────────────────

#[allow(clippy::too_many_arguments)]
fn pitch_live(
    mut play: ResMut<Play>,
    intents: Res<Intents>,
    config: Res<GameConfig>,
    mut score: ResMut<ScoreBoard>,
    mut bases: ResMut<Bases>,
    ball_q: Query<&Transform, With<Baseball>>,
    mut hit_ev: EventWriter<HitEvent>,
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
            let hit = compute_hit(pos.z, intent.aim);
            let outcome = classify_batted_ball(hit.velocity);
            hit_ev.send(hit);
            resolve_contact(outcome, &mut score, &mut bases, &mut banner);
            if outcome == Outcome::Foul {
                end_pitch(&mut play);
            } else {
                play.phase = Phase::InPlay;
                play.timer = Timer::from_seconds(INPLAY_SECS, TimerMode::Once);
                play.resolved = true;
            }
        } else {
            add_strike(&mut score, &mut bases, &mut banner, true);
            end_pitch(&mut play);
        }
        maybe_end_game(&score, &config, &mut next_state);
        return;
    }

    // No swing: once the ball is well past the plate, judge the take.
    if pos.z < SWING_LATE_Z {
        let cross = play.crossing.unwrap_or(Vec2::new(pos.x, pos.y));
        let in_zone =
            cross.x.abs() <= ZONE_HALF_WIDTH && cross.y >= ZONE_LOW && cross.y <= ZONE_HIGH;
        if in_zone {
            add_strike(&mut score, &mut bases, &mut banner, false);
        } else {
            add_ball(&mut score, &mut bases, &mut banner);
        }
        end_pitch(&mut play);
        maybe_end_game(&score, &config, &mut next_state);
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
    mut ball_q: Query<(Entity, &mut Transform, &mut Velocity), With<Baseball>>,
    mut commands: Commands,
) {
    if play.phase != Phase::Result {
        return;
    }
    if play.timer.tick(time.delta()).finished() {
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

// ── Contact → velocity ────────────────────────────────────────────────────────

/// Converts contact timing + aim into a batted-ball velocity.
fn compute_hit(contact_z: f32, aim: Vec2) -> HitEvent {
    let ideal = 0.4_f32;
    let window = (SWING_EARLY_Z - SWING_LATE_Z) * 0.5;
    let quality = (1.0 - (contact_z - ideal).abs() / window).clamp(0.15, 1.0);

    let speed = 22.0 + 24.0 * quality;
    let launch = (8.0 + 30.0 * (aim.y * 0.5 + 0.5)).to_radians();
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

// ── Pure rules (unit-tested below) ────────────────────────────────────────────

/// Classifies a batted ball from its launch velocity by predicting where it
/// lands (drag-free projectile range scaled by [`DRAG_RANGE_FACTOR`]).
pub fn classify_batted_ball(vel: Vec3) -> Outcome {
    // Time to return to ground from the contact height.
    let disc = vel.y * vel.y + 2.0 * GRAVITY * CONTACT_HEIGHT;
    let t = (vel.y + disc.sqrt()) / GRAVITY;

    let land = Vec3::new(vel.x, 0.0, vel.z) * t * DRAG_RANGE_FACTOR;
    let dist = land.length();
    let (x, z) = (land.x, land.z);

    // Fair territory is the 45° wedge opening toward +Z (centre field).
    let fair = z > 1.0 && x.abs() <= z + 0.01;
    if !fair {
        return Outcome::Foul;
    }

    let speed = vel.length().max(0.001);
    let launch_deg = (vel.y / speed).asin().to_degrees();

    // Radial fence: down the lines ≈ 100 m, straightaway centre ≈ 122 m.
    let centeredness = (((z / dist) - 0.707) / (1.0 - 0.707)).clamp(0.0, 1.0);
    let fence = 100.0 + 22.0 * centeredness;

    if dist > fence {
        return Outcome::HomeRun;
    }
    if launch_deg > 55.0 && dist < 50.0 {
        return Outcome::Out(OutKind::Pop);
    }
    if launch_deg > 45.0 && dist < 82.0 {
        return Outcome::Out(OutKind::Fly);
    }
    if dist < 18.0 {
        return Outcome::Out(OutKind::Ground);
    }
    if dist < 40.0 {
        Outcome::Single
    } else if dist < 62.0 {
        Outcome::Double
    } else {
        Outcome::Triple
    }
}

/// Advances runners for a clean hit where everyone moves up `hit_bases`.
/// Returns the number of runs that scored.
pub fn advance_hit(bases: &mut Bases, hit_bases: u32) -> u32 {
    let mut runs = 0;
    let mut next = Bases::default();

    let mut place = |base: u32, runs: &mut u32, next: &mut Bases| {
        let dest = base + hit_bases;
        match dest {
            1 => next.first = true,
            2 => next.second = true,
            3 => next.third = true,
            _ => *runs += 1, // dest >= 4 → scored
        }
    };

    if bases.third {
        place(3, &mut runs, &mut next);
    }
    if bases.second {
        place(2, &mut runs, &mut next);
    }
    if bases.first {
        place(1, &mut runs, &mut next);
    }
    place(0, &mut runs, &mut next); // the batter

    *bases = next;
    runs
}

/// Advances only forced runners for a walk. Returns runs scored (a bases-loaded
/// walk forces in one run).
pub fn advance_walk(bases: &mut Bases) -> u32 {
    if !bases.first {
        bases.first = true;
        0
    } else if !bases.second {
        bases.second = true;
        0
    } else if !bases.third {
        bases.third = true;
        0
    } else {
        1 // bases loaded: everyone forced up one, runner from third scores
    }
}

/// Returns `true` if the game is over given the current score and inning count.
pub fn is_game_over(score: &ScoreBoard, innings: u32) -> bool {
    // Home has won (or walked off) once regulation is reached and it leads while
    // batting/entering the bottom half.
    if !score.top_of_inning && score.inning >= innings && score.home_runs > score.away_runs {
        return true;
    }
    // A completed bottom half (we've advanced past regulation) that is not tied.
    if score.top_of_inning && score.inning > innings && score.home_runs != score.away_runs {
        return true;
    }
    false
}

// ── Count / scoring mutations ─────────────────────────────────────────────────

fn resolve_contact(
    outcome: Outcome,
    score: &mut ScoreBoard,
    bases: &mut Bases,
    banner: &mut EventWriter<PlayBanner>,
) {
    match outcome {
        Outcome::Foul => foul_ball(score, banner),
        Outcome::Out(kind) => {
            let (text, color) = match kind {
                OutKind::Ground => ("GROUND OUT", Color::srgb(1.0, 0.6, 0.4)),
                OutKind::Fly => ("FLY OUT", Color::srgb(1.0, 0.6, 0.4)),
                OutKind::Pop => ("POP OUT", Color::srgb(1.0, 0.6, 0.4)),
            };
            banner.send(PlayBanner::new(text, color));
            record_out(score, bases);
        }
        Outcome::Single => hit(
            score,
            bases,
            banner,
            1,
            "SINGLE",
            Color::srgb(0.7, 1.0, 0.7),
        ),
        Outcome::Double => hit(
            score,
            bases,
            banner,
            2,
            "DOUBLE",
            Color::srgb(0.6, 1.0, 0.8),
        ),
        Outcome::Triple => hit(
            score,
            bases,
            banner,
            3,
            "TRIPLE",
            Color::srgb(0.5, 1.0, 0.9),
        ),
        Outcome::HomeRun => hit(
            score,
            bases,
            banner,
            4,
            "HOME RUN!",
            Color::srgb(1.0, 0.86, 0.2),
        ),
    }
}

fn hit(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    banner: &mut EventWriter<PlayBanner>,
    hit_bases: u32,
    label: &str,
    color: Color,
) {
    let runs = advance_hit(bases, hit_bases);
    score.add_runs(runs);
    let text = if runs > 0 {
        format!("{label}  +{runs}")
    } else {
        label.to_string()
    };
    banner.send(PlayBanner::new(text, color));
    reset_count(score);
}

fn foul_ball(score: &mut ScoreBoard, banner: &mut EventWriter<PlayBanner>) {
    // A foul is a strike unless it would be the third strike.
    if score.strikes < 2 {
        score.strikes += 1;
    }
    banner.send(PlayBanner::new("FOUL", Color::srgb(0.9, 0.9, 0.6)));
}

fn add_ball(score: &mut ScoreBoard, bases: &mut Bases, banner: &mut EventWriter<PlayBanner>) {
    score.balls += 1;
    if score.balls >= 4 {
        let runs = advance_walk(bases);
        score.add_runs(runs);
        banner.send(PlayBanner::new("WALK", Color::srgb(0.6, 0.85, 1.0)));
        reset_count(score);
    } else {
        banner.send(PlayBanner::new("BALL", Color::srgb(0.7, 0.9, 0.7)));
    }
}

fn add_strike(
    score: &mut ScoreBoard,
    bases: &mut Bases,
    banner: &mut EventWriter<PlayBanner>,
    swinging: bool,
) {
    score.strikes += 1;
    if score.strikes >= 3 {
        banner.send(PlayBanner::new("STRIKEOUT!", Color::srgb(1.0, 0.5, 0.35)));
        record_out(score, bases);
    } else if swinging {
        banner.send(PlayBanner::new("SWING & MISS", Color::srgb(1.0, 0.7, 0.4)));
    } else {
        banner.send(PlayBanner::new("STRIKE", Color::srgb(1.0, 0.8, 0.4)));
    }
}

/// Records an out, ends the at-bat, and flips the half-inning after three.
fn record_out(score: &mut ScoreBoard, bases: &mut Bases) {
    reset_count(score);
    score.outs += 1;
    if score.outs >= 3 {
        score.outs = 0;
        bases.clear();
        if score.top_of_inning {
            score.top_of_inning = false;
        } else {
            score.top_of_inning = true;
            score.inning += 1;
        }
    }
}

fn reset_count(score: &mut ScoreBoard) {
    score.balls = 0;
    score.strikes = 0;
}

fn maybe_end_game(score: &ScoreBoard, config: &GameConfig, next_state: &mut NextState<GameState>) {
    if is_game_over(score, config.innings) {
        next_state.set(GameState::GameOver);
    }
}

fn end_pitch(play: &mut Play) {
    play.phase = Phase::Result;
    play.timer = Timer::from_seconds(RESULT_SECS, TimerMode::Once);
    play.resolved = true;
}

// ── Tests: pure base-running & game-over logic ────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Bases {
        Bases::default()
    }

    #[test]
    fn single_puts_batter_on_first() {
        let mut b = empty();
        assert_eq!(advance_hit(&mut b, 1), 0);
        assert_eq!(
            b,
            Bases {
                first: true,
                second: false,
                third: false
            }
        );
    }

    #[test]
    fn single_scores_runner_from_third() {
        let mut b = Bases {
            first: false,
            second: false,
            third: true,
        };
        // Everyone advances one: third scores, batter to first.
        assert_eq!(advance_hit(&mut b, 1), 1);
        assert_eq!(
            b,
            Bases {
                first: true,
                second: false,
                third: false
            }
        );
    }

    #[test]
    fn grand_slam_clears_bases_and_scores_four() {
        let mut b = Bases {
            first: true,
            second: true,
            third: true,
        };
        assert_eq!(advance_hit(&mut b, 4), 4);
        assert_eq!(b, empty());
    }

    #[test]
    fn double_with_runner_on_first() {
        let mut b = Bases {
            first: true,
            second: false,
            third: false,
        };
        // Batter to second, runner from first to third.
        assert_eq!(advance_hit(&mut b, 2), 0);
        assert_eq!(
            b,
            Bases {
                first: false,
                second: true,
                third: true
            }
        );
    }

    #[test]
    fn walk_forces_only_when_bases_ahead_are_occupied() {
        let mut b = empty();
        assert_eq!(advance_walk(&mut b), 0);
        assert!(b.first && !b.second && !b.third);

        // Runner on first: batter forces him to second.
        let mut b = Bases {
            first: true,
            second: false,
            third: false,
        };
        assert_eq!(advance_walk(&mut b), 0);
        assert!(b.first && b.second && !b.third);

        // Bases loaded: forces in a run, still loaded.
        let mut b = Bases {
            first: true,
            second: true,
            third: true,
        };
        assert_eq!(advance_walk(&mut b), 1);
        assert!(b.first && b.second && b.third);
    }

    #[test]
    fn home_run_classifies_as_home_run() {
        // Straightaway centre, ~30° launch, high exit speed.
        let launch = 30.0_f32.to_radians();
        let speed = 46.0;
        let vel = Vec3::new(0.0, speed * launch.sin(), speed * launch.cos());
        assert_eq!(classify_batted_ball(vel), Outcome::HomeRun);
    }

    #[test]
    fn straight_up_is_a_pop_out() {
        let vel = Vec3::new(0.0, 20.0, 2.0);
        assert!(matches!(classify_batted_ball(vel), Outcome::Out(_)));
    }

    #[test]
    fn pulled_way_foul_is_foul() {
        // Mostly sideways: |x| > z → outside the fair wedge.
        let vel = Vec3::new(30.0, 8.0, 5.0);
        assert_eq!(classify_batted_ball(vel), Outcome::Foul);
    }

    #[test]
    fn walkoff_when_home_leads_in_bottom_of_final() {
        let score = ScoreBoard {
            home_runs: 3,
            away_runs: 2,
            inning: 9,
            top_of_inning: false,
            ..Default::default()
        };
        assert!(is_game_over(&score, 9));
    }

    #[test]
    fn tie_after_regulation_goes_to_extras() {
        let score = ScoreBoard {
            home_runs: 2,
            away_runs: 2,
            inning: 10,
            top_of_inning: true,
            ..Default::default()
        };
        assert!(!is_game_over(&score, 9));
    }

    #[test]
    fn away_leads_after_bottom_nine_ends_game() {
        let score = ScoreBoard {
            home_runs: 1,
            away_runs: 4,
            inning: 10,
            top_of_inning: true,
            ..Default::default()
        };
        assert!(is_game_over(&score, 9));
    }
}
