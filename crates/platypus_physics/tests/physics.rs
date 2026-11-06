// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use platypus_core::Value;
use platypus_physics::{Contact, Physics};

fn num(v: f64) -> Value {
    Value::Number(v)
}

fn arr(items: &[f64]) -> Value {
    Value::Array(items.iter().copied().map(num).collect())
}

fn op(p: &mut Physics, name: &str, args: Vec<Value>) {
    p.apply(name, &args)
        .unwrap_or_else(|e| panic!("{name}: {e}"));
}

/// A fixed 20×1×20 floor whose top is at y = 0, collider 1 on body 1.
fn floor(p: &mut Physics) {
    op(
        p,
        "body",
        vec![num(1.0), "fixed".into(), arr(&[0.0, -0.5, 0.0])],
    );
    op(
        p,
        "collider",
        vec![num(1.0), num(1.0), "box".into(), arr(&[10.0, 0.5, 10.0])],
    );
}

fn steps(p: &mut Physics, n: usize) {
    for _ in 0..n {
        p.step();
    }
}

// M4-PHY-01
#[test]
fn dropped_box_comes_to_rest_on_the_floor() {
    let mut p = Physics::new();
    floor(&mut p);
    op(
        &mut p,
        "body",
        vec![num(2.0), "dynamic".into(), arr(&[0.0, 3.0, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![num(2.0), num(2.0), "box".into(), arr(&[0.5, 0.5, 0.5])],
    );
    steps(&mut p, 240);
    let [x, y, z] = p.position(2).unwrap();
    // Half its height above the floor's top, give or take the contact skin.
    assert!((y - 0.5).abs() < 0.02, "y = {y}");
    assert!(x.abs() < 0.02 && z.abs() < 0.02, "drifted to {x}, {z}");

    // Moving bodies are reported each tick; the fixed floor never is.
    let batch = p.take_batch();
    assert!(batch.bodies.iter().all(|b| b.id == 2));
    assert!(!batch.bodies.is_empty());
}

// M4-PHY-02
#[test]
fn collisions_start_and_stop_with_script_ids() {
    let mut p = Physics::new();
    floor(&mut p);
    // A ball dropped through a sensor (a trigger volume) onto the floor.
    op(
        &mut p,
        "body",
        vec![num(3.0), "fixed".into(), arr(&[0.0, 2.0, 0.0])],
    );
    let sensor_opts = Value::Array(vec![
        num(0.5),
        num(0.0),
        num(1.0),
        Value::Bool(true),
        Value::Bool(true),
    ]);
    op(
        &mut p,
        "collider",
        vec![
            num(30.0),
            num(3.0),
            "box".into(),
            arr(&[1.0, 0.25, 1.0]),
            arr(&[0.0, 0.0, 0.0]),
            arr(&[0.0, 0.0, 0.0, 1.0]),
            sensor_opts,
        ],
    );
    op(
        &mut p,
        "body",
        vec![num(2.0), "dynamic".into(), arr(&[0.0, 4.0, 0.0])],
    );
    let events = Value::Array(vec![
        num(0.5),
        num(0.0),
        num(1.0),
        Value::Bool(false),
        Value::Bool(true),
    ]);
    op(
        &mut p,
        "collider",
        vec![
            num(20.0),
            num(2.0),
            "ball".into(),
            arr(&[0.25]),
            Value::Null,
            Value::Null,
            events,
        ],
    );
    steps(&mut p, 180);
    let contacts = p.take_batch().contacts;
    let pair = |c: &Contact, a, b| (c.a, c.b) == (a, b) || (c.a, c.b) == (b, a);
    let sensor: Vec<_> = contacts.iter().filter(|c| pair(c, 20, 30)).collect();
    assert_eq!(sensor.len(), 2, "enter and exit the trigger: {contacts:?}");
    assert!(sensor[0].started && !sensor[1].started && sensor[0].sensor);
    let floor_hits: Vec<_> = contacts.iter().filter(|c| pair(c, 20, 1)).collect();
    assert!(
        floor_hits.first().is_some_and(|c| c.started && !c.sensor),
        "{contacts:?}"
    );
    // Dropped from 4 m to 0.25 m: about sqrt(2 g h) = 8.6 m/s going in.
    let speed = floor_hits[0].speed;
    assert!((speed - 8.6).abs() < 0.4, "impact speed {speed}");

    // Removing the ball ends its contact with the floor.
    op(&mut p, "removeBody", vec![num(2.0)]);
    assert_eq!(p.collider_count(), 2);
    steps(&mut p, 1);
    let after = p.take_batch().contacts;
    assert!(after.iter().all(|c| !c.started), "{after:?}");
}

// M4-PHY-03
#[test]
fn character_controller_climbs_a_step_and_stops_at_a_wall() {
    let mut p = Physics::new();
    floor(&mut p);
    // A 0.2 m step from x = 1, and a wall at x = 4.
    op(
        &mut p,
        "body",
        vec![num(5.0), "fixed".into(), arr(&[2.5, 0.1, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![num(5.0), num(5.0), "box".into(), arr(&[1.5, 0.1, 2.0])],
    );
    op(
        &mut p,
        "body",
        vec![num(6.0), "fixed".into(), arr(&[4.5, 1.5, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![num(6.0), num(6.0), "box".into(), arr(&[0.5, 1.5, 2.0])],
    );
    // A capsule character standing at the origin: 0.5 half height + 0.3 radius.
    op(
        &mut p,
        "body",
        vec![num(7.0), "kinematic".into(), arr(&[0.0, 0.8, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![num(7.0), num(7.0), "capsule".into(), arr(&[0.5, 0.3])],
    );
    // 45° slopes, snap 0.2 m down, step up to 0.3 m.
    op(
        &mut p,
        "character",
        vec![num(7.0), num(45.0), num(0.2), num(0.3)],
    );

    let mut grounded = false;
    for _ in 0..300 {
        // 3 m/s forward, with a small downward pull so it stays grounded (a pull as
        // steep as the forward move makes rapier treat the step edge as a slope).
        op(&mut p, "move", vec![num(7.0), arr(&[0.05, -0.01, 0.0])]);
        p.step();
        grounded = p.take_batch().characters.last().is_some_and(|c| c.1);
    }
    let [x, y, _] = p.position(7).unwrap();
    assert!(grounded);
    // On top of the step (0.2 + 0.8 above it), pressed against the wall (x = 4 - 0.3).
    assert!((y - 1.0).abs() < 0.05, "x = {x}, y = {y}");
    assert!(x > 3.5 && x < 3.71, "x = {x}");
}

// M4-PHY-05
#[test]
fn raycasts_answer_after_the_step() {
    let mut p = Physics::new();
    floor(&mut p);
    op(
        &mut p,
        "raycast",
        vec![
            num(9.0),
            arr(&[0.0, 5.0, 0.0]),
            arr(&[0.0, -2.0, 0.0]),
            num(100.0),
        ],
    );
    op(
        &mut p,
        "raycast",
        vec![
            num(10.0),
            arr(&[0.0, 5.0, 0.0]),
            arr(&[0.0, 1.0, 0.0]),
            num(100.0),
        ],
    );
    p.step();
    let rays = p.take_batch().rays;
    assert_eq!(rays.len(), 2);
    let (collider, dist, normal) = rays[0].hit.unwrap();
    assert_eq!((rays[0].query, collider), (9, 1));
    assert!((dist - 5.0).abs() < 1e-3 && (normal[1] - 1.0).abs() < 1e-3);
    assert_eq!((rays[1].query, rays[1].hit), (10, None));

    // Flat on the wire: query, collider (-1: miss), distance, normal.
    let args = platypus_physics::Batch {
        rays,
        ..Default::default()
    }
    .to_args();
    let Value::Array(flat) = &args[2] else {
        panic!()
    };
    assert_eq!(flat.len(), 12);
    assert_eq!(flat[7], num(-1.0));
}

// M4-PHY-06
#[test]
fn bad_ops_are_rejected_and_meshes_collide() {
    let mut p = Physics::new();
    for (name, args) in [
        ("body", vec![num(1.0), "floating".into()]),
        (
            "collider",
            vec![num(1.0), num(99.0), "box".into(), arr(&[1.0, 1.0, 1.0])],
        ),
        (
            "collider",
            vec![num(1.0), Value::Null, "blob".into(), arr(&[1.0])],
        ),
        (
            "set",
            vec![num(42.0), "position".into(), arr(&[0.0, 0.0, 0.0])],
        ),
        (
            "raycast",
            vec![
                num(1.0),
                arr(&[0.0, 0.0, 0.0]),
                arr(&[0.0, 0.0, 0.0]),
                num(1.0),
            ],
        ),
        ("gravity", vec![arr(&[0.0, f64::NAN, 0.0])]),
        ("warp", vec![]),
    ] {
        assert!(p.apply(name, &args).is_err(), "{name} {args:?} should fail");
    }
    assert_eq!((p.body_count(), p.collider_count()), (0, 0));

    // A triangle-mesh floor (two triangles) catches a ball.
    let points = arr(&[
        -5.0, 0.0, -5.0, 5.0, 0.0, -5.0, 5.0, 0.0, 5.0, -5.0, 0.0, 5.0,
    ]);
    let bad = Value::Array(vec![
        num(0.5),
        num(0.0),
        num(1.0),
        Value::Bool(false),
        Value::Bool(false),
        arr(&[0.0, 1.0, 9.0]),
    ]);
    let mesh_args = |opts| {
        vec![
            num(1.0),
            Value::Null,
            "mesh".into(),
            points.clone(),
            Value::Null,
            Value::Null,
            opts,
        ]
    };
    assert!(p.apply("collider", &mesh_args(bad)).is_err());
    let good = Value::Array(vec![
        num(0.5),
        num(0.0),
        num(1.0),
        Value::Bool(false),
        Value::Bool(false),
        arr(&[0.0, 2.0, 1.0, 0.0, 3.0, 2.0]),
    ]);
    op(&mut p, "collider", mesh_args(good));
    op(
        &mut p,
        "body",
        vec![num(2.0), "dynamic".into(), arr(&[0.0, 2.0, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![num(2.0), num(2.0), "ball".into(), arr(&[0.5])],
    );
    steps(&mut p, 240);
    let y = p.position(2).unwrap()[1];
    assert!((y - 0.5).abs() < 0.05, "y = {y}");
}

// M5-JNT-01
#[test]
fn joints_hold_bodies_together() {
    let mut p = Physics::new();
    let ball = |p: &mut Physics, id: u32, at: &[f64; 3]| {
        op(p, "body", vec![num(id as f64), "dynamic".into(), arr(at)]);
        op(
            p,
            "collider",
            vec![num(id as f64), num(id as f64), "ball".into(), arr(&[0.2])],
        );
    };
    // A rope 2 m long from a world anchor at (0, 5, 0): the ball starts 1 m out and
    // falls until the rope is taut.
    ball(&mut p, 1, &[1.0, 5.0, 0.0]);
    op(
        &mut p,
        "joint",
        vec![
            num(1.0),
            "rope".into(),
            num(1.0),
            Value::Null,
            arr(&[0.0, 0.0, 0.0]),
            arr(&[0.0, 5.0, 0.0]),
            arr(&[2.0]),
        ],
    );
    // A pendulum on a ball joint, 1.5 m below its anchor.
    ball(&mut p, 2, &[5.0, 3.5, 0.0]);
    op(
        &mut p,
        "joint",
        vec![
            num(2.0),
            "ball".into(),
            num(2.0),
            Value::Null,
            arr(&[0.0, 1.5, 0.0]),
            arr(&[5.0, 5.0, 0.0]),
        ],
    );
    // A wheel on a motorised hinge about z, turning at 90°/s.
    op(
        &mut p,
        "body",
        vec![num(3.0), "dynamic".into(), arr(&[-5.0, 5.0, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![num(3.0), num(3.0), "box".into(), arr(&[0.5, 0.1, 0.1])],
    );
    op(
        &mut p,
        "set",
        vec![num(3.0), "gravityScale".into(), num(0.0)],
    );
    op(
        &mut p,
        "joint",
        vec![
            num(3.0),
            "hinge".into(),
            num(3.0),
            Value::Null,
            arr(&[0.0, 0.0, 0.0]),
            arr(&[-5.0, 5.0, 0.0]),
            arr(&[0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 90.0, 10.0]),
        ],
    );
    // Two bodies on a stiff spring with a 1 m rest length, in zero gravity.
    for (id, x) in [(4.0, 10.0), (5.0, 13.0)] {
        ball(&mut p, id as u32, &[x, 5.0, 0.0]);
        op(
            &mut p,
            "set",
            vec![num(id), "gravityScale".into(), num(0.0)],
        );
        op(
            &mut p,
            "set",
            vec![num(id), "linearDamping".into(), num(2.0)],
        );
    }
    op(
        &mut p,
        "joint",
        vec![
            num(4.0),
            "spring".into(),
            num(4.0),
            num(5.0),
            Value::Null,
            Value::Null,
            arr(&[1.0, 200.0, 5.0]),
        ],
    );
    assert_eq!(p.joint_count(), 4);
    assert!(
        p.apply("joint", &[num(9.0), "weld".into(), num(1.0), Value::Null])
            .is_err()
    );
    assert!(
        p.apply("joint", &[num(9.0), "rope".into(), num(99.0), Value::Null])
            .is_err()
    );

    steps(&mut p, 240);
    let dist = |a: [f32; 3], b: [f32; 3]| {
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
    };
    let rope = dist(p.position(1).unwrap(), [0.0, 5.0, 0.0]);
    assert!(rope <= 2.05 && rope > 1.5, "rope length {rope}");
    let pendulum = dist(p.position(2).unwrap(), [5.0, 5.0, 0.0]);
    assert!((pendulum - 1.5).abs() < 0.05, "pendulum length {pendulum}");
    let (_, q) = p.pose(3).unwrap();
    let angle = 2.0 * q[3].clamp(-1.0, 1.0).acos();
    assert!(angle > 0.5, "the motor turned the wheel: {angle} rad");
    let spring = dist(p.position(4).unwrap(), p.position(5).unwrap());
    assert!((spring - 1.0).abs() < 0.2, "spring settled at {spring}");

    op(&mut p, "removeJoint", vec![num(1.0)]);
    assert_eq!(p.joint_count(), 3);
}

/// How far a box resting on the floor slides after a 3 m/s shove along x, with these
/// `[x, y, z]` movements and turns allowed.
fn slide_after_shove(translations: [bool; 3], rotations: [bool; 3]) -> f32 {
    let mut p = Physics::new();
    floor(&mut p);
    op(
        &mut p,
        "body",
        vec![num(2.0), "dynamic".into(), arr(&[0.0, 0.5, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![num(2.0), num(2.0), "box".into(), arr(&[0.5, 0.5, 0.5])],
    );
    let allowed = translations
        .iter()
        .chain(&rotations)
        .map(|&a| Value::Bool(a));
    op(
        &mut p,
        "set",
        vec![
            num(2.0),
            "constraints".into(),
            Value::Array(allowed.collect()),
        ],
    );
    steps(&mut p, 30);
    op(
        &mut p,
        "set",
        vec![num(2.0), "velocity".into(), arr(&[3.0, 0.0, 0.0])],
    );
    steps(&mut p, 180);
    p.position(2).unwrap()[0]
}

// M4-PHY-07
#[test]
fn friction_stops_a_box_locked_to_a_plane() {
    // Unlocked, friction stops it in about a metre (v² / 2μg with μ = 0.5).
    let free = slide_after_shove([true; 3], [true; 3]);
    assert!(free < 2.0, "a free box slid {free} m");
    // Kept in the x-y plane (no z movement, turning only about z) it must stop the same way;
    // rapier's coupled friction solve used to give up on the zero-mass z direction.
    let planar = slide_after_shove([true, true, false], [false, false, true]);
    assert!(
        planar < 2.0,
        "a plane-locked box slid {planar} m, as if on ice"
    );
    // Locking just the tilts, or just z movement, keeps friction too.
    for (t, r) in [
        ([true; 3], [false, false, true]),
        ([true, true, false], [true; 3]),
    ] {
        let slid = slide_after_shove(t, r);
        assert!(slid < 2.0, "locks {t:?} {r:?}: slid {slid} m");
    }

    // The plane lock still holds (Platypus pins z itself for this combination): shoved along z,
    // moved by `position`, the box stays in its plane.
    let mut p = Physics::new();
    floor(&mut p);
    op(
        &mut p,
        "body",
        vec![num(2.0), "dynamic".into(), arr(&[0.0, 0.5, 0.25])],
    );
    op(
        &mut p,
        "collider",
        vec![num(2.0), num(2.0), "box".into(), arr(&[0.5, 0.5, 0.5])],
    );
    let plane = [true, true, false, false, false, true].map(Value::Bool);
    op(
        &mut p,
        "set",
        vec![num(2.0), "constraints".into(), Value::Array(plane.to_vec())],
    );
    op(
        &mut p,
        "set",
        vec![num(2.0), "velocity".into(), arr(&[1.0, 0.0, 5.0])],
    );
    steps(&mut p, 60);
    let [_, _, z] = p.position(2).unwrap();
    assert_eq!(z, 0.25, "left its plane");
    op(
        &mut p,
        "set",
        vec![num(2.0), "position".into(), arr(&[0.0, 0.5, -1.0])],
    );
    steps(&mut p, 10);
    assert_eq!(p.position(2).unwrap()[2], -1.0, "not held where it was put");
}

// M4-PHY-08
//
// After a step the contact points of a pair are read from either side: a box resting on the floor
// touches it at its corners, at the floor's top, with normals towards the side asking and normal
// impulses that hold up its weight; shoved along x, friction pushes it back. A collision's start
// carries the velocity going in.
#[test]
fn contact_points_are_read_from_either_side() {
    let mut p = Physics::new();
    floor(&mut p);
    op(
        &mut p,
        "body",
        vec![num(2.0), "dynamic".into(), arr(&[0.0, 1.0, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![
            num(2.0),
            num(2.0),
            "box".into(),
            arr(&[0.5, 0.5, 0.5]),
            Value::Null,
            Value::Null,
            // friction, restitution, density, sensor, events
            Value::Array(vec![
                num(0.5),
                num(0.0),
                num(1.0),
                Value::Bool(false),
                Value::Bool(true),
            ]),
        ],
    );
    steps(&mut p, 120);
    let start = p
        .take_batch()
        .contacts
        .into_iter()
        .find(|c| c.started)
        .expect("the box lands");
    // b's velocity minus a's: the box (2) was falling at about sqrt(2 g 0.5) = 3.1 m/s.
    let v = if start.b == 2 {
        -start.velocity[1]
    } else {
        start.velocity[1]
    };
    assert!((v - 3.1).abs() < 0.3, "{start:?}");
    assert!((start.speed - v.abs()).abs() < 1e-4);

    let mut out = Vec::new();
    p.contact_points(2, 1, &mut out);
    assert!(out.len() >= 4, "{out:?}");
    let mut total = 0.0;
    for c in &out {
        assert!(c.point[1].abs() < 0.05, "{c:?}");
        assert!(c.point[0].abs() < 0.55 && c.point[2].abs() < 0.55, "{c:?}");
        assert!((c.normal[1] - 1.0).abs() < 1e-3, "towards the box: {c:?}");
        assert!(c.separation < 0.05, "{c:?}");
        total += c.normal_impulse;
    }
    // A 1 kg box: m g dt = 9.81 / 60 N·s a step.
    assert!((total - 9.81 / 60.0).abs() < 0.03, "{total}");
    let mut floor_side = Vec::new();
    p.contact_points(1, 2, &mut floor_side);
    assert_eq!(floor_side.len(), out.len());
    assert!(floor_side.iter().all(|c| (c.normal[1] + 1.0).abs() < 1e-3));

    // Sliding along +x: the friction on the box points back along -x, the floor moves at -x
    // relative to it.
    op(
        &mut p,
        "set",
        vec![num(2.0), "velocity".into(), arr(&[3.0, 0.0, 0.0])],
    );
    steps(&mut p, 2);
    out.clear();
    p.contact_points(2, 1, &mut out);
    let fx: f32 = out.iter().map(|c| c.tangent_impulse[0]).sum();
    assert!(fx < 0.0, "{out:?}");
    assert!(out.iter().all(|c| c.relative_velocity[0] < 0.0), "{out:?}");

    // No pair, no points.
    out.clear();
    p.contact_points(2, 99, &mut out);
    assert!(out.is_empty());
}

// RDY-PHY-01
#[test]
fn ignored_collider_pairs_do_not_collide() {
    let drop = |ignore: bool| {
        let mut p = Physics::new();
        floor(&mut p);
        op(
            &mut p,
            "body",
            vec![num(2.0), "dynamic".into(), arr(&[0.0, 3.0, 0.0])],
        );
        op(
            &mut p,
            "collider",
            vec![num(2.0), num(2.0), "box".into(), arr(&[0.5, 0.5, 0.5])],
        );
        if ignore {
            op(&mut p, "ignore", vec![num(2.0), num(1.0)]);
        }
        steps(&mut p, 120);
        p.position(2).unwrap()[1]
    };
    assert!((drop(false) - 0.5).abs() < 0.05, "rests on the floor");
    assert!(drop(true) < -2.0, "falls through an ignored floor");

    // Switching it off again restores the contact (the box is rebuilt mid-air).
    let mut p = Physics::new();
    floor(&mut p);
    op(
        &mut p,
        "body",
        vec![num(2.0), "dynamic".into(), arr(&[0.0, 3.0, 0.0])],
    );
    let make = |p: &mut Physics| {
        op(
            p,
            "collider",
            vec![num(2.0), num(2.0), "box".into(), arr(&[0.5, 0.5, 0.5])],
        )
    };
    make(&mut p);
    op(&mut p, "ignore", vec![num(1.0), num(2.0)]);
    make(&mut p); // a rebuilt collider keeps being ignored
    steps(&mut p, 120);
    assert!(p.position(2).unwrap()[1] < -2.0);
    op(
        &mut p,
        "ignore",
        vec![num(1.0), num(2.0), Value::Bool(false)],
    );
    op(
        &mut p,
        "set",
        vec![num(2.0), "position".into(), arr(&[0.0, 3.0, 0.0])],
    );
    steps(&mut p, 240);
    assert!((p.position(2).unwrap()[1] - 0.5).abs() < 0.05);
}

// M4-PHY-09
#[test]
fn a_body_asleep_without_gravity_falls_when_gravity_returns() {
    let mut p = Physics::new();
    floor(&mut p);
    op(
        &mut p,
        "body",
        vec![num(2.0), "dynamic".into(), arr(&[0.0, 3.0, 0.0])],
    );
    op(
        &mut p,
        "collider",
        vec![num(2.0), num(2.0), "ball".into(), arr(&[0.25])],
    );
    op(
        &mut p,
        "set",
        vec![num(2.0), "gravityScale".into(), num(0.0)],
    );
    // Long enough to fall asleep floating.
    steps(&mut p, 600);
    assert_eq!(p.position(2).unwrap()[1], 3.0);
    op(
        &mut p,
        "set",
        vec![num(2.0), "gravityScale".into(), num(1.0)],
    );
    steps(&mut p, 240);
    assert!((p.position(2).unwrap()[1] - 0.25).abs() < 0.05);
}

// RDY-PHY-05
#[test]
fn a_collider_less_hinged_chain_swings_when_pushed() {
    let swing = |limits: Option<(f64, f64)>| {
        let mut p = Physics::new();
        op(
            &mut p,
            "body",
            vec![num(1.0), "fixed".into(), arr(&[0.0, 0.0, 0.0])],
        );
        op(
            &mut p,
            "body",
            vec![num(2.0), "dynamic".into(), arr(&[0.0, -1.5, 0.0])],
        );
        // No collider: its mass and inertia come from the box it stands for.
        op(
            &mut p,
            "set",
            vec![num(2.0), "mass".into(), num(1.0), arr(&[0.05, 3.0, 1.0])],
        );
        let mut params = vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        if let Some((lo, hi)) = limits {
            params[3] = 1.0;
            params[4] = lo;
            params[5] = hi;
        }
        op(
            &mut p,
            "joint",
            vec![
                num(1.0),
                "hinge".into(),
                num(1.0),
                num(2.0),
                arr(&[0.0, 0.0, 0.0]),
                arr(&[0.0, 1.5, 0.0]),
                arr(&params),
            ],
        );
        op(
            &mut p,
            "force",
            vec![num(2.0), "impulse".into(), arr(&[8.0, 0.0, 0.0])],
        );
        let mut far = 0.0f32;
        for _ in 0..90 {
            p.step();
            far = far.max(p.position(2).unwrap()[0]);
        }
        far
    };
    let free = swing(None);
    let limited = swing(Some((-45.0, 225.0)));
    assert!(free > 0.8, "a free chain swung out {free} m");
    assert!(
        limited > 0.8,
        "a chain limited to -45..225 degrees swung out {limited} m"
    );
}
