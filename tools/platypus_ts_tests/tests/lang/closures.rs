// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Closures compiled by `web_ui_ts` (`bench/closures.ts`). The expected numbers are what bun
//! computes from the same file. Every test runs three times: normally, with the collector run
//! at every allocation (a missing root crashes loudly), and with automatic collection off.
//! IDs are in TESTS.md (M9-TS-34..).
use platypus_ts_tests::clo;
use platypus_ts_tests::*;
use web_ui_rt::{Closure, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { clo::ts_clo_init() };
    f();
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    f();
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

/// Calls a `() => number` closure the way generated code does: `func(env)`.
fn call0(c: *mut Closure) -> f64 {
    unsafe {
        let f: extern "C" fn(*mut u8) -> f64 = std::mem::transmute((*c).func);
        f((*c).env)
    }
}

macro_rules! demo {
    ($test:ident, $id:literal, $f:ident, $want:expr, $doc:literal) => {
        // $id
        #[doc = $doc]
        #[test]
        fn $test() {
            in_every_gc_mode(|| assert_eq!(unsafe { clo::$f() }, $want));
        }
    };
}

demo!(
    a_closure_and_its_maker_share_one_variable,
    "M9-TS-34",
    ts_clo_counterDemo,
    33.0,
    "count is 3 in both"
);
demo!(
    closures_from_one_function_keep_separate_captured_values,
    "M9-TS-35",
    ts_clo_addersDemo,
    101011.0,
    "makeAdder(10) and makeAdder(100)"
);
demo!(
    two_closures_over_one_variable_see_each_others_writes,
    "M9-TS-36",
    ts_clo_counterObjectDemo,
    7101.0,
    "inc and get share n"
);
demo!(
    a_variable_declared_in_a_for_of_loop_is_new_every_iteration,
    "M9-TS-37",
    ts_clo_perIterationDemo,
    10409.0,
    "1, 4, 9"
);
demo!(
    a_const_declared_in_a_while_body_is_new_every_iteration,
    "M9-TS-38",
    ts_clo_whileDemo,
    211101.0,
    "1, 11, 21"
);
demo!(
    top_level_functions_work_as_values_and_callbacks,
    "M9-TS-39",
    ts_clo_functionValueDemo,
    42060608.0,
    "apply(double, 21), xs.map(double)"
);
demo!(
    a_closure_can_call_itself,
    "M9-TS-40",
    ts_clo_recursiveDemo,
    720.0,
    "fact(6)"
);
demo!(
    a_closure_over_an_object_changes_the_shared_object,
    "M9-TS-41",
    ts_clo_captureObjectDemo,
    34.0,
    "item.v"
);
demo!(
    a_closure_made_in_an_inlined_callback_captures_its_parameter,
    "M9-TS-42",
    ts_clo_nestedDemo,
    332211.0,
    "x => y => x + y"
);
demo!(
    an_immediately_called_closure_works,
    "M9-TS-43",
    ts_clo_iifeDemo,
    42.0,
    "(() => base + 2)()"
);
demo!(
    closures_can_be_passed_returned_and_called_through_a_call,
    "M9-TS-46",
    ts_clo_composeDemo,
    13.0,
    "compose(makeAdder(1), makeAdder(2))(10)"
);

// M9-TS-44
//
// The host holds closures that script made. They keep their captured variables alive across
// collections (the environment array and the cells are traced), and each has its own variables.
#[test]
fn closures_held_by_the_host_keep_their_captured_state_through_collections() {
    in_every_gc_mode(|| unsafe {
        let a = root(clo::ts_clo_makeAccumulator(10.0));
        let b = root(clo::ts_clo_makeAccumulator(100.0));
        let r = root(clo::ts_clo_makeItemReader(7.0));
        assert_eq!(call0(a), 11.0);
        assert_eq!(clo::ts_clo_churn(300.0), 44850.0);
        assert_eq!(call0(a), 12.0);
        assert_eq!(call0(b), 101.0);
        assert_eq!(clo::ts_clo_churn(300.0), 44850.0);
        web_ui_rt::ts_rt_gc_collect();
        assert_eq!(call0(r), 7.0, "a captured object survives");
        assert_eq!(call0(a), 13.0);
    });
}

// M9-TS-45
#[test]
fn churning_closures_and_cells_is_collected() {
    assert_ne!(link_anchor(), 0);
    unsafe {
        clo::ts_clo_init();
        let before = web_ui_rt::gc_stats();
        // 20,000 short-lived arrays, cells and closures.
        assert_eq!(clo::ts_clo_churn(20_000.0), 199_990_000.0);
        web_ui_rt::ts_rt_gc_collect();
        let after = web_ui_rt::gc_stats();
        assert!(after.collections > before.collections);
        assert!(after.objects < 1_000, "{after:?}");
    }
}
