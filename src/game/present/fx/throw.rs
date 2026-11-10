//! The running-game throw, seen: a pickoff from the pitcher's hand or the
//! catcher's throw on a steal flies to its bag along a flat arc, timed off
//! `flow`'s staging (`Play::duel_throw`) so the ball lands exactly when the
//! call does. The gameplay ball is hidden while its stand-in flies — it is
//! sitting on the mound or in the mitt, and two balls would read as a bug.

use bevy::prelude::*;

use crate::game::GameplayEntity;
use crate::game::ball::Baseball;
use crate::game::flow::{DuelThrowKind, Play};
use crate::game::player::{CatcherRole, Pitcher};
use crate::game::variant::FieldSpec;

/// The stand-in ball for a running-game throw. Public so headless tests can
/// watch it fly.
#[derive(Component)]
pub struct DuelThrowBall {
    from: Vec3,
}

/// Where the thrower's hand is, above his rig root.
const HAND_HEIGHT: f32 = 1.2;
/// Where the cover man's glove takes it, above the bag.
const GLOVE_HEIGHT: f32 = 1.0;
/// The throw's peak above the straight line, per metre thrown — a firm,
/// flat throw, not a lob.
const ARC_PER_M: f32 = 0.06;

/// The ball's position `fraction` of the way along a throw from `from` to
/// `to` (a parabola over the straight line, peaking mid-flight).
pub fn throw_arc(from: Vec3, to: Vec3, fraction: f32) -> Vec3 {
    let f = fraction.clamp(0.0, 1.0);
    let lift = ARC_PER_M * from.distance(to) * 4.0 * f * (1.0 - f);
    from.lerp(to, f) + Vec3::Y * lift
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn stage_duel_throw(
    play: Res<Play>,
    field: Res<FieldSpec>,
    mut commands: Commands,
    mut real: Query<
        (&Mesh3d, &MeshMaterial3d<StandardMaterial>, &mut Visibility),
        (With<Baseball>, Without<DuelThrowBall>),
    >,
    mut ghosts: Query<(Entity, &mut Transform, &DuelThrowBall)>,
    pitchers: Query<&Transform, (With<Pitcher>, Without<DuelThrowBall>)>,
    catchers: Query<&Transform, (With<CatcherRole>, Without<DuelThrowBall>)>,
    mut hid_real: Local<bool>,
) {
    let Some(throw) = play.duel_throw() else {
        for (entity, ..) in &ghosts {
            commands.entity(entity).despawn();
        }
        if std::mem::take(&mut *hid_real) {
            for (_, _, mut vis) in &mut real {
                *vis = Visibility::Inherited;
            }
        }
        return;
    };
    let to = field.base_positions[throw.target] + Vec3::Y * GLOVE_HEIGHT;
    if ghosts.is_empty() {
        let thrower = match throw.kind {
            DuelThrowKind::Pickoff => pitchers.iter().next(),
            DuelThrowKind::Steal => catchers.iter().next(),
        };
        let from = thrower.map_or(Vec3::Y * HAND_HEIGHT, |tf| {
            tf.translation + Vec3::Y * HAND_HEIGHT
        });
        if let Some((mesh, material, mut vis)) = real.iter_mut().next() {
            commands.spawn((
                DuelThrowBall { from },
                GameplayEntity,
                mesh.clone(),
                material.clone(),
                Transform::from_translation(from),
            ));
            *vis = Visibility::Hidden;
            *hid_real = true;
        }
        return;
    }
    for (_, mut tf, ball) in &mut ghosts {
        tf.translation = throw_arc(ball.from, to, throw.flight_fraction());
    }
}

#[cfg(test)]
#[path = "throw.test.rs"]
mod tests;
