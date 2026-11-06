// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/vectors.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_vectors_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-661..).
use platypus_ts_tests::vectors_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The engine state a program starts from; change it for this program.
fn reset() {}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_vct_init() };
    reset();
    f();
    ts_rt_gc_stress(true);
    reset();
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    reset();
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/vectors_expected.json")).unwrap()
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

// M9-TS-661
#[test]
fn vector2_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("vector2", ts_vct_vector2());
    });
}

// M9-TS-662
#[test]
fn vector3_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("vector3", ts_vct_vector3());
    });
}

// M9-TS-663
#[test]
fn vector4_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("vector4", ts_vct_vector4());
    });
}

// M9-TS-664
#[test]
fn grid_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("grid", ts_vct_grid());
    });
}
