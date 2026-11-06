// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/generators.ts` against bun (`bench/gen_generators_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-382..).
use platypus_ts_tests::generators_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_gn_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/generators_expected.json")).unwrap()
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

// M9-TS-382
#[test]
fn basics_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("basics", ts_gn_basics());
    });
}

// M9-TS-383
#[test]
fn infinite_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("infinite", ts_gn_infinite());
    });
}

// M9-TS-384
#[test]
fn strings_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("strings", ts_gn_strings());
    });
}

// M9-TS-385
#[test]
fn objects_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("objects", ts_gn_objects());
    });
}

// M9-TS-386
#[test]
fn delegation_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("delegation", ts_gn_delegation());
    });
}

// M9-TS-387
#[test]
fn methods_and_trees_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("methodsAndTrees", ts_gn_methodsAndTrees());
    });
}

// M9-TS-388
#[test]
fn interleaved_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("interleaved", ts_gn_interleaved());
    });
}

// M9-TS-389
#[test]
fn early_return_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("earlyReturn", ts_gn_earlyReturn());
    });
}

// M9-TS-390
#[test]
fn try_and_throw_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("tryAndThrow", ts_gn_tryAndThrow());
    });
}

// M9-TS-391
#[test]
fn sending_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("sending", ts_gn_sending());
    });
}

// M9-TS-392
#[test]
fn many_generators_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("manyGenerators", ts_gn_manyGenerators());
    });
}
