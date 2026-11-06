// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The editor's model tests (`editor/test/*.test.ts`, formerly bun tests of `js/editor-ui`),
//! bundled like a game (`web_ui_ts::modules`) and compiled natively with the whole engine API.
//! Each file's `run()` returns what failed, one line each; every GC mode must give "".
//! IDs are in the files (M5-EDT-06..14, 18..20, 22, 24, M5-DND-08) and in TESTS.md.
use std::path::Path;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use web_ui_ts::jit::{JitOptions, compile_and_load};

fn manifests() -> Vec<String> {
    let mut all = platypus_engine_api::manifests();
    all.push(platypus_scene::api::scene_api::MANIFEST.to_json());
    all.extend(platypus_scenes::manifests());
    all.extend(platypus_physics_api::manifests());
    all.extend(platypus_render_api::manifests());
    all.extend(platypus_components::manifests());
    all
}

/// Compiles `editor/test/<file>` and runs its `run()` in the normal, stress GC and collection-off
/// modes; returns the failures (the same in every mode).
fn run_editor_test(file: &str, prefix: &'static str, struct_base: u32) -> String {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let entry = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../editor/test")
        .join(file);
    let ms = manifests();
    // The engine's global names stay the engine's (`dock.ts`'s `Rect` is renamed), as in a build.
    let dts = web_ui_ts::types_dts::generate(&ms).unwrap();
    let reserved = web_ui_ts::modules::global_names(&dts);
    let bundle = web_ui_ts::modules::bundle_reserving(&entry, &reserved).unwrap();
    let game = compile_and_load(
        &bundle.source,
        &JitOptions {
            prefix,
            struct_base,
            manifests: &ms,
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("{file}: {}", bundle.locate_in(&e)));
    let mut out = Vec::new();
    for mode in 0..3 {
        match mode {
            1 => ts_rt_gc_stress(true),
            2 => {
                ts_rt_gc_stress(false);
                ts_rt_gc_enable(false);
            }
            _ => {}
        }
        game.init().unwrap();
        let run: unsafe extern "C" fn() -> *mut u8 =
            unsafe { std::mem::transmute(game.function("run").expect("run()")) };
        let p = unsafe { run() };
        out.push(String::from_utf16_lossy(
            &unsafe { Str::from_raw(p as *const Str) }.to_utf16(),
        ));
    }
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(true);
    assert!(
        out.iter().all(|o| *o == out[0]),
        "{file}: GC modes disagree: {out:?}"
    );
    out.remove(0)
}

// M5-EDT-06..14, M5-EDT-18, M5-EDT-19
#[test]
fn the_editor_models_pass() {
    let failures = run_editor_test("models.test.ts", "edm_", 960_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M5-EDT-20
#[test]
fn the_editor_log_feed_passes() {
    let failures = run_editor_test("log.test.ts", "edl_", 962_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M5-DND-08
#[test]
fn the_editor_project_paths_pass() {
    platypus_engine_api::editor::reset();
    let failures = run_editor_test("project.test.ts", "edp_", 963_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M5-EDT-22
#[test]
fn the_editor_scene_camera_passes() {
    let failures = run_editor_test("scene-camera.test.ts", "eds_", 964_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M5-EDT-24
#[test]
fn the_editor_transform_handles_pass() {
    let failures = run_editor_test("handles.test.ts", "edh_", 965_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M5-UNDO-02..05
#[test]
fn the_editor_history_passes() {
    let failures = run_editor_test("history.test.ts", "edu_", 967_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M5-DND-21
#[test]
fn the_editor_placement_passes() {
    let failures = run_editor_test("placement.test.ts", "edpl_", 969_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M5-EDT-37
#[test]
fn the_editor_inspector_model_passes() {
    let failures = run_editor_test("inspector.test.ts", "edi_", 968_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M3-INS-11
#[test]
fn the_ui_inspector_value_logic_passes() {
    let failures = run_editor_test("ui-values.test.ts", "edv_", 970_000);
    assert!(failures.is_empty(), "\n{failures}");
}

// M5-EDT-31..33
#[test]
fn the_editor_file_dialog_passes() {
    let failures = run_editor_test("file-dialog.test.ts", "edf_", 966_000);
    assert!(failures.is_empty(), "\n{failures}");
}
