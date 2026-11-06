// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/rotations.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_rotations_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-665..).
use platypus_ts_tests::rotations_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The engine state a program starts from; change it for this program.
fn reset() {}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_rot_init() };
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
    serde_json::from_str(include_str!("../../bench/rotations_expected.json")).unwrap()
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

// M9-TS-665
#[test]
fn construction_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("construction", ts_rot_construction());
    });
}

// M9-TS-666
#[test]
fn products_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("products", ts_rot_products());
    });
}

// M9-TS-667
#[test]
fn euler_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("euler", ts_rot_euler());
    });
}

// M9-TS-668
#[test]
fn directions_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("directions", ts_rot_directions());
    });
}

// M9-TS-669
#[test]
fn matrices_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("matrices", ts_rot_matrices());
    });
}

// M9-TS-670
#[test]
fn transforming_points_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("transformingPoints", ts_rot_transformingPoints());
    });
}
