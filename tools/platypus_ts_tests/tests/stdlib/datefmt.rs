// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Date formatting in compiled code (`bench/datefmt.ts`): `toLocale*String` and
//! `Intl.DateTimeFormat`. `bench/gen_datefmt_expected.ts` ran the same file in bun once per time
//! zone; every function here must return what bun returned in every zone, in the normal, stress GC
//! and collection-off modes. IDs are in TESTS.md (M9-TS-277..). The columns ICU4X cannot match are
//! not in the test program; `docs/differences.md` lists them.
use platypus_ts_tests::dfm::*;
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
    unsafe { ts_df_init() };
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
    serde_json::from_str(include_str!("../../bench/datefmt_expected.json")).unwrap()
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
    let (got, want) = (
        String::from_utf16_lossy(&got),
        String::from_utf16_lossy(&want),
    );
    if got != want {
        // The columns (fields of a line) that differ on any line, for trimming the test matrix;
        // `DF_DETAIL=1` also shows the first differing value of each.
        let mut cols = std::collections::BTreeSet::new();
        let (gl, wl): (Vec<_>, Vec<_>) = (got.lines().collect(), want.lines().collect());
        if gl.len() != wl.len() {
            eprintln!("MISMATCH {name} lines {} vs {}", gl.len(), wl.len());
        }
        for (g, w) in gl.iter().zip(&wl) {
            let (gp, wp): (Vec<_>, Vec<_>) = (g.split(" ; ").collect(), w.split(" ; ").collect());
            for (k, (a, b)) in gp.iter().zip(&wp).enumerate() {
                if a != b && cols.insert(k) && std::env::var("DF_DETAIL").is_ok() {
                    eprintln!("DETAIL {name} col {k}: got {a:?} want {b:?}");
                }
            }
        }
        eprintln!("MISMATCH {name} {zone} {cols:?}");
        panic!("{key}: output differs");
    }
}

macro_rules! date_test {
    ($id:literal, $test:ident, $name:literal, $f:ident) => {
        #[test]
        fn $test() {
            in_every_mode(|zone| unsafe { check_str($name, zone, $f()) });
        }
    };
}

// M9-TS-277: toLocaleString / toLocaleDateString / toLocaleTimeString with a locale and no options.
date_test!(
    277,
    defaults_per_locale_match_javascript,
    "defaultsPerLocale",
    ts_df_defaultsPerLocale
);
// M9-TS-278: dateStyle in 15 locales.
date_test!(
    278,
    date_style_full_matches_javascript,
    "dateStyle_full",
    ts_df_dateStyle_full
);
date_test!(
    278,
    date_style_long_matches_javascript,
    "dateStyle_long",
    ts_df_dateStyle_long
);
date_test!(
    278,
    date_style_medium_matches_javascript,
    "dateStyle_medium",
    ts_df_dateStyle_medium
);
date_test!(
    278,
    date_style_short_matches_javascript,
    "dateStyle_short",
    ts_df_dateStyle_short
);
// M9-TS-279: timeStyle in 15 locales, with zone names.
date_test!(
    279,
    time_style_full_matches_javascript,
    "timeStyle_full",
    ts_df_timeStyle_full
);
date_test!(
    279,
    time_style_long_matches_javascript,
    "timeStyle_long",
    ts_df_timeStyle_long
);
date_test!(
    279,
    time_style_medium_matches_javascript,
    "timeStyle_medium",
    ts_df_timeStyle_medium
);
date_test!(
    279,
    time_style_short_matches_javascript,
    "timeStyle_short",
    ts_df_timeStyle_short
);
// M9-TS-280: dateStyle with timeStyle.
date_test!(
    280,
    style_combos_match_javascript,
    "styleCombos",
    ts_df_styleCombos
);
// M9-TS-281: component options (year, month, day, weekday, hour, minute, second) in five locales.
date_test!(
    281,
    components_en_us_match_javascript,
    "componentsEnUs",
    ts_df_componentsEnUs
);
date_test!(
    281,
    components_en_gb_match_javascript,
    "componentsEnGb",
    ts_df_componentsEnGb
);
date_test!(
    281,
    components_de_match_javascript,
    "componentsDe",
    ts_df_componentsDe
);
date_test!(
    281,
    components_fr_match_javascript,
    "componentsFr",
    ts_df_componentsFr
);
date_test!(
    281,
    components_ja_match_javascript,
    "componentsJa",
    ts_df_componentsJa
);
// M9-TS-282: timeZoneName in six widths.
date_test!(
    282,
    time_zone_names_match_javascript,
    "timeZoneNames",
    ts_df_timeZoneNames
);
// M9-TS-283: the timeZone option.
date_test!(
    283,
    time_zone_option_matches_javascript,
    "timeZoneOption",
    ts_df_timeZoneOption
);
// M9-TS-284: `Intl.DateTimeFormat.format`, also detached and as a callback.
date_test!(
    284,
    format_object_matches_javascript,
    "formatObject",
    ts_df_formatObject
);
// M9-TS-285: an invalid date.
date_test!(
    285,
    invalid_date_matches_javascript,
    "invalidDate",
    ts_df_invalidDate
);
// M9-TS-286: bad options and tags are bun's RangeError / TypeError text.
date_test!(286, errors_match_javascript, "errors", ts_df_errors);
