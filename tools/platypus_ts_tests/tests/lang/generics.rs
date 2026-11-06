// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/generics.ts` against bun (`bench/gen_generics_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-562..).
use platypus_ts_tests::generics_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_gx_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/generics_expected.json")).unwrap()
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

// M9-TS-562
#[test]
fn identity_types_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("identityTypes", ts_gx_identityTypes());
    });
}

// M9-TS-563
#[test]
fn first_of_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("firstOf", ts_gx_firstOf());
    });
}

// M9-TS-564
#[test]
fn mapping_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("mapping", ts_gx_mapping());
    });
}

// M9-TS-565
#[test]
fn pairs_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("pairs", ts_gx_pairs());
    });
}

// M9-TS-566
#[test]
fn fills_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("fills", ts_gx_fills());
    });
}

// M9-TS-567
#[test]
fn reducing_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("reducing", ts_gx_reducing());
    });
}

// M9-TS-568
#[test]
fn boxes_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("boxes", ts_gx_boxes());
    });
}

// M9-TS-569
#[test]
fn stacks_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("stacks", ts_gx_stacks());
    });
}

// M9-TS-570
#[test]
fn lists_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("lists", ts_gx_lists());
    });
}

// M9-TS-571
#[test]
fn subclass_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("subclass", ts_gx_subclass());
    });
}

// M9-TS-572
#[test]
fn aliases_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("aliases", ts_gx_aliases());
    });
}

// M9-TS-573
#[test]
fn nested_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("nested", ts_gx_nested());
    });
}
