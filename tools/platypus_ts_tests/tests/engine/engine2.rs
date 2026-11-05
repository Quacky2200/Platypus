// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Compiled code calling the engine API beyond the scene (`bench/engine2.ts`, `platypus_engine_api`):
//! `Color` statics and methods, namespace properties (`Time`, `Screen`, `Gizmos.color`) and a
//! handle class with a static (`Camera.attach`). IDs are in TESTS.md (M9-TS-593..).
use platypus_engine_api::{camera, gizmos, input, screen, time};
use platypus_scene::Scene;
use platypus_ts_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use platypus_ts_tests::en2::*;

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    unsafe { ts_en2_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn text(p: *mut u8) -> String {
    unsafe { ts_api::script_str(p as *const u8) }.into_owned()
}

// M9-TS-593
//
// `Color.hex`, `Color.black`, `new Color(..)` and the value methods `lerp` and `withAlpha`.
#[test]
fn color_statics_and_methods_work_in_compiled_code() {
    in_every_gc_mode(|| {
        assert_eq!(unsafe { ts_en2_colors() }, 10_000_250_075.0);
        assert_eq!(text(unsafe { ts_en2_hexRoundTrip() }), "#336699ff");
    });
}

// M9-TS-594
//
// Namespace properties: `Time.timeScale = ..` writes, `Time.deltaTime` reads; input state set by
// the host; `Screen.fullScreen = ..` and `QualitySettings.vSyncCount`.
#[test]
fn time_input_and_display_properties_work_in_compiled_code() {
    in_every_gc_mode(|| {
        time::reset();
        time::advance(0.5);
        assert_eq!(unsafe { ts_en2_timing() }, 551.0);
        assert_eq!(time::state().time_scale, 1.0);

        input::reset();
        input::key_down_event("Space");
        input::key_down_event("KeyV");
        input::mouse_button_event(0, true);
        input::mouse_move(2.0, 3.0);
        input::tick();
        assert_eq!(unsafe { ts_en2_keys() }, 2111.0);

        // Each GC mode starts afresh (the last one left vsync off).
        screen::reset();
        screen::set_size(800, 600);
        assert_eq!(screen::quality().v_sync_count, 1, "on by default");
        screen::quality_api::platypus_QualitySettings_setVSyncCount(1);
        let before = screen::state().full_screen;
        let v = unsafe { ts_en2_display() };
        assert_eq!(screen::state().full_screen, !before);
        assert_eq!(screen::quality().v_sync_count + 1, 1, "toggled from 1 to 0");
        assert_eq!(v % 100_000.0, 8000.0 + f64::from(u8::from(!before)));
    });
}

// M9-TS-595
//
// `Camera.attach(go)`, its properties (a bool, a number, a `Color`) and `Gizmos`: a static colour
// property, `drawLine` with `Vector3` values and `present(camera)`.
#[test]
fn gizmos_and_a_camera_work_in_compiled_code() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| {
        gizmos::reset();
        camera::reset();
        assert_eq!(unsafe { ts_en2_drawCurve(3.0) }, 5.0);
        let cams = camera::cameras();
        assert_eq!(cams.len(), 1);
        assert!(cams[0].1.orthographic);
        assert_eq!(cams[0].1.background.r, 1.0);
        let frame = gizmos::take_frame().expect("presented");
        assert_eq!(frame.camera, cams[0].0);
        assert_eq!(frame.lines.len(), 3);
        assert_eq!(frame.lines[2].to.x, 3.0);
        assert_eq!(frame.lines[0].color.r, 1.0);
        assert_eq!(frame.lines[0].color.g, 0.0);
    });
}
