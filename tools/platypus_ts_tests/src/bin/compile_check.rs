// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `compile_check file.ts`: compiles a program with every engine API manifest (scene, colour, time,
//! input, gizmos, camera, screen, quality settings) and prints the first error, or the number of
//! functions. A development aid for the swap-out work (`docs/research/platypus-ts-sine-demo.md`).
fn main() {
    // Link everything the test crate's objects call (they are linked into every binary).
    assert_ne!(platypus_ts_rt::link_anchor(), 0);
    assert_ne!(platypus_ts_ui::link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    assert_ne!(platypus_ts_tests::demo_api_anchor(), 0);
    let _ = platypus_ts_tests::scene_anchor();
    let path = std::env::args().nth(1).expect("compile_check file.ts");
    let src = std::fs::read_to_string(&path).expect("read the file");
    let mut manifests = vec![platypus_scene::api::scene_api::MANIFEST.to_json()];
    manifests.extend(platypus_engine_api::manifests());
    manifests.extend(platypus_scenes::manifests());
    manifests.extend(platypus_physics_api::manifests());
    manifests.extend(platypus_render_api::manifests());
    manifests.extend(platypus_components::manifests());

    // Lowering and code generation both (set PLATYPUS_TS_TRACE=1 to see each function as it is built).
    match platypus_ts::compile_with_manifests(&src, "speed", "chk_", 40000, &manifests) {
        Ok(out) => println!("ok: {} bytes of object code", out.object.len()),
        Err(e) => {
            println!("error: {e}");
            std::process::exit(1);
        }
    }
}

// The `ts_rt_component_*` functions the linked programs call.
use platypus_components as _;
