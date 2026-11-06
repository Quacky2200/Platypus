// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Inheritance compiled by `web_ui_ts` (`bench/inherit.ts`). The expected numbers are what bun
//! computes from the same file. Every test runs three times: normally, with a collection at every
//! allocation, and with collection off. IDs are in TESTS.md (M9-TS-120..).
use platypus_ts_tests::inh::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The statics are module variables, so the tests take turns and `ts_init` runs before each mode.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert_ne!(link_anchor(), 0);
    unsafe { ts_inh_init() };
    f();
    ts_rt_gc_stress(true);
    unsafe { ts_inh_init() };
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    unsafe { ts_inh_init() };
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
    a_call_on_a_base_class_variable_runs_the_subclasss_method,
    "M9-TS-120",
    ts_inh_demoVirtual,
    6171206.0,
    "areas 6, 17, 12, 6 of a Rect, a Square, a Circle and a Tri held as Shape"
);
demo!(
    this_dispatches_on_the_real_class_inside_base_methods,
    "M9-TS-121",
    ts_inh_demoThisDispatch,
    11041.0,
    "describe() in Shape calls this.area()"
);
demo!(
    getters_can_be_overridden_and_are_dispatched_too,
    "M9-TS-122",
    ts_inh_demoAccessors,
    1200.0,
    "kind: 1, 2, 0, 0 for Rect, Square, Circle, Tri"
);
demo!(
    super_calls_the_base_implementation_and_a_base_typed_variable_still_gets_the_override,
    "M9-TS-123",
    ts_inh_demoSuper,
    201010.0,
    "Square.area = super.area() + 1"
);
demo!(
    instanceof_is_true_for_the_class_and_its_subclasses_and_false_for_null,
    "M9-TS-124",
    ts_inh_demoInstanceof,
    2114.0,
    "2 rects (Rect, Square), 1 square, 1 circle, 4 shapes, null counts for none"
);
demo!(
    a_member_only_a_subclass_has_is_usable_after_narrowing_or_as,
    "M9-TS-125",
    ts_inh_demoDowncast,
    506023.0,
    "sh.onlyOnSquare() after instanceof, (x as Rect).w"
);
demo!(
    subclass_objects_flow_through_variables_arrays_and_conditionals_as_their_base,
    "M9-TS-126",
    ts_inh_demoSubclassAsBase,
    233.0,
    "pick(flag): Shape, an array concat of Square, Tri and Circle"
);
demo!(
    a_subclass_constructor_assigns_after_super_returns,
    "M9-TS-127",
    ts_inh_demoTags,
    221.0,
    "tag is 1 in the base initialiser, 2 after Rect's super(4)"
);
demo!(
    a_field_a_subclass_declares_again_is_the_same_slot_and_its_initialiser_wins,
    "M9-TS-128",
    ts_inh_demoFieldRedeclared,
    5200.0,
    "A has x = 1, B extends A with x = 5"
);
demo!(
    a_base_constructor_calling_an_overridden_method_reaches_the_subclass,
    "M9-TS-129",
    ts_inh_demoConstructorDispatch,
    1020.0,
    "Base.v = this.compute(): 10 for Base, 20 for Derived"
);
demo!(
    a_derived_class_without_a_constructor_forwards_its_parameters_then_runs_its_initialisers,
    "M9-TS-130",
    ts_inh_demoImplicitConstructor,
    531.0,
    "new Tri(5): sides 5, corner 3, tag 1"
);
demo!(
    an_override_calls_super_through_a_recursive_structure,
    "M9-TS-131",
    ts_inh_demoRecursionThroughBase,
    2006.0,
    "Node3.total = 1000 + super.total()"
);
demo!(
    a_static_counter_in_the_base_class_counts_every_subclass_construction,
    "M9-TS-132",
    ts_inh_demoCreatedCount,
    11.0,
    "three constructions plus sides 4 + 4 + 0"
);

demo!(
    a_virtual_call_receiver_waiting_for_its_arguments_is_not_freed,
    "M9-TS-135",
    ts_inh_demoVirtualReceiverHeld,
    3.0,
    "h.s.scaled(swapShape(h)): the old Circle is the receiver"
);

demo!(
    a_method_taken_as_a_value_still_dispatches_on_the_real_class,
    "M9-TS-138",
    ts_inh_demoBoundVirtual,
    21.0,
    "a.sound as a value, a: Animal holding a Dog"
);

// M9-TS-133
#[test]
fn virtual_calls_instanceof_and_subclass_arrays_survive_collections() {
    in_every_gc_mode(|| assert_eq!(unsafe { ts_inh_allocating(40.0) }, 196499.0));
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    unsafe {
        ts_inh_init();
        assert_eq!(ts_inh_allocating(40.0), 196499.0);
    }
    ts_rt_gc_stress(false);
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
}
