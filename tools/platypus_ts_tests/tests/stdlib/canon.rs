// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/canon.ts` against bun (`bench/gen_canon_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-533..).
use platypus_ts_tests::canon_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_cn_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/canon_expected.json")).unwrap()
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

// M9-TS-533
#[test]
fn single_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("single", ts_cn_single());
    });
}

// M9-TS-534
#[test]
fn list_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("list", ts_cn_list());
    });
}

// M9-TS-535
#[test]
fn empty_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("empty", ts_cn_empty());
    });
}

// M9-TS-536
#[test]
fn invalid_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("invalid", ts_cn_invalid());
    });
}
