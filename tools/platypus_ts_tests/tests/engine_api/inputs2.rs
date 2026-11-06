// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/inputs2.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_inputs2_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-632..).
use platypus_ts_tests::inputs2_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// Two frames (`bench/inputs2_prelude.ts`): a key released, another pressed, a gamepad gone, and
/// the window losing focus.
fn reset() {
    use platypus_engine_api::{input, time};
    input::reset();
    time::reset();
    input::key_down("KeyW", false);
    input::key_down("Space", false);
    input::key_down("MouseLeft", false);
    input::mouse_move(10.5, 20.25);
    input::mouse_delta(3.0, 4.0);
    input::wheel(0.0, 2.0);
    input::gamepad_connected(0, "Pad A");
    input::gamepad_button(0, "South", 1.0);
    input::gamepad_connected(1, "Pad B");
    input::gamepad_axis(1, "LeftStickX", -0.9);
    time::advance(0.016);
    input::key_up_event("Space");
    input::mouse_button_event(0, false);
    input::key_down("KeyD", false);
    input::mouse_delta(7.0, 7.0);
    input::gamepad_disconnected(1);
    time::advance(0.016);
    input::blur();
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_in2_init() };
    reset();
    f();
    ts_rt_gc_stress(true);
    reset();
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    reset();
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/inputs2_expected.json")).unwrap()
}

fn check_str(name: &str, got: *mut u8) {
    let want: Vec<u16> = expected()[name]["s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u16)
        .collect();
    assert!(!got.is_null(), "{name} returned null");
    let got = unsafe { Str::from_raw(got as *const Str) }.to_utf16();
    assert_eq!(
        String::from_utf16_lossy(&got),
        String::from_utf16_lossy(&want),
        "{name}"
    );
}

// M9-TS-632
#[test]
fn keys_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("keys", ts_in2_keys());
    });
}

// M9-TS-633
#[test]
fn mouse_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("mouse", ts_in2_mouse());
    });
}

// M9-TS-634
#[test]
fn axes_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("axes", ts_in2_axes());
    });
}

// M9-TS-635
#[test]
fn buttons_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("buttons", ts_in2_buttons());
    });
}

// M9-TS-636
#[test]
fn pads_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("pads", ts_in2_pads());
    });
}
