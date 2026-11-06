// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Compiled scripts calling the engine API directly (`bench/engine.ts`). The API is
//! `platypus_demo_api`, written with `#[ts_api]`: the compiler reads its JSON manifest, a call
//! `Demo.scale(3, 4)` becomes a call of the shim `platypus_Demo_scale`, and handles are plain ids.
//! Every test runs normally, with a collection at every allocation, and with collection off.
//! IDs are in TESTS.md (M9-TS-142..).
use platypus_ts_tests::eng::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The demo engine's world is thread-local state; each test thread has its own.
fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_demo_api::link_anchor(), 0);
    unsafe { ts_eng_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-142
//
// `Demo.scale(3, 4)` is `platypus_Demo_scale(3.0, 4.0)`; `clampI` takes i32s (the f64 arguments are
// converted at the call), `isEven` takes a u32 and returns a bool.
#[test]
fn a_script_calls_engine_functions_with_the_engines_number_types() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_eng_arithmetic(), 113000.0);
        // 3.9 -> 3 and -5.7 -> -5: truncation toward zero, like `as i32`.
        assert_eq!(ts_eng_truncates(), 25.0);
    });
}

// M9-TS-143
//
// The engine computes in f32: 0.1 narrows on the way in, the result widens on the way out.
#[test]
fn f32_arguments_and_results_are_narrowed_and_widened_at_the_call() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_eng_halfOfTenth(), (0.1f32 * 0.5f32) as f64);
        assert_ne!(ts_eng_halfOfTenth(), 0.05);
    });
}

// M9-TS-144
//
// Handles are ids: `a === b` compares them, a destroyed entity's id (generation) stops working
// even when its slot is reused, and an engine function can say "no such entity" (hp -1).
#[test]
fn handles_are_plain_ids_and_a_stale_one_misses() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_eng_entities(), 2_709_000.0);
        assert_eq!(ts_eng_liveHp(), 55.5);
        assert_eq!(ts_eng_staleHp(), -1.0);
    });
}

// M9-TS-145
//
// A handle is not a pointer: it sits in objects, arrays, nullable variables and closure cells
// without the collector tracing it, between allocations that collect at every step.
#[test]
fn handles_live_in_objects_arrays_nullable_variables_and_closures() {
    in_every_gc_mode(|| unsafe {
        // sum(kind) + sum(label) = 190 + 380, plus kind of ids[3] * 1000.
        assert_eq!(ts_eng_dataStructures(), 3570.0);
        assert_eq!(ts_eng_nullableHandle(true), 5.0);
        assert_eq!(ts_eng_nullableHandle(false), -1.0);
        assert_eq!(ts_eng_closureOverHandle(), 42.0);
    });
}

// M9-TS-147
//
// A value class is an object in the script (`new Vector3(3, 0, 4)`, `b.x`). Passed to the engine it
// becomes one scalar per field; a result comes back through a pointer into a new object.
#[test]
fn a_value_class_is_passed_as_scalars_and_comes_back_as_a_new_object() {
    in_every_gc_mode(|| unsafe {
        // length 5, scaled (6, 0, 8), cross z 1.
        assert_eq!(ts_eng_vectors(), 5.0 * 1000.0 + 600.0 + 8.0 + 10000.0);
        // 0.1 narrowed to f32 on the way in and back out.
        assert_eq!(ts_eng_vectorRounding(), 0.1f32 as f64);
        assert_ne!(ts_eng_vectorRounding(), 0.1);
    });
}

// M9-TS-148
#[test]
fn a_value_stored_in_the_engine_next_to_a_handle_reads_back() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_eng_positions(), 150.0 + 25.0 + 3.5);
    });
}

// M9-TS-149
//
// Dozens of short-lived vectors, each a new object, with a collection at every allocation: the
// destination object of a result is rooted while the engine writes into it.
#[test]
fn many_short_lived_vectors_survive_collections() {
    in_every_gc_mode(|| unsafe {
        let mut want = 0.0f64;
        let mut v = [1.0f32, 0.0, 0.0];
        for _ in 0..40 {
            v = [v[0] * 1.5, v[1] * 1.5, v[2] * 1.5];
            // cross(v, (0, 1, 0)) = (-v.z, 0, v.x); its length in f32.
            let w = [-v[2], 0.0f32, v[0]];
            want += (w[0] * w[0] + w[1] * w[1] + w[2] * w[2]).sqrt() as f64;
        }
        assert_eq!(ts_eng_manyVectors(40.0), want);
    });
}
