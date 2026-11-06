// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The Math library surface and the operators around it compiled by `web_ui_ts`
//! (`bench/mathlib.ts`). The expected values are what bun computes from the same file, compared
//! bit for bit (the sign of zero counts; NaN equals NaN). IDs are in TESTS.md (M9-TS-85..).
use platypus_ts_tests::ml::*;
use web_ui_rt::{Array, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress, ts_rt_random_seed};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_ml_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn same(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a.to_bits() == b.to_bits()
}

fn check(got: *mut Array, want: &[f64]) {
    let got: Vec<f64> = unsafe { (0..(*got).len()).map(|i| (*got).get::<f64>(i)).collect() };
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        assert!(
            same(*g, *w),
            "element {i}: got {g:?}, want {w:?} (all: {got:?})"
        );
    }
}

const NAN: f64 = f64::NAN;
const INF: f64 = f64::INFINITY;

// M9-TS-85
#[test]
fn ceil_and_trunc_keep_the_sign_of_zero() {
    in_every_gc_mode(|| {
        check(
            unsafe { ts_ml_ceilTrunc() },
            &[2.0, -1.0, -0.0, 2.0, -2.0, -0.0, -1.0, 5.0],
        )
    });
}

// M9-TS-86
//
// JS: `Math.round` sends halves toward +Infinity (round(-2.5) is -2, round(2.5) is 3),
// keeps -0 for small negatives, and `round(0.49999999999999994)` is 0 (floor(x + 0.5) would
// say 1).
#[test]
fn round_sends_halves_up_and_keeps_negative_zero() {
    in_every_gc_mode(|| {
        check(
            unsafe { ts_ml_rounds() },
            &[
                1.0,
                -0.0,
                2.0,
                -1.0,
                3.0,
                -2.0,
                2.0,
                -3.0,
                0.0,
                -0.0,
                4503599627370497.0,
                7.0,
            ],
        )
    });
}

// M9-TS-87
//
// JS: `**` is right associative (`3 ** 2 ** 2` is 81), and `Math.pow(1, Infinity)` is NaN where
// C's pow gives 1.
#[test]
fn pow_follows_javascript_for_the_special_cases() {
    in_every_gc_mode(|| {
        check(
            unsafe { ts_ml_powers() },
            &[
                1024.0, 0.5, 81.0, -8.0, NAN, NAN, NAN, 1.0, 1.0, NAN, INF, 100.0,
            ],
        )
    });
}

// M9-TS-88
#[test]
fn clz32_fround_and_bitwise_not() {
    in_every_gc_mode(|| {
        check(
            unsafe { ts_ml_bits() },
            &[
                31.0, 32.0, 0.0, 0.0, 24.0, -6.0, 0.0, -1.0, -2.0, 0.0, 5.5, 16777216.0,
            ],
        )
    });
}

// M9-TS-89
#[test]
fn tan_asin_acos_atan_exp_and_logarithms_agree_to_nine_places() {
    in_every_gc_mode(|| {
        check(
            unsafe { ts_ml_transcendental() },
            &[
                1557407725.0,
                523598776.0,
                1047197551.0,
                1107148718.0,
                2718281828.0,
                2302585093.0,
                3321928095.0,
                3000000000.0,
                3000000000.0,
                -2000000000.0,
                693147181.0,
                2302585093.0,
                1442695041.0,
                434294482.0,
                707106781.0,
            ],
        )
    });
}

// M9-TS-90
//
// JS: `a ??= 5` assigns only when `a` is null, `b ||= 7` when `b` is falsy, `c &&= 4` when `c` is
// truthy, and the target of `xs[i++] ??= 10` is evaluated once (i ends at 2).
#[test]
fn logical_and_power_assignments_assign_only_when_they_must() {
    in_every_gc_mode(|| {
        check(unsafe { ts_ml_assignments() }, &[5.0, 7.0, 4.0, 0.0, 32.0]);
        check(unsafe { ts_ml_assignOnce() }, &[10.0, 4.0, 2.0]);
    });
}

// M9-TS-91
//
// `Math.random()` is in [0, 1) (no bad values over 10,000 draws, mean about 0.5), and seeding the
// generator makes the sequence repeat.
#[test]
fn random_is_in_range_and_repeats_after_the_same_seed() {
    in_every_gc_mode(|| unsafe {
        ts_rt_random_seed(42);
        let first = ts_ml_randomInRange(10000.0);
        assert_eq!(first, 5.0);
        ts_rt_random_seed(42);
        assert_eq!(ts_ml_randomInRange(10000.0), first);
    });
}
