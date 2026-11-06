// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/locale.ts` against bun (`bench/gen_locale_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-465..).
use platypus_ts_tests::locale_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_lc_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/locale_expected.json")).unwrap()
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

// M9-TS-465
#[test]
fn parts_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("parts", ts_lc_parts());
    });
}

// M9-TS-466
#[test]
fn canonicalisation_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("canonicalisation", ts_lc_canonicalisation());
    });
}

// M9-TS-467
#[test]
fn extensions_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("extensions", ts_lc_extensions());
    });
}

// M9-TS-468
#[test]
fn options_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("options", ts_lc_options());
    });
}

// M9-TS-469
#[test]
fn maximize_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("maximize", ts_lc_maximize());
    });
}

// M9-TS-470
#[test]
fn minimize_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("minimize", ts_lc_minimize());
    });
}

// M9-TS-471
#[test]
fn keeps_extensions_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("keepsExtensions", ts_lc_keepsExtensions());
    });
}

// M9-TS-472
#[test]
fn invalid_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("invalid", ts_lc_invalid());
    });
}
