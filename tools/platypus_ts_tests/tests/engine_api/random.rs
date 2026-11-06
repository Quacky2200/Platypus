// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/random.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_random_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-605..).
use platypus_ts_tests::random_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The engine state a program starts from; change it for this program.
fn reset() {}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_rnd_init() };
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
    serde_json::from_str(include_str!("../../bench/random_expected.json")).unwrap()
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

// M9-TS-605
#[test]
fn doubles_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("doubles", ts_rnd_doubles());
    });
}

// M9-TS-606
#[test]
fn integers_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("integers", ts_rnd_integers());
    });
}

// M9-TS-607
#[test]
fn floats_and_bools_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("floatsAndBools", ts_rnd_floatsAndBools());
    });
}

// M9-TS-608
#[test]
fn seeds_wrap_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("seedsWrap", ts_rnd_seedsWrap());
    });
}

// M9-TS-609
#[test]
fn two_generators_are_independent_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str(
            "twoGeneratorsAreIndependent",
            ts_rnd_twoGeneratorsAreIndependent(),
        );
    });
}

// M9-TS-610
#[test]
fn shared_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("shared", ts_rnd_shared());
    });
}

// M9-TS-675
#[test]
fn vectors_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("vectors", ts_rnd_vectors());
    });
}
