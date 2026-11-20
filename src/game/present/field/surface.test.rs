//! Unit tests for [`super`] — ground surface detail.

use super::*;
use crate::game::quality::QualityTier;

fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
    let width = image.texture_descriptor.size.width as usize;
    let data = image.data.as_ref().expect("pixels");
    let at = (y * width + x) * 4;
    [data[at], data[at + 1], data[at + 2], data[at + 3]]
}

/// Medium gets the turf's shade and the cleat marks, High adds the worn
/// clay and the normal maps, Low has none.
#[test]
fn surface_detail_steps_up_with_the_tier() {
    let [low, medium, high] =
        [QualityTier::Low, QualityTier::Medium, QualityTier::High].map(QualityTier::budget);
    assert!(!low.surface_color && !low.surface_detail);
    assert!(medium.surface_color && !medium.surface_detail);
    assert!(high.surface_color && high.surface_detail);
}

#[test]
fn the_noise_tiles_and_stays_in_range() {
    for i in 0..50 {
        let (x, y) = (i as f32 * 0.37, i as f32 * 0.91);
        let n = tiling_noise(x, y, 8, 1.0);
        assert!((0.0..=1.0).contains(&n));
        assert!((n - tiling_noise(x + 8.0, y, 8, 1.0)).abs() < 1e-5);
        assert!((n - tiling_noise(x, y + 8.0, 8, 1.0)).abs() < 1e-5);
    }
}

/// The normal maps point up everywhere, aren't flat, and tile: the wrap
/// from the last column to the first is no steeper than inside the tile.
#[test]
fn the_normal_maps_are_bumpy_tiling_and_up() {
    for image in [grass_normal_image(), dirt_normal_image()] {
        assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        let n = NORMAL_SIZE;
        let mut tilted = 0;
        let mut inner_step: u32 = 0;
        let mut wrap_step: u32 = 0;
        for y in 0..n {
            for x in 0..n {
                let [r, g, b, _] = pixel(&image, x, y);
                assert!(b > 160, "a normal tips past 45 degrees at {x},{y}");
                if r.abs_diff(128) > 6 || g.abs_diff(128) > 6 {
                    tilted += 1;
                }
                if x + 1 < n {
                    inner_step = inner_step.max(u32::from(r.abs_diff(pixel(&image, x + 1, y)[0])));
                }
            }
            wrap_step = wrap_step.max(u32::from(
                pixel(&image, n - 1, y)[0].abs_diff(pixel(&image, 0, y)[0]),
            ));
        }
        assert!(tilted > n * n / 4, "too flat to see: {tilted}");
        assert!(
            wrap_step <= inner_step,
            "a seam at the tile edge: {wrap_step} vs {inner_step}"
        );
    }
}

/// The ground's shade wanders by a few percent and no more.
#[test]
fn the_ground_shade_varies_gently() {
    let shades: Vec<f32> = (0..200)
        .map(|i| ground_shade(i as f32 * 1.7 - 150.0, i as f32 * 0.9 - 40.0)[1])
        .collect();
    let lo = shades.iter().copied().fold(f32::MAX, f32::min);
    let hi = shades.iter().copied().fold(f32::MIN, f32::max);
    assert!(lo >= 1.0 - SHADE_SWING - 1e-4 && hi <= 1.0 + SHADE_SWING + 1e-4);
    assert!(hi - lo > SHADE_SWING * 0.6, "barely varies: {lo}..{hi}");
}

#[test]
fn the_varied_ground_is_coloured_at_every_vertex() {
    let mesh = varied_ground_mesh(150.0);
    let positions = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .expect("positions")
        .len();
    let colors = mesh
        .attribute(Mesh::ATTRIBUTE_COLOR)
        .expect("colours")
        .len();
    assert_eq!(positions, colors);
    assert!(positions >= 100, "too coarse to vary: {positions}");
}

/// Home, first (world -X), second, third, as the stadium lays them out.
fn bases() -> [Vec2; 4] {
    let h = crate::game::rules::HALF_DIAGONAL;
    [
        Vec2::ZERO,
        Vec2::new(-h, h),
        Vec2::new(0.0, 2.0 * h),
        Vec2::new(h, h),
    ]
}

/// The clay wears down each runners' lane and round the home circle's rim,
/// and stays fresh in the middle of the band, at the bags and away from
/// both.
#[test]
fn the_clay_wears_where_the_runners_and_batters_go() {
    let b = bases();
    let radius = 3.96;
    let centre = (b[0] + b[2]) / 2.0;
    for i in 0..4 {
        let (from, to) = (b[i], b[(i + 1) % 4]);
        let mid = (from + to) / 2.0;
        let inward = (centre - mid).normalize();
        let on_lane = mid + inward * LANE_INSET;
        assert!(clay_wear(on_lane, &b, radius) > 0.5, "lane {i}");
        let inside = mid + inward * 3.0;
        assert!(clay_wear(inside, &b, radius) < 0.05, "band {i}");
        let at_bag = to + (from - to).normalize() * 0.5 + inward * LANE_INSET;
        if i < 3 {
            assert!(clay_wear(at_bag, &b, radius) < 0.05, "bag {i}");
            // Still worn just inside the bag's cutout, so the fade shows on
            // the (painted) cutout rather than hiding beneath it.
            let in_pit = to + (from - to).normalize() * 3.0 + inward * LANE_INSET;
            assert!(clay_wear(in_pit, &b, radius) > 0.3, "pit {i}");
        }
    }
    let rim = Vec2::new(0.0, -radius * 0.92);
    assert!(clay_wear(rim, &b, radius) > 0.5, "home rim");
    let middle = Vec2::new(0.0, -radius * 0.4);
    assert!(clay_wear(middle, &b, radius) < clay_wear(rim, &b, radius));
    assert!(clay_wear(Vec2::new(0.0, -radius * 2.0), &b, radius) < 0.05);
}

/// The worn meshes face up, carry a shade per vertex and darken where worn.
#[test]
fn the_worn_clay_meshes_are_painted_by_their_wear() {
    let at = Transform::from_xyz(0.0, 0.002, 0.0);
    let wear = |p: Vec2| (p.length() / 4.0).min(1.0);
    for mesh in [
        worn_clay_disc(4.0, 0.0005, &at, wear),
        worn_clay_frame(4.0, 1.5, 0.0005, &at, wear),
    ] {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions");
        };
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            panic!("colours");
        };
        assert_eq!(positions.len(), colors.len());
        // The least-worn (innermost) vertex reads lighter than the most-worn
        // (outermost).
        let radius = |p: &[f32; 3]| Vec2::new(p[0], p[2]).length();
        let shade_at = |far: bool| {
            positions
                .iter()
                .zip(colors)
                .max_by(|(a, _), (b, _)| {
                    let (a, b) = (radius(a), radius(b));
                    if far {
                        a.total_cmp(&b)
                    } else {
                        b.total_cmp(&a)
                    }
                })
                .map(|(_, c)| c[0])
                .expect("vertices")
        };
        let (centre, edge) = (shade_at(false), shade_at(true));
        assert!(
            centre > edge,
            "worn ground reads darker: {centre} vs {edge}"
        );
        // Every triangle faces up.
        let Some(indices) = mesh.indices() else {
            panic!("indices")
        };
        let idx: Vec<usize> = indices.iter().collect();
        for tri in idx.chunks(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| Vec3::from(positions[i]));
            assert!((b - a).cross(c - a).y > 0.0, "a face points down");
        }
    }
}

/// The boxes start fresh and wear in: each stage keeps every earlier mark
/// and adds more.
#[test]
fn cleat_marks_build_through_the_game() {
    assert_eq!(cleat_stage(0), None);
    assert_eq!(cleat_stage(1), Some(0));
    assert_eq!(cleat_stage(39), Some(0));
    assert_eq!(cleat_stage(40), Some(1));
    assert_eq!(cleat_stage(300), Some(CLEAT_STAGES.len() - 1));
    let covered: Vec<f32> = (0..CLEAT_STAGES.len())
        .map(|stage| coverage(&cleat_mask(stage, 0.37)))
        .collect();
    assert!(covered[0] > 0.0, "the first stage shows marks");
    for pair in covered.windows(2) {
        assert!(pair[1] > pair[0], "{covered:?}");
    }
    // Every mark of a stage is still there in the next.
    let (early, late) = (cleat_mask(1, 0.37), cleat_mask(2, 0.37));
    for y in 0..DECAL_SIZE {
        for x in 0..DECAL_SIZE {
            assert!(pixel(&late, x, y)[3] >= pixel(&early, x, y)[3]);
        }
    }
}
