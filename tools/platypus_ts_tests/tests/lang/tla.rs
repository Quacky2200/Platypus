// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Top-level `await`: the module's code runs as an implicit async function started by `ts_init`.
//! The programs of `bench/tla_cases.ts` run (init, then the event loop) in the normal, stress GC
//! and collection-off modes and are compared with bun (`bench/gen_tla_expected.ts`). IDs are in
//! TESTS.md (M9-TS-3157..).
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress, ts_rt_run_loop};
use web_ui_ts::jit::{JitOptions, compile_and_load};

const CASES: &str = include_str!("../../bench/tla_cases.ts");

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
    let v: Value = serde_json::from_str(include_str!("../../bench/tla_expected.json")).unwrap();
    v[name].as_str().unwrap().to_string()
}

/// Compiles `src`, runs its init in the three GC modes and returns `result()` right after the
/// init and after the event loop ran, per mode (all modes must agree).
fn run(
    src: &str,
    prefix: &'static str,
    base: u32,
    read_early: bool,
) -> Result<(String, String), String> {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let game = compile_and_load(
        src,
        &JitOptions {
            prefix,
            struct_base: base,
            ..Default::default()
        },
    )?;
    let mut out: Vec<(String, String)> = Vec::new();
    for mode in 0..3 {
        match mode {
            1 => ts_rt_gc_stress(true),
            2 => {
                ts_rt_gc_stress(false);
                ts_rt_gc_enable(false);
            }
            _ => {}
        }
        game.init()?;
        let result: unsafe extern "C" fn() -> *mut u8 =
            unsafe { std::mem::transmute(game.function("result").ok_or("no result")?) };
        let read = || {
            let p = unsafe { result() };
            assert!(!p.is_null());
            String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
        };
        let early = if read_early { read() } else { String::new() };
        ts_rt_run_loop();
        out.push((early, read()));
    }
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(true);
    if out.iter().any(|r| *r != out[0]) {
        return Err(format!("GC modes disagree: {out:?}"));
    }
    Ok(out.remove(0))
}

macro_rules! bun_tests {
    ($( $id:literal $fn:ident $name:literal $prefix:literal $base:literal; )*) => {$(
        #[doc = concat!("M9-TS-", stringify!($id))]
        #[test]
        fn $fn() {
            let (_, late) = run(case($name), $prefix, $base, false).unwrap_or_else(|e| panic!("{}: {e}", $name));
            assert_eq!(late, expected($name), "{}", $name);
        }
    )*};
}

bun_tests! {
    3157 top_level_await_orders_the_module_code "ordering_and_variables" "tl0_" 900000;
    3158 awaits_in_blocks_loops_and_try "awaits_in_blocks_and_loops" "tl1_" 901000;
    3159 classes_closures_and_functions_see_the_module_variables "classes_closures_and_a_later_function" "tl2_" 902000;
    3160 top_level_promise_all_of_different_types "promise_all_tuple_at_the_top" "tl3_" 903000;
}

// M9-TS-3161
//
// The code before the first `await` runs inside `ts_init`; the rest waits for the event loop.
#[test]
fn only_the_code_before_the_first_await_runs_in_init() {
    let (early, late) = run(case("ordering_and_variables"), "tl4_", 904_000, true).unwrap();
    assert_eq!(early, "ab");
    assert_eq!(late, "abc5d10");
}
