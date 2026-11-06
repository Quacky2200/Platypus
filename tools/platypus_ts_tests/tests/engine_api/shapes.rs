// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/shapes.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_shapes_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-671..).
use platypus_ts_tests::shapes_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The engine state a program starts from; change it for this program.
fn reset() {}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_shp_init() };
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
    serde_json::from_str(include_str!("../../bench/shapes_expected.json")).unwrap()
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

// M9-TS-671
#[test]
fn rects_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("rects", ts_shp_rects());
    });
}

// M9-TS-672
#[test]
fn rays_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("rays", ts_shp_rays());
    });
}

// M9-TS-673
#[test]
fn bounds_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("bounds", ts_shp_bounds());
    });
}

// M9-TS-674
#[test]
fn poses_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("poses", ts_shp_poses());
    });
}
