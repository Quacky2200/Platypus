// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/bigint.ts` against bun (`bench/gen_bigint_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-360..).
use platypus_ts_tests::bigint_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_bi_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/bigint_expected.json")).unwrap()
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

// M9-TS-360
#[test]
fn literals_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("literals", ts_bi_literals());
    });
}

// M9-TS-361
#[test]
fn arithmetic_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("arithmetic", ts_bi_arithmetic());
    });
}

// M9-TS-362
#[test]
fn bitwise_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("bitwise", ts_bi_bitwise());
    });
}

// M9-TS-363
#[test]
fn comparison_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("comparison", ts_bi_comparison());
    });
}

// M9-TS-364
#[test]
fn conversions_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("conversions", ts_bi_conversions());
    });
}

// M9-TS-365
#[test]
fn truthiness_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("truthiness", ts_bi_truthiness());
    });
}

// M9-TS-366
#[test]
fn as_n_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("asN", ts_bi_asN());
    });
}

// M9-TS-367
#[test]
fn loops_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("loops", ts_bi_loops());
    });
}

// M9-TS-368
#[test]
fn errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("errors", ts_bi_errors());
    });
}
