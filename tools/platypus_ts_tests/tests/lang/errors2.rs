// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/errors2.ts` against bun (`bench/gen_errors2_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-292..).
use platypus_ts_tests::errors2_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_e2_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/errors2_expected.json")).unwrap()
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

// M9-TS-292
#[test]
fn cause_chain_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("causeChain", ts_e2_causeChain());
    });
}

// M9-TS-293
#[test]
fn cause_in_catch_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("causeInCatch", ts_e2_causeInCatch());
    });
}

// M9-TS-294
#[test]
fn aggregate_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("aggregate", ts_e2_aggregate());
    });
}

// M9-TS-295
#[test]
fn aggregate_caught_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("aggregateCaught", ts_e2_aggregateCaught());
    });
}
