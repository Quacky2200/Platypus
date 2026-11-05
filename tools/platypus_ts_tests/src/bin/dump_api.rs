// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `dump_api`: prints every engine API manifest (the ones `compile_check` loads) as a compact
//! listing, one line per function: `Namespace.Class kind name(params) -> ret`. A development
//! aid for the engine facade (what the TypeScript examples can call, and what they cannot).
fn main() {
    assert_ne!(platypus_ts_rt::link_anchor(), 0);
    assert_ne!(platypus_ts_ui::link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    assert_ne!(platypus_ts_tests::demo_api_anchor(), 0);
    let _ = platypus_ts_tests::scene_anchor();
    let mut manifests = vec![platypus_scene::api::scene_api::MANIFEST.to_json()];
    manifests.extend(platypus_engine_api::manifests());
    manifests.extend(platypus_scenes::manifests());
    manifests.extend(platypus_physics_api::manifests());
    manifests.extend(platypus_render_api::manifests());
    manifests.extend(platypus_components::manifests());
    let mut lines = Vec::new();
    for m in &manifests {
        let v: serde_json::Value = serde_json::from_str(m).expect("manifest json");
        let ns = v["namespace"].as_str().unwrap_or("?");
        let fmt = |owner: &str, f: &serde_json::Value| {
            let params: Vec<String> = f["params"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|p| {
                    format!(
                        "{}{}: {}",
                        p["name"].as_str().unwrap_or("?"),
                        if p["optional"].as_bool() == Some(true) {
                            "?"
                        } else {
                            ""
                        },
                        p["type"].as_str().unwrap_or("?")
                    )
                })
                .collect();
            format!(
                "{owner} {} {}({}) -> {}",
                f["kind"].as_str().unwrap_or("static"),
                f["name"].as_str().unwrap_or("?"),
                params.join(", "),
                f["returns"].as_str().unwrap_or("?")
            )
        };
        for f in v["functions"].as_array().into_iter().flatten() {
            lines.push(fmt(ns, f));
        }
        for c in v["classes"].as_array().into_iter().flatten() {
            let owner = format!("{ns}.{}", c["name"].as_str().unwrap_or("?"));
            for f in c["functions"].as_array().into_iter().flatten() {
                lines.push(fmt(&owner, f));
            }
        }
    }
    lines.sort();
    for l in lines {
        println!("{l}");
    }
}

// The `ts_rt_component_*` functions the linked programs call.
use platypus_components as _;
