// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The runs of pixels the modes hand over whole: a colour over a span (a box's inside, mode 1) and a
//! layer's row over a span at an opacity (a plain composite, mode 6). Most of a UI frame's pixels go
//! through these two loops, so they are written for the compiler to vectorise: no branches per
//! pixel, whole `[u8; 4]` pixels, floats only where GL has them. They are compiled once per
//! instruction set the kernels are (`kernels::simd`): SSE2 or AVX2 + FMA on x86-64, NEON on arm64 and
//! on armv7 built with it, plain code elsewhere.
//!
//! On an sRGB canvas the result is GL's exactly: GL blends in float and stores
//! `round(255 * (src + dst / 255 * (1 - src_a)))`, which is `round(255 * src + dst * (1 - src_a))`.

use super::super::canvas::{Space, blend_over};
use super::super::color::to_byte;

/// `c` (premultiplied, canvas space) over every pixel of `span`.
#[inline(always)]
pub fn over(span: &mut [u8], c: [f32; 4], space: Space) {
    let pixels = span.as_chunks_mut::<4>().0;
    match space {
        Space::Srgb => {
            if c[3] >= 1.0 {
                pixels.fill(c.map(to_byte));
                return;
            }
            let k = 1.0 - c[3];
            let add = c.map(|v| v * 255.0 + 0.5);
            for p in pixels {
                for ch in 0..4 {
                    p[ch] = (add[ch] + f32::from(p[ch]) * k).min(255.0) as u8;
                }
            }
        }
        Space::Linear => {
            for p in pixels {
                blend_over(p, c, space);
            }
        }
    }
}

/// `src` (a layer's pixels, the same length) times `opacity`, over `span`.
#[inline(always)]
pub fn over_scaled(span: &mut [u8], src: &[u8], opacity: f32, space: Space) {
    let pixels = span.as_chunks_mut::<4>().0;
    let src = src.as_chunks::<4>().0;
    match space {
        Space::Srgb => {
            let fade = opacity / 255.0;
            for (p, s) in pixels.iter_mut().zip(src) {
                let k = 1.0 - f32::from(s[3]) * fade;
                for ch in 0..4 {
                    p[ch] =
                        (f32::from(s[ch]) * opacity + f32::from(p[ch]) * k + 0.5).min(255.0) as u8;
                }
            }
        }
        Space::Linear => {
            for (p, s) in pixels.iter_mut().zip(src) {
                let c = space.load(*s).map(|v| v * opacity);
                blend_over(p, c, space);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_over_a_span_rounds_as_gl() {
        let mut span = vec![200u8, 100, 50, 255, 0, 0, 0, 0];
        let c = [0.25, 0.0, 0.0, 0.5];
        over(&mut span, c, Space::Srgb);
        let expect = |d: u8, s: f32| (s * 255.0 + f32::from(d) * 0.5).round() as u8;
        assert_eq!(
            span[..4],
            [
                expect(200, 0.25),
                expect(100, 0.0),
                expect(50, 0.0),
                expect(255, 0.5)
            ]
        );
        assert_eq!(span[4..], [expect(0, 0.25), 0, 0, expect(0, 0.5)]);
    }
}
