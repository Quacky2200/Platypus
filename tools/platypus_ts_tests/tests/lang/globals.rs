// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Module-level `let` variables compiled by `web_ui_ts` (`bench/globals.ts`). The expected numbers
//! are what bun computes from the same file, one fresh process per scenario. Every test runs three
//! times (normally, with a collection at every allocation, with collection off), and calls
//! `ts_init` first so each run starts from the initialisers. IDs are in TESTS.md (M9-TS-47..).
use platypus_ts_tests::glob::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The module's variables are process-wide, so the tests take turns.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert_ne!(link_anchor(), 0);
    unsafe { ts_glob_init() };
    f();
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    unsafe { ts_glob_init() };
    f();
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    unsafe { ts_glob_init() };
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-47
//
// JS: `let counter = 0` is changed by functions (`counter += 1`, `counter++`), by an arrow
// function that captures nothing but the module, and by a `forEach` callback; a second call
// continues from where the first stopped, and running the module again starts over.
#[test]
fn a_counter_is_changed_by_functions_closures_and_callbacks() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_glob_counterDemo(), 19.0);
        assert_eq!(ts_glob_counterDemo(), 38.0);
        assert_eq!(ts_glob_counterValue(), 38.0);
        ts_glob_init();
        assert_eq!(ts_glob_counterValue(), 0.0);
        assert_eq!(ts_glob_counterDemo(), 19.0);
    });
}

// M9-TS-48
//
// JS:
//
//     sum(current, swapCurrent(100))   // current = {v: 1} beforehand
//
// evaluates `current` first (the old item). `swapCurrent` replaces the global and allocates
// again, so the old item is reachable only from the half-evaluated argument list. JS keeps it
// alive and the result is 1 + 100; the global ends up holding the new item (v = 100).
#[test]
fn a_global_replaced_by_a_call_while_its_old_value_waits_as_an_argument_survives() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_glob_globalReplacedByACall(), 101.0);
        assert_eq!(ts_glob_currentValue(), 100.0);
    });
}

// M9-TS-49
//
// JS: `sum(current, ((current = { v: 50 }), { v: 7 }.v))`. The assignment sits in the argument
// itself and the next allocation is an object literal, with no call in between: the old item
// (v = 1) is still the first argument. 1 + 7 = 8, and `current` is the item with v = 50.
#[test]
fn a_global_assigned_inside_a_later_argument_keeps_the_old_value_alive() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_glob_globalReplacedInTheArgument(), 8.0);
        assert_eq!(ts_glob_currentValue(), 50.0);
    });
}

// M9-TS-50
#[test]
fn a_global_that_may_be_null_holds_an_object_or_nothing() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_glob_lastOr(-9.0), -9.0);
        ts_glob_remember(4.0);
        assert_eq!(ts_glob_lastOr(-9.0), 4.0);
        ts_glob_forget();
        assert_eq!(ts_glob_lastOr(-9.0), -9.0);
    });
}

// M9-TS-51
#[test]
fn a_global_number_or_null_keeps_the_largest_value_seen() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_glob_bestOr(-1.0), -1.0);
        for x in [3.0, 9.0, 5.0] {
            ts_glob_consider(x);
        }
        assert_eq!(ts_glob_bestOr(-1.0), 9.0);
    });
}

// M9-TS-52
//
// JS: `let pending: Item | undefined;` starts as undefined (here null); the first call stores an
// item and returns -1, the second reads it back.
#[test]
fn a_global_declared_without_a_value_starts_as_null() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_glob_pendingOr(8.0), -1.0);
        assert_eq!(ts_glob_pendingOr(0.0), 8.0);
        assert_eq!(ts_glob_unsetPlusOne(), 1.0);
        assert_eq!(ts_glob_unsetPlusOne(), 2.0);
    });
}

// M9-TS-53
//
// JS: `handler = (n) => n * k + base` stores a closure over `k` in a module variable; calling it
// later still sees `k`. 4 * 3 + 5 = 17.
#[test]
fn a_closure_stored_in_a_global_keeps_what_it_captured() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_glob_callHandler(4.0), 4.0);
        ts_glob_setHandler(3.0);
        assert_eq!(ts_glob_callHandler(4.0), 17.0);
    });
}

// M9-TS-54
//
// JS: a linked list whose only owner is the module variable `chain` is grown 100 nodes, then
// 10 more. Natively every node must be found through the global root: 0+..+99 = 4950, then
// the ten new values add 0+..+9 = 45.
#[test]
fn a_list_reachable_only_from_a_global_survives_collections() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_glob_growChain(100.0), 4950.0);
        assert_eq!(ts_glob_growChain(10.0), 4995.0);
    });
}
