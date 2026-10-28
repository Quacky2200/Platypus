// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `colorSpace` in `project.json`: `linear` (the default) means the colours a game gives world
//! objects (sprites, materials, lights, text meshes, lines, particles, the camera's background)
//! are linear light values, as the renderer works in. `srgb` means they are authored the way
//! Unity's inspector and most art tools show them (gamma encoded), and are converted to linear on
//! their way to the renderer, so ported games look the way they did. UI colours (CSS) are always
//! sRGB.

use platypus_engine_api::color::Rgba;
use std::sync::atomic::{AtomicBool, Ordering};

static SRGB: AtomicBool = AtomicBool::new(false);

/// Whether world colours are authored as sRGB.
pub fn set_srgb(on: bool) {
    SRGB.store(on, Ordering::Relaxed);
}

/// One channel as the renderer wants it.
pub fn linear(v: f64) -> f64 {
    if !SRGB.load(Ordering::Relaxed) {
        return v;
    }
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// A colour as the renderer wants it: red, green, blue converted, alpha as it is.
pub fn rgba(c: Rgba) -> [f64; 4] {
    [linear(c.r), linear(c.g), linear(c.b), c.a]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static SERIAL: Mutex<()> = Mutex::new(());

    // RDY-RND-01
    #[test]
    fn srgb_colours_are_converted_to_linear_and_alpha_is_not() {
        let _guard = SERIAL.lock().unwrap();
        let c = Rgba {
            r: 0.5,
            g: 1.0,
            b: 0.0,
            a: 0.5,
        };
        set_srgb(false);
        assert_eq!(rgba(c), [0.5, 1.0, 0.0, 0.5]);
        set_srgb(true);
        let l = rgba(c);
        assert!((l[0] - 0.214).abs() < 1e-3, "{l:?}");
        assert_eq!((l[1], l[2], l[3]), (1.0, 0.0, 0.5));
        set_srgb(false);
    }
}
