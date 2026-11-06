// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/anim_animator.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_anim_animator_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-692..).
use platypus_ts_tests::anim_animator_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// A new scene and no animation objects.
fn reset() {
    platypus_engine_api::animation::reset();
    platypus_scene::api::with_scene(|s| *s = platypus_scene::Scene::new());
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_ani_init() };
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
    serde_json::from_str(include_str!("../../bench/anim_animator_expected.json")).unwrap()
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

// M9-TS-692
#[test]
fn builder_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("builder", ts_ani_builder());
    });
}

// M9-TS-693
#[test]
fn classes_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("classes", ts_ani_classes());
    });
}

// M9-TS-694
#[test]
fn same_either_way_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("sameEitherWay", ts_ani_sameEitherWay());
    });
}

// M9-TS-695
#[test]
fn blend_tree_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("blendTree", ts_ani_blendTree());
    });
}

// M9-TS-696
#[test]
fn jumping_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("jumping", ts_ani_jumping());
    });
}

// M9-TS-697
#[test]
fn persistence_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("persistence", ts_ani_persistence());
    });
}
