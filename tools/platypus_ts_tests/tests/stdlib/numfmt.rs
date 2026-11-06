// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Intl.NumberFormat` and `Number.prototype.toLocaleString` in compiled code (`bench/numfmt.ts`).
//! `bench/gen_numfmt_expected.ts` ran the same file in bun; every function here must return what
//! bun returned, in the normal, stress GC and collection-off modes. IDs are in TESTS.md
//! (M9-TS-264..).
use platypus_ts_tests::nfm::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_nf_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/numfmt_expected.json")).unwrap()
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

// M9-TS-264
#[test]
fn decimal_locales_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("decimalLocales", ts_nf_decimalLocales());
    });
}

// M9-TS-265
#[test]
fn fraction_digits_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("fractionDigits", ts_nf_fractionDigits());
    });
}

// M9-TS-266
#[test]
fn significant_digits_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("significantDigits", ts_nf_significantDigits());
    });
}

// M9-TS-267
#[test]
fn percent_and_sign_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("percentAndSign", ts_nf_percentAndSign());
    });
}

// M9-TS-268
#[test]
fn currency_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("currency", ts_nf_currency());
    });
}

// M9-TS-269
#[test]
fn currency_display_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("currencyDisplay", ts_nf_currencyDisplay());
    });
}

// M9-TS-270
#[test]
fn compact_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("compact", ts_nf_compact());
    });
}

// M9-TS-271
#[test]
fn scientific_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("scientific", ts_nf_scientific());
    });
}

// M9-TS-272
#[test]
fn grouping_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("grouping", ts_nf_grouping());
    });
}

// M9-TS-273
#[test]
fn rounding_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rounding", ts_nf_rounding());
    });
}

// M9-TS-274
#[test]
fn format_object_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("formatObject", ts_nf_formatObject());
    });
}

// M9-TS-275
#[test]
fn errors_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("errors", ts_nf_errors());
    });
}
