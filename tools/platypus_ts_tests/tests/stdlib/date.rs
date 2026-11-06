// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Date` in compiled code (`bench/date.ts`): construction, getters and setters in local time and
//! UTC, parsing, ISO and UTC strings, arithmetic. `bench/gen_date_expected.ts` ran the same file in
//! bun once per time zone; every function here must return what bun returned in every zone, in the
//! normal, stress GC and collection-off modes. IDs are in TESTS.md (M9-TS-240..).
use platypus_ts_tests::dat::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_date_set_zone, ts_rt_gc_enable, ts_rt_gc_stress};

const ZONES: [&str; 5] = [
    "America/New_York",
    "Europe/London",
    "Asia/Kolkata",
    "Australia/Lord_Howe",
    "UTC",
];

/// Runs `f(zone)` in every zone and GC mode.
fn in_every_mode(f: impl Fn(&str)) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_dt_init() };
    for zone in ZONES {
        assert!(unsafe { ts_rt_date_set_zone(zone.as_ptr(), zone.len() as i64) });
        f(zone);
        ts_rt_gc_stress(true);
        f(zone);
        ts_rt_gc_stress(false);
        ts_rt_gc_enable(false);
        f(zone);
        ts_rt_gc_enable(true);
    }
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/date_expected.json")).unwrap()
}

fn check_str(name: &str, zone: &str, got: *mut u8) {
    let key = format!("{name}@{zone}");
    let want: Vec<u16> = expected()[&key]["s"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u16)
        .collect();
    assert!(!got.is_null(), "{key} returned null");
    let got = unsafe { Str::from_raw(got as *const Str) }.to_utf16();
    assert_eq!(
        String::from_utf16_lossy(&got),
        String::from_utf16_lossy(&want),
        "{key}"
    );
}

macro_rules! date_test {
    ($id:literal, $test:ident, $name:literal, $f:ident) => {
        // M9-TS-$id
        #[test]
        fn $test() {
            in_every_mode(|zone| unsafe { check_str($name, zone, $f()) });
        }
    };
}

// M9-TS-240
date_test!(
    240,
    fields_of_instants_match_javascript,
    "fieldsOfInstants",
    ts_dt_fieldsOfInstants
);
// M9-TS-241
date_test!(
    241,
    utc_and_overflow_match_javascript,
    "utcAndOverflow",
    ts_dt_utcAndOverflow
);
// M9-TS-242
date_test!(
    242,
    local_construction_matches_javascript,
    "constructLocal",
    ts_dt_constructLocal
);
// M9-TS-243
date_test!(
    243,
    other_constructors_match_javascript,
    "constructOther",
    ts_dt_constructOther
);
// M9-TS-244
date_test!(244, setters_match_javascript, "setters", ts_dt_setters);
// M9-TS-245
date_test!(
    245,
    iso_and_json_match_javascript,
    "isoAndJson",
    ts_dt_isoAndJson
);
// M9-TS-246
date_test!(
    246,
    utc_and_date_strings_match_javascript,
    "strings",
    ts_dt_strings
);
// M9-TS-247
date_test!(
    247,
    iso_parsing_matches_javascript,
    "parseIso",
    ts_dt_parseIso
);
// M9-TS-248
date_test!(
    248,
    legacy_parsing_matches_javascript,
    "parseLegacy",
    ts_dt_parseLegacy
);
// M9-TS-249
date_test!(
    249,
    arithmetic_and_comparison_match_javascript,
    "arithmetic",
    ts_dt_arithmetic
);
// M9-TS-250
date_test!(
    250,
    timezone_offsets_match_javascript,
    "offsets",
    ts_dt_offsets
);
// M9-TS-251
date_test!(251, now_is_the_current_time, "now", ts_dt_now);
// M9-TS-252
date_test!(
    252,
    time_clipping_matches_javascript,
    "clipping",
    ts_dt_clipping
);
// M9-TS-276
date_test!(
    276,
    to_string_matches_javascript,
    "toStrings",
    ts_dt_toStrings
);
