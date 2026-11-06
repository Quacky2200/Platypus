// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `for(;;)`, `do...while` and labelled jumps compiled by `web_ui_ts` (`bench/loops.ts`). The
//! expected numbers are what bun computes from the same file. Every test runs three times:
//! normally, with a collection at every allocation, and with collection off.
//! IDs are in TESTS.md (M9-TS-57..).
use platypus_ts_tests::lp::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_loop_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-57
#[test]
fn a_for_loop_counts_and_runs_its_update_after_continue() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_loop_sumTo(10.0), 45.0);
        assert_eq!(ts_loop_sumTo(0.0), 0.0);
        assert_eq!(ts_loop_sumOdd(10.0), 25.0);
    });
}

// M9-TS-58
#[test]
fn a_for_loop_may_have_several_declarators_or_leave_any_clause_out() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_loop_converge(), 30.0);
        assert_eq!(ts_loop_firstSquareOver(50.0), 8.0);
        assert_eq!(ts_loop_countDown(10.0), 398.0);
        assert_eq!(ts_loop_countDown(0.0), 0.0);
    });
}

// M9-TS-59
#[test]
fn a_do_while_body_runs_once_and_continue_reaches_its_test() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_loop_atLeastOnce(), 111.0);
        assert_eq!(ts_loop_doContinue(), 1206.0);
    });
}

// M9-TS-60
#[test]
fn labelled_continue_and_break_reach_the_outer_loop() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_loop_labelledContinue(), 15.0);
        assert_eq!(ts_loop_labelledBreak(), 26.0);
    });
}

// M9-TS-61
#[test]
fn labels_work_on_while_for_of_and_plain_blocks() {
    in_every_gc_mode(|| unsafe {
        let xs = |v: &[f64]| {
            let a = web_ui_rt::ts_rt_arr_new(0, 8);
            platypus_ts_tests::push_f64s(a, v);
            a
        };
        assert_eq!(
            ts_loop_labelledWhileAndForOf(xs(&[1.0, 2.0, 3.0])),
            600100.0
        );
        assert_eq!(
            ts_loop_labelledWhileAndForOf(xs(&[1.0, 0.0, 5.0])),
            100100.0
        );
        assert_eq!(ts_loop_labelledWhileAndForOf(xs(&[2.0, -1.0, 9.0])), 2001.0);
        assert_eq!(ts_loop_labelledBlock(9.0), 2.0);
        assert_eq!(ts_loop_labelledBlock(1.0), 3.0);
    });
}

// M9-TS-62
//
// JS gives a `let` in a `for` head a new binding for each iteration, so closures made in the body
// see 0, 1 and 2 (a `var` would give 3, 3, 3). A closure that changes the variable changes the
// copy the next iteration starts from.
#[test]
fn a_captured_for_variable_is_new_every_iteration() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_loop_closuresSeeEachIteration(), 12.0);
        assert_eq!(ts_loop_closureChangesTheLoopVariable(), 3.0);
    });
}

// M9-TS-63
#[test]
fn allocating_in_every_part_of_a_for_head_survives_collections() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_loop_allocatingLoop(100.0), 5050.0);
    });
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    unsafe { assert_eq!(ts_loop_allocatingLoop(100.0), 5050.0) };
    ts_rt_gc_stress(false);
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
}
