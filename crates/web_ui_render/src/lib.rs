// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! web-ui's renderer over wgpu: device setup, window surfaces, offscreen targets and the UI
//! renderer (`ui`: the draw list to a `FramePlan`, executed by `UiRenderer`).
//!
//! This crate doesn't depend on winit; anything wgpu accepts as a surface target
//! (such as `Arc<winit::window::Window>`) works.

mod gpu;
mod offscreen;
mod surface;
pub mod testing;
pub mod ui;

pub use gpu::{Gpu, RenderError, set_low_memory};
pub use offscreen::OffscreenTarget;
pub use surface::WindowSurface;
pub use ui::backend::UiBackend;
pub use ui::{
    Blending, CANVAS_FORMAT, RenderStats, RenderTarget, UiRenderer, canvas_format, glyphs,
    image_cache, needs_backdrop, plan,
};
pub use wgpu;

/// A linear RGBA colour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl Color {
    pub const TRANSPARENT: Color = Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
    };

    pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b, a: 1.0 }
    }
}

impl From<Color> for wgpu::Color {
    fn from(c: Color) -> Self {
        wgpu::Color {
            r: c.r,
            g: c.g,
            b: c.b,
            a: c.a,
        }
    }
}

/// Records a render pass that only clears `view` to `color`.
pub(crate) fn encode_clear(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    color: Color,
) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("platypus clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color.into()),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
}
