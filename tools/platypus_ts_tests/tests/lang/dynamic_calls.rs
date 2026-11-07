// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Calls through dynamic `any` / `unknown` values (design: `docs/research/platypus-dynamic-values.md`,
//! phase P3): boxed closures called through `any`, adapters back to function types, methods of
//! class instances, plain objects, strings, arrays, maps, sets and numbers through `any`, and
//! `for..of` over `any`. The cases of `bench/dynamic_calls_cases.ts` run in the normal, stress GC
//! and collection-off modes and are compared with bun (`bench/gen_cases_expected.ts
//! dynamic_calls_cases.ts`). IDs are in TESTS.md (M9-TS-3576..).
use crate::cases::{case, run};
use serde_json::Value;

const CASES: &str = include_str!("../../bench/dynamic_calls_cases.ts");

fn expected(name: &str) -> String {
    let v: Value =
        serde_json::from_str(include_str!("../../bench/dynamic_calls_expected.json")).unwrap();
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
    3576 call_boxed_closures_with_each_argument_kind "dc0_" 1500000;
    3577 arity_missing_and_extra_arguments "dc1_" 1501000;
    3578 adapters_both_ways "dc2_" 1502000;
    3579 captured_variables_after_scope_returns "dc3_" 1503000;
    3580 recursion_through_any "dc4_" 1504000;
    3581 exceptions_through_dynamic_calls "dc5_" 1505000;
    3582 record_of_unknown_functions "dc6_" 1506000;
    3583 methods_of_class_instances "dc7_" 1507000;
    3584 function_fields_and_plain_objects "dc8_" 1508000;
    3585 string_methods "dc9_" 1509000;
    3586 string_methods_edge_cases "dc10_" 1510000;
    3587 array_methods_mutating "dc11_" 1511000;
    3588 array_methods_callbacks "dc12_" 1512000;
    3589 array_methods_queries_and_edge_cases "dc13_" 1513000;
    3590 array_reduce_of_empty_throws "dc14_" 1514000;
    3591 map_and_set_methods "dc15_" 1515000;
    3592 number_boolean_and_function_methods "dc16_" 1516000;
    3593 for_of_each_iterable_kind "dc17_" 1517000;
    3594 for_of_type_errors "dc18_" 1518000;
    3595 not_a_function_errors "dc19_" 1519000;
    3596 optional_calls_on_any "dc20_" 1520000;
    3597 many_dynamic_calls_checksum "dc21_" 1521000;
    3598 gc_closures_kept_only_through_any_slots "dc22_" 1522000;
    3599 gc_adapters_survive_collections "dc23_" 1523000;
    3600 gc_arguments_survive_a_collection_in_the_callee "dc24_" 1524000;
    3601 long_loop_boxes_calls_and_allocates "dc25_" 1525000;
    3602 million_dynamic_calls_checksum "dc26_" 1526000;
    3606 typed_map_set_keeps_its_key_across_a_collection "dc27_" 1527000;
}

// M9-TS-3603: a host (Rust) calls a closure a script boxed into a dynamic value, with dynamic
// arguments, through `web_ui_rt::dynv_call::call` (what `web_api_wasm` needs for imports): the
// result comes back as a dynamic word, a thrown error as `CallError::Thrown`, and a value that is
// not a function as a `TypeError` the runtime recorded.
#[test]
fn a_host_calls_a_boxed_closure_with_dynamic_arguments() {
    use web_ui_rt::dynv_call::{CallError, as_number, box_number, call, is_object_word};
    let _serial = platypus_ts_tests::serial();
    assert_ne!(web_ui_rt::link_anchor(), 0);
    let game = web_ui_ts::jit::compile_and_load(
        r##"
export function getAdd(): unknown { return (a: number, b: number): number => a * 10 + b; }
export function getConcat(): unknown { return (s: string, n: number): string => s + "#" + n; }
export function getThrower(): unknown { return (m: string): void => { throw new RangeError(m); }; }
"##,
        &web_ui_ts::jit::JitOptions {
            prefix: "dch_",
            struct_base: 1_530_000,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let get = |name: &str| -> u64 {
        let f: unsafe extern "C" fn() -> u64 =
            unsafe { std::mem::transmute(game.function(name).unwrap()) };
        unsafe { f() }
    };
    let add = get("getAdd");
    let r = call(add, &[box_number(4.0), box_number(2.0)]).unwrap();
    assert_eq!(as_number(r), Some(42.0));
    // Missing arguments are `undefined`: a number parameter refuses it.
    match call(add, &[box_number(1.0)]) {
        Err(CallError::Thrown(e)) => assert!(is_object_word(e)),
        other => panic!("{other:?}"),
    }
    let concat = get("getConcat");
    let s = unsafe { web_ui_rt::ts_rt_str_new(b"ab".as_ptr(), 2) } as u64;
    let r = call(concat, &[s, box_number(7.0)]).unwrap();
    assert!(is_object_word(r));
    let text = unsafe { web_ui_rt::Str::from_raw(r as *const web_ui_rt::Str) }.to_string_lossy();
    assert_eq!(text, "ab#7");
    let thrower = get("getThrower");
    let m = unsafe { web_ui_rt::ts_rt_str_new(b"bad".as_ptr(), 3) } as u64;
    match call(thrower, &[m]) {
        Err(CallError::Thrown(e)) => assert!(is_object_word(e)),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        call(box_number(1.0), &[]),
        Err(CallError::Error(2.0, "1 is not a function".into()))
    );
}

// M9-TS-3604: a long loop of dynamic calls that box, allocate and drop does not keep its garbage:
// after it (and a collection) the live heap is about what it was before.
#[test]
fn a_long_dynamic_loop_leaves_no_garbage_behind() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(web_ui_rt::link_anchor(), 0);
    let game = web_ui_ts::jit::compile_and_load(
        r#"
export function work(n: number): number {
  const f: any = (xs: any, s: string): any => [xs, s + "!"];
  const g: any = (v: any): number => (v as any[]).length;
  let total = 0;
  for (let i = 0; i < n; i++) {
    const r = f([i, "x" + i], "k");
    total += g(r) as number;
  }
  return total;
}
"#,
        &web_ui_ts::jit::JitOptions {
            prefix: "dcm_",
            struct_base: 1_531_000,
            ..Default::default()
        },
    )
    .unwrap();
    game.init().unwrap();
    let work: unsafe extern "C" fn(f64) -> f64 =
        unsafe { std::mem::transmute(game.function("work").unwrap()) };
    assert_eq!(unsafe { work(1000.0) }, 2000.0);
    web_ui_rt::ts_rt_gc_collect();
    let before = web_ui_rt::gc_stats();
    assert_eq!(unsafe { work(200_000.0) }, 400_000.0);
    web_ui_rt::ts_rt_gc_collect();
    let after = web_ui_rt::gc_stats();
    assert!(
        after.collections > before.collections + 2,
        "the loop collected: {before:?} -> {after:?}"
    );
    assert!(
        after.live_bytes <= before.live_bytes + 4096,
        "garbage survived: {before:?} -> {after:?}"
    );
}

// M9-TS-3609: where calls through `any` differ from JavaScript on purpose (design section 6):
// a typed array or map seen through `any` keeps its element type, so a write of another type is a
// `TypeError`; a detached method keeps its `this`; a closure called with a wrong argument type
// fails at its boundary (checked unboxing).
#[test]
fn dynamic_calls_keep_typed_storage_and_check_their_arguments() {
    let got = crate::cases::run(
        r#"
class C { n: number = 2; get(): number { return this.n; } }
function tryIt(f: () => void): string {
  try { f(); return "ok"; } catch (e) { return (e as Error).name + ": " + (e as Error).message; }
}
function t(): string {
  const nums: number[] = [1, 2];
  const xs: any = nums;
  const m: any = new Map<string, number>();
  const half: any = (x: number): number => x / 2;
  const c: any = new C();
  const out: string[] = [];
  out.push(tryIt(() => { xs.push(3); }));
  out.push(tryIt(() => { xs.push("a"); }));
  out.push(tryIt(() => { m.set("k", true); }));
  out.push(tryIt(() => { half("4"); }));
  const g = c.get;
  out.push(String(g()) + " " + nums.length);
  return out.join("\n");
}
"#,
        "dcd_",
        1_532_000,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        got,
        "ok\n\
         TypeError: expected a number (the array holds numbers), got string\n\
         TypeError: expected a number (the map holds numbers), got boolean\n\
         TypeError: expected a number, got string\n\
         2 3"
    );
}

// M9-TS-3605: what calls through dynamic values do not do yet says so at compile time: spreading
// arguments into a call of a dynamic value.
#[test]
fn what_dynamic_calls_cannot_do_yet_is_said_plainly() {
    let e = web_ui_ts::lower::lower_source(
        "export function f(g: any, xs: number[]): any { return g(...xs); }",
    )
    .err()
    .unwrap();
    assert!(
        e.contains("spreading arguments into a call of a dynamic value is not supported yet"),
        "{e}"
    );
}
