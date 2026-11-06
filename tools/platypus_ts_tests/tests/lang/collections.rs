// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Map`, `Set`, `WeakMap` and `WeakSet` in compiled code (`bench/collections.ts`).
//! `bench/gen_collections_expected.ts` ran the same file in bun; every function here must return what
//! bun returned, in the normal, stress GC and collection-off modes. IDs are in TESTS.md
//! (M9-TS-209..).
use platypus_ts_tests::cols::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_col_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/collections_expected.json")).unwrap()
}

fn check_num(name: &str, got: f64) {
    assert_eq!(got, expected()[name].as_f64().unwrap(), "{name}");
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

// M9-TS-209
//
// `Map` with string keys: `set` (chained, overwriting), `get`, `has`, `delete`, `size`, `clear`,
// `keys()` / `values()` as arrays, `for...of` over the map and its keys and values, `forEach`.
#[test]
fn map_basics_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("mapBasics", ts_col_mapBasics());
        check_str(
            "stringKeysAndArrayValues",
            ts_col_stringKeysAndArrayValues(),
        );
    });
}

// M9-TS-210
//
// Numbers as keys follow SameValueZero (`NaN` is a key, `-0` is `0`); objects are keys by
// identity (a different object with the same field is a different key).
#[test]
fn number_and_object_keys_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("numberKeys", ts_col_numberKeys());
        check_str("objectKeys", ts_col_objectKeys());
    });
}

// M9-TS-211
//
// `Set`: built from an array, `add` (chained), `has`, `delete`, `size`, spread, `Array.from`,
// `for...of`, `forEach`, and the element type inferred from `new Set(["b", "a"])`.
#[test]
fn set_basics_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("setBasics", ts_col_setBasics());
    });
}

// M9-TS-212
//
// Iteration is live: entries deleted before they are reached are skipped, entries added during
// the loop are visited, and the table grows and rebuilds under the loop.
#[test]
fn iteration_survives_deletes_and_adds_like_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("mutateWhileIterating", ts_col_mutateWhileIterating());
    });
}

// M9-TS-213
//
// Two thousand string keys (half deleted, then more added) with a collection at every allocation,
// and `WeakMap` / `WeakSet` over objects.
#[test]
fn many_entries_and_weak_collections_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_num("manyEntries", ts_col_manyEntries());
        check_str("weak", ts_col_weak());
    });
}
