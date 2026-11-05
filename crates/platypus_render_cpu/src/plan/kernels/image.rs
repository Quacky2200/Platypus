// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! **Mode 4, Image: sampled pictures.** Background and border images (a `Step::Draw` with an image,
//! the GLES `IMAGE_MAIN`): `rect` and `radii` the box (the clip shape), `p0` one tile of the image in
//! logical px, `params.yz` whether it repeats along x and y, `p1.xy` the gaps of
//! `background-repeat: space`, `color0` a border-image piece's part of the texture, `color1` a tint.
//!
//! Sampling is GL's for an sRGB texture: texels decoded to linear light (a table), filtered there
//! (bilinear, or nearest for `image-rendering: pixelated`), wrapping by repeating, then encoded back
//! for a canvas that holds sRGB. Decoded images stay straight-alpha sRGB bytes as `ImageCache` made
//! them, so an image costs its pixels once, as the GLES texture did.

use platypus_render::plan::Instance;

use super::super::color::{decode_table, encode};
use super::super::geometry::{Frame, clip_coverage, shape_coverage};

/// A decoded image: straight-alpha sRGB, RGBA8, top row first.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub px: Vec<u8>,
}

impl Image {
    /// The texel at (`x`, `y`), wrapped (GL's `REPEAT`), decoded: linear rgb, alpha.
    #[inline]
    fn texel(&self, x: i64, y: i64) -> [f32; 4] {
        let (w, h) = (i64::from(self.width), i64::from(self.height));
        let (x, y) = (x.rem_euclid(w) as usize, y.rem_euclid(h) as usize);
        let at = (y * self.width as usize + x) * 4;
        let p = &self.px[at..at + 4];
        let d = decode_table();
        [
            d[usize::from(p[0])],
            d[usize::from(p[1])],
            d[usize::from(p[2])],
            f32::from(p[3]) / 255.0,
        ]
    }

    /// `textureLod(uv)`: straight linear rgb and alpha.
    #[inline]
    pub fn sample(&self, uv: [f32; 2], nearest: bool) -> [f32; 4] {
        let (w, h) = (self.width as f32, self.height as f32);
        if nearest {
            return self.texel((uv[0] * w).floor() as i64, (uv[1] * h).floor() as i64);
        }
        let (x, y) = (uv[0] * w - 0.5, uv[1] * h - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let a = self.texel(x0, y0);
        let b = self.texel(x0 + 1, y0);
        let c = self.texel(x0, y0 + 1);
        let d = self.texel(x0 + 1, y0 + 1);
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }
}

/// `background-image-tint` / `border-image-tint` (premultiplied); all zeros means none.
#[inline]
fn tint(t: [f32; 4]) -> [f32; 4] {
    if t == [0.0; 4] { [1.0; 4] } else { t }
}

#[inline]
fn fract(v: f32) -> f32 {
    v - v.floor()
}

/// The image's colour at `world` (window, logical px) whose point in the instance's plane is `p`,
/// or `None` where the shader discards.
#[inline]
pub fn image(
    f: &Frame,
    img: &Image,
    nearest: bool,
    i: &Instance,
    world: [f32; 2],
    p: [f32; 2],
) -> Option<[f32; 4]> {
    let clip = shape_coverage(p, i.rect, i.radii, i.radii_y, f.scale)
        * clip_coverage(f, world, i.params[3] as u32);
    if clip <= 0.0 {
        return None;
    }
    let tile = i.p0;
    let mut uv = [(p[0] - tile[0]) / tile[2], (p[1] - tile[1]) / tile[3]];
    let (repeat_x, repeat_y) = (i.params[1] > 0.5, i.params[2] > 0.5);
    // `background-repeat: space`: tiles are `p1.xy` apart; nothing shows in the gaps.
    let gap = [i.p1[0], i.p1[1]];
    let period = [tile[2] + gap[0], tile[3] + gap[1]];
    let local = [p[0] - tile[0], p[1] - tile[1]];
    let wrapped = [
        local[0] - (local[0] / period[0]).floor() * period[0],
        local[1] - (local[1] / period[1]).floor() * period[1],
    ];
    let (gap_x, gap_y) = (repeat_x && gap[0] > 0.0, repeat_y && gap[1] > 0.0);
    if (gap_x && wrapped[0] > tile[2]) || (gap_y && wrapped[1] > tile[3]) {
        return None;
    }
    if gap_x {
        uv[0] = wrapped[0] / tile[2];
    }
    if gap_y {
        uv[1] = wrapped[1] / tile[3];
    }
    if (!repeat_x && !(0.0..=1.0).contains(&uv[0])) || (!repeat_y && !(0.0..=1.0).contains(&uv[1]))
    {
        return None;
    }
    let half = [0.5 / img.width as f32, 0.5 / img.height as f32];
    let at = if i.color0[2] > 0.0 {
        // A border-image piece: `color0` is its part of the texture (uv). Wrap within the piece and
        // keep filtering inside it.
        let sub = i.color0;
        let t = [
            if repeat_x { fract(uv[0]) } else { uv[0] },
            if repeat_y { fract(uv[1]) } else { uv[1] },
        ];
        [
            (sub[0] + t[0] * sub[2]).clamp(sub[0] + half[0], sub[0] + sub[2] - half[0]),
            (sub[1] + t[1] * sub[3]).clamp(sub[1] + half[1], sub[1] + sub[3] - half[1]),
        ]
    } else {
        // The texture repeats; on axes that don't, keep bilinear filtering from wrapping round.
        [
            if repeat_x {
                uv[0]
            } else {
                uv[0].clamp(half[0], 1.0 - half[0])
            },
            if repeat_y {
                uv[1]
            } else {
                uv[1].clamp(half[1], 1.0 - half[1])
            },
        ]
    };
    let c = img.sample(at, nearest);
    let rgb = |v: f32| if f.srgb { encode(v) } else { v };
    let t = tint(i.color1);
    let k = c[3] * clip;
    Some([
        rgb(c[0]) * k * t[0],
        rgb(c[1]) * k * t[1],
        rgb(c[2]) * k * t[2],
        c[3] * clip * t[3],
    ])
}
