//! Playing-field geometry, spawned from the chosen [`FieldSpec`].
//!
//! Shared pieces (ground, bases, mound, lighting) are placed wherever the spec
//! says; the surroundings are dressed by the spec's [`Scenery`] routine —
//! a classic ballpark or a suburban front yard.
//!
//! **Standard field dimensions** (metric, matching real MLB proportions scaled
//! to Bevy world units where 1 unit ≈ 1 metre):
//!
//! | Feature                     | Real feet | Metres (≈) |
//! |-----------------------------|-----------|------------|
//! | Base-to-base                | 90 ft     | 27.43 m    |
//! | Home plate → pitcher's mound| 60.5 ft   | 18.44 m    |
//! | Home plate → centre-field   | 400 ft    | 121.9 m    |
//! | Foul lines (1B / 3B)        | 330 ft    | 100.6 m    |

use bevy::math::Affine2;
use bevy::prelude::*;

use crate::game::GameState;

mod diamond;
mod stadium;
mod textures;
mod zone;

// ── Distances in metres ───────────────────────────────────────────────────────
/// Distance between consecutive bases (90 ft).
pub const BASE_DISTANCE: f32 = 27.43;
/// Home plate → pitching rubber (60.5 ft).
pub const PITCH_DISTANCE: f32 = 18.44;
/// Half the base-path diagonal, used to place second base along the Z axis.
pub const HALF_DIAGONAL: f32 = BASE_DISTANCE * std::f32::consts::SQRT_2 / 2.0;

pub use stadium::WALL_HEIGHT;

// ── Field-object marker components ───────────────────────────────────────────
/// Marks the entire playing-surface ground plane.
#[derive(Component)]
pub struct GroundPlane;

/// Marks a base object: `Some(i)` is the (0-indexed) i-th base in running
/// order, `None` is home plate.
#[allow(dead_code)]
#[derive(Component)]
pub struct Base {
    pub index: Option<usize>,
}

/// Marks the pitcher's mound.
#[derive(Component)]
pub struct PitchersMound;

/// Marks one of the four foul-line poles.
#[derive(Component)]
pub struct FoulPole;

/// Marks an outfield-wall panel — collision partners for wall-carom effects.
#[derive(Component)]
pub struct OutfieldWall;

/// Marks a piece of the floating strike-zone box shown during the duel.
/// Public so e2e tests can query its `Visibility` directly (the same pattern
/// `player::CatcherRole` uses for `e2e_camera_views`).
#[derive(Component)]
pub struct StrikeZoneOverlay;

/// Marks the PCI aiming cursor — the small quad a PCI batter steers around the
/// zone plane. Public so e2e tests can query its `Visibility`, like
/// [`StrikeZoneOverlay`]. Shown only while a human PCI batter is up (see
/// [`zone::pci_cursor_visibility`]); a non-rig marker, so its transform is moved
/// directly (like `fx.rs`'s landing ring).
#[derive(Component)]
pub struct PciCursorMarker;

/// The zone box's frame material plus enough to pulse and restore it: a
/// contact worth celebrating (Solid/Perfect, Task B4) briefly tints the
/// frame bars, then this ticks the pulse back to `base_color`. Rebuilt
/// fresh in [`zone::spawn_strike_zone`] every game start alongside the material
/// it points at.
#[derive(Resource)]
struct ZoneFlash {
    material: Handle<StandardMaterial>,
    base_color: Color,
    flash_color: Color,
    timer: Option<Timer>,
}

/// The pair of tiled surface materials every park is dressed with.
struct FieldSurfaces {
    grass: Handle<Image>,
    dirt: Handle<Image>,
}

impl FieldSurfaces {
    fn build(images: &mut Assets<Image>) -> Self {
        Self {
            grass: images.add(textures::grass_image()),
            dirt: images.add(textures::dirt_image()),
        }
    }

    /// A material tiling `texture` `repeats` times across a unit UV face.
    fn tiled(
        materials: &mut Assets<StandardMaterial>,
        texture: &Handle<Image>,
        repeats: f32,
    ) -> Handle<StandardMaterial> {
        materials.add(StandardMaterial {
            base_color_texture: Some(texture.clone()),
            uv_transform: Affine2::from_scale(Vec2::splat(repeats)),
            perceptual_roughness: 0.95,
            ..default()
        })
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────
pub struct FieldPlugin;

impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(crate::game::game_start(), diamond::spawn_field)
            .add_systems(
                Update,
                // trigger/restore before visibility: a same-frame contact
                // must land in `ZoneFlash` before `strike_zone_visibility`
                // reads it, or the box hides for the one frame the read
                // beat the write — see that system's doc comment.
                (
                    zone::trigger_zone_flash,
                    zone::restore_zone_flash,
                    zone::strike_zone_visibility,
                    zone::pci_cursor_visibility,
                )
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}
