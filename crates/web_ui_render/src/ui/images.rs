// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The wgpu half of the UI's images: the textures an `ImageCache` (GPU-free, `image_cache.rs`)
//! decodes pixels for, by key, each with both samplers' bind groups.

use std::collections::HashMap;

use super::image_cache::{ImageCache, ImageUpload};
use crate::Gpu;

/// The two ways images are sampled: filtered, and nearest-neighbour
/// (`image-rendering: pixelated`). Both repeat.
pub struct ImageSamplers {
    pub smooth: wgpu::Sampler,
    pub nearest: wgpu::Sampler,
}

pub struct LoadedImage {
    pub bind_group: wgpu::BindGroup,
    /// The same texture with the nearest-neighbour sampler.
    pub bind_group_nearest: wgpu::BindGroup,
}

/// Uploads straight-alpha RGBA `pixels` (`w` x `h`) as a texture with both samplers.
fn upload(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    samplers: &ImageSamplers,
    key: &str,
    pixels: &[u8],
    (w, h): (u32, u32),
) -> LoadedImage {
    let label = short(key);
    let size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * w),
            rows_per_image: Some(h),
        },
        size,
    );
    let view = texture.create_view(&Default::default());
    let bind = |sampler: &wgpu::Sampler| {
        gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    };
    LoadedImage {
        bind_group: bind(&samplers.smooth),
        bind_group_nearest: bind(&samplers.nearest),
    }
}

/// The image textures of one wgpu renderer, by the cache's key.
#[derive(Default)]
pub struct ImageTextures {
    map: HashMap<String, LoadedImage>,
}

impl ImageTextures {
    /// Applies what the cache loaded and dropped since the last call.
    pub fn apply(
        &mut self,
        gpu: &Gpu,
        layout: &wgpu::BindGroupLayout,
        samplers: &ImageSamplers,
        cache: &mut ImageCache,
    ) {
        for key in cache.take_dropped() {
            self.map.remove(&key);
        }
        for ImageUpload {
            key,
            width,
            height,
            pixels,
        } in cache.take_uploads()
        {
            let image = upload(gpu, layout, samplers, &key, &pixels, (width, height));
            self.map.insert(key, image);
        }
    }

    pub fn get(&self, key: &str) -> Option<&LoadedImage> {
        self.map.get(key)
    }
}

/// A URL shortened for log messages (data URLs can be huge).
fn short(url: &str) -> &str {
    match url.char_indices().nth(60) {
        Some((i, _)) => &url[..i],
        None => url,
    }
}

#[cfg(test)]
mod tests {
    use platypus_ui::{
        BackgroundPosition, BackgroundRepeat, BackgroundSize, BorderImageRepeat, Len, Rect,
        RepeatAxis, border_image_axis, repeat_tiles, tile_rect,
    };

    #[test]
    fn background_size_and_position_follow_css() {
        let area = Rect::new(10.0, 20.0, 100.0, 50.0);
        let natural = (20.0, 10.0);
        let top_left = BackgroundPosition::default();
        let centre = BackgroundPosition {
            x: Len::Percent(0.5),
            y: Len::Percent(0.5),
        };

        // auto: natural size at the top-left.
        let r = tile_rect(area, natural, BackgroundSize::default(), top_left);
        assert_eq!(r, Rect::new(10.0, 20.0, 20.0, 10.0));
        // cover fills the box, keeping the aspect ratio (2:1 image, 2:1 box).
        assert_eq!(
            tile_rect(area, natural, BackgroundSize::Cover, top_left),
            Rect::new(10.0, 20.0, 100.0, 50.0)
        );
        // contain on a tall box: limited by width, centred vertically.
        let tall = Rect::new(0.0, 0.0, 40.0, 100.0);
        assert_eq!(
            tile_rect(tall, natural, BackgroundSize::Contain, centre),
            Rect::new(0.0, 40.0, 40.0, 20.0)
        );
        // cover on a tall box: limited by height, centred horizontally.
        assert_eq!(
            tile_rect(tall, natural, BackgroundSize::Cover, centre),
            Rect::new(-80.0, 0.0, 200.0, 100.0)
        );
        // One explicit length keeps the aspect ratio; percentages use the box.
        assert_eq!(
            tile_rect(
                area,
                natural,
                BackgroundSize::Explicit(Some(Len::Px(40.0)), None),
                top_left
            ),
            Rect::new(10.0, 20.0, 40.0, 20.0)
        );
        assert_eq!(
            tile_rect(
                area,
                natural,
                BackgroundSize::Explicit(Some(Len::Percent(0.5)), Some(Len::Px(5.0))),
                centre
            ),
            Rect::new(35.0, 42.5, 50.0, 5.0)
        );
        // Pixel positions offset from the top-left.
        let px = BackgroundPosition {
            x: Len::Px(7.0),
            y: Len::Px(3.0),
        };
        assert_eq!(
            tile_rect(area, natural, BackgroundSize::default(), px),
            Rect::new(17.0, 23.0, 20.0, 10.0)
        );
    }

    // M3-BRD-04
    #[test]
    fn border_image_tiles_along_an_axis() {
        use BorderImageRepeat::*;
        assert_eq!(
            border_image_axis(10.0, 100.0, 30.0, Stretch),
            Some((10.0, 100.0, 0.0, false))
        );
        assert_eq!(
            border_image_axis(10.0, 100.0, 30.0, Repeat),
            Some((45.0, 30.0, 0.0, true)),
            "centred"
        );
        let (start, size, gap, rep) = border_image_axis(10.0, 100.0, 30.0, Round).unwrap();
        assert!((size - 100.0 / 3.0).abs() < 1e-4 && start == 10.0 && gap == 0.0 && rep);
        assert_eq!(
            border_image_axis(0.0, 100.0, 30.0, Space),
            Some((2.5, 30.0, 2.5, true))
        );
        assert_eq!(
            border_image_axis(0.0, 20.0, 30.0, Space),
            None,
            "no room for one"
        );
    }

    // M3-BG-02
    #[test]
    fn round_and_space_fit_whole_tiles() {
        let area = Rect::new(0.0, 0.0, 100.0, 50.0);
        let tile = Rect::new(7.0, 3.0, 30.0, 22.0);
        let repeat = |x, y| BackgroundRepeat { x, y };
        // 100 / 30 rounds to 3 tiles of 33.3; 50 / 22 rounds to 2 tiles of 25.
        let (t, gap, rep) = repeat_tiles(tile, area, repeat(RepeatAxis::Round, RepeatAxis::Round));
        assert!((t.width - 100.0 / 3.0).abs() < 1e-4 && t.height == 25.0);
        assert_eq!((t.x, t.y, gap, rep), (0.0, 0.0, [0.0, 0.0], [true, true]));
        // Space: 3 tiles of 30 with 5px gaps; only 2 of 22 fit in 50, one 6px gap.
        let (t, gap, rep) = repeat_tiles(tile, area, repeat(RepeatAxis::Space, RepeatAxis::Space));
        assert_eq!((t.x, t.width, t.y, t.height), (0.0, 30.0, 0.0, 22.0));
        assert_eq!((gap, rep), ([5.0, 6.0], [true, true]));
        // A tile too big to space twice is placed once, where `position` put it.
        let big = Rect::new(7.0, 3.0, 60.0, 30.0);
        let (t, _, rep) = repeat_tiles(big, area, repeat(RepeatAxis::Space, RepeatAxis::NoRepeat));
        assert_eq!((t.x, rep), (7.0, [false, false]));
    }
}
