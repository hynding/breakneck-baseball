//! InPlay: the ball is live — turning the fielding simulation's physical
//! reports into the umpire's call, and translating a decided rule result
//! into a banner.

use bevy::prelude::*;

use crate::game::ScoreBoard;
use crate::game::ball::{Baseball, WallBangEvent};
use crate::game::input::Intents;
use crate::game::rules::{self, Bases, BattingOrder, Outcome};
use crate::game::variant::{FieldSpec, Ruleset};

use super::umpire::Umpire;
use super::{BannerTone, LiveBallEvent, Phase, Play, PlayBanner};

/// Backstop on a decided throw still in the air: if the settle report never
/// arrives (a dropped relay edge case), the pending call is announced after
/// this many seconds so the game can never hang on presentation.
// A ~27 m/s throw crosses the diamond in about a second; 2 s covers a relay
// with margin, where the old 4 s let a missed Settled hold a decided call in
// silence (TODO 95).
const THROW_SETTLE_CAP: f32 = 2.0;

/// The offense's send-the-runner gesture: the same held-Down read the live
/// runner call uses ([`rules::runner_call_from_aim`]), so leads, steals, and
/// send-the-batter share one stick convention.
pub(super) fn wants_send(aim: Vec2) -> bool {
    rules::runner_call_from_aim(aim) == rules::RunnerCall::Send
}

/// Ticks the play clock. Resolved plays (home runs, or anything already
/// called by [`resolve_live_play`]) move on to the result pause when the
/// timer runs out; unresolved plays are force-called by `resolve_live_play`,
/// and a decided-but-unannounced throw waits there too (the announcement is
/// what flips the phase).
pub(super) fn in_play(mut play: ResMut<Play>, time: Res<Time>, rules: Res<Ruleset>) {
    if play.phase != Phase::InPlay {
        return;
    }
    play.timer.tick(time.delta());
    if play.resolved && play.live.pending_call.is_none() && play.timer.finished() {
        play.phase = Phase::Result;
        play.timer = Timer::from_seconds(rules.pace.result_secs, TimerMode::Once);
    }
}

/// Turns the fielding simulation's physical reports into the umpire's call.
/// This is where a live ball actually becomes an out, a hit, or a foul —
/// seconds after contact, from what really happened on the grass.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_live_play(
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
) {
    if play.phase != Phase::InPlay {
        events.clear();
        return;
    }
    // A decided throw still in the air: the umpire's call is made, but the
    // announcement waits for the ball to arrive (fielding's Settled report),
    // with the flight cap as a backstop — bang-bang plays look bang-bang.
    if play.resolved {
        let arrived = events.read().any(|ev| matches!(ev, LiveBallEvent::Settled));
        if play.live.pending_call.is_some() && (arrived || play.timer.finished()) {
            let outcome = play.live.pending_call.take().unwrap();
            let batter = score.batting_team();
            Umpire::new(&mut score, &mut bases, &rules_res, &mut banner)
                .resolve_contact(outcome, play.duel.armed);
            if outcome != Outcome::Foul {
                order.advance(batter);
            }
            play.phase = Phase::Result;
            play.timer = Timer::from_seconds(rules_res.pace.result_secs, TimerMode::Once);
        }
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
            LiveBallEvent::Settled => continue,
            LiveBallEvent::Thrown {
                pos,
                base,
                race_time,
            } => {
                let call = rules::runner_call_from_aim(intents.get(score.batting_team()).aim);
                let outcome = rules::resolve_thrown(
                    pos,
                    race_time,
                    base,
                    &bases,
                    play.runners_going(),
                    call,
                    &field,
                    &rules_res,
                );
                // The race is decided the moment the ball leaves the hand —
                // but the play stays visually alive until it lands in the
                // glove: the throw flies, the batter rounds the bases, and
                // only then is the call announced.
                play.live.pending_call = Some(outcome);
                play.resolved = true;
                play.timer = Timer::from_seconds(THROW_SETTLE_CAP, TimerMode::Once);
                return;
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
        let t = time.elapsed_secs() - play.live.contact_at;
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
    let going = play.duel.armed;
    let outcome = resolved.unwrap_or(Outcome::Foul);
    Umpire::new(&mut score, &mut bases, &rules_res, &mut banner).resolve_contact(outcome, going);
    if outcome != Outcome::Foul {
        order.advance(batter);
    }
    play.resolved = true;
    play.phase = Phase::Result;
    play.timer = Timer::from_seconds(rules_res.pace.result_secs, TimerMode::Once);
}

/// A live ball caroms off the wall: one excited call per play. Resolved
/// plays (a rare home run clipping the top of the wall) stay silent — the
/// call was already made.
pub(super) fn announce_wall_bang(
    mut bangs: EventReader<WallBangEvent>,
    mut play: ResMut<Play>,
    mut banner: EventWriter<PlayBanner>,
) {
    let banged = bangs.read().next().is_some();
    if banged && play.phase == Phase::InPlay && !play.resolved && !play.live.wall_called {
        play.live.wall_called = true;
        banner.send(PlayBanner::new("OFF THE WALL!", BannerTone::Good));
    }
}
