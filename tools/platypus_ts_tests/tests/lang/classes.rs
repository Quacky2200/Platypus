// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Plain classes compiled by `web_ui_ts` (`bench/classes.ts`). The expected numbers are what bun
//! computes from the same file. Every test runs three times: normally, with a collection at every
//! allocation, and with collection off. IDs are in TESTS.md (M9-TS-111..).
use platypus_ts_tests::cls::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The statics are module variables, so the tests take turns and `ts_init` runs before each mode.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert_ne!(link_anchor(), 0);
    unsafe { ts_cls_init() };
    f();
    ts_rt_gc_stress(true);
    unsafe { ts_cls_init() };
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    unsafe { ts_cls_init() };
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-111
//
// JS: `new Vec2(3, 4).add(Vec2.of(1, 2)).scale(2)` is (8, 12); `length` is a getter (5); a static
// method counts its calls; `static readonly zero` is an object built when the module loads.
#[test]
fn a_class_with_methods_a_getter_and_statics_computes_like_js() {
    in_every_gc_mode(|| unsafe { assert_eq!(ts_cls_demoVec(), 9251.0) });
}

// M9-TS-112
#[test]
fn constructor_parameters_take_their_defaults() {
    in_every_gc_mode(|| unsafe { assert_eq!(ts_cls_demoDefaults(), 270.0) });
}

// M9-TS-113
//
// JS: `#count` is private; `increment()` returns `this` for chaining; a getter/setter pair reads
// and writes it; `#bump` is a private method; an arrow function inside a method sees `this`; an
// optional field (`label?: number`) starts undefined.
#[test]
fn private_fields_private_methods_accessors_and_this_in_closures() {
    in_every_gc_mode(|| unsafe { assert_eq!(ts_cls_demoCounter(), 726364421.0) });
}

// M9-TS-114
#[test]
fn objects_linked_through_class_fields_survive_collections() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_cls_demoStack(5.0), 100504.0);
        assert_eq!(ts_cls_demoStack(0.0), -100.0);
    });
}

// M9-TS-115
//
// JS runs the parameter properties, then the field initialisers in source order (`b = this.a + 1`
// sees `a`), then the constructor body; a bare `return` in a constructor still gives the object.
#[test]
fn constructors_initialise_fields_in_order_and_may_return_early() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_cls_demoOrder(), 12012.0);
        assert_eq!(ts_cls_demoEarly(), 12.0);
    });
}

// M9-TS-116
//
// JS:
//
//     h.item.plus(swap(h))   // h.item is Box(5) beforehand
//
// evaluates the receiver `h.item` first. `swap` replaces `h.item` and allocates, so the old box is
// reachable only from the half-evaluated call. The method gets the old `this`: 5 + 1.
#[test]
fn a_receiver_waiting_for_its_arguments_is_not_freed() {
    in_every_gc_mode(|| unsafe { assert_eq!(ts_cls_receiverHeld(), 6.0) });
}

// M9-TS-117
#[test]
fn objects_have_reference_identity() {
    in_every_gc_mode(|| unsafe { assert_eq!(ts_cls_identity(), 910.0) });
}

// M9-TS-118
#[test]
fn methods_constructors_and_a_stack_in_a_loop_survive_collections() {
    in_every_gc_mode(|| unsafe { assert_eq!(ts_cls_allocating(30.0), 1320620.0) });
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        ts_cls_init();
        assert_eq!(ts_cls_allocating(30.0), 1320620.0);
    }
    ts_rt_gc_stress(false);
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
}

// M9-TS-137
//
// JS: `g.twice` taken as a value (it does not use `this`), `xs.map(this.twice)`, and an arrow
// function field that captures `this`. (Unlike JS, a detached method that does use `this` keeps
// its receiver here; JS would lose it, so no program that works in JS depends on the difference.)
#[test]
fn methods_as_values_and_arrow_function_fields_work() {
    in_every_gc_mode(|| unsafe { assert_eq!(ts_cls_demoBound(), 10673.0) });
}
