//! Ground surface detail (graphics plan task 16), by tier
//! ([`RenderBudget::surface_color`], [`RenderBudget::surface_normals`]):
//!
//! - Medium and High: turf and clay that vary in shade across the park (a
//!   vertex-coloured ground instead of one repeating tile), base paths worn
//!   darker where the runners go, a dirt halo round the home-plate circle,
//!   and cleat marks in the batter's boxes that build as the game goes on.
//! - High also: procedural normal maps on the turf and the clay.
//! - Low: none of it; the park is exactly as before.
//!
//! Like the rest of the park, the ground mesh and the normal maps are picked
//! once at game start (a mid-game step-down keeps them). The decals are flat
//! alpha-blended quads that follow the tier mid-game by `Visibility`, as the
//! towers' fill spots do. Every texture is generated at runtime from hashes,
//! no asset files, the same as `textures.rs`.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::game::GameplayEntity;
use crate::game::ai::hash01;
use crate::game::ball::PitchEvent;
use crate::game::quality::ActiveQuality;

// ── Pure noise ────────────────────────────────────────────────────────────────

/// Smooth value noise on a lattice of `period` cells that wraps every
/// `period` cells (so a texture built from it tiles), at lattice coordinates
/// `(x, y)`. In `0..=1`.
pub(super) fn tiling_noise(x: f32, y: f32, period: u32, seed: f32) -> f32 {
    let p = period as i64;
    let corner = |ix: i64, iy: i64| {
        let (wx, wy) = (ix.rem_euclid(p), iy.rem_euclid(p));
        hash01(wx as f32 * 127.1 + wy as f32 * 311.7 + seed * 74.7)
    };
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i64, fy as i64);
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty) = (smooth(x - fx), smooth(y - fy));
    let top = corner(ix, iy) + (corner(ix + 1, iy) - corner(ix, iy)) * tx;
    let bottom = corner(ix, iy + 1) + (corner(ix + 1, iy + 1) - corner(ix, iy + 1)) * tx;
    top + (bottom - top) * ty
}

/// Two octaves of [`tiling_noise`] over a `size`-pixel square tile: broad
/// cells of `cell` pixels plus half-size detail. In `0..=1`.
fn tile_height(x: usize, y: usize, size: usize, cell: usize, seed: f32) -> f32 {
    let period = (size / cell) as u32;
    let (u, v) = (x as f32 / cell as f32, y as f32 / cell as f32);
    let broad = tiling_noise(u, v, period, seed);
    let fine = tiling_noise(u * 2.0, v * 2.0, period * 2, seed + 9.1);
    broad * 0.65 + fine * 0.35
}

// ── Normal maps (High) ────────────────────────────────────────────────────────

/// Texture size of the normal maps: the same tile as the colour textures.
const NORMAL_SIZE: usize = 64;

/// A tangent-space normal map (linear RGBA8, +Y up) for a tiling height
/// field `height` over a `NORMAL_SIZE` square, `strength` steep.
fn normal_image(height: impl Fn(usize, usize) -> f32, strength: f32) -> Image {
    let n = NORMAL_SIZE;
    let h = |x: i64, y: i64| {
        height(
            x.rem_euclid(n as i64) as usize,
            y.rem_euclid(n as i64) as usize,
        )
    };
    let mut data = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (xi, yi) = (x as i64, y as i64);
            let dx = (h(xi + 1, yi) - h(xi - 1, yi)) * 0.5;
            let dy = (h(xi, yi + 1) - h(xi, yi - 1)) * 0.5;
            let normal = Vec3::new(-dx * strength, -dy * strength, 1.0).normalize();
            let at = (y * n + x) * 4;
            for (i, c) in normal.to_array().into_iter().enumerate() {
                data[at + i] = ((c * 0.5 + 0.5) * 255.0).round() as u8;
            }
            data[at + 3] = 255;
        }
    }
    repeat_image(n as u32, data, TextureFormat::Rgba8Unorm)
}

/// Turf: soft tufts.
pub(super) fn grass_normal_image() -> Image {
    normal_image(|x, y| tile_height(x, y, NORMAL_SIZE, 8, 3.3), 6.0)
}

/// Clay: a raked, pebbly surface.
pub(super) fn dirt_normal_image() -> Image {
    normal_image(
        |x, y| {
            let pebble = hash01(x as f32 * 31.7 + y as f32 * 57.3 * 1.7) > 0.93;
            tile_height(x, y, NORMAL_SIZE, 4, 5.9) + if pebble { 0.35 } else { 0.0 }
        },
        4.0,
    )
}

// ── Ground variation (Medium and High) ────────────────────────────────────────

/// How far the turf's shade wanders across the park (fraction of its
/// colour, either way), and over what distance (m).
const SHADE_SWING: f32 = 0.09;
const SHADE_CELL_M: f32 = 14.0;

/// The ground's vertex shade at `(x, z)` (m): a multiplier near 1, the turf
/// a touch warmer where it's lighter (sun-faded) and cooler where darker.
pub(super) fn ground_shade(x: f32, z: f32) -> [f32; 4] {
    // A period far beyond the park, so nothing repeats on screen.
    let (u, v) = (x / SHADE_CELL_M + 400.0, z / SHADE_CELL_M + 400.0);
    let broad = tiling_noise(u, v, 1024, 1.3);
    let fine = tiling_noise(u * 2.7, v * 2.7, 1024, 7.7);
    let s = ((broad * 0.7 + fine * 0.3) * 2.0 - 1.0) * SHADE_SWING;
    [1.0 + s * 1.1, 1.0 + s, 1.0 + s * 0.6, 1.0]
}

/// The ground slab's visible top: a `half`-metre square at y = 0, gridded
/// every few metres and vertex-coloured by [`ground_shade`].
pub(super) fn varied_ground_mesh(half: f32) -> Mesh {
    let mut mesh = Plane3d::default()
        .mesh()
        .size(half * 2.0, half * 2.0)
        .subdivisions(99)
        .build();
    let colors: Vec<[f32; 4]> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(positions)) => {
            positions.iter().map(|p| ground_shade(p[0], p[2])).collect()
        }
        _ => Vec::new(),
    };
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

// ── Decals (Medium and High) ──────────────────────────────────────────────────

/// Marks a ground decal: worn paths, the halo at home, cleat marks.
#[derive(Component)]
pub struct SurfaceDecal;

/// A batter's box's cleat marks (`stages` are the wear textures, light to
/// heavy).
#[derive(Component)]
pub struct CleatMarks {
    stages: Vec<Handle<Image>>,
}

/// Pitches thrown this game: the cleat marks in the boxes deepen with it.
#[derive(Resource, Default, Debug)]
pub struct CleatWear {
    pub pitches: u32,
}

/// Pitch counts at which the boxes show their first marks and each heavier
/// stage after it: a full nine innings is ~290 pitches.
pub const CLEAT_STAGES: [u32; 4] = [1, 40, 120, 220];

/// Which wear stage the boxes are at after `pitches`: `None` before the
/// first pitch (fresh-raked boxes).
pub fn cleat_stage(pitches: u32) -> Option<usize> {
    CLEAT_STAGES.iter().rposition(|&from| pitches >= from)
}

/// Texture size of the decals.
const DECAL_SIZE: usize = 64;

/// An alpha mask for a decal: white, with `alpha(u, v)` (u, v in 0..1).
fn mask_image(alpha: impl Fn(f32, f32) -> f32) -> Image {
    let n = DECAL_SIZE;
    let mut data = vec![255u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (u, v) = ((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32);
            data[(y * n + x) * 4 + 3] = (alpha(u, v).clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    clamp_image(n as u32, data)
}

/// A worn lane between two bases: darkest down its middle, ragged at the
/// sides, fading out over the last stretch at each end (the bag cutouts
/// take over there). `u` runs across the lane, `v` along it.
pub(super) fn path_mask() -> Image {
    mask_image(|u, v| {
        let across = 1.0 - ((u - 0.5).abs() * 2.0).powf(1.6);
        let ends = ((v.min(1.0 - v) - 0.02) / 0.12).clamp(0.0, 1.0);
        let ragged = 0.75 + 0.25 * tiling_noise(u * 6.0, v * 40.0, 1024, 2.2);
        0.42 * across * ends * ragged
    })
}

/// The halo at home: the raked circle's edge scuffed out over the grass
/// line, and a faint, uneven wash across the circle itself. `r` is the
/// distance from the centre as a fraction of the decal's radius.
pub(super) fn halo_mask() -> Image {
    mask_image(|u, v| {
        let (x, y) = (u * 2.0 - 1.0, v * 2.0 - 1.0);
        let r = (x * x + y * y).sqrt();
        let angle = y.atan2(x);
        let fray = tiling_noise(angle * 6.0 + 50.0, r * 8.0, 1024, 4.4);
        let edge = HALO_EDGE + (fray - 0.5) * 0.08;
        if r >= 1.0 {
            0.0
        } else if r > edge {
            // The scuffed rim, thinning out past the grass line.
            0.5 * (1.0 - (r - edge) / (1.0 - edge)) * (0.6 + 0.4 * fray)
        } else {
            // The circle: worn darker where the batter, catcher and umpire
            // stand, lighter at the sides.
            let worn = 0.12 + 0.12 * tiling_noise(u * 9.0, v * 9.0, 1024, 6.6);
            worn + 0.3 * (r / edge).powi(6)
        }
    })
}

/// Where the home circle's edge falls on the halo decal (fraction of its
/// radius); the decal reaches a little past it, over the grass.
const HALO_EDGE: f32 = 1.0 / HALO_OVERHANG;
/// How far past the home circle the halo reaches (multiple of its radius).
const HALO_OVERHANG: f32 = 1.15;

/// Cleat marks in a box at wear `stage` (index into [`CLEAT_STAGES`]): short
/// scuffs, densest where the feet plant (the box's middle and front half),
/// each mark arriving at a stage and staying for the rest of the game.
pub(super) fn cleat_mask(stage: usize, seed: f32) -> Image {
    const MARKS: usize = 70;
    let marks: Vec<(Vec2, Vec2, usize)> = (0..MARKS)
        .map(|i| {
            let f = i as f32 + seed * 100.0;
            // Feet plant across the box but favour its middle.
            let centre = Vec2::new(
                0.5 + (hash01(f * 1.31) - 0.5) * 0.8,
                0.5 + (hash01(f * 2.17) + hash01(f * 3.07) - 1.0) * 0.6,
            );
            let angle = hash01(f * 5.3) * std::f32::consts::PI;
            let half = Vec2::new(angle.cos(), angle.sin()) * (0.03 + 0.05 * hash01(f * 7.9));
            // Earlier stages hold fewer marks.
            let arrives = (hash01(f * 11.3) * CLEAT_STAGES.len() as f32) as usize;
            (centre, half, arrives.min(CLEAT_STAGES.len() - 1))
        })
        .collect();
    mask_image(|u, v| {
        let p = Vec2::new(u, v);
        marks
            .iter()
            .filter(|(_, _, arrives)| *arrives <= stage)
            .map(|(c, half, _)| {
                let d = segment_distance(p, *c - *half, *c + *half);
                (1.0 - d / 0.022).clamp(0.0, 1.0) * 0.7
            })
            .fold(0.0, f32::max)
    })
}

fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Total opacity of a mask (sum of its alpha, 0..=1 per pixel): how much a
/// decal covers.
#[cfg(test)]
pub(super) fn coverage(image: &Image) -> f32 {
    image
        .data
        .as_ref()
        .map(|d| d.chunks(4).map(|px| px[3] as f32 / 255.0).sum())
        .unwrap_or(0.0)
}

/// Height the decals sit at: over every ground layer (the stadium's grass
/// interior is the topmost at 0.0027 m) and under the chalk.
pub(super) const DECAL_Y: f32 = 0.0045;

/// Where the decals go and how they look: the parts `spawn_decals` needs
/// from the field and the clay.
pub(super) struct DecalPlan {
    /// Home, first, second, third.
    pub bases: [Vec3; 4],
    /// Radius of the home-plate dirt circle (m).
    pub home_radius: f32,
    /// Each batter's box: centre and half-size (x, z).
    pub boxes: [(Vec3, Vec2); 2],
    /// The clay's own colour (texture times tint), which worn ground darkens.
    pub clay: Color,
}

/// Spawns the decals, hidden unless the tier at game start budgets them
/// (`decals_follow_budget` keeps them in step after).
pub(super) fn spawn_decals(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    plan: &DecalPlan,
    shown: bool,
) {
    let visibility = if shown {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    let clay = plan.clay.to_linear();
    let shade =
        |k: f32| Color::LinearRgba(LinearRgba::rgb(clay.red * k, clay.green * k, clay.blue * k));
    let decal_material =
        |materials: &mut Assets<StandardMaterial>, texture: Handle<Image>, k: f32| {
            materials.add(StandardMaterial {
                base_color: shade(k),
                base_color_texture: Some(texture),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.95,
                ..default()
            })
        };
    let quad = meshes.add(Plane3d::default().mesh().size(1.0, 1.0));

    // Worn lanes, a little inside each base line, where the runners go.
    let path = decal_material(materials, images.add(path_mask()), 0.62);
    let centre = (plan.bases[0] + plan.bases[2]) / 2.0;
    for i in 0..4 {
        let (from, to) = (plan.bases[i], plan.bases[(i + 1) % 4]);
        let along = to - from;
        let inward = (centre - (from + to) / 2.0).normalize_or_zero();
        let mid = (from + to) / 2.0 + inward * LANE_INSET;
        commands.spawn((
            SurfaceDecal,
            GameplayEntity,
            Mesh3d(quad.clone()),
            MeshMaterial3d(path.clone()),
            Transform {
                translation: mid.with_y(DECAL_Y),
                rotation: Quat::from_rotation_y(along.x.atan2(along.z)),
                scale: Vec3::new(LANE_WIDTH, 1.0, along.length()),
            },
            visibility,
        ));
    }

    // The halo round home.
    let halo = decal_material(materials, images.add(halo_mask()), 0.7);
    commands.spawn((
        SurfaceDecal,
        GameplayEntity,
        Mesh3d(quad.clone()),
        MeshMaterial3d(halo),
        Transform {
            translation: plan.bases[0].with_y(DECAL_Y),
            scale: Vec3::new(1.0, 1.0, 1.0) * plan.home_radius * HALO_OVERHANG * 2.0,
            ..default()
        },
        visibility,
    ));

    // Cleat marks: one decal per box, its texture swapped as the wear grows,
    // hidden until the first pitch.
    for (i, (at, half)) in plan.boxes.iter().enumerate() {
        let stages: Vec<Handle<Image>> = (0..CLEAT_STAGES.len())
            .map(|stage| images.add(cleat_mask(stage, i as f32 + 0.37)))
            .collect();
        commands.spawn((
            SurfaceDecal,
            CleatMarks {
                stages: stages.clone(),
            },
            GameplayEntity,
            Mesh3d(quad.clone()),
            MeshMaterial3d(decal_material(materials, stages[0].clone(), 0.45)),
            Transform {
                translation: at.with_y(DECAL_Y),
                scale: Vec3::new(half.x * 2.0, 1.0, half.y * 2.0),
                ..default()
            },
            Visibility::Hidden,
        ));
    }
}

/// The worn lane's width (m) and how far inside the base line its centre
/// runs (on the clay band, where the runners' feet land).
const LANE_WIDTH: f32 = 1.1;
const LANE_INSET: f32 = 0.6;

/// Counts the game's pitches for the cleat marks; a new game starts with
/// fresh-raked boxes.
pub(super) fn reset_cleat_wear(mut commands: Commands) {
    commands.insert_resource(CleatWear::default());
}

pub(super) fn count_pitches(mut pitches: MessageReader<PitchEvent>, mut wear: ResMut<CleatWear>) {
    let thrown = pitches.read().count() as u32;
    if thrown > 0 {
        wear.pitches += thrown;
    }
}

/// A decal as `decals_follow_budget` sees it.
type DecalRow<'a> = (
    &'a mut Visibility,
    Option<&'a CleatMarks>,
    &'a MeshMaterial3d<StandardMaterial>,
    Ref<'a, SurfaceDecal>,
);

/// Keeps the decals in step with the tier (Auto's step-downs, a settings
/// change mid-game) and the cleat marks with the wear.
pub(super) fn decals_follow_budget(
    active: Option<Res<ActiveQuality>>,
    wear: Res<CleatWear>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut decals: Query<DecalRow>,
    mut shown_stage: Local<Option<usize>>,
) {
    let tier_changed = active.as_ref().is_some_and(|a| a.is_changed());
    let stage = cleat_stage(wear.pitches);
    let stage_changed = *shown_stage != stage;
    let on = active.as_ref().is_some_and(|a| a.budget().surface_color);
    for (mut visibility, cleats, material, marker) in &mut decals {
        if !(tier_changed || stage_changed || marker.is_added()) {
            continue;
        }
        let want_on = on && (cleats.is_none() || stage.is_some());
        let want = if want_on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
        if let (Some(cleats), Some(stage)) = (cleats, stage)
            && let Some(material) = materials.get_mut(&material.0)
        {
            let texture = cleats.stages[stage].clone();
            if material.base_color_texture.as_ref() != Some(&texture) {
                material.base_color_texture = Some(texture);
            }
        }
    }
    *shown_stage = stage;
}

// ── Images ────────────────────────────────────────────────────────────────────

fn repeat_image(size: u32, data: Vec<u8>, format: TextureFormat) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
    image
}

/// A decal mask: clamped at its edges so nothing wraps round.
fn clamp_image(size: u32, data: Vec<u8>) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..default()
    });
    image
}

#[cfg(test)]
#[path = "surface.test.rs"]
mod tests;
