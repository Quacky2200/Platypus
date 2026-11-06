// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Default, optional and rest parameters and spread compiled by `web_ui_ts` (`bench/params.ts`).
//! The expected numbers are what bun computes from the same file. Every test runs three times:
//! normally, with a collection at every allocation, and with collection off.
//! IDs are in TESTS.md (M9-TS-77..).
use platypus_ts_tests::par::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_par_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-77
//
// JS: `scale(3)` uses the default `k = 2`; a default may use an earlier parameter
// (`hi = lo + 10`) and several defaults may be left out together.
#[test]
fn a_missing_argument_takes_the_parameters_default() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_par_demoDefaults(), 615.0);
        assert_eq!(ts_par_demoRange(), 451818.0);
        assert_eq!(ts_par_demoDescribe(), 1031100.0);
    });
}

// M9-TS-78
#[test]
fn optional_parameters_are_null_when_left_out() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_par_demoOptional(), 5519.0);
    });
}

// M9-TS-79
//
// JS evaluates a default when the call happens, so an object literal default is a new object
// per call and a default that calls a function runs that function.
#[test]
fn defaults_may_be_objects_or_calls_that_allocate() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_par_demoObjectDefault(), 509.0);
        assert_eq!(ts_par_demoCallDefault(), 1136048.0);
    });
}

// M9-TS-80
#[test]
fn a_rest_parameter_collects_the_remaining_arguments() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_par_demoRest(), 12800610.0);
        assert_eq!(ts_par_demoMixed(), 150121202.0);
    });
}

// M9-TS-81
#[test]
fn spread_in_a_call_and_in_an_array_literal() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_par_demoSpreadCall(), 62131.0);
        assert_eq!(ts_par_demoArraySpread(), 12312306.0);
        assert_eq!(ts_par_demoItems(), 7128.0);
    });
}

// M9-TS-82
//
// JS: `f(...a)` gives the function a new array, and `[...a]` is a copy, so writing to either
// leaves `a` alone.
#[test]
fn spread_copies_the_array() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_par_demoNoAliasing(), 869102.0);
    });
}

// M9-TS-83
#[test]
fn defaults_rest_arrays_and_spreads_that_allocate_survive_collections() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_par_allocating(10.0), 2915.0);
    });
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    unsafe { assert_eq!(ts_par_allocating(10.0), 2915.0) };
    ts_rt_gc_stress(false);
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
}

// M9-TS-136
//
// A call that returns nothing and needs a default filled in is wrapped in a block that stores the
// arguments first; the call itself must still run (it once was skipped because its value was void).
#[test]
fn a_void_call_with_a_default_to_fill_in_still_runs() {
    // `sink` is a module variable: start each run from the initialisers.
    in_every_gc_mode(|| unsafe {
        ts_par_init();
        assert_eq!(ts_par_demoVoidDefault(), 7.0)
    });
}
