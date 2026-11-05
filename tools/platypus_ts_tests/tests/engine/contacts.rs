// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Contact hooks on compiled components (`bench/contacts.ts`): the physics step queues contacts
//! and `NativeHost::dispatch_physics` calls `onCollisionEnter` and friends by name. IDs are in
//! TESTS.md (M9-TS-1000..).
use platypus_components::{ContactEvent, NativeHost};
use platypus_scene::Scene;
use platypus_scene::api::scene_api as api;
use platypus_ts_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};
use platypus_ts_tests::contacts_t::*;
use ts_api::Handle;

fn all(_: u64) -> bool {
    true
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    let run = || {
        platypus_physics_api::reset();
        unsafe { ts_cnt_init() };
        f();
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
}

fn log() -> String {
    let p = unsafe { ts_cnt_logText() };
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

fn named(name: &str) -> platypus_scene::ObjectId {
    let go = api::create();
    go.set_name(name);
    go
}

// M9-TS-1000
//
// A ball dropped on a floor tells the components of both objects `onCollisionEnter` with the other
// object and the impact speed; both sides get it, and a component without the hook is skipped.
#[test]
fn a_collision_tells_both_objects() {
    in_every_gc_mode(|| unsafe {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        ts_cnt_clearLog();
        let floor = named("floor");
        let ball = named("ball");
        host.spawn("Probe", Some(floor.to_raw())).unwrap();
        host.spawn("Probe", Some(ball.to_raw())).unwrap();
        ts_cnt_build(floor.to_raw(), ball.to_raw(), 1.5, false);
        let mut enters = String::new();
        for _ in 0..60 {
            ts_cnt_steps(1.0);
            assert!(host.dispatch_physics(&all).is_empty());
            let l = log();
            if l.contains("E(") {
                enters = l;
                break;
            }
        }
        assert!(enters.contains("floor:E(ball,fast)"), "{enters}");
        assert!(enters.contains("ball:E(floor,fast)"), "{enters}");
    });
}

// M9-TS-1001
//
// `onCollisionStay` runs on each dispatch while the contact lasts, and `onCollisionExit` when it
// ends (here: the ball is lifted away); an inherited hook (a subclass) is called too.
#[test]
fn stay_runs_while_touching_and_exit_when_it_ends() {
    in_every_gc_mode(|| unsafe {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        ts_cnt_clearLog();
        let floor = named("floor");
        let ball = named("ball");
        host.spawn("Child", Some(ball.to_raw())).unwrap();
        ts_cnt_build(floor.to_raw(), ball.to_raw(), 0.9, false);
        for _ in 0..30 {
            ts_cnt_steps(1.0);
            host.dispatch_physics(&all);
        }
        let l = log();
        assert!(l.starts_with("ball:E(floor,"), "{l}");
        assert!(l.matches("ball:S").count() >= 20, "{l}");
        assert!(!l.contains("ball:X"), "{l}");
        ts_cnt_clearLog();
        ts_cnt_lift();
        for _ in 0..3 {
            ts_cnt_steps(1.0);
            host.dispatch_physics(&all);
        }
        let l = log();
        assert!(
            l.starts_with("ball:X(floor,0)"),
            "no contact points on exit: {l}"
        );
        assert_eq!(l.matches("ball:S").count(), 0, "no Stay after Exit: {l}");
    });
}

// M9-TS-1002
//
// A sensor collider reports `onTriggerEnter` (a hook that asks for a `Collision` gets one, without
// contact points) and `onTriggerStay`, not the collision hooks.
#[test]
fn a_sensor_calls_the_trigger_hooks() {
    in_every_gc_mode(|| unsafe {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        ts_cnt_clearLog();
        let floor = named("floor");
        let ball = named("ball");
        host.spawn("Probe", Some(floor.to_raw())).unwrap();
        host.spawn("Probe", Some(ball.to_raw())).unwrap();
        ts_cnt_build(floor.to_raw(), ball.to_raw(), 0.5, true);
        for _ in 0..5 {
            ts_cnt_steps(1.0);
            host.dispatch_physics(&all);
        }
        let l = log();
        assert!(l.contains("ball:TE(floor,0)"), "{l}");
        assert!(l.contains("floor:TE(ball,0)"), "{l}");
        assert!(l.contains("ball:TS"), "{l}");
        assert!(!l.contains(":E(") && !l.contains(":S"), "{l}");
        ts_cnt_clearLog();
        ts_cnt_lift();
        ts_cnt_steps(2.0);
        host.dispatch_physics(&all);
        assert!(log().contains("ball:TX"), "{}", log());
    });
}

// M9-TS-1003
//
// Events can be fed by hand: inactive objects miss them, a hook that destroys its own component
// ends the walk safely, and a hook that throws is reported and disabled.
#[test]
fn events_skip_inactive_objects_and_faults_are_reported() {
    in_every_gc_mode(|| unsafe {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        ts_cnt_clearLog();
        let a = named("a");
        let b = named("b");
        host.spawn("Quitter", Some(a.to_raw())).unwrap();
        let t = host.spawn("Thrower", Some(a.to_raw())).unwrap();
        let ev = |object: u64| ContactEvent {
            started: true,
            object,
            own: 6,
            other: b.to_raw(),
            collider: 7,
            velocity: [0.0, 3.0, 0.0],
            sensor: false,
        };
        let inactive = |_: u64| false;
        assert!(
            host.dispatch_contacts(&[ev(a.to_raw())], &inactive)
                .is_empty()
        );
        assert_eq!(log(), "");
        let faults = host.dispatch_contacts(&[ev(a.to_raw())], &all);
        assert_eq!(log(), "QT");
        assert_eq!(faults.len(), 1);
        assert_eq!(faults[0].hook, "onCollisionEnter");
        assert!(!host.is_enabled(t));
        assert_eq!(host.len(), 1, "the quitter is gone");
    });
}

/// The `key=value` fields of the log entry that starts with `head`.
fn entry(log: &str, head: &str) -> std::collections::HashMap<String, String> {
    let e = log
        .split(';')
        .find(|e| e.starts_with(head))
        .unwrap_or_else(|| panic!("no {head} in {log}"));
    e.split(' ')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn num(f: &std::collections::HashMap<String, String>, k: &str) -> f64 {
    f[k].parse().unwrap_or_else(|_| panic!("{k} = {}", f[k]))
}

/// Steps and dispatches until the log has an entry starting with `head`.
fn until(host: &mut NativeHost, head: &str) -> String {
    for _ in 0..120 {
        unsafe { ts_cnt_steps(1.0) };
        assert!(host.dispatch_physics(&all).is_empty());
        if log().contains(head) {
            return log();
        }
    }
    panic!("no {head}: {}", log())
}

// M9-TS-1004
//
// A collision hook sees Unity's `Collision`: the object hit, its real `Collider` (with `isTrigger`
// and `gameObject`), its `Rigidbody` (none for a static floor), `transform`, `relativeVelocity`
// (the other side minus this one, going in), contact points at the floor's top with normals
// towards the object told, an impulse that pushes it apart, and the same points through
// `contacts` and `getContact(i)`. A trigger hook gets the other `Collider`.
#[test]
fn a_collision_hook_sees_unity_collision_members() {
    in_every_gc_mode(|| unsafe {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        ts_cnt_clearLog();
        let floor = named("floor");
        let ball = named("ball");
        let zone = named("zone");
        host.spawn("Recorder", Some(floor.to_raw())).unwrap();
        host.spawn("Recorder", Some(ball.to_raw())).unwrap();
        ts_cnt_buildComponents(floor.to_raw(), ball.to_raw(), zone.to_raw());
        until(&mut host, "floor:ball");
        let l = log();
        // The ball passed the trigger first: its Collider, a trigger with no Rigidbody, 2 m across.
        assert!(l.contains("ball:T(zone,true,none,2"), "{l}");

        let b = entry(&l, "ball:floor");
        assert_eq!(b["collider"], "floor");
        assert_eq!(b["trigger"], "false");
        assert_eq!(b["rigidbody"], "none");
        assert_eq!(num(&b, "ty"), -0.5);
        // Fallen 1.5 m: about sqrt(2 g h) = 5.4 m/s; the floor comes up at it.
        let vy = num(&b, "vy");
        assert!((vy - 5.4).abs() < 0.5, "{vy}");
        assert!(num(&b, "n") >= 1.0, "{l}");
        assert!(num(&b, "y").abs() < 0.05, "{l}");
        assert!((num(&b, "ny") - 1.0).abs() < 1e-3, "{l}");
        assert!(num(&b, "iy") > 0.0, "{l}");
        assert_eq!(b["same"], "true");

        let f = entry(&l, "floor:ball");
        assert_eq!(f["collider"], "ball");
        assert_eq!(f["rigidbody"], "ball");
        assert!((num(&f, "vy") + vy).abs() < 1e-9, "{l}");
        assert!((num(&f, "ny") + 1.0).abs() < 1e-3, "{l}");
        assert!(num(&f, "iy") < 0.0, "{l}");
        assert_eq!(f["same"], "true");
    });
}

// M9-TS-1005
//
// The 2D hooks see `Collision2D`: the collider hit and this object's own (`otherCollider`), the
// Rigidbody2D on each side (none for the floor), a 2D `relativeVelocity`, contact points with
// normals and normal impulses, `enabled`; a 2D trigger hook gets the other `Collider2D`.
#[test]
fn a_2d_collision_hook_sees_unity_collision2d_members() {
    in_every_gc_mode(|| unsafe {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let mut host = NativeHost::new(classes());
        ts_cnt_clearLog();
        let floor = named("floor");
        let ball = named("ball");
        let zone = named("zone");
        host.spawn("Recorder2D", Some(floor.to_raw())).unwrap();
        host.spawn("Recorder2D", Some(ball.to_raw())).unwrap();
        ts_cnt_buildComponents2D(floor.to_raw(), ball.to_raw(), zone.to_raw());
        until(&mut host, "floor:ball");
        let l = log();
        assert!(l.contains("ball:T2(zone,true)"), "{l}");

        let b = entry(&l, "ball:floor");
        assert_eq!(b["collider"], "floor");
        assert_eq!(b["other"], "ball");
        assert_eq!(b["rigidbody"], "none");
        assert_eq!(b["own"], "ball");
        let vy = num(&b, "vy");
        assert!((vy - 5.4).abs() < 0.5, "{vy}");
        assert!(num(&b, "n") >= 1.0, "{l}");
        assert!(num(&b, "y").abs() < 0.05, "{l}");
        assert!((num(&b, "ny") - 1.0).abs() < 1e-3, "{l}");
        assert!(num(&b, "impulse") > 0.0, "{l}");
        assert_eq!(b["enabled"], "true");

        let f = entry(&l, "floor:ball");
        assert_eq!(f["collider"], "ball");
        assert_eq!(f["other"], "floor");
        assert_eq!(f["rigidbody"], "ball");
        assert_eq!(f["own"], "none");
        assert!((num(&f, "ny") + 1.0).abs() < 1e-3, "{l}");
    });
}

// M9-TS-1006
//
// The concrete 2D colliders name their body `attachedRigidbody`, as `Collider2D` and Unity do: the
// ball's CircleCollider2D gives its own Rigidbody2D, the floor's BoxCollider2D none.
#[test]
fn concrete_2d_colliders_have_attached_rigidbody() {
    in_every_gc_mode(|| unsafe {
        let mut scene = Scene::new();
        let _scope = platypus_scene::api::enter(&mut scene);
        let floor = named("floor");
        let ball = named("ball");
        let zone = named("zone");
        ts_cnt_buildComponents2D(floor.to_raw(), ball.to_raw(), zone.to_raw());
        let p = ts_cnt_attached2D(floor.to_raw(), ball.to_raw());
        let s = String::from_utf16_lossy(&Str::from_raw(p as *const Str).to_utf16());
        assert_eq!(s, "true|ball|none");
    });
}
