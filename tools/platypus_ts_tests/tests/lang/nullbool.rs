// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/nullbool.ts` against bun (`bench/gen_nullbool_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-598..).
use platypus_ts_tests::nullbool_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_nb_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/nullbool_expected.json")).unwrap()
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

// M9-TS-598
#[test]
fn fields_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("fields", ts_nb_fields());
    });
}

// M9-TS-599
#[test]
fn params_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("params", ts_nb_params());
    });
}

// M9-TS-600
#[test]
fn members_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("members", ts_nb_members());
    });
}
