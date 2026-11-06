// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The component / GameObject surface of scene.ts from compiled code (`bench/comp_api.ts`). IDs
//! are in TESTS.md (M9-TS-1010..).
use platypus_components::NativeHost;
use platypus_scene::Scene;
use platypus_ts_tests::comp_api_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_cap_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn text(p: *mut u8) -> String {
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

fn host() -> (Scene, NativeHost) {
    (Scene::new(), NativeHost::new(classes()))
}

// M9-TS-1010
//
// `children` lists an object's children in order (length, for-of, index), `root` and `parent`.
#[test]
fn children_lists_the_object_tree() {
    in_every_gc_mode(|| unsafe {
        let (mut scene, _host) = host();
        let _scope = platypus_scene::api::enter(&mut scene);
        assert_eq!(text(ts_cap_tree()), "2;BC;C;true,true,true,true,true;");
    });
}

// M9-TS-1011
//
// `components` lists the components on an object in the order they were added (subclass tests
// with `instanceof` work on the elements).
#[test]
fn components_lists_the_components_of_an_object() {
    in_every_gc_mode(|| unsafe {
        let (mut scene, _host) = host();
        let _scope = platypus_scene::api::enter(&mut scene);
        assert_eq!(text(ts_cap_listing()), "3,true,true");
    });
}

// M9-TS-1012
//
// `GameObject.roots` lists the world roots; `find` and `byId` find objects.
#[test]
fn roots_find_and_by_id() {
    in_every_gc_mode(|| unsafe {
        let (mut scene, _host) = host();
        let _scope = platypus_scene::api::enter(&mut scene);
        assert_eq!(text(ts_cap_roots()), "1,true,true,true,true");
    });
}

// M9-TS-1013
//
// `go.scene` is the scene the object is in (`SceneManager.sceneOf`).
#[test]
fn an_object_knows_its_scene() {
    in_every_gc_mode(|| unsafe {
        let (mut scene, _host) = host();
        let _scope = platypus_scene::api::enter(&mut scene);
        let s = text(ts_cap_sceneOfObject());
        assert!(s.starts_with("true,"), "{s}");
    });
}
