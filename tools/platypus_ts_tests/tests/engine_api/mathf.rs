// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/mathf.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_mathf_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-652..).
use platypus_ts_tests::mathf_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The engine state a program starts from; change it for this program.
fn reset() {}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_mtf_init() };
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
    serde_json::from_str(include_str!("../../bench/mathf_expected.json")).unwrap()
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

// M9-TS-652
#[test]
fn constants_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("constants", ts_mtf_constants());
    });
}

// M9-TS-653
#[test]
fn interpolation_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("interpolation", ts_mtf_interpolation());
    });
}

// M9-TS-654
#[test]
fn stepping_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("stepping", ts_mtf_stepping());
    });
}

// M9-TS-655
#[test]
fn angles_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("angles", ts_mtf_angles());
    });
}

// M9-TS-656
#[test]
fn comparisons_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("comparisons", ts_mtf_comparisons());
    });
}

// M9-TS-657
#[test]
fn powers_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("powers", ts_mtf_powers());
    });
}

// M9-TS-658
#[test]
fn noise_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("noise", ts_mtf_noise());
    });
}

// M9-TS-659
#[test]
fn colour_space_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("colourSpace", ts_mtf_colourSpace());
    });
}

// M9-TS-660
#[test]
fn damping_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("damping", ts_mtf_damping());
    });
}
