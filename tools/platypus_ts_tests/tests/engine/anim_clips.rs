// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/anim_clips.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_anim_clips_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-676..).
use platypus_ts_tests::anim_clips_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// A new scene and no curves or clips.
fn reset() {
    platypus_animation::clip::reset();
    platypus_scene::api::with_scene(|s| *s = platypus_scene::Scene::new());
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_acl_init() };
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
    serde_json::from_str(include_str!("../../bench/anim_clips_expected.json")).unwrap()
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

// M9-TS-676
#[test]
fn curves_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("curves", ts_acl_curves());
    });
}

// M9-TS-677
#[test]
fn wrapping_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("wrapping", ts_acl_wrapping());
    });
}

// M9-TS-678
#[test]
fn editing_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("editing", ts_acl_editing());
    });
}

// M9-TS-679
#[test]
fn clips_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("clips", ts_acl_clips());
    });
}

// M9-TS-680
#[test]
fn sampling_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("sampling", ts_acl_sampling());
    });
}

// M9-TS-681
#[test]
fn wrap_modes_when_sampling_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("wrapModesWhenSampling", ts_acl_wrapModesWhenSampling());
    });
}

// M9-TS-682
#[test]
fn euler_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("euler", ts_acl_euler());
    });
}

// M9-TS-683
#[test]
fn persistence_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("persistence", ts_acl_persistence());
    });
}
