// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! **Mode 3, Glyph: text from the glyph atlas.** `KIND_GLYPH`: `rect` the destination (logical px,
//! pixel-aligned in physical px), `p0` the glyph's place in the atlas, `params.y` 1 for a colour glyph
//! (emoji, from the colour page) and 2 for `font-smooth: never`, `params.z` a gradient for
//! `background-clip: text`, `color0` the text colour. As the GLES shader's `glyph`.
//!
//! Glyphs are copies, not shapes: no anti-aliasing beyond the mask's own, one atlas texel per
//! physical pixel, so the kernel is a lookup and a multiply per pixel of a small rectangle. The masks
//! were rasterised once on the CPU (`GlyphCache`, swash) for both executors; here they are read where
//! they are, with no upload.

use web_ui_render::plan::Instance;

use super::super::geometry::Frame;
use super::super::gradient::gradient_color;
use super::Textures;

/// Coverage adjustment for text: blending in linear light makes dark text on light backgrounds look
/// thin, so dark text gets more coverage there. Blending in sRGB needs none.
#[inline]
fn text_coverage(a: f32, color: [f32; 4], srgb: bool) -> f32 {
    if srgb {
        return a;
    }
    let al = color[3].max(1e-4);
    let luma = (color[0] * 0.2126 + color[1] * 0.7152 + color[2] * 0.0722) / al;
    let t = ((luma - 0.1) / 0.5).clamp(0.0, 1.0);
    let dark = 1.0 - t * t * (3.0 - 2.0 * t);
    a + (a.powf(1.0 / 1.6) - a) * dark
}

/// The glyph's colour at `p` (logical px, its plane), or `None` outside its image (the shader's
/// `discard`).
#[inline]
pub fn glyph(f: &Frame, t: &Textures, i: &Instance, p: [f32; 2]) -> Option<[f32; 4]> {
    let s = f.scale;
    let origin = [(i.rect[0] * s).round(), (i.rect[1] * s).round()];
    let local = [
        (p[0] * s).floor() - origin[0],
        (p[1] * s).floor() - origin[1],
    ];
    let size = [i.p0[2], i.p0[3]];
    if local[0] < 0.0 || local[1] < 0.0 || local[0] >= size[0] || local[1] >= size[1] {
        return None;
    }
    let (tx, ty) = ((i.p0[0] + local[0]) as usize, (i.p0[1] + local[1]) as usize);
    let kind = i.params[1];
    if kind > 0.5 && kind < 1.5 {
        // Colour glyph: straight sRGB on the colour page (the GLES texture decodes and the shader
        // re-encodes, which cancels on an sRGB canvas).
        let page = t.glyph_color.as_deref()?;
        let at = (ty * web_ui_render::glyphs::COLOR_SIZE as usize + tx) * 4;
        let c = &page[at..at + 4];
        let a = f32::from(c[3]) / 255.0;
        let rgb = |v: u8| {
            let v = f32::from(v) / 255.0;
            if f.srgb {
                v
            } else {
                super::super::color::srgb_to_linear(v)
            }
        };
        let k = i.color0[3];
        return Some([
            rgb(c[0]) * a * k,
            rgb(c[1]) * a * k,
            rgb(c[2]) * a * k,
            a * k,
        ]);
    }
    let a = f32::from(t.glyph_mask[ty * web_ui_render::glyphs::MASK_SIZE as usize + tx]) / 255.0;
    if kind > 1.5 {
        // `font-smooth: never`: hard edges, no coverage boost.
        let crisp = if a >= 0.5 { 1.0 } else { 0.0 };
        return Some(i.color0.map(|c| c * crisp));
    }
    if i.params[2] >= 0.0
        && let Some(g) = f.gradients.get(i.params[2] as usize)
    {
        // `background-clip: text`: the gradient over the element's box (`p1`).
        let c = gradient_color(g, p, i.p1, f.srgb);
        let k = text_coverage(a, c, f.srgb);
        return Some(c.map(|v| v * k));
    }
    let k = text_coverage(a, i.color0, f.srgb);
    Some(i.color0.map(|v| v * k))
}
