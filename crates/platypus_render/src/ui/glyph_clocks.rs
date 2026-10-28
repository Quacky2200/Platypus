// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The clock behind `text-effect` / `text-reveal`: when each glyph of an animated text element
//! was first shown, so a glyph ages in real seconds from its first frame and a pause in the
//! typing doesn't hold the animation of the glyphs already out.

use std::collections::HashMap;
use std::time::Instant;

/// When the glyphs of one text element were first shown (seconds on the effect clock; NaN: not
/// shown; negative infinity: shown all at once, settled).
#[derive(Default)]
pub(super) struct GlyphClock {
    touched: f32,
    pub shown_at: Vec<f32>,
}

/// The effect clock of a renderer and the [`GlyphClock`] of every animated text element (by UI node).
pub struct GlyphClocks {
    start: Instant,
    pinned: Option<f32>,
    clocks: HashMap<u32, GlyphClock>,
}

impl Default for GlyphClocks {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            pinned: None,
            clocks: HashMap::new(),
        }
    }
}

impl GlyphClocks {
    /// Pins the clock (seconds), for repeatable renders; `None` goes back to real time.
    pub fn set_time(&mut self, time: Option<f32>) {
        self.pinned = time;
    }

    /// Seconds on the effect clock.
    pub fn time(&self) -> f32 {
        self.pinned
            .unwrap_or_else(|| self.start.elapsed().as_secs_f32())
    }

    /// Forgets elements not drawn for a while.
    pub fn begin_frame(&mut self) {
        if self.clocks.len() > 32 {
            let now = self.time();
            self.clocks.retain(|_, c| now - c.touched < 30.0);
        }
    }

    /// Takes the clock of `node` (to read and update while its glyphs are placed); give it back
    /// with [`Self::put`].
    pub(super) fn take(&mut self, node: u32, now: f32) -> Vec<f32> {
        let clock = self.clocks.entry(node).or_default();
        clock.touched = now;
        std::mem::take(&mut clock.shown_at)
    }

    pub(super) fn put(&mut self, node: u32, shown_at: Vec<f32>) {
        self.clocks.entry(node).or_default().shown_at = shown_at;
    }
}

/// A fully saturated, light colour of hue `h` degrees (the rainbow text effects).
pub(super) fn hue_color(h: f32, alpha: f32) -> platypus_ui::Rgba {
    let k = |n: f32| {
        let k = (n + h / 30.0).rem_euclid(12.0);
        0.65 - 0.35 * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    platypus_ui::Rgba::new(k(0.0), k(8.0), k(4.0), alpha)
}
