//! Player entities — pitcher, batter, and fielders.
//!
//! Each player is a small rig (torso + head + cap + brim, the batter also a
//! bat) hanging off one kinematic Rapier body. Looks come from the active
//! [`Theme`]'s per-team [`crate::game::theme::PlayerTemplate`]s: a
//! [`TeamPalette`] of shared material handles is built at spawn, and
//! [`recolor_teams`] reassigns handles whenever the half-inning flips so the
//! defense always wears the fielding team's colours and the batter the
//! batting team's.
//!
//! Split into [`rig`] (team palette, rig construction/spawning, per-frame
//! recolouring, identity sync) and [`behavior`] (duel-phase animation
//! systems: stance, fidgets, catcher crouch, swing trigger, home-run
//! celebration) — this module keeps the shared markers/rig-unit types every
//! rig carries plus the plugin wiring.

use bevy::prelude::*;

use crate::game::GameState;

mod behavior;
mod rig;

// Genuinely used in this module tree (spawn_rig by `rig`'s own spawn_players
// as well as externally by `sim::runner`; sync_identities by `jersey.rs`;
// TeamPalette by `sim::runner`) — no allow needed.
pub(crate) use rig::{TeamPalette, spawn_rig, sync_identities};
// build_materials/build_rig_model's only external caller is the Creator's
// preview rig (`meta::creator::preview`, `debug` feature only), so a
// non-debug build sees this re-export as unused; RigMaterials has no
// external caller by name at all (only reached via `TeamPalette::for_team`'s
// return type inference). Kept anyway so `game::player::<item>` stays
// nameable exactly as it was pre-split — dropping these would silently
// break that path for the next caller, the failure mode this split must
// avoid.
#[allow(unused_imports)]
pub(crate) use rig::RigMaterials;
pub use rig::{BATTER_STAND_X, RIG_HEIGHT_M, RigMeshes, RigModel};
#[cfg_attr(not(feature = "debug"), allow(unused_imports))]
pub(crate) use rig::{build_materials, build_rig_model};

use behavior::{
    BatterFidgetTimer, batter_fidgets, batter_stance, catcher_crouch, celebrate_home_run,
    reset_batter_fidget_timer, trigger_swing,
};
use rig::{recolor_gltf, recolor_teams, spawn_players};

// ── Roles & rig parts ─────────────────────────────────────────────────────────

/// Marker for the player controlling the pitcher.
#[derive(Component)]
pub struct Pitcher;

/// Marker for the player controlling the batter.
#[derive(Component)]
pub struct Batter;

/// Marker for a defensive fielder: the i-th spot in the field spec's
/// `fielder_positions`.
#[derive(Component)]
pub struct Fielder {
    pub index: usize,
}

/// The fielder stationed behind the plate (spawn spot at z < 0): crouches
/// through the pitch duel. Parks without one (the front yard) have none.
#[derive(Component)]
pub struct CatcherRole;

/// The umpire behind the plate, who crouches through the duel like the
/// catcher he's peering over.
#[derive(Component)]
pub struct PlateUmpire;

/// Facing direction for the player model (world-space).
#[allow(dead_code)]
#[derive(Component, Default)]
pub struct FacingDirection(pub Vec3);

/// Whether a rig belongs to the defense (pitcher + fielders) or the batting
/// side (batter, runners) — decides which team's colours it wears as innings
/// flip. Umpires wear their own blacks and never recolour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RigUnit {
    Defense,
    Batter,
    Umpire,
}

/// A rig root's team unit, readable by wiring/recolour systems.
#[derive(Component, Clone, Copy)]
pub struct RigUnitTag(pub RigUnit);

/// Marks a glTF rig root awaiting (or holding) its wired skeleton.
#[derive(Component)]
pub struct GltfRig;

// ── Bat swing ─────────────────────────────────────────────────────────────────

/// Marks the batter's bat pivot — the entity the swing clips rotate. Shared
/// across [`rig`] (spawns it) and [`behavior`] (queries it for the swing).
#[derive(Component)]
pub(crate) struct BatPivot;

// ── Identity sync ordering ───────────────────────────────────────────────────

/// Systems that stamp [`crate::game::roster::PlayerIdentity`] run in this
/// set; identity *consumers* order `.after(IdentitySet)` and get Bevy's
/// auto-inserted sync point, seeing the same-frame stamps. Promoted from a
/// bare `.chain()` per the Phase 1 review so new consumers join
/// declaratively.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdentitySet;

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BatterFidgetTimer>()
            .add_systems(
                crate::game::game_start(),
                (spawn_players, reset_batter_fidget_timer),
            )
            .add_systems(
                Update,
                (
                    recolor_teams,
                    recolor_gltf,
                    // Reads `PlayerIdentity` to resolve the batter's personal
                    // stance — must see the same-frame stamp on the exact
                    // frame a new batter steps up (the `dress_jerseys`
                    // pattern in jersey.rs).
                    batter_stance.after(IdentitySet),
                    // Also reads `PlayerIdentity` for the fidget lookup;
                    // chained after `batter_stance` so it always sees this
                    // frame's freshly (re)resolved stance/cut before
                    // deciding whether to break it.
                    batter_fidgets.after(IdentitySet),
                    trigger_swing,
                    // Reads `PlayerIdentity` to resolve the batter's
                    // authored celebration; ordered after `trigger_swing` so
                    // a same-frame swing press has already landed
                    // `BatterSwing` before it decides whether to chain the
                    // flip.
                    celebrate_home_run.after(IdentitySet),
                    catcher_crouch,
                )
                    .chain()
                    // Same-frame phase-flip guarantee (batter_stance's
                    // fidget-cut vs. a swing pressed right at the windup):
                    // `flow::PhaseSet` mutates `Play::phase` earlier this
                    // frame, unpinned otherwise against the multi-threaded
                    // executor's worker-timing tie-break.
                    .after(crate::game::flow::PhaseSet)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
