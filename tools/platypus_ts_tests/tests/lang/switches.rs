// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `switch` and enums compiled by `web_ui_ts` (`bench/switches.ts`). The expected numbers are what
//! bun computes from the same file. Every test runs three times: normally, with a collection at
//! every allocation, and with collection off. IDs are in TESTS.md (M9-TS-64..).
use platypus_ts_tests::sw::*;
use web_ui_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_sw_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-64
#[test]
fn a_switch_on_an_enum_picks_the_case_and_the_default() {
    in_every_gc_mode(|| unsafe {
        let got: Vec<f64> = [0.0, 1.0, 5.0, 6.0, 3.0]
            .iter()
            .map(|d| ts_sw_dirCode(*d))
            .collect();
        assert_eq!(got, [10.0, 20.0, 30.0, 30.0, -1.0]);
    });
}

// M9-TS-65
//
// JS: `enum Dir { Up, Down, Left = 5, Right }` numbers 0, 1, 5, 6; `const enum Flag { A = 1,
// B = A << 1, C = B << 1 }` is 1, 2, 4. Members are folded, so they cost nothing at run time.
#[test]
fn enum_members_are_numbered_and_can_refer_to_earlier_members() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_sw_dirValues(), 156.0);
        assert_eq!(ts_sw_flags(), 502.0);
        assert_eq!(ts_sw_turn(0.0), 6.0);
        assert_eq!(ts_sw_turn(1.0), 0.0);
        assert_eq!(ts_sw_dirOf(3.0), 5.0);
        assert_eq!(ts_sw_dirOf(1.0), 1.0);
    });
}

// M9-TS-66
//
// JS falls through from a matching case into the next, and `default` may sit between cases:
//
//     switch (n) { case 1: r += 1; case 2: r += 10; break; case 3: r += 100;
//                  default: r += 1000; case 4: r += 10000; }
//
// n = 3 runs case 3, default and case 4 (11100); n = 9 matches nothing, so the default runs and
// falls into case 4 (11000).
#[test]
fn a_switch_falls_through_and_the_default_may_be_in_the_middle() {
    in_every_gc_mode(|| unsafe {
        let got: Vec<f64> = [0.0, 1.0, 2.0, 3.0, 4.0, 9.0]
            .iter()
            .map(|n| ts_sw_fallThrough(*n))
            .collect();
        assert_eq!(got, [11000.0, 11.0, 10.0, 11100.0, 10000.0, 11000.0]);
    });
}

// M9-TS-67
#[test]
fn switching_on_a_string_union_and_a_string_enum_compares_tags() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_sw_kindDemo(), 1220.0);
        assert_eq!(ts_sw_moodDemo(), 2101.0);
    });
}

// M9-TS-68
//
// JS: `continue` in a switch continues the enclosing loop (it is not a switch construct), `break`
// leaves only the switch, a switch with no match and no default does nothing, and `break label`
// can leave a labelled switch from inside a loop.
#[test]
fn continue_in_a_switch_reaches_the_loop_and_break_only_the_switch() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_sw_loopWithSwitch(9.0), 633.0);
        assert_eq!(ts_sw_noMatch(1.0), 1.0);
        assert_eq!(ts_sw_noMatch(2.0), 7.0);
        assert_eq!(ts_sw_breakOutOfSwitchLabel(), 10.0);
    });
}

// M9-TS-69
//
// The discriminant is evaluated once and the case expressions in order until one matches, even
// when they allocate; blocks inside cases keep their own `const`s.
#[test]
fn a_switch_with_allocating_discriminant_and_cases_survives_collections() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_sw_allocatingSwitch(8.0), 9242.0);
        assert_eq!(ts_sw_sharedScope(1.0), 5.0);
        assert_eq!(ts_sw_sharedScope(2.0), 12.0);
        assert_eq!(ts_sw_sharedScope(3.0), 0.0);
    });
    ts_rt_gc_stress(true);
    let before = web_ui_rt::gc_stats().collections;
    unsafe { assert_eq!(ts_sw_allocatingSwitch(8.0), 9242.0) };
    ts_rt_gc_stress(false);
    assert!(
        web_ui_rt::gc_stats().collections > before,
        "stress mode collected nothing"
    );
}
