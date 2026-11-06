// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/scene_objects.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_scene_objects_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-698..).
use platypus_ts_tests::scene_objects_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// A new scene.
fn reset() {
    platypus_scene::api::with_scene(|s| *s = platypus_scene::Scene::new());
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_sob_init() };
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
    serde_json::from_str(include_str!("../../bench/scene_objects_expected.json")).unwrap()
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

// M9-TS-698
#[test]
fn hierarchy_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("hierarchy", ts_sob_hierarchy());
    });
}

// M9-TS-699
#[test]
fn world_transform_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("worldTransform", ts_sob_worldTransform());
    });
}

// M9-TS-700
#[test]
fn moving_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("moving", ts_sob_moving());
    });
}

// M9-TS-701
#[test]
fn points_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("points", ts_sob_points());
    });
}

// M9-TS-702
#[test]
fn keeping_world_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("keepingWorld", ts_sob_keepingWorld());
    });
}
