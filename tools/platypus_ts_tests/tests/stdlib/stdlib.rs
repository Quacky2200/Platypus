// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Standard library pieces in compiled code (`bench/stdlib.ts`): `typeof`, `Number` statics, `Math`
//! extras and `Array` methods. `bench/gen_stdlib_expected.ts` ran the same file in bun; every
//! function here must return what bun returned, in the normal, stress GC and collection-off
//! modes. IDs are in TESTS.md (M9-TS-199..).
use platypus_ts_tests::stdl::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_std_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/stdlib_expected.json")).unwrap()
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

// M9-TS-199
//
// `typeof` of numbers, strings, booleans, functions, objects, arrays, a missing value and a name
// that was never declared, and `typeof x === "..."` comparisons.
#[test]
fn typeof_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("typeofs", ts_std_typeofs());
    });
}

// M9-TS-200
//
// `Number.isInteger/isNaN/isFinite/isSafeInteger`, the global `isNaN` and `isFinite`, the number
// constants, `Number.parseFloat` and `Number.parseInt`.
#[test]
fn number_statics_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("numberStatics", ts_std_numberStatics());
    });
}

// M9-TS-201
//
// `Math.sinh cosh tanh asinh acosh atanh expm1 log1p` (to ten decimals).
#[test]
fn math_extras_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("mathExtras", ts_std_mathExtras());
    });
}

// M9-TS-202
//
// `toSorted`, `toReversed`, `findLast`, `findLastIndex`, `reduceRight`, `flat`, `flatMap`, the
// default `sort()` of numbers (by string value) and strings, and `entries()` / `keys()` /
// `values()` in `for...of`.
#[test]
fn array_methods_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("arrayGaps", ts_std_arrayGaps());
    });
}

thread_local! {
    static LOGGED: std::cell::RefCell<Vec<(i32, String)>> = const { std::cell::RefCell::new(Vec::new()) };
}

extern "C" fn capture(level: i32, text: *const u8, len: usize) {
    let s = unsafe { std::str::from_utf8(std::slice::from_raw_parts(text, len)) }.unwrap();
    LOGGED.with(|l| l.borrow_mut().push((level, s.to_string())));
}

// M9-TS-203
//
// `console.log/warn/error` join their arguments with spaces (as bun prints them) and go to the
// host's console hook with the level.
#[test]
fn console_calls_reach_the_hosts_hook() {
    web_ui_rt::ts_rt_set_console(Some(capture));
    in_every_gc_mode(|| unsafe {
        LOGGED.with(|l| l.borrow_mut().clear());
        assert_eq!(ts_std_logs(), 4.0);
        let got = LOGGED.with(|l| l.borrow().clone());
        let want: Vec<(i32, String)> = vec![
            (0, "a 1 true".into()),
            (3, "w:2".into()),
            (4, "e null".into()),
            (0, "".into()),
        ];
        assert_eq!(got, want);
    });
    web_ui_rt::ts_rt_set_console(None);
}

// M9-TS-204
//
// `Object.keys/values/entries/assign/freeze/hasOwn`, `in`, object spread (new fields, later
// values winning, source order) and rest, and array rest, equal to bun.
#[test]
fn object_functions_spread_and_rest_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("objects", ts_std_objects());
    });
}
