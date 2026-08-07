//! Procedural rig animation — the single pathway through which rigs move.
//!
//! Systems never rotate rig parts directly; they insert a [`Playing`] clip on
//! a rig root (or the bat pivot) and a backend poses it every frame: glTF
//! rigs (`RigPlayer`-tagged) drive an `AnimationGraph`'s `AnimationPlayer` via
//! [`drive_graph_rigs`], while `Blocky` rigs keep the procedural [`sample_clips`]
//! sampler — the two backends never touch the same entity, so callers only
//! ever see the one [`Playing`] protocol. Likewise all locomotion goes through
//! [`MoveIntent`], so a human controller can later drive a fielder by writing
//! the same component the CPU choreography writes.

use std::time::Duration;

use bevy::prelude::*;

use crate::game::appearance::{CelebrationId, FidgetId, StanceId};
use crate::game::model_assets::{RigAnimations, RigPlayer};
use crate::game::GameState;

/// Every animation the game can play, by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimClip {
    /// Pitcher rocks back, throwing arm winds up, leg kicks.
    WindUp,
    /// Pitcher's arm whips through release.
    ThrowRelease,
    /// Looping run for any moving rig.
    RunCycle,
    /// Crouch-and-scoop for gathering a grounder.
    ScoopBall,
    /// Glove arm goes straight up to receive a ball.
    GloveUp,
    /// Bat pivot sweeps through the zone (poses the entity itself).
    SwingBat,
    /// Bat pivot returns to the shoulder (poses the entity itself).
    RecoverSwing,
    /// Catcher's receiving stance: knees bent, glove presented. Loops
    /// through the whole pitch duel.
    CatcherCrouch,
    /// Full-extension dive: body pitches forward and drops, arms out.
    Dive,
    /// Feet-first slide into a bag: body leans back and drops low.
    Slide,
    /// The batter's arms drive through the swing (the bat pivot plays
    /// [`AnimClip::SwingBat`] in parallel — this is the body half).
    BatterSwing,
    /// Held right-handed batting stance: knees softened, torso coiled
    /// toward the catcher, both arms up holding the bat off the right
    /// shoulder. Loops through the duel until the swing or the ball being
    /// put in play releases it.
    BattingStance,
    /// Neutral resting stance the glTF driver settles rigs into. Loops.
    Idle,
    /// Personality batting stance: wide open base, sunk hips, deeper crouch.
    /// Same solved arm/bat hold as `BattingStance` so the swing crossfade
    /// never pops the arms. Loops.
    StanceOpen,
    /// Personality batting stance: tall and upright, quiet legs, bat cocked
    /// closer to vertical, deeper spine coil. Loops.
    StanceClosed,
    /// Personality batting stance: `BattingStance`'s legs, a restless barrel
    /// waggle and bigger torso sway. Loops.
    StanceWaggle,
    /// Fidget: dips the bat barrel to the plate and back. Starts and ends on
    /// the `BattingStance` hold so chaining back into a stance is seamless.
    FidgetBatTap,
    /// Fidget: a partial practice unwind and back, arms riding the torso.
    /// Starts and ends on the `BattingStance` hold.
    FidgetHalfSwing,
    /// Celebration: arms sweep up and out, the bat flicks skyward, chest
    /// opens. Chained via `Playing::then` right after `BatterSwing`, so its
    /// frame 0 matches `BatterSwing`'s real end pose.
    CelebrateBatFlip,
}

impl AnimClip {
    /// Seconds one play-through lasts.
    pub fn duration(self) -> f32 {
        match self {
            AnimClip::WindUp => 0.5,
            AnimClip::ThrowRelease => 0.22,
            AnimClip::RunCycle => 0.45,
            AnimClip::ScoopBall => 0.32,
            AnimClip::GloveUp => 0.28,
            AnimClip::SwingBat => 0.16,
            AnimClip::RecoverSwing => 0.25,
            AnimClip::CatcherCrouch => 1.2,
            AnimClip::Dive => 0.5,
            AnimClip::Slide => 0.6,
            AnimClip::BatterSwing => 0.42,
            AnimClip::BattingStance => 1.2,
            AnimClip::Idle => 1.0,
            AnimClip::StanceOpen => 1.2,
            AnimClip::StanceClosed => 1.2,
            AnimClip::StanceWaggle => 1.2,
            AnimClip::FidgetBatTap => 0.8,
            AnimClip::FidgetHalfSwing => 0.9,
            AnimClip::CelebrateBatFlip => 0.85,
        }
    }

    /// Clips that repeat until the component is removed.
    pub fn looping(self) -> bool {
        matches!(
            self,
            AnimClip::RunCycle
                | AnimClip::CatcherCrouch
                | AnimClip::Idle
                | AnimClip::BattingStance
                | AnimClip::StanceOpen
                | AnimClip::StanceClosed
                | AnimClip::StanceWaggle
        )
    }
}

/// StyleSet → clip resolution. Lives here (not appearance.rs) so the schema
/// module stays serde-pure with no animation dependency.
pub fn stance_clip(id: StanceId) -> AnimClip {
    match id {
        StanceId::Standard => AnimClip::BattingStance,
        StanceId::OpenCrouch => AnimClip::StanceOpen,
        StanceId::UprightClosed => AnimClip::StanceClosed,
        StanceId::BatWaggle => AnimClip::StanceWaggle,
    }
}

pub fn fidget_clip(id: FidgetId) -> AnimClip {
    match id {
        FidgetId::BatTap => AnimClip::FidgetBatTap,
        FidgetId::HalfSwing => AnimClip::FidgetHalfSwing,
    }
}

pub fn celebration_clip(id: CelebrationId) -> Option<AnimClip> {
    match id {
        CelebrationId::Standard => None,
        CelebrationId::BatFlip => Some(AnimClip::CelebrateBatFlip),
    }
}

/// Any of the four held batting stances (shared or personal).
pub fn is_stance(clip: AnimClip) -> bool {
    matches!(
        clip,
        AnimClip::BattingStance
            | AnimClip::StanceOpen
            | AnimClip::StanceClosed
            | AnimClip::StanceWaggle
    )
}

/// Either idle fidget (helmet tap, practice half-swing) — the clips
/// `player::batter_stance`'s continuation-cut arm replaces the instant the
/// duel leaves `Phase::PrePitch`, so a fidget never survives into the
/// windup or blocks a swing press (`player::trigger_swing`'s stance-only
/// gate).
pub fn is_fidget(clip: AnimClip) -> bool {
    matches!(clip, AnimClip::FidgetBatTap | AnimClip::FidgetHalfSwing)
}

/// Insert to suppress idle fidgets outright — the scripted e2e harness
/// does (a fidget replaces the batter's Playing state mid-script, which
/// perturbs timing-sensitive drivers even though swings can interrupt it).
/// The `JuiceDisabled` pattern.
#[derive(Resource)]
pub struct FidgetsDisabled;

/// What a rig is currently playing. Insert to start, remove to stop; the
/// sampler chains to `next` when a one-shot clip finishes.
#[derive(Component)]
pub struct Playing {
    pub clip: AnimClip,
    pub timer: Timer,
    pub next: Option<AnimClip>,
}

impl Playing {
    pub fn new(clip: AnimClip) -> Self {
        let mode = if clip.looping() {
            TimerMode::Repeating
        } else {
            TimerMode::Once
        };
        Self {
            clip,
            timer: Timer::from_seconds(clip.duration(), mode),
            next: None,
        }
    }

    pub fn then(clip: AnimClip, next: AnimClip) -> Self {
        Self {
            next: Some(next),
            ..Self::new(clip)
        }
    }
}

/// Which limb a joint entity poses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimbKind {
    ArmL,
    ArmR,
    LegL,
    LegR,
}

/// Marks a poseable joint (direct child of a rig root). The joint sits at the
/// shoulder/hip and the limb mesh hangs below it, so rotating the joint
/// swings the limb.
#[derive(Component)]
pub struct RigLimb {
    pub kind: LimbKind,
}

/// Movement order for a rig: written by choreography (or, later, a human
/// controller), consumed by [`locomote`]. Cleared on arrival.
#[derive(Component, Default)]
pub struct MoveIntent {
    pub target: Option<Vec3>,
    /// Metres per second.
    pub speed: f32,
}

/// A rig root's resting height, captured at spawn — the reference the
/// sampler's root-drop channel offsets from (crouches and scoops actually
/// lower the body) and [`settle_removed`] restores.
#[derive(Component)]
pub struct RigBaseY(pub f32);

// ── Poses ─────────────────────────────────────────────────────────────────────

fn ease_out(f: f32) -> f32 {
    1.0 - (1.0 - f).powi(3)
}

/// Bat resting over the shoulder (the pivot's spawn rotation).
pub fn bat_idle_rotation() -> Quat {
    Quat::from_euler(EulerRot::ZXY, -0.5, 0.35, 0.0)
}

/// Bat laid horizontal, swept `angle` around Y.
fn bat_sweep_rotation(angle: f32) -> Quat {
    Quat::from_rotation_y(angle) * Quat::from_rotation_z(1.45)
}

/// Horizontal sweep range (radians about Y): cocked toward the catcher,
/// through the plate, to a follow-through toward the pitcher.
const SWEEP_FROM: f32 = -1.7;
const SWEEP_TO: f32 = 1.7;

/// Rotation for clips that pose the playing entity itself (the bat pivot).
fn self_pose(clip: AnimClip, f: f32) -> Option<Quat> {
    match clip {
        AnimClip::SwingBat => Some(bat_sweep_rotation(
            SWEEP_FROM + (SWEEP_TO - SWEEP_FROM) * ease_out(f),
        )),
        AnimClip::RecoverSwing => Some(bat_sweep_rotation(SWEEP_TO).slerp(bat_idle_rotation(), f)),
        _ => None,
    }
}

/// Joint rotation for a limb-posing clip at progress `f` (0..=1). Rotations
/// are in rig-local space; the rig root's yaw supplies world facing.
fn limb_pose(clip: AnimClip, kind: LimbKind, f: f32) -> Quat {
    use AnimClip::*;
    use LimbKind::*;
    match clip {
        WindUp => {
            let lift = ease_out(f);
            match kind {
                ArmR => Quat::from_rotation_x(-2.6 * lift),
                ArmL => Quat::from_rotation_x(-1.2 * lift),
                LegL => Quat::from_rotation_x(1.0 * lift),
                LegR => Quat::IDENTITY,
            }
        }
        ThrowRelease => {
            let s = ease_out(f);
            match kind {
                ArmR => Quat::from_rotation_x(-2.6 + 3.4 * s),
                ArmL => Quat::from_rotation_x(-1.2 + 1.2 * s),
                LegL => Quat::from_rotation_x(1.0 - 1.0 * s),
                LegR => Quat::IDENTITY,
            }
        }
        RunCycle => {
            let swing = (f * std::f32::consts::TAU).sin() * 0.9;
            match kind {
                ArmL | LegR => Quat::from_rotation_x(swing),
                ArmR | LegL => Quat::from_rotation_x(-swing),
            }
        }
        ScoopBall => {
            let dip = (f * std::f32::consts::PI).sin();
            match kind {
                ArmL | ArmR => Quat::from_rotation_x(1.6 * dip),
                LegL | LegR => Quat::from_rotation_x(0.4 * dip),
            }
        }
        GloveUp => {
            let lift = ease_out(f);
            match kind {
                ArmL => Quat::from_rotation_x(-2.9 * lift),
                _ => Quat::IDENTITY,
            }
        }
        CatcherCrouch => {
            // A held stance with a slow breath: legs folded, glove arm out.
            let sway = (f * std::f32::consts::TAU).sin() * 0.04;
            match kind {
                LegL | LegR => Quat::from_rotation_x(1.35),
                ArmL => Quat::from_rotation_x(-1.15 + sway),
                ArmR => Quat::from_rotation_x(-0.55 - sway),
            }
        }
        Dive => {
            // Arms reach out ahead, legs trail behind the layout.
            let s = ease_out(f);
            match kind {
                ArmL | ArmR => Quat::from_rotation_x(-2.6 * s),
                LegL | LegR => Quat::from_rotation_x(0.5 * s),
            }
        }
        Slide => {
            // Legs kick out in front, arms thrown up for balance.
            let s = ease_out(f);
            match kind {
                LegL | LegR => Quat::from_rotation_x(-1.2 * s),
                ArmL | ArmR => Quat::from_rotation_x(-0.7 * s),
            }
        }
        BatterSwing => {
            // Both arms whip horizontally through the zone and settle back —
            // one out-and-return arc matching the bat pivot's sweep+recover.
            let sweep = (f * std::f32::consts::PI).sin() * 1.9;
            match kind {
                ArmL | ArmR => Quat::from_rotation_y(sweep) * Quat::from_rotation_x(-0.5),
                LegL | LegR => Quat::IDENTITY,
            }
        }
        // Blocky fallback: the two-handed grip is a glTF-only bone reposition
        // (tools/build_player.py), so the procedural rig just holds Idle's
        // neutral limb pose rather than approximating the stance. The three
        // personality stances delegate to BattingStance's branch and the two
        // fidgets plus the celebration delegate to Idle's — all four already
        // resolve to the same neutral identity pose here.
        SwingBat | RecoverSwing | Idle | BattingStance | StanceOpen | StanceClosed
        | StanceWaggle | FidgetBatTap | FidgetHalfSwing | CelebrateBatFlip => Quat::IDENTITY,
    }
}

/// How far a clip sinks the whole rig root below its resting height at
/// progress `f` — the vertical body channel limb rotations can't fake.
fn root_drop(clip: AnimClip, f: f32) -> f32 {
    match clip {
        AnimClip::CatcherCrouch => 0.22,
        AnimClip::ScoopBall => 0.26 * (f * std::f32::consts::PI).sin(),
        AnimClip::Dive => 0.38 * ease_out(f),
        AnimClip::Slide => 0.30 * ease_out(f),
        _ => 0.0,
    }
}

/// How far a clip pitches the whole rig root about its local X axis at
/// progress `f` (radians; positive = face-first forward) — the body-lean
/// channel dives and slides need. Composed on top of the rig's travel yaw.
fn root_pitch(clip: AnimClip, f: f32) -> f32 {
    match clip {
        AnimClip::Dive => 1.25 * ease_out(f),
        AnimClip::Slide => -0.85 * ease_out(f),
        _ => 0.0,
    }
}

// ── Systems ───────────────────────────────────────────────────────────────────

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

fn sample_clips(
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
                playing.timer = Timer::from_seconds(next.duration(), TimerMode::Once);
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
fn settle_removed(
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
/// animation.rs owns rig root height. Runs last so it wins the frame over the
/// settle systems that restore the batter root to `RigBaseY`.
const METER_SINK_M: f32 = 0.12;
fn meter_stance_sink(
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
fn locomote(
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
fn drive_graph_rigs(
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
                playing.timer = Timer::from_seconds(next.duration(), TimerMode::Once);
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
fn idle_graph_rigs(
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
fn settle_graph_removed(mut removed: RemovedComponents<Playing>, mut rigs: Query<&mut RigPlayer>) {
    for entity in removed.read() {
        if let Ok(mut rig) = rigs.get_mut(entity) {
            rig.current = None;
        }
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                locomote,
                drive_graph_rigs,
                settle_graph_removed,
                idle_graph_rigs,
                sample_clips,
                settle_removed,
                // Composes the meter's stance-sink over `RigBaseY` last, so it
                // wins the batter root's y after `settle_removed`/`sample_clips`
                // have restored it.
                meter_stance_sink,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `StanceId` resolves to a clip `is_stance` accepts — the personal
    /// stances must stay indistinguishable from `BattingStance` to every
    /// system that gates on "is the batter holding a stance".
    #[test]
    fn every_stance_id_resolves_to_a_stance_clip() {
        for id in [
            StanceId::Standard,
            StanceId::OpenCrouch,
            StanceId::UprightClosed,
            StanceId::BatWaggle,
        ] {
            assert!(
                is_stance(stance_clip(id)),
                "{id:?} resolved to a clip is_stance rejects"
            );
        }
    }

    #[test]
    fn stance_clip_is_personal_not_shared() {
        assert_eq!(stance_clip(StanceId::Standard), AnimClip::BattingStance);
        assert_eq!(stance_clip(StanceId::OpenCrouch), AnimClip::StanceOpen);
        assert_eq!(stance_clip(StanceId::UprightClosed), AnimClip::StanceClosed);
        assert_eq!(stance_clip(StanceId::BatWaggle), AnimClip::StanceWaggle);
    }

    #[test]
    fn fidget_ids_resolve_to_their_clips() {
        assert_eq!(fidget_clip(FidgetId::BatTap), AnimClip::FidgetBatTap);
        assert_eq!(fidget_clip(FidgetId::HalfSwing), AnimClip::FidgetHalfSwing);
    }

    #[test]
    fn celebration_standard_is_none_bat_flip_is_some() {
        assert_eq!(celebration_clip(CelebrationId::Standard), None);
        assert_eq!(
            celebration_clip(CelebrationId::BatFlip),
            Some(AnimClip::CelebrateBatFlip)
        );
    }

    #[test]
    fn is_stance_rejects_non_stance_clips() {
        assert!(!is_stance(AnimClip::Idle));
        assert!(!is_stance(AnimClip::BatterSwing));
        assert!(!is_stance(AnimClip::FidgetBatTap));
        assert!(!is_stance(AnimClip::CelebrateBatFlip));
    }
}
