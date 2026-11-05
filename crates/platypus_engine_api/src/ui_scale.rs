// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `UiScale` (s&box's ScreenPanel scaling): screen UI is laid out in "UI pixels" and drawn bigger or
//! smaller, so a menu designed for 1080p fills a 4K window the same. Scripts pick a mode
//! (`UiScale.height(1080)`); the host reads it with [`current`] and sizes the screen UI by
//! [`UiScale::factor`] on every renderer. It goes back to [`UiScale::None`] on a reload ([`reset`]).

use std::cell::Cell;
use ts_api::ts_api;

/// How screen UI is scaled to the window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum UiScale {
    /// One UI pixel per logical (DPI-scaled) pixel.
    #[default]
    None,
    /// A fixed zoom.
    Fixed(f32),
    /// Scale so the window is this many UI pixels high (s&box targets 1080).
    Height(f32),
    /// Scale so a `width` × `height` design fits inside the window.
    Fit(f32, f32),
}

/// Never shrink UI below a quarter or grow it past 8×: a tiny window shouldn't make text
/// unreadable or a huge factor explode the glyph atlas.
const LIMITS: (f32, f32) = (0.25, 8.0);

impl UiScale {
    /// UI pixels → logical pixels, for a window `width` × `height` logical pixels. A minimised
    /// (zero-sized) window gets 1.
    pub fn factor(self, width: f32, height: f32) -> f32 {
        let f = match self {
            UiScale::None => 1.0,
            UiScale::Fixed(f) => f,
            UiScale::Height(h) => height / h,
            UiScale::Fit(w, h) => (width / w).min(height / h),
        };
        if f.is_finite() && f > 0.0 {
            f.clamp(LIMITS.0, LIMITS.1)
        } else {
            1.0
        }
    }
}

thread_local! {
    static CURRENT: Cell<UiScale> = const { Cell::new(UiScale::None) };
}

/// The mode scripts set (the host sizes the screen UI by it).
pub fn current() -> UiScale {
    CURRENT.with(Cell::get)
}

/// Back to one UI pixel per logical pixel (a test, or a reload).
pub fn reset() {
    set_current(UiScale::None);
}

/// Sets the mode from the host (a failed reload puts the old code's back).
pub fn set_current(mode: UiScale) {
    CURRENT.with(|c| c.set(mode));
}

/// A positive, finite size, or none.
fn positive(v: f64) -> Option<f32> {
    let v = v as f32;
    (v.is_finite() && v > 0.0).then_some(v)
}

/// Sets the mode when every size is valid; a zero, negative or NaN size leaves it as it was.
fn set(mode: Option<UiScale>) {
    if let Some(mode) = mode {
        set_current(mode);
    }
}

/// How screen UI is scaled to the window: laid out in UI pixels, drawn bigger or smaller, so a
/// design for one size fills any window the same (s&box's ScreenPanel scaling). The result is
/// clamped between a quarter and 8 times; `Screen.width` / `height` are in UI pixels.
#[ts_api(namespace = "UiScale")]
pub mod ui_scale_api {
    use super::{UiScale, positive, set};

    /// One UI pixel per logical (DPI-scaled) pixel: no scaling (the default).
    pub fn none() {
        set(Some(UiScale::None));
    }

    /// A fixed zoom: every UI pixel is `factor` logical pixels.
    pub fn fixed(factor: f64) {
        set(positive(factor).map(UiScale::Fixed));
    }

    /// Scales so the window is `pixels` UI pixels high (a design for 1080p: `UiScale.height(1080)`).
    pub fn height(pixels: f64) {
        set(positive(pixels).map(UiScale::Height));
    }

    /// Scales so a `width` × `height` design fits inside the window.
    pub fn fit(width: f64, height: f64) {
        set(positive(width)
            .zip(positive(height))
            .map(|(w, h)| UiScale::Fit(w, h)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M3-GAME-02
    #[test]
    fn modes_scale_to_the_window() {
        assert_eq!(UiScale::None.factor(1920.0, 1080.0), 1.0);
        assert_eq!(UiScale::Fixed(1.5).factor(800.0, 600.0), 1.5);
        assert_eq!(UiScale::Height(1080.0).factor(3840.0, 2160.0), 2.0);
        assert_eq!(
            UiScale::Height(1080.0).factor(1280.0, 720.0),
            720.0 / 1080.0
        );
        // A wide window fits by height, a tall one by width.
        assert_eq!(UiScale::Fit(1920.0, 1080.0).factor(3840.0, 1080.0), 1.0);
        assert_eq!(UiScale::Fit(1920.0, 1080.0).factor(960.0, 1080.0), 0.5);
        assert_eq!(UiScale::Height(1080.0).factor(100.0, 10.0), 0.25, "clamped");
        assert_eq!(UiScale::Height(1080.0).factor(100.0, 0.0), 1.0, "minimised");
    }

    // M3-GAME-02
    #[test]
    fn scripts_set_the_mode_and_bad_sizes_are_ignored() {
        reset();
        ui_scale_api::height(1080.0);
        assert_eq!(current(), UiScale::Height(1080.0));
        ui_scale_api::fit(1280.0, 720.0);
        assert_eq!(current(), UiScale::Fit(1280.0, 720.0));
        ui_scale_api::fixed(-1.0);
        ui_scale_api::fit(1280.0, f64::NAN);
        assert_eq!(current(), UiScale::Fit(1280.0, 720.0), "left as it was");
        ui_scale_api::fixed(1.5);
        assert_eq!(current(), UiScale::Fixed(1.5));
        ui_scale_api::none();
        assert_eq!(current(), UiScale::None);
        // The shims are what compiled code calls.
        ui_scale_api::platypus_UiScale_height(720.0);
        assert_eq!(current(), UiScale::Height(720.0));
        reset();
        assert_eq!(current(), UiScale::None);
    }
}
