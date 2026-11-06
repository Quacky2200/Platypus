// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Destructuring compiled by `web_ui_ts` (`bench/destructure.ts`). The expected numbers are what bun
//! computes from the same file. Every test runs three times: normally, with a collection at every
//! allocation, and with collection off. IDs are in TESTS.md (M9-TS-71..).
use platypus_ts_tests::des::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_des_init() };
    f();
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    f();
    ts_rt_gc_stress(false);
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-71
//
// JS: `const { x: ax, y: ay } = a;` renames, `min: { x, y }` nests, and `function f({ x, y }: Vec)`
// takes a parameter apart.
#[test]
fn object_patterns_rename_nest_and_work_as_parameters() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_des_demoDot(), 11.0);
        assert_eq!(ts_des_demoNested(), 4303.0);
        assert_eq!(ts_des_demoParam(), 42.0);
    });
}

// M9-TS-72
//
// JS: `const { label = 7 } = b` uses 7 when `label` is missing (undefined) and the value when it is
// there; the default may be any expression, here a call that allocates.
#[test]
fn a_pattern_default_replaces_a_missing_property() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_des_demoDefaults(), 709.0);
        assert_eq!(ts_des_demoNestedDefault(), 450.0);
    });
}

// M9-TS-73
#[test]
fn array_patterns_have_holes_and_defaults_for_missing_elements() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_des_demoArrays(), 123125.0);
        assert_eq!(ts_des_demoHole(), 24.0);
    });
}

// M9-TS-74
#[test]
fn for_of_variables_and_callback_parameters_can_be_patterns() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_des_demoLoops(), 1409.0);
        assert_eq!(ts_des_demoCallbacks(), 12201.0);
    });
}

// M9-TS-75
#[test]
fn closures_take_patterns_and_capture_destructured_variables() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_des_closures(), 514.0);
        assert_eq!(ts_des_capturedPatternVariable(), 1113.0);
    });
}

// M9-TS-76
#[test]
fn patterns_over_fresh_objects_and_arrays_survive_collections() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_des_allocating(10.0), 145.0);
    });
}
