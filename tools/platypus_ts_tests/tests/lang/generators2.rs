// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/generators2.ts` against bun (`bench/gen_generators2_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-399..).
use platypus_ts_tests::generators2_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

// The program's globals (`log`) are shared, so the tests take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert_ne!(link_anchor(), 0);
    unsafe { ts_gn2_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/generators2_expected.json")).unwrap()
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

// M9-TS-399
#[test]
fn throw_caught_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("throwCaught", ts_gn2_throwCaught());
    });
}

// M9-TS-400
#[test]
fn throw_escapes_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("throwEscapes", ts_gn2_throwEscapes());
    });
}

// M9-TS-401
#[test]
fn throw_before_start_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("throwBeforeStart", ts_gn2_throwBeforeStart());
    });
}

// M9-TS-402
#[test]
fn return_runs_finally_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("returnRunsFinally", ts_gn2_returnRunsFinally());
    });
}

// M9-TS-403
#[test]
fn return_before_start_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("returnBeforeStart", ts_gn2_returnBeforeStart());
    });
}

// M9-TS-404
#[test]
fn return_not_swallowed_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("returnNotSwallowed", ts_gn2_returnNotSwallowed());
    });
}

// M9-TS-405
#[test]
fn return_finally_yields_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("returnFinallyYields", ts_gn2_returnFinallyYields());
    });
}

// M9-TS-406
#[test]
fn return_values_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("returnValues", ts_gn2_returnValues());
    });
}

// M9-TS-407
#[test]
fn early_return_value_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("earlyReturnValue", ts_gn2_earlyReturnValue());
    });
}
