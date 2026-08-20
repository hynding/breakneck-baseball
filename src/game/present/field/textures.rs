//! Procedural surfaces — runtime-generated textures, no asset files (the same
//! philosophy as the procedural audio and jerseys): mowing-striped grass and
//! speckled infield dirt, per the groundskeeping notes in docs/BASEBALL.md.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::game::ai::hash01;

/// Grass with alternating mow stripes and per-blade jitter.
pub(super) fn grass_image() -> Image {
    const SIZE: usize = 64;
    const STRIPE: usize = 8;
    let mut data = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        let light = (y / STRIPE) % 2 == 0;
        for x in 0..SIZE {
            let n = hash01(x as f32 * 12.9 + y as f32 * 78.2) * 14.0 - 7.0;
            let (r, g, b) = if light {
                (52.0, 142.0, 52.0)
            } else {
                (42.0, 122.0, 44.0)
            };
            let at = (y * SIZE + x) * 4;
            data[at] = (r + n) as u8;
            data[at + 1] = (g + n) as u8;
            data[at + 2] = (b + n) as u8;
            data[at + 3] = 255;
        }
    }
    tiling_image(SIZE as u32, data)
}

/// Infield dirt: warm clay with darker speckles.
pub(super) fn dirt_image() -> Image {
    const SIZE: usize = 64;
    let mut data = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let seed = x as f32 * 31.7 + y as f32 * 57.3;
            let n = hash01(seed) * 24.0 - 12.0;
            let (r, g, b) = if hash01(seed * 1.7) > 0.93 {
                (150.0, 115.0, 82.0) // a pebble
            } else {
                (194.0, 153.0, 108.0)
            };
            let at = (y * SIZE + x) * 4;
            data[at] = (r + n) as u8;
            data[at + 1] = (g + n) as u8;
            data[at + 2] = (b + n) as u8;
            data[at + 3] = 255;
        }
    }
    tiling_image(SIZE as u32, data)
}

/// Wraps raw RGBA pixels in a repeat-sampled square texture.
fn tiling_image(size: u32, data: Vec<u8>) -> Image {
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
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..default()
    });
    image
}
