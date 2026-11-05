// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The renderer tiers (`platypus_core::renderer`) against the real engine API, and what the compiler
//! finds a program needs.

use platypus_build::manifests;
use platypus_core::renderer::{PRELUDE_NAMES, TAGS, Tier, Usage, owner_names, usage_of_symbols};
use std::collections::BTreeSet;

/// The classes and namespaces (not enums) of some manifests.
fn owners(ms: &[String]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for m in ms {
        let v: serde_json::Value = serde_json::from_str(m).unwrap();
        names.insert(v["namespace"].as_str().unwrap().to_string());
        for c in v["classes"].as_array().into_iter().flatten() {
            names.insert(c["name"].as_str().unwrap().to_string());
        }
    }
    names
}

fn usage(src: &str) -> Usage {
    let ms = manifests();
    let out = platypus_ts::compile_with_manifests(src, "speed", "t_", 0, &ms).unwrap();
    usage_of_symbols(&out.externs, &out.uses, &ms)
}

fn names(u: &Usage) -> Vec<&str> {
    u.uses.iter().map(String::as_str).collect()
}

// M9-HOST-04
#[test]
fn every_tagged_name_is_a_class_or_namespace_of_the_engine_api() {
    let all = owner_names(&manifests());
    let stale: Vec<&str> = TAGS
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !all.contains(*n) && !PRELUDE_NAMES.contains(n))
        .collect();
    assert!(stale.is_empty(), "tagged but not in the API: {stale:?}");
}

// M9-HOST-05
#[test]
fn every_class_and_namespace_of_the_render_api_needs_the_world() {
    let untagged: Vec<String> = owners(&platypus_render_api::manifests())
        .into_iter()
        .filter(|n| platypus_core::renderer::tier_of(n) == Tier::Ui)
        .collect();
    assert!(
        untagged.is_empty(),
        "platypus_render_api names without a tier (add them to platypus_core::renderer::TAGS): {untagged:?}"
    );
    // And the world renderers declared elsewhere.
    for n in ["Camera", "Gizmos"] {
        assert!(platypus_core::renderer::tier_of(n) > Tier::Ui, "{n}");
    }
}

// M9-HOST-06
#[test]
fn a_program_that_is_only_ui_needs_only_the_ui() {
    let u = usage(
        r#"function App() { const [n, setN] = useState(0);
             return <box onClick={() => setN(n + 1)}><text>{`clicked ${n}`}</text></box>; }
           createRoot().render(<App />);
           Debug.log(`${Time.time} ${Vector3.up.y}`);"#,
    );
    assert_eq!(u.tier, Tier::Ui, "{:?}", u.uses);
    assert!(u.uses.is_empty());
}

// M9-HOST-07
#[test]
fn a_camera_is_the_2d_world_and_a_mesh_the_3d_one() {
    let cam = usage(
        r#"const go = new GameObject("cam");
           go.addComponent(Camera, { orthographic: true });"#,
    );
    assert_eq!(
        (cam.tier, names(&cam)),
        (Tier::World2d, vec!["Camera", "GameObject"])
    );
    let mesh = usage(
        r#"const go = new GameObject("cam");
           go.addComponent(Camera, {});
           const cube = new GameObject("cube");
           cube.addComponent(MeshRenderer, { mesh: "cube" });"#,
    );
    assert_eq!(mesh.tier, Tier::World3d, "{:?}", mesh.uses);
    assert!(mesh.uses.contains(&"MeshRenderer".to_string()));
}

// M9-HOST-08
#[test]
fn world_text_is_the_2d_world() {
    let u = usage(
        r#"const go = new GameObject("label");
           const t = go.addComponent(TextMesh, {});
           t.text = "hi";"#,
    );
    assert_eq!(u.tier, Tier::World2d, "{:?}", u.uses);
    assert!(u.uses.contains(&"TextMesh".to_string()));
}

// M9-HOST-13
#[test]
fn engine_calls_named_only_in_strings_do_not_count() {
    // The md-reader bundles docs that show `Model.load(..).whenLoaded()` and
    // `Light.measureIlluminance(..)`: text, not calls.
    let u = usage(
        r#"const DOC = "const m = Model.load('a.glb'); await m.whenLoaded(); Light.measureIlluminance(p);";
           Debug.log(DOC);"#,
    );
    assert_eq!(u.tier, Tier::Ui, "{:?}", u.uses);
}

// M9-HOST-16
#[test]
fn a_registered_component_alone_is_the_2d_world() {
    // It touches no object, but the host runs the scene for it (the base class's `transform`
    // getter may name `GameObject` too).
    let u = usage(
        r#"class Ticker extends Component {
             override onUpdate(_dt: number) { console.log("tick"); }
           }
           register({ Ticker });"#,
    );
    assert_eq!(u.tier, Tier::World2d, "{:?}", u.uses);
    assert!(names(&u).contains(&"Component"), "{:?}", u.uses);
    // A class that extends Component counts even if nothing registers it here.
    let u = usage(r#"class Idle extends Component {} console.log("x");"#);
    assert_eq!(u.tier, Tier::World2d, "{:?}", u.uses);
    assert!(names(&u).contains(&"Component"), "{:?}", u.uses);
}

// M9-HOST-17
#[test]
fn a_game_object_alone_is_the_2d_world() {
    let u = usage(r#"const go = new GameObject("empty"); go.name = "still empty";"#);
    assert_eq!((u.tier, names(&u)), (Tier::World2d, vec!["GameObject"]));
}

// M9-HOST-18
#[test]
fn components_named_only_in_strings_and_comments_do_not_count() {
    let u = usage(
        r#"// class Old extends Component {} and register({ Old })
           const DOC = "class Player extends Component { onUpdate() {} }\nregister({ Player });";
           const go = "new GameObject('x')";
           console.log(DOC + go);"#,
    );
    assert_eq!(u.tier, Tier::Ui, "{:?}", u.uses);
    // A `.children` of plain data (a markdown tree) is not the scene's.
    let u = usage(
        r#"type Node = { children: string[] };
           const n: Node = { children: ["a"] };
           console.log(`${n.children.length}`);"#,
    );
    assert_eq!(u.tier, Tier::Ui, "{:?}", u.uses);
}

// M9-HOST-09
#[test]
fn a_camera_in_a_function_nothing_calls_does_not_count() {
    let u = usage(
        r#"function unused(): void {
             const go = new GameObject("cam");
             go.addComponent(Camera, {});
           }
           Debug.log("only ui");"#,
    );
    assert_eq!(u.tier, Tier::Ui, "{:?}", u.uses);
}
