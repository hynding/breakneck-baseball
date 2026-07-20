//! Fielder simulation — the defense actually plays the ball. The assigned
//! chaser re-plans its intercept every frame from the live ball's physics
//! (drag and Magnus bend the flight, so the plan shifts as the ball does),
//! and reports what physically happens as [`LiveBallEvent`]s: a catch, the
//! first bounce, a pickup. It never touches the score or bases — flow and
//! the pure race rules turn those reports into the call.
//!
//! All movement flows through `MoveIntent`, the same seam a future
//! player-controlled fielder will write — take over the intent and the
//! resolution races still just read what really happened.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::animation::{AnimClip, MoveIntent, Playing};
use crate::game::ball::{Baseball, InFlight, BALL_DRAG_FACTOR, MAGNUS_FACTOR};
use crate::game::flow::{BallInPlayEvent, LiveBallEvent, Phase, Play};
use crate::game::player::Fielder;
use crate::game::rules;
use crate::game::variant::FieldSpec;
use crate::game::GameState;

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

/// Choreography state for the current live ball.
#[derive(Resource, Default)]
struct ActivePlay(PlayState);

#[derive(Default, Clone, Copy, PartialEq)]
enum PlayState {
    #[default]
    Idle,
    /// `chaser` is playing the ball; `bounced` once it has touched grass.
    Chasing {
        chaser: Entity,
        bounced: bool,
    },
    /// The ball has been gathered and lobbed to `catcher`.
    Thrown {
        catcher: Entity,
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

/// Caps a chase target at the warning track so nobody runs through the wall.
fn cap_at_fence(target: Vec3, field: &FieldSpec) -> Vec3 {
    let flat = Vec2::new(target.x, target.z);
    let max = field.fence_line.min(field.fence_center) - 3.0;
    if flat.length() > max {
        let capped = flat.normalize_or_zero() * max;
        Vec3::new(capped.x, 0.0, capped.y)
    } else {
        target
    }
}

/// On contact, the fielder who can reach the predicted landing point soonest
/// takes the job (first step and sprint speed included — not just whoever is
/// nearest).
fn assign_on_contact(
    mut events: EventReader<BallInPlayEvent>,
    field: Res<FieldSpec>,
    mut fielders: Query<(Entity, &Transform, &mut MoveIntent), With<Fielder>>,
    mut active: ResMut<ActivePlay>,
) {
    for ev in events.read() {
        let target = cap_at_fence(ev.landing, &field);
        let Some(chaser) = fielders
            .iter()
            .min_by(|a, b| {
                rules::catch_time(a.1.translation, target)
                    .total_cmp(&rules::catch_time(b.1.translation, target))
            })
            .map(|(entity, _, _)| entity)
        else {
            continue;
        };
        if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
            intent.target = Some(target);
            intent.speed = CHASE_SPEED;
        }
        active.0 = PlayState::Chasing {
            chaser,
            bounced: false,
        };
    }
}

/// Runs the live play: while the ball is airborne the chaser re-plans its
/// intercept from the ball's actual state each frame and gloves it if it
/// arrives in time; after the bounce it runs the ball down and gathers it.
/// Each physical milestone is reported as a [`LiveBallEvent`].
#[allow(clippy::too_many_arguments)]
fn chase_and_gather(
    mut active: ResMut<ActivePlay>,
    play: Res<Play>,
    field: Res<FieldSpec>,
    mut ball_q: Query<(Entity, &Transform, &mut Velocity), With<Baseball>>,
    mut fielders: Query<(Entity, &Transform, &mut MoveIntent), With<Fielder>>,
    mut reports: EventWriter<LiveBallEvent>,
    mut commands: Commands,
) {
    let PlayState::Chasing { chaser, bounced } = active.0 else {
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
        active.0 = PlayState::Done;
        return;
    };
    let chaser_pos = chaser_tf.translation;
    let d = horizontal_distance(ball_pos, chaser_pos);

    if !bounced {
        // Catch: on the ball as it drops in, before it touches grass.
        if d < 1.2 && ball_pos.y < CATCH_HEIGHT && ball_vel.linvel.y < 0.0 {
            ball_vel.linvel = Vec3::ZERO;
            ball_vel.angvel = Vec3::ZERO;
            commands.entity(ball_entity).remove::<InFlight>();
            commands
                .entity(chaser)
                .insert(Playing::new(AnimClip::GloveUp));
            if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
                intent.target = None;
            }
            reports.send(LiveBallEvent::Caught { pos: ball_pos });
            active.0 = PlayState::Done;
            return;
        }
        // First bounce: the fair/foul call point; from here it's a ground ball.
        if ball_pos.y < BOUNCE_HEIGHT && ball_vel.linvel.y <= 0.0 {
            reports.send(LiveBallEvent::Landed { pos: ball_pos });
            active.0 = PlayState::Chasing {
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
            intent.target = Some(cap_at_fence(landing, &field));
            intent.speed = CHASE_SPEED;
        }
        return;
    }

    // Ground ball: run at the real ball until it's in reach, then gather.
    if d < REACH && ball_pos.y < 1.2 {
        commands
            .entity(chaser)
            .insert(Playing::new(AnimClip::ScoopBall));
        reports.send(LiveBallEvent::Gathered { pos: ball_pos });

        // Cosmetic relay: fire it to the fielder covering first for the race
        // the rules are deciding.
        let first = field.base_positions.first().copied().unwrap_or(Vec3::ZERO);
        let receiver = fielders
            .iter()
            .filter(|(entity, _, _)| *entity != chaser)
            .min_by(|a, b| {
                horizontal_distance(a.1.translation, first)
                    .total_cmp(&horizontal_distance(b.1.translation, first))
            })
            .map(|(entity, tf, _)| (entity, tf.translation));
        if let Some((catcher, catcher_pos)) = receiver {
            ball_vel.linvel = lob_velocity(ball_pos, catcher_pos + Vec3::Y * 0.6);
            ball_vel.angvel = Vec3::ZERO;
            commands
                .entity(catcher)
                .insert(Playing::new(AnimClip::GloveUp));
            active.0 = PlayState::Thrown { catcher };
        } else {
            ball_vel.linvel = Vec3::ZERO;
            active.0 = PlayState::Done;
        }
        if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
            intent.target = None;
        }
    } else if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
        intent.target = Some(ball_pos);
        intent.speed = CHASE_SPEED;
    }
}

/// The throw arrives: the receiver stops the ball dead.
fn receive_throw(
    mut active: ResMut<ActivePlay>,
    mut ball_q: Query<(Entity, &Transform, &mut Velocity), With<Baseball>>,
    fielders: Query<&Transform, With<Fielder>>,
    mut commands: Commands,
) {
    let PlayState::Thrown { catcher } = active.0 else {
        return;
    };
    let Ok((ball_entity, ball_tf, mut ball_vel)) = ball_q.get_single_mut() else {
        return;
    };
    let Ok(catcher_tf) = fielders.get(catcher) else {
        active.0 = PlayState::Done;
        return;
    };
    let arrived = ball_tf.translation.distance(catcher_tf.translation) < 1.0
        || (ball_tf.translation.y < 0.15 && ball_vel.linvel.length() < 2.0);
    if arrived {
        ball_vel.linvel = Vec3::ZERO;
        ball_vel.angvel = Vec3::ZERO;
        commands.entity(ball_entity).remove::<InFlight>();
        active.0 = PlayState::Done;
    }
}

/// During the result pause, everyone jogs back to their spot.
fn return_to_spots(
    play: Res<Play>,
    field: Res<FieldSpec>,
    mut active: ResMut<ActivePlay>,
    mut fielders: Query<(&Fielder, &Transform, &mut MoveIntent)>,
) {
    if play.phase != Phase::Result || active.0 == PlayState::Idle {
        return;
    }
    active.0 = PlayState::Idle;
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
    active.0 = PlayState::Idle;
}

pub struct FieldingPlugin;

impl Plugin for FieldingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ActivePlay>()
            .add_systems(OnEnter(GameState::Playing), reset_active)
            .add_systems(
                Update,
                (
                    assign_on_contact,
                    chase_and_gather,
                    receive_throw,
                    return_to_spots,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
