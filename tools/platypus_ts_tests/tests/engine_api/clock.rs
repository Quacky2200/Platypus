// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/clock.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_clock_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-624..).
use platypus_ts_tests::clock_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The engine ticks the clock three times before the game's code runs.
fn reset() {
    use platypus_engine_api::time;
    time::reset();
    for _ in 0..3 {
        time::advance(0.25);
    }
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_clk_init() };
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
    serde_json::from_str(include_str!("../../bench/clock_expected.json")).unwrap()
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

// M9-TS-624
#[test]
fn since_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("since", ts_clk_since());
    });
}

// M9-TS-625
#[test]
fn until_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("until", ts_clk_until());
    });
}

// M9-TS-626
#[test]
fn after_time_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("afterTime", ts_clk_afterTime());
    });
}
