// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Promises beyond `bench/promises.ts`: generic promise-returning functions and classes,
//! `for await`, `Promise.withResolvers`, unhandled rejections, clear compile errors. The programs
//! of `bench/promises2_cases.ts` are compiled in the process (the JIT), run in the normal, stress
//! GC and collection-off modes and compared with bun (`bench/gen_promises2_expected.ts`). IDs are
//! in TESTS.md (M9-TS-2897..).
use serde_json::Value;
use std::cell::RefCell;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress, ts_rt_run_loop};
use web_ui_ts::jit::{JitOptions, compile_and_load};

const CASES: &str = include_str!("../../bench/promises2_cases.ts");

const GLUE: &str = "\nlet __r = \"pending\";\n\
export function start(): void {\n\
  t().then((v) => { __r = v; }).catch((e) => { __r = \"rejected:\" + e.message; });\n\
}\n\
export function result(): string { return __r; }\n";

thread_local! {
    static CONSOLE: RefCell<Vec<(i32, String)>> = const { RefCell::new(Vec::new()) };
}

extern "C" fn capture(level: i32, text: *const u8, len: usize) {
    let s = String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, len) }).into_owned();
    CONSOLE.with(|c| c.borrow_mut().push((level, s)));
}

/// What a program gave: the string `t()` resolved with and the console lines it wrote.
#[derive(Debug, PartialEq, Clone)]
struct Run {
    result: String,
    console: Vec<(i32, String)>,
}

/// Compiles `src` + glue and runs `t()` to completion in the three GC modes.
fn run(src: &str, prefix: &'static str, struct_base: u32) -> Result<Run, String> {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let game = compile_and_load(
        &format!("{src}{GLUE}"),
        &JitOptions {
            prefix,
            struct_base,
            ..Default::default()
        },
    )?;
    web_ui_rt::ts_rt_set_console(Some(capture));
    let mut out: Vec<Run> = Vec::new();
    for mode in 0..3 {
        match mode {
            1 => ts_rt_gc_stress(true),
            2 => {
                ts_rt_gc_stress(false);
                ts_rt_gc_enable(false);
            }
            _ => {}
        }
        CONSOLE.with(|c| c.borrow_mut().clear());
        game.init()?;
        let start: unsafe extern "C" fn() =
            unsafe { std::mem::transmute(game.function("start").unwrap()) };
        let result: unsafe extern "C" fn() -> *mut u8 =
            unsafe { std::mem::transmute(game.function("result").unwrap()) };
        unsafe { start() };
        ts_rt_run_loop();
        let p = unsafe { result() };
        assert!(!p.is_null());
        let result =
            String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16());
        out.push(Run {
            result,
            console: CONSOLE.with(|c| c.borrow().clone()),
        });
    }
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(true);
    web_ui_rt::ts_rt_set_console(None);
    if out.iter().any(|r| *r != out[0]) {
        return Err(format!("GC modes disagree: {out:?}"));
    }
    Ok(out.remove(0))
}

fn case(name: &str) -> &'static str {
    let mut parts = CASES.split("//## ");
    parts.next();
    for p in parts {
        let (head, body) = p.split_once('\n').unwrap();
        if head.trim() == name {
            return body;
        }
    }
    panic!("no case {name}")
}

fn expected(name: &str) -> String {
    let v: Value =
        serde_json::from_str(include_str!("../../bench/promises2_expected.json")).unwrap();
    v[name].as_str().unwrap().to_string()
}

/// A bun-compared case: `prefix` and `base` must be unique per case.
fn bun_case(name: &str, prefix: &'static str, base: u32) {
    let r = run(case(name), prefix, base).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(r.result, expected(name), "{name}");
    assert!(r.console.is_empty(), "{name}: console {:?}", r.console);
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
    2897 generic_function_returns_a_promise_of_its_type_parameter "generic_identity" "pg1_" 210_000;
    2898 generic_function_result_inferred_from_a_closure "generic_closure_result" "pg2_" 211_000;
    2899 generic_function_promises_an_array_of_its_type_parameter "generic_array_result" "pg3_" 212_000;
    2900 generic_function_promises_a_map_of_its_type_parameter "generic_map_result" "pg4_" 213_000;
    2901 generic_new_promise_with_a_timer "generic_delay" "pg5_" 214_000;
    2902 generic_retry_takes_a_closure_returning_a_promise "generic_retry" "pg6_" 215_000;
    2903 generic_then_chain_over_a_promise_parameter "generic_then_chain" "pg7_" 216_000;
    2904 generic_promise_all_over_a_mapped_array "generic_all_map" "pg8_" 217_000;
    2905 generic_class_holds_and_returns_promises "generic_class_with_promise" "pg9_" 218_000;
    2906 generic_promise_of_a_program_class "generic_with_class_value" "pg10_" 219_000;
    2907 generic_async_function_recurses "generic_recursive" "pg11_" 220_000;
    2908 async_methods_keep_this_across_awaits "class_async_methods" "pg12_" 221_000;
    2909 promises_of_interfaces_and_unions "interface_and_union_results" "pg13_" 222_000;
    2910 try_catch_finally_with_awaits_in_every_block "try_catch_finally_with_awaits" "pg14_" 223_000;
    2911 for_await_over_promises_and_plain_values "for_await_array_of_promises" "pg15_" 224_000;
    2912 promise_with_resolvers "with_resolvers" "pg16_" 225_000;
    2913 await_of_values_and_async_iifes "awaiting_values_and_iifes" "pg17_" 226_000;
    2914 timers_race_and_all_inside_async_code "timers_order_inside_async" "pg18_" 227_000;
    2915 rejection_paths_through_catch_finally_and_all_settled "rejection_paths" "pg19_" 228_000;
    3155 await_promise_all_of_different_types_gives_a_tuple "all_tuple_of_different_types" "pg20_" 231_000;
    3156 await_promise_all_tuple_rejects_with_the_first_failure "all_tuple_rejects_with_the_first_failure" "pg21_" 232_000;
}

// M9-TS-2916
#[test]
fn an_unhandled_rejection_is_reported_once_after_the_microtasks_and_does_not_stop_the_program() {
    let r = run(
        "async function boom(): Promise<number> { throw new RangeError(\"lost\"); }\n\
         async function t(): Promise<string> {\n\
           boom();\n\
           Promise.reject(new Error(\"second\"));\n\
           await Promise.resolve();\n\
           return \"went on\";\n\
         }",
        "pu1_",
        230_000,
    )
    .unwrap();
    assert_eq!(r.result, "went on");
    let lines: Vec<&str> = r.console.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(
        lines,
        [
            "Unhandled promise rejection: RangeError: lost (await it in a try/catch or add .catch)",
            "Unhandled promise rejection: Error: second (await it in a try/catch or add .catch)",
        ]
    );
    assert!(r.console.iter().all(|(level, _)| *level == 4), "errors");
}

// M9-TS-2917
#[test]
fn a_rejection_handled_in_the_same_turn_is_not_reported_and_a_chain_reports_its_end_once() {
    let r = run(
        "async function bad(): Promise<number> { throw new Error(\"x\"); }\n\
         async function t(): Promise<string> {\n\
           const p = bad();\n\
           const q = Promise.reject(new Error(\"late\"));\n\
           await Promise.resolve();\n\
           p.catch((e) => 1);\n\
           try { await q; } catch (e) {}\n\
           bad().then((v) => v + 1).then((v) => v + 2);\n\
           await Promise.allSettled([bad(), bad()]);\n\
           return \"ok\";\n\
         }",
        "pu2_",
        231_000,
    )
    .unwrap();
    assert_eq!(r.result, "ok");
    let lines: Vec<&str> = r.console.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(
        lines,
        ["Unhandled promise rejection: Error: x (await it in a try/catch or add .catch)"],
        "only the end of the unhandled chain"
    );
}

// M9-TS-2918
#[test]
fn an_exception_in_a_timer_or_microtask_callback_is_reported_and_the_rest_runs() {
    let r = run(
        "let log = \"\";\n\
         async function t(): Promise<string> {\n\
           setTimeout(() => { throw new Error(\"in timer\"); }, 1);\n\
           queueMicrotask(() => { throw new TypeError(\"in microtask\"); });\n\
           setTimeout(() => { log += \"after\"; }, 2);\n\
           await new Promise<void>((resolve) => { setTimeout(resolve, 5); });\n\
           return log;\n\
         }",
        "pu3_",
        232_000,
    )
    .unwrap();
    assert_eq!(r.result, "after");
    let lines: Vec<&str> = r.console.iter().map(|(_, s)| s.as_str()).collect();
    assert_eq!(
        lines,
        [
            "Uncaught exception in a timer or microtask: TypeError: in microtask",
            "Uncaught exception in a timer or microtask: Error: in timer",
        ]
    );
}

// M9-TS-2919
#[test]
fn unsupported_promise_shapes_are_clear_compile_errors() {
    let bad = |src: &str, prefix: &'static str| -> String {
        match run(src, prefix, 233_000) {
            Ok(r) => panic!("compiled and ran: {r:?}"),
            Err(e) => e,
        }
    };
    let e = bad(
        "async function t(): Promise<string> {\n\
           const p = Promise.all([Promise.resolve(1), Promise.resolve(\"x\")]);\n\
           return \"\" + (await p);\n\
         }",
        "pe1_",
    );
    assert!(
        e.contains("different value types") && e.contains("line 2"),
        "{e}"
    );
    let e = bad(
        "function f(): number { return await Promise.resolve(1); }\nasync function t(): Promise<string> { return \"\" + f(); }",
        "pe2_",
    );
    assert!(e.contains("only allowed within async functions"), "{e}");
    let e = bad(
        "async function inner(): Promise<Promise<number>> { return Promise.resolve(4); }\n\
         async function t(): Promise<string> { return \"\" + await inner(); }",
        "pe3_",
    );
    assert!(e.contains("promise of a promise"), "{e}");
    let e = bad(
        "async function* gen(): AsyncGenerator<number> { yield 1; }\n\
         async function t(): Promise<string> { return \"x\"; }",
        "pe4_",
    );
    assert!(e.contains("async generators"), "{e}");
    let e = bad(
        "async function t(): Promise<string> { for await (const x of [1]) { } return \"x\"; }\n\
         function f(): void { for await (const x of [1]) { } }",
        "pe5_",
    );
    assert!(
        e.contains("`for await` loops are only allowed within async functions"),
        "{e}"
    );
    let e = bad(
        "type Pt = { x: number };\n\
         async function make<T>(f: () => T): Promise<T> { return f(); }\n\
         async function t(): Promise<string> { const p = await make<Pt>(() => ({ x: 1 })); return \"\" + p.x; }",
        "pe6_",
    );
    assert!(e.contains("never writes `Promise<"), "{e}");
}
