// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Compiled scripts driving the real engine scene (`bench/scene.ts`): `platypus_scene`'s `Scene`
//! API, written with `#[ts_api]`, is called directly by native code the compiler made from
//! TypeScript. The scene is made current with `platypus_scene::api::enter`, as the host will around
//! running scripts. Every test runs normally, in stress GC mode and with collection off.
//! IDs are in TESTS.md (M9-TS-151..).
use platypus_scene::Scene;
use platypus_ts_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use platypus_ts_tests::scn::*;

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_scn_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-151
//
// A parent and child made, moved and destroyed from compiled code; the child's world position
// includes the parent's, `Scene.parent(x)` is truthy only when there is one, and destroying the
// parent takes the child with it.
#[test]
fn compiled_code_builds_and_moves_a_hierarchy_in_the_real_scene() {
    let mut scene = Scene::new();
    let scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_scn_moveAround(), 110_203_100.0);
    });
    drop(scope);
    assert!(scene.is_empty());
}

// M9-TS-152
#[test]
fn a_per_frame_update_loop_moves_an_object_round_a_circle() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        let want: f64 = (0..50)
            .map(|i| {
                let a = i as f64 * 0.1;
                a.cos() * 5.0 + a.sin() * 5.0
            })
            .sum();
        let got = ts_scn_orbit(50.0);
        assert!((got - want).abs() < 1e-3, "{got} vs {want}");
    });
}

// M9-TS-153
#[test]
fn stale_object_handles_miss_even_when_the_slot_is_reused() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_scn_staleHandles(), 10.0);
    });
}

// M9-TS-154
#[test]
fn a_quaternion_value_goes_in_and_comes_back_normalised() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        let q = glam::Quat::from_xyzw(0.0, 0.5, 0.0, 0.5).normalize();
        assert_eq!(ts_scn_rotation(), q.y as f64 * 1000.0 + q.w as f64);
    });
}

// M9-TS-155
//
// Thirty objects, their handles in an array, between collections at every allocation.
#[test]
fn many_objects_and_their_handles_survive_collections() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_scn_crowd(30.0), 870_030.0);
        assert_eq!(platypus_ts_tests::scene_anchor(), 0);
    });
}

// M9-TS-156
//
// The hierarchy of M9-TS-151 written with handle classes: `child.transform.setParent(..)`,
// `transform.localPosition = new Vector3(..)`, `transform.position` read back, `go.destroy()`.
#[test]
fn handle_classes_build_and_move_a_hierarchy() {
    let mut scene = Scene::new();
    let scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_scn_classHierarchy(), 110_203_100.0);
    });
    drop(scope);
    assert!(scene.is_empty());
}

// M9-TS-157
//
// Boolean, number and handle properties on a `GameObject`, a method (`exists`) and the
// `transform.gameObject` round trip.
#[test]
fn handle_class_properties_methods_and_round_trips() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        // layer 5, off while disabled, was disabled, on again, same object, alive, then gone.
        assert_eq!(ts_scn_classFlags(), 150_011.0 + 0.0);
    });
}

// M9-TS-158
#[test]
fn an_update_loop_moves_an_object_through_transform_position() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        let want: f64 = (0..50)
            .map(|i| {
                let a = i as f64 * 0.1;
                a.cos() * 5.0 + a.sin() * 5.0
            })
            .sum();
        let got = ts_scn_classOrbit(50.0);
        assert!((got - want).abs() < 1e-3, "{got} vs {want}");
    });
}

// M9-TS-159
//
// Methods and getters of the `Vector3` value class (`add`, `scale`, `dot`, `cross`, `lerp`,
// `distance`, `magnitude`, `normalized`), including a chained call on a call's result.
#[test]
fn vector3_methods_and_getters_are_direct_calls() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_scn_vectorMath(), 14_110_532_042.0);
    });
}

// M9-TS-160
#[test]
fn quaternion_methods_rotate_combine_and_invert() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        let got = ts_scn_quatMath();
        assert!((got - 99_000.0).abs() < 0.5, "{got}");
    });
}

// M9-TS-161
//
// A vector held across the allocations of a loop (a new argument and a new result per step), at
// every allocation in stress mode.
#[test]
fn a_vector_survives_allocations_between_method_calls() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        let mut acc = glam::Vec3::ZERO;
        for i in 0..200 {
            acc = (acc + glam::Vec3::new(i as f32, 1.0, 0.0)) * 0.5;
        }
        let want = acc.x as f64 * 1000.0 + acc.y as f64;
        assert_eq!(ts_scn_vectorLoop(200.0), want);
    });
}

// M9-TS-183
//
// A string goes in as a script string (borrowed by the engine for the call) and comes back as a
// new one: `name` set and read, `GameObject.find(name)`, a name with a handle comparison.
#[test]
fn names_cross_the_boundary_as_strings() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        let r = ts_scn_names() as *const platypus_ts_rt::Str;
        assert_eq!(
            platypus_ts_rt::Str::from_raw(r).to_string_lossy(),
            "player-1/same/no/5"
        );
    });
}

// M9-TS-184
//
// Accents and an emoji survive the round trip, and a lone surrogate arrives as U+FFFD.
#[test]
fn unicode_names_round_trip_and_lone_surrogates_become_replacement_characters() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        assert_eq!(ts_scn_unicodeName(), 1008.0);
        assert_eq!(ts_scn_lossyName(), 300_000.0 + 0xFFFD as f64);
    });
}

// M9-TS-185
//
// Thirty names made, read back and found, with a collection at every allocation.
#[test]
fn many_names_survive_collections() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| unsafe {
        // "n0".."n9" are 2 bytes, "n10".."n29" 3: 10 * 2 + 20 * 3 = 80; the found name "n2" is 2.
        assert_eq!(ts_scn_manyNames(30.0), 80_002.0);
    });
}

// M9-TS-186
//
// The engine side reads script strings through `ts_api::ScriptStr`, so its layout must be the
// runtime's `Str`.
#[test]
fn the_engines_view_of_a_script_string_is_the_runtimes_layout() {
    assert_eq!(
        std::mem::size_of::<ts_api::ScriptStr>(),
        std::mem::size_of::<platypus_ts_rt::Str>()
    );
    let s = unsafe { platypus_ts_rt::ts_rt_str_new("añ😀".as_ptr(), "añ😀".len() as i64) };
    assert_eq!(unsafe { ts_api::script_str(s as *const u8) }, "añ😀");
}
