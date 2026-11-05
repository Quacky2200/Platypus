// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! TypeScript-semantics fixes: `flat(depth)` with a depth known only at run time, `this` in
//! object literal methods and accessors, `Object.assign` into objects by key, string array
//! parameters of host functions. The programs of `bench/lang3_cases.ts` run in the normal, stress
//! GC and collection-off modes and are compared with bun (`bench/gen_cases_expected.ts`). IDs are
//! in TESTS.md (M9-TS-3313..).
use crate::cases::{case, run, run_with};
use serde_json::Value;
use ts_api::ts_api;

const CASES: &str = include_str!("../../bench/lang3_cases.ts");

fn expected(name: &str) -> String {
    let v: Value = serde_json::from_str(include_str!("../../bench/lang3_expected.json")).unwrap();
    v[name].as_str().unwrap().to_string()
}

fn bun_case(name: &str, prefix: &'static str, base: u32) {
    let got = run(case(CASES, name), prefix, base).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(got, expected(name), "{name}");
}

macro_rules! bun_tests {
    ($( $id:literal $fn:ident $name:literal $prefix:literal $base:literal; )*) => {$(
        #[doc = concat!("M9-TS-", stringify!($id))]
        #[test]
        fn $fn() {
            bun_case($name, $prefix, $base);
        }
    )*};
}

bun_tests! {
    3313 flat_with_a_run_time_depth_over_two_levels_returns_a_union_array "flat_run_time_depth_two_levels" "m1_" 2300000;
    3314 flat_with_a_run_time_depth_over_three_levels_flattens_exactly_that_deep "flat_run_time_depth_three_levels" "m2_" 2301000;
    3315 flat_with_a_constant_depth_keeps_its_exact_type "flat_constant_depths_keep_exact_types" "m3_" 2302000;
    3317 this_in_object_literal_methods_and_accessors_is_the_object "this_in_object_literal_methods_and_accessors" "m5_" 2304000;
    3319 object_assign_copies_a_dictionary_into_an_object_by_key "object_assign_a_dictionary_into_an_object" "m7_" 2306000;
    3320 spreading_a_dictionary_into_an_object_type_or_a_dictionary "spread_a_dictionary_into_an_object_type_and_into_a_dictionary" "m8_" 2307000;
    3321 object_keys_values_and_entries_read_the_fields_of_an_object "object_keys_values_and_entries_of_objects" "m9_" 2308000;
    3318 this_in_literal_methods_with_a_declared_type_nesting_and_closures "this_in_literal_methods_with_a_declared_type_and_nesting" "m6_" 2305000;
}

// M9-TS-3316
//
// `xs.flat(depth) as number[]` with a depth that does not reach the numbers: bun returns the
// nested arrays under a wrong type; here the cast names the problem with a TypeError (documented
// in docs/platypus-ts.md, so no bun comparison).
#[test]
fn a_cast_of_a_flat_result_to_a_member_type_throws_when_the_depth_is_too_shallow() {
    let got = run(
        case(CASES, "flat_cast_with_a_too_shallow_depth"),
        "m4_",
        2_303_000,
    )
    .unwrap();
    assert!(
        got.starts_with("threw:") && got.contains("flat depth"),
        "{got}"
    );
}

// M9-TS-3322
//
// A dictionary value that does not fit the field of the object it is assigned to is a TypeError
// naming the key and the struct (bun stores the number in the string field: no bun comparison).
#[test]
fn object_assign_of_a_dictionary_value_of_the_wrong_type_throws_naming_the_key() {
    let got = run(
        case(CASES, "object_assign_a_dictionary_value_of_the_wrong_type"),
        "ma_",
        2_309_000,
    )
    .unwrap();
    assert!(
        got.starts_with("threw:") && got.contains("`title`") && got.contains("Named"),
        "{got}"
    );
}

/// A small host API with string array parameters (`&[String]` and `Vec<String>`).
#[ts_api(namespace = "Words")]
pub mod words_api {
    /// The words with a separator between them.
    pub fn join(words: &[String], sep: &str) -> String {
        words.join(sep)
    }

    /// How many words are not empty (the function owns the list).
    pub fn count(words: Vec<String>) -> u32 {
        words.into_iter().filter(|w| !w.is_empty()).count() as u32
    }

    /// The longest word (the first one on a tie).
    pub fn longest(words: &[String]) -> String {
        let mut best = String::new();
        for w in words {
            if w.chars().count() > best.chars().count() {
                best = w.clone();
            }
        }
        best
    }
}

// M9-TS-3323
#[test]
fn a_compiled_program_passes_string_arrays_to_a_host_function() {
    let name = "string_arrays_into_a_host_function";
    let manifests = [words_api::MANIFEST.to_json()];
    let got = run_with(case(CASES, name), "mb_", 2_310_000, &manifests)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(got, expected(name), "{name}");
    let dts = words_api::MANIFEST.to_dts();
    assert!(
        dts.contains("function join(words: string[], sep: string): string;"),
        "{dts}"
    );
}
