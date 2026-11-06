// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/uri.ts` against bun (`bench/gen_uri_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-287..).
use platypus_ts_tests::uri_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_uri_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/uri_expected.json")).unwrap()
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

// M9-TS-287
#[test]
fn encode_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("encode", ts_uri_encode());
    });
}

// M9-TS-288
#[test]
fn decode_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("decode", ts_uri_decode());
    });
}

// M9-TS-289
#[test]
fn round_trip_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("roundTrip", ts_uri_roundTrip());
    });
}

// M9-TS-290
#[test]
fn errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("errors", ts_uri_errors());
    });
}

// M9-TS-291
#[test]
fn legacy_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("legacy", ts_uri_legacy());
    });
}
