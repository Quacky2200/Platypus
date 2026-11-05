// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `UiResources`: what a game asks of the UI from script: stylesheets from CSS text
//! (`addStyleSheet` / `removeStyleSheet`) and fonts (`loadFont`). Nothing is drawn here: the host
//! takes the requests with [`take_requests`] and applies them to the screen UI and every world
//! panel, the way it did the `addStyleSheet` / `removeStyleSheet` / `loadFont` ops.
//!
//! The TypeScript functions of `@platypus/engine` are written over this in the compiler's facade.

use std::cell::RefCell;
use ts_api::ts_api;

/// One request, in the order scripts made them.
#[derive(Clone, Debug, PartialEq)]
pub enum UiRequest {
    /// A stylesheet from CSS text under the id `addStyleSheet` returned.
    AddStyleSheet {
        id: u32,
        css: String,
    },
    RemoveStyleSheet {
        id: u32,
    },
    /// `weight` 0 and an empty `style` mean the font's own.
    LoadFont {
        family: String,
        src: String,
        weight: f64,
        style: String,
    },
}

#[derive(Default)]
struct State {
    next_sheet: u32,
    requests: Vec<UiRequest>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// What scripts asked since the last call.
pub fn take_requests() -> Vec<UiRequest> {
    STATE.with(|s| std::mem::take(&mut s.borrow_mut().requests))
}

/// Forgets everything (a test, or a reload).
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}

#[ts_api(namespace = "UiResources")]
pub mod ui_resources_api {
    use super::{STATE, UiRequest};

    /// Adds a stylesheet from CSS text; `url()`s are relative to the game folder. Returns an id for
    /// `removeStyleSheet`.
    #[global]
    pub fn add_style_sheet(css: &str) -> f64 {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            s.next_sheet += 1;
            let id = s.next_sheet;
            s.requests.push(UiRequest::AddStyleSheet {
                id,
                css: css.to_string(),
            });
            f64::from(id)
        })
    }

    /// Removes a stylesheet added with `addStyleSheet`.
    #[global]
    pub fn remove_style_sheet(id: f64) {
        STATE.with(|s| {
            s.borrow_mut()
                .requests
                .push(UiRequest::RemoveStyleSheet { id: id as u32 });
        });
    }

    /// Loads a font for `fontFamily` to use (like CSS `@font-face`).
    /// family: The name `fontFamily` uses.
    /// src: The font file, relative to the game folder.
    /// weight: The CSS weight it is for (the font's own when left out).
    /// style: `normal`, `italic` or `oblique` (the font's own when left out).
    #[global]
    #[options(name = "FontFace", only, object)]
    #[optional(weight = 0, style)]
    pub fn load_font(family: &str, src: &str, weight: f64, style: &str) {
        STATE.with(|s| {
            s.borrow_mut().requests.push(UiRequest::LoadFont {
                family: family.to_string(),
                src: src.to_string(),
                weight,
                style: style.to_string(),
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // M9-API-500
    #[test]
    fn requests_come_out_in_order_with_stylesheet_ids() {
        reset();
        let a = ui_resources_api::add_style_sheet(".a {}");
        ui_resources_api::load_font("Charm", "fonts/Charm.ttf", 700.0, "");
        ui_resources_api::remove_style_sheet(a);
        let b = ui_resources_api::add_style_sheet(".b {}");
        assert_eq!((a, b), (1.0, 2.0));
        let r = take_requests();
        assert_eq!(r.len(), 4);
        assert_eq!(
            r[1],
            UiRequest::LoadFont {
                family: "Charm".into(),
                src: "fonts/Charm.ttf".into(),
                weight: 700.0,
                style: String::new(),
            }
        );
        assert_eq!(r[2], UiRequest::RemoveStyleSheet { id: 1 });
        assert!(take_requests().is_empty());
    }
}
