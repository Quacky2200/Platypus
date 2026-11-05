// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The wgpu half of the glyph atlas: the two page textures (an R8 coverage page and an RGBA
//! page for colour emoji) the [`GlyphCache`] places glyphs in. The cache is GPU-free
//! (`glyphs.rs`); this writes the pixels it queues into the textures.

use platypus_ui::GlyphRun;

use super::glyphs::{COLOR_SIZE, GlyphCache, GlyphEntry, MASK_SIZE};
use crate::Gpu;

struct Page {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    bytes_per_pixel: u32,
}

impl Page {
    fn new(gpu: &Gpu, size: u32, format: wgpu::TextureFormat, label: &str) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        Self {
            texture,
            view,
            bytes_per_pixel: if format == wgpu::TextureFormat::R8Unorm {
                1
            } else {
                4
            },
        }
    }

    /// Writes `data` (a `width` x `height` image) at (`x`, `y`).
    fn write(&self, gpu: &Gpu, x: u32, y: u32, width: u32, height: u32, data: &[u8]) {
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * self.bytes_per_pixel),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
}

pub struct GlyphAtlas {
    mask: Page,
    color: Page,
    cache: GlyphCache,
}

impl GlyphAtlas {
    pub fn new(gpu: &Gpu) -> Self {
        Self {
            mask: Page::new(
                gpu,
                MASK_SIZE,
                wgpu::TextureFormat::R8Unorm,
                "glyph coverage",
            ),
            color: Page::new(
                gpu,
                COLOR_SIZE,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                "glyph colour",
            ),
            cache: GlyphCache::new(),
        }
    }

    pub fn mask_view(&self) -> &wgpu::TextureView {
        &self.mask.view
    }

    pub fn color_view(&self) -> &wgpu::TextureView {
        &self.color.view
    }

    /// Call once per frame before rasterizing (see [`GlyphCache::begin_frame`]).
    pub fn begin_frame(&mut self) {
        self.cache.begin_frame();
    }

    /// The entry for one glyph (see [`GlyphCache::glyph`]); its pixels reach the page when
    /// [`Self::flush`] runs, before the frame's draws.
    pub fn glyph(
        &mut self,
        run: &GlyphRun,
        glyph: u32,
        size_px: f32,
        subpixel: u8,
        stroke_px: f32,
    ) -> Option<GlyphEntry> {
        self.cache.glyph(run, glyph, size_px, subpixel, stroke_px)
    }

    /// Writes the glyphs placed since the last flush into the page textures.
    pub fn flush(&mut self, gpu: &Gpu) {
        for u in self.cache.take_uploads() {
            let page = if u.color { &self.color } else { &self.mask };
            page.write(gpu, u.x, u.y, u.width, u.height, &u.data);
        }
    }
}
