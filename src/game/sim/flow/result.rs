//! Result: the brief pause after a play, plus the rule-result → banner
//! helpers shared by the live-ball and pitch resolutions.

use bevy::prelude::*;
use bevy_rapier3d::prelude::*;

use crate::game::ball::{Baseball, InFlight};
use crate::game::rules::{self, Bases};
use crate::game::runner::RunnersSettled;
use crate::game::variant::{FieldSpec, PaceTuning, Ruleset};
use crate::game::{GameState, ScoreBoard};

use super::pitch::steal_window_for;
use super::{LeadState, Phase, Play, ResultBeat};

/// Extra seconds the result pause will wait for runner rigs to finish their
/// paths (the home-run trot, a first-to-third sprint) before the next batter
/// steps in — a hard cap so a stray path can never stall the game.
// 10 s comfortably clears the longest legitimate path (the brisk HR trot,
// ~9 s including its 0.9 s look) — the old 20 s only bought stuck rigs more
// dead air (TODO 95).
const RESULT_SETTLE_CAP: f32 = 10.0;

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
    if !play.timer.tick(time.delta()).is_finished() {
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
    // A dipping beat closes the curtain before anything resets: the phase
    // flips — and the ball teleports to the mound — on a black screen, the
    // way the reference footage hides every reset (TODO 101).
    if let Some(beat) = play.beat.filter(|b| b.dips()) {
        let curtain = play
            .curtain
            .get_or_insert_with(|| Timer::from_seconds(beat.curtain_secs(), TimerMode::Once));
        if !curtain.tick(time.delta()).is_finished() {
            return;
        }
    }
    // The play has fully finished on screen — banner shown, runners settled
    // (the walk-off home-run trot included). Only now, once the play looks
    // over, does a decided game actually end: a walk-off's fireworks, slow-mo,
    // and trot all play out before GAME OVER instead of being cut off at
    // contact. Every game-ending call routes through this one Result gate.
    if rules::is_game_over(&score, rules_res.counts.innings) {
        next_state.set(GameState::GameOver);
        return;
    }
    if let Ok((entity, mut transform, mut vel, mut vis)) = ball_q.single_mut() {
        transform.translation = rules::mound_reset_pos(field.pitch_distance);
        vel.linvel = Vec3::ZERO;
        vel.angvel = Vec3::ZERO;
        commands.entity(entity).remove::<InFlight>();
        // Safety net: a presentational catch always restores visibility
        // itself (see `catcher_receives`), but a stray edge case must never
        // leave the ball invisible into the next pitch.
        *vis = Visibility::Inherited;
    }
    // A beat that ended the plate appearance introduces the next batter
    // before the ball is live again (TODO 103): the walk-up holds the
    // pitch, and the curtain opens on it rather than on the duel.
    play.walkup = play
        .beat
        .is_some_and(ResultBeat::ends_plate_appearance)
        .then(|| Timer::from_seconds(rules_res.pace.walkup_secs, TimerMode::Once));
    play.phase = Phase::PrePitch;
    play.beat = None;
    play.curtain = None;
    play.pitch.crossing = None;
    play.resolved = false;
    play.pitch.presentational_catch = false;
    play.pitch.gloved = false;
    play.pitch.pending = None;
    play.pitch.kind = None;
    play.duel.armed = false;
    play.duel.big_jump = false;
    play.duel.window_lead = false;
    play.pitch.taken = false;
    play.live.pending_call = None;
    play.live.wall_called = false;
    play.live.home_run = false;
    play.live.last_contact_quality = None;
    play.live.last_outcome = None;
    play.pitch.last_strike_call = None;
    // A runner in stealing position opens the duel window for the next at-bat.
    play.duel.hold = steal_window_for(&bases, &rules_res);
    lead.extended = false;
}

/// Ends the pitch or play into its result pause: the hold `pace` gives
/// `beat`, then (for every beat but a taken ball) the curtain, then the
/// PrePitch reset. Every path into `Phase::Result` comes through here so
/// the beat is never left unset.
pub(super) fn end_pitch(play: &mut Play, pace: &PaceTuning, beat: ResultBeat) {
    play.phase = Phase::Result;
    play.timer = Timer::from_seconds(pace.result_secs_for(beat), TimerMode::Once);
    play.resolved = true;
    play.beat = Some(beat);
    play.curtain = None;
}
