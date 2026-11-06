// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/layers.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_layers_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-611..).
use platypus_ts_tests::layers_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The game names layers 8, 9 and 4 (renaming "Water"); 40 is out of range and 10 has no name.
fn reset() {
    platypus_engine_api::layers::set_game_layers([
        (8, "Player".to_string()),
        (9, "Minimap".to_string()),
        (4, "Liquid".to_string()),
        (40, "TooHigh".to_string()),
        (10, String::new()),
    ]);
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_lyr_init() };
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
    serde_json::from_str(include_str!("../../bench/layers_expected.json")).unwrap()
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

// M9-TS-611
#[test]
fn names_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("names", ts_lyr_names());
    });
}

// M9-TS-612
#[test]
fn numbers_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("numbers", ts_lyr_numbers());
    });
}

// M9-TS-613
#[test]
fn masks_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("masks", ts_lyr_masks());
    });
}

// M9-TS-614
#[test]
fn contains_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("contains", ts_lyr_contains());
    });
}
