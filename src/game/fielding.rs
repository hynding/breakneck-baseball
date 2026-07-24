//! Fielder simulation — the defense actually plays the ball. On contact the
//! whole defense reacts: the fielder who can reach the ball soonest chases it
//! (re-planning its intercept every frame as drag and Magnus bend the
//! flight), the free fielders sprint to cover the bases, and the next-best
//! chaser backs the play up. Once the ball is gathered the holder throws for
//! the most reasonable out — or wherever the human defense aims (hold a
//! direction for a base, press action) during the hold window.
//!
//! It never touches the score or bases — every physical milestone is reported
//! as a [`LiveBallEvent`], and flow plus the pure race rules turn those
//! reports into the call. All movement flows through `MoveIntent`, the same
//! seam a future player-controlled fielder will write.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::animation::{AnimClip, MoveIntent, Playing};
use crate::game::ball::{Baseball, InFlight, BALL_DRAG_FACTOR, MAGNUS_FACTOR};
use crate::game::flow::{BallInPlayEvent, LiveBallEvent, Phase, Play};
use crate::game::input::{Controllers, InputSource, Intents};
use crate::game::player::Fielder;
use crate::game::rules;
use crate::game::variant::{FieldSpec, Ruleset};
use crate::game::{GameState, ScoreBoard};

/// Sprint speed while chasing (m/s) — mirrors `rules::FIELDER_SPEED` so the
/// race maths and the legs agree — and jog speed returning to position.
const CHASE_SPEED: f32 = rules::FIELDER_SPEED;
const RETURN_SPEED: f32 = 4.0;
/// Horizontal distance that counts as "on the ball".
const REACH: f32 = 0.9;
/// Ball height below which a descending ball is catchable / has bounced.
const CATCH_HEIGHT: f32 = 2.4;
const BOUNCE_HEIGHT: f32 = 0.25;
/// Horizontal speed of a cosmetic lob throw.
const THROW_SPEED: f32 = 16.0;
/// How long the holder waits for a manual throw before making the smart one.
const AUTO_THROW_DELAY: f32 = 0.6;
/// How far beyond the landing spot the backup fielder stations itself.
const BACKUP_DEPTH: f32 = 6.0;
/// How far inside the wall an aerial intercept is allowed to plan.
const INTERCEPT_FENCE_MARGIN: f32 = 2.0;
/// Ground chases may go nearly to the wall pad to dig a ball out.
const GROUND_FENCE_MARGIN: f32 = 0.6;

/// Choreography state for the current live ball.
#[derive(Resource, Default)]
struct ActivePlay {
    state: PlayState,
    /// Base index (`base_count()` = home) → the fielder sent to cover it.
    cover: Vec<(usize, Entity)>,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum PlayState {
    #[default]
    Idle,
    /// `chaser` is playing the ball; `bounced` once it has touched grass.
    Chasing {
        chaser: Entity,
        bounced: bool,
    },
    /// `holder` has the ball and may throw: manually (defense aims a base and
    /// presses action) until `held_at + AUTO_THROW_DELAY`, then automatically
    /// for the most reasonable out. `race_time` is the gather instant on the
    /// contact clock — the backdated time an auto-throw resolves with.
    Holding {
        holder: Entity,
        held_at: f32,
        race_time: f32,
    },
    /// The ball has been thrown and is on its way to `catcher`. When the
    /// throw started a double play, `relay_to` is the bag the receiver fires
    /// on to (the visible 6-4-3) — pure choreography, the call is made.
    Thrown {
        catcher: Entity,
        relay_to: Option<usize>,
    },
    Done,
}

/// A lobbed velocity from `from` to `to` under gravity.
fn lob_velocity(from: Vec3, to: Vec3) -> Vec3 {
    let flat = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    let d = flat.length().max(0.1);
    let t = (d / THROW_SPEED).clamp(0.4, 1.2);
    let vy = 0.5 * rules::GRAVITY * t + (to.y - from.y) / t;
    flat / t + Vec3::Y * vy
}

fn horizontal_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

/// Caps a target `margin` metres inside the wall (the spec fence via
/// [`rules::fence_at`]) so nobody plans a route through it.
fn cap_inside_fence(target: Vec3, field: &FieldSpec, margin: f32) -> Vec3 {
    let flat = Vec2::new(target.x, target.z);
    let max = rules::fence_at(target, field) - margin;
    if flat.length() > max {
        let capped = flat.normalize_or_zero() * max;
        Vec3::new(capped.x, 0.0, capped.y)
    } else {
        target
    }
}

/// World position of covered base `base` (`base_count()` = home plate).
fn cover_pos(field: &FieldSpec, base: usize) -> Vec3 {
    if base == field.base_count() {
        Vec3::ZERO
    } else {
        field.base_positions[base]
    }
}

/// On contact the whole defense reacts: the fielder who can reach the
/// predicted landing point soonest takes the chase (first step and sprint
/// speed included — not just whoever is nearest), each base gets covered by
/// the nearest free fielder, and the best remaining fielder backs up the
/// landing spot from deeper.
fn assign_on_contact(
    mut events: EventReader<BallInPlayEvent>,
    field: Res<FieldSpec>,
    mut fielders: Query<(Entity, &Transform, &mut MoveIntent), With<Fielder>>,
    mut active: ResMut<ActivePlay>,
) {
    for ev in events.read() {
        let target = cap_inside_fence(ev.landing, &field, INTERCEPT_FENCE_MARGIN);

        // Everyone, ranked by how soon they could reach the landing point.
        let mut ranked: Vec<(Entity, Vec3)> = fielders
            .iter()
            .map(|(entity, tf, _)| (entity, tf.translation))
            .collect();
        ranked.sort_by(|a, b| {
            rules::catch_time(a.1, target).total_cmp(&rules::catch_time(b.1, target))
        });
        let Some(&(chaser, _)) = ranked.first() else {
            continue;
        };
        if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
            intent.target = Some(target);
            intent.speed = CHASE_SPEED;
        }

        // Cover every base (home included) with the nearest free fielder.
        let mut free: Vec<(Entity, Vec3)> = ranked
            .iter()
            .filter(|(entity, _)| *entity != chaser)
            .copied()
            .collect();
        active.cover.clear();
        for base in 0..=field.base_count() {
            let spot = cover_pos(&field, base);
            let Some(i) = free
                .iter()
                .enumerate()
                .min_by(|a, b| {
                    horizontal_distance(a.1 .1, spot).total_cmp(&horizontal_distance(b.1 .1, spot))
                })
                .map(|(i, _)| i)
            else {
                break;
            };
            let (coverer, _) = free.remove(i);
            if let Ok((_, _, mut intent)) = fielders.get_mut(coverer) {
                intent.target = Some(spot);
                intent.speed = CHASE_SPEED;
            }
            active.cover.push((base, coverer));
        }

        // The best fielder still free trails the play from behind the
        // landing spot in case the ball gets past the chaser.
        if let Some(&(backup, _)) = free.first() {
            let out = Vec3::new(target.x, 0.0, target.z).normalize_or_zero();
            let spot =
                cap_inside_fence(target + out * BACKUP_DEPTH, &field, INTERCEPT_FENCE_MARGIN);
            if let Ok((_, _, mut intent)) = fielders.get_mut(backup) {
                intent.target = Some(spot);
                intent.speed = CHASE_SPEED;
            }
        }

        active.state = PlayState::Chasing {
            chaser,
            bounced: false,
        };
    }
}

/// Query alias for reading fielder positions while the ball query is also
/// live (keeps clippy's type-complexity check happy).
type FielderSpots<'w, 's> =
    Query<'w, 's, (Entity, &'static Transform), (With<Fielder>, Without<Baseball>)>;

/// Runs the live chase: while the ball is airborne the chaser re-plans its
/// intercept from the ball's actual state each frame and gloves it if it
/// arrives in time; after the bounce it runs the ball down and gathers it
/// into the hold. Each physical milestone is reported as a [`LiveBallEvent`].
#[allow(clippy::too_many_arguments)]
fn chase_and_gather(
    time: Res<Time>,
    mut active: ResMut<ActivePlay>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    mut ball_q: Query<(Entity, &Transform, &mut Velocity), With<Baseball>>,
    mut fielders: Query<(Entity, &Transform, &mut MoveIntent), With<Fielder>>,
    mut reports: EventWriter<LiveBallEvent>,
    mut commands: Commands,
) {
    let PlayState::Chasing { chaser, bounced } = active.state else {
        return;
    };
    // The play was called (foul, or the clock ran out): stop performing.
    if play.phase != Phase::InPlay {
        return;
    }
    let Ok((ball_entity, ball_tf, mut ball_vel)) = ball_q.get_single_mut() else {
        return;
    };
    let ball_pos = ball_tf.translation;

    let Ok((_, chaser_tf, _)) = fielders.get(chaser) else {
        active.state = PlayState::Done;
        return;
    };
    let chaser_pos = chaser_tf.translation;
    let d = horizontal_distance(ball_pos, chaser_pos);

    if !bounced {
        // Catch: on the ball as it drops in, before it touches grass. A ball
        // at the edge of reach is a full-extension dive; anything closer is a
        // routine glove-up.
        if d < 1.2 && ball_pos.y < CATCH_HEIGHT && ball_vel.linvel.y < 0.0 {
            ball_vel.linvel = Vec3::ZERO;
            ball_vel.angvel = Vec3::ZERO;
            commands.entity(ball_entity).remove::<InFlight>();
            let catch_clip = if d > 0.85 {
                AnimClip::Dive
            } else {
                AnimClip::GloveUp
            };
            commands.entity(chaser).insert(Playing::new(catch_clip));
            if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
                intent.target = None;
            }
            reports.send(LiveBallEvent::Caught { pos: ball_pos });
            active.state = PlayState::Done;
            return;
        }
        // First bounce: the fair/foul call point; from here it's a ground ball.
        if ball_pos.y < BOUNCE_HEIGHT && ball_vel.linvel.y <= 0.0 {
            reports.send(LiveBallEvent::Landed { pos: ball_pos });
            active.state = PlayState::Chasing {
                chaser,
                bounced: true,
            };
            return;
        }
        // Still in the air: re-plan the intercept from the live ball state —
        // this is where the chaser "adjusts" as drag and Magnus bend the ball.
        let (landing, _) = rules::predict_landing_from(
            ball_pos,
            ball_vel.linvel,
            ball_vel.angvel,
            BALL_DRAG_FACTOR,
            MAGNUS_FACTOR,
        );
        if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
            intent.target = Some(cap_inside_fence(landing, &field, INTERCEPT_FENCE_MARGIN));
            intent.speed = CHASE_SPEED;
        }
        return;
    }

    // Ground ball: run at the real ball until it's in reach, then gather it
    // into the hold — the throw decision comes in `hold_and_throw`.
    if d < REACH && ball_pos.y < 1.2 {
        ball_vel.linvel = Vec3::ZERO;
        ball_vel.angvel = Vec3::ZERO;
        commands
            .entity(chaser)
            .insert(Playing::new(AnimClip::ScoopBall));
        if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
            intent.target = None;
        }
        active.state = PlayState::Holding {
            holder: chaser,
            held_at: time.elapsed_secs(),
            race_time: play.since_contact(time.elapsed_secs()),
        };
    } else if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
        intent.target = Some(cap_inside_fence(ball_pos, &field, GROUND_FENCE_MARGIN));
        intent.speed = CHASE_SPEED;
    }
}

/// The holder throws: to whatever base the human defense aims (hold the
/// base's direction and press action) on the honest race clock, or, once the
/// hold window lapses, to the most reasonable out on the backdated gather
/// clock — so the analytic outcome never punishes the presentation delay.
#[allow(clippy::too_many_arguments)]
fn hold_and_throw(
    time: Res<Time>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    ruleset: Res<Ruleset>,
    bases: Res<rules::Bases>,
    score: Res<ScoreBoard>,
    intents: Res<Intents>,
    mut active: ResMut<ActivePlay>,
    mut ball_q: Query<(&mut Transform, &mut Velocity), With<Baseball>>,
    fielders: FielderSpots,
    mut reports: EventWriter<LiveBallEvent>,
    mut commands: Commands,
) {
    let PlayState::Holding {
        holder,
        held_at,
        race_time,
    } = active.state
    else {
        return;
    };
    if play.phase != Phase::InPlay {
        return;
    }
    let Ok((mut ball_tf, mut ball_vel)) = ball_q.get_single_mut() else {
        return;
    };

    // The ball sits in the holder's glove while the decision is made.
    if let Ok((_, holder_tf)) = fielders.get(holder) {
        ball_tf.translation = holder_tf.translation + Vec3::Y * 0.5;
    }
    ball_vel.linvel = Vec3::ZERO;
    ball_vel.angvel = Vec3::ZERO;

    let now = time.elapsed_secs();
    let intent = intents.get(score.fielding_team());
    let manual = if intent.action {
        rules::aimed_base(intent.aim, &field)
    } else {
        None
    };
    let going = play.runners_going();
    let (base, throw_time) = if let Some(base) = manual {
        (base, play.since_contact(now))
    } else if now - held_at >= AUTO_THROW_DELAY {
        let target = rules::throw_target(ball_tf.translation, race_time, &bases, going, &field);
        (target, race_time)
    } else {
        return;
    };

    // Same pure race flow will run: if this throw turns two, the receiver
    // fires the visible relay on to first.
    let call = rules::runner_call_from_aim(intents.get(score.batting_team()).aim);
    let outcome = rules::resolve_thrown(
        ball_tf.translation,
        throw_time,
        base,
        &bases,
        going,
        call,
        &field,
        &ruleset,
    );
    let relay_to = matches!(outcome, rules::Outcome::DoublePlay).then_some(0);

    let target_pos = cover_pos(&field, base);
    let receiver = active
        .cover
        .iter()
        .find(|(b, _)| *b == base)
        .map(|&(_, entity)| entity)
        .or_else(|| {
            fielders
                .iter()
                .filter(|(entity, _)| *entity != holder)
                .min_by(|a, b| {
                    horizontal_distance(a.1.translation, target_pos)
                        .total_cmp(&horizontal_distance(b.1.translation, target_pos))
                })
                .map(|(entity, _)| entity)
        });

    let from = ball_tf.translation;
    ball_vel.linvel = lob_velocity(from, target_pos + Vec3::Y * 0.6);
    ball_vel.angvel = Vec3::ZERO;
    if let Some(receiver) = receiver {
        commands
            .entity(receiver)
            .insert(Playing::new(AnimClip::GloveUp));
    }
    reports.send(LiveBallEvent::Thrown {
        pos: from,
        base,
        race_time: throw_time,
    });
    active.state = PlayState::Thrown {
        catcher: receiver.unwrap_or(holder),
        relay_to,
    };
}

/// The throw arrives: the receiver stops the ball dead — or, on a double
/// play, pivots and fires the relay leg on to the next bag. The final
/// arrival is reported as [`LiveBallEvent::Settled`]: the cue flow waits
/// for before announcing a call that was decided at the throw.
fn receive_throw(
    field: Res<FieldSpec>,
    mut active: ResMut<ActivePlay>,
    mut ball_q: Query<(Entity, &Transform, &mut Velocity), With<Baseball>>,
    fielders: FielderSpots,
    mut reports: EventWriter<LiveBallEvent>,
    mut commands: Commands,
) {
    let PlayState::Thrown { catcher, relay_to } = active.state else {
        return;
    };
    let Ok((ball_entity, ball_tf, mut ball_vel)) = ball_q.get_single_mut() else {
        return;
    };
    let Ok((_, catcher_tf)) = fielders.get(catcher) else {
        active.state = PlayState::Done;
        reports.send(LiveBallEvent::Settled);
        return;
    };
    let arrived = ball_tf.translation.distance(catcher_tf.translation) < 1.0
        || (ball_tf.translation.y < 0.15 && ball_vel.linvel.length() < 2.0);
    if !arrived {
        return;
    }
    if let Some(base) = relay_to {
        let target_pos = cover_pos(&field, base);
        let pivot = active
            .cover
            .iter()
            .find(|(b, _)| *b == base)
            .map(|&(_, entity)| entity)
            .filter(|&entity| entity != catcher)
            .or_else(|| {
                fielders
                    .iter()
                    .filter(|(entity, _)| *entity != catcher)
                    .min_by(|a, b| {
                        horizontal_distance(a.1.translation, target_pos)
                            .total_cmp(&horizontal_distance(b.1.translation, target_pos))
                    })
                    .map(|(entity, _)| entity)
            });
        if let Some(pivot) = pivot {
            ball_vel.linvel = lob_velocity(ball_tf.translation, target_pos + Vec3::Y * 0.6);
            ball_vel.angvel = Vec3::ZERO;
            commands
                .entity(pivot)
                .insert(Playing::new(AnimClip::GloveUp));
            active.state = PlayState::Thrown {
                catcher: pivot,
                relay_to: None,
            };
            return;
        }
    }
    ball_vel.linvel = Vec3::ZERO;
    ball_vel.angvel = Vec3::ZERO;
    commands.entity(ball_entity).remove::<InFlight>();
    active.state = PlayState::Done;
    reports.send(LiveBallEvent::Settled);
}

/// A human defense steers the chaser directly: while the stick is deflected
/// the fielder runs where it points (screen aim → world, same mapping as the
/// throw selector) instead of the auto intercept — risk and reward, since a
/// missed route means a dropped-in hit. Runs after `chase_and_gather` so the
/// override wins the frame; the CPU never steers.
fn steer_chaser(
    play: Res<Play>,
    score: Res<ScoreBoard>,
    controllers: Res<Controllers>,
    intents: Res<Intents>,
    field: Res<FieldSpec>,
    active: Res<ActivePlay>,
    mut fielders: Query<(&Transform, &mut MoveIntent), With<Fielder>>,
) {
    let PlayState::Chasing { chaser, .. } = active.state else {
        return;
    };
    if play.phase != Phase::InPlay {
        return;
    }
    if controllers.source(score.fielding_team()) == InputSource::Cpu {
        return;
    }
    let aim = intents.get(score.fielding_team()).aim;
    if aim.length() < 0.5 {
        return;
    }
    let dir = Vec3::new(-aim.x, 0.0, aim.y).normalize_or_zero();
    if let Ok((tf, mut intent)) = fielders.get_mut(chaser) {
        intent.target = Some(cap_inside_fence(
            tf.translation + dir * 4.0,
            &field,
            GROUND_FENCE_MARGIN,
        ));
        intent.speed = CHASE_SPEED;
    }
}

/// During the result pause, everyone jogs back to their spot.
fn return_to_spots(
    play: Res<Play>,
    field: Res<FieldSpec>,
    mut active: ResMut<ActivePlay>,
    mut fielders: Query<(&Fielder, &Transform, &mut MoveIntent)>,
) {
    if play.phase != Phase::Result || active.state == PlayState::Idle {
        return;
    }
    active.state = PlayState::Idle;
    active.cover.clear();
    for (fielder, tf, mut intent) in &mut fielders {
        let Some(spot) = field.fielder_positions.get(fielder.index) else {
            continue;
        };
        if Vec2::new(tf.translation.x - spot.x, tf.translation.z - spot.z).length() > 0.3 {
            intent.target = Some(*spot);
            intent.speed = RETURN_SPEED;
        }
    }
}

fn reset_active(mut active: ResMut<ActivePlay>) {
    active.state = PlayState::Idle;
    active.cover.clear();
}

pub struct FieldingPlugin;

impl Plugin for FieldingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActivePlay>()
            .add_systems(crate::game::game_start(), reset_active)
            .add_systems(
                Update,
                (
                    assign_on_contact,
                    chase_and_gather,
                    steer_chaser,
                    hold_and_throw,
                    receive_throw,
                    return_to_spots,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
