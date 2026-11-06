// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! JSON in compiled code (`bench/json.ts`): `JSON.stringify` and `JSON.parse` from the static types.
//! `bench/gen_json_expected.ts` ran the same file in bun; every function here must return what bun
//! returned, in the normal, stress GC and collection-off modes. IDs are in TESTS.md (M9-TS-215..).
use platypus_ts_tests::jsn::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_json_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/json_expected.json")).unwrap()
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

// M9-TS-215
//
// `JSON.stringify` of objects, arrays, strings with escapes and lone surrogates, numbers (`NaN`,
// `-0`, exponents), booleans, empty collections, with and without indentation (a number clamped to
// ten, or a string).
#[test]
fn stringify_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("stringifyBasic", ts_json_stringifyBasic());
        check_str(
            "stringifyUnionsAndNesting",
            ts_json_stringifyUnionsAndNesting(),
        );
    });
}

// M9-TS-216
//
// Optional fields (`price?`) are left out when absent and `T | null` fields print `null`; a subclass
// instance prints its own fields.
#[test]
fn optional_fields_and_subclasses_stringify_like_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str(
            "optionalFieldsAreOmitted",
            ts_json_optionalFieldsAreOmitted(),
        );
        check_str("polymorphic", ts_json_polymorphic());
    });
}

// M9-TS-217
//
// `JSON.parse` into a typed object (unknown fields skipped, a surrogate pair from two escapes, a
// missing optional field), back through `stringify`, and into arrays of numbers, strings, booleans
// and nullable numbers, and into string-literal unions.
#[test]
fn parse_builds_typed_values_like_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("roundTrip", ts_json_roundTrip());
        check_str("parseValues", ts_json_parseValues());
        check_str("parseUnions", ts_json_parseUnions());
        check_str("unknownFieldsAreSkipped", ts_json_unknownFieldsAreSkipped());
    });
}

// M9-TS-218
//
// Sixty-seven malformed texts raise a `SyntaxError` with the text bun gives.
#[test]
fn parse_errors_have_javascripts_class_and_text() {
    in_every_gc_mode(|| unsafe {
        check_str("parseErrors", ts_json_parseErrors());
    });
}

// M9-TS-219
//
// Three hundred objects stringified, parsed and stringified again, with a collection at every
// allocation.
#[test]
fn many_objects_round_trip_under_collection() {
    in_every_gc_mode(|| unsafe {
        check_str("manyObjects", ts_json_manyObjects());
    });
}

// M9-TS-220
//
// JSON that does not have the shape of the type is a `TypeError` (a missing property, a string for
// a number, an array for an object, a string outside a literal union); a syntax error anywhere in
// the text wins over a shape error, as the text is invalid whatever the type.
#[test]
fn a_shape_mismatch_is_a_type_error_and_a_syntax_error_wins() {
    in_every_gc_mode(|| unsafe {
        let got = ts_json_shapeMismatches();
        let s = Str::from_raw(got as *const Str).to_string_lossy();
        assert_eq!(
            s,
            "TypeError:JSON.parse: expected an object;\
             TypeError:JSON.parse: expected a number;\
             TypeError:JSON.parse: missing property \"name\";\
             SyntaxError:JSON Parse error: Expected ']';\
             TypeError:JSON.parse: the string is not one of the allowed values;"
        );
    });
}

// M9-TS-3344
//
// `JSON.parse(f())`: the text a call returns is kept alive while the parser allocates the value.
#[test]
fn parse_of_a_call_result_survives_collections() {
    in_every_gc_mode(|| unsafe {
        let got = ts_json_parseOfCall();
        let want: Vec<u16> = "50:1515".encode_utf16().collect();
        assert_eq!(Str::from_raw(got as *const Str).to_utf16(), want);
    });
}
