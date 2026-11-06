// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/assets.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_assets_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-617..).
use platypus_ts_tests::assets_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The game has two assets with `.meta` files.
fn reset() {
    use platypus_engine_api::assets::{self, AssetEntry};
    assets::reset();
    assets::set_assets(vec![
        AssetEntry {
            path: "textures/a.png".into(),
            guid: "guid-a".into(),
            importer_json: r#"{"type":"texture","filterMode":"point"}"#.into(),
        },
        AssetEntry {
            path: "sounds/b.ogg".into(),
            guid: "guid-b".into(),
            importer_json: r#"{"type":"audio"}"#.into(),
        },
    ]);
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_ast_init() };
    reset();
    f();
    ts_rt_gc_stress(true);
    reset();
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    reset();
    f();
    ts_rt_gc_enable(true);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/assets_expected.json")).unwrap()
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

// M9-TS-617
#[test]
fn lookups_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("lookups", ts_ast_lookups());
    });
}

// M9-TS-618
#[test]
fn sources_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("sources", ts_ast_sources());
    });
}

// M9-TS-619
#[test]
fn importers_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("importers", ts_ast_importers());
    });
}

// M9-TS-620
#[test]
fn enumerate_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("enumerate", ts_ast_enumerate());
    });
}
