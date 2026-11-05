// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What every kernel shares, ported from the GLES shaders' `GEOMETRY` and `QUAD_COMMON`: rounded-box
//! distances, one-physical-pixel anti-aliasing, the clip table (rounded boxes and polygons chained to
//! their parents) and the transforms it holds. Points are in logical pixels, y down.

use web_ui_render::plan::GpuClip;

/// What a frame's kernels read besides the instance: the scale and the tables.
pub struct Frame<'a> {
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// The canvases hold sRGB values (`Blending::Srgb`).
    pub srgb: bool,
    pub clips: &'a [GpuClip],
    pub gradients: &'a [web_ui_render::plan::GpuGradient],
}

/// Picks the corner radius for the quadrant of `q` (relative to the centre, y down).
#[inline]
pub fn corner_radius(q: [f32; 2], radii: [f32; 4]) -> f32 {
    let (top, bottom) = if q[0] > 0.0 {
        (radii[1], radii[2])
    } else {
        (radii[0], radii[3])
    };
    if q[1] > 0.0 { bottom } else { top }
}

/// Signed distance (logical px) to a rounded rectangle with per-corner elliptical radii.
#[inline]
pub fn sd_rounded_box(p: [f32; 2], rect: [f32; 4], rx: [f32; 4], ry: [f32; 4]) -> f32 {
    let half = [rect[2] * 0.5, rect[3] * 0.5];
    let q0 = [p[0] - (rect[0] + half[0]), p[1] - (rect[1] + half[1])];
    let edge = [q0[0].abs() - half[0], q0[1].abs() - half[1]];
    let r = [
        corner_radius(q0, rx).min(half[0]),
        corner_radius(q0, ry).min(half[1]),
    ];
    if r[0] > 0.0 && r[1] > 0.0 {
        let q = [edge[0] + r[0], edge[1] + r[1]];
        if q[0] > 0.0 && q[1] > 0.0 {
            let (a, b) = (q[0] / r[0], q[1] / r[1]);
            return ((a * a + b * b).sqrt() - 1.0) * r[0].min(r[1]);
        }
    }
    let (ox, oy) = (edge[0].max(0.0), edge[1].max(0.0));
    edge[0].max(edge[1]).min(0.0) + (ox * ox + oy * oy).sqrt()
}

/// Anti-aliased coverage from a signed distance: one physical pixel of feathering.
#[inline]
pub fn coverage(d: f32, scale: f32) -> f32 {
    (0.5 - d * scale).clamp(0.0, 1.0)
}

#[inline]
pub fn shape_coverage(p: [f32; 2], rect: [f32; 4], rx: [f32; 4], ry: [f32; 4], scale: f32) -> f32 {
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        return 0.0;
    }
    coverage(sd_rounded_box(p, rect, rx, ry), scale)
}

/// The table's entries as one array of vec4s (polygon points follow their entry).
#[inline]
fn word(clips: &[GpuClip], i: usize) -> [f32; 4] {
    let e = &clips[i / 5];
    match i % 5 {
        0 => e.rect,
        1 => e.rx,
        2 => e.ry,
        3 => e.bounds,
        _ => e.info,
    }
}

/// Into the window through transform `i` (1-based) of the clip table.
#[inline]
pub fn xf_apply(clips: &[GpuClip], i: u32, p: [f32; 2]) -> [f32; 2] {
    let e = &clips[i as usize - 1];
    let (e0, e1, e2) = (e.rect, e.rx, e.ry);
    let x = e0[0] * p[0] + e0[1] * p[1] + e0[2];
    let y = e0[3] * p[0] + e1[0] * p[1] + e1[1];
    let w = (e1[2] * p[0] + e1[3] * p[1] + e2[0]).max(1e-6);
    [x / w, y / w]
}

/// From the window into transform `i`'s plane.
#[inline]
pub fn xf_unapply(clips: &[GpuClip], i: u32, p: [f32; 2]) -> [f32; 2] {
    let e = &clips[i as usize - 1];
    let (e2, e3, e4) = (e.ry, e.bounds, e.info);
    let x = e2[1] * p[0] + e2[2] * p[1] + e2[3];
    let y = e3[0] * p[0] + e3[1] * p[1] + e3[2];
    let w = (e3[3] * p[0] + e4[0] * p[1] + e4[1]).max(1e-6);
    [x / w, y / w]
}

/// Signed distance to a polygon clip's edges, negative inside by its fill rule.
fn sd_polygon(clips: &[GpuClip], p: [f32; 2], c: &GpuClip, even_odd: bool) -> f32 {
    let first = c.info[2] as usize;
    let count = c.info[3] as usize;
    let mut d = 1e9f32;
    let mut winding = 0i32;
    let mut crossings = 0u32;
    for k in 0..count {
        let a4 = word(clips, first + k);
        let next = if k + 1 < a4[3] as usize {
            k + 1
        } else {
            a4[2] as usize
        };
        let a = [a4[0], a4[1]];
        let b4 = word(clips, first + next);
        let b = [b4[0], b4[1]];
        let e = [b[0] - a[0], b[1] - a[1]];
        let w = [p[0] - a[0], p[1] - a[1]];
        let t =
            ((w[0] * e[0] + w[1] * e[1]) / (e[0] * e[0] + e[1] * e[1]).max(1e-8)).clamp(0.0, 1.0);
        let (dx, dy) = (w[0] - e[0] * t, w[1] - e[1] * t);
        d = d.min((dx * dx + dy * dy).sqrt());
        let left = e[0] * w[1] - e[1] * w[0];
        if a[1] <= p[1] && b[1] > p[1] && left > 0.0 {
            winding += 1;
        } else if a[1] > p[1] && b[1] <= p[1] && left < 0.0 {
            winding -= 1;
        }
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) * e[0] / e[1] {
            crossings += 1;
        }
    }
    let inside = if even_odd {
        !crossings.is_multiple_of(2)
    } else {
        winding != 0
    };
    if inside { -d } else { d }
}

/// Coverage of clip `index` (1-based, 0 = none) and its parents at `p` (window, logical px).
#[inline]
pub fn clip_coverage(f: &Frame, p: [f32; 2], index: u32) -> f32 {
    let mut i = index;
    let mut cover = 1.0f32;
    let mut n = 0;
    while n < 32 && i > 0 && cover > 0.0 {
        let c = &f.clips[i as usize - 1];
        let kind = (c.info[1] as u32) % 4;
        let t = (c.info[1] as u32) / 4;
        let q = if t > 0 { xf_unapply(f.clips, t, p) } else { p };
        let d = if kind > 0 {
            sd_polygon(f.clips, q, c, kind == 2)
        } else {
            sd_rounded_box(q, c.rect, c.rx, c.ry)
        };
        cover *= (0.5 - d * f.scale).clamp(0.0, 1.0);
        i = c.info[0] as u32;
        n += 1;
    }
    cover
}

/// Whether clip `index`'s whole chain is made of shapes whose inside is convex (rounded boxes, in a
/// plane or not): its full-coverage pixels on a row are then one run, which the span kernels rely on.
pub fn clip_is_convex(clips: &[GpuClip], index: u32) -> bool {
    let mut i = index;
    let mut n = 0;
    while n < 32 && i > 0 {
        let c = &clips[i as usize - 1];
        // A shape kind other than 0 (a polygon) in the low two bits of `info.y`.
        if !(c.info[1] as u32).is_multiple_of(4) {
            return false;
        }
        i = c.info[0] as u32;
        n += 1;
    }
    true
}

/// Whether the whole of `rect` (window, logical px) is fully inside clip `index`: every rounded box of
/// the chain, untransformed, holds it once its corners are taken off. Lets a kernel skip the clip.
pub fn clip_holds(clips: &[GpuClip], index: u32, rect: [f32; 4], scale: f32) -> bool {
    let mut i = index;
    let mut n = 0;
    // Full coverage needs half a physical pixel inside the edge.
    let margin = 0.5 / scale;
    while n < 32 && i > 0 {
        let c = &clips[i as usize - 1];
        if c.info[1] != 0.0 {
            return false;
        }
        let r =
            c.rx.iter()
                .chain(c.ry.iter())
                .fold(0.0f32, |m, v| m.max(*v));
        let inner = [
            c.rect[0] + r + margin,
            c.rect[1] + r + margin,
            c.rect[0] + c.rect[2] - r - margin,
            c.rect[1] + c.rect[3] - r - margin,
        ];
        let inside_flat = rect[0] >= c.rect[0] + margin
            && rect[2] <= c.rect[0] + c.rect[2] - margin
            && rect[1] >= c.rect[1] + margin
            && rect[3] <= c.rect[1] + c.rect[3] - margin;
        // Clear of the corners: inside the box, and within the band between them on one axis.
        let clear = (rect[0] >= inner[0] && rect[2] <= inner[2])
            || (rect[1] >= inner[1] && rect[3] <= inner[3]);
        if !(inside_flat && (r <= 0.0 || clear)) {
            return false;
        }
        i = c.info[0] as u32;
        n += 1;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rounded_box_is_negative_inside_and_rounds_its_corners() {
        let rect = [0.0, 0.0, 100.0, 50.0];
        let r = [10.0; 4];
        assert!(sd_rounded_box([50.0, 25.0], rect, r, r) < -20.0);
        assert!((sd_rounded_box([110.0, 25.0], rect, r, r) - 10.0).abs() < 1e-4);
        // The corner's centre is 10 px in on both axes: its corner pixel is outside the curve.
        assert!(sd_rounded_box([0.5, 0.5], rect, r, r) > 0.0);
        assert_eq!(coverage(-1.0, 1.0), 1.0);
        assert_eq!(coverage(1.0, 1.0), 0.0);
    }
}
