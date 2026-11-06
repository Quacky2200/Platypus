// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Engine API shapes the original (QuickJS-era) example sources use, from compiled code
//! (`bench/originals_engine.ts`, prefix `ogn_`), in the normal, stress GC and collection-off
//! modes. IDs are in TESTS.md (M9-TS-3184..).
use platypus_scene::Scene;
use platypus_ts_tests::originals_engine_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn reset() {
    platypus_render_api::reset();
    platypus_engine_api::camera::reset();
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    let run = || {
        reset();
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        unsafe { ts_ogn_init() };
        f();
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
    reset();
}

fn text(p: *mut u8) -> String {
    assert!(!p.is_null());
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

// M9-TS-3184
#[test]
fn pop_and_shift_over_handle_arrays_give_a_handle_or_nothing() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_ogn_handleArrays()), "true|true|true|true|0");
    });
}

// M9-TS-3185
#[test]
fn line_renderer_points_read_and_write_as_a_property() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_ogn_lineRendererPoints()), "3|3|4|true|0");
    });
}

// M9-TS-3186
#[test]
fn colors_come_from_spread_arrays_with_alpha_defaulting_to_one() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_ogn_colorFromArrays()), "0.5|0.75|0.5|1");
    });
}

// M9-TS-3187
#[test]
fn flat_meshes_take_only_some_of_their_lists() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_ogn_partialFlatMesh()), "3|1|3");
    });
}

// M9-TS-3188
#[test]
fn engine_enum_strings_and_the_programs_literal_unions_meet() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(
            text(ts_ogn_enumsMeetUnions()),
            "exponential,linear,off,exponentialSquared,exponential|exponential|false|E"
        );
    });
}

// M9-TS-3189
#[test]
fn handles_are_map_values_map_keys_and_set_members() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_ogn_handlesInMaps()), "beta|true|3|2|alphabeta");
    });
}

// M9-TS-3198
#[test]
fn gamepads_have_buttons_and_axes_by_name_and_connect_events_carry_the_pad() {
    use platypus_engine_api::input;
    in_every_gc_mode(|| unsafe {
        input::reset();
        input::gamepad_connected(3, "Pad");
        input::gamepad_button(3, "South", 1.0);
        input::gamepad_axis(3, "LeftStickX", 0.5);
        assert_eq!(text(ts_ogn_gamepadList()), "1|Pad|1|0.5|0|true");
        assert_eq!(ts_ogn_watchPads(), 1.0);
        assert_eq!(text(ts_ogn_lastPadSeen()), "none");
        input::gamepad_connected(4, "Two");
        // The pad is connected before its buttons move: the event shows them at that time.
        assert_eq!(text(ts_ogn_lastPadSeen()), "Two:4:0");
        input::gamepad_disconnected(4);
        input::gamepad_connected(5, "Three");
        assert_eq!(text(ts_ogn_lastPadSeen()), "Three:5:0");
        input::reset();
    });
}

// M9-TS-3199
#[test]
fn measure_illuminance_resolves_after_the_next_physics_step() {
    use web_ui_rt::ts_rt_microtasks_run;
    in_every_gc_mode(|| unsafe {
        platypus_physics_api::reset();
        assert_eq!(ts_ogn_startMeasure(), -1.0);
        ts_rt_microtasks_run();
        assert_eq!(ts_ogn_measuredLux(), -1.0, "nothing before a step");
        platypus_physics_api::step();
        ts_rt_microtasks_run();
        let lux = ts_ogn_measuredLux();
        assert!(lux > 0.0, "{lux}");
        assert_eq!(lux, ts_ogn_expectedLux());
        platypus_physics_api::reset();
    });
}

// M9-API-1645
#[test]
fn the_native_light_meter_faces_the_normal_and_counts_colliders_as_shadows() {
    use web_ui_rt::ts_rt_microtasks_run;
    in_every_gc_mode(|| unsafe {
        platypus_physics_api::reset();
        // 16 pi lumens over 4 pi sr = 4 cd, 1 lux at 2 m head-on (times the range fade, 0.9998); 0 facing away.
        assert_eq!((ts_ogn_startShadowed() * 1000.0).round(), 9998.0);
        platypus_physics_api::step();
        ts_rt_microtasks_run();
        assert_eq!(
            ts_ogn_shadowedLux(),
            0.0,
            "the roof's collider is in the way"
        );
        platypus_physics_api::reset();
    });
}

// M9-TS-3200
#[test]
fn a_closure_given_to_after_next_step_runs_once_after_a_step() {
    in_every_gc_mode(|| unsafe {
        platypus_physics_api::reset();
        let before = ts_ogn_stepsSeen();
        assert_eq!(ts_ogn_afterStepTwice(), before);
        platypus_physics_api::step();
        assert_eq!(ts_ogn_stepsSeen(), before + 1.0);
        platypus_physics_api::step();
        assert_eq!(ts_ogn_stepsSeen(), before + 1.0, "once");
        platypus_physics_api::reset();
    });
}
