// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Window`: the game's windows as scripts see them. The API lives in web-ui's `web_api_window`
//! crate (its state, the `#[ts_api]` declarations, the host functions); this module re-exports it
//! under its old path, so the host (`platypus_app`) and the tests keep working. What stays here:
//! closing the main window ends the game ([`crate::app::take_requests`] turns web-ui's quit flag
//! into `Request::Quit`), and the log sink ([`crate::log::set_sink`]) is the API's too.

pub use web_api_window::*;
