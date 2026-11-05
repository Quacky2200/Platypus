// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! **Mode 1, Fill: shapes with a colour or a gradient.** `KIND_QUAD` (a box: background colour, an
//! optional gradient layer over it, then a solid border), `KIND_BORDER` (styled borders: dashed,
//! dotted, double, groove, ridge, inset, outset), `KIND_WAVY` (a wavy text decoration) and
//! `KIND_GRADIENT_PIECE` (a border-image piece cut from a gradient). Ported line for line from the
//! GLES shader's `shade`.
//!
//! The fast path is the box with no gradient: inside its inner edge (and its clip) every pixel is the
//! background colour, so [`quad`] says so and the raster loop fills that run of the row in one go
//! (`spans::over`); only the anti-aliased edge and the border are shaded pixel by pixel.

use platypus_render::plan::Instance;

use super::super::geometry::{Frame, coverage, shape_coverage};
use super::super::gradient::gradient_color;

/// Inner edge radii, as CSS: horizontal radii lose the left/right width, vertical ones the
/// top/bottom (`b` is top, right, bottom, left).
fn inner_rx(rx: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        (rx[0] - b[3]).max(0.0),
        (rx[1] - b[1]).max(0.0),
        (rx[2] - b[1]).max(0.0),
        (rx[3] - b[3]).max(0.0),
    ]
}

fn inner_ry(ry: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        (ry[0] - b[0]).max(0.0),
        (ry[1] - b[0]).max(0.0),
        (ry[2] - b[2]).max(0.0),
        (ry[3] - b[2]).max(0.0),
    ]
}

/// The box inside the border widths `b` (top, right, bottom, left).
fn inner_rect(r: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        r[0] + b[3],
        r[1] + b[0],
        r[2] - b[3] - b[1],
        r[3] - b[0] - b[2],
    ]
}

#[inline]
fn scaled(c: [f32; 4], k: f32) -> [f32; 4] {
    c.map(|v| v * k)
}

/// A box (`KIND_QUAD`) at `p` (its plane, logical px): the colour, and whether `p` is fully inside
/// its inner edge, where the colour is just its background ([`background`]).
#[inline]
pub fn quad(f: &Frame, i: &Instance, p: [f32; 2]) -> ([f32; 4], bool) {
    let s = f.scale;
    let outer = shape_coverage(p, i.rect, i.radii, i.radii_y, s);
    if outer <= 0.0 {
        return ([0.0; 4], false);
    }
    let b = i.p0;
    let inner = shape_coverage(
        p,
        inner_rect(i.rect, b),
        inner_rx(i.radii, b),
        inner_ry(i.radii_y, b),
        s,
    )
    .min(outer);
    let fill = background(f, i, p);
    // CSS backgrounds extend under the border (background-clip: border-box).
    let base = scaled(fill, outer);
    let border = scaled(i.color1, outer - inner);
    let out = std::array::from_fn(|k| border[k] + base[k] * (1.0 - border[3]));
    (out, inner >= 1.0)
}

/// A box's background at `p`: its colour (`color0`), under its gradient layer if it has one. Inside the
/// inner edge this is the whole of [`quad`]'s colour, without the shape's distances.
#[inline]
pub fn background(f: &Frame, i: &Instance, p: [f32; 2]) -> [f32; 4] {
    let mut fill = i.color0;
    let gradient = i.params[2];
    if gradient >= 0.0 {
        // A background gradient layer: `p1` is one tile, repeating along x with bit 1 of
        // `params.y` and along y with bit 2.
        let mut at = p;
        let mut tile = i.rect;
        let mut inside = true;
        if i.p1[2] > 0.0 && i.p1[3] > 0.0 {
            tile = i.p1;
            let bits = i.params[1] as u32;
            let local = [p[0] - tile[0], p[1] - tile[1]];
            let wrapped = [
                local[0] - (local[0] / tile[2]).floor() * tile[2],
                local[1] - (local[1] / tile[3]).floor() * tile[3],
            ];
            let (rx, ry) = (bits & 1 != 0, bits & 2 != 0);
            at = [
                tile[0] + if rx { wrapped[0] } else { local[0] },
                tile[1] + if ry { wrapped[1] } else { local[1] },
            ];
            inside = (rx || (local[0] >= 0.0 && local[0] <= tile[2]))
                && (ry || (local[1] >= 0.0 && local[1] <= tile[3]));
        }
        if inside && let Some(g) = f.gradients.get(gradient as usize) {
            let g = gradient_color(g, at, tile, f.srgb);
            fill = std::array::from_fn(|k| g[k] + fill[k] * (1.0 - g[3]));
        }
    }
    fill
}

/// A styled border (`KIND_BORDER`): `p0` the widths, gradient entry `params.z` the side colours and
/// styles.
pub fn styled_border(f: &Frame, i: &Instance, p: [f32; 2]) -> [f32; 4] {
    let s = f.scale;
    let (r, b) = (i.rect, i.p0);
    let outer = shape_coverage(p, r, i.radii, i.radii_y, s);
    let inner = shape_coverage(
        p,
        inner_rect(r, b),
        inner_rx(i.radii, b),
        inner_ry(i.radii_y, b),
        s,
    );
    let ring = outer - inner.min(outer);
    if ring <= 0.0 {
        return [0.0; 4];
    }
    let Some(entry) = f.gradients.get(i.params[2] as usize) else {
        return [0.0; 4];
    };
    // Which side: the nearest outer edge relative to its width, so corners split along the line
    // from the outer to the inner corner, like CSS.
    let d = [
        p[1] - r[1],
        r[0] + r[2] - p[0],
        r[1] + r[3] - p[1],
        p[0] - r[0],
    ];
    let n: [f32; 4] = std::array::from_fn(|k| {
        if b[k] > 0.0 {
            d[k] / b[k].max(1e-4)
        } else {
            1e9
        }
    });
    let mut side = 0;
    let mut best = n[0];
    for (k, v) in n.iter().enumerate().skip(1) {
        if *v < best {
            side = k;
            best = *v;
        }
    }
    let w = b[side];
    let t = d[side];
    let (along, len) = match side {
        0 => (p[0] - r[0], r[2]),
        1 => (p[1] - r[1], r[3]),
        2 => (r[0] + r[2] - p[0], r[2]),
        _ => (r[1] + r[3] - p[1], r[3]),
    };
    let style = entry.info[side] as u32;
    let mut color = entry.colors[side];
    // Half as bright in sRGB (0.22 in linear light).
    let dim = if f.srgb { 0.5 } else { 0.22 };
    let dark = [color[0] * dim, color[1] * dim, color[2] * dim, color[3]];
    let top_left = side == 0 || side == 3;
    let outer_half = t < w * 0.5;
    let mut m = 1.0;
    match style {
        0 | 1 => m = 0.0,
        3 => {
            // Dashes twice as long as the gaps, a whole number per side, centred on the corners.
            let unit = len / (len / (3.0 * w)).round().max(1.0);
            let x = along / unit + 1.0 / 3.0;
            m = if x - x.floor() < 2.0 / 3.0 { 1.0 } else { 0.0 };
        }
        4 => {
            // Round dots one width across, a whole number per side, one per corner.
            let unit = len / (len / (2.0 * w)).round().max(1.0);
            let x = along / unit + 0.5;
            let a = (x - x.floor() - 0.5) * unit;
            let dy = t - w * 0.5;
            let dist = (a * a + dy * dy).sqrt();
            m = ((w * 0.5 - dist) * s + 0.5).clamp(0.0, 1.0);
        }
        5 => {
            m = if t < w / 3.0 || t > w * 2.0 / 3.0 {
                1.0
            } else {
                0.0
            }
        }
        6 | 7 => {
            // groove: the outer half dark on the top/left sides, the inner half on the
            // bottom/right; ridge the other way round.
            if (outer_half == top_left) == (style == 6) {
                color = dark;
            }
        }
        8 if top_left => color = dark,
        9 if !top_left => color = dark,
        _ => {}
    }
    scaled(color, ring * m)
}

/// A wavy text decoration (`KIND_WAVY`): a sine wave `params.y` thick swinging across `rect`.
pub fn wavy(f: &Frame, i: &Instance, p: [f32; 2]) -> [f32; 4] {
    let r = i.rect;
    let t = i.params[1].max(1.0 / f.scale);
    let amp = (r[3] - t).max(0.0) * 0.5;
    let k = std::f32::consts::TAU / (4.0 * t).max(4.0);
    let x = p[0] - r[0];
    let y = r[1] + r[3] * 0.5 + amp * (x * k).sin();
    let slope = amp * k * (x * k).cos();
    let d = (p[1] - y).abs() / (1.0 + slope * slope).sqrt() - t * 0.5;
    if x < 0.0 || x > r[2] {
        return [0.0; 4];
    }
    scaled(i.color0, coverage(d, f.scale))
}

/// A border-image piece cut from a gradient (`KIND_GRADIENT_PIECE`): `p0` the part of the gradient
/// (absolute px) stretched over `rect`, `p1` the area the gradient spans.
pub fn gradient_piece(f: &Frame, i: &Instance, p: [f32; 2]) -> [f32; 4] {
    let inside = shape_coverage(p, i.rect, [0.0; 4], [0.0; 4], f.scale);
    let Some(g) = f.gradients.get(i.params[2] as usize) else {
        return [0.0; 4];
    };
    let src = [
        i.p0[0] + (p[0] - i.rect[0]) * i.p0[2] / i.rect[2].max(1e-4),
        i.p0[1] + (p[1] - i.rect[1]) * i.p0[3] / i.rect[3].max(1e-4),
    ];
    scaled(gradient_color(g, src, i.p1, f.srgb), inside)
}
