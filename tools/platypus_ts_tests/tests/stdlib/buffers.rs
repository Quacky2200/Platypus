// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/buffers.ts` against bun (`bench/gen_buffers_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-431..).
use platypus_ts_tests::buffers_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_bf_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/buffers_expected.json")).unwrap()
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

// M9-TS-431
#[test]
fn views_share_memory_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("viewsShareMemory", ts_bf_viewsShareMemory());
    });
}

// M9-TS-432
#[test]
fn offsets_and_lengths_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("offsetsAndLengths", ts_bf_offsetsAndLengths());
    });
}

// M9-TS-433
#[test]
fn view_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("viewErrors", ts_bf_viewErrors());
    });
}

// M9-TS-434
#[test]
fn subarray_shares_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("subarrayShares", ts_bf_subarrayShares());
    });
}

// M9-TS-435
#[test]
fn buffer_of_an_array_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("bufferOfAnArray", ts_bf_bufferOfAnArray());
    });
}

// M9-TS-436
#[test]
fn buffer_of_a_subarray_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("bufferOfASubarray", ts_bf_bufferOfASubarray());
    });
}

// M9-TS-437
#[test]
fn slice_of_a_buffer_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("sliceOfABuffer", ts_bf_sliceOfABuffer());
    });
}

// M9-TS-438
#[test]
fn data_view_integers_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dataViewIntegers", ts_bf_dataViewIntegers());
    });
}

// M9-TS-439
#[test]
fn data_view_floats_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dataViewFloats", ts_bf_dataViewFloats());
    });
}

// M9-TS-440
#[test]
fn data_view_window_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dataViewWindow", ts_bf_dataViewWindow());
    });
}

// M9-TS-441
#[test]
fn views_survive_collection_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("viewsSurviveCollection", ts_bf_viewsSurviveCollection());
    });
}
