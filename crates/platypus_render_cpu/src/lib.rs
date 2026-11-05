// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! CPU renderers for the UI draw list: no GPU API, no GPU driver, no shader compiler.
//!
//! - [`plan`]: a third executor of `web_ui_render`'s frame plan (beside the wgpu and GLES ones),
//!   drawing it with seven kernel families compiled ahead of time (`plan::kernels`). It draws what
//!   the GLES executor draws, the same way, so a host can choose it instead of GLES on a machine
//!   whose GL is a software rasteriser (llvmpipe, softpipe), without loading Mesa.
//! - The vello renderer (feature `vello`, on by default): the draw list through `vello_cpu`, shown
//!   through `softbuffer`, for `platypus-app`'s `renderer: "cpu"`.

pub mod plan;

#[cfg(feature = "vello")]
mod blur;
#[cfg(feature = "vello")]
mod images;
#[cfg(feature = "vello")]
mod paint;
#[cfg(feature = "vello")]
mod vello;

#[cfg(feature = "vello")]
pub use vello::*;
