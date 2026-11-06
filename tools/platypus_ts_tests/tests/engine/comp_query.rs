// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/comp_query.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_comp_query_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-703..).
use platypus_ts_tests::comp_query_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// A new scene and a host for the program classes.
fn reset() {
    platypus_scene::api::with_scene(|s| *s = platypus_scene::Scene::new());
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_cqy_init() };
    // Every mode starts with a new host (the registry of live components) and scene.
    let mut host = platypus_components::NativeHost::new(classes());
    reset();
    f();
    ts_rt_gc_stress(true);
    drop(host);
    host = platypus_components::NativeHost::new(classes());
    reset();
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    drop(host);
    host = platypus_components::NativeHost::new(classes());
    reset();
    f();
    ts_rt_gc_enable(true);
    drop(host);
}

fn expected() -> Value {
    serde_json::from_str(include_str!("../../bench/comp_query_expected.json")).unwrap()
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

// M9-TS-703
#[test]
fn lookups_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("lookups", ts_cqy_lookups());
    });
}

// M9-TS-704
#[test]
fn children_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("children", ts_cqy_children());
    });
}

// M9-TS-707
#[test]
fn hierarchy_searches_match_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("hierarchy", ts_cqy_hierarchy());
    });
}

// M9-TS-705
#[test]
fn finding_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("finding", ts_cqy_finding());
    });
}

// M9-TS-706
#[test]
fn removing_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("removing", ts_cqy_removing());
    });
}
