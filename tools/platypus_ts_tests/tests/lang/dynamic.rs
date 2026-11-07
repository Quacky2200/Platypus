// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Dynamic `any` / `unknown` values (design: `docs/research/platypus-dynamic-values.md`), phases
//! P0 (the representation, boxing, the operations that need no property access) and P1 (`catch`
//! of any value, `JSON.parse` / `JSON.stringify` of dynamic values, `Record<string, any>`).
//!
//! `typed_code_is_unchanged_by_dynamic_values` is the guard of the design's section 4: a program
//! that never says `any` compiles to the same IR and the same object code as before dynamic values
//! existed. The cases of `bench/dynamic_cases.ts` run in the normal, stress GC and collection-off
//! modes and are compared with bun (`bench/gen_cases_expected.ts dynamic_cases.ts`). IDs are in
//! TESTS.md (M9-TS-3539..).
use crate::cases::{case, run};
use serde_json::Value;
use std::path::Path;

// M9-TS-3539: the guard. `fixtures/sim.ts` (what `bench/bench.ts` runs: the zombie simulation,
// which compiles on its own, where bench.ts itself needs QuickJS's `globalThis`) and
// `examples/arcade` (React UI, scenes, physics: the UI prelude and most of the typed lowering) are
// compiled to IR and to an ELF object for a fixed triple (so the bytes do not depend on the
// machine's CPU) and each result is hashed (FNV-1a 64). `bench/dynamic_guard.json` holds the
// hashes of the build made before dynamic values were added. A change that touches typed code (a
// `Box` node, a dyn table, a spill slot, a changed array header constant) changes a hash and fails
// here. When a change to typed code is meant (a new optimisation), rerun with
// `PLATYPUS_UPDATE_GUARD=1` and commit the new file.
fn fnv(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn guard_manifests() -> Vec<String> {
    let mut all = platypus_engine_api::manifests();
    all.push(platypus_scene::api::scene_api::MANIFEST.to_json());
    all.extend(platypus_scenes::manifests());
    all.extend(platypus_physics_api::manifests());
    all.extend(platypus_render_api::manifests());
    all.extend(platypus_components::manifests());
    all.push(web_ui_bridge::manifest_json());
    all
}

fn guard_hashes(name: &str, source: &str, manifests: &[String]) -> Vec<(String, String)> {
    let target = web_ui_ts::codegen::ObjectTarget {
        triple: Some("x86_64-unknown-linux-gnu".into()),
        pic: true,
    };
    let out = web_ui_ts::compile_for_target(source, "speed", "guard_", 0, manifests, &target)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    vec![
        (format!("{name}.ir"), fnv(out.ir.as_bytes())),
        (format!("{name}.object"), fnv(&out.object)),
    ]
}

#[test]
fn typed_code_is_unchanged_by_dynamic_values() {
    let _serial = platypus_ts_tests::serial();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut got: Vec<(String, String)> = Vec::new();
    let bench = web_ui_ts::modules::bundle(&root.join("fixtures/sim.ts")).unwrap();
    got.extend(guard_hashes("sim", &bench.source, &[]));
    let entry = root.join("../../examples/arcade/src/main.tsx");
    let bundle = web_ui_ts::modules::bundle(&entry).unwrap();
    got.extend(guard_hashes("arcade", &bundle.source, &guard_manifests()));

    let file = root.join("bench/dynamic_guard.json");
    if std::env::var_os("PLATYPUS_UPDATE_GUARD").is_some() {
        let map: serde_json::Map<String, Value> = got
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        std::fs::write(
            &file,
            serde_json::to_string_pretty(&Value::Object(map)).unwrap() + "\n",
        )
        .unwrap();
        return;
    }
    let want: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    for (k, v) in &got {
        assert_eq!(
            want[k].as_str(),
            Some(v.as_str()),
            "{k}: typed code compiles differently than before dynamic values (see the test's comment)"
        );
    }
}

const CASES: &str = include_str!("../../bench/dynamic_cases.ts");

fn expected(name: &str) -> String {
    let v: Value = serde_json::from_str(include_str!("../../bench/dynamic_expected.json")).unwrap();
    v[name].as_str().unwrap().to_string()
}

fn bun_case(name: &str, prefix: &'static str, base: u32) {
    let got = run(case(CASES, name), prefix, base).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(got, expected(name), "{name}");
}

macro_rules! bun_tests {
    ($( $id:literal $name:ident $prefix:literal $base:literal; )*) => {$(
        #[doc = concat!("M9-TS-", stringify!($id))]
        #[test]
        fn $name() {
            bun_case(stringify!($name), $prefix, $base);
        }
    )*};
}

bun_tests! {
    3540 box_and_unbox_round_trips "dy0_" 340000;
    3541 typeof_of_each_kind "dy1_" 341000;
    3542 truthiness "dy2_" 342000;
    3543 null_and_undefined "dy3_" 343000;
    3544 strict_equality "dy4_" 344000;
    3545 typeof_narrowing "dy5_" 345000;
    3546 instanceof_narrowing "dy6_" 346000;
    3547 string_conversions "dy7_" 347000;
    3548 as_any_and_back "dy8_" 348000;
    3549 any_params_returns_fields "dy9_" 349000;
    3550 any_arrays "dy10_" 350000;
    3551 mixed_array_of_unknown "dy11_" 351000;
    3552 catch_non_errors "dy12_" 352000;
    3553 catch_error_through_unknown "dy13_" 353000;
    3554 finally_passes_values_through "dy14_" 354000;
    3555 json_parse_dynamic "dy15_" 355000;
    3556 json_round_trips_and_pretty_print "dy16_" 356000;
    3557 json_parse_errors "dy17_" 357000;
    3558 get_of_undefined_throws "dy18_" 358000;
    3559 optional_chaining_and_index_on_any "dy19_" 359000;
    3560 record_of_any "dy20_" 360000;
    3561 record_of_unknown_from_json "dy21_" 361000;
    3562 any_in_objects_and_classes_through_generics "dy22_" 362000;
    3563 json_stringify_of_other_dynamic_values "dy23_" 363000;
    3564 any_survives_collections "dy24_" 364000;
    3565 boxing_bool_and_nullable_arrays "dy25_" 365000;
    3566 new_array_slots_are_undefined "dy26_" 366000;
    3567 any_in_class_fields_and_json "dy27_" 367000;
}

/// Runs `t()` of a program that does not exist for bun (a deviation, or a message of ours).
fn ours(src: &str, prefix: &'static str, base: u32) -> String {
    run(src, prefix, base).unwrap_or_else(|e| panic!("{e}"))
}

// M9-TS-3568: a conversion out of a dynamic value is checked, where TypeScript trusts it: a value
// of the wrong type is a `TypeError` at the boundary (design Q1), for numbers, strings, booleans,
// classes, arrays and nullable targets alike; the right type, `null` and `undefined` into a
// nullable target pass. (An `any[]` becomes a typed array by a copy that checks each element.)
#[test]
fn a_wrong_dynamic_value_at_a_typed_boundary_is_a_type_error() {
    let got = ours(
        r#"
class P { x: number = 1; }
class Q { y: number = 2; }
function tryIt(f: () => void): string {
  try { f(); return "ok"; } catch (e) { return (e as Error).name + ": " + (e as Error).message; }
}
function t(): string {
  const s: any = "text";
  const n: any = 5;
  const o: any = new P();
  const a: any = [1, 2];
  const u: any = undefined;
  const out: string[] = [];
  out.push(tryIt(() => { const x: number = s; out.push(String(x)); }));
  out.push(tryIt(() => { const x: string = n; out.push(x); }));
  out.push(tryIt(() => { const x: boolean = n; out.push(String(x)); }));
  out.push(tryIt(() => { const x: Q = o; out.push(String(x.y)); }));
  out.push(tryIt(() => { const x: P = o; out.push(String(x.x)); }));
  out.push(tryIt(() => { const x: string[] = a; out.push(String(x.length)); }));
  out.push(tryIt(() => { const x: number[] = a; out.push(String(x.length)); }));
  out.push(tryIt(() => { const x: number | null = u; out.push(String(x)); }));
  out.push(tryIt(() => { const x: number | null = s; out.push(String(x)); }));
  out.push(tryIt(() => { const x: P | null = u; out.push(String(x === null)); }));
  out.push(tryIt(() => { const x: P = u; out.push(String(x.x)); }));
  return out.join("\n");
}
"#,
        "dy28_",
        368_000,
    );
    assert_eq!(
        got,
        "TypeError: expected a number, got string\n\
         TypeError: expected a string, got number\n\
         TypeError: expected a boolean, got number\n\
         TypeError: expected an object of Q, got object\n\
         1\nok\n\
         TypeError: expected an array of strings, got object\n\
         2\nok\n\
         null\nok\n\
         TypeError: expected a number, got string\n\
         true\nok\n\
         TypeError: expected an object of P, got undefined"
    );
}

// M9-TS-3569: `JSON.parse` into `Record<string, any>` or `any[]` is the dynamic parse followed by
// the checked conversion, so the shape is still validated: an array where an object goes (and the
// other way) is a `TypeError`, not a wrong value; a syntax error is still a `SyntaxError`.
#[test]
fn a_typed_json_parse_of_a_dynamic_shape_is_validated() {
    let got = ours(
        r#"
function tryIt(f: () => void): string {
  try { f(); return "ok"; } catch (e) { return (e as Error).name; }
}
function t(): string {
  const out: string[] = [];
  out.push(tryIt(() => { const r: Record<string, any> = JSON.parse("[1]"); out.push(String(r["k"])); }));
  out.push(tryIt(() => { const a: any[] = JSON.parse("{}"); out.push(String(a.length)); }));
  out.push(tryIt(() => { const a: any[] = JSON.parse("[1,2,3]"); out.push(String(a.length)); }));
  out.push(tryIt(() => { const r: Record<string, any> = JSON.parse("{\"k\":1}"); out.push(String(r["k"])); }));
  out.push(tryIt(() => { const r: Record<string, any> = JSON.parse("{"); out.push(String(r["k"])); }));
  return out.join(",");
}
"#,
        "dy29_",
        369_000,
    );
    assert_eq!(got, "TypeError,TypeError,3,ok,1,ok,SyntaxError");
}

// M9-TS-3570: a program that throws only errors keeps `catch (e)` an `Error` (the typed fast path:
// `e.message` with no unboxing), also when `e` is annotated `unknown`; one `throw` of anything else
// makes every `catch` of the program bind a dynamic value, and an error object thrown there is
// still the same object (`e instanceof RangeError`, `(e as Error).message`).
#[test]
fn catch_binds_a_dynamic_value_only_when_the_program_throws_one() {
    let only_errors = web_ui_ts::lower::lower_source(
        "export function f(): string { try { throw new RangeError(\"r\"); } catch (e) { return e.message; } }",
    )
    .unwrap();
    assert!(
        !web_ui_ts::ir::dump(&only_errors).contains("ts_rt_dyn"),
        "a program that throws only errors binds no dynamic value"
    );
    let mixed = web_ui_ts::lower::lower_source(
        "export function f(): string { try { throw 1; } catch (e) { return String(e); } }",
    )
    .unwrap();
    assert!(
        web_ui_ts::ir::dump(&mixed).contains("ts_rt_dyn"),
        "a throw of a number makes catch bind a dynamic value"
    );
    let got = ours(
        r#"
function t(): string {
  try { throw new RangeError("r"); } catch (e) {
    const same = e instanceof RangeError;
    return String(same) + (e as Error).message + String(e);
  }
}
function other(): void { throw 7; }
"#,
        "dy30_",
        370_000,
    );
    assert_eq!(got, "truerRangeError: r");
}

// M9-TS-3571: what stays unsupported says so. A value that is not a dynamic one can still not be
// compared with `==` against one (only `=== ` and `== null`), and a typed object has no property
// reads through `any` until phase P2.
#[test]
fn what_dynamic_values_cannot_do_yet_is_said_plainly() {
    let e = web_ui_ts::lower::lower_source("export function f(v: any): boolean { return v == 1; }")
        .err()
        .unwrap();
    assert!(
        e.contains("`==` with a dynamic value is not supported yet"),
        "{e}"
    );
    let got = ours(
        r#"
class K { n: number = 1; }
function t(): string {
  const v: any = new K();
  try { return String(v.n); } catch (e) { return (e as Error).name + ": " + (e as Error).message; }
}
"#,
        "dy31_",
        371_000,
    );
    assert_eq!(
        got,
        "TypeError: reading a property of a typed object through a dynamic value is not supported yet"
    );
}
