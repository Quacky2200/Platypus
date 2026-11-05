// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The seven kernel families the frame plan is drawn with. What llvmpipe compiles at run time for a
//! UI app (about 24 functions: variants of one fragment shader, texture samplers, triangle setup,
//! docs in Fluid's `docs/cpu-rendering.md`) comes down to these, written once and compiled ahead of
//! time, with the plan's state as arguments instead of a variant per state:
//!
//! | mode | file | draws | the shader it ports |
//! |---|---|---|---|
//! | 1 Fill | [`fill`] | boxes (colour, gradient, solid border), styled borders, wavy lines, gradient pieces | `shade` (quad fragment) |
//! | 2 Shadow | [`shadow`] | outer and inset box shadows | `blurred_box` |
//! | 3 Glyph | [`glyph`] | text from the glyph atlas (mask, colour, gradient) | `glyph` |
//! | 4 Image | [`image`] | background and border images (bilinear or nearest) | `IMAGE_MAIN` |
//! | 5 Transform | [`transform`] | any of 1-4 in a transformed plane (affine or perspective) | the vertex shader's homography |
//! | 6 Composite | [`composite`] | layers: opacity, clip, colour matrix, drop shadow, mask, blend modes | `effects.rs` composites |
//! | 7 Blur | [`blur`] | the separable blur of layers and backdrop filters | `BLUR_FRAGMENT` |
//!
//! Modes 1-5 run inside a `Step::Draw` ([`draw`]): every instance of the step is prepared once (its
//! pixels, its mode, whether its clip can be skipped), then the target's rows are split into bands
//! that the process's pool (`plan::pool`) draws in parallel, each band drawing every instance that
//! crosses it, in the plan's order. A band never writes another band's rows, so the order per pixel
//! is the plan's.
//!
//! **Instruction sets.** The kernels are plain Rust (no intrinsics), so every target builds and
//! vectorises them its own way. On x86-64 the band loop is compiled twice, for the baseline (SSE2) and
//! for AVX2 with FMA, and the second is chosen at run time where the processor has it ([`simd`]);
//! arm64 always has NEON and armv7 has it when built with `+neon`, so their one build is already
//! vectorised. `PLATYPUS_CPU_SIMD=off` forces the baseline build (to compare).

pub mod blur;
pub mod composite;
pub mod fill;
pub mod glyph;
pub mod image;
pub mod shadow;
pub mod spans;
pub mod transform;

use std::collections::HashMap;

use platypus_render::plan::{
    Instance, KIND_BORDER, KIND_GLYPH, KIND_GRADIENT_PIECE, KIND_INSET_SHADOW, KIND_OUTER_SHADOW,
    KIND_QUAD, KIND_WAVY, PxRect,
};

use self::image::Image;
use self::transform::Plane;
use super::canvas::{Rows, Space, blend_over};
use super::geometry::{Frame, clip_coverage, clip_holds, clip_is_convex};
use super::pool::Pool;

/// What the kernels sample besides canvases: the glyph pages and the decoded images.
pub struct Textures {
    /// The coverage page (`MASK_SIZE` squared, one byte a texel).
    pub glyph_mask: Vec<u8>,
    /// The colour page (`COLOR_SIZE` squared, straight sRGB RGBA), made with the first colour glyph.
    pub glyph_color: Option<Vec<u8>>,
    pub images: HashMap<String, Image>,
}

/// How an instance is drawn: its mode, and whether runs of its rows can be handed over whole.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    /// Mode 1, a box. `span`: its inside is one colour, filled a run at a time.
    Box {
        span: bool,
    },
    Border,
    Wavy,
    GradientPiece,
    /// Mode 2. The inside it doesn't draw is skipped a run at a time.
    OuterShadow,
    InsetShadow,
    /// Mode 3.
    Glyph,
    /// Mode 4 (a `Step::Draw` with an image).
    Image,
}

/// An instance ready to draw.
struct Prepared<'a> {
    inst: &'a Instance,
    /// Pixels: first x, first y, end x, end y.
    bounds: [u32; 4],
    route: Route,
    /// Clip index to apply (0: none, or it holds the whole instance).
    clip: u32,
    /// Mode 5: the plane the instance is in.
    plane: Option<Plane<'a>>,
}

/// What one `Step::Draw` reads.
pub struct DrawStep<'a> {
    pub frame: &'a Frame<'a>,
    pub space: Space,
    pub textures: &'a Textures,
    pub instances: &'a [Instance],
    /// The image of an image draw, and whether it is sampled nearest.
    pub image: Option<(&'a Image, bool)>,
    /// The canvas's size (physical px).
    pub size: (u32, u32),
}

/// Rows per band of a draw: small, so a draw of a few hundred rows still spreads over the pool.
pub const BAND: u32 = 16;
/// Below this many pixels a step runs on the calling thread: waking the pool (tens of microseconds) costs more.
pub const PARALLEL_PIXELS: u64 = 4_096;

impl<'a> DrawStep<'a> {
    fn prepare(&self, inst: &'a Instance) -> Option<Prepared<'a>> {
        let s = self.frame.scale;
        let kind = inst.params[0];
        // As the vertex shader: one physical pixel for anti-aliasing, the blur's reach for outer
        // shadows, nothing for glyphs (pixel-aligned atlas images).
        let mut grow = 1.0 / s;
        if kind == KIND_OUTER_SHADOW {
            grow += 3.0 * inst.params[1];
        }
        if kind == KIND_GLYPH {
            grow = 0.0;
        }
        let r = inst.rect;
        let mut area = [
            r[0] - grow,
            r[1] - grow,
            r[2] + 2.0 * grow,
            r[3] + 2.0 * grow,
        ];
        let clip = inst.params[3] as u32;
        let xf = inst.xf[0] as u32;
        if clip > 0 && xf == 0 {
            // Don't rasterise what the clip removes anyway.
            let b = self.frame.clips[clip as usize - 1].bounds;
            let lo = [area[0].max(b[0] - grow), area[1].max(b[1] - grow)];
            let hi = [
                lo[0].max((area[0] + area[2]).min(b[0] + b[2] + grow)),
                lo[1].max((area[1] + area[3]).min(b[1] + b[3] + grow)),
            ];
            area = [lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1]];
        }
        let (plane, window) = if xf > 0 {
            let plane = Plane::new(self.frame.clips, xf, area);
            let b = plane.window_bounds();
            (Some(plane), b)
        } else {
            (
                None,
                [area[0], area[1], area[0] + area[2], area[1] + area[3]],
            )
        };
        // Pixels whose centres are inside (GL's rule for the quad's edges).
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let px = |v: f32, max: f32| (v * s - 0.5).ceil().clamp(0.0, max) as u32;
        let bounds = [
            px(window[0], w),
            px(window[1], h),
            px(window[2], w),
            px(window[3], h),
        ];
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            return None;
        }
        let route = if self.image.is_some() {
            Route::Image
        } else if kind == KIND_OUTER_SHADOW {
            Route::OuterShadow
        } else if kind == KIND_INSET_SHADOW {
            Route::InsetShadow
        } else if kind == KIND_GLYPH {
            Route::Glyph
        } else if kind == KIND_BORDER {
            Route::Border
        } else if kind == KIND_WAVY {
            Route::Wavy
        } else if kind == KIND_GRADIENT_PIECE {
            Route::GradientPiece
        } else {
            debug_assert_eq!(kind, KIND_QUAD);
            Route::Box {
                span: clip_is_convex(self.frame.clips, clip),
            }
        };
        // A clip that holds every pixel the instance draws needn't be computed per pixel.
        let logical = [
            bounds[0] as f32 / s,
            bounds[1] as f32 / s,
            bounds[2] as f32 / s,
            bounds[3] as f32 / s,
        ];
        let clip = if clip > 0 && clip_holds(self.frame.clips, clip, logical, s) {
            0
        } else {
            clip
        };
        Some(Prepared {
            inst,
            bounds,
            route,
            clip,
            plane,
        })
    }

    /// One pixel of a prepared instance: its colour (premultiplied, canvas space) and whether it is
    /// in the run the instance's route hands over whole; `None` where GL draws nothing.
    #[inline(always)]
    fn eval(&self, p: &Prepared, x: u32, y: u32) -> Option<([f32; 4], bool)> {
        let f = self.frame;
        let world = [(x as f32 + 0.5) / f.scale, (y as f32 + 0.5) / f.scale];
        let local = match &p.plane {
            Some(plane) => {
                let l = plane.local(world);
                if !plane.inside(l) {
                    return None;
                }
                l
            }
            None => world,
        };
        let i = p.inst;
        if p.route == Route::Image {
            let (img, nearest) = self.image?;
            // The image program reads the clip itself (and the box as a clip).
            return image::image(f, img, nearest, i, world, local).map(|c| (c, false));
        }
        let clip = if p.clip > 0 {
            clip_coverage(f, world, p.clip)
        } else {
            1.0
        };
        if clip <= 0.0 {
            return None;
        }
        let (c, inside) = match p.route {
            Route::Box { .. } => {
                let (c, inside) = fill::quad(f, i, local);
                (c, inside && clip >= 1.0)
            }
            Route::Border => (fill::styled_border(f, i, local), false),
            Route::Wavy => (fill::wavy(f, i, local), false),
            Route::GradientPiece => (fill::gradient_piece(f, i, local), false),
            // What a shadow skips is empty whatever the clip.
            Route::OuterShadow => shadow::outer(f, i, local),
            Route::InsetShadow => shadow::inset(f, i, local),
            Route::Glyph => (glyph::glyph(f, self.textures, i, local)?, false),
            Route::Image => unreachable!("handled above"),
        };
        Some((c.map(|v| v * clip), inside))
    }

    /// Draws `p`'s pixels on row `y` (columns `x0..x1`).
    #[inline(always)]
    fn row(&self, p: &Prepared, row: &mut [u8], y: u32, x0: u32, x1: u32) {
        let space = self.space;
        let mut put = |x: u32, c: [f32; 4]| {
            let at = x as usize * 4;
            blend_over(&mut row[at..at + 4], c, space);
        };
        // The routes whose inside is one run: scan in from both ends to it, then hand it over.
        let middle = match p.route {
            Route::Box { span: true } => Some(true),
            Route::OuterShadow | Route::InsetShadow => Some(false),
            _ => None,
        };
        let Some(fills) = middle else {
            for x in x0..x1 {
                if let Some((c, _)) = self.eval(p, x, y) {
                    put(x, c);
                }
            }
            return;
        };
        let mut left = x0;
        while left < x1 {
            match self.eval(p, left, y) {
                Some((_, true)) => break,
                Some((c, false)) => put(left, c),
                None => {}
            }
            left += 1;
        }
        if left == x1 {
            return;
        }
        let mut right = x1;
        while right > left + 1 {
            match self.eval(p, right - 1, y) {
                Some((_, true)) => break,
                Some((c, false)) => put(right - 1, c),
                None => {}
            }
            right -= 1;
        }
        if !fills {
            return;
        }
        // A box's inside is its background (`fill::quad`): one colour, or its gradient per pixel without the shape.
        if p.inst.params[2] < 0.0 {
            spans::over(
                &mut row[left as usize * 4..right as usize * 4],
                p.inst.color0,
                space,
            );
            return;
        }
        let s = self.frame.scale;
        for x in left..right {
            let world = [(x as f32 + 0.5) / s, (y as f32 + 0.5) / s];
            let local = p.plane.as_ref().map_or(world, |plane| plane.local(world));
            let at = x as usize * 4;
            blend_over(
                &mut row[at..at + 4],
                fill::background(self.frame, p.inst, local),
                space,
            );
        }
    }

    /// Draws every prepared instance's part of rows `y0..y1`.
    #[inline(always)]
    fn band(&self, prepared: &[Prepared], rows: Rows, y0: u32, y1: u32) {
        for p in prepared {
            let (ya, yb) = (p.bounds[1].max(y0), p.bounds[3].min(y1));
            for y in ya..yb {
                // SAFETY: rows `y0..y1` are this band's alone (`draw`).
                let row = unsafe { rows.row(y) };
                self.row(p, row, y, p.bounds[0], p.bounds[2]);
            }
        }
    }
}

/// The instruction-set builds of the band loop (see the module docs).
mod simd {
    use super::*;

    /// Whether to run the AVX2 + FMA build (x86-64 only; asked once).
    #[cfg(target_arch = "x86_64")]
    pub fn wide() -> bool {
        static WIDE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *WIDE.get_or_init(|| {
            std::env::var("PLATYPUS_CPU_SIMD").as_deref() != Ok("off")
                && std::arch::is_x86_feature_detected!("avx2")
                && std::arch::is_x86_feature_detected!("fma")
        })
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2,fma")]
    fn band_avx2(step: &DrawStep, prepared: &[Prepared], rows: Rows, y0: u32, y1: u32) {
        step.band(prepared, rows, y0, y1);
    }

    /// Draws rows `y0..y1` with the best build this processor runs.
    pub fn band(step: &DrawStep, prepared: &[Prepared], rows: Rows, y0: u32, y1: u32) {
        #[cfg(target_arch = "x86_64")]
        if wide() {
            // SAFETY: the processor has AVX2 and FMA (checked in `wide`).
            unsafe { band_avx2(step, prepared, rows, y0, y1) };
            return;
        }
        step.band(prepared, rows, y0, y1);
    }

    /// Which build runs, for logs.
    pub fn name() -> &'static str {
        #[cfg(target_arch = "x86_64")]
        if wide() {
            return "x86-64 AVX2+FMA";
        }
        if cfg!(target_arch = "x86_64") {
            "x86-64 SSE2"
        } else if cfg!(target_arch = "aarch64") {
            "arm64 NEON"
        } else if cfg!(all(target_arch = "arm", target_feature = "neon")) {
            "armv7 NEON"
        } else {
            "portable"
        }
    }
}

pub use simd::name as simd_name;

/// Draws a `Step::Draw` into `rows` (a canvas of `step.size`); returns the pixels it may have
/// written.
pub fn draw(step: &DrawStep, rows: Rows, pool: &Pool) -> Option<PxRect> {
    let prepared: Vec<Prepared> = step
        .instances
        .iter()
        .filter_map(|i| step.prepare(i))
        .collect();
    let mut b = [u32::MAX, u32::MAX, 0, 0];
    let mut pixels = 0u64;
    for p in &prepared {
        b = [
            b[0].min(p.bounds[0]),
            b[1].min(p.bounds[1]),
            b[2].max(p.bounds[2]),
            b[3].max(p.bounds[3]),
        ];
        pixels += u64::from(p.bounds[2] - p.bounds[0]) * u64::from(p.bounds[3] - p.bounds[1]);
    }
    if prepared.is_empty() {
        return None;
    }
    if pixels < PARALLEL_PIXELS || pool.threads() == 1 {
        simd::band(step, &prepared, rows, b[1], b[3]);
    } else {
        let bands = (b[3] - b[1]).div_ceil(BAND) as usize;
        pool.run(bands, &|i| {
            let y0 = b[1] + i as u32 * BAND;
            simd::band(step, &prepared, rows, y0, (y0 + BAND).min(b[3]));
        });
    }
    Some(PxRect {
        x: b[0],
        y: b[1],
        w: b[2] - b[0],
        h: b[3] - b[1],
    })
}
