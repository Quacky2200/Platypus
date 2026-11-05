// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The frame's canvases: premultiplied RGBA8, top row first, as the GLES executor's textures.
//!
//! With `Blending::Srgb` (the default) a canvas holds sRGB values and blends them as they are (GL's
//! `RGBA8`). With `Blending::Linear` it holds linear values stored sRGB-encoded (GL's `SRGB8_ALPHA8`):
//! reads decode, writes encode. [`Space`] does that for every kernel.
//!
//! A canvas remembers the rectangle anything was written in since it was last cleared, so clearing a
//! canvas nothing touched costs nothing, and clearing one a layer used costs the layer's area: the
//! GLES executor clears whole frame-sized textures.

use web_ui_render::Color;
use web_ui_render::plan::PxRect;

use super::color::{decode_table, encode, to_byte};

/// How a canvas's bytes map to the colours the kernels compute with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// sRGB values, as they are.
    Srgb,
    /// Linear values, stored sRGB-encoded.
    Linear,
}

impl Space {
    /// A stored pixel as the premultiplied colour the shaders see (`texelFetch`).
    #[inline]
    pub fn load(self, p: [u8; 4]) -> [f32; 4] {
        match self {
            Space::Srgb => p.map(|c| f32::from(c) / 255.0),
            Space::Linear => {
                let d = decode_table();
                [
                    d[usize::from(p[0])],
                    d[usize::from(p[1])],
                    d[usize::from(p[2])],
                    f32::from(p[3]) / 255.0,
                ]
            }
        }
    }

    /// A premultiplied colour as stored.
    #[inline]
    pub fn store(self, c: [f32; 4]) -> [u8; 4] {
        match self {
            Space::Srgb => c.map(to_byte),
            Space::Linear => [
                to_byte(encode(c[0])),
                to_byte(encode(c[1])),
                to_byte(encode(c[2])),
                to_byte(c[3]),
            ],
        }
    }
}

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub px: Vec<u8>,
    /// Where anything may have been written since the last clear (`None`: all transparent).
    dirty: Option<PxRect>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Canvas {
        Canvas {
            width,
            height,
            // Zeroed by the allocator: pages nothing writes are never touched.
            px: vec![0; width as usize * height as usize * 4],
            dirty: None,
        }
    }

    pub fn full(&self) -> PxRect {
        PxRect {
            x: 0,
            y: 0,
            w: self.width,
            h: self.height,
        }
    }

    /// Notes that `r` may have been written.
    pub fn touch(&mut self, r: PxRect) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        self.dirty = Some(match self.dirty {
            None => r,
            Some(d) => union(d, r),
        });
    }

    /// Fills the canvas with `color` (premultiplied, in the canvas's space, as GL's clear).
    pub fn clear(&mut self, color: Color, space: Space) {
        let c = space.store([
            color.r as f32,
            color.g as f32,
            color.b as f32,
            color.a as f32,
        ]);
        if c == [0; 4] {
            if let Some(d) = self.dirty.take() {
                let stride = self.width as usize * 4;
                for y in d.y..d.y + d.h {
                    let row = y as usize * stride;
                    self.px[row + d.x as usize * 4..row + (d.x + d.w) as usize * 4].fill(0);
                }
            }
            return;
        }
        self.px.as_chunks_mut::<4>().0.fill(c);
        self.dirty = Some(self.full());
    }

    /// Copies `src` (the same size) into this canvas.
    pub fn copy_from(&mut self, src: &Canvas) {
        self.px.copy_from_slice(&src.px);
        self.dirty = src.dirty;
    }

    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    /// The canvas's rows, for kernels that write disjoint bands from several threads.
    pub fn rows(&mut self) -> Rows {
        Rows {
            ptr: self.px.as_mut_ptr(),
            stride: self.width as usize * 4,
            height: self.height,
        }
    }
}

/// The smallest rectangle holding both.
pub fn union(a: PxRect, b: PxRect) -> PxRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = (a.x + a.w).max(b.x + b.w);
    let bottom = (a.y + a.h).max(b.y + b.h);
    PxRect {
        x,
        y,
        w: r - x,
        h: bottom - y,
    }
}

/// A canvas's pixels shared between threads that each write their own rows.
#[derive(Clone, Copy)]
pub struct Rows {
    ptr: *mut u8,
    stride: usize,
    height: u32,
}

// SAFETY: a `Rows` only hands out rows; callers give each thread its own rows (`row`'s contract).
unsafe impl Send for Rows {}
// SAFETY: as above.
unsafe impl Sync for Rows {}

impl Rows {
    /// Row `y`'s bytes.
    ///
    /// # Safety
    /// No other thread may access row `y` while the slice lives, and the canvas must outlive it.
    #[allow(clippy::mut_from_ref)] // The point of the type: disjoint rows from shared handles.
    pub unsafe fn row(&self, y: u32) -> &mut [u8] {
        debug_assert!(y < self.height);
        // SAFETY: in bounds (`y < height`); exclusivity is the caller's.
        unsafe {
            std::slice::from_raw_parts_mut(self.ptr.add(y as usize * self.stride), self.stride)
        }
    }
}

/// Premultiplied source-over of `c` onto the stored pixel `p` (GL's `ONE, ONE_MINUS_SRC_ALPHA` for
/// colour and alpha alike).
#[inline]
pub fn blend_over(p: &mut [u8], c: [f32; 4], space: Space) {
    if c[3] <= 0.0 && c[0] <= 0.0 && c[1] <= 0.0 && c[2] <= 0.0 {
        return;
    }
    let d = space.load([p[0], p[1], p[2], p[3]]);
    let k = 1.0 - c[3];
    let out = space.store([
        c[0] + d[0] * k,
        c[1] + d[1] * k,
        c[2] + d[2] * k,
        c[3] + d[3] * k,
    ]);
    p.copy_from_slice(&out);
}
