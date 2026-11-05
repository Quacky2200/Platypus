// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The host functions of the engine API's `Application`. IDs are in TESTS.md.
use platypus_engine_api::app;
use std::cell::RefCell;
use std::rc::Rc;

// M9-VSP-15
//
// `Application.showVideoSettings()` asks the host for the dialog; the host's
// `video_settings_changed` runs the `onVideoSettingsChanged` listeners until they are removed.
#[test]
fn show_video_settings_is_a_request_and_the_change_reaches_listeners() {
    use ts_api::Callback;
    app::reset();
    app::application_api::show_video_settings();
    assert_eq!(app::take_requests(), [app::Request::ShowVideoSettings]);
    let heard = Rc::new(RefCell::new(0));
    let h = heard.clone();
    let token = app::application_api::on_video_settings_changed(Callback::from_fn(move |()| {
        *h.borrow_mut() += 1;
    }));
    app::video_settings_changed();
    assert_eq!(*heard.borrow(), 1);
    assert!(app::application_api::off_video_settings_changed(token));
    app::video_settings_changed();
    assert_eq!(*heard.borrow(), 1);
    app::reset();
}
