// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `@property` save and load, `getComponent` and `addComponent` for compiled components driven by
//! `NativeHost` (`bench/components2.ts`). IDs are in TESTS.md (M9-TS-601..).
use platypus_components::{NativeHost, PropValue};
use platypus_scene::Scene;
use platypus_scene::api::scene_api as api;
use platypus_ts_rt::{link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use platypus_ts_tests::cm2::*;
use ts_api::Handle;

fn all(_: u64) -> bool {
    true
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    unsafe { ts_cm2_init() };
    f();
    ts_rt_gc_stress(true);
    unsafe { ts_cm2_init() };
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    unsafe { ts_cm2_init() };
    f();
    ts_rt_gc_enable(true);
}

fn log() -> String {
    let p = unsafe { ts_cm2_logText() };
    String::from_utf16_lossy(
        &unsafe { platypus_ts_rt::Str::from_raw(p as *const platypus_ts_rt::Str) }.to_utf16(),
    )
}

// M9-TS-601
//
// The class table carries each class's reflection table: Health has five properties (the field
// `hits` is not one), Brain has none.
#[test]
fn the_host_lists_the_properties_of_a_component() {
    in_every_gc_mode(|| {
        let mut host = NativeHost::new(classes());
        let h = host.spawn("Health", None).unwrap();
        let b = host.spawn("Brain", None).unwrap();
        assert_eq!(
            host.properties(h).unwrap(),
            [
                ("hp", "number"),
                ("armor", "number?"),
                ("alive", "boolean"),
                ("label", "string"),
                ("buddy", "object:Health?"),
            ]
        );
        assert!(host.properties(b).unwrap().is_empty());
    });
}

// M9-TS-602
//
// A property is read and written through the table with no script running: numbers, a nullable
// number (null and a value), a boolean and a string; a value of the wrong kind is an error that
// names the property and changes nothing.
#[test]
fn properties_are_read_and_written_through_the_table() {
    in_every_gc_mode(|| {
        let mut host = NativeHost::new(classes());
        let h = host.spawn("Health", None).unwrap();
        assert_eq!(host.get_property(h, "hp"), Ok(PropValue::Num(10.0)));
        assert_eq!(host.get_property(h, "armor"), Ok(PropValue::Null));
        assert_eq!(host.get_property(h, "alive"), Ok(PropValue::Bool(true)));
        assert_eq!(
            host.get_property(h, "label"),
            Ok(PropValue::Str("hero".into()))
        );
        assert_eq!(host.get_property(h, "buddy"), Ok(PropValue::Null));
        host.set_property(h, "hp", PropValue::Num(42.5)).unwrap();
        host.set_property(h, "armor", PropValue::Num(0.0)).unwrap();
        host.set_property(h, "alive", PropValue::Bool(false))
            .unwrap();
        host.set_property(h, "label", PropValue::Str("héros 🛡".into()))
            .unwrap();
        // A collection between the write and the read must keep the new string alive.
        platypus_ts_rt::ts_rt_gc_collect();
        assert_eq!(host.get_property(h, "hp"), Ok(PropValue::Num(42.5)));
        assert_eq!(host.get_property(h, "armor"), Ok(PropValue::Num(0.0)));
        assert_eq!(host.get_property(h, "alive"), Ok(PropValue::Bool(false)));
        assert_eq!(
            host.get_property(h, "label"),
            Ok(PropValue::Str("héros 🛡".into()))
        );
        host.set_property(h, "armor", PropValue::Null).unwrap();
        assert_eq!(host.get_property(h, "armor"), Ok(PropValue::Null));
        let e = host
            .set_property(h, "hp", PropValue::Str("x".into()))
            .unwrap_err();
        assert!(e.contains("hp") && e.contains("number"), "{e}");
        let e = host.set_property(h, "hp", PropValue::Null).unwrap_err();
        assert!(e.contains("hp"), "{e}");
        assert_eq!(host.get_property(h, "hp"), Ok(PropValue::Num(42.5)));
        let e = host.get_property(h, "hits").unwrap_err();
        assert!(e.contains("no property"), "{e}");
    });
}

// M9-TS-603
//
// Save and load: the values of one component applied to another make them equal, and a component
// reference (`buddy`) is stored as the other component's id and resolved on load.
#[test]
fn saved_properties_load_into_another_component() {
    in_every_gc_mode(|| {
        let mut host = NativeHost::new(classes());
        let a = host.spawn("Health", None).unwrap();
        let b = host.spawn("Health", None).unwrap();
        host.set_property(a, "hp", PropValue::Num(7.0)).unwrap();
        host.set_property(a, "label", PropValue::Str("saved".into()))
            .unwrap();
        host.set_property(a, "buddy", PropValue::Component(b))
            .unwrap();
        let saved = host.save_properties(a).unwrap();
        assert_eq!(saved.len(), 5);
        assert_eq!(saved[4], ("buddy", PropValue::Component(b)));
        let c = host.spawn("Health", None).unwrap();
        let values: Vec<(&str, PropValue)> = saved.iter().map(|(n, v)| (*n, v.clone())).collect();
        assert!(host.load_properties(c, &values).is_empty());
        assert_eq!(host.save_properties(c).unwrap(), saved);
        // A reference to a component of another class, or to none, is an error; the rest load.
        let brain = host.spawn("Brain", None).unwrap();
        let errs = host.load_properties(
            c,
            &[
                ("hp", PropValue::Num(1.0)),
                ("buddy", PropValue::Component(brain)),
                ("ghost", PropValue::Num(0.0)),
            ],
        );
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert_eq!(host.get_property(c, "hp"), Ok(PropValue::Num(1.0)));
    });
}

// M9-TS-604
//
// `this.getComponent(Health)` finds a sibling on the same object, `addComponent(Health, { .. })`
// makes another with the options applied and its `onAwake` run, and `getComponent` on an object
// without that class is `undefined`.
#[test]
fn get_component_and_add_component_work_between_compiled_components() {
    let mut scene = Scene::new();
    let _scope = platypus_scene::api::enter(&mut scene);
    in_every_gc_mode(|| {
        let go = api::create();
        let other = api::create();
        let mut host = NativeHost::new(classes());
        let h = host.spawn("Health", Some(go.to_raw())).unwrap();
        host.spawn("Brain", Some(go.to_raw())).unwrap();
        host.spawn("Lonely", Some(other.to_raw())).unwrap();
        assert_eq!(unsafe { ts_cm2_awakeCount() }, 1.0);
        assert!(host.update(0.1, &all).is_empty());
        // The brain hit the sibling (10 - 3), then added a second Health with its options.
        assert_eq!(host.get_property(h, "hp"), Ok(PropValue::Num(7.0)));
        assert_eq!(host.len(), 4);
        assert_eq!(unsafe { ts_cm2_awakeCount() }, 2.0);
        let added = *host.ids().last().unwrap();
        assert_eq!(host.get_property(added, "hp"), Ok(PropValue::Num(99.0)));
        assert_eq!(
            host.get_property(added, "label"),
            Ok(PropValue::Str("added".into()))
        );
        assert_eq!(log(), "found7,added99added,selfnone");
        // The added component is a full citizen: updated, found, destroyed with its object.
        assert!(host.update(0.1, &all).is_empty());
        host.destroy_on_objects(&[go.to_raw()]);
        assert_eq!(host.len(), 1);
        api::destroy(go);
        api::destroy(other);
    });
}
