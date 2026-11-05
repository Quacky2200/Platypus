// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! **Mode 6, Composite: a layer onto what is beneath it** (`Step::Composite`: opacity groups,
//! `filter` colour functions, drop shadows, masks, `mix-blend-mode`), as the GLES `effects.rs` composite
//! programs: the layer's pixel (shifted for a drop shadow, then tinted, then through the colour matrix
//! in straight-alpha sRGB), times opacity, mask and clip; then source-over, or a W3C blend mode with
//! a copy of the parent, which replaces the parent's pixels.
//!
//! The fast path is the plain opacity group: no shift, tint, matrix or mask and a clip that holds the
//! whole area, which is one scaled source-over per pixel (`spans::over_scaled`).

use platypus_ui::{ColorMatrix, MaskMode};
use web_ui_render::plan::PxRect;

use super::super::canvas::{Canvas, Rows, Space, blend_over};
use super::super::color::{encode, srgb_to_linear};
use super::super::geometry::{Frame, clip_coverage, clip_holds, clip_is_convex};
use super::spans;

/// What one composite pass reads.
pub struct Composite<'a> {
    pub frame: &'a Frame<'a>,
    pub space: Space,
    pub src: &'a Canvas,
    /// A blend mode other than normal, with the copy of the parent it blends with.
    pub blend: Option<(u32, &'a Canvas)>,
    pub mask: Option<(&'a Canvas, MaskMode)>,
    pub area: PxRect,
    pub opacity: f32,
    pub clip: u32,
    pub matrix: Option<ColorMatrix>,
    pub tint: Option<[f32; 4]>,
    pub shift: [f32; 2],
}

/// How a composite's rows are drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Path {
    /// A scaled source-over of the layer, a run at a time.
    Plain,
    /// The same inside a convex clip: the clip's edge pixel by pixel, the run inside it at a time.
    PlainClipped,
    /// Everything per pixel (tint, shift, matrix, mask, blend modes, polygon clips).
    General,
}

impl Composite<'_> {
    fn path(&self) -> Path {
        let s = self.frame.scale;
        let a = self.area;
        let logical = [
            a.x as f32 / s,
            a.y as f32 / s,
            (a.x + a.w) as f32 / s,
            (a.y + a.h) as f32 / s,
        ];
        let plain = self.blend.is_none()
            && self.mask.is_none()
            && self.matrix.is_none()
            && self.tint.is_none()
            && self.shift == [0.0, 0.0];
        if !plain {
            Path::General
        } else if self.clip == 0 || clip_holds(self.frame.clips, self.clip, logical, s) {
            Path::Plain
        } else if clip_is_convex(self.frame.clips, self.clip) {
            Path::PlainClipped
        } else {
            Path::General
        }
    }

    /// The clip's coverage at pixel (`x`, `y`).
    #[inline]
    fn clip_at(&self, x: u32, y: u32) -> f32 {
        let s = self.frame.scale;
        clip_coverage(
            self.frame,
            [(x as f32 + 0.5) / s, (y as f32 + 0.5) / s],
            self.clip,
        )
    }

    /// Composites the area's rows `y0..y1` into `dst`.
    ///
    /// # Safety
    /// No other thread may touch `dst`'s rows `y0..y1` meanwhile (the bands are disjoint).
    pub unsafe fn rows(&self, dst: Rows, y0: u32, y1: u32) {
        let a = self.area;
        let path = self.path();
        for y in y0..y1 {
            // SAFETY: this band's own row (the caller's contract).
            let row = unsafe { dst.row(y) };
            let src_row = &self.src.px[(y as usize * self.src.width as usize) * 4..]
                [..self.src.width as usize * 4];
            let (mut left, mut right) = (a.x, a.x + a.w);
            if path == Path::PlainClipped {
                // In from both ends to the run the clip covers whole (one run: the clip is convex).
                while left < right && self.clip_at(left, y) < 1.0 {
                    let at = left as usize * 4;
                    self.pixel(&mut row[at..at + 4], left, y);
                    left += 1;
                }
                while right > left && self.clip_at(right - 1, y) < 1.0 {
                    let at = (right - 1) as usize * 4;
                    self.pixel(&mut row[at..at + 4], right - 1, y);
                    right -= 1;
                }
            }
            let (l, r) = (left as usize * 4, right as usize * 4);
            if path != Path::General {
                spans::over_scaled(&mut row[l..r], &src_row[l..r], self.opacity, self.space);
                continue;
            }
            for (k, px) in row[l..r].as_chunks_mut::<4>().0.iter_mut().enumerate() {
                self.pixel(px, left + k as u32, y);
            }
        }
    }

    fn pixel(&self, px: &mut [u8], x: u32, y: u32) {
        let f = self.frame;
        let frag = [x as f32 + 0.5, y as f32 + 0.5];
        let clip = clip_coverage(f, [frag[0] / f.scale, frag[1] / f.scale], self.clip);
        if clip <= 0.0 && self.blend.is_none() {
            // Source-over of nothing.
            return;
        }
        let s = self
            .filtered(frag)
            .map(|c| c * self.opacity * self.mask_at(x, y));
        match self.blend {
            None => blend_over(px, s.map(|c| c * clip), self.space),
            Some((mode, backdrop)) => {
                let b = self.space.load(backdrop.pixel(x, y));
                let out = blend_pixel(mode, s, b, f.srgb);
                let mixed: [f32; 4] = std::array::from_fn(|k| b[k] + (out[k] - b[k]) * clip);
                px.copy_from_slice(&self.space.store(mixed));
            }
        }
    }

    /// The layer's texel at plan point `q` (canvas filtering: bilinear, clamped to the edge).
    fn sample(&self, q: [f32; 2]) -> [f32; 4] {
        let src = self.src;
        let (x, y) = (q[0] - 0.5, q[1] - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |dx: i64, dy: i64| {
            let cx = (x0 as i64 + dx).clamp(0, i64::from(src.width) - 1) as u32;
            let cy = (y0 as i64 + dy).clamp(0, i64::from(src.height) - 1) as u32;
            self.space.load(src.pixel(cx, cy))
        };
        if fx == 0.0 && fy == 0.0 {
            return at(0, 0);
        }
        let (a, b, c, d) = (at(0, 0), at(1, 0), at(0, 1), at(1, 1));
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }

    /// The layer's pixel after its drop-shadow tint and colour matrix.
    fn filtered(&self, frag: [f32; 2]) -> [f32; 4] {
        let mut c = self.sample([frag[0] - self.shift[0], frag[1] - self.shift[1]]);
        if let Some(t) = self.tint {
            c = t.map(|v| v * c[3]);
        }
        if let Some(m) = self.matrix {
            // CSS filter functions work on straight-alpha sRGB, like browsers.
            let srgb = self.frame.srgb;
            let straight = straight(c);
            let v = [
                to_srgb(straight[0], srgb),
                to_srgb(straight[1], srgb),
                to_srgb(straight[2], srgb),
                c[3],
            ];
            let r: [f32; 4] = std::array::from_fn(|row| {
                (m[row * 5] * v[0]
                    + m[row * 5 + 1] * v[1]
                    + m[row * 5 + 2] * v[2]
                    + m[row * 5 + 3] * v[3]
                    + m[row * 5 + 4])
                    .clamp(0.0, 1.0)
            });
            c = [
                from_srgb(r[0], srgb) * r[3],
                from_srgb(r[1], srgb) * r[3],
                from_srgb(r[2], srgb) * r[3],
                r[3],
            ];
        }
        c
    }

    /// How much of the layer shows at the pixel (`mask-image`).
    fn mask_at(&self, x: u32, y: u32) -> f32 {
        let Some((mask, mode)) = self.mask else {
            return 1.0;
        };
        let m = self.space.load(mask.pixel(x, y));
        match mode {
            MaskMode::Alpha => m[3],
            MaskMode::Luminance => {
                // Luminance of the (premultiplied) mask colour, in sRGB like CSS.
                let srgb = self.frame.srgb;
                let s = straight(m);
                (to_srgb(s[0], srgb) * 0.2126
                    + to_srgb(s[1], srgb) * 0.7152
                    + to_srgb(s[2], srgb) * 0.0722)
                    * m[3]
            }
        }
    }
}

#[inline]
fn straight(c: [f32; 4]) -> [f32; 3] {
    if c[3] > 0.0 {
        [c[0] / c[3], c[1] / c[3], c[2] / c[3]]
    } else {
        [0.0; 3]
    }
}

/// A canvas value as sRGB (identity on an sRGB canvas), and back.
#[inline]
fn to_srgb(c: f32, srgb_canvas: bool) -> f32 {
    if srgb_canvas { c } else { encode(c) }
}

#[inline]
fn from_srgb(c: f32, srgb_canvas: bool) -> f32 {
    if srgb_canvas { c } else { srgb_to_linear(c) }
}

/// W3C compositing of source `s` over backdrop `b` (premultiplied, canvas space) with blend `mode`,
/// in sRGB like browsers.
fn blend_pixel(mode: u32, s: [f32; 4], b: [f32; 4], srgb: bool) -> [f32; 4] {
    let conv = |c: [f32; 4]| {
        let a = c[3].max(1e-6);
        [
            to_srgb(c[0] / a, srgb),
            to_srgb(c[1] / a, srgb),
            to_srgb(c[2] / a, srgb),
        ]
    };
    let (cs, cb) = (conv(s), conv(b));
    let mixed = blend(mode, cb, cs).map(|v| v.clamp(0.0, 1.0));
    let a = s[3] + b[3] * (1.0 - s[3]);
    let c: [f32; 3] = std::array::from_fn(|k| {
        s[3] * (1.0 - b[3]) * cs[k] + b[3] * (1.0 - s[3]) * cb[k] + s[3] * b[3] * mixed[k]
    });
    if a <= 0.0 {
        return [0.0; 4];
    }
    [
        from_srgb(c[0] / a, srgb) * a,
        from_srgb(c[1] / a, srgb) * a,
        from_srgb(c[2] / a, srgb) * a,
        a,
    ]
}

fn per<F: Fn(f32, f32) -> f32>(cb: [f32; 3], cs: [f32; 3], f: F) -> [f32; 3] {
    std::array::from_fn(|k| f(cb[k], cs[k]))
}

fn hard_light(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    per(cb, cs, |b, s| {
        if s <= 0.5 {
            b * 2.0 * s
        } else {
            let t = 2.0 * s - 1.0;
            b + t - b * t
        }
    })
}

fn soft_light(cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    per(cb, cs, |b, s| {
        let d = if b <= 0.25 {
            ((16.0 * b - 12.0) * b + 4.0) * b
        } else {
            b.sqrt()
        };
        if s <= 0.5 {
            b - (1.0 - 2.0 * s) * b * (1.0 - b)
        } else {
            b + (2.0 * s - 1.0) * (d - b)
        }
    })
}

fn lum(c: [f32; 3]) -> f32 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}

fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut r = c;
    if n < 0.0 {
        r = r.map(|v| l + (v - l) * l / (l - n).max(1e-6));
    }
    if x > 1.0 {
        r = r.map(|v| l + (v - l) * (1.0 - l) / (x - l).max(1e-6));
    }
    r
}

fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// The spec's SetSat: the largest channel becomes `s`, the smallest 0, the middle one in between.
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let n = c[0].min(c[1]).min(c[2]);
    let range = sat(c);
    if range <= 0.0 {
        return [0.0; 3];
    }
    c.map(|v| (v - n) * s / range)
}

fn blend(mode: u32, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    match mode {
        1 => per(cb, cs, |b, s| b * s),
        2 => per(cb, cs, |b, s| b + s - b * s),
        3 => hard_light(cs, cb),
        4 => per(cb, cs, f32::min),
        5 => per(cb, cs, f32::max),
        6 => per(cb, cs, |b, s| {
            if b <= 0.0 {
                0.0
            } else if s >= 1.0 {
                1.0
            } else {
                (b / (1.0 - s).max(1e-6)).min(1.0)
            }
        }),
        7 => per(cb, cs, |b, s| {
            if b >= 1.0 {
                1.0
            } else if s <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - b) / s.max(1e-6)).min(1.0)
            }
        }),
        8 => hard_light(cb, cs),
        9 => soft_light(cb, cs),
        10 => per(cb, cs, |b, s| (b - s).abs()),
        11 => per(cb, cs, |b, s| b + s - 2.0 * b * s),
        12 => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        13 => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        14 => set_lum(cs, lum(cb)),
        15 => set_lum(cb, lum(cs)),
        _ => cs,
    }
}
