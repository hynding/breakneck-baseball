//! The two `Playing` backends — the procedural [`sample_clips`] sampler for
//! `Blocky` rigs and the glTF `AnimationGraph` driver ([`drive_graph_rigs`]
//! and friends) — plus locomotion ([`locomote`]) and the Swing Meter's
//! stance sink ([`meter_stance_sink`]). Both backends pose from the same
//! [`super::poses`] math so a rig looks identical regardless of which one
//! drives it.

use std::time::Duration;

use bevy::prelude::*;

use crate::game::model_assets::{RigAnimations, RigPlayer};

use super::poses::{bat_idle_rotation, limb_pose, root_drop, root_pitch, self_pose};
use super::{AnimClip, MoveIntent, Playing, RigBaseY, RigLimb};

/// Poses every playing rig from `(clip, progress)`, chains `next`, and removes
/// finished one-shots. The only code in the game that rotates rig parts.
/// Query alias for every rig currently playing a clip (keeps clippy's
/// type-complexity check happy).
type PlayingRigs<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static mut Playing,
        &'static mut Transform,
        Option<&'static Children>,
        Option<&'static RigBaseY>,
    ),
    Without<RigPlayer>,
>;

pub(super) fn sample_clips(
    time: Res<Time>,
    mut commands: Commands,
    mut playing_q: PlayingRigs,
    mut limb_q: Query<(&RigLimb, &mut Transform), Without<Playing>>,
) {
    for (entity, mut playing, mut transform, children, base_y) in &mut playing_q {
        playing.timer.tick(time.delta());
        let f = playing.timer.fraction();

        if let Some(rot) = self_pose(playing.clip, f) {
            transform.rotation = rot;
        } else if let Some(children) = children {
            for &child in children {
                if let Ok((limb, mut limb_tf)) = limb_q.get_mut(child) {
                    limb_tf.rotation = limb_pose(playing.clip, limb.kind, f);
                }
            }
        }
        if let Some(base) = base_y {
            transform.translation.y = base.0 - root_drop(playing.clip, f);
            // Body lean rides on top of whatever facing locomotion set.
            let pitch = root_pitch(playing.clip, f);
            if pitch != 0.0 {
                let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
                transform.rotation = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
            }
        }

        if playing.timer.finished() && !playing.clip.looping() {
            if let Some(next) = playing.next.take() {
                playing.clip = next;
                // Mode-aware re-arm, mirroring `Playing::new`: a chained clip
                // that loops (e.g. a fidget chaining back into a held
                // stance) must keep repeating, not fire this branch again
                // next frame and immediately fall into the `else` removal.
                let mode = if next.looping() {
                    TimerMode::Repeating
                } else {
                    TimerMode::Once
                };
                playing.timer = Timer::from_seconds(next.duration(), mode);
            } else {
                if self_pose(playing.clip, 1.0).is_some() {
                    transform.rotation = bat_idle_rotation();
                }
                commands.entity(entity).remove::<Playing>();
            }
        }
    }
}

/// Query alias for `settle_removed`'s root lookup (keeps clippy's
/// type-complexity check happy) — Blocky rig roots only, glTF rigs settle via
/// [`settle_graph_removed`] instead.
type SettledRoots<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static RigBaseY),
    (Without<RigLimb>, Without<RigPlayer>),
>;

/// Returns limbs to neutral and the root to its resting height whenever a
/// clip stops (covers both the sampler's own removal and choreography
/// removing `RunCycle` mid-loop).
pub(super) fn settle_removed(
    mut removed: RemovedComponents<Playing>,
    children_q: Query<&Children>,
    mut limb_q: Query<(&RigLimb, &mut Transform)>,
    mut root_q: SettledRoots,
) {
    for entity in removed.read() {
        if let Ok(children) = children_q.get(entity) {
            for &child in children {
                if let Ok((_, mut limb_tf)) = limb_q.get_mut(child) {
                    limb_tf.rotation = Quat::IDENTITY;
                }
            }
        }
        if let Ok((mut root_tf, base)) = root_q.get_mut(entity) {
            root_tf.translation.y = base.0;
            // Straighten any body lean, keeping only the facing yaw.
            let (yaw, _, _) = root_tf.rotation.to_euler(EulerRot::YXZ);
            root_tf.rotation = Quat::from_rotation_y(yaw);
        }
    }
}

/// The Swing Meter's visible load: the batter's stance deepens as the meter
/// fills — a bounded root sink composed over `RigBaseY`, owned here because
/// `game::animation` owns rig root height. Runs last so it wins the frame over the
/// settle systems that restore the batter root to `RigBaseY`.
const METER_SINK_M: f32 = 0.12;
pub(super) fn meter_stance_sink(
    load: Res<crate::game::batting::MeterLoad>,
    mut batters: Query<(&mut Transform, &RigBaseY), With<crate::game::player::Batter>>,
) {
    for (mut tf, base) in &mut batters {
        tf.translation.y = base.0 - load.0 * METER_SINK_M;
    }
}

/// Distance that counts as "arrived".
const ARRIVE_EPS: f32 = 0.2;

/// Steps every rig with a [`MoveIntent`] toward its target, faces it along the
/// travel direction, and keeps `RunCycle` playing while it moves.
pub(super) fn locomote(
    time: Res<Time>,
    mut commands: Commands,
    mut movers: Query<(Entity, &mut Transform, &mut MoveIntent, Option<&Playing>)>,
) {
    let dt = time.delta_secs();
    for (entity, mut transform, mut intent, playing) in &mut movers {
        let Some(target) = intent.target else {
            // No target to run to: shed any lingering RunCycle so the rig
            // settles instead of jogging in place forever. `RunCycle` is
            // locomote's own clip, so locomote is what takes it back off —
            // and it must do so whenever the target is gone, not only on the
            // frame the mover happens to arrive. Otherwise a target cleared
            // by another system (e.g. a fielder pulled off a cover as the play
            // ends) strands the clip: nothing else removes RunCycle, and a
            // stuck runner (like the catcher) never re-crouches for the duel.
            if playing.is_some_and(|p| p.clip == AnimClip::RunCycle) {
                commands.entity(entity).remove::<Playing>();
            }
            continue;
        };
        let mut to = target - transform.translation;
        to.y = 0.0; // rigs stay at their spawn height
        let dist = to.length();
        if dist <= ARRIVE_EPS {
            intent.target = None;
            if playing.is_some_and(|p| p.clip == AnimClip::RunCycle) {
                commands.entity(entity).remove::<Playing>();
            }
            continue;
        }
        let dir = to / dist;
        transform.translation += dir * (intent.speed * dt).min(dist);
        transform.rotation = Quat::from_rotation_y(dir.x.atan2(dir.z));
        if playing.is_none() {
            commands
                .entity(entity)
                .insert(Playing::new(AnimClip::RunCycle));
        }
    }
}

/// Cross-fade length between clips — the production-blending upgrade.
const BLEND: Duration = Duration::from_millis(150);

/// Starts `clip` on a rig's AnimationPlayer with a cross-fade, applying the
/// speed factor and loop mode. The one place transitions are touched.
fn start_clip(
    anims: &RigAnimations,
    players: &mut Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
    rig: &mut RigPlayer,
    clip: AnimClip,
) {
    let Ok((mut player, mut transitions)) = players.get_mut(rig.player) else {
        return;
    };
    let (node, speed) = anims.node(clip);
    let anim = transitions.play(&mut player, node, BLEND);
    anim.set_speed(speed);
    if clip.looping() {
        anim.repeat();
    }
    rig.current = Some(clip);
}

/// The glTF backend of the `Playing` protocol: reacts to clip changes,
/// ticks the same timer/chaining semantics `sample_clips` keeps for Blocky
/// rigs, and removes finished one-shots.
pub(super) fn drive_graph_rigs(
    time: Res<Time>,
    anims: Option<Res<RigAnimations>>,
    mut commands: Commands,
    mut rigs: Query<(Entity, &mut Playing, &mut RigPlayer)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Some(anims) = anims else {
        return;
    };
    for (entity, mut playing, mut rig) in &mut rigs {
        if rig.current != Some(playing.clip) {
            start_clip(&anims, &mut players, &mut rig, playing.clip);
        }
        playing.timer.tick(time.delta());
        if playing.timer.finished() && !playing.clip.looping() {
            if let Some(next) = playing.next.take() {
                playing.clip = next;
                // Mode-aware re-arm (mirrors the `sample_clips` site above
                // and `Playing::new`): the graph backend's own visual loop
                // comes from `start_clip`'s `anim.repeat()`, but `Playing`'s
                // timer still has to keep ticking in `Repeating` mode for a
                // looping `next` — otherwise it latches `finished()` forever
                // once `TimerMode::Once` clamps at duration.
                let mode = if next.looping() {
                    TimerMode::Repeating
                } else {
                    TimerMode::Once
                };
                playing.timer = Timer::from_seconds(next.duration(), mode);
                // The mismatch with rig.current starts `next` (with blend)
                // on the next pass.
            } else {
                commands.entity(entity).remove::<Playing>();
            }
        }
    }
}

/// Rigs with nothing to play settle into the looping Idle — covers both
/// freshly wired rigs and clip removal (runs after `settle_graph_removed`,
/// which has already cleared `current` for anything dropped this frame, so
/// each removal starts Idle exactly once instead of twice).
pub(super) fn idle_graph_rigs(
    anims: Option<Res<RigAnimations>>,
    mut rigs: Query<&mut RigPlayer, Without<Playing>>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Some(anims) = anims else {
        return;
    };
    for mut rig in &mut rigs {
        if rig.current != Some(AnimClip::Idle) {
            start_clip(&anims, &mut players, &mut rig, AnimClip::Idle);
        }
    }
}

/// The glTF half of settle: when choreography removes `Playing` mid-loop
/// (e.g. `RunCycle` on arrival), forget the clip so `idle_graph_rigs` (which
/// runs right after, in the same frame) starts Idle exactly once instead of
/// this system clobbering a start it already made.
pub(super) fn settle_graph_removed(
    mut removed: RemovedComponents<Playing>,
    mut rigs: Query<&mut RigPlayer>,
) {
    for entity in removed.read() {
        if let Ok(mut rig) = rigs.get_mut(entity) {
            rig.current = None;
        }
    }
}
