// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/clone.ts` against bun (`bench/gen_clone_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-443..).
use platypus_ts_tests::clone_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_cl_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/clone_expected.json")).unwrap()
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

// M9-TS-443
#[test]
fn equal_copy_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("equalCopy", ts_cl_equalCopy());
    });
}

// M9-TS-444
#[test]
fn copy_is_independent_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("copyIsIndependent", ts_cl_copyIsIndependent());
    });
}

// M9-TS-445
#[test]
fn null_fields_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("nullFields", ts_cl_nullFields());
    });
}

// M9-TS-446
#[test]
fn recursive_tree_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("recursiveTree", ts_cl_recursiveTree());
    });
}

// M9-TS-447
#[test]
fn arrays_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("arrays", ts_cl_arrays());
    });
}

// M9-TS-448
#[test]
fn typed_arrays_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("typedArrays", ts_cl_typedArrays());
    });
}

// M9-TS-449
#[test]
fn primitives_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("primitives", ts_cl_primitives());
    });
}

// M9-TS-450
#[test]
fn clone_under_collection_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("cloneUnderCollection", ts_cl_cloneUnderCollection());
    });
}
