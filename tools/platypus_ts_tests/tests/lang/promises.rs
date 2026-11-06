// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/promises.ts` against bun (`bench/gen_promises_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-408..).
use platypus_ts_tests::promises_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress, ts_rt_run_loop};

// The program's globals are shared, so the tests take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert_ne!(link_anchor(), 0);
    unsafe { ts_pr_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/promises_expected.json")).unwrap()
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

// M9-TS-408
#[test]
fn basic_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_basic();
        ts_rt_run_loop();
        check_str("basic_result", ts_pr_basic_result());
    });
}

// M9-TS-409
#[test]
fn await_value_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_await_value();
        ts_rt_run_loop();
        check_str("await_value_result", ts_pr_await_value_result());
    });
}

// M9-TS-410
#[test]
fn order_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_order();
        ts_rt_run_loop();
        check_str("order_result", ts_pr_order_result());
    });
}

// M9-TS-411
#[test]
fn throw_caught_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_throw_caught();
        ts_rt_run_loop();
        check_str("throw_caught_result", ts_pr_throw_caught_result());
    });
}

// M9-TS-412
#[test]
fn catch_method_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_catch_method();
        ts_rt_run_loop();
        check_str("catch_method_result", ts_pr_catch_method_result());
    });
}

// M9-TS-413
#[test]
fn then_types_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_then_types();
        ts_rt_run_loop();
        check_str("then_types_result", ts_pr_then_types_result());
    });
}

// M9-TS-414
#[test]
fn then_flatten_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_then_flatten();
        ts_rt_run_loop();
        check_str("then_flatten_result", ts_pr_then_flatten_result());
    });
}

// M9-TS-415
#[test]
fn finally_order_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_finally_order();
        ts_rt_run_loop();
        check_str("finally_order_result", ts_pr_finally_order_result());
    });
}

// M9-TS-416
#[test]
fn all_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_all();
        ts_rt_run_loop();
        check_str("all_result", ts_pr_all_result());
    });
}

// M9-TS-417
#[test]
fn all_reject_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_all_reject();
        ts_rt_run_loop();
        check_str("all_reject_result", ts_pr_all_reject_result());
    });
}

// M9-TS-418
#[test]
fn race_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_race();
        ts_rt_run_loop();
        check_str("race_result", ts_pr_race_result());
    });
}

// M9-TS-419
#[test]
fn timer_order_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_timer_order();
        ts_rt_run_loop();
        check_str("timer_order_result", ts_pr_timer_order_result());
    });
}

// M9-TS-420
#[test]
fn interval_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_interval();
        ts_rt_run_loop();
        check_str("interval_result", ts_pr_interval_result());
    });
}

// M9-TS-421
#[test]
fn clear_timeout_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_clear_timeout();
        ts_rt_run_loop();
        check_str("clear_timeout_result", ts_pr_clear_timeout_result());
    });
}

// M9-TS-422
#[test]
fn methods_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_methods();
        ts_rt_run_loop();
        check_str("methods_result", ts_pr_methods_result());
    });
}

// M9-TS-423
#[test]
fn executor_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_executor();
        ts_rt_run_loop();
        check_str("executor_result", ts_pr_executor_result());
    });
}

// M9-TS-424
#[test]
fn return_promise_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_return_promise();
        ts_rt_run_loop();
        check_str("return_promise_result", ts_pr_return_promise_result());
    });
}

// M9-TS-425
#[test]
fn many_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_many();
        ts_rt_run_loop();
        check_str("many_result", ts_pr_many_result());
    });
}

// M9-TS-426
#[test]
fn sequence_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_sequence();
        ts_rt_run_loop();
        check_str("sequence_result", ts_pr_sequence_result());
    });
}

// M9-TS-427
#[test]
fn then_two_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_then_two();
        ts_rt_run_loop();
        check_str("then_two_result", ts_pr_then_two_result());
    });
}

// M9-TS-428
#[test]
fn any_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_any();
        ts_rt_run_loop();
        check_str("any_result", ts_pr_any_result());
    });
}

// M9-TS-429
#[test]
fn all_settled_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        ts_pr_all_settled();
        ts_rt_run_loop();
        check_str("all_settled_result", ts_pr_all_settled_result());
    });
}
