// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Shapes`: GDI-style outlines (rectangles, ellipses, arcs, pies, polygons, Beziers, cardinal
//! curves) as lists of points for `LineRenderer2D.points`. The maths is ported from
//! `js/engine/src/shapes.ts` and the compiled facade, in the same order of operations, so the
//! points are bit for bit what they were. Compiled scripts and QuickJS call the same functions.

use crate::jsnum;
use crate::math::{V2, v2};
use ts_api::ts_api;

/// Degrees to radians, as `Math.PI / 180` is in JavaScript.
const DEG: f64 = 0.017453292519943295;

/// The point at `degrees` on the ellipse round `(cx, cy)` with radii `rx` and `ry`.
fn at(cx: f64, cy: f64, rx: f64, ry: f64, degrees: f64) -> V2 {
    v2(
        cx + (degrees * DEG).cos() * rx,
        cy + (degrees * DEG).sin() * ry,
    )
}

/// The `i`-th point of a cardinal curve, wrapping when closed and clamped to the ends otherwise.
fn pick(points: &[V2], i: i64, closed: bool) -> V2 {
    let n = points.len() as i64;
    if closed {
        points[(((i % n) + n) % n) as usize]
    } else {
        points[i.clamp(0, n - 1) as usize]
    }
}

/// A cardinal spline through `points`, `segments` points per span (Hermite form).
fn cardinal(points: &[V2], tension: f64, segments: f64, closed: bool) -> Vec<V2> {
    let n = points.len();
    if n < 2 {
        return points.to_vec();
    }
    let spans = if closed { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..spans as i64 {
        let p0 = pick(points, i - 1, closed);
        let p1 = pick(points, i, closed);
        let p2 = pick(points, i + 1, closed);
        let p3 = pick(points, i + 2, closed);
        // GDI+ tangents: `tension` times the chord across each point (0.5 is Catmull-Rom).
        let m1x = tension * (p2.x - p0.x);
        let m1y = tension * (p2.y - p0.y);
        let m2x = tension * (p3.x - p1.x);
        let m2y = tension * (p3.y - p1.y);
        let mut k = 0.0;
        while k < segments {
            let t = k / segments;
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            out.push(v2(
                h00 * p1.x + h10 * m1x + h01 * p2.x + h11 * m2x,
                h00 * p1.y + h10 * m1y + h01 * p2.y + h11 * m2y,
            ));
            k += 1.0;
        }
    }
    if !closed {
        out.push(points[n - 1]);
    }
    out
}

/// `n` or `fallback` when the argument was left out (NaN).
fn or(n: f64, fallback: f64) -> f64 {
    if n.is_nan() { fallback } else { n }
}

/// Outlines for `LineRenderer2D.points` and fills: y up, angles in degrees counter-clockwise from
/// +x (GDI's y points down, so its clockwise angles come out mirrored). Closed shapes do not
/// repeat their first point: set the renderer's `loop` (a `fillColor` always closes the shape).
#[ts_api(namespace = "Shapes", like, values(Vector2 = V2 [x: f64, y: f64]))]
pub mod shapes_api {
    use super::*;

    /// A rectangle from its bottom-left corner (GDI DrawRectangle).
    pub fn rectangle(x: f64, y: f64, width: f64, height: f64) -> Vec<V2> {
        vec![
            v2(x, y),
            v2(x + width, y),
            v2(x + width, y + height),
            v2(x, y + height),
        ]
    }

    /// A rectangle with corners rounded to `radius` (at most half the shorter side).
    #[optional(segments = 8)]
    pub fn rounded_rectangle(
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        segments: f64,
    ) -> Vec<V2> {
        let r = jsnum::max(
            0.0,
            jsnum::min(radius, jsnum::min(width / 2.0, height / 2.0)),
        );
        if r == 0.0 {
            return rectangle(x, y, width, height);
        }
        let segments = or(segments, 8.0);
        let corners = [
            [x + width - r, y + r, -90.0],
            [x + width - r, y + height - r, 0.0],
            [x + r, y + height - r, 90.0],
            [x + r, y + r, 180.0],
        ];
        let mut out = Vec::new();
        for k in corners {
            let mut i = 0.0;
            while i <= segments {
                out.push(at(k[0], k[1], r, r, k[2] + (90.0 * i) / segments));
                i += 1.0;
            }
        }
        out
    }

    /// An ellipse around `center` with radii `rx` and `ry` (GDI DrawEllipse).
    #[optional(segments = 64)]
    pub fn ellipse(center: V2, rx: f64, ry: f64, segments: f64) -> Vec<V2> {
        let segments = or(segments, 64.0);
        let mut out = Vec::new();
        let mut i = 0.0;
        while i < segments {
            out.push(at(center.x, center.y, rx, ry, (360.0 * i) / segments));
            i += 1.0;
        }
        out
    }

    /// A circle: an ellipse with equal radii.
    #[optional(segments = 64)]
    pub fn circle(center: V2, radius: f64, segments: f64) -> Vec<V2> {
        ellipse(center, radius, radius, segments)
    }

    /// Part of an ellipse from `startAngle`, `sweepAngle` degrees round (negative goes clockwise),
    /// GDI DrawArc. Without `segments` it takes 64 per full turn.
    #[optional(segments)]
    pub fn arc(
        center: V2,
        rx: f64,
        ry: f64,
        start_angle: f64,
        sweep_angle: f64,
        segments: f64,
    ) -> Vec<V2> {
        let n = jsnum::max(
            1.0,
            if segments.is_nan() {
                (sweep_angle.abs() / 360.0 * 64.0).ceil()
            } else {
                segments
            },
        );
        let mut out = Vec::new();
        let mut i = 0.0;
        while i <= n {
            out.push(at(
                center.x,
                center.y,
                rx,
                ry,
                start_angle + (sweep_angle * i) / n,
            ));
            i += 1.0;
        }
        out
    }

    /// A pie slice: the centre, then the arc (GDI DrawPie / FillPie).
    #[optional(segments)]
    pub fn pie(
        center: V2,
        rx: f64,
        ry: f64,
        start_angle: f64,
        sweep_angle: f64,
        segments: f64,
    ) -> Vec<V2> {
        let mut out = vec![center];
        out.extend(arc(center, rx, ry, start_angle, sweep_angle, segments));
        out
    }

    /// `sides` corners evenly round a circle, the first at `rotation` degrees (90: on top).
    #[optional(rotation = 90)]
    pub fn regular_polygon(center: V2, radius: f64, sides: f64, rotation: f64) -> Vec<V2> {
        let n = jsnum::max(3.0, jsnum::round(sides));
        let rotation = or(rotation, 90.0);
        let mut out = Vec::new();
        let mut i = 0.0;
        while i < n {
            out.push(at(
                center.x,
                center.y,
                radius,
                radius,
                rotation + (360.0 * i) / n,
            ));
            i += 1.0;
        }
        out
    }

    /// The points as given (GDI DrawPolygon).
    #[rest(points)]
    pub fn polygon(points: &[V2]) -> Vec<V2> {
        points.to_vec()
    }

    /// Three corners.
    pub fn triangle(a: V2, b: V2, c: V2) -> Vec<V2> {
        vec![a, b, c]
    }

    /// A cubic Bezier from `p0` to `p3`, pulled toward `p1` and `p2` (GDI DrawBezier).
    #[optional(segments = 32)]
    pub fn bezier(p0: V2, p1: V2, p2: V2, p3: V2, segments: f64) -> Vec<V2> {
        let segments = or(segments, 32.0);
        let mut out = Vec::new();
        let mut i = 0.0;
        while i <= segments {
            let t = i / segments;
            let u = 1.0 - t;
            let a = u * u * u;
            let b = 3.0 * u * u * t;
            let c = 3.0 * u * t * t;
            let d = t * t * t;
            out.push(v2(
                a * p0.x + b * p1.x + c * p2.x + d * p3.x,
                a * p0.y + b * p1.y + c * p2.y + d * p3.y,
            ));
            i += 1.0;
        }
        out
    }

    /// A smooth curve through every point (GDI DrawCurve): `tension` 0 is straight lines, 0.5 a
    /// Catmull-Rom spline.
    #[optional(tension = 0.5, segments = 8)]
    pub fn curve(points: &[V2], tension: f64, segments: f64) -> Vec<V2> {
        cardinal(points, or(tension, 0.5), or(segments, 8.0), false)
    }

    /// The same, closed back to the first point (GDI DrawClosedCurve / FillClosedCurve).
    #[optional(tension = 0.5, segments = 8)]
    pub fn closed_curve(points: &[V2], tension: f64, segments: f64) -> Vec<V2> {
        cardinal(points, or(tension, 0.5), or(segments, 8.0), true)
    }
}
