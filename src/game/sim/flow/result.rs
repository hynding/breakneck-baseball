//! Result: the brief pause after a play, plus the rule-result → banner
//! helpers shared by the live-ball and pitch resolutions.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::ball::{Baseball, InFlight};
use crate::game::rules::{self, BallCall, Bases, StealResult, StrikeCall};
use crate::game::runner::RunnersSettled;
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::{GameState, ScoreBoard};

use super::pitch::steal_window_for;
use super::{BannerTone, LeadState, Phase, Play, PlayBanner};

/// Extra seconds the result pause will wait for runner rigs to finish their
/// paths (the home-run trot, a first-to-third sprint) before the next batter
/// steps in — a hard cap so a stray path can never stall the game.
const RESULT_SETTLE_CAP: f32 = 20.0;

#[allow(clippy::too_many_arguments)]
pub(super) fn result_phase(
    mut play: ResMut<Play>,
    time: Res<Time>,
    field: Res<FieldSpec>,
    rules_res: Res<Ruleset>,
    bases: Res<Bases>,
    score: Res<ScoreBoard>,
    settled: Res<RunnersSettled>,
    mut overtime: Local<f32>,
    mut lead: ResMut<LeadState>,
    mut next_state: ResMut<NextState<GameState>>,
    mut ball_q: Query<(Entity, &mut Transform, &mut Velocity, &mut Visibility), With<Baseball>>,
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
    // The play has fully finished on screen — banner shown, runners settled
    // (the walk-off home-run trot included). Only now, once the play looks
    // over, does a decided game actually end: a walk-off's fireworks, slow-mo,
    // and trot all play out before GAME OVER instead of being cut off at
    // contact. Every game-ending call routes through this one Result gate.
    if rules::is_game_over(&score, rules_res.counts.innings) {
        next_state.set(GameState::GameOver);
        return;
    }
    if let Ok((entity, mut transform, mut vel, mut vis)) = ball_q.get_single_mut() {
        transform.translation = rules::mound_reset_pos(field.pitch_distance);
        vel.linvel = Vec3::ZERO;
        vel.angvel = Vec3::ZERO;
        commands.entity(entity).remove::<InFlight>();
        // Safety net: a presentational catch always restores visibility
        // itself (see `catcher_receives`), but a stray edge case must never
        // leave the ball invisible into the next pitch.
        *vis = Visibility::Inherited;
    }
    play.phase = Phase::PrePitch;
    play.crossing = None;
    play.resolved = false;
    play.presentational_catch = false;
    play.pitch_gloved = false;
    play.pending_pitch = None;
    play.live_kind = None;
    play.steal_armed = false;
    play.big_jump = false;
    play.window_lead = false;
    play.pitch_taken = false;
    play.pending_call = None;
    play.wall_called = false;
    play.home_run = false;
    play.last_contact_quality = None;
    // A runner in stealing position opens the duel window for the next at-bat.
    play.hold = steal_window_for(&bases, &rules_res);
    lead.extended = false;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn hit(
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
pub(super) fn add_ball(
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
pub(super) fn resolve_steal(
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

pub(super) fn add_strike(
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

pub(super) fn end_pitch(play: &mut Play, result_secs: f32) {
    play.phase = Phase::Result;
    play.timer = Timer::from_seconds(result_secs, TimerMode::Once);
    play.resolved = true;
}
