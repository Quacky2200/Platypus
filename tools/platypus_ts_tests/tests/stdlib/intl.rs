// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Intl` step I1 in compiled code (`bench/intl.ts`): `localeCompare`, `Intl.Collator`, `normalize`
//! and `toLocaleUpperCase`/`toLocaleLowerCase`. `bench/gen_intl_expected.ts` ran the same file in
//! bun; every function here must return what bun returned, in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-254..).
use platypus_ts_tests::itl::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_in_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/intl_expected.json")).unwrap()
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

// M9-TS-254
#[test]
fn locale_compare_basics_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("localeCompareBasics", ts_in_localeCompareBasics());
    });
}

// M9-TS-255
#[test]
fn locale_compare_options_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("localeCompareOptions", ts_in_localeCompareOptions());
    });
}

// M9-TS-256
#[test]
fn locale_compare_errors_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("localeCompareErrors", ts_in_localeCompareErrors());
    });
}

// M9-TS-257
#[test]
fn normalize_forms_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("normalizeForms", ts_in_normalizeForms());
    });
}

// M9-TS-258
#[test]
fn normalize_errors_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("normalizeErrors", ts_in_normalizeErrors());
    });
}

// M9-TS-259
#[test]
fn locale_case_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("localeCase", ts_in_localeCase());
    });
}

// M9-TS-260
#[test]
fn collator_sorts_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("collatorSorts", ts_in_collatorSorts());
    });
}

// M9-TS-261
#[test]
fn collator_compare_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("collatorCompare", ts_in_collatorCompare());
    });
}

// M9-TS-262
#[test]
fn collator_errors_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("collatorErrors", ts_in_collatorErrors());
    });
}
