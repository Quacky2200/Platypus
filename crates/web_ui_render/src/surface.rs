// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use crate::{Color, Gpu, encode_clear};

/// A window's swapchain. Tolerates 0×0 sizes (minimized windows) by skipping frames.
pub struct WindowSurface {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    configured: bool,
}

impl WindowSurface {
    pub(crate) fn new(gpu: &Gpu, surface: wgpu::Surface<'static>, width: u32, height: u32) -> Self {
        let caps = surface.get_capabilities(&gpu.adapter);
        // Prefer an sRGB format so colours written in linear space display correctly.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoNoVsync,
            desired_maximum_frame_latency: frame_latency(gpu.adapter.get_info().backend),
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::Auto,
        };
        let mut s = Self {
            surface,
            config,
            configured: false,
        };
        s.resize(gpu, width, height);
        s
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    pub fn resize(&mut self, gpu: &Gpu, width: u32, height: u32) {
        if width == 0 || height == 0 {
            self.configured = false;
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&gpu.device, &self.config);
        self.configured = true;
    }

    /// Vsync on (frames wait for the display's refresh) or off (shown as soon as
    /// they're drawn; may tear). wgpu picks what the platform has for each:
    /// relaxed / plain FIFO, or mailbox / immediate.
    pub fn set_vsync(&mut self, gpu: &Gpu, on: bool) {
        let mode = if on {
            wgpu::PresentMode::AutoVsync
        } else {
            wgpu::PresentMode::AutoNoVsync
        };
        if self.config.present_mode == mode {
            return;
        }
        self.config.present_mode = mode;
        if self.configured {
            self.surface.configure(&gpu.device, &self.config);
        }
    }

    pub fn vsync(&self) -> bool {
        self.config.present_mode != wgpu::PresentMode::AutoNoVsync
    }

    /// Size in physical pixels.
    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// Clears the window to `color` and presents it.
    pub fn present_clear(&mut self, gpu: &Gpu, color: Color) {
        self.present_with(gpu, |view, _, _| {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            encode_clear(&mut encoder, view, color);
            gpu.queue.submit([encoder.finish()]);
        });
    }

    /// Acquires the next frame, lets `draw` render into it (given the view, its
    /// format and its physical size), then presents it. Skips the frame when the
    /// window is minimized, occluded or the surface needs reconfiguring.
    pub fn present_with(
        &mut self,
        gpu: &Gpu,
        draw: impl FnOnce(&wgpu::TextureView, wgpu::TextureFormat, (u32, u32)),
    ) {
        if !self.configured {
            return;
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                // Draw this frame, reconfigure before the next one.
                self.configured = false;
                f
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&gpu.device, &self.config);
                return;
            }
            other => {
                tracing::debug!(?other, "skipping frame");
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        draw(&view, self.config.format, self.size());
        gpu.queue.present(frame);
        if !self.configured {
            self.surface.configure(&gpu.device, &self.config);
            self.configured = true;
        }
    }
}

/// Frames the swapchain may queue. One shows resizes and input soonest, but on DX12
/// it made live resizing jitter (checked by hand on an RTX 5080), so DX12 queues two.
/// `PLATYPUS_FRAME_LATENCY` (1 or 2) overrides it for comparing on other machines; more
/// would only add input lag.
fn frame_latency(backend: wgpu::Backend) -> u32 {
    let default = if backend == wgpu::Backend::Dx12 { 2 } else { 1 };
    std::env::var("PLATYPUS_FRAME_LATENCY")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|n| (1..=2).contains(n))
        .unwrap_or(default)
}
