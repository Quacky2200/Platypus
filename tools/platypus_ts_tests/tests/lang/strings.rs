// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Strings in compiled code (`bench/strings.ts`): `+` and `+=`, template literals, methods,
//! comparisons, `switch`, arrays of strings, nullable strings and conversions. `bench/strings.ts`
//! also runs in bun; `bench/gen_strings_expected.ts` wrote what bun returned
//! (`bench/strings_expected.json`) and every function here must return the same, in the normal,
//! stress GC and collection-off modes. IDs are in TESTS.md (M9-TS-178..).
use platypus_ts_tests::strs::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_str_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/strings_expected.json")).unwrap()
}

fn check_num(name: &str, got: f64) {
    let want = expected()[name].as_f64().unwrap();
    assert_eq!(got, want, "{name}");
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
    assert_eq!(got, want, "{name} (units)");
}

// M9-TS-178
//
// Lengths, comparison, searching, `switch`, truthiness and parsing in compiled code give what bun
// gives (`length` in UTF-16 units, ordering by code unit).
#[test]
fn numbers_from_strings_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_num("length", ts_str_length());
        check_num("compare", ts_str_compare());
        check_num("search", ts_str_search());
        check_num("parse", ts_str_parse());
        check_num("unicode", ts_str_unicode());
        check_num("loop", ts_str_loop());
        check_num("switching", ts_str_switching());
        check_num("truthy", ts_str_truthy());
    });
}

// M9-TS-179
//
// `+`, `+=` (with numbers, booleans and null), template literals, method chains, padding,
// `fromCharCode`, `toFixed`, `toString(16)` and `split`/`join`.
#[test]
fn built_strings_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("concat", ts_str_concat());
        check_str("template", ts_str_template());
        check_str("chain", ts_str_chain());
        check_str("splitJoin", ts_str_splitJoin());
        check_str("numbers", ts_str_numbers());
        check_str("pad", ts_str_pad());
        check_str("chars", ts_str_chars());
        check_str("nested", ts_str_nested());
    });
}

// M9-TS-180
//
// Strings in arrays (`includes`, `indexOf`, `sort` with a comparator), in object fields, as
// `string | null`, and mixed with string-literal unions.
#[test]
fn strings_in_arrays_fields_and_unions_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("inArrays", ts_str_inArrays());
        check_str("sorting", ts_str_sorting());
        check_str("nullable", ts_str_nullable());
        check_str("fields", ts_str_fields());
        check_str("literalUnion", ts_str_literalUnion());
    });
}

// M9-TS-181
//
// Three hundred strings made in a loop with a collection at every allocation: the strings that
// are kept survive, the rest are freed, and the result still equals bun's.
#[test]
fn many_strings_survive_collections() {
    in_every_gc_mode(|| unsafe {
        check_num("manyStrings", ts_str_manyStrings());
    });
}
