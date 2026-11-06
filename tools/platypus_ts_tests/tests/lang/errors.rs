// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Exceptions in compiled code (`bench/errors.ts`): `throw`, `try` / `catch`, error classes,
//! unwinding through calls, closures and loops. `bench/gen_errors_expected.ts` ran the same file in
//! bun; every function here must return what bun returned, in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-187..).
use platypus_ts_tests::errs::*;
use serde_json::Value;
use web_ui_rt::{
    Str, link_anchor, ts_rt_exc_pending, ts_rt_exc_take, ts_rt_gc_enable, ts_rt_gc_stress,
};

/// The module's globals are shared by every thread, so one test runs at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert_ne!(link_anchor(), 0);
    unsafe { ts_err_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/errors_expected.json")).unwrap()
}

fn check_num(name: &str, got: f64) {
    assert!(!ts_rt_exc_pending(), "{name} left an exception pending");
    assert_eq!(got, expected()[name].as_f64().unwrap(), "{name}");
}

fn check_str(name: &str, got: *mut u8) {
    assert!(!ts_rt_exc_pending(), "{name} left an exception pending");
    let want: Vec<u16> = expected()[name]["s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u16)
        .collect();
    let got = unsafe { Str::from_raw(got as *const Str) }.to_utf16();
    assert_eq!(
        String::from_utf16_lossy(&got),
        String::from_utf16_lossy(&want),
        "{name}"
    );
}

// M9-TS-187
//
// `throw` in a callee and `catch` in the caller: the message, the name, `toString`, a subclass
// with its own field, and `instanceof` to tell the classes apart.
#[test]
fn thrown_errors_are_caught_by_the_caller() {
    in_every_gc_mode(|| unsafe {
        check_str("basic", ts_err_basic());
        check_str("kinds", ts_err_kinds());
        check_num("clean", ts_err_clean());
    });
}

// M9-TS-188
//
// An exception unwinds through six frames (each holding strings), is rethrown wrapped, and
// leaves an inlined array callback and a closure.
#[test]
fn exceptions_unwind_through_calls_closures_and_rethrows() {
    in_every_gc_mode(|| unsafe {
        check_str("unwinding", ts_err_unwinding());
        check_str("rethrow", ts_err_rethrow());
        check_str("inCallback", ts_err_inCallback());
    });
}

// M9-TS-189
//
// `continue` and `break` inside `catch`, and two hundred throws and catches with a collection
// at every allocation.
#[test]
fn control_flow_in_catch_and_many_exceptions_match_javascript() {
    in_every_gc_mode(|| unsafe {
        check_num("loops", ts_err_loops());
        check_num("many", ts_err_many());
    });
}

// M9-TS-190
//
// An exception nothing catches leaves the exported function with the exception pending, which
// the host takes; it is the error object of the program (here `ValidationError`).
#[test]
fn an_uncaught_exception_is_left_pending_for_the_host() {
    in_every_gc_mode(|| unsafe {
        assert!(!ts_rt_exc_pending());
        ts_err_uncaught();
        assert!(ts_rt_exc_pending());
        let e = ts_rt_exc_take() as *const ValidationError;
        assert!(!ts_rt_exc_pending());
        let text = |p: *mut u8| Str::from_raw(p as *const Str).to_string_lossy();
        assert_eq!(text((*e).message), "negative: -1");
        assert_eq!(text((*e).name), "ValidationError");
        assert_eq!(text((*e).field), "n");
        assert_eq!(
            expected()["uncaught"]["threw"].as_str().unwrap(),
            "ValidationError: negative: -1"
        );
    });
}

// M9-TS-193
//
// `finally` runs on a normal exit, after `catch`, and on `return` (once per call, in the order of
// the nested blocks).
#[test]
fn finally_runs_once_on_every_way_out_of_a_try() {
    in_every_gc_mode(|| unsafe {
        check_str("finallyOrder", ts_err_finallyOrder());
        check_str("nestedFinally", ts_err_nestedFinally());
        check_str("finallyWhileThrowing", ts_err_finallyWhileThrowing());
    });
}

// M9-TS-194
//
// `break`, `continue` and `continue label` leave a `try` through its `finally`; a `return` in a
// `finally` replaces the one in the `try`.
#[test]
fn break_continue_and_return_pass_through_finally() {
    in_every_gc_mode(|| unsafe {
        check_str("finallyLoops", ts_err_finallyLoops());
        check_str("labelled", ts_err_labelled());
        check_num("finallyOverrides", ts_err_finallyOverrides());
    });
}

// M9-TS-195
//
// The runtime's own errors (`repeat(-1)`, `toFixed(101)`, `toString(37)`, `fromCodePoint(-1)`) are
// `RangeError`s with bun's messages, catchable like any other, and the calls that succeed still do.
#[test]
fn runtime_errors_are_catchable_range_errors() {
    in_every_gc_mode(|| unsafe {
        check_str("runtimeErrors", ts_err_runtimeErrors());
    });
}

// M9-TS-196
//
// A runtime error nothing catches reaches the host as a pending `RangeError`.
#[test]
fn an_uncaught_runtime_error_is_a_pending_range_error() {
    in_every_gc_mode(|| unsafe {
        ts_err_runtimeErrorEscapes();
        assert!(ts_rt_exc_pending());
        let e = ts_rt_exc_take() as *const RangeError;
        let text = |p: *mut u8| Str::from_raw(p as *const Str).to_string_lossy();
        assert_eq!(text((*e).name), "RangeError");
        let want = expected()["runtimeErrorEscapes"]["threw"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(format!("RangeError: {}", text((*e).message)), want);
    });
}

// M9-TS-198
//
// A callback inlined into a `try ... finally` has its own loops: a `break` inside it stays inside.
#[test]
fn a_break_in_an_inlined_callback_is_not_a_jump_through_the_finally_around_the_call() {
    in_every_gc_mode(|| unsafe {
        check_str("callbackLoopInFinally", ts_err_callbackLoopInFinally());
    });
}
