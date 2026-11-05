// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! **Mode 7, Blur: the separable Gaussian** of `Step::Blur` (`backdrop-filter: blur()`, `filter:
//! blur()` on layers, blurred drop shadows), as the GLES executor runs it: horizontally from the
//! texture into `temp` over the rows `blur_reach` says, then vertically back into the texture over
//! the area, every read clamped to the same bounds (the whole canvas, or a backdrop filter's own box).
//! With linear blur on an sRGB canvas (Fluid's default) the average is taken in linear light.
//!
//! The Gaussian is the one difference in method. GLES samples it directly: `2r + 1` taps per pixel
//! per pass with `r = 3 sigma` (up to 128), which is what makes a large frosted-glass area cost tens of
//! milliseconds on llvmpipe. Here a small blur (`r <= EXACT_RADIUS`) uses the same taps; a larger one
//! uses three box passes whose widths are chosen from sigma (Kovesi, "Fast almost-Gaussian
//! filtering"), each a running sum, so the cost per pixel is the same at any radius. The result is a
//! Gaussian to within a few percent at the tails, which a frosted backdrop doesn't show.

use platypus_render::plan::PxRect;

use super::super::canvas::{Canvas, Rows, Space};
use super::super::color::{decode, encode};

/// The largest radius drawn with exact Gaussian taps.
const EXACT_RADIUS: u32 = 8;
/// Columns a vertical pass decodes at a time (bounds its scratch memory).
const STRIP: u32 = 256;

/// How the blur is sampled.
pub enum Kernel {
    /// Gaussian weights for offsets `-r..=r`, normalised.
    Taps(Vec<f32>),
    /// Three box radii (box widths `2r + 1`).
    Boxes([u32; 3]),
}

impl Kernel {
    /// How far a pass reads either side.
    fn reach(&self) -> u32 {
        match self {
            Kernel::Taps(w) => (w.len() as u32 - 1) / 2,
            Kernel::Boxes(r) => r.iter().sum(),
        }
    }
}

/// The kernel for `sigma` (px) and the GLES radius `radius`.
pub fn kernel(sigma: f32, radius: u32) -> Kernel {
    let sigma = sigma.max(0.01);
    if radius <= EXACT_RADIUS {
        let r = radius as i32;
        let mut w: Vec<f32> = (-r..=r)
            .map(|i| (-((i * i) as f32) / (2.0 * sigma * sigma)).exp())
            .collect();
        let sum: f32 = w.iter().sum();
        w.iter_mut().for_each(|v| *v /= sum);
        return Kernel::Taps(w);
    }
    // Box widths whose three passes have the Gaussian's variance (Kovesi): `m` boxes of the
    // lower odd width `wl`, the rest `wl + 2`.
    let n = 3.0f32;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut wl = ideal.floor() as i32;
    if wl % 2 == 0 {
        wl -= 1;
    }
    let wl = wl.max(1);
    let wu = wl + 2;
    let m = ((12.0 * sigma * sigma - n * (wl * wl) as f32 - 4.0 * n * wl as f32 - 3.0 * n)
        / (-4.0 * wl as f32 - 4.0))
        .round() as i32;
    let radii = std::array::from_fn(|i| {
        let w = if (i as i32) < m { wl } else { wu };
        ((w - 1) / 2) as u32
    });
    Kernel::Boxes(radii)
}

/// One blur step, ready to run in bands.
pub struct Blur {
    pub area: PxRect,
    /// The rows the horizontal pass writes.
    pub tall: PxRect,
    /// Reads stay inside these pixels (first x, first y, last x, last y).
    pub bounds: [u32; 4],
    /// Reads past the bounds reflect back in (a `backdrop-filter`'s box, as Chrome and the GLES
    /// executor's `u_mirror`); otherwise they clamp.
    pub mirror: bool,
    pub kernel: Kernel,
    /// Average in linear light on an sRGB canvas.
    pub linear: bool,
    pub space: Space,
}

/// Where a read at `v` lands with bounds `lo..=hi`: reflected back in first when `mirror` (as the GLES
/// shader: `2 lo - 1 - v` below, `2 hi + 1 - v` above), then clamped.
#[inline]
fn edge(v: i64, lo: i64, hi: i64, mirror: bool) -> u32 {
    let mut v = v;
    if mirror {
        if v < lo {
            v = 2 * lo - 1 - v;
        }
        if v > hi {
            v = 2 * hi + 1 - v;
        }
    }
    v.clamp(lo, hi) as u32
}

impl Blur {
    /// A canvas pixel as the value the blur averages (premultiplied; linear light when `linear`).
    #[inline]
    fn decode(&self, p: [u8; 4]) -> [f32; 4] {
        let c = self.space.load(p);
        if !self.linear || c[3] <= 0.0 {
            return c;
        }
        let a = c[3];
        [
            decode(c[0] / a) * a,
            decode(c[1] / a) * a,
            decode(c[2] / a) * a,
            a,
        ]
    }

    #[inline]
    fn encode(&self, c: [f32; 4]) -> [u8; 4] {
        if !self.linear || c[3] <= 0.0 {
            return self.space.store(c);
        }
        let a = c[3];
        self.space.store([
            encode(c[0] / a) * a,
            encode(c[1] / a) * a,
            encode(c[2] / a) * a,
            a,
        ])
    }

    /// The horizontal pass for `tall`'s rows `y0..y1`: from `src` into `temp`.
    ///
    /// # Safety
    /// No other thread may touch `temp`'s rows `y0..y1` meanwhile.
    pub unsafe fn horizontal(&self, src: &Canvas, temp: Rows, y0: u32, y1: u32) {
        let reach = self.kernel.reach();
        let a = self.area;
        let len = (a.w + 2 * reach) as usize;
        let mut line = vec![[0.0f32; 4]; len];
        let mut spare = vec![[0.0f32; 4]; len];
        let [bx0, by0, bx1, by1] = self.bounds;
        for y in y0..y1 {
            let sy = y.clamp(by0, by1);
            for (k, v) in line.iter_mut().enumerate() {
                let x = edge(
                    i64::from(a.x) - i64::from(reach) + k as i64,
                    i64::from(bx0),
                    i64::from(bx1),
                    self.mirror,
                );
                *v = self.decode(src.pixel(x, sy));
            }
            let out = convolve(&self.kernel, &mut line, &mut spare);
            // SAFETY: this band's own row (the caller's contract).
            let row = unsafe { temp.row(y) };
            for (k, c) in out[reach as usize..reach as usize + a.w as usize]
                .iter()
                .enumerate()
            {
                let at = (a.x as usize + k) * 4;
                row[at..at + 4].copy_from_slice(&self.encode(*c));
            }
        }
    }

    /// The vertical pass for the area's rows `y0..y1`: from `temp` into `dst`.
    ///
    /// # Safety
    /// No other thread may touch `dst`'s rows `y0..y1` meanwhile.
    pub unsafe fn vertical(&self, temp: &Canvas, dst: Rows, y0: u32, y1: u32) {
        let reach = self.kernel.reach();
        let a = self.area;
        let [_, by0, _, by1] = self.bounds;
        // Reads stay within the rows the horizontal pass wrote (reflected at the bounds first).
        let (lo, hi) = (
            i64::from(by0.max(self.tall.y)),
            i64::from(by1.min(self.tall.y + self.tall.h - 1)),
        );
        let (b0, b1) = (i64::from(by0), i64::from(by1));
        let rows = (y1 - y0 + 2 * reach) as usize;
        let mut x0 = a.x;
        while x0 < a.x + a.w {
            let width = STRIP.min(a.x + a.w - x0) as usize;
            let mut block = vec![[0.0f32; 4]; rows * width];
            for r in 0..rows {
                let at = i64::from(y0) - i64::from(reach) + r as i64;
                let y = (i64::from(edge(at, b0, b1, self.mirror))).clamp(lo, hi) as u32;
                for k in 0..width {
                    block[r * width + k] = self.decode(temp.pixel(x0 + k as u32, y));
                }
            }
            let out = convolve_columns(&self.kernel, block, rows, width);
            for y in y0..y1 {
                // SAFETY: this band's own row (the caller's contract).
                let row = unsafe { dst.row(y) };
                let r = (y - y0 + reach) as usize;
                for k in 0..width {
                    let at = (x0 as usize + k) * 4;
                    row[at..at + 4].copy_from_slice(&self.encode(out[r * width + k]));
                }
            }
            x0 += width as u32;
        }
    }
}

/// One box pass along `line` (radius `r`), into `out`; reads past the ends repeat the end values.
fn box_pass(line: &[[f32; 4]], out: &mut [[f32; 4]], r: usize) {
    let n = line.len();
    let width = (2 * r + 1) as f32;
    let at = |i: isize| line[i.clamp(0, n as isize - 1) as usize];
    let mut sum = [0.0f32; 4];
    for i in -(r as isize)..=(r as isize) {
        let v = at(i);
        for k in 0..4 {
            sum[k] += v[k];
        }
    }
    for (i, o) in out.iter_mut().enumerate() {
        *o = sum.map(|s| s / width);
        let (add, sub) = (at(i as isize + r as isize + 1), at(i as isize - r as isize));
        for k in 0..4 {
            sum[k] += add[k] - sub[k];
        }
    }
}

/// The kernel along `line` (which reads `reach` either side of what is kept); returns the result,
/// in `line` or `spare`.
fn convolve<'a>(
    kernel: &Kernel,
    line: &'a mut [[f32; 4]],
    spare: &'a mut [[f32; 4]],
) -> &'a [[f32; 4]] {
    match kernel {
        Kernel::Taps(w) => {
            let r = (w.len() - 1) / 2;
            let n = line.len();
            for i in r..n.saturating_sub(r) {
                let mut s = [0.0f32; 4];
                for (t, wt) in w.iter().enumerate() {
                    let v = line[i + t - r];
                    for k in 0..4 {
                        s[k] += v[k] * wt;
                    }
                }
                spare[i] = s;
            }
            spare
        }
        Kernel::Boxes(radii) => {
            box_pass(line, spare, radii[0] as usize);
            box_pass(spare, line, radii[1] as usize);
            box_pass(line, spare, radii[2] as usize);
            spare
        }
    }
}

/// The kernel down the columns of `block` (`rows` x `width`, row-major), running along x so the
/// inner loops are contiguous.
fn convolve_columns(
    kernel: &Kernel,
    block: Vec<[f32; 4]>,
    rows: usize,
    width: usize,
) -> Vec<[f32; 4]> {
    match kernel {
        Kernel::Taps(w) => {
            let r = (w.len() - 1) / 2;
            let mut out = vec![[0.0f32; 4]; rows * width];
            for y in r..rows.saturating_sub(r) {
                let dst = &mut out[y * width..(y + 1) * width];
                for (t, wt) in w.iter().enumerate() {
                    let src = &block[(y + t - r) * width..(y + t - r + 1) * width];
                    for (d, s) in dst.iter_mut().zip(src) {
                        for k in 0..4 {
                            d[k] += s[k] * wt;
                        }
                    }
                }
            }
            out
        }
        Kernel::Boxes(radii) => {
            let mut a = block;
            let mut b = vec![[0.0f32; 4]; rows * width];
            for r in radii {
                box_columns(&a, &mut b, rows, width, *r as usize);
                std::mem::swap(&mut a, &mut b);
            }
            a
        }
    }
}

/// One box pass down the columns (radius `r`), as `box_pass` per column.
fn box_columns(src: &[[f32; 4]], out: &mut [[f32; 4]], rows: usize, width: usize, r: usize) {
    let row = |i: isize| &src[i.clamp(0, rows as isize - 1) as usize * width..][..width];
    let k = 1.0 / (2 * r + 1) as f32;
    let mut sum = vec![[0.0f32; 4]; width];
    for i in -(r as isize)..=(r as isize) {
        for (s, v) in sum.iter_mut().zip(row(i)) {
            for c in 0..4 {
                s[c] += v[c];
            }
        }
    }
    for y in 0..rows {
        for (o, s) in out[y * width..(y + 1) * width].iter_mut().zip(&sum) {
            *o = s.map(|v| v * k);
        }
        let (add, sub) = (
            row(y as isize + r as isize + 1),
            row(y as isize - r as isize),
        );
        for ((s, a), b) in sum.iter_mut().zip(add).zip(sub) {
            for c in 0..4 {
                s[c] += a[c] - b[c];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_boxes_have_about_the_gaussians_spread() {
        for sigma in [4.0f32, 10.0, 30.0] {
            let Kernel::Boxes(r) = kernel(sigma, (3.0 * sigma).ceil() as u32) else {
                panic!("a large blur uses boxes");
            };
            // A box of width w has variance (w^2 - 1) / 12; the three add up.
            let var: f32 = r
                .iter()
                .map(|r| ((2 * r + 1).pow(2) - 1) as f32 / 12.0)
                .sum();
            assert!(
                (var.sqrt() - sigma).abs() / sigma < 0.08,
                "sigma {sigma}: {r:?}"
            );
        }
        assert!(matches!(kernel(2.0, 6), Kernel::Taps(_)));
    }

    #[test]
    fn a_box_pass_keeps_a_flat_line_flat() {
        let line = vec![[0.5f32; 4]; 40];
        let mut out = vec![[0.0f32; 4]; 40];
        box_pass(&line, &mut out, 5);
        assert!(out.iter().all(|v| (v[0] - 0.5).abs() < 1e-6));
    }
}
