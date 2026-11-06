// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/callbacks.ts`: closures given to the engine (`input.onKeyDown(..)`), called back by the
//! engine with numbers, booleans and strings, in the normal, stress GC and collection-off modes.
//! IDs are in TESTS.md (M9-TS-1200..).
use platypus_engine_api::input;
use platypus_ts_tests::callbacks_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_cbk_init() };
    input::reset();
    f();
    ts_rt_gc_stress(true);
    input::reset();
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    input::reset();
    f();
    ts_rt_gc_enable(true);
    input::reset();
}

fn text(p: *mut u8) -> String {
    assert!(!p.is_null());
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

// M9-TS-1200
#[test]
fn a_key_listener_gets_the_code_and_the_repeat_flag() {
    in_every_gc_mode(|| unsafe {
        let _ = text(ts_cbk_readLog());
        let token = ts_cbk_subscribeKeyDown();
        assert!(token > 0.0);
        input::key_down("KeyA", false);
        ts_cbk_churn();
        input::key_down("KeyA", true);
        input::key_down("Space", false);
        assert_eq!(text(ts_cbk_readLog()), "KeyA;KeyAr;Space;");
    });
}

// M9-TS-1201
#[test]
fn a_listener_that_captures_a_variable_keeps_it_between_calls() {
    in_every_gc_mode(|| unsafe {
        let _ = text(ts_cbk_readLog());
        ts_cbk_subscribeKeyUpCounting();
        input::key_down("KeyW", false);
        input::key_up_event("KeyW");
        ts_cbk_churn();
        input::key_down("KeyW", false);
        input::key_up_event("KeyW");
        assert_eq!(text(ts_cbk_readLog()), "up:KeyW1;up:KeyW2;");
        assert_eq!(ts_cbk_upCount(), 2.0);
    });
}

// M9-TS-1202
#[test]
fn off_stops_a_listener_and_reports_an_unknown_token() {
    in_every_gc_mode(|| unsafe {
        let _ = text(ts_cbk_readLog());
        let token = ts_cbk_subscribeKeyDown();
        input::key_down("KeyA", false);
        assert!(ts_cbk_unsubscribe(token));
        assert!(!ts_cbk_unsubscribe(token));
        input::key_down("KeyB", false);
        assert_eq!(text(ts_cbk_readLog()), "KeyA;");
    });
}

// M9-TS-1203
#[test]
fn gamepad_listeners_get_ids_strings_and_values() {
    in_every_gc_mode(|| unsafe {
        let _ = text(ts_cbk_readLog());
        ts_cbk_subscribeButtons();
        input::gamepad_connected(3, "Pad");
        input::gamepad_button(3, "South", 1.0);
        input::gamepad_button(3, "South", 0.9); // still down: no event
        input::gamepad_axis(3, "LeftStickX", 0.5);
        assert_eq!(text(ts_cbk_readLog()), "down3South;LeftStickX3=0.5;");
    });
}

// M9-TS-1205
#[test]
fn assets_on_changed_gets_the_changed_paths() {
    in_every_gc_mode(|| unsafe {
        let _ = text(ts_cbk_readLog());
        ts_cbk_subscribeAssets();
        platypus_engine_api::assets::changed(["a.png".to_string(), "b.ogg".to_string()]);
        ts_cbk_churn();
        platypus_engine_api::assets::changed(["c.png".to_string()]);
        assert_eq!(text(ts_cbk_readLog()), "assets:a.png,b.ogg;assets:c.png;");
        platypus_engine_api::assets::reset();
    });
}

// M9-TS-1206
#[test]
fn application_on_focus_changed_fires_on_a_change_only() {
    in_every_gc_mode(|| unsafe {
        use platypus_engine_api::app;
        app::reset();
        let _ = text(ts_cbk_readLog());
        let token = ts_cbk_subscribeFocus();
        app::set_focus(true, false); // unchanged
        app::set_focus(false, true);
        ts_cbk_churn();
        app::set_focus(true, false);
        assert_eq!(text(ts_cbk_readLog()), "blur;focus;");
        assert!(ts_cbk_offFocus(token));
        app::set_focus(false, true);
        assert_eq!(text(ts_cbk_readLog()), "");
        app::reset();
    });
}

// M9-TS-1207
#[test]
fn scene_events_reach_script_listeners_with_scene_handles() {
    in_every_gc_mode(|| unsafe {
        platypus_scenes::reset();
        let _ = text(ts_cbk_readLog());
        ts_cbk_subscribeScenes();
        ts_cbk_loadSecond(true);
        assert_eq!(text(ts_cbk_readLog()), "loaded:Second:additive;");
        ts_cbk_loadSecond(false);
        ts_cbk_churn();
        let log = text(ts_cbk_readLog());
        assert!(log.contains("loaded:Second:single;"), "{log}");
        assert!(log.contains("unloaded;"), "{log}");
        assert!(log.contains("active:"), "{log}");
        platypus_scenes::reset();
    });
}

// M9-TS-1204
#[test]
fn a_listener_may_unsubscribe_from_inside_an_event() {
    // The engine calls a snapshot, so a Rust listener removing itself is safe.
    input::reset();
    let token = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let t2 = token.clone();
    let seen = std::rc::Rc::new(std::cell::Cell::new(0));
    let s2 = seen.clone();
    token.set(input::on_key_down(move |_, _| {
        s2.set(s2.get() + 1);
        input::off(t2.get());
    }));
    input::key_down("KeyA", false);
    input::key_down("KeyB", false);
    assert_eq!(seen.get(), 1);
    input::reset();
}
