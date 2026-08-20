//! Pure clip → pose math: rotations and root offsets for a given
//! [`AnimClip`] at progress `f` (0..=1). No ECS here — [`driver`](super::driver)
//! samples these every frame and writes the results onto rig transforms.

use bevy::prelude::*;

use super::{AnimClip, LimbKind};

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
pub(super) fn self_pose(clip: AnimClip, f: f32) -> Option<Quat> {
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
pub(super) fn limb_pose(clip: AnimClip, kind: LimbKind, f: f32) -> Quat {
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
pub(super) fn root_drop(clip: AnimClip, f: f32) -> f32 {
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
pub(super) fn root_pitch(clip: AnimClip, f: f32) -> f32 {
    match clip {
        AnimClip::Dive => 1.25 * ease_out(f),
        AnimClip::Slide => -0.85 * ease_out(f),
        _ => 0.0,
    }
}
