// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! String methods that take a regular expression (`bench/regexp2.ts`): `match`, `matchAll`, `search`,
//! `replace`, `replaceAll` and `split`. `bench/gen_regexp2_expected.ts` ran
//! the same file in bun; every function here must return what bun returned, in the normal, stress
//! GC and collection-off modes. IDs are in TESTS.md (M9-TS-232..).
use platypus_ts_tests::rex2::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_re2_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/regexp2_expected.json")).unwrap()
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

// M9-TS-232
#[test]
fn search_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("search", ts_re2_search());
    });
}

// M9-TS-233
#[test]
fn match_plain_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("matchPlain", ts_re2_matchPlain());
    });
}

// M9-TS-233
#[test]
fn match_global_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("matchGlobal", ts_re2_matchGlobal());
    });
}

// M9-TS-233
#[test]
fn match_string_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("matchString", ts_re2_matchString());
    });
}

// M9-TS-234
#[test]
fn match_all_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("matchAll", ts_re2_matchAll());
    });
}

// M9-TS-234
#[test]
fn match_all_needs_g_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("matchAllNeedsG", ts_re2_matchAllNeedsG());
    });
}

// M9-TS-235
#[test]
fn replace_string_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("replaceString", ts_re2_replaceString());
    });
}

// M9-TS-236
#[test]
fn replace_regex_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("replaceRegex", ts_re2_replaceRegex());
    });
}

// M9-TS-237
#[test]
fn replace_function_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("replaceFunction", ts_re2_replaceFunction());
    });
}

// M9-TS-237
#[test]
fn replace_function_groups_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("replaceFunctionGroups", ts_re2_replaceFunctionGroups());
    });
}

// M9-TS-235
#[test]
fn replace_all_needs_g_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("replaceAllNeedsG", ts_re2_replaceAllNeedsG());
    });
}

// M9-TS-238
#[test]
fn split_regex_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("splitRegex", ts_re2_splitRegex());
    });
}

// M9-TS-238
#[test]
fn split_string_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("splitString", ts_re2_splitString());
    });
}
