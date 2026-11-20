//! Ground surface detail (graphics plan task 16), by tier
//! ([`RenderBudget::surface_color`], [`RenderBudget::surface_detail`]):
//!
//! - Medium and High: turf that varies in shade across the park (vertex
//!   colours on a coarse grid) and cleat marks in the batter's boxes that
//!   build as the game goes on (two small blended decals).
//! - High also: clay worn darker down the runners' lanes and round the
//!   home-plate circle (vertex colours on gridded clay, opaque so it costs
//!   no extra fill), and procedural normal maps on the turf and the clay.
//! - Low: none of it; the park is exactly as before.
//!
//! The split is measured (`tools/frame_cost.mjs`): under software WebGL the
//! worn clay's few thousand triangles cost Medium ~7% a frame, so it is
//! High's, and blended overlay decals for the lanes and the halo cost ~25%
//! (a full PBR pass over much of the batting view), so they are baked into
//! the clay instead.
//!
//! Like the rest of the park, the ground mesh and the normal maps are picked
//! once at game start (a mid-game step-down keeps them). The cleat decals
//! follow the tier mid-game by `Visibility`, as the towers' fill spots do. Every texture is generated at runtime from hashes,
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
        2.5,
    )
}

// ── Ground variation (Medium and High) ────────────────────────────────────────

/// How far the turf's shade wanders across the park (fraction of its
/// colour, either way), and over what distance (m).
const SHADE_SWING: f32 = 0.09;
const SHADE_CELL_M: f32 = 30.0;

/// The ground's vertex shade at `(x, z)` (m): a multiplier near 1, the turf
/// a touch warmer where it's lighter (sun-faded) and cooler where darker.
pub(super) fn ground_shade(x: f32, z: f32) -> [f32; 4] {
    // A period far beyond the park, so nothing repeats on screen.
    let (u, v) = (x / SHADE_CELL_M + 400.0, z / SHADE_CELL_M + 400.0);
    // One octave: the grid samples it every ~10 m, three to a cell, so a
    // finer octave would only alias.
    let s = (tiling_noise(u, v, 1024, 1.3) * 2.0 - 1.0) * SHADE_SWING;
    [1.0 + s * 1.1, 1.0 + s, 1.0 + s * 0.6, 1.0]
}

/// The ground slab's visible top: a `half`-metre square at y = 0, gridded
/// every ~10 m (on the stadium's 150 m half) and vertex-coloured by
/// [`ground_shade`].
pub(super) fn varied_ground_mesh(half: f32) -> Mesh {
    let mut mesh = Plane3d::default()
        .mesh()
        .size(half * 2.0, half * 2.0)
        .subdivisions(29)
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

// ── Worn clay (High) ──────────────────────────────────────────────────

/// A worn lane's half-width (m) and how far inside the base line its centre
/// runs (on the clay band, where the runners' feet land).
const LANE_HALF: f32 = 0.6;
const LANE_INSET: f32 = 0.6;
/// How much the most-worn clay darkens.
const WEAR_DARKEN: f32 = 0.32;

/// How worn the clay is at `p` (x, z; m), 0 to 1: the runners' lanes a
/// little inside each base line, fading out near the bags, and the
/// home-plate circle, raked smooth in the middle and scuffed out at its
/// rim. `bases` are home, first, second and third.
pub(super) fn clay_wear(p: Vec2, bases: &[Vec2; 4], home_radius: f32) -> f32 {
    let centre = (bases[0] + bases[2]) / 2.0;
    let lane = (0..4)
        .map(|i| {
            let (a, b) = (bases[i], bases[(i + 1) % 4]);
            let inward = (centre - (a + b) / 2.0).normalize_or_zero() * LANE_INSET;
            let (a, b) = (a + inward, b + inward);
            let len = a.distance(b);
            let t = (p - a).dot(b - a) / (len * len).max(1e-6);
            let from_bag = t.min(1.0 - t) * len;
            let fade = ((from_bag - 1.0) / 2.5).clamp(0.0, 1.0);
            let d = segment_distance(p, a, b) / LANE_HALF;
            (1.0 - d * d).max(0.0) * fade
        })
        .fold(0.0, f32::max);
    let r = p.distance(bases[0]);
    let rim = (-((r - home_radius * 0.92) / 0.5).powi(2)).exp();
    let circle = if r < home_radius { 0.25 } else { 0.0 };
    let ragged = 0.85 + 0.3 * tiling_noise(p.x * 0.9 + 300.0, p.y * 0.9 + 300.0, 1024, 8.8);
    (lane.max(rim * 0.9).max(circle) * ragged).clamp(0.0, 1.0)
}

/// The vertex shade for clay worn `wear` deep: darker, a touch redder.
fn wear_shade(wear: f32) -> [f32; 4] {
    let k = 1.0 - WEAR_DARKEN * wear;
    [k, k * 0.97, k * 0.94, 1.0]
}

/// Paints `mesh`'s vertices by `wear` at each one's world position under
/// `transform`.
fn paint_wear(mut mesh: Mesh, transform: &Transform, wear: impl Fn(Vec2) -> f32) -> Mesh {
    let colors: Vec<[f32; 4]> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(positions)) => positions
            .iter()
            .map(|p| wear_shade(wear(transform.transform_point(Vec3::from(*p)).xz())))
            .collect(),
        _ => Vec::new(),
    };
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh
}

/// The clay band round a square of clay (`half` m), `band` m deep from
/// its edges in: drawn as its top face at `top` m and painted by `wear`
/// where it will stand. Only the band, since the grass interior covers the
/// rest; finest across the lane a little in from each edge.
pub(super) fn worn_clay_frame(
    half: f32,
    band: f32,
    top: f32,
    transform: &Transform,
    wear: impl Fn(Vec2) -> f32,
) -> Mesh {
    const ALONG: u32 = 24;
    let depths = [0.0, 0.3, 0.6, 0.9, 1.3, 2.2, band];
    let mut positions = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    // Each side as a mitred strip: its edge along +x at z = half, rotated
    // a quarter turn per side.
    for side in 0..4 {
        let turn = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2 * side as f32);
        let first = positions.len() as u32;
        for &d in &depths {
            let reach = half - d;
            for i in 0..=ALONG {
                let x = -reach + 2.0 * reach * i as f32 / ALONG as f32;
                positions.push((turn * Vec3::new(x, top, half - d)).to_array());
            }
        }
        let row = ALONG + 1;
        for r in 0..depths.len() as u32 - 1 {
            for i in 0..ALONG {
                let (a, b) = (first + r * row + i, first + r * row + i + 1);
                let (c, d) = (a + row, b + row);
                indices.extend([a, b, d, a, d, c]);
            }
        }
    }
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .map(|p| [p[0] / (2.0 * half) + 0.5, p[2] / (2.0 * half) + 0.5])
        .collect();
    let mesh = up_facing_mesh(positions, uvs, indices);
    paint_wear(mesh, transform, wear)
}

/// A flat mesh facing +Y, with every triangle wound to face up.
fn up_facing_mesh(positions: Vec<[f32; 3]>, uvs: Vec<[f32; 2]>, mut indices: Vec<u32>) -> Mesh {
    for tri in indices.chunks_mut(3) {
        let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from(positions[i as usize]));
        if (b - a).cross(c - a).y < 0.0 {
            tri.swap(1, 2);
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; positions.len()];
    Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(bevy::mesh::Indices::U32(indices))
}

/// A thin disc of clay (`radius` m), drawn as its top face at `top` m in
/// rings placed to carry the wear, painted by `wear` where it will stand.
/// Textured like a cylinder's cap.
pub(super) fn worn_clay_disc(
    radius: f32,
    top: f32,
    transform: &Transform,
    wear: impl Fn(Vec2) -> f32,
) -> Mesh {
    // Rings where the wear changes: the raked middle, then close in to
    // the scuffed rim.
    const RINGS: [f32; 6] = [0.35, 0.65, 0.8, 0.9, 0.96, 1.0];
    const SEGMENTS: u32 = 40;
    let mut positions = vec![[0.0, top, 0.0]];
    for f in RINGS {
        let r = radius * f;
        for seg in 0..SEGMENTS {
            let a = std::f32::consts::TAU * seg as f32 / SEGMENTS as f32;
            positions.push([r * a.cos(), top, r * a.sin()]);
        }
    }
    let uvs: Vec<[f32; 2]> = positions
        .iter()
        .map(|p| [p[0] / (2.0 * radius) + 0.5, p[2] / (2.0 * radius) + 0.5])
        .collect();
    let at = |ring: u32, seg: u32| 1 + ring * SEGMENTS + seg % SEGMENTS;
    let mut indices = Vec::new();
    for seg in 0..SEGMENTS {
        indices.extend([0, at(0, seg + 1), at(0, seg)]);
    }
    for ring in 0..RINGS.len() as u32 - 1 {
        for seg in 0..SEGMENTS {
            let (a, b) = (at(ring, seg), at(ring, seg + 1));
            let (c, d) = (at(ring + 1, seg), at(ring + 1, seg + 1));
            indices.extend([a, b, d, a, d, c]);
        }
    }
    let mesh = up_facing_mesh(positions, uvs, indices);
    paint_wear(mesh, transform, wear)
}

// ── Cleat marks (Medium and High) ─────────────────────────────────────────────

/// Marks a ground decal (the cleat marks in the batter's boxes).
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

/// Spawns the cleat-mark decals in `boxes` (centre and half-size, x and z),
/// tinted from the `clay`'s own colour; hidden until the first pitch, and
/// shown only on tiers that budget them (`decals_follow_budget`).
pub(super) fn spawn_cleat_marks(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    boxes: [(Vec3, Vec2); 2],
    clay: Color,
) {
    let clay = clay.to_linear();
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

    // Cleat marks: one decal per box, its texture swapped as the wear grows,
    // hidden until the first pitch.
    for (i, (at, half)) in boxes.iter().enumerate() {
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
