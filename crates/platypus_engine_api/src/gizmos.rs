// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Gizmos`: immediate-mode debug lines. A script clears, sets the colour, draws lines and calls
//! `present(camera)`; the host takes the finished frame with [`take_frame`] and draws it as thin
//! lines, so no mesh is built in script each frame.

use crate::camera::CameraRef;
use crate::color::Rgba;
use platypus_math::{M4, V3};
use std::cell::RefCell;
use ts_api::ts_api;

#[derive(Clone, Debug, PartialEq)]
pub struct GizmoLine {
    pub from: V3,
    pub to: V3,
    pub color: Rgba,
}

/// The lines of one `present`, for the camera it named.
#[derive(Clone, Debug, PartialEq)]
pub struct GizmoFrame {
    pub camera: CameraRef,
    pub lines: Vec<GizmoLine>,
}

struct State {
    /// Places what is drawn next (a shape in an object's space).
    matrix: M4,
    color: Rgba,
    pending: Vec<GizmoLine>,
    presented: Option<GizmoFrame>,
}

const WHITE: Rgba = Rgba {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State { matrix: M4::IDENTITY, color: WHITE, pending: Vec::new(), presented: None })
    };
}

/// The last presented frame, once (the host draws it until the script presents another).
pub fn take_frame() -> Option<GizmoFrame> {
    STATE.with(|s| s.borrow_mut().presented.take())
}

/// Segments in a full circle.
const CIRCLE: usize = 48;

/// Adds a line through the current matrix, in the current colour.
fn push(a: V3, b: V3) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let m = s.matrix;
        let pt = |p: V3| {
            let m = m.m;
            V3 {
                x: m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
                y: m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
                z: m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14],
            }
        };
        let color = s.color;
        s.pending.push(GizmoLine {
            from: pt(a),
            to: pt(b),
            color,
        });
    });
}

pub fn reset() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.color = WHITE;
        s.matrix = M4::IDENTITY;
        s.pending.clear();
        s.presented = None;
    });
}

#[ts_api(
    namespace = "Gizmos",
    handles(Camera = CameraRef),
    values(
        Vector3 = V3 [x: f64, y: f64, z: f64],
        Color = Rgba [r: f64, g: f64, b: f64, a: f64],
        Matrix4x4 = M4 [m0: f64, m1: f64, m2: f64, m3: f64, m4: f64, m5: f64, m6: f64, m7: f64,
                        m8: f64, m9: f64, m10: f64, m11: f64, m12: f64, m13: f64, m14: f64, m15: f64]
    )
)]
pub mod gizmos_api {
    use super::{GizmoFrame, STATE, push};
    use crate::camera::CameraRef;
    use crate::color::Rgba;
    use platypus_math::{M4, V3};

    /// Places what is drawn next (`Matrix4x4.trs(..)` for a shape in an object's space).
    #[property]
    pub fn matrix() -> M4 {
        STATE.with(|s| s.borrow().matrix)
    }

    pub fn set_matrix(m: M4) {
        STATE.with(|s| s.borrow_mut().matrix = m);
    }

    /// A line from `from` along `direction` (its length included).
    pub fn draw_ray(from: V3, direction: V3) {
        push(from, from.add(direction));
    }

    /// The edges of a box.
    pub fn draw_wire_cube(center: V3, size: V3) {
        let h = size.scale(0.5);
        let corner = |i: usize| {
            center.add(V3 {
                x: if i & 1 != 0 { h.x } else { -h.x },
                y: if i & 2 != 0 { h.y } else { -h.y },
                z: if i & 4 != 0 { h.z } else { -h.z },
            })
        };
        for i in 0..8usize {
            for bit in [1usize, 2, 4] {
                if i & bit == 0 {
                    push(corner(i), corner(i | bit));
                }
            }
        }
    }

    /// A circle about `normal` (Unity's `Handles.DrawWireDisc`).
    pub fn draw_wire_disc(center: V3, normal: V3, radius: f64) {
        let n = normal.normalized();
        let up = if n.y.abs() < 0.9 { V3::UP } else { V3::RIGHT };
        let u = n.cross(up).normalized();
        let v = n.cross(u);
        let mut prev = center.add(u.scale(radius));
        for i in 1..=super::CIRCLE {
            let a = i as f64 / super::CIRCLE as f64 * std::f64::consts::PI * 2.0;
            let p = center
                .add(u.scale(a.cos() * radius))
                .add(v.scale(a.sin() * radius));
            push(prev, p);
            prev = p;
        }
    }

    /// A sphere as three circles, one about each axis.
    pub fn draw_wire_sphere(center: V3, radius: f64) {
        for n in [
            V3::RIGHT,
            V3::UP,
            V3 {
                x: 0.0,
                y: 0.0,
                z: -1.0,
            },
        ] {
            draw_wire_disc(center, n, radius);
        }
    }

    /// A view frustum from `center` looking along -z (forward), like a camera's: `fov` degrees
    /// tall, `aspect` wide per tall, from `minRange` to `maxRange`.
    pub fn draw_frustum(center: V3, fov: f64, max_range: f64, min_range: f64, aspect: f64) {
        let t = (fov / 2.0 * std::f64::consts::PI / 180.0).tan();
        let rect = |d: f64| {
            let h = d * t;
            let w = h * aspect;
            [(-w, -h), (w, -h), (w, h), (-w, h)].map(|(x, y)| center.add(V3 { x, y, z: -d }))
        };
        let near = rect(min_range);
        let far = rect(max_range);
        for i in 0..4 {
            push(near[i], near[(i + 1) % 4]);
            push(far[i], far[(i + 1) % 4]);
            push(near[i], far[i]);
        }
    }

    /// How many lines are waiting to be presented.
    #[property]
    pub fn line_count() -> f64 {
        STATE.with(|s| s.borrow().pending.len() as f64)
    }

    /// The colour of the lines drawn next.
    #[property]
    pub fn color() -> Rgba {
        STATE.with(|s| s.borrow().color)
    }

    pub fn set_color(c: Rgba) {
        STATE.with(|s| s.borrow_mut().color = c);
    }

    /// Forgets the lines drawn so far.
    pub fn clear() {
        STATE.with(|s| s.borrow_mut().pending.clear());
    }

    pub fn draw_line(from: V3, to: V3) {
        push(from, to);
    }

    /// Hands the lines drawn since `clear` to the renderer for this camera.
    pub fn present(camera: CameraRef) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let lines = std::mem::take(&mut s.pending);
            s.presented = Some(GizmoFrame { camera, lines });
        });
    }
}
