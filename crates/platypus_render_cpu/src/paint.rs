// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! CSS gradients as `vello_cpu` gradients, in the draw list's logical pixels.

use platypus_ui::{BackgroundImage, GradientDirection, RadialShape, Rect, resolve_stops};
use vello_cpu::kurbo::{Affine, Point};
use vello_cpu::peniko::{Color, Extend, Gradient};

/// A gradient painted over one tile (`tile`), and the paint transform that stretches a circle
/// into the ellipse of a radial gradient (identity otherwise). `None` for `url()` images.
pub fn gradient(image: &BackgroundImage, tile: Rect) -> Option<(Gradient, Affine)> {
    let (w, h) = (tile.width.max(0.0), tile.height.max(0.0));
    let center = Point::new(f64::from(tile.x + w / 2.0), f64::from(tile.y + h / 2.0));
    let color = |c: platypus_ui::Rgba| Color::new([c.r, c.g, c.b, c.a]);
    let extend = |repeating: bool| {
        if repeating {
            Extend::Repeat
        } else {
            Extend::Pad
        }
    };
    // The stops' positions along the gradient line (a fraction of its length), rebased so the
    // first is 0 and the last 1: repeating gradients repeat that span, the others pad.
    let rebase = |stops: &[platypus_ui::ColorStop], length: f32| {
        let resolved = resolve_stops(stops, length);
        let first = resolved.first().map_or(0.0, |s| s.1);
        let last = resolved.last().map_or(1.0, |s| s.1).max(first + 1e-4);
        let list: Vec<(f32, Color)> = resolved
            .iter()
            .map(|(c, p)| {
                (
                    (p - first) / (last - first),
                    color(platypus_ui::Rgba::new(c[0], c[1], c[2], c[3])),
                )
            })
            .collect();
        (list, first, last)
    };
    match image {
        BackgroundImage::Url(_) => None,
        BackgroundImage::Linear {
            direction,
            stops,
            repeating,
        } => {
            let (dx, dy) = match *direction {
                GradientDirection::Angle(deg) => {
                    let t = deg.to_radians();
                    (t.sin(), -t.cos())
                }
                GradientDirection::Corner { right, bottom } => {
                    let sx = if right { 1.0 } else { -1.0 };
                    let sy = if bottom { 1.0 } else { -1.0 };
                    let (x, y) = (h * sx, w * sy);
                    let len = (x * x + y * y).sqrt().max(f32::EPSILON);
                    (x / len, y / len)
                }
            };
            let length = ((w * dx).abs() + (h * dy).abs()).max(f32::EPSILON);
            let (list, first, last) = rebase(stops, length);
            let along = |t: f32| {
                Point::new(
                    center.x + f64::from(dx * length * (t - 0.5)),
                    center.y + f64::from(dy * length * (t - 0.5)),
                )
            };
            let g = Gradient::new_linear(along(first), along(last))
                .with_stops(list.as_slice())
                .with_extend(extend(*repeating));
            Some((g, Affine::IDENTITY))
        }
        BackgroundImage::Radial {
            shape,
            at,
            stops,
            repeating,
        } => {
            // Centred on `at`, sized to the farthest corner from it (as the GPU path does).
            let c = Point::new(
                f64::from(tile.x + at.x.resolve(w)),
                f64::from(tile.y + at.y.resolve(h)),
            );
            let (dx, dy) = (
                at.x.resolve(w).max(w - at.x.resolve(w)),
                at.y.resolve(h).max(h - at.y.resolve(h)),
            );
            let (rx, ry) = match shape {
                RadialShape::Circle => {
                    let r = (dx * dx + dy * dy).sqrt();
                    (r, r)
                }
                RadialShape::Ellipse => {
                    (dx * std::f32::consts::SQRT_2, dy * std::f32::consts::SQRT_2)
                }
            };
            let rx = rx.max(f32::EPSILON);
            let (list, first, last) = rebase(stops, rx);
            // Stops are a fraction of the horizontal radius; the ellipse stretches y. The
            // rebased span starts at the first stop's radius, not at the centre.
            let g = Gradient::new_two_point_radial(c, rx * first, c, rx * last)
                .with_stops(list.as_slice())
                .with_extend(extend(*repeating));
            let k = f64::from(ry.max(f32::EPSILON) / rx);
            let stretch = Affine::translate(c.to_vec2())
                * Affine::scale_non_uniform(1.0, k)
                * Affine::translate(-c.to_vec2());
            Some((g, stretch))
        }
        BackgroundImage::Conic {
            from,
            at,
            stops,
            repeating,
        } => {
            let c = Point::new(
                f64::from(tile.x + at.x.resolve(w)),
                f64::from(tile.y + at.y.resolve(h)),
            );
            let (list, first, last) = rebase(stops, 1.0);
            // CSS starts at the top and turns clockwise; the sweep starts at +x, y down.
            let turn = std::f32::consts::TAU;
            let start = (from - 90.0).to_radians();
            let g = Gradient::new_sweep(c, start + turn * first, start + turn * last)
                .with_stops(list.as_slice())
                .with_extend(extend(*repeating));
            Some((g, Affine::IDENTITY))
        }
    }
}
