// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/zoned.ts` against bun (`bench/gen_zoned_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-507..).
use platypus_ts_tests::zoned_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_zn_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/zoned_expected.json")).unwrap()
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

// M9-TS-507
#[test]
fn parse_and_print_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("parseAndPrint", ts_zn_parseAndPrint());
    });
}

// M9-TS-508
#[test]
fn fields_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("fields", ts_zn_fields());
    });
}

// M9-TS-509
#[test]
fn spring_forward_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("springForward", ts_zn_springForward());
    });
}

// M9-TS-510
#[test]
fn fall_back_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("fallBack", ts_zn_fallBack());
    });
}

// M9-TS-511
#[test]
fn convert_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("convert", ts_zn_convert());
    });
}

// M9-TS-512
#[test]
fn arithmetic_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("arithmetic", ts_zn_arithmetic());
    });
}

// M9-TS-513
#[test]
fn from_fields_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("fromFields", ts_zn_fromFields());
    });
}

// M9-TS-514
#[test]
fn errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("errors", ts_zn_errors());
    });
}

// M9-TS-515
#[test]
fn now_zoned_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("nowZoned", ts_zn_nowZoned());
    });
}
