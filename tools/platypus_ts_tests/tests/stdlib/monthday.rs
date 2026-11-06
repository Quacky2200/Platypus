// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/monthday.ts` against bun (`bench/gen_monthday_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-516..).
use platypus_ts_tests::monthday_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_md_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/monthday_expected.json")).unwrap()
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

// M9-TS-516
#[test]
fn year_month_basics_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("yearMonthBasics", ts_md_yearMonthBasics());
    });
}

// M9-TS-517
#[test]
fn year_month_math_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("yearMonthMath", ts_md_yearMonthMath());
    });
}

// M9-TS-518
#[test]
fn year_month_diff_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("yearMonthDiff", ts_md_yearMonthDiff());
    });
}

// M9-TS-519
#[test]
fn year_month_compare_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("yearMonthCompare", ts_md_yearMonthCompare());
    });
}

// M9-TS-520
#[test]
fn year_month_conversions_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("yearMonthConversions", ts_md_yearMonthConversions());
    });
}

// M9-TS-521
#[test]
fn year_month_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("yearMonthErrors", ts_md_yearMonthErrors());
    });
}

// M9-TS-522
#[test]
fn month_day_basics_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("monthDayBasics", ts_md_monthDayBasics());
    });
}

// M9-TS-523
#[test]
fn month_day_math_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("monthDayMath", ts_md_monthDayMath());
    });
}

// M9-TS-524
#[test]
fn month_day_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("monthDayErrors", ts_md_monthDayErrors());
    });
}
