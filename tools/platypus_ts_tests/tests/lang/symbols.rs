// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/symbols.ts` against bun (`bench/gen_symbols_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-394..).
use platypus_ts_tests::symbols_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

// The program's globals (the `Symbol.for` registry) are shared, so the tests take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert_ne!(link_anchor(), 0);
    unsafe { ts_sym_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/symbols_expected.json")).unwrap()
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

// M9-TS-394
#[test]
fn identity_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("identity", ts_sym_identity());
    });
}

// M9-TS-395
#[test]
fn describe_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("describe", ts_sym_describe());
    });
}

// M9-TS-396
#[test]
fn registry_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("registry", ts_sym_registry());
    });
}

// M9-TS-397
#[test]
fn typeof_symbol_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("typeofSymbol", ts_sym_typeofSymbol());
    });
}

// M9-TS-398
#[test]
fn in_classes_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("inClasses", ts_sym_inClasses());
    });
}
