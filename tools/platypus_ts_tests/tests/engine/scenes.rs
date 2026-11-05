// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Scene files, `instantiate`, prefabs and `SceneManager` for compiled programs
//! (`bench/scenes.ts`, `platypus_scenes`). IDs are in TESTS.md (M9-TS-712..).
use platypus_scene::api::scene_api as api;
use platypus_scene::{ObjectId, Scene};
use platypus_ts_rt::{
    NativeHost, Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress, ts_rt_str_new,
};
use platypus_ts_tests::scenes_t::*;
use serde_json::{Value, json};
use ts_api::Handle;

const ARENA: &str = r#"{"format":"platypus.scene","version":1,"name":"Arena","objects":[
 {"id":1,"name":"Cam","position":[0,2,5],"components":[
   {"id":2,"type":"Camera","properties":{"fieldOfView":50,"backgroundColor":{"$type":"Color","value":[0.1,0.2,0.3,1]}}}]},
 {"id":3,"name":"Hero","position":[1,0,0],"scale":[2,2,2],"layer":3,"components":[
   {"id":4,"type":"Spinner","properties":{"speed":90,"axis":{"$type":"Vector3","value":[0,0.5,1]},
     "buddy":{"$ref":"Component","id":6},"target":{"$ref":"GameObject","id":5}}}],
  "children":[{"id":5,"name":"Pal","enabled":false,"components":[
    {"id":6,"type":"Spinner","properties":{"speed":7}},
    {"id":7,"type":"Bobber","properties":{"height":1.5}},
    {"id":8,"type":"Hologram","properties":{"intensity":2}}]}]}
]}"#;

const OTHER: &str = r#"{"format":"platypus.scene","version":1,"name":"Other","objects":[
 {"id":1,"name":"Solo","components":[{"id":2,"type":"Bobber","properties":{"height":9}}]}]}"#;

fn s(text: &str) -> *mut u8 {
    unsafe { ts_rt_str_new(text.as_ptr(), text.len() as i64) as *mut u8 }
}

fn text(p: *mut u8) -> String {
    assert!(!p.is_null());
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

fn obj(raw: u64) -> ObjectId {
    ObjectId::from_raw(raw)
}

/// A fresh world per GC mode: a scene, a host with the program's classes and no scene state.
fn in_every_gc_mode(f: impl Fn(&mut NativeHost)) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let run = || {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        platypus_scenes::reset();
        platypus_engine_api::camera::reset();
        platypus_engine_api::animation::reset();
        platypus_scenes::take_warnings();
        unsafe { ts_scf_init() };
        let mut host = NativeHost::new(classes());
        f(&mut host);
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
}

fn child_of(o: ObjectId) -> ObjectId {
    platypus_scene::api::with_scene(|sc| sc.children(o)[0]).unwrap()
}

// M9-TS-712
//
// A scene file loads: objects, transforms, layers, enabled flags, the engine `Camera` and the
// program's components with number, value-class and reference properties; a component of a kind
// the game does not have is skipped with a warning; components wake when the host settles.
#[test]
fn a_scene_file_loads_into_objects_and_components() {
    in_every_gc_mode(|host| unsafe {
        assert_ne!(ts_scf_loadScene(s(ARENA)), 0.0);
        assert_eq!(text(ts_scf_activeName()), "Arena");
        assert_eq!(ts_scf_sceneCount(), 1.0, "the Main scene was replaced");
        assert_eq!(ts_scf_rootCount(), 2.0);
        let cam = obj(ts_scf_rootAt(0.0));
        let hero = obj(ts_scf_rootAt(1.0));
        let pal = child_of(hero);
        assert_eq!(
            platypus_scene::api::with_scene(|sc| (
                sc.name(hero).unwrap().to_owned(),
                sc.layer(hero),
                sc.local(hero).unwrap(),
                sc.enabled(pal)
            ))
            .unwrap()
            .0,
            "Hero"
        );
        let local = platypus_scene::api::with_scene(|sc| sc.local(hero).unwrap()).unwrap();
        assert_eq!((local.position.x, local.scale.y), (1.0, 2.0));
        assert_eq!(
            platypus_scene::api::with_scene(|sc| sc.layer(hero)).unwrap(),
            3
        );
        assert!(!platypus_scene::api::with_scene(|sc| sc.enabled(pal)).unwrap());
        // The camera.
        let c = platypus_engine_api::camera::camera_on(cam).expect("a camera");
        let (fov, bg) =
            platypus_engine_api::camera::with_camera(c, |d| (d.fov, d.background.b)).unwrap();
        assert_eq!((fov, bg), (50.0, 0.3));
        // The program's components.
        assert_eq!(ts_scf_speedOf(hero.to_raw()), 90.0);
        assert_eq!(ts_scf_speedOf(pal.to_raw()), 7.0);
        assert_eq!(ts_scf_axisY(hero.to_raw()), 0.5);
        assert_eq!(ts_scf_bobHeight(pal.to_raw()), 1.5);
        assert_eq!(ts_scf_buddyObject(hero.to_raw()), pal.to_raw());
        assert_eq!(ts_scf_targetObject(hero.to_raw()), pal.to_raw());
        // Hologram is not a kind of this game.
        let warnings = platypus_scenes::take_warnings();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("unknown component Hologram")),
            "{warnings:?}"
        );
        // Components wake with the host.
        assert_eq!(text(ts_scf_logText()), "");
        host.settle();
        assert_eq!(text(ts_scf_logText()), "aa");
    });
}

// M9-TS-713
//
// Saving gives the same scene data back: file ids for objects and components, references as
// `$ref` ids, defaults left out; loading what was saved and saving again changes nothing.
#[test]
fn a_scene_saves_and_loads_back_the_same() {
    in_every_gc_mode(|host| unsafe {
        ts_scf_loadScene(s(ARENA));
        host.settle();
        let first: Value = serde_json::from_str(&text(ts_scf_saved())).unwrap();
        assert_eq!(first["format"], "platypus.scene");
        assert_eq!(first["name"], "Arena");
        let hero = &first["objects"][1];
        assert_eq!(hero["name"], "Hero");
        assert_eq!(hero["layer"], 3);
        let spinner = &hero["components"][0];
        assert_eq!(spinner["type"], "Spinner");
        assert_eq!(spinner["properties"]["speed"], 90.0);
        assert_eq!(
            spinner["properties"]["axis"],
            json!({"$type": "Vector3", "value": [0.0, 0.5, 1.0]})
        );
        let pal = &hero["children"][0];
        assert_eq!(pal["enabled"], false);
        let pal_spinner = &pal["components"][0];
        // The buddy reference is the id the file gives Pal's spinner; the target is Pal.
        assert_eq!(
            spinner["properties"]["buddy"],
            json!({"$ref": "Component", "id": pal_spinner["id"]})
        );
        assert_eq!(
            spinner["properties"]["target"],
            json!({"$ref": "GameObject", "id": pal["id"]})
        );
        assert_eq!(pal["components"][1]["properties"]["height"], 1.5);
        // Loading the saved data and saving it again gives the same data.
        let again = text(ts_scf_saved());
        ts_scf_loadScene(s(&again));
        host.settle();
        let second: Value = serde_json::from_str(&text(ts_scf_saved())).unwrap();
        assert_eq!(first, second);
    });
}

// M9-TS-714
//
// `GameObject.instantiate(go)` copies the object with its children and components; references
// inside the copy point at the copies and the copy keeps the original's world place.
#[test]
fn instantiate_copies_an_object_tree_and_remaps_references() {
    in_every_gc_mode(|host| unsafe {
        ts_scf_loadScene(s(ARENA));
        host.settle();
        let hero = obj(ts_scf_rootAt(1.0));
        let copy = obj(ts_scf_copy(hero.to_raw()));
        host.settle();
        assert_ne!(copy, hero);
        assert_eq!(ts_scf_rootCount(), 3.0);
        let copy_pal = child_of(copy);
        assert_ne!(copy_pal, child_of(hero));
        assert_eq!(ts_scf_speedOf(copy.to_raw()), 90.0);
        assert_eq!(ts_scf_speedOf(copy_pal.to_raw()), 7.0);
        assert_eq!(ts_scf_buddyObject(copy.to_raw()), copy_pal.to_raw());
        assert_eq!(ts_scf_targetObject(copy.to_raw()), copy_pal.to_raw());
        assert_eq!(ts_scf_bobHeight(copy_pal.to_raw()), 1.5);
        let p = platypus_scene::api::with_scene(|sc| {
            (
                sc.world(copy).position.x,
                sc.world(copy).scale.x,
                sc.enabled(copy_pal),
            )
        })
        .unwrap();
        assert_eq!(p, (1.0, 2.0, false));
        // The copy's components woke too (two spinners).
        assert_eq!(text(ts_scf_logText()), "aaaa");
        // Changing the copy leaves the original alone.
        ts_scf_setAxis(copy.to_raw(), 0.0, 4.0, 0.0);
        assert_eq!(ts_scf_axisY(copy.to_raw()), 4.0);
        assert_eq!(ts_scf_axisY(hero.to_raw()), 0.5);
    });
}

// M9-TS-715
//
// `instantiate(go, { position })` places the copy; `{ parent }` makes it a child; a reference to
// something outside the copy stays pointing at the original.
#[test]
fn instantiate_options_and_references_outside_the_copy() {
    in_every_gc_mode(|host| unsafe {
        ts_scf_loadScene(s(ARENA));
        host.settle();
        let hero = obj(ts_scf_rootAt(1.0));
        let pal = child_of(hero);
        let cam = obj(ts_scf_rootAt(0.0));
        let at = obj(ts_scf_copyAt(hero.to_raw(), 5.0, 6.0, 7.0));
        let w = platypus_scene::api::with_scene(|sc| sc.world(at).position).unwrap();
        assert_eq!((w.x, w.y, w.z), (5.0, 6.0, 7.0));
        let under = obj(ts_scf_copyUnder(pal.to_raw(), cam.to_raw()));
        assert_eq!(
            platypus_scene::api::with_scene(|sc| sc.parent(under)).unwrap(),
            Some(cam)
        );
        // Pal's spinner points at nothing; link it to Hero's, then copy Pal: the copy's buddy is
        // the original Hero spinner (outside the copy).
        ts_scf_link(pal.to_raw(), hero.to_raw());
        let copy = obj(ts_scf_copy(pal.to_raw()));
        assert_eq!(ts_scf_buddyObject(copy.to_raw()), hero.to_raw());
        assert_eq!(ts_scf_targetObject(copy.to_raw()), hero.to_raw());
        host.settle();
    });
}

// M9-TS-716
//
// Several scenes: additive loads keep the others, a single load replaces them, unloading destroys
// a scene's objects, and `dontDestroyOnLoad` objects survive a single load.
#[test]
fn scenes_load_additively_unload_and_keep_objects() {
    in_every_gc_mode(|host| unsafe {
        ts_scf_loadScene(s(ARENA));
        let hero = obj(ts_scf_rootAt(1.0));
        let other = ts_scf_loadAdditive(s(OTHER));
        assert_eq!(ts_scf_sceneCount(), 2.0);
        assert_eq!(
            text(ts_scf_activeName()),
            "Arena",
            "additive does not change the active scene"
        );
        let events = platypus_scenes::take_events();
        assert!(events.iter().any(|e| matches!(
            e,
            platypus_scenes::SceneEvent::Loaded { additive: true, .. }
        )));
        assert!(platypus_scenes::set_active(other as u32));
        assert_eq!(text(ts_scf_activeName()), "Other");
        assert_eq!(ts_scf_rootCount(), 1.0);
        let solo = obj(ts_scf_rootAt(0.0));
        assert_eq!(text(ts_scf_sceneOfName(solo.to_raw())), "Other");
        assert_eq!(text(ts_scf_sceneOfName(hero.to_raw())), "Arena");
        // Keep Hero, then load a scene on its own: Arena goes, Hero stays.
        ts_scf_keep(hero.to_raw());
        ts_scf_loadScene(s(OTHER));
        assert_eq!(ts_scf_sceneCount(), 1.0);
        assert!(
            !platypus_scene::api::with_scene(|sc| sc.contains(solo)).unwrap(),
            "the old scene's objects are gone"
        );
        assert!(
            platypus_scene::api::with_scene(|sc| sc.contains(hero)).unwrap(),
            "kept"
        );
        // Unloading destroys: the destroyed objects are reported once for the host.
        let gone = platypus_engine_api::objects::process_destroyed();
        assert!(!gone.is_empty());
        host.destroy_on_objects(&gone.iter().map(|o| o.to_raw()).collect::<Vec<_>>());
    });
}

// M9-TS-717
//
// A prefab captures an object tree and places copies; its JSON round-trips through
// `Prefab.fromData`.
#[test]
fn prefabs_place_copies() {
    in_every_gc_mode(|host| unsafe {
        ts_scf_loadScene(s(ARENA));
        host.settle();
        let hero = obj(ts_scf_rootAt(1.0));
        assert_eq!(ts_scf_prefabOf(hero.to_raw()), 4.0, "named like the object");
        let placed = obj(ts_scf_placePrefab(hero.to_raw(), 3.0, 0.0, 0.0));
        host.settle();
        let pos = platypus_scene::api::with_scene(|sc| sc.world(placed).position.x).unwrap();
        assert_eq!(pos, 3.0);
        assert_eq!(ts_scf_speedOf(placed.to_raw()), 90.0);
        let pal = child_of(placed);
        assert_eq!(ts_scf_buddyObject(placed.to_raw()), pal.to_raw());
        let json = platypus_scenes::prefab_json(platypus_scenes::prefab_from_object(hero));
        let v: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["format"], "platypus.prefab");
        assert!(platypus_scenes::prefab_from_data(&json, None).is_ok());
    });
}

// M9-TS-718
//
// The scene files of `examples/scenes` load: the cameras and the compiled `Bobber`s are made with
// their saved properties, and the rendering components (not native yet) are skipped.
#[test]
fn the_example_scene_files_load() {
    in_every_gc_mode(|host| unsafe {
        let level2 = include_str!("../../../../examples/scenes/scenes/level2.scene.json");
        assert_ne!(ts_scf_loadScene(s(level2)), 0.0);
        host.settle();
        let data: Value = serde_json::from_str(level2).unwrap();
        let orbs: Vec<&Value> = data["objects"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["name"].as_str().unwrap().starts_with("Orb"))
            .collect();
        assert_eq!(orbs.len(), 6);
        let root_count = ts_scf_rootCount() as usize;
        assert_eq!(root_count, data["objects"].as_array().unwrap().len());
        let expected = orbs[0]["components"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["type"] == "Bobber")
            .unwrap()["properties"]["height"]
            .as_f64()
            .unwrap();
        let first_orb = (0..root_count)
            .map(|i| obj(ts_scf_rootAt(i as f64)))
            .find(|o| platypus_scene::api::with_scene(|sc| sc.name(*o) == Some("Orb 1")).unwrap())
            .unwrap();
        assert_eq!(ts_scf_bobHeight(first_orb.to_raw()), expected);
        let warnings = platypus_scenes::take_warnings();
        // The engine's own rendering components load now (M9-TS-1226); only the game's are unknown.
        assert!(
            !warnings
                .iter()
                .any(|w| w.contains("unknown component MeshRenderer")
                    || w.contains("unknown component Light")),
            "{warnings:?}"
        );
        // The camera came with its saved field of view.
        let cam = (0..root_count)
            .map(|i| obj(ts_scf_rootAt(i as f64)))
            .find(|o| platypus_engine_api::camera::camera_on(*o).is_some())
            .unwrap();
        let fov = platypus_engine_api::camera::with_camera(
            platypus_engine_api::camera::camera_on(cam).unwrap(),
            |d| f64::from(d.fov),
        )
        .unwrap();
        let want = data["objects"][0]["components"][0]["properties"]["fieldOfView"]
            .as_f64()
            .unwrap();
        assert_eq!(fov, want);
        let _ = api::count();
    });
}

// M9-TS-719
//
// An engine object stringifies as `{}` (its members are not own properties, as with host
// objects in a browser); script-made fields beside it are kept.
#[test]
fn engine_objects_stringify_as_empty_objects() {
    in_every_gc_mode(|host| unsafe {
        ts_scf_loadScene(s(ARENA));
        host.settle();
        let hero = obj(ts_scf_rootAt(1.0));
        assert_eq!(
            text(ts_scf_jsonOfHandle(hero.to_raw())),
            r#"{"object":{},"n":1}"#
        );
    });
}

// M9-TS-723
//
// Value classes stringify compactly as their numbers (`[1,2,3]`, `[r,g,b,a]`): a difference to
// the TypeScript classes, which give an object of fields.
#[test]
fn value_classes_stringify_as_their_fields() {
    in_every_gc_mode(|_| unsafe {
        assert_eq!(
            text(ts_scf_jsonOfVector(1.0, 2.0, 3.0)),
            r#"{"v":[1,2,3],"c":[0.5,0.25,1,1],"q":[0,0,0,1]}"#
        );
    });
}

// M9-TS-724
//
// `JSON.parse` of a compact value class text builds the value.
#[test]
fn value_classes_parse_from_their_numbers() {
    in_every_gc_mode(|_| unsafe {
        assert_eq!(ts_scf_parseVector(s("[1,2,3]")), 321.0);
    });
}

// M9-TS-725
//
// The object form parses too: fields by name, unknown keys skipped, left-out fields 0 (alpha 1).
#[test]
fn value_classes_parse_from_objects_too() {
    in_every_gc_mode(|_| unsafe {
        assert_eq!(ts_scf_parseVector(s(r#"{"x":1,"y":2,"z":3}"#)), 321.0);
        assert_eq!(ts_scf_parseVector(s(r#"{"z":3,"w":9,"x":1}"#)), 301.0);
        assert_eq!(ts_scf_parseVector(s("{}")), 0.0);
        assert_eq!(ts_scf_parseColorAlpha(s(r#"{"r":0.5}"#)), 1.0);
        assert_eq!(ts_scf_parseColorAlpha(s(r#"{"a":0.25}"#)), 0.25);
    });
}

// M9-TS-726
//
// A `Vector2[]` crosses the boundary in place: `AnimationCurve.fromPoints(points, mode)`.
#[test]
fn a_vector_array_reaches_the_engine() {
    in_every_gc_mode(|_| unsafe {
        assert_eq!(ts_scf_curveFromPoints(s("linear"), 0.5), 5.0);
        assert_eq!(ts_scf_curveFromPoints(s("linear"), 1.5), 5.0);
        assert_eq!(ts_scf_curveFromPoints(s("step"), 0.5), 0.0);
        assert_eq!(ts_scf_curveDefaultMode(1.0), 2.0);
    });
}
