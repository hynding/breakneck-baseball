//! Unit tests for [`super`] — ground surface detail.

use super::*;
use crate::game::quality::QualityTier;

fn pixel(image: &Image, x: usize, y: usize) -> [u8; 4] {
    let width = image.texture_descriptor.size.width as usize;
    let data = image.data.as_ref().expect("pixels");
    let at = (y * width + x) * 4;
    [data[at], data[at + 1], data[at + 2], data[at + 3]]
}

/// Medium gets the colour detail, High adds the normal maps, Low has none.
#[test]
fn surface_detail_steps_up_with_the_tier() {
    let [low, medium, high] =
        [QualityTier::Low, QualityTier::Medium, QualityTier::High].map(QualityTier::budget);
    assert!(!low.surface_color && !low.surface_normals);
    assert!(medium.surface_color && !medium.surface_normals);
    assert!(high.surface_color && high.surface_normals);
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
    assert!(positions > 5_000, "too coarse to vary: {positions}");
}

/// The worn lane fades out at its ends and is darkest down the middle.
#[test]
fn the_worn_lane_fades_at_its_ends() {
    let lane = path_mask();
    let n = DECAL_SIZE;
    assert_eq!(pixel(&lane, n / 2, 0)[3], 0);
    assert_eq!(pixel(&lane, n / 2, n - 1)[3], 0);
    let middle = pixel(&lane, n / 2, n / 2)[3];
    assert!(middle > 40, "{middle}");
    assert!(pixel(&lane, 0, n / 2)[3] < middle / 4);
}

/// The halo is round: nothing in the decal's corners, its rim the darkest
/// ring.
#[test]
fn the_halo_is_a_scuffed_ring() {
    let halo = halo_mask();
    let n = DECAL_SIZE;
    for (x, y) in [(0, 0), (n - 1, 0), (0, n - 1), (n - 1, n - 1)] {
        assert_eq!(pixel(&halo, x, y)[3], 0, "corner {x},{y}");
    }
    let centre = pixel(&halo, n / 2, n / 2)[3];
    // Just inside the home circle's edge, along +x.
    let rim_x = n / 2 + ((HALO_EDGE * 0.97) * (n as f32 / 2.0)) as usize;
    let rim = pixel(&halo, rim_x, n / 2)[3];
    assert!(rim > centre, "rim {rim} vs centre {centre}");
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
