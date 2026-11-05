// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Color`: linear RGBA, as `js/engine`'s `Color` (same constants, same `hex` with the sRGB to
//! linear conversion). `Color.white`, `Color.hex("#ff0000")`, `new Color(r, g, b, a)`, `c.lerp(d, t)`.

use ts_api::{ValueType, ts_api};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl ValueType<4> for Rgba {
    fn from_parts(p: [f64; 4]) -> Self {
        Rgba {
            r: p[0],
            g: p[1],
            b: p[2],
            a: p[3],
        }
    }

    fn to_parts(self) -> [f64; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

const fn rgba(r: f64, g: f64, b: f64, a: f64) -> Rgba {
    Rgba { r, g, b, a }
}

fn gamma_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_gamma(c: f64) -> f64 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// From `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa`; sRGB, converted to linear (alpha as it is).
/// Anything unparsable reads as 0 per channel, like `Number.parseInt` giving NaN does not: the
/// engine's version yields NaN there; this one gives 0.
pub fn parse_hex(hex: &str) -> Rgba {
    let mut h: String = hex.strip_prefix('#').unwrap_or(hex).to_string();
    if h.len() == 3 || h.len() == 4 {
        h = h.chars().flat_map(|c| [c, c]).collect();
    }
    let n = |i: usize| -> f64 {
        h.get(i..i + 2)
            .and_then(|s| u8::from_str_radix(s, 16).ok())
            .map_or(0.0, |v| f64::from(v) / 255.0)
    };
    let lin = |i: usize| gamma_to_linear(n(i));
    rgba(
        lin(0),
        lin(2),
        lin(4),
        if h.len() >= 8 { n(6) } else { 1.0 },
    )
}

fn channel_hex(v: f64) -> String {
    format!("{:02x}", (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

#[ts_api(namespace = "Color", values(Color = Rgba [r: f64, g: f64, b: f64, a: f64]))]
pub mod color_api {
    use super::{Rgba, channel_hex, gamma_to_linear, linear_to_gamma, parse_hex, rgba};

    #[property]
    pub fn white() -> Rgba {
        rgba(1.0, 1.0, 1.0, 1.0)
    }

    #[property]
    pub fn black() -> Rgba {
        rgba(0.0, 0.0, 0.0, 1.0)
    }

    #[property]
    pub fn red() -> Rgba {
        rgba(1.0, 0.0, 0.0, 1.0)
    }

    #[property]
    pub fn green() -> Rgba {
        rgba(0.0, 1.0, 0.0, 1.0)
    }

    #[property]
    pub fn blue() -> Rgba {
        rgba(0.0, 0.0, 1.0, 1.0)
    }

    #[property]
    pub fn yellow() -> Rgba {
        rgba(1.0, 0.92, 0.016, 1.0)
    }

    #[property]
    pub fn clear() -> Rgba {
        rgba(0.0, 0.0, 0.0, 0.0)
    }

    /// From a `#rgb`, `#rgba`, `#rrggbb` or `#rrggbbaa` hex string. Hex colours are sRGB (as colour
    /// pickers and CSS write them), so they are converted to linear; alpha stays as is.
    pub fn hex(text: &str) -> Rgba {
        parse_hex(text)
    }

    /// From sRGB components 0 to 1, converted to linear.
    pub fn from_srgb(r: f64, g: f64, b: f64, a: f64) -> Rgba {
        rgba(
            gamma_to_linear(r),
            gamma_to_linear(g),
            gamma_to_linear(b),
            a,
        )
    }

    /// The colour as sRGB hex, `#rrggbbaa` (the inverse of `hex`).
    #[method(Color)]
    pub fn to_hex(c: Rgba) -> String {
        let g = |v: f64| channel_hex(linear_to_gamma(v.clamp(0.0, 1.0)));
        format!("#{}{}{}{}", g(c.r), g(c.g), g(c.b), channel_hex(c.a))
    }

    /// Blends towards `to` by `t` (clamped to 0..1).
    #[method(Color)]
    pub fn lerp(c: Rgba, to: Rgba, t: f64) -> Rgba {
        let k = t.clamp(0.0, 1.0);
        rgba(
            c.r + (to.r - c.r) * k,
            c.g + (to.g - c.g) * k,
            c.b + (to.b - c.b) * k,
            c.a + (to.a - c.a) * k,
        )
    }

    #[method(Color)]
    pub fn with_alpha(c: Rgba, a: f64) -> Rgba {
        rgba(c.r, c.g, c.b, a)
    }
}
