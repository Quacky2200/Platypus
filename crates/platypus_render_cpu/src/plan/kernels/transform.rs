// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! **Mode 5, Transform: instances in a transformed plane** (`transform`, `rotate`, `scale`,
//! `perspective` on an element: `PushTransform`). The plan keeps each instance in its own plane and
//! the homography in the clip table (`xf`). As the GLES vertex shader does, the instance's area goes
//! through the homography for its place in the window; then each pixel there comes back into the
//! plane (the inverse, with the perspective divide), is tested against the area (the quad GL would
//! rasterise), and is shaded by its kind's mode as if untransformed.
//!
//! An affine transform (no perspective row) maps with two multiply-adds per coordinate and no divide.

use web_ui_render::plan::GpuClip;

use super::super::geometry::{xf_apply, xf_unapply};

/// Transform `index` (1-based) of the clip table, ready for per-pixel use.
pub struct Plane<'a> {
    clips: &'a [GpuClip],
    index: u32,
    /// The inverse as an affine map, when the transform has no perspective.
    affine: Option<[f32; 6]>,
    /// The instance's area in its plane (logical px: x, y, w, h).
    area: [f32; 4],
}

impl<'a> Plane<'a> {
    pub fn new(clips: &'a [GpuClip], index: u32, area: [f32; 4]) -> Plane<'a> {
        let e = &clips[index as usize - 1];
        // The inverse's rows are in `ry.yzw`, `bounds.xyz` and `bounds.w, info.xy`.
        let (e2, e3, e4) = (e.ry, e.bounds, e.info);
        let affine = (e3[3].abs() < 1e-9 && e4[0].abs() < 1e-9 && (e4[1] - 1.0).abs() < 1e-6)
            .then_some([e2[1], e2[2], e2[3], e3[0], e3[1], e3[2]]);
        Plane {
            clips,
            index,
            affine,
            area,
        }
    }

    /// A window point (logical px) in the plane.
    #[inline]
    pub fn local(&self, world: [f32; 2]) -> [f32; 2] {
        match self.affine {
            Some(m) => [
                m[0] * world[0] + m[1] * world[1] + m[2],
                m[3] * world[0] + m[4] * world[1] + m[5],
            ],
            None => xf_unapply(self.clips, self.index, world),
        }
    }

    /// Whether a plane point is inside the area GL would rasterise.
    #[inline]
    pub fn inside(&self, p: [f32; 2]) -> bool {
        let a = self.area;
        p[0] >= a[0] && p[0] < a[0] + a[2] && p[1] >= a[1] && p[1] < a[1] + a[3]
    }

    /// The area's bounds in the window (logical px: x0, y0, x1, y1).
    pub fn window_bounds(&self) -> [f32; 4] {
        let a = self.area;
        let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        for c in [
            [a[0], a[1]],
            [a[0] + a[2], a[1]],
            [a[0], a[1] + a[3]],
            [a[0] + a[2], a[1] + a[3]],
        ] {
            let w = xf_apply(self.clips, self.index, c);
            b = [
                b[0].min(w[0]),
                b[1].min(w[1]),
                b[2].max(w[0]),
                b[3].max(w[1]),
            ];
        }
        b
    }
}
