//! Procedural rig animation — the single pathway through which rigs move.
//!
//! Systems never rotate rig parts directly; they insert a [`Playing`] clip on
//! a rig root (or the bat pivot) and [`sample_clips`] poses it every frame.
//! This boundary is deliberate: a future `AnimationGraph` backend replaces the
//! sampler without touching any caller. Likewise all locomotion goes through
//! [`MoveIntent`], so a human controller can later drive a fielder by writing
//! the same component the CPU choreography writes.

use bevy::prelude::*;

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
        }
    }

    /// Clips that repeat until the component is removed.
    pub fn looping(self) -> bool {
        matches!(self, AnimClip::RunCycle | AnimClip::CatcherCrouch)
    }
}

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
        SwingBat | RecoverSwing => Quat::IDENTITY,
    }
}

/// How far a clip sinks the whole rig root below its resting height at
/// progress `f` — the vertical body channel limb rotations can't fake.
fn root_drop(clip: AnimClip, f: f32) -> f32 {
    match clip {
        AnimClip::CatcherCrouch => 0.22,
        AnimClip::ScoopBall => 0.26 * (f * std::f32::consts::PI).sin(),
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

/// Returns limbs to neutral and the root to its resting height whenever a
/// clip stops (covers both the sampler's own removal and choreography
/// removing `RunCycle` mid-loop).
fn settle_removed(
    mut removed: RemovedComponents<Playing>,
    children_q: Query<&Children>,
    mut limb_q: Query<(&RigLimb, &mut Transform)>,
    mut root_q: Query<(&mut Transform, &RigBaseY), Without<RigLimb>>,
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
        }
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

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (locomote, sample_clips, settle_removed)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}
