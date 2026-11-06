// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/rounding.ts` against bun (`bench/gen_rounding_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-525..).
use platypus_ts_tests::rounding_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_rd_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/rounding_expected.json")).unwrap()
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

// M9-TS-525
#[test]
fn time_units_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("timeUnits", ts_rd_timeUnits());
    });
}

// M9-TS-526
#[test]
fn time_modes_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("timeModes", ts_rd_timeModes());
    });
}

// M9-TS-527
#[test]
fn time_increments_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("timeIncrements", ts_rd_timeIncrements());
    });
}

// M9-TS-528
#[test]
fn date_time_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateTime", ts_rd_dateTime());
    });
}

// M9-TS-529
#[test]
fn instant_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("instant", ts_rd_instant());
    });
}

// M9-TS-530
#[test]
fn zoned_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("zoned", ts_rd_zoned());
    });
}

// M9-TS-531
#[test]
fn duration_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("duration", ts_rd_duration());
    });
}

// M9-TS-532
#[test]
fn errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("errors", ts_rd_errors());
    });
}
