//! Procedural rig animation — the single pathway through which rigs move.
//!
//! Systems never rotate rig parts directly; they insert a [`Playing`] clip on
//! a rig root (or the bat pivot) and a backend poses it every frame: glTF
//! rigs (`RigPlayer`-tagged) drive an `AnimationGraph`'s `AnimationPlayer` via
//! `drive_graph_rigs`, while `Blocky` rigs keep the procedural `sample_clips`
//! sampler — the two backends never touch the same entity, so callers only
//! ever see the one [`Playing`] protocol. Likewise all locomotion goes through
//! [`MoveIntent`], so a human controller can later drive a fielder by writing
//! the same component the CPU choreography writes.
//!
//! Split into [`poses`] (pure clip → rotation/offset math) and [`driver`]
//! (the two `Playing` backends, locomotion, and the Swing Meter's stance
//! sink) — this module keeps the clip catalogue, the shared marker/component
//! types every rig carries, and the plugin wiring.

use bevy::prelude::*;

use crate::game::GameState;
use crate::game::appearance::{CelebrationId, FidgetId, StanceId};

mod driver;
mod poses;

pub use poses::bat_idle_rotation;

use driver::{
    drive_graph_rigs, idle_graph_rigs, locomote, meter_stance_sink, sample_clips,
    settle_graph_removed, settle_removed,
};

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
/// controller), consumed by [`locomote`](driver::locomote). Cleared on
/// arrival.
#[derive(Component, Default)]
pub struct MoveIntent {
    pub target: Option<Vec3>,
    /// Metres per second.
    pub speed: f32,
}

/// A rig root's resting height, captured at spawn — the reference the
/// sampler's root-drop channel offsets from (crouches and scoops actually
/// lower the body) and `settle_removed` restores.
#[derive(Component)]
pub struct RigBaseY(pub f32);

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                // `locomote` only ever drives real gameplay movers (fielders,
                // runners) and `meter_stance_sink` only the Swing Meter's
                // batting-adapter feedback — both stay `Playing`-only. The
                // five clip-driver systems between them widen to
                // `dressing_active` so the Creator's preview rig (`--features
                // debug`) is posed by the exact same drivers gameplay uses;
                // `.chain()` still orders every system here regardless of
                // which run condition gates it.
                locomote.run_if(in_state(GameState::Playing)),
                drive_graph_rigs.run_if(crate::game::dressing_active),
                settle_graph_removed.run_if(crate::game::dressing_active),
                idle_graph_rigs.run_if(crate::game::dressing_active),
                sample_clips.run_if(crate::game::dressing_active),
                settle_removed.run_if(crate::game::dressing_active),
                // Composes the meter's stance-sink over `RigBaseY` last, so it
                // wins the batter root's y after `settle_removed`/`sample_clips`
                // have restored it.
                meter_stance_sink.run_if(in_state(GameState::Playing)),
            )
                .chain(),
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

    /// Every `FidgetId` resolves to a clip `is_fidget` accepts — the mirror
    /// of `every_stance_id_resolves_to_a_stance_clip`.
    #[test]
    fn every_fidget_id_resolves_to_a_fidget_clip() {
        for id in [FidgetId::BatTap, FidgetId::HalfSwing] {
            assert!(
                is_fidget(fidget_clip(id)),
                "{id:?} resolved to a clip is_fidget rejects"
            );
        }
    }

    #[test]
    fn is_fidget_rejects_non_fidget_clips() {
        assert!(!is_fidget(AnimClip::Idle));
        assert!(!is_fidget(AnimClip::BatterSwing));
        assert!(!is_fidget(AnimClip::BattingStance));
        assert!(!is_fidget(AnimClip::CelebrateBatFlip));
    }

    /// Pins the personality clips' loop mode against `looping()`'s
    /// exhaustive `matches!` (no wildcard arm): the three held stances loop
    /// through the duel, while the two fidgets and the bat-flip celebration
    /// are one-shots that must finish and hand off via `Playing::next`
    /// instead of repeating forever. A future clip added to the enum without
    /// updating this match fails to compile, but nothing catches a clip
    /// landing in the *wrong* arm — this test does.
    #[test]
    fn personality_clips_have_the_right_loop_mode() {
        for clip in [
            AnimClip::StanceOpen,
            AnimClip::StanceClosed,
            AnimClip::StanceWaggle,
        ] {
            assert!(clip.looping(), "{clip:?} must loop");
        }
        for clip in [
            AnimClip::FidgetBatTap,
            AnimClip::FidgetHalfSwing,
            AnimClip::CelebrateBatFlip,
        ] {
            assert!(!clip.looping(), "{clip:?} must not loop");
        }
    }
}
