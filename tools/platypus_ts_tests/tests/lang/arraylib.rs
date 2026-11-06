// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The array library compiled by `web_ui_ts` (`bench/arraylib.ts`). The expected numbers are what
//! bun computes from the same file. Every test runs three times: normally, with a collection at
//! every allocation, and with collection off. IDs are in TESTS.md (M9-TS-92..).
use platypus_ts_tests::al::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_al_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

macro_rules! demo {
    ($test:ident, $id:literal, $f:ident, $want:expr, $doc:literal) => {
        // $id
        #[doc = $doc]
        #[test]
        fn $test() {
            in_every_gc_mode(|| assert_eq!(unsafe { $f() }, $want));
        }
    };
}

demo!(
    pop_and_shift_return_the_element_or_null_on_an_empty_array,
    "M9-TS-92",
    ts_al_popAndShift,
    2234111.0,
    "pop 4, shift 1, then [2, 3] left; empty pop and shift are undefined"
);
demo!(
    unshift_puts_values_in_front_and_returns_the_new_length,
    "M9-TS-93",
    ts_al_unshifting,
    478564.0,
    "[7, 8, 5, 6] and 4"
);
demo!(
    slice_takes_a_copy_with_negative_indexes_counted_from_the_end,
    "M9-TS-94",
    ts_al_slicing,
    51234725862340.0,
    "slice(1, 3), slice(-2), slice(0, -1), slice(7), slice()"
);
demo!(
    splice_removes_inserts_and_replaces,
    "M9-TS-95",
    ts_al_splicing,
    315243524947453.0,
    "splice(1, 2), splice(1, 0, 9, 9), splice(0, 1, 7), splice(-2)"
);
demo!(
    concat_joins_arrays_and_single_values_into_a_new_array,
    "M9-TS-96",
    ts_al_concatenating,
    612348720.0,
    "[1, 2].concat([3], 4, [5, 6]) leaves the original alone"
);
demo!(
    reverse_works_in_place_and_returns_the_same_array,
    "M9-TS-97",
    ts_al_reversing,
    543210.0,
    "reverse() returns the array itself (a push through the result shows in the original)"
);
demo!(
    sort_with_a_comparator_orders_numbers_both_ways,
    "M9-TS-98",
    ts_al_sortingNumbers,
    61335796975331.0,
    "ascending then descending"
);
demo!(
    sort_is_stable_for_objects_and_the_comparator_may_allocate,
    "M9-TS-99",
    ts_al_sortingObjects,
    143025112333.0,
    "equal keys keep their order; the comparator allocates"
);
demo!(
    fill_and_new_array_fill_a_range,
    "M9-TS-102",
    ts_al_filling,
    522731903124.0,
    "new Array(5).fill(2), fill(9, 1, 3), fill(4, -1)"
);
demo!(
    index_of_last_index_of_find_index_and_includes,
    "M9-TS-103",
    ts_al_searching,
    140531.0,
    "positions found and not found"
);
demo!(
    at_counts_negative_indexes_from_the_end,
    "M9-TS-104",
    ts_al_atIndex,
    1103011.0,
    "at(0), at(-1), at(-3), at(5), empty at(-1)"
);
demo!(
    reduce_folds_numbers_and_objects_with_index_and_initial_value,
    "M9-TS-105",
    ts_al_reducing,
    105202.0,
    "sum, max, indexed sum, count"
);
demo!(
    assigning_length_truncates,
    "M9-TS-106",
    ts_al_lengthAssign,
    312318.0,
    "length = 3, then length = 0 and a push"
);
demo!(
    array_from_of_and_is_array,
    "M9-TS-107",
    ts_al_statics,
    50150613789005.0,
    "Array.from({ length }, fn), Array.from(xs) is a copy, Array.of, Array.isArray"
);

// M9-TS-100
//
// 200 pseudo-random items sorted by a comparator that allocates nothing: in order, and equal
// keys keep their original order (stability), and the same under a collection at every
// allocation.
#[test]
fn sorting_200_items_gives_a_stable_order() {
    in_every_gc_mode(|| assert_eq!(unsafe { ts_al_sortingMany(200.0) }, 968.0));
}

// M9-TS-101
//
// The collector runs while the comparator allocates; the array, the comparator closure and the
// merge buffer are held by the sort, so no element is lost.
#[test]
fn sorting_with_collections_during_the_comparator_loses_nothing() {
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    assert_ne!(link_anchor(), 0);
    unsafe {
        ts_al_init();
        assert_eq!(ts_al_sortingObjects(), 143025112333.0);
        assert_eq!(ts_al_sortingMany(60.0), ts_al_sortingMany(60.0));
        assert_eq!(ts_al_sortingWithCapture(), 11320.0);
    }
    ts_rt_gc_stress(false);
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
}

// M9-TS-108
#[test]
fn array_operations_on_objects_in_a_loop_survive_collections() {
    in_every_gc_mode(|| assert_eq!(unsafe { ts_al_allocating(20.0) }, 726.0));
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    unsafe { assert_eq!(ts_al_allocating(20.0), 726.0) };
    ts_rt_gc_stress(false);
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
}
