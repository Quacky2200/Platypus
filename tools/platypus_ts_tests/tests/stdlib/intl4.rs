// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/intl4.ts` against bun (`bench/gen_intl4_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-296..).
use platypus_ts_tests::intl4_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_i4_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/intl4_expected.json")).unwrap()
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

// M9-TS-296
#[test]
fn plural_cardinal_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("pluralCardinal", ts_i4_pluralCardinal());
    });
}

// M9-TS-297
#[test]
fn plural_ordinal_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("pluralOrdinal", ts_i4_pluralOrdinal());
    });
}

// M9-TS-298
#[test]
fn plural_fraction_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("pluralFraction", ts_i4_pluralFraction());
    });
}

// M9-TS-299
#[test]
fn plural_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("pluralErrors", ts_i4_pluralErrors());
    });
}

// M9-TS-300
#[test]
fn list_conjunction_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("list_conjunction", ts_i4_list_conjunction());
    });
}

// M9-TS-301
#[test]
fn list_disjunction_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("list_disjunction", ts_i4_list_disjunction());
    });
}

// M9-TS-302
#[test]
fn list_unit_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("list_unit", ts_i4_list_unit());
    });
}

// M9-TS-303
#[test]
fn list_defaults_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("listDefaults", ts_i4_listDefaults());
    });
}

// M9-TS-304
#[test]
fn list_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("listErrors", ts_i4_listErrors());
    });
}

// M9-TS-305
#[test]
fn rtf_always_long_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rtf_always_long", ts_i4_rtf_always_long());
    });
}

// M9-TS-306
#[test]
fn rtf_always_short_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rtf_always_short", ts_i4_rtf_always_short());
    });
}

// M9-TS-307
#[test]
fn rtf_always_narrow_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rtf_always_narrow", ts_i4_rtf_always_narrow());
    });
}

// M9-TS-308
#[test]
fn rtf_auto_long_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rtf_auto_long", ts_i4_rtf_auto_long());
    });
}

// M9-TS-309
#[test]
fn rtf_auto_short_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rtf_auto_short", ts_i4_rtf_auto_short());
    });
}

// M9-TS-310
#[test]
fn rtf_auto_narrow_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rtf_auto_narrow", ts_i4_rtf_auto_narrow());
    });
}

// M9-TS-311
#[test]
fn rtf_plural_units_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rtfPluralUnits", ts_i4_rtfPluralUnits());
    });
}

// M9-TS-312
#[test]
fn rtf_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("rtfErrors", ts_i4_rtfErrors());
    });
}
