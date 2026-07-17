//! Fielder choreography — a cosmetic performance of the outcome that
//! `rules::classify_batted_ball` already decided at contact. Fielders chase
//! the real ball (no teleporting) and move only through `MoveIntent`, the
//! same seam a future player-controlled fielder will write.

use bevy::prelude::*;
use bevy_rapier3d::prelude::Velocity;

use crate::game::animation::{AnimClip, MoveIntent, Playing};
use crate::game::ball::{Baseball, InFlight};
use crate::game::flow::{BallInPlayEvent, Phase, Play};
use crate::game::player::Fielder;
use crate::game::rules::{self, OutKind, Outcome};
use crate::game::variant::FieldSpec;
use crate::game::GameState;

/// Sprint speed while chasing (m/s) and jog speed returning to position.
const CHASE_SPEED: f32 = 7.0;
const RETURN_SPEED: f32 = 4.0;
/// Horizontal distance that counts as "on the ball".
const REACH: f32 = 0.9;
/// Horizontal speed of a cosmetic lob throw.
const THROW_SPEED: f32 = 16.0;

/// Choreography state for the current live ball.
#[derive(Resource, Default)]
struct ActivePlay(PlayState);

#[derive(Default, Clone, Copy, PartialEq)]
enum PlayState {
    #[default]
    Idle,
    /// `chaser` is running the ball down.
    Chasing {
        chaser: Entity,
        outcome: Outcome,
        airborne: bool,
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

/// On contact, the fielder nearest the predicted landing point takes the job.
fn assign_on_contact(
    mut events: EventReader<BallInPlayEvent>,
    field: Res<FieldSpec>,
    mut fielders: Query<(Entity, &Transform, &mut MoveIntent), With<Fielder>>,
    mut active: ResMut<ActivePlay>,
) {
    for ev in events.read() {
        // Don't chase over the fence: cap the run at the warning track.
        let mut target = ev.landing;
        let flat = Vec2::new(target.x, target.z);
        let max = field.fence_line.min(field.fence_center) - 3.0;
        if flat.length() > max {
            let capped = flat.normalize_or_zero() * max;
            target = Vec3::new(capped.x, 0.0, capped.y);
        }

        let Some(chaser) = fielders
            .iter()
            .min_by(|a, b| {
                horizontal_distance(a.1.translation, target)
                    .total_cmp(&horizontal_distance(b.1.translation, target))
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
            outcome: ev.outcome,
            airborne: matches!(ev.outcome, Outcome::Out(OutKind::Fly | OutKind::Pop)),
        };
    }
}

/// Runs the chase to its finish: a catch (fly/pop), or a scoop and throw.
fn chase_and_gather(
    mut active: ResMut<ActivePlay>,
    field: Res<FieldSpec>,
    mut ball_q: Query<(Entity, &Transform, &mut Velocity), With<Baseball>>,
    mut fielders: Query<(Entity, &Transform, &mut MoveIntent), With<Fielder>>,
    mut commands: Commands,
) {
    let PlayState::Chasing {
        chaser,
        outcome,
        airborne,
    } = active.0
    else {
        return;
    };
    let Ok((ball_entity, ball_tf, mut ball_vel)) = ball_q.get_single_mut() else {
        return;
    };
    let ball_pos = ball_tf.translation;

    // The chaser's position first (immutable pass), then its intent.
    let Ok((_, chaser_tf, _)) = fielders.get(chaser) else {
        active.0 = PlayState::Done;
        return;
    };
    let chaser_pos = chaser_tf.translation;
    let d = horizontal_distance(ball_pos, chaser_pos);

    if airborne {
        // Camp under the predicted landing point; glove the ball as it drops in.
        if d < 1.2 && ball_pos.y < 2.4 && ball_vel.linvel.y < 0.0 {
            ball_vel.linvel = Vec3::ZERO;
            ball_vel.angvel = Vec3::ZERO;
            commands.entity(ball_entity).remove::<InFlight>();
            commands
                .entity(chaser)
                .insert(Playing::new(AnimClip::GloveUp));
            if let Ok((_, _, mut intent)) = fielders.get_mut(chaser) {
                intent.target = None;
            }
            active.0 = PlayState::Done;
        }
        return;
    }

    // Grounders and hits: run at the real ball until it's in reach.
    if d < REACH && ball_pos.y < 1.2 {
        commands
            .entity(chaser)
            .insert(Playing::new(AnimClip::ScoopBall));

        match outcome {
            Outcome::Out(OutKind::Ground) | Outcome::Out(OutKind::Pegged) => {
                // Fire it to the fielder nearest first base for the (already
                // recorded) out. FieldSpec has no named positions —
                // nearest-to-base is the rule.
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
            }
            _ => {
                // A hit: lob it back in toward the mound and let it land.
                ball_vel.linvel = lob_velocity(ball_pos, Vec3::new(0.0, 0.6, field.pitch_distance));
                ball_vel.angvel = Vec3::ZERO;
                active.0 = PlayState::Done;
            }
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
