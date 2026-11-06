// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Regular expressions in compiled code (`bench/regexp.ts`): literals, `new RegExp`, `test`,
//! `exec`, `lastIndex`, flags, named groups and match objects. `bench/gen_regexp_expected.ts` ran
//! the same file in bun; every function here must return what bun returned, in the normal, stress
//! GC and collection-off modes. IDs are in TESTS.md (M9-TS-222..).
use platypus_ts_tests::rex::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_re_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/regexp_expected.json")).unwrap()
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

// M9-TS-222
//
// `exec` and `test`: the match object (`m[0]`, groups, `index`, `length`, `input`), a miss, and the
// regex's `source`, `flags` and `global`.
#[test]
fn exec_and_test_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("basics", ts_re_basics());
    });
}

// M9-TS-223
//
// A `g` regex steps through a string with `lastIndex`; a miss resets it to 0.
#[test]
fn global_regexes_keep_last_index_like_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("globalLoop", ts_re_globalLoop());
    });
}

// M9-TS-224
//
// Named groups (`m.groups.year`), a missing group name, and no `groups` when the regex has none.
#[test]
fn named_groups_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("namedGroups", ts_re_namedGroups());
    });
}

// M9-TS-225
//
// The flags `m s i y` change the matching as in JavaScript (sticky stops at `lastIndex`).
#[test]
fn flags_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("flags", ts_re_flags());
    });
}

// M9-TS-226
//
// Unicode: `u` matches a surrogate pair as one character, indices count UTF-16 units, property
// escapes and `\u{...}`.
#[test]
fn unicode_matching_and_utf16_indices_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("unicode", ts_re_unicode());
    });
}

// M9-TS-227
//
// Lookbehind (positive and negative), backreferences, nested alternation, lazy quantifiers,
// negated classes and word boundaries.
#[test]
fn engine_features_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("engineFeatures", ts_re_engineFeatures());
    });
}

// M9-TS-228
//
// `new RegExp(string, flags)`, `RegExp(..)` without `new`, copying a regex with new flags,
// canonical flag order, `source` escaping a slash, `String(re)`.
#[test]
fn dynamic_regexes_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dynamic", ts_re_dynamic());
    });
}

// M9-TS-229
//
// Bad patterns and flags are `SyntaxError`s with JavaScriptCore's text.
#[test]
fn syntax_errors_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("errors", ts_re_errors());
    });
}

// M9-TS-230
//
// A match is an array of strings too: destructuring, spread, `for...of`, `map`.
#[test]
fn a_match_object_works_as_an_array() {
    in_every_gc_mode(|| unsafe {
        check_str("matchAsArray", ts_re_matchAsArray());
    });
}
