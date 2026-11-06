// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/logic.ts` against bun (`bench/gen_logic_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-370..).
use platypus_ts_tests::logic_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_lg_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/logic_expected.json")).unwrap()
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

// M9-TS-370
#[test]
fn numbers_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("numbers", ts_lg_numbers());
    });
}

// M9-TS-371
#[test]
fn strings_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("strings", ts_lg_strings());
    });
}

// M9-TS-372
#[test]
fn nullables_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("nullables", ts_lg_nullables());
    });
}

// M9-TS-373
#[test]
fn chains_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("chains", ts_lg_chains());
    });
}

// M9-TS-374
#[test]
fn conditions_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("conditions", ts_lg_conditions());
    });
}

// M9-TS-375
#[test]
fn defaults_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("defaults", ts_lg_defaults());
    });
}
