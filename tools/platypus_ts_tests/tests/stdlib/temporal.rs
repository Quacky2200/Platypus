// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/temporal.ts` against bun (`bench/gen_temporal_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-480..).
use platypus_ts_tests::temporal_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_tp_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/temporal_expected.json")).unwrap()
}

fn check_str(name: &str, got: *mut u8) {
    let want: Vec<u16> = expected()[name]["s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u16)
        .collect();
    assert!(!got.is_null(), "{name} returned null");
    let got = unsafe { Str::from_raw(got as *const Str) }.to_utf16();
    assert_eq!(
        String::from_utf16_lossy(&got),
        String::from_utf16_lossy(&want),
        "{name}"
    );
}

// M9-TS-480
#[test]
fn date_fields_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateFields", ts_tp_dateFields());
    });
}

// M9-TS-481
#[test]
fn date_week_years_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateWeekYears", ts_tp_dateWeekYears());
    });
}

// M9-TS-482
#[test]
fn date_constrain_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateConstrain", ts_tp_dateConstrain());
    });
}

// M9-TS-483
#[test]
fn date_add_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateAdd", ts_tp_dateAdd());
    });
}

// M9-TS-484
#[test]
fn date_with_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateWith", ts_tp_dateWith());
    });
}

// M9-TS-485
#[test]
fn date_until_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateUntil", ts_tp_dateUntil());
    });
}

// M9-TS-486
#[test]
fn date_compare_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateCompare", ts_tp_dateCompare());
    });
}

// M9-TS-487
#[test]
fn date_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateErrors", ts_tp_dateErrors());
    });
}

// M9-TS-488
#[test]
fn date_formats_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateFormats", ts_tp_dateFormats());
    });
}

// M9-TS-489
#[test]
fn time_basics_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("timeBasics", ts_tp_timeBasics());
    });
}

// M9-TS-490
#[test]
fn time_math_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("timeMath", ts_tp_timeMath());
    });
}

// M9-TS-491
#[test]
fn time_compare_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("timeCompare", ts_tp_timeCompare());
    });
}

// M9-TS-492
#[test]
fn time_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("timeErrors", ts_tp_timeErrors());
    });
}

// M9-TS-493
#[test]
fn date_time_basics_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateTimeBasics", ts_tp_dateTimeBasics());
    });
}

// M9-TS-494
#[test]
fn date_time_math_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateTimeMath", ts_tp_dateTimeMath());
    });
}

// M9-TS-495
#[test]
fn date_time_diff_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateTimeDiff", ts_tp_dateTimeDiff());
    });
}

// M9-TS-496
#[test]
fn date_to_date_time_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("dateToDateTime", ts_tp_dateToDateTime());
    });
}

// M9-TS-497
#[test]
fn duration_basics_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("durationBasics", ts_tp_durationBasics());
    });
}

// M9-TS-498
#[test]
fn duration_strings_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("durationStrings", ts_tp_durationStrings());
    });
}

// M9-TS-499
#[test]
fn duration_math_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("durationMath", ts_tp_durationMath());
    });
}

// M9-TS-500
#[test]
fn duration_total_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("durationTotal", ts_tp_durationTotal());
    });
}

// M9-TS-501
#[test]
fn duration_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("durationErrors", ts_tp_durationErrors());
    });
}

// M9-TS-502
#[test]
fn instant_basics_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("instantBasics", ts_tp_instantBasics());
    });
}

// M9-TS-503
#[test]
fn instant_offsets_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("instantOffsets", ts_tp_instantOffsets());
    });
}

// M9-TS-504
#[test]
fn instant_math_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("instantMath", ts_tp_instantMath());
    });
}

// M9-TS-505
#[test]
fn instant_errors_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("instantErrors", ts_tp_instantErrors());
    });
}

// M9-TS-506
#[test]
fn now_is_sane_matches_javascript() {
    in_every_gc_mode(|| unsafe {
        check_str("nowIsSane", ts_tp_nowIsSane());
    });
}
