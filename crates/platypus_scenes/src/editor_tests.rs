// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `EditorScene`: snapshots for undo, and scene files in the project.

use crate::editor_api::{editor_scene_api as api, scene_name_of};
use crate::manager;
use platypus_scene::ObjectId;
use platypus_scene::api::with_scene;
use serde_json::Value;
use ts_api::Handle;

const SCENE: &str = r#"{"format":"platypus.scene","version":1,"name":"Room","objects":[
  {"id":1,"name":"A"},
  {"id":2,"name":"Lamp","position":[1,2,3],"children":[{"id":3,"name":"Bulb","components":[{"id":4,"type":"Camera","properties":{"fieldOfView":40}}]}]},
  {"id":5,"name":"C"}
]}"#;

fn names(list: &[ObjectId]) -> Vec<String> {
    with_scene(|s| {
        list.iter()
            .map(|o| s.name(*o).unwrap_or_default().to_owned())
            .collect()
    })
    .unwrap()
}

/// The editor's project is one for the process: these tests take turns.
static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn project(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "platypus-editor-scene-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    platypus_engine_api::editor::enable(&dir, &dir);
    dir
}

// M5-UNDO-01
#[test]
fn a_snapshot_builds_the_object_again_in_its_place() {
    manager::reset();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = project("snap");
    let s = manager::load_scene(SCENE, false).unwrap();
    let lamp = manager::roots(s)[1];
    let text = api::snapshot(lamp);
    assert!(!text.is_empty());
    platypus_scene::api::scene_api::destroy(lamp);
    assert_eq!(names(&manager::roots(s)), ["A", "C"]);
    let back = api::restore(&text, ObjectId::from_raw(0), 1.0);
    assert_ne!(back.to_raw(), 0);
    assert_eq!(names(&manager::roots(s)), ["A", "Lamp", "C"]);
    let kids = with_scene(|sc| sc.children(back).to_vec()).unwrap();
    assert_eq!(names(&kids), ["Bulb"]);
    let saved: Value = serde_json::from_str(&manager::serialize(s).unwrap()).unwrap();
    assert_eq!(
        saved["objects"][1]["position"],
        serde_json::json!([1.0, 2.0, 3.0])
    );
    assert_eq!(
        saved["objects"][1]["children"][0]["components"][0]["type"],
        "Camera"
    );
    // Under a parent.
    let a = manager::roots(s)[0];
    api::restore(&text, a, 0.0);
    let under = with_scene(|sc| sc.children(a).to_vec()).unwrap();
    assert_eq!(names(&under), ["Lamp"]);
    assert_eq!(api::restore("{}", ObjectId::from_raw(0), 0.0).to_raw(), 0);
    platypus_engine_api::editor::reset();
    let _ = std::fs::remove_dir_all(&dir);
}

// M5-SCN-11
#[test]
fn scenes_save_to_project_files_and_open_again() {
    manager::reset();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = project("files");
    manager::load_scene(SCENE, false).unwrap();
    assert!(api::save_scene("scenes/level.scene.json"));
    assert_eq!(api::scene_path(), "scenes/level.scene.json");
    assert_eq!(api::scene_name(), "level");
    let text = std::fs::read_to_string(dir.join("scenes/level.scene.json")).unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(data["name"], "level");
    assert_eq!(data["objects"].as_array().unwrap().len(), 3);

    assert!(api::new_scene("Untitled"));
    assert_eq!(api::scene_path(), "");
    assert_eq!(manager::roots(manager::active()).len(), 0);
    assert!(api::open_scene("scenes/level.scene.json"));
    assert_eq!(api::scene_path(), "scenes/level.scene.json");
    assert_eq!(
        names(&manager::roots(manager::active())),
        ["A", "Lamp", "C"]
    );
    assert!(!api::open_scene("scenes/missing.scene.json"));
    assert!(platypus_engine_api::editor::editor_api::last_error().contains("missing"));
    assert!(!api::save_scene("../outside.scene.json"));
    platypus_engine_api::editor::reset();
    let _ = std::fs::remove_dir_all(&dir);
}

// M5-EDT-35
#[test]
fn the_inspector_reads_and_sets_fields_of_built_in_and_kept_components() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    manager::reset();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let dir = project("inspect");
    let s = manager::load_scene(
        r#"{"format":"platypus.scene","version":1,"name":"I","objects":[{"id":1,"name":"Lamp","components":[
          {"id":2,"type":"Light","properties":{"type":"spot","intensity":2}},
          {"id":3,"type":"Spinner","properties":{"speed":3}}]}]}"#,
        false,
    )
    .unwrap();
    let lamp = manager::roots(s)[0];
    let comps = api::components(lamp);
    let kinds: Vec<_> = comps.iter().map(|c| (c.kind.as_str(), c.opaque)).collect();
    assert_eq!(kinds, [("Light", false), ("Spinner", true)]);
    let light = &comps[0];
    let fields = api::fields(lamp, "Light", light.handle);
    let ty = fields.iter().find(|f| f.name == "type").unwrap();
    assert_eq!((ty.ty.as_str(), ty.text.as_str()), ("enum", "spot"));
    assert_eq!(ty.options, "directional|point|spot");
    let color = fields.iter().find(|f| f.name == "color").unwrap();
    assert_eq!(color.ty, "color");
    assert_eq!(color.vec.len(), 4);
    let intensity = fields.iter().find(|f| f.name == "intensity").unwrap();
    assert_eq!((intensity.ty.as_str(), intensity.num), ("number", 2.0));

    assert!(api::set_field(
        lamp,
        "Light",
        light.handle,
        "intensity",
        "5"
    ));
    assert!(api::set_field(
        lamp,
        "Light",
        light.handle,
        "color",
        "[1,0,0,1]"
    ));
    let fields = api::fields(lamp, "Light", light.handle);
    let get = |n: &str| fields.iter().find(|f| f.name == n).unwrap().clone();
    assert_eq!(get("intensity").num, 5.0);
    assert_eq!(get("color").vec, [1.0, 0.0, 0.0, 1.0]);
    // Putting a field's old JSON back restores it (undo).
    assert!(api::set_field(
        lamp,
        "Light",
        light.handle,
        "intensity",
        &intensity.json
    ));
    assert_eq!(
        api::fields(lamp, "Light", light.handle)
            .iter()
            .find(|f| f.name == "intensity")
            .unwrap()
            .num,
        2.0
    );

    // A kept component's data is edited as data, and saved so.
    let spinner = &comps[1];
    assert!(api::set_field(
        lamp,
        "Spinner",
        spinner.handle,
        "speed",
        "7"
    ));
    let saved = manager::serialize(s).unwrap();
    assert!(saved.contains("\"speed\":7"), "{saved}");

    assert!(api::component_kinds().iter().any(|k| k == "Camera"));
    assert!(api::add_component(lamp, "Camera") > 0.0);
    assert_eq!(api::components(lamp).len(), 3);
    assert!(api::remove_component(lamp, "Spinner", spinner.handle));
    assert!(api::components(lamp).iter().all(|c| c.kind != "Spinner"));
    assert_eq!(api::add_component(lamp, "Nope"), 0.0);
    platypus_engine_api::editor::reset();
    let _ = std::fs::remove_dir_all(&dir);
}

// M5-EDT-48
#[test]
fn references_and_assets_are_typed_editable_fields() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    manager::reset();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let dir = project("refs");
    let s = manager::load_scene(
        r#"{"format":"platypus.scene","version":1,"name":"R","objects":[
          {"id":1,"name":"Target"},
          {"id":2,"name":"Cam","components":[{"id":3,"type":"CameraFollow","properties":{}},{"id":4,"type":"Camera","properties":{}}]}]}"#,
        false,
    )
    .unwrap();
    let (target, cam) = (manager::roots(s)[0], manager::roots(s)[1]);
    let comps = api::components(cam);
    let follow = comps.iter().find(|c| c.kind == "CameraFollow").unwrap();
    let field = |kind: &str, handle: f64, name: &str| {
        api::fields(cam, kind, handle)
            .into_iter()
            .find(|f| f.name == name)
            .unwrap()
    };
    // An empty reference is an editable object field, by the schema.
    let t = field("CameraFollow", follow.handle, "target");
    assert_eq!(
        (t.ty.as_str(), t.ref_kind.as_str(), t.read_only),
        ("object", "GameObject", false)
    );
    let json = format!(r#"{{"$ref":"GameObject","object":{}}}"#, target.to_raw());
    assert!(api::set_field(
        cam,
        "CameraFollow",
        follow.handle,
        "target",
        &json
    ));
    let t = field("CameraFollow", follow.handle, "target");
    assert_eq!((t.ty.as_str(), t.text.as_str()), ("object", "Target"));
    // An empty texture is an asset field (the hand-written Camera's schema).
    let camera = comps.iter().find(|c| c.kind == "Camera").unwrap();
    let tex = field("Camera", camera.handle, "targetTexture");
    assert_eq!(
        (tex.ty.as_str(), tex.ref_kind.as_str()),
        ("asset", "Texture")
    );
    let clear = field("Camera", camera.handle, "clearFlags");
    assert_eq!(
        (clear.ty.as_str(), clear.options.as_str()),
        ("enum", "skybox|solidColor")
    );
    assert_eq!(api::asset_from_file("Nope", "a.png"), "");
    assert!(
        platypus_kinds::fields_of("AudioSource")
            .iter()
            .any(|f| f.name == "clip" && f.ty == "sound")
    );
    platypus_engine_api::editor::reset();
    let _ = std::fs::remove_dir_all(&dir);
}

// M5-PRF-10
#[test]
fn objects_save_as_prefab_files_and_place_again() {
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    manager::reset();
    let mut scene = platypus_scene::Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    let dir = project("prefab");
    let s = manager::load_scene(SCENE, false).unwrap();
    let lamp = manager::roots(s)[1];
    assert!(api::save_prefab(lamp, "prefabs/Lamp.prefab.json"));
    let text = std::fs::read_to_string(dir.join("prefabs/Lamp.prefab.json")).unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(data["format"], "platypus.prefab");
    assert_eq!(data["name"], "Lamp");
    assert!(
        data["root"].get("position").is_none(),
        "placed at the origin"
    );
    assert_eq!(
        data["root"]["children"][0]["components"][0]["type"],
        "Camera"
    );

    let a = api::place_prefab("prefabs/Lamp.prefab.json", ObjectId::from_raw(0));
    let first = manager::roots(s)[0];
    let b = api::place_prefab("prefabs/Lamp.prefab.json", first);
    assert!(a.to_raw() != 0 && b.to_raw() != 0);
    assert_eq!(names(&manager::roots(s)), ["A", "Lamp", "C", "Lamp"]);
    let under = with_scene(|sc| sc.children(first).to_vec()).unwrap();
    assert_eq!(names(&under), ["Lamp"]);
    // Copies remember their file (saved as `prefab`), whole.
    let saved: Value = serde_json::from_str(&manager::serialize(s).unwrap()).unwrap();
    assert_eq!(saved["objects"][3]["prefab"], "prefabs/Lamp.prefab.json");
    assert_eq!(saved["objects"][3]["children"][0]["name"], "Bulb");
    assert_eq!(
        api::place_prefab("missing.prefab.json", ObjectId::from_raw(0)).to_raw(),
        0
    );

    // A file's `assets` table reaches the copy's loader (an inline-only prefab has none).
    let p = manager::prefab_from_data(
        r#"{"format":"platypus.prefab","version":1,"name":"P","root":{"id":1,"name":"P"},"assets":{"t1":{"type":"Nope"}}}"#,
        None,
    )
    .unwrap();
    assert!(manager::prefab_json(p).contains("\"assets\""));
    platypus_engine_api::editor::reset();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scene_names_come_from_their_files() {
    assert_eq!(scene_name_of("scenes/level.scene.json"), "level");
    assert_eq!(scene_name_of("a.json"), "a");
    assert_eq!(scene_name_of("x"), "x");
}
