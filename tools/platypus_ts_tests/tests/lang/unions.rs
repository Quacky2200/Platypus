// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! String-literal unions as real types: locals, parameters, returns, fields, arrays, maps,
//! `switch` and `===`, and optional fields assigned to locals. The programs of
//! `bench/unions_cases.ts` run in the normal, stress GC and collection-off modes and are compared
//! with bun (`bench/gen_cases_expected.ts`). IDs are in TESTS.md (M9-TS-3000..).
use crate::cases::{case, run};
use serde_json::Value;

const CASES: &str = include_str!("../../bench/unions_cases.ts");

fn expected(name: &str) -> String {
    let v: Value = serde_json::from_str(include_str!("../../bench/unions_expected.json")).unwrap();
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
    3000 from_optional_field "from_optional_field" "un0_" 300000;
    3001 from_optional_field_with_as "from_optional_field_with_as" "un1_" 301000;
    3002 from_nullish_coalescing "from_nullish_coalescing" "un2_" 302000;
    3003 argument_and_return "argument_and_return" "un3_" 303000;
    3004 fields_arrays_maps "fields_arrays_maps" "un4_" 304000;
    3005 narrowing_switch "narrowing_switch" "un5_" 305000;
    3006 compare_and_indexof "compare_and_indexof" "un6_" 306000;
    3007 string_interop "string_interop" "un7_" 307000;
    3008 keyof_literals "keyof_literals" "un8_" 308000;
    3009 loop_with_optional_state "loop_with_optional_state" "un9_" 309000;
    3010 json_and_gc_churn "json_and_gc_churn" "un10_" 310000;
    3011 switch_on_optional_and_returns "switch_on_optional_and_returns" "un11_" 311000;
}

// M9-TS-3012
#[test]
fn a_literal_outside_the_union_is_a_compile_error() {
    let _serial = platypus_ts_tests::serial();
    let e = web_ui_ts::jit::compile_and_load(
        "type M = \"a\" | \"b\";
export function f(): string { const m: M = \"c\"; return m; }",
        &web_ui_ts::jit::JitOptions {
            prefix: "un12_",
            struct_base: 312_000,
            ..Default::default()
        },
    )
    .err()
    .expect("should not compile");
    assert!(
        e.contains("\"c\" is not assignable to type \"a\" | \"b\""),
        "{e}"
    );
    assert!(e.contains("line 2"), "{e}");
}

// M9-TS-3013: a conditional of two object shapes that can't unify names both shapes by their
// fields, not by their internal ids.
#[test]
fn a_conditional_of_different_shapes_names_both_fields() {
    let _serial = platypus_ts_tests::serial();
    let e = web_ui_ts::jit::compile_and_load(
        "export function f(v: boolean): number { let s = v ? { bottom: 1 } : { top: 2 }; s = 3; return 0; }",
        &web_ui_ts::jit::JitOptions {
            prefix: "un13_",
            struct_base: 313_000,
            ..Default::default()
        },
    )
    .err()
    .expect("should not compile");
    assert!(
        e.contains("cannot unify { bottom: f64 } and { top: f64 }"),
        "{e}"
    );
}
