// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! sRGB transfer functions, as the GLES shaders write them (`shaders.rs`), and the tables the kernels
//! use instead of `powf` per channel: decoding a byte is one lookup, encoding a linear value one
//! lookup and a lerp (4096 steps, under a hundredth of a byte off).

use std::sync::OnceLock;

/// sRGB to linear, exactly as the shaders (`srgb_to_linear`).
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear to sRGB, exactly as the shaders (`linear_to_srgb`).
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

const ENCODE_STEPS: usize = 4096;

struct Tables {
    /// A byte (sRGB) as a linear value.
    decode: [f32; 256],
    /// `srgb_to_linear` at `ENCODE_STEPS + 1` points over 0..=1 (for values that aren't bytes).
    decode_f: Vec<f32>,
    /// `linear_to_srgb` at `ENCODE_STEPS + 1` points over 0..=1.
    encode: Vec<f32>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| Tables {
        decode: std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.0)),
        decode_f: (0..=ENCODE_STEPS)
            .map(|i| srgb_to_linear(i as f32 / ENCODE_STEPS as f32))
            .collect(),
        encode: (0..=ENCODE_STEPS)
            .map(|i| linear_to_srgb(i as f32 / ENCODE_STEPS as f32))
            .collect(),
    })
}

#[inline]
fn lookup(t: &[f32], c: f32) -> f32 {
    let x = c.clamp(0.0, 1.0) * ENCODE_STEPS as f32;
    let i = (x as usize).min(ENCODE_STEPS - 1);
    let f = x - i as f32;
    t[i] + (t[i + 1] - t[i]) * f
}

/// sRGB (0..=1, clamped) to linear through the table.
#[inline]
pub fn decode(c: f32) -> f32 {
    lookup(&tables().decode_f, c)
}

/// The decode table (byte to linear), for kernels that hold on to it in a loop.
pub fn decode_table() -> &'static [f32; 256] {
    &tables().decode
}

/// Linear (0..=1, clamped) to sRGB through the table.
#[inline]
pub fn encode(c: f32) -> f32 {
    lookup(&tables().encode, c)
}

/// A float channel (0..=1) as a byte, rounded as GL stores a UNORM8 value.
#[inline]
pub fn to_byte(c: f32) -> u8 {
    (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_agree_with_the_formulas() {
        for i in 0..=255u8 {
            let c = f32::from(i) / 255.0;
            assert!((decode_table()[usize::from(i)] - srgb_to_linear(c)).abs() < 1e-6);
            assert!(
                (encode(srgb_to_linear(c)) - c).abs() < 0.5 / 255.0,
                "byte {i}"
            );
        }
    }
}
