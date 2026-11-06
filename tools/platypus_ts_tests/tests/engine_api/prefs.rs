// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/prefs.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_prefs_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-615..).
use platypus_ts_tests::prefs_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The saved file had "saved.n" = 7.9 and "saved.s" = "hello".
fn reset() {
    use platypus_engine_api::prefs::{self, Pref};
    prefs::reset();
    prefs::load([
        ("saved.n".to_string(), Pref::Num(7.9)),
        ("saved.s".to_string(), Pref::Str("hello".into())),
    ]);
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_prf_init() };
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
    serde_json::from_str(include_str!("../../bench/prefs_expected.json")).unwrap()
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

// M9-TS-615
#[test]
fn reads_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("reads", ts_prf_reads());
    });
}

// M9-TS-616
#[test]
fn writes_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("writes", ts_prf_writes());
    });
}
