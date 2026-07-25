//! The glTF player model: contract constants shared by the runtime loader
//! and the model-contract test. The loader/wiring systems land here too
//! (later tasks) — this module owns everything about the embedded `.glb`.

use crate::game::animation::AnimClip;

/// Repo-relative path of the committed model (used by the contract test and
/// the export script's output).
pub const PLAYER_GLB: &str = "src/game/models/player.glb";

/// Named material the recolour system re-tints per team.
pub const JERSEY_MATERIAL: &str = "JerseyBody";
/// Named cap material, also team-tinted.
pub const CAP_MATERIAL: &str = "Cap";

/// Bones gameplay attaches to (jersey lettering, the bat, future props).
pub const ATTACH_BONES: &[&str] = &["Hips", "Spine", "Head", "UpperArm.L", "UpperArm.R", "Bat"];

/// Budgets per docs/superpowers/specs/2026-07-24-gltf-player-models-design.md
/// §7 — ~18 skinned rigs at once on a WebGL2 floor.
pub const MAX_BONES: usize = 48;
pub const MAX_TRIANGLES: usize = 5_000;
pub const MAX_GLB_BYTES: usize = 512 * 1024;

/// AnimClip → baked clip name: the single source of truth for the runtime
/// graph AND the contract test, so the Rust enum and the Blender file can
/// only drift in ways that fail CI loudly. `SwingBat`/`RecoverSwing` are
/// absent by design — they alias `BatterSwing` via [`node_for`] (the bat is
/// a bone, so one clip covers body and bat).
pub const CLIP_TABLE: &[(AnimClip, &str)] = &[
    (AnimClip::Idle, "Idle"),
    (AnimClip::WindUp, "WindUp"),
    (AnimClip::ThrowRelease, "ThrowRelease"),
    (AnimClip::RunCycle, "RunCycle"),
    (AnimClip::ScoopBall, "ScoopBall"),
    (AnimClip::GloveUp, "GloveUp"),
    (AnimClip::CatcherCrouch, "CatcherCrouch"),
    (AnimClip::Dive, "Dive"),
    (AnimClip::Slide, "Slide"),
    (AnimClip::BatterSwing, "BatterSwing"),
];

/// Clips without their own baked action fold onto the one that covers them.
pub fn node_for(clip: AnimClip) -> AnimClip {
    match clip {
        AnimClip::SwingBat | AnimClip::RecoverSwing => AnimClip::BatterSwing,
        c => c,
    }
}
