// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Gradient colours from the plan's gradient table, as the shaders' `gradient_color`: linear, radial
//! and conic, repeating or not, stops interpolated in premultiplied sRGB like CSS.

use web_ui_render::plan::GpuGradient;

use super::color::srgb_to_linear;

#[inline]
fn stop(g: &GpuGradient, i: usize) -> f32 {
    g.stops[i / 4][i % 4]
}

#[inline]
fn premultiply(c: [f32; 4]) -> [f32; 4] {
    [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
}

/// Gradient `g`'s colour at `p` for a box `rect` (logical px), premultiplied in the canvas's space.
pub fn gradient_color(g: &GpuGradient, p: [f32; 2], rect: [f32; 4], srgb: bool) -> [f32; 4] {
    let (info, info2) = (g.info, g.info2);
    let center = [rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5];
    let mut t = if info[0] < 0.5 {
        ((p[0] - center[0]) * info[2] + (p[1] - center[1]) * info[3]) / info2[0] + 0.5
    } else if info[0] < 1.5 {
        // Radial: about its own centre (`info2.yz`, relative to the box), as the shaders.
        let c = [rect[0] + info2[1], rect[1] + info2[2]];
        let (dx, dy) = ((p[0] - c[0]) / info[2], (p[1] - c[1]) / info[3]);
        (dx * dx + dy * dy).sqrt()
    } else {
        // Clockwise from up, from the `from` angle, as a fraction of a turn.
        let d = [p[0] - (rect[0] + info2[1]), p[1] - (rect[1] + info2[2])];
        let turn = std::f32::consts::TAU;
        let a = (d[0].atan2(-d[1]) - info[2]) / turn;
        a - a.floor()
    };
    let count = (info[1] as usize).clamp(1, g.colors.len());
    if info2[3] > 0.5 {
        let first = stop(g, 0);
        let span = stop(g, count - 1) - first;
        if span > 1e-6 {
            let f = (t - first) / span;
            t = first + (f - f.floor()) * span;
        }
    }
    let mut c = g.colors[0];
    if t >= stop(g, count - 1) {
        c = g.colors[count - 1];
    } else if t > stop(g, 0) {
        for i in 0..count - 1 {
            let (a, b) = (stop(g, i), stop(g, i + 1));
            if t >= a && t < b {
                let f = if b > a { (t - a) / (b - a) } else { 0.0 };
                let (ca, cb) = (premultiply(g.colors[i]), premultiply(g.colors[i + 1]));
                let m: [f32; 4] = std::array::from_fn(|k| ca[k] + (cb[k] - ca[k]) * f);
                c = if m[3] > 0.0 {
                    [m[0] / m[3], m[1] / m[3], m[2] / m[3], m[3]]
                } else {
                    [0.0, 0.0, 0.0, m[3]]
                };
                break;
            }
        }
    }
    if srgb {
        premultiply(c)
    } else {
        [
            srgb_to_linear(c[0]) * c[3],
            srgb_to_linear(c[1]) * c[3],
            srgb_to_linear(c[2]) * c[3],
            c[3],
        ]
    }
}
