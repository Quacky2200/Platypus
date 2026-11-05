// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Compiled components driven by `platypus_components::NativeHost` over the real scene
//! (`bench/components.ts`): `class X extends Component` + `register(X)` and the component
//! lifecycle, no script engine. IDs are in TESTS.md (M9-TS-556..).
use platypus_components::NativeHost;
use platypus_scene::Scene;
use platypus_scene::api::scene_api as api;
use platypus_ts_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use platypus_ts_tests::cmp::*;
use std::sync::Mutex;
use ts_api::Handle;

/// The scripts' globals (`awakes`, `order`) are shared; one test at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn all(_: u64) -> bool {
    true
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { ts_cmp_init() };
    f();
    ts_rt_gc_stress(true);
    unsafe { ts_cmp_init() };
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    unsafe { ts_cmp_init() };
    f();
    ts_rt_gc_enable(true);
}

// M9-TS-556
//
// The generated table lists the registered classes in order, and a hook a class does not
// define (Physics has only onFixedUpdate) is absent from it.
#[test]
fn the_bindings_list_the_registered_classes_and_their_hooks() {
    let classes = classes();
    let names: Vec<_> = classes.iter().map(|c| c.name).collect();
    assert_eq!(names, ["Mover", "Fast", "Faulty", "Physics"]);
    // The table follows the host's hook list (`platypus_components::HOOK_LIST`): the nine
    // lifecycle hooks, then the six contact hooks, which none of these classes has.
    assert!(
        classes
            .iter()
            .all(|c| c.hooks.len() == 15 && c.hooks[9..].iter().all(Option::is_none))
    );
    let have =
        |c: usize| -> Vec<bool> { classes[c].hooks[..9].iter().map(|h| h.is_some()).collect() };
    // onAwake onStart onUpdate onLateUpdate onFixedUpdate onPreRender onDestroy onApplicationFocus onApplicationPause
    assert_eq!(
        have(0),
        [true, true, true, true, false, false, true, false, false]
    );
    assert_eq!(have(1), have(0), "a subclass has the inherited hooks");
    assert_eq!(
        have(3),
        [false, false, false, false, true, false, false, false, false]
    );
}

// M9-TS-557
//
// A component moves its own GameObject: onStart once, then onUpdate and onLateUpdate each frame,
// `this.gameObject` is the object it was spawned on and fields persist between frames.
#[test]
fn a_component_updates_its_game_object_every_frame() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| {
        let go = api::create();
        let mut host = NativeHost::new(classes());
        let id = host.spawn("Mover", Some(go.to_raw())).unwrap();
        assert_eq!(unsafe { ts_cmp_awakeCount() }, 1.0 + 0.0 * id as f64);
        for _ in 0..3 {
            assert!(host.update(0.5, &all).is_empty());
        }
        // speed 2 * dt 0.5 * 3 frames
        let x = api::local_position(go).x;
        assert_eq!(x, 3.0);
        host.destroy(id);
        api::destroy(go);
        assert!(host.is_empty());
    });
}

fn order() -> f64 {
    unsafe { ts_cmp_hookOrder() }
}

// M9-TS-558
//
// `onStart` runs once, before the first update; every component's onUpdate runs before any
// onLateUpdate: two components give s u s u l l (1=start, 2=update, 3=late), then u u l l.
#[test]
fn hooks_run_in_the_hosts_order_and_start_runs_once() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| {
        let a = api::create();
        let b = api::create();
        let mut host = NativeHost::new(classes());
        host.spawn("Mover", Some(a.to_raw())).unwrap();
        host.spawn("Mover", Some(b.to_raw())).unwrap();
        host.update(0.1, &all);
        assert_eq!(order(), 121_233.0);
        host.update(0.1, &all);
        assert_eq!(order(), 1_212_332_233.0);
        api::destroy(a);
        api::destroy(b);
    });
}

// M9-TS-559
//
// A subclass inherits the hooks and may override one (Fast runs its update twice); a component
// whose object is inactive is skipped; destroy runs onDestroy once.
#[test]
fn subclasses_inherit_hooks_and_inactive_objects_are_skipped() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| {
        let a = api::create();
        let b = api::create();
        let mut host = NativeHost::new(classes());
        let fast = host.spawn("Fast", Some(a.to_raw())).unwrap();
        host.spawn("Mover", Some(b.to_raw())).unwrap();
        let off = b.to_raw();
        host.update(1.0, &|o| o != off);
        assert_eq!(api::local_position(a).x, 4.0);
        assert_eq!(api::local_position(b).x, 0.0);
        host.destroy(fast);
        assert_eq!(unsafe { ts_cmp_destroyCount() }, 1.0);
        host.destroy_on_objects(&[off]);
        assert_eq!(unsafe { ts_cmp_destroyCount() }, 2.0);
        assert!(host.is_empty());
        api::destroy(a);
        api::destroy(b);
    });
}

// M9-TS-560
//
// A component that throws is reported and disabled; the others keep running, and an unknown
// class is an error.
#[test]
fn a_throwing_component_is_disabled_and_the_others_run() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| {
        let a = api::create();
        let mut host = NativeHost::new(classes());
        let bad = host.spawn("Faulty", None).unwrap();
        host.spawn("Mover", Some(a.to_raw())).unwrap();
        let faults = host.update(1.0, &all);
        assert_eq!(faults.len(), 1);
        assert_eq!((faults[0].class, faults[0].hook), ("Faulty", "onUpdate"));
        assert!(!host.is_enabled(bad));
        assert!(host.update(1.0, &all).is_empty());
        assert_eq!(api::local_position(a).x, 4.0);
        assert!(host.spawn("Nope", None).is_err());
        api::destroy(a);
    });
}

// M9-TS-561
//
// Fixed ticks call only the classes that have onFixedUpdate.
#[test]
fn fixed_update_reaches_only_classes_with_the_hook() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| {
        let a = api::create();
        let mut host = NativeHost::new(classes());
        host.spawn("Physics", Some(a.to_raw())).unwrap();
        for _ in 0..4 {
            assert!(host.fixed_update(0.25, &all).is_empty());
        }
        assert_eq!(api::local_position(a).y, 1.0);
        api::destroy(a);
    });
}
