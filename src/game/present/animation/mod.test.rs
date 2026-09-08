//! Unit tests for [`super`] — the animation module.

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
