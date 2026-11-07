//! Helpers for turning CPU-generated pixel data into GPU textures with mipmaps.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Box-filters an RGBA8 image down to 1x1, returning all levels concatenated.
/// Note: averaging sRGB values directly is slightly dark but fine for terrain detail.
fn build_mips(mut data: Vec<u8>, mut w: u32, mut h: u32) -> (Vec<u8>, u32) {
    let mut levels = 1;
    let mut prev_start = 0;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = Vec::with_capacity((nw * nh * 4) as usize);
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += data[prev_start + ((sy * w + sx) * 4 + c) as usize] as u32;
                    }
                    next.push((sum / 4) as u8);
                }
            }
        }
        prev_start = data.len();
        data.extend(next);
        w = nw;
        h = nh;
        levels += 1;
    }
    (data, levels)
}

/// Recomputes the mipmaps (as [`build_mips`] made them) over the base-level
/// texels `x0..=x1` (wrapping in x), `y0..=y1` after they were rewritten.
pub fn update_mips(data: &mut [u8], width: u32, height: u32, x0: i64, x1: i64, y0: u32, y1: u32) {
    let (mut w, mut h) = (width, height);
    let mut start = 0usize;
    let (mut x0, mut x1, mut y0, mut y1) = (x0, x1, y0 as i64, y1 as i64);
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let next = start + (w * h * 4) as usize;
        (x0, x1, y0, y1) = (x0.div_euclid(2), x1.div_euclid(2), y0 / 2, (y1 / 2).min(nh as i64 - 1));
        // The whole level once the rectangle is as wide as it.
        let (ax0, ax1) = if x1 - x0 + 1 >= nw as i64 { (0, nw as i64 - 1) } else { (x0, x1) };
        for y in y0.max(0)..=y1 {
            for xu in ax0..=ax1 {
                let x = xu.rem_euclid(nw as i64) as u32;
                let y = y as u32;
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += data[start + ((sy * w + sx) * 4 + c) as usize] as u32;
                    }
                    data[next + ((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
                }
            }
        }
        start = next;
        (w, h) = (nw, nh);
    }
}

/// Creates a mipmapped, anisotropically filtered RGBA8 texture.
pub fn rgba_image(data: Vec<u8>, width: u32, height: u32, srgb: bool, repeat_u: bool, repeat_v: bool) -> Image {
    let (data, levels) = build_mips(data, width, height);
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let mut image = Image::new_uninit(
        Extent3d { width, height, depth_or_array_layers: 1 },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_descriptor.mip_level_count = levels;
    let mode = |repeat| if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode(repeat_u),
        address_mode_v: mode(repeat_v),
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 16,
        ..Default::default()
    });
    image
}
