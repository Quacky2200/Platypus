// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use crate::{Color, Gpu, RenderError, encode_clear};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// A render target with no window, used by headless and golden-image tests.
pub struct OffscreenTarget {
    texture: Option<wgpu::Texture>,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
}

impl OffscreenTarget {
    pub fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        Self::with_format(gpu, width, height, FORMAT)
    }

    /// A target with a specific format, e.g. `Rgba8UnormSrgb` for UI golden images.
    pub fn with_format(gpu: &Gpu, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
        let mut t = Self {
            texture: None,
            format,
            width: 0,
            height: 0,
        };
        t.resize(gpu, width, height);
        t
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// A view for rendering into the target; `None` while it is 0×0.
    pub fn view(&self) -> Option<wgpu::TextureView> {
        self.texture
            .as_ref()
            .map(|t| t.create_view(&Default::default()))
    }

    /// Resizing to 0×0 is allowed (like a minimized window): the target simply has no pixels.
    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.texture = (width > 0 && height > 0).then(|| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("platypus offscreen"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        });
    }

    pub fn clear(&self, gpu: &Gpu, color: Color) {
        let Some(texture) = &self.texture else { return };
        let view = texture.create_view(&Default::default());
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encode_clear(&mut encoder, &view, color);
        gpu.queue.submit([encoder.finish()]);
    }

    /// Reads the target back as tightly packed RGBA8 rows (top row first).
    pub fn read_pixels(&self, gpu: &Gpu) -> Result<Vec<u8>, RenderError> {
        let Some(texture) = &self.texture else {
            return Ok(Vec::new());
        };
        let unpadded = self.width * 4;
        let padded = unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("platypus readback"),
            size: u64::from(padded * self.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(self.height),
                },
            },
            texture.size(),
        );
        gpu.queue.submit([encoder.finish()]);

        let (tx, rx) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        gpu.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        rx.recv()
            .map_err(|e| RenderError::Readback(e.to_string()))?
            .map_err(|e| RenderError::Readback(e.to_string()))?;

        let data = buffer
            .get_mapped_range(..)
            .map_err(|e| RenderError::Readback(e.to_string()))?;
        let mut pixels = Vec::with_capacity((unpadded * self.height) as usize);
        for row in data.chunks(padded as usize) {
            pixels.extend_from_slice(&row[..unpadded as usize]);
        }
        Ok(pixels)
    }
}
