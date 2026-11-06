// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `bench/anim_player.ts` compiled against the native engine API and compared with the original
//! TypeScript run in bun (`bench/gen_anim_player_expected.ts`), in the normal, stress GC and
//! collection-off modes. IDs are in TESTS.md (M9-TS-684..).
use platypus_ts_tests::anim_player_t::*;
use serde_json::Value;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// A new scene and no animation objects.
fn reset() {
    platypus_engine_api::animation::reset();
    platypus_engine_api::camera::reset();
    platypus_scene::api::with_scene(|s| *s = platypus_scene::Scene::new());
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_apl_init() };
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
    serde_json::from_str(include_str!("../../bench/anim_player_expected.json")).unwrap()
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

// M9-TS-684
#[test]
fn playing_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("playing", ts_apl_playing());
    });
}

// M9-TS-685
#[test]
fn fading_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("fading", ts_apl_fading());
    });
}

// M9-TS-686
#[test]
fn wrap_modes_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("wrapModes", ts_apl_wrapModes());
    });
}

// M9-TS-687
#[test]
fn events_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("events", ts_apl_events());
    });
}

// M9-TS-688
#[test]
fn default_clip_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("defaultClip", ts_apl_defaultClip());
    });
}

// M9-TS-689
#[test]
fn components_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("components", ts_apl_components());
    });
}

// M9-TS-690
#[test]
fn blending_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("blending", ts_apl_blending());
    });
}

// M9-TS-691
#[test]
fn inactive_matches_the_original() {
    in_every_gc_mode(|| unsafe {
        check_str("inactive", ts_apl_inactive());
    });
}
