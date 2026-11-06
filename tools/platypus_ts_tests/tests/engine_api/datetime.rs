// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/datetime.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_datetime_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-643..).
use platypus_ts_tests::datetime_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// Local time is New York time, as the generator ran.
fn reset() {
    platypus_engine_api::datetime::set_zone(Some("America/New_York")).unwrap();
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_dtm_init() };
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
    serde_json::from_str(include_str!("../../bench/datetime_expected.json")).unwrap()
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

// M9-TS-643
#[test]
fn fields_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("fields", ts_dtm_fields());
    });
}

// M9-TS-644
#[test]
fn epoch_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("epoch", ts_dtm_epoch());
    });
}

// M9-TS-645
#[test]
fn formats_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("formats", ts_dtm_formats());
    });
}

// M9-TS-646
#[test]
fn arithmetic_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("arithmetic", ts_dtm_arithmetic());
    });
}

// M9-TS-647
#[test]
fn daylight_saving_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("daylightSaving", ts_dtm_daylightSaving());
    });
}

// M9-TS-648
#[test]
fn carrying_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("carrying", ts_dtm_carrying());
    });
}

// M9-TS-649
#[test]
fn parsing_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("parsing", ts_dtm_parsing());
    });
}

// M9-TS-650
#[test]
fn statics_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("statics", ts_dtm_statics());
    });
}
