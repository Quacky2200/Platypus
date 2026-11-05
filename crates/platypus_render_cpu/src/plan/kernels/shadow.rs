// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! **Mode 2, Shadow: blurred rounded boxes.** `KIND_OUTER_SHADOW` (`box-shadow`, drawn outside the
//! box that casts it) and `KIND_INSET_SHADOW` (`inset`, inside the padding box around a blurred hole),
//! with Evan Wallace's analytic blurred box as the GLES shader has it: an erf across x, four Gaussian
//! samples along y.
//!
//! The fast path is what a shadow does not draw: inside the casting box (outer) or inside the hole
//! (inset) nothing shows, so [`outer`] and [`inset`] say so and the raster loop skips that run of
//! the row. Only the blurred ring is computed per pixel.

use platypus_render::plan::Instance;

use super::super::geometry::{Frame, corner_radius, shape_coverage};

#[inline]
fn erf2(x: [f32; 2]) -> [f32; 2] {
    x.map(|v| {
        let s = v.signum();
        let a = v.abs();
        let r = 1.0 + (0.278_393 + (0.230_389 + 0.078_108 * (a * a)) * a) * a;
        let r = r * r;
        s - s / (r * r)
    })
}

#[inline]
fn gaussian(x: f32, sigma: f32) -> f32 {
    let pi = std::f32::consts::PI;
    (-(x * x) / (2.0 * sigma * sigma)).exp() / ((2.0 * pi).sqrt() * sigma)
}

#[inline]
fn shadow_x(x: f32, y: f32, sigma: f32, corner: f32, half: [f32; 2]) -> f32 {
    let delta = (half[1] - corner - y.abs()).min(0.0);
    let curved = half[0] - corner + (corner * corner - delta * delta).max(0.0).sqrt();
    let k = std::f32::consts::FRAC_1_SQRT_2 / sigma;
    let e = erf2([(x - curved) * k, (x + curved) * k]);
    (0.5 + 0.5 * e[1]) - (0.5 + 0.5 * e[0])
}

/// Coverage of a rounded rectangle convolved with a Gaussian of `sigma`.
#[inline]
pub fn blurred_box(p: [f32; 2], rect: [f32; 4], radii: [f32; 4], sigma: f32, scale: f32) -> f32 {
    if rect[2] <= 0.0 || rect[3] <= 0.0 {
        return 0.0;
    }
    if sigma < 0.01 {
        return shape_coverage(p, rect, radii, radii, scale);
    }
    let half = [rect[2] * 0.5, rect[3] * 0.5];
    let point = [p[0] - (rect[0] + half[0]), p[1] - (rect[1] + half[1])];
    let corner = corner_radius(point, radii).min(half[0].min(half[1]));
    let low = point[1] - half[1];
    let high = point[1] + half[1];
    let start = (-3.0 * sigma).clamp(low, high);
    let end = (3.0 * sigma).clamp(low, high);
    let step = (end - start) / 4.0;
    let mut y = start + step * 0.5;
    let mut value = 0.0;
    for _ in 0..4 {
        value += shadow_x(point[0], point[1] - y, sigma, corner, half) * gaussian(y, sigma) * step;
        y += step;
    }
    value.clamp(0.0, 1.0)
}

/// An outer shadow at `p`: its colour, and whether `p` is fully inside the box casting it (where
/// CSS draws none).
#[inline]
pub fn outer(f: &Frame, i: &Instance, p: [f32; 2]) -> ([f32; 4], bool) {
    let caster = shape_coverage(p, i.p0, i.p1, i.p1, f.scale);
    if caster >= 1.0 {
        return ([0.0; 4], true);
    }
    let shadow = blurred_box(p, i.rect, i.radii, i.params[1], f.scale);
    (i.color0.map(|c| c * shadow * (1.0 - caster)), false)
}

/// An inset shadow at `p`: its colour, and whether `p` is fully inside the hole (none drawn).
#[inline]
pub fn inset(f: &Frame, i: &Instance, p: [f32; 2]) -> ([f32; 4], bool) {
    let inside = shape_coverage(p, i.rect, i.radii, i.radii_y, f.scale);
    if inside <= 0.0 {
        return ([0.0; 4], false);
    }
    let hole = blurred_box(p, i.p0, i.p1, i.params[1], f.scale);
    (i.color0.map(|c| c * inside * (1.0 - hole)), hole >= 1.0)
}
