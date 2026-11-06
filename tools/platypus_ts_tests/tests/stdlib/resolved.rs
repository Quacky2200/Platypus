// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/resolved.ts` against bun (`bench/gen_resolved_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-550..).
use platypus_ts_tests::resolved_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_rs_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/resolved_expected.json")).unwrap()
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

// M9-TS-550
#[test]
fn date_time_format_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateTimeFormat", ts_rs_dateTimeFormat());
    });
}

// M9-TS-551
#[test]
fn date_time_zones_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateTimeZones", ts_rs_dateTimeZones());
    });
}

// M9-TS-552
#[test]
fn default_locale_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("defaultLocale", ts_rs_defaultLocale());
    });
}

// M9-TS-553
#[test]
fn number_format_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("numberFormat", ts_rs_numberFormat());
    });
}

// M9-TS-554
#[test]
fn number_currency_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("numberCurrency", ts_rs_numberCurrency());
    });
}

// M9-TS-555
#[test]
fn number_options_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("numberOptions", ts_rs_numberOptions());
    });
}
