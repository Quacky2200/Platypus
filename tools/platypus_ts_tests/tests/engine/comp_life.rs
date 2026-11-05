// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The lifecycle of components script adds and removes (`bench/comp_life.ts`): `onAwake` and
//! `onDestroy` when the calling hook returns, `destroy()` on a component, the focus and pause hooks,
//! and objects destroyed through the scene API. IDs are in TESTS.md (M9-TS-707..).
use platypus_scene::Scene;
use platypus_scene::api::scene_api as api;
use platypus_ts_rt::{NativeHost, Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use platypus_ts_tests::comp_life_t::*;
use ts_api::Handle;

fn all(_: u64) -> bool {
    true
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_clf_init() };
    f();
    ts_rt_gc_stress(true);
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    f();
    ts_rt_gc_enable(true);
}

fn log() -> String {
    let p = unsafe { ts_clf_logText() };
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

// M9-TS-707
//
// A component removed by script (`removeComponent`, or `destroy()` on itself) is gone for
// `getComponents` at once and its `onDestroy` runs when the calling code returns to the host.
#[test]
fn removed_components_run_on_destroy_when_the_host_settles() {
    in_every_gc_mode(|| {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        unsafe { ts_clf_clearLog() };
        let go = api::create();
        host.spawn("Watcher", Some(go.to_raw())).unwrap();
        assert_eq!(log(), "A");
        assert_eq!(unsafe { ts_clf_count(go.to_raw()) }, 1.0);
        unsafe { ts_clf_removeWatcher(go.to_raw()) };
        assert_eq!(unsafe { ts_clf_count(go.to_raw()) }, 0.0, "gone at once");
        assert_eq!(log(), "A", "onDestroy waits for the host");
        assert!(host.settle().is_empty());
        assert_eq!(log(), "AD");
        assert!(host.is_empty());

        unsafe { ts_clf_clearLog() };
        host.spawn("Watcher", Some(go.to_raw())).unwrap();
        unsafe { ts_clf_suicide(go.to_raw()) };
        host.settle();
        assert_eq!(log(), "AD");
        assert_eq!(unsafe { ts_clf_count(go.to_raw()) }, 0.0);
    });
}

// M9-TS-708
//
// A component added and removed in the same hook never wakes; the other hooks keep their order
// (`onStart` before the first `onUpdate`).
#[test]
fn a_component_added_and_removed_in_one_hook_never_wakes() {
    in_every_gc_mode(|| {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        unsafe { ts_clf_clearLog() };
        let go = api::create();
        host.spawn("Spawner", Some(go.to_raw())).unwrap();
        assert!(host.update(0.1, &all).is_empty());
        assert_eq!(log(), "+-");
        assert_eq!(unsafe { ts_clf_count(go.to_raw()) }, 0.0);
        assert_eq!(host.len(), 1, "only the spawner");
        unsafe { ts_clf_clearLog() };
        host.spawn("Watcher", Some(go.to_raw())).unwrap();
        host.update(0.1, &all);
        // Awake at spawn; the spawner's own add/remove; then start before the first update.
        assert_eq!(log(), "A+-SU");
    });
}

// M9-TS-709
//
// `onApplicationFocus(hasFocus)` and `onApplicationPause(paused)` get their flag; components of
// inactive objects are skipped.
#[test]
fn focus_and_pause_hooks_get_their_flag() {
    in_every_gc_mode(|| {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        let go = api::create();
        host.spawn("Watcher", Some(go.to_raw())).unwrap();
        unsafe { ts_clf_clearLog() };
        host.application_pause(true, &all);
        host.application_focus(false, &all);
        host.application_focus(true, &all);
        host.application_pause(false, &all);
        assert_eq!(log(), "PfFp");
        unsafe { ts_clf_clearLog() };
        host.application_focus(true, &|_| false);
        assert_eq!(log(), "", "inactive objects are skipped");
    });
}

// M9-TS-710
//
// Destroying an object through the scene API reports it; the host removes its components (their
// `onDestroy` runs) and the engine components on it.
#[test]
fn destroyed_objects_take_their_components_with_them() {
    in_every_gc_mode(|| {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        let go = api::create();
        let child = api::create();
        api::set_parent(child, go, false);
        host.spawn("Watcher", Some(go.to_raw())).unwrap();
        host.spawn("Watcher", Some(child.to_raw())).unwrap();
        unsafe { ts_clf_clearLog() };
        api::destroy(go);
        let gone = platypus_engine_api::objects::process_destroyed();
        assert_eq!(gone.len(), 2, "the object and its child");
        let raws: Vec<u64> = gone.iter().map(|o| o.to_raw()).collect();
        host.destroy_on_objects(&raws);
        assert_eq!(log(), "DD");
        assert!(host.is_empty());
    });
}

// M9-TS-711
//
// `go.getComponent(Camera)` (and `Animation`, `Animator`) finds the engine component on that
// object only.
#[test]
fn get_component_finds_engine_components() {
    in_every_gc_mode(|| {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        platypus_engine_api::animation::reset();
        platypus_engine_api::camera::reset();
        let a = api::create();
        let b = api::create();
        let kinds = |o: platypus_scene::ObjectId| unsafe { ts_clf_engineKinds(o.to_raw()) };
        assert_eq!(kinds(a), 0.0);
        unsafe {
            ts_clf_addEngine(a.to_raw(), 1.0);
            ts_clf_addEngine(b.to_raw(), 4.0);
            ts_clf_addEngine(a.to_raw(), 2.0);
        }
        assert_eq!(kinds(a), 3.0);
        assert_eq!(kinds(b), 4.0);
    });
}
