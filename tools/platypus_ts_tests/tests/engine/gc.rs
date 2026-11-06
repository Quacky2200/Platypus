// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The collector under the zombie sim: it must not change what the game computes, and it must
//! keep memory bounded. IDs are in TESTS.md (M9-TS-23..25).
use platypus_ts_tests::sim::*;
use platypus_ts_tests::*;
use web_ui_rt::{gc_stats, link_anchor, ts_rt_gc_collect, ts_rt_gc_stress};

fn run_busy(steps: usize) -> f64 {
    assert_ne!(link_anchor(), 0);
    let b = busy_state();
    for _ in 0..steps {
        unsafe { ts_step(b.state, b.input, 1.0 / 60.0, b.walls) };
    }
    busy_checksum(b.state)
}

// M9-TS-23
#[test]
fn collecting_at_every_allocation_changes_nothing_the_game_computes() {
    let _serial = platypus_ts_tests::serial();
    let normal = run_busy(600);
    ts_rt_gc_stress(true);
    let before = gc_stats().collections;
    let stressed = run_busy(600);
    ts_rt_gc_stress(false);
    assert!(
        gc_stats().collections - before > 1_000,
        "stress mode collects on every allocation: {} collections",
        gc_stats().collections - before
    );
    assert_eq!(normal, stressed);
}

// M9-TS-24
#[test]
fn a_long_run_collects_on_its_own_and_memory_stays_bounded() {
    let _serial = platypus_ts_tests::serial();
    let b = {
        assert_ne!(link_anchor(), 0);
        busy_state()
    };
    let start = gc_stats();
    for _ in 0..8_000 {
        unsafe { ts_step(b.state, b.input, 1.0 / 60.0, b.walls) };
    }
    let end = gc_stats();
    assert!(end.collections - start.collections >= 10, "{end:?}");
    // What the run handed out in all is far more than what is alive.
    assert!(
        end.allocated_bytes - start.allocated_bytes > 8 << 20,
        "{end:?}"
    );
    ts_rt_gc_collect();
    let live = gc_stats();
    assert!(
        live.live_bytes < (1 << 20) && live.objects < 5_000,
        "{live:?}"
    );
}

// M9-TS-25
#[test]
fn what_the_host_roots_survives_and_the_rest_is_freed() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let b = busy_state();
    for _ in 0..300 {
        unsafe { ts_step(b.state, b.input, 1.0 / 60.0, b.walls) };
    }
    ts_rt_gc_stress(true);
    ts_rt_gc_collect();
    // The state is rooted, so its players, zombies and bullets are still readable (not 0xDD poison).
    unsafe {
        let st = &*b.state;
        let p = &*(*st.players).get::<*mut Player>(0);
        assert!(p.score >= 0.0 && p.health > 1e8);
        assert!(arr_len(st.bullets) < 1_000);
    }
    ts_rt_gc_stress(false);
}

// M9-TS-27
//
// In JavaScript:
//
//     sum(h.item, replaceItem(h, 100))   // h.item = {v: 5} beforehand
//
// evaluates `h.item` (the old item) first. `replaceItem` then overwrites `h.item` and allocates
// again, so the old item is reachable only from the half-evaluated argument list. JS keeps it
// alive and the result is 5 + 100. The native code used to treat a field read of a rooted object
// as "still reachable through its parent" and not root it: the collector freed the old item
// and `sum` read poisoned memory.
#[test]
fn a_field_value_held_as_an_argument_survives_the_object_being_overwritten() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    ts_rt_gc_stress(true);
    unsafe {
        gap::ts_gap_init();
        let h = root(gap::ts_gap_makeHolder(5.0));
        assert_eq!(gap::ts_gap_fieldHeldAcrossStore(h), 105.0);
    }
    ts_rt_gc_stress(false);
}

// M9-TS-28
//
// In JavaScript:
//
//     let a = fresh(5);
//     sum(a, (a = fresh(100), fresh(1).v))
//
// passes the *old* `a` (v = 5) to `sum`: arguments are evaluated left to right and `a` was read
// before it was reassigned. Only the argument list holds that old item once `a` points to the new
// one, and `fresh(1)` allocates after the reassignment. Result: 5 + 1.
#[test]
fn a_local_reassigned_while_its_old_value_waits_as_an_argument_survives() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    ts_rt_gc_stress(true);
    unsafe {
        gap::ts_gap_init();
        assert_eq!(gap::ts_gap_localReassigned(), 6.0);
    }
    ts_rt_gc_stress(false);
}

// M9-TS-29
//
// In JavaScript:
//
//     h.item === swapAndAlloc(h)   // false: two different objects
//
// The left side is evaluated first and then waits for the right side, which overwrites
// `h.item` and allocates. With the old item unrooted the collector frees it and the allocator
// hands the same block to the new object, so the addresses compare equal. Comparing pointers
// needs the same rooting as passing them.
#[test]
fn a_pointer_waiting_to_be_compared_is_not_freed_and_reused() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    ts_rt_gc_stress(true);
    unsafe {
        gap::ts_gap_init();
        let h = root(gap::ts_gap_makeHolder(5.0));
        assert!(!gap::ts_gap_sameAsOld(h));
    }
    ts_rt_gc_stress(false);
}

// M9-TS-30
//
// Two compiled modules live in one program and one heap (a game's scripts are several files).
// Their struct ids must not collide: each module gets its own range, otherwise the second
// module's `ts_init` overwrites the first's table of which fields are pointers and the collector
// traces the wrong slots (or frees what it cannot see).
#[test]
fn two_modules_in_one_heap_keep_their_own_pointer_maps() {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    ts_rt_gc_stress(true);
    unsafe {
        ts_init();
        gap::ts_gap_init();
        let b = busy_state();
        let h = root(gap::ts_gap_makeHolder(5.0));
        for _ in 0..200 {
            ts_step(b.state, b.input, 1.0 / 60.0, b.walls);
            assert_eq!(gap::ts_gap_fieldHeldAcrossStore(h), 105.0);
            (*h).item = (*gap::ts_gap_makeHolder(5.0)).item;
        }
        assert!(busy_checksum(b.state) > 0.0);
    }
    ts_rt_gc_stress(false);
}
