// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What stays of the `Window` API's tests here: the API moved to web-ui's `web_api_window` (its
//! tests moved with it); the main window's close ends the game through `Application`. IDs are in
//! TESTS.md (M9-API-14xx).
use platypus_engine_api::app;
use platypus_engine_api::window::{self, MAIN};

// M9-API-1426
#[test]
fn closing_the_main_window_from_script_asks_to_quit() {
    window::reset();
    let _ = window::take_requests();
    let _ = app::take_requests();
    MAIN.close();
    assert!(window::take_requests().is_empty(), "no Close(MAIN) request");
    assert!(app::take_requests().contains(&app::Request::Quit(0)));
    assert!(MAIN.is_open(), "the host ends the game");
}
