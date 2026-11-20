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
use crate::game::theme::FieldTheme;

mod bowl;
mod diamond;
mod flags;
mod stadium;
mod surface;
mod textures;
mod zone;

// ── Distances in metres ───────────────────────────────────────────────────────
/// Distance between consecutive bases (90 ft), home plate → pitching rubber
/// (60.5 ft), and half the base-path diagonal (used to place second base
/// along the Z axis) are pure geometry the rules engine also needs, so
/// they're canonically defined in `core::rules` — re-exported here so every
/// existing `field::BASE_DISTANCE`/`field::PITCH_DISTANCE`-style path still
/// resolves unchanged.
pub use crate::game::rules::{BASE_DISTANCE_M as BASE_DISTANCE, HALF_DIAGONAL, PITCH_DISTANCE};

pub use bowl::{SCOREBOARD_SIZE, scoreboard_centre, stands_height_under};
pub use diamond::foul_line_spans;
pub use flags::WindFlag;
pub use stadium::WALL_HEIGHT;
pub use surface::{CLEAT_STAGES, CleatMarks, CleatWear, SurfaceDecal, cleat_stage};

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
/// directly (like `game::fx`'s landing ring).
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

/// The pair of tiled surface textures every park is dressed with, plus the
/// theme's tint over each — the one place a night game darkens the ground
/// (the textures themselves are painted once, in daylight; see
/// [`FieldTheme`]).
struct FieldSurfaces {
    grass: Handle<Image>,
    dirt: Handle<Image>,
    grass_tint: Color,
    dirt_tint: Color,
    /// Perceptual roughness of both surfaces: matte, or glossier when wet.
    roughness: f32,
    /// The turf's and clay's normal maps, on tiers that budget them
    /// (`RenderBudget::surface_detail`).
    normals: Option<(Handle<Image>, Handle<Image>)>,
    /// The turf varies in shade across the park
    /// (`RenderBudget::surface_color`).
    varied: bool,
    /// The clay is worn down the runners' lanes and round home
    /// (`RenderBudget::surface_detail`).
    worn: bool,
}

/// Dry turf and clay: fully matte.
const DRY_ROUGHNESS: f32 = 0.95;
/// Rain-soaked turf and clay on tiers that budget wet reflections: glossy
/// enough to catch the sky and the lights.
const WET_ROUGHNESS: f32 = 0.65;
/// How much darker rain-soaked ground reads, at every tier.
const WET_DARKEN: f32 = 0.78;

impl FieldSurfaces {
    /// `wet` darkens both surfaces (rain); `glossy` also lowers their
    /// roughness (rain on a tier with `RenderBudget::wet_reflections`);
    /// `detail` is the tier's surface colour and detail
    /// (`RenderBudget::surface_color`, `surface_detail`).
    fn build(
        images: &mut Assets<Image>,
        dressing: &FieldTheme,
        wet: bool,
        glossy: bool,
        detail: (bool, bool),
    ) -> Self {
        let darken = |c: Color| {
            if !wet {
                return c;
            }
            let s = c.to_srgba();
            Color::srgb(
                s.red * WET_DARKEN,
                s.green * WET_DARKEN,
                s.blue * WET_DARKEN,
            )
        };
        Self {
            grass: images.add(textures::grass_image()),
            dirt: images.add(textures::dirt_image()),
            grass_tint: darken(dressing.grass_tint),
            dirt_tint: darken(dressing.dirt_tint),
            roughness: if wet && glossy {
                WET_ROUGHNESS
            } else {
                DRY_ROUGHNESS
            },
            normals: detail.1.then(|| {
                (
                    images.add(surface::grass_normal_image()),
                    images.add(surface::dirt_normal_image()),
                )
            }),
            varied: detail.0,
            worn: detail.1,
        }
    }

    /// The clay's own colour, texture times tint: what worn ground darkens.
    fn clay(&self) -> Color {
        let [r, g, b] = textures::DIRT_RGB;
        let base = Color::srgb_u8(r, g, b).to_linear();
        let tint = self.dirt_tint.to_linear();
        Color::LinearRgba(LinearRgba::rgb(
            base.red * tint.red,
            base.green * tint.green,
            base.blue * tint.blue,
        ))
    }

    /// Adds a mesh for turf or clay, with the tangents a normal map needs
    /// on tiers that have them.
    fn mesh(&self, meshes: &mut Assets<Mesh>, mesh: impl Into<Mesh>) -> Handle<Mesh> {
        let mut mesh = mesh.into();
        if self.normals.is_some()
            && let Err(e) = mesh.generate_tangents()
        {
            warn!("surface detail: no tangents for a ground mesh ({e})");
        }
        meshes.add(mesh)
    }

    /// The grass material, tiled `repeats` times across a unit UV face.
    fn grass(
        &self,
        materials: &mut Assets<StandardMaterial>,
        repeats: f32,
    ) -> Handle<StandardMaterial> {
        Self::tiled(
            materials,
            &self.grass,
            self.normals.as_ref().map(|n| &n.0),
            self.grass_tint,
            self.roughness,
            repeats,
        )
    }

    /// The infield-dirt material, tiled `repeats` times across a unit UV face.
    fn dirt(
        &self,
        materials: &mut Assets<StandardMaterial>,
        repeats: f32,
    ) -> Handle<StandardMaterial> {
        Self::tiled(
            materials,
            &self.dirt,
            self.normals.as_ref().map(|n| &n.1),
            self.dirt_tint,
            self.roughness,
            repeats,
        )
    }

    /// A material tiling `texture` `repeats` times across a unit UV face,
    /// multiplied by `tint` (`Color::WHITE` leaves the texture as painted).
    fn tiled(
        materials: &mut Assets<StandardMaterial>,
        texture: &Handle<Image>,
        normal: Option<&Handle<Image>>,
        tint: Color,
        roughness: f32,
        repeats: f32,
    ) -> Handle<StandardMaterial> {
        materials.add(StandardMaterial {
            base_color: tint,
            base_color_texture: Some(texture.clone()),
            normal_map_texture: normal.cloned(),
            uv_transform: Affine2::from_scale(Vec2::splat(repeats)),
            perceptual_roughness: roughness,
            ..default()
        })
    }
}

// ── Plugin ────────────────────────────────────────────────────────────────────
pub struct FieldPlugin;

impl Plugin for FieldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<surface::CleatWear>()
            .add_systems(
                crate::game::game_start(),
                (
                    diamond::spawn_field,
                    bowl::spawn_bowl,
                    surface::reset_cleat_wear,
                ),
            )
            .add_systems(
                Update,
                (
                    bowl::build_crowd,
                    bowl::cheer_crowd,
                    bowl::update_scoreboard,
                    bowl::tower_lights_follow_budget,
                    (surface::count_pitches, surface::decals_follow_budget).chain(),
                    (flags::flags_follow_budget, flags::wave_flags).chain(),
                )
                    .run_if(in_state(GameState::Playing)),
            )
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
