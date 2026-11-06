// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The whole sine-circle demo component (`bench/sine_demo_full.ts`: the `SineDemo` class and its
//! tweens, copied from commit 9dc2f51 with the single edit its header lists) compiled by
//! web_ui_ts and driven by the Rust host against the declared engine API: a camera is made, the
//! curve is drawn as gizmo lines, keys change the formula and pause the demo, the music is asked
//! for. IDs are in TESTS.md (M9-TS-596..).
use platypus_components::NativeHost;
use platypus_engine_api::audio::{self, AudioCommand};
use platypus_engine_api::{camera, gizmos, input};
use platypus_scene::Scene;
use platypus_ts_tests::sdf;
use web_ui_rt::link_anchor;

fn all(_: u64) -> bool {
    true
}

fn fresh_host() -> NativeHost {
    assert_ne!(link_anchor(), 0);
    assert_ne!(web_ui_bridge::link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    gizmos::reset();
    camera::reset();
    audio::reset();
    input::reset();
    unsafe { sdf::ts_sdf_init() };
    NativeHost::new(sdf::classes())
}

// M9-TS-596
//
// `onAwake` makes the camera (`new GameObject("Main Camera", { position })` and
// `addComponent(Camera, { orthographic, orthographicSize, backgroundColor })`), loads the music
// (`audio.load(path, { stream: true })`) and draws the first frame of the curve (361 points, 3
// spline steps each, closed: 1,083 lines).
#[test]
fn the_demo_wakes_up_makes_a_camera_loads_music_and_draws_the_curve() {
    let _serial = platypus_ts_tests::serial();
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let mut host = fresh_host();
    host.spawn("SineDemo", None).expect("spawns");
    let cams = camera::cameras();
    assert_eq!(cams.len(), 1);
    assert!(cams[0].1.orthographic);
    assert_eq!(cams[0].1.orthographic_size, 5.0);
    assert_eq!(cams[0].1.background.r, 1.0, "white");
    let frame = gizmos::take_frame().expect("the first frame was drawn");
    assert_eq!(frame.camera, cams[0].0);
    assert_eq!(frame.lines.len(), 1083);
    let cmds = audio::take_commands();
    assert!(matches!(
        &cmds[0],
        AudioCommand::Load { path, stream: true, .. } if path == "assets/dancin.mp3"
    ));
}

// M9-TS-597
//
// Input drives it: Space starts the demo (the music plays, looped, and the tweens move the
// curve), a digit picks another formula, F5 adds the centre ring (24 more lines).
#[test]
fn keys_start_the_music_change_the_formula_and_show_the_centre() {
    let _serial = platypus_ts_tests::serial();
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let mut host = fresh_host();
    host.spawn("SineDemo", None).expect("spawns");
    let first = gizmos::take_frame().expect("frame 1");
    audio::take_commands();

    // Paused at the start: a frame changes nothing.
    input::tick();
    assert!(host.update(0.1, &all).is_empty());
    assert!(gizmos::take_frame().is_none(), "paused: nothing is redrawn");

    input::key_down_event("Space");
    input::tick();
    assert!(host.update(0.5, &all).is_empty());
    input::key_up_event("Space");
    let cmds = audio::take_commands();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, AudioCommand::Play { options, .. } if options.looped)),
        "{cmds:?}"
    );
    // Space is handled at the end of the update: the next frame is the first one that runs.
    input::tick();
    assert!(host.update(0.5, &all).is_empty());
    let moved = gizmos::take_frame().expect("running: redrawn");
    assert_eq!(moved.lines.len(), 1083);
    assert_ne!(
        moved.lines[10].from, first.lines[10].from,
        "the tweens moved the curve"
    );

    // Digit2: the "normal" formula; a new curve.
    input::key_down_event("Digit2");
    input::tick();
    assert!(host.update(0.1, &all).is_empty());
    let normal = gizmos::take_frame().expect("redrawn");
    assert_ne!(normal.lines[100].from, moved.lines[100].from);

    // F5: the centre ring is drawn as well.
    input::key_down_event("F5");
    input::tick();
    assert!(host.update(0.1, &all).is_empty());
    let with_centre = gizmos::take_frame().expect("redrawn");
    assert_eq!(with_centre.lines.len(), 1083 + 24);
}
