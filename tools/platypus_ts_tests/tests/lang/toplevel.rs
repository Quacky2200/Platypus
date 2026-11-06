// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/toplevel.ts` against bun (`bench/gen_toplevel_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-2849..).
use platypus_ts_tests::toplevel_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The module's variables are process-wide, so the tests take turns, and each run starts from
/// the initialisers (the statements under test).
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert_ne!(link_anchor(), 0);
    unsafe { ts_tl_init() };
    f();
    ts_rt_gc_stress(true);
    unsafe { ts_tl_init() };
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    unsafe { ts_tl_init() };
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/toplevel_expected.json")).unwrap()
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

// M9-TS-2849
#[test]
fn top_log_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("topLog", ts_tl_topLog());
    });
}

// M9-TS-2850
#[test]
fn top_squares_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("topSquares", ts_tl_topSquares());
    });
}

// M9-TS-2851
#[test]
fn top_numbers_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("topNumbers", ts_tl_topNumbers());
    });
}

// M9-TS-2852
#[test]
fn top_imported_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("topImported", ts_tl_topImported());
    });
}

// M9-TS-2853
//
// A top-level `return` is rejected with its line (the parser says so); a statement kind the
// compiler cannot lower (here a namespace) is a compile error naming it and its line, never
// dropped silently.
#[test]
fn an_unsupported_top_level_statement_is_a_compile_error_with_a_line() {
    let e = web_ui_ts::compile_source(
        "export const a = 1;
if (a > 0) {}
return;
",
        "speed",
    )
    .err()
    .expect("must not compile");
    assert!(e.contains("return") && e.contains("line 3"), "{e}");
    let e = web_ui_ts::compile_source(
        "export const a = 1;

namespace N {}
",
        "speed",
    )
    .err()
    .expect("must not compile");
    assert!(e.contains("unsupported top-level statement"), "{e}");
    assert!(e.contains("line 3"), "{e}");
}
