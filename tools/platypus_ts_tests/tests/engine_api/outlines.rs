// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The native `Shapes` and `Spline` from compiled code, against the TypeScript the facade used to
//! compile (`bench/outlines.ts`, prefix `otl_`), in the normal, stress GC and collection-off modes.
use platypus_scene::Scene;
use platypus_ts_tests::outlines_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    let run = || {
        platypus_engine_api::spline::reset();
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        unsafe { ts_otl_init() };
        f();
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
    platypus_engine_api::spline::reset();
}

fn text(p: *mut u8) -> String {
    assert!(!p.is_null());
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

// M9-TS-3110
#[test]
fn native_shapes_equal_the_facade_typescript_bit_for_bit() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_otl_shapesMatch()), "");
    });
}

// M9-TS-3111
#[test]
fn shape_points_are_vector2_objects_and_object_literals_pass_where_points_go() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_otl_shapePointsAreVectors()), "4|3|200|2|2");
    });
}

// M9-TS-3112
#[test]
fn native_splines_equal_the_facade_typescript_bit_for_bit() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_otl_splineMatch()), "");
    });
}

// M9-TS-3113
#[test]
fn spline_animate_pose_equals_the_old_components_path_and_direction() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_otl_animatePoseMatches()), "");
    });
}

// M9-TS-3114
#[test]
fn spline_knots_hits_and_enum_names_are_plain_objects_and_strings() {
    in_every_gc_mode(|| unsafe {
        // The hit is in the first segment half way; the point (1, 3, 0) is 3 from the line;
        // `parse` gives the index of a name (-1 for none) and `name` the name of an index.
        assert_eq!(
            text(ts_otl_splineSnapshotsAreObjects()),
            "2|linear|2|50|0|300|1|linear|bezier|-1|0.25"
        );
    });
}

// M9-TS-3115
#[test]
fn a_spline_animate_component_moves_its_object_each_update_and_pings_back() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_otl_animateSetup()),
            "true|2|pingPong|none|true|true"
        );
        // pingPong over 2 s on a straight 4 long path: 1 at 0.5 s, 3 at 1.5 s, the end at 2 s,
        // and back at 2.5 s.
        for (dt, want) in [
            (0.5, "1|0.25|0.5"),
            (1.0, "3|0.75|1.5"),
            (0.5, "4|1|2"),
            (0.5, "3|0.75|2.5"),
        ] {
            platypus_engine_api::spline::update(dt);
            assert_eq!(text(ts_otl_animateWhere()), want);
        }
    });
}

// M9-TS-3116
#[test]
fn a_spline_animate_places_its_object_on_the_path_facing_along_it_in_every_loop_mode() {
    in_every_gc_mode(|| unsafe {
        // The test `js/engine` had: 1 s of 4 is 2 along, facing +x; 3.5 s is (4, 3) facing +y; 4.5 s
        // is round again; ping-pong at 5 s is on the way back; once stops at the end; a container
        // moves the path.
        assert_eq!(
            text(ts_otl_animateBehaviour()),
            "2,0|1,0|4,3|0,1|1,0|4,2|4,4|10,0"
        );
    });
}
