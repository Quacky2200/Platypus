// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/weak.ts` against bun (`bench/gen_weak_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-451..).
use platypus_ts_tests::weak_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_wk_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/weak_expected.json")).unwrap()
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

// M9-TS-451
#[test]
fn weak_ref_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("weakRef", ts_wk_weakRef());
    });
}

// M9-TS-452
#[test]
fn typed_weak_ref_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("typedWeakRef", ts_wk_typedWeakRef());
    });
}

// M9-TS-453
#[test]
fn weak_refs_in_an_array_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("weakRefsInAnArray", ts_wk_weakRefsInAnArray());
    });
}

// M9-TS-454
#[test]
fn weak_map_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("weakMap", ts_wk_weakMap());
    });
}

// M9-TS-455
#[test]
fn weak_set_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("weakSet", ts_wk_weakSet());
    });
}
