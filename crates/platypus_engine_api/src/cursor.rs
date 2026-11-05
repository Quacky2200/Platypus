// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Cursor`: the OS mouse cursor (`js/engine/src/cursor.ts`): show or hide it, hold it in the
//! window, and give it an image. The host reads [`state`] when [`version`] changed. UI elements'
//! CSS `cursor` still wins where they set one.

use std::cell::RefCell;
use ts_api::ts_api;

/// What scripts asked of the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct CursorState {
    pub visible: bool,
    /// `none` (free to leave the window), `locked` (hidden and held, for mouse look) or `confined`.
    pub lock_state: String,
    /// An image file and the pixel that clicks, or none for the arrow.
    pub image: Option<(String, Option<[f64; 2]>)>,
}

impl Default for CursorState {
    fn default() -> Self {
        CursorState {
            visible: true,
            lock_state: "none".to_string(),
            image: None,
        }
    }
}

thread_local! {
    static STATE: RefCell<(CursorState, u64)> = RefCell::new((CursorState::default(), 0));
}

/// The cursor as scripts want it.
pub fn state() -> CursorState {
    STATE.with(|s| s.borrow().0.clone())
}

/// A number that grows with every change.
pub fn version() -> u64 {
    STATE.with(|s| s.borrow().1)
}

/// Back to a visible, free arrow (a test, or a reload).
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = (CursorState::default(), 0));
}

fn change(f: impl FnOnce(&mut CursorState)) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.1 += 1;
        f(&mut s.0);
    });
}

#[ts_api(namespace = "Cursor")]
pub mod cursor_api {
    use super::{STATE, change};

    /// Whether the cursor shows over the window (locking hides it as well).
    #[property]
    pub fn visible() -> bool {
        STATE.with(|s| s.borrow().0.visible)
    }

    pub fn set_visible(v: bool) {
        change(|s| s.visible = v);
    }

    /// `CursorLockMode.None`, `Locked` or `Confined`; another name is ignored.
    #[property]
    pub fn lock_state() -> String {
        STATE.with(|s| s.borrow().0.lock_state.clone())
    }

    pub fn set_lock_state(mode: &str) {
        if ["none", "locked", "confined"].contains(&mode) {
            change(|s| s.lock_state = mode.to_string());
        }
    }

    /// Shows an image as the cursor (PNG, JPEG or `.cur`, up to 128 px) from a path in the game
    /// folder, with the hotspot (in image pixels) being the point that clicks. A hotspot left
    /// out is the host's choice (the file's own, else the corner).
    #[optional(hotspot_x, hotspot_y)]
    pub fn set_cursor(path: &str, hotspot_x: f64, hotspot_y: f64) {
        let hotspot =
            (!hotspot_x.is_nan() && !hotspot_y.is_nan()).then_some([hotspot_x, hotspot_y]);
        change(|s| s.image = Some((path.to_string(), hotspot)));
    }

    /// Goes back to the arrow.
    pub fn clear_cursor() {
        change(|s| s.image = None);
    }
}

#[ts_api(namespace = "CursorLockMode")]
pub mod cursor_lock_mode_api {
    /// Free to leave the window.
    #[property]
    #[js_name = "None"]
    pub fn none() -> String {
        "none".to_string()
    }

    /// Hidden and held in place, for mouse look: read `input.mouseDelta`.
    #[property]
    #[js_name = "Locked"]
    pub fn locked() -> String {
        "locked".to_string()
    }

    /// Kept inside the window.
    #[property]
    #[js_name = "Confined"]
    pub fn confined() -> String {
        "confined".to_string()
    }
}
