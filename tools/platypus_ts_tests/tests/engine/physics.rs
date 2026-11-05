// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Native physics from compiled programs (`bench/physics.ts`, `platypus_physics_api`). IDs are in
//! TESTS.md (M9-TS-727..).
use platypus_scene::Scene;
use platypus_scene::api::scene_api as api;
use platypus_ts_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use platypus_ts_tests::physics_t::*;
use ts_api::Handle;

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let run = || {
        platypus_physics_api::reset();
        unsafe { ts_phy_init() };
        f();
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
}

// M9-TS-727
//
// A ball dropped on a floor falls under gravity and comes to rest on it (centre one radius above
// the floor's top face), with no speed left.
#[test]
fn a_ball_falls_and_rests_on_a_floor() {
    in_every_gc_mode(|| unsafe {
        let y = ts_phy_fall(240.0);
        assert!((y - 1.0).abs() < 0.05, "rests at {y}");
        platypus_physics_api::reset();
        assert!(ts_phy_restSpeed(240.0) < 0.05);
    });
}

// M9-TS-728
//
// A ray answers at once, even for colliders added since the last step: distance, point and normal.
#[test]
fn a_ray_answers_at_once() {
    in_every_gc_mode(|| unsafe {
        // Distance 4.5 (from y 5 to the top face at 0.5), point y 0.5, normal (0, 1, 0).
        let packed = ts_phy_rayDown();
        assert!((packed - (45.0 + 0.5 + 1000.0)).abs() < 1e-3, "{packed}");
        platypus_physics_api::reset();
        assert_eq!(ts_phy_rayMiss(), 0.0);
    });
}

// M9-TS-729
//
// A body attached to a GameObject moves it each tick.
#[test]
fn an_attached_body_moves_its_object() {
    in_every_gc_mode(|| unsafe {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let go = api::create();
        let y = ts_phy_followed(go.to_raw(), 200.0);
        assert!((y - 1.0).abs() < 0.05, "{y}");
    });
}

// M9-TS-730
//
// Gravity, impulses and the body kind through the API.
#[test]
fn gravity_impulses_and_kinds() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_phy_gravityY(), -3.0);
        platypus_physics_api::reset();
        let vx = ts_phy_impulse();
        assert!(
            (vx - 2.0).abs() < 1e-4,
            "impulse 4 on 2 kg gives 2 m/s: {vx}"
        );
        platypus_physics_api::reset();
        let p = ts_phy_dropAndKind();
        assert_eq!(
            String::from_utf16_lossy(&Str::from_raw(p as *const Str).to_utf16()),
            "fixed"
        );
    });
}
