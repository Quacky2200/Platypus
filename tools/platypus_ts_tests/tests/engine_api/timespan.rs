// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/timespan.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_timespan_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-637..).
use platypus_ts_tests::timespan_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The engine state a program starts from; change it for this program.
fn reset() {}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_tsp_init() };
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
    serde_json::from_str(include_str!("../../bench/timespan_expected.json")).unwrap()
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

// M9-TS-637
#[test]
fn construction_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("construction", ts_tsp_construction());
    });
}

// M9-TS-638
#[test]
fn factories_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("factories", ts_tsp_factories());
    });
}

// M9-TS-639
#[test]
fn parts_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("parts", ts_tsp_parts());
    });
}

// M9-TS-640
#[test]
fn arithmetic_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("arithmetic", ts_tsp_arithmetic());
    });
}

// M9-TS-641
#[test]
fn formats_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("formats", ts_tsp_formats());
    });
}

// M9-TS-642
#[test]
fn parsing_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("parsing", ts_tsp_parsing());
    });
}
