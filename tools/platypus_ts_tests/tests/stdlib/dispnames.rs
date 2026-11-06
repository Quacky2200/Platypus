// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/dispnames.ts` against bun (`bench/gen_dispnames_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-473..).
use platypus_ts_tests::dispnames_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_dn_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/dispnames_expected.json")).unwrap()
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

// M9-TS-473
#[test]
fn regions_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("regions", ts_dn_regions());
    });
}

// M9-TS-474
#[test]
fn regions_in_other_locales_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("regionsInOtherLocales", ts_dn_regionsInOtherLocales());
    });
}

// M9-TS-475
#[test]
fn scripts_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("scripts", ts_dn_scripts());
    });
}

// M9-TS-476
#[test]
fn languages_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("languages", ts_dn_languages());
    });
}

// M9-TS-477
#[test]
fn languages_standard_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("languagesStandard", ts_dn_languagesStandard());
    });
}

// M9-TS-478
#[test]
fn languages_in_french_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("languagesInFrench", ts_dn_languagesInFrench());
    });
}

// M9-TS-479
#[test]
fn missing_type_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("missingType", ts_dn_missingType());
    });
}
