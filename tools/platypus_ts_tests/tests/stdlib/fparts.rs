// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/fparts.ts` against bun (`bench/gen_fparts_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-537..).
use platypus_ts_tests::fparts_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_fp_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/fparts_expected.json")).unwrap()
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

// M9-TS-537
#[test]
fn decimal_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("decimal", ts_fp_decimal());
    });
}

// M9-TS-538
#[test]
fn currency_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("currency", ts_fp_currency());
    });
}

// M9-TS-539
#[test]
fn percent_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("percent", ts_fp_percent());
    });
}

// M9-TS-540
#[test]
fn other_locales_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("otherLocales", ts_fp_otherLocales());
    });
}

// M9-TS-541
#[test]
fn signs_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("signs", ts_fp_signs());
    });
}

// M9-TS-542
#[test]
fn compact_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("compact", ts_fp_compact());
    });
}

// M9-TS-543
#[test]
fn scientific_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("scientific", ts_fp_scientific());
    });
}

// M9-TS-544
#[test]
fn special_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("special", ts_fp_special());
    });
}

// M9-TS-545
#[test]
fn date_english_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateEnglish", ts_fp_dateEnglish());
    });
}

// M9-TS-546
#[test]
fn date_long_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateLong", ts_fp_dateLong());
    });
}

// M9-TS-547
#[test]
fn date_fields_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateFields", ts_fp_dateFields());
    });
}

// M9-TS-548
#[test]
fn date_other_locales_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateOtherLocales", ts_fp_dateOtherLocales());
    });
}

// M9-TS-549
#[test]
fn date_hour12_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateHour12", ts_fp_dateHour12());
    });
}
