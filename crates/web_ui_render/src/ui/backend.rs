// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What the app needs from a UI renderer, whichever draws it.
//!
//! A backend draws `platypus_ui` draw lists ([`DrawCmd`]) into a target. The draw list and the
//! frame plan built from it (`plan.rs`) are backend-neutral: a backend owns only the GPU
//! side, the resources a plan names (glyph atlas pages, images, offscreen textures) and how
//! its [`plan::Step`](super::plan::Step)s run. The wgpu renderer is [`UiRenderer`](super::UiRenderer);
//! a GLES renderer (through ANGLE on Windows) is to be the second.

use std::path::PathBuf;

use platypus_ui::DrawCmd;

use super::{Blending, RenderStats};
use crate::Color;

pub trait UiBackend {
    /// The device a renderer draws with (wgpu: [`Gpu`](crate::Gpu); GLES: its context).
    type Context;
    /// Where a frame is drawn (wgpu: a texture view with its format, size and scale).
    type Target<'a>;

    /// The folder `url(...)` images are read from (the game folder).
    fn set_asset_root(&mut self, root: PathBuf);

    /// How translucent colours combine from the next frame on (sRGB by default).
    fn set_blending(&mut self, blending: Blending);

    /// Loads the image at `url` and returns its natural size in pixels, or `None` if it can't
    /// be loaded (layout needs it).
    fn image_size(&mut self, cx: &Self::Context, url: &str) -> Option<(u32, u32)>;

    /// Draws `cmds` over `clear` into `target`.
    fn render(
        &mut self,
        cx: &Self::Context,
        target: &Self::Target<'_>,
        clear: Color,
        cmds: &[DrawCmd],
    ) -> RenderStats;

    /// Draws `cmds` over what `target` already holds (screen UI over the 3D world).
    fn render_overlay(
        &mut self,
        cx: &Self::Context,
        target: &Self::Target<'_>,
        cmds: &[DrawCmd],
    ) -> RenderStats;
}
