// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/iterators.ts` against bun (`bench/gen_iterators_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-376..).
use platypus_ts_tests::iterators_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_itr_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/iterators_expected.json")).unwrap()
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

// M9-TS-376
#[test]
fn for_of_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("forOf", ts_itr_forOf());
    });
}

// M9-TS-377
#[test]
fn break_continue_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("breakContinue", ts_itr_breakContinue());
    });
}

// M9-TS-378
#[test]
fn spread_and_from_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("spreadAndFrom", ts_itr_spreadAndFrom());
    });
}

// M9-TS-379
#[test]
fn manual_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("manual", ts_itr_manual());
    });
}

// M9-TS-380
#[test]
fn exhausted_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("exhausted", ts_itr_exhausted());
    });
}
