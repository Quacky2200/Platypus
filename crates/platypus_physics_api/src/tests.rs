// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Unit tests of the physics components (`M9-API-200..`): they work on `comp.rs` and the kinds
//! directly; the compiled-program tests are in `tools/platypus_ts_tests/tests/engine/physics.rs`.

use crate::comp::{self, ColShape};
use crate::{add_collider, reset, step, take_contacts};
use glam::{Quat, Vec3};
use platypus_math::{V2, V3};
use platypus_scene::{ObjectId, Scene};
use serde_json::{Map, json};
use ts_api::Handle;

fn make(name: &str) -> ObjectId {
    platypus_scene::api::with_scene(|s| s.create(name)).unwrap()
}

fn place(o: ObjectId, x: f32, y: f32, z: f32) {
    platypus_scene::api::with_scene(|s| s.set_world(o, Vec3::new(x, y, z), Quat::IDENTITY));
}

fn pos(o: ObjectId) -> Vec3 {
    platypus_scene::api::with_scene(|s| s.world(o).position).unwrap()
}

fn run(n: usize) {
    for _ in 0..n {
        step();
    }
}

fn floor() {
    add_collider(0, "box", vec![10.0, 0.5, 10.0], None);
}

fn unit_box() -> ColShape {
    ColShape::Box {
        size: V3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
    }
}

/// A fresh world and scene for one test.
fn setup() -> Scene {
    reset();
    Scene::new()
}

// M9-API-200
//
// A Rigidbody with a BoxCollider falls under gravity, rests on a floor and carries its object.
#[test]
fn a_rigidbody_with_a_box_rests_on_the_floor() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    floor();
    let o = make("box");
    place(o, 0.0, 4.0, 0.0);
    comp::rb_attach(o, false);
    comp::col_attach(o, unit_box());
    run(240);
    let y = pos(o).y;
    assert!((y - 1.0).abs() < 0.05, "rests at {y}");
}

// M8-PHY-03
//
// `Physics.snapshot` / `restore`: the objects on bodies go back with the world, a restore can be
// repeated, stepping on repeats the same steps, and a freed or unknown snapshot is refused.
#[test]
fn restoring_a_snapshot_rewinds_the_objects_and_stepping_on_repeats() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    floor();
    let boxes: Vec<ObjectId> = (0..4)
        .map(|i| {
            let o = make("box");
            place(o, 0.2 * i as f32, 1.0 + 1.1 * i as f32, 0.0);
            comp::rb_attach(o, false);
            comp::col_attach(o, unit_box());
            o
        })
        .collect();
    let at = |boxes: &[ObjectId]| boxes.iter().map(|&o| pos(o)).collect::<Vec<_>>();
    run(20);
    let snap = crate::physics_api::snapshot();
    assert_eq!(snap, 1.0);
    assert_eq!(crate::physics_api::snapshot_count(), 1.0);
    let then = at(&boxes);
    run(100);
    let later = at(&boxes);
    assert_ne!(then, later, "the stack moves on");

    assert!(crate::physics_api::restore(snap));
    assert_eq!(at(&boxes), then, "the objects went back with the bodies");
    run(100);
    assert_eq!(at(&boxes), later, "stepping on repeats the same steps");
    // Again, from the same snapshot.
    assert!(crate::physics_api::restore(snap));
    assert_eq!(at(&boxes), then);

    crate::physics_api::drop_snapshot(snap);
    assert_eq!(crate::physics_api::snapshot_count(), 0.0);
    assert!(
        !crate::physics_api::restore(snap),
        "a freed snapshot is refused"
    );
    assert!(!crate::physics_api::restore(0.0) && !crate::physics_api::restore(99.0));
    // The freed place is used again.
    assert_eq!(crate::physics_api::snapshot(), 1.0);
}

// M9-API-201
//
// Moving the GameObject of a kinematic body from a script carries the body there.
#[test]
fn moving_a_kinematic_object_moves_its_body() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let o = make("platform");
    let rb = comp::rb_attach(o, false);
    comp::rb_set_kind(rb, "kinematic");
    comp::col_attach(o, unit_box());
    place(o, 3.0, 2.0, 0.0);
    run(3);
    let body = comp::body_of(o);
    let (p, _) = crate::st(|s| s.physics.pose(body)).unwrap();
    assert!(
        (p[0] - 3.0).abs() < 0.01 && (p[1] - 2.0).abs() < 0.01,
        "{p:?}"
    );
    // The object is where the script put it.
    assert!((pos(o).x - 3.0).abs() < 0.01);
}

// M9-API-202
//
// A collider is remade at the object's world scale when the scale changes.
#[test]
fn colliders_follow_the_object_scale() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let o = make("wall");
    comp::col_attach(o, unit_box());
    run(1);
    let ray = |x: f64| {
        crate::do_raycast(
            V3 { x, y: 5.0, z: 0.0 },
            V3 {
                x: 0.0,
                y: -1.0,
                z: 0.0,
            },
            20.0,
        )
    };
    assert_eq!(ray(1.2), 0, "a 1 m box is not at x = 1.2");
    platypus_scene::api::with_scene(|s| {
        let t = s.local(o).unwrap();
        s.set_local(
            o,
            platypus_scene::Transform {
                scale: Vec3::splat(3.0),
                ..t
            },
        );
    });
    run(1);
    assert_ne!(ray(1.2), 0, "a 3 m box is");
}

// M9-API-203
//
// A collider's center offsets the shape; a trigger reports a sensor contact that maps back to its
// component and object.
#[test]
fn centers_and_triggers_report_through_collider_info() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    floor();
    let zone = make("zone");
    place(zone, 0.0, 2.0, 0.0);
    let zc = comp::col_attach(zone, unit_box());
    comp::col_set_sensor(zc, true);
    comp::col_set_center(
        zc,
        V3 {
            x: 4.0,
            y: 0.0,
            z: 0.0,
        },
    );
    let ball = make("ball");
    place(ball, 4.0, 6.0, 0.0);
    comp::rb_attach(ball, false);
    comp::col_attach(ball, ColShape::Sphere { radius: 0.3 });
    run(120);
    let contacts = take_contacts();
    let enter = contacts
        .iter()
        .find(|c| c.started && c.sensor)
        .expect("the ball entered the trigger");
    let sides: Vec<_> = [enter.a, enter.b]
        .iter()
        .filter_map(|c| comp::collider_info(*c))
        .collect();
    assert!(
        sides
            .iter()
            .any(|i| i.object == zone && i.class == "BoxCollider")
    );
    assert!(
        sides
            .iter()
            .any(|i| i.object == ball && i.class == "SphereCollider" && i.rigidbody != 0)
    );
    assert!(crate::object_of_collider(enter.a).is_some());
}

// M9-API-204
//
// A fixed joint holds a body where it hangs; a rope joint stops it at its length.
#[test]
fn joints_hold_bodies() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let anchor = make("anchor");
    place(anchor, 0.0, 5.0, 0.0);
    let arb = comp::rb_attach(anchor, false);
    comp::rb_set_kind(arb, "kinematic");
    comp::col_attach(anchor, unit_box());

    let held = make("held");
    place(held, 0.0, 3.5, 0.0);
    comp::rb_attach(held, false);
    comp::col_attach(held, ColShape::Sphere { radius: 0.2 });
    let j = comp::add_comp(held, comp::Data::Joint(comp::Jt::new("fixed")));
    comp::with_joint(j, |j| j.connected = arb);
    comp::joint_build(j);

    let swing = make("swing");
    place(swing, 3.0, 5.0, 0.0);
    let srb = comp::rb_attach(swing, false);
    let _ = srb;
    comp::col_attach(swing, ColShape::Sphere { radius: 0.2 });
    let anchor2 = make("anchor2");
    place(anchor2, 3.0, 5.0, 0.0);
    let a2 = comp::rb_attach(anchor2, false);
    comp::rb_set_kind(a2, "kinematic");
    comp::col_attach(anchor2, ColShape::Sphere { radius: 0.1 });
    let rj = comp::add_comp(swing, comp::Data::Joint(comp::Jt::new("rope")));
    comp::with_joint(rj, |j| {
        j.connected = a2;
        j.max_distance = 1.0;
    });
    comp::joint_build(rj);

    run(180);
    assert!((pos(held).y - 3.5).abs() < 0.05, "held at {}", pos(held).y);
    let d = (pos(swing) - pos(anchor2)).length();
    assert!(d < 1.1, "rope length {d}");
    assert!(pos(swing).y < 4.2, "it hangs: {}", pos(swing).y);
}

// M9-API-205
//
// A Rope makes its links on the first step and hangs from where it is.
#[test]
fn a_rope_hangs_from_its_object() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let o = make("rope");
    place(o, 0.0, 5.0, 0.0);
    let r = comp::add_comp(o, comp::Data::Rope(comp::rope_new()));
    comp::with_rope(r, |r| {
        r.length = 2.0;
        r.segments = 4.0;
    });
    run(60);
    let pts = comp::rope_points(r);
    assert_eq!(pts.len(), 5);
    assert!((pts[0].y - 5.0).abs() < 0.05, "{:?}", pts[0]);
    assert!((pts[4].y - 3.0).abs() < 0.3, "{:?}", pts[4]);
    comp::rope_destroy(r);
    assert!(comp::rope_points(r).is_empty());
}

// M9-API-206
//
// A character controller drops onto the floor, reports grounded and carries its object.
#[test]
fn a_character_walks_on_the_floor() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    floor();
    let o = make("hero");
    place(o, 0.0, 3.0, 0.0);
    let c = crate::components::character_attach(o);
    for _ in 0..90 {
        crate::components::character_move(
            c,
            V3 {
                x: 0.02,
                y: -0.1,
                z: 0.0,
            },
        );
        step();
    }
    assert!(crate::components::is_grounded(c));
    let p = pos(o);
    assert!((p.y - 1.5).abs() < 0.1, "{p:?}");
    assert!(p.x > 1.0, "{p:?}");
}

// M9-API-207
//
// 2D bodies fall in the XY plane (z stays) and rest on a 2D floor; a 2D ray names the object hit.
#[test]
fn two_d_bodies_keep_their_z() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let ground = make("ground");
    comp::col_attach(
        ground,
        ColShape::Box2D {
            size: V2 { x: 20.0, y: 1.0 },
        },
    );
    let o = make("ball");
    place(o, 0.0, 5.0, 0.7);
    comp::rb_attach(o, true);
    comp::col_attach(o, ColShape::Circle2D { radius: 0.5 });
    run(240);
    let p = pos(o);
    assert!((p.y - 1.0).abs() < 0.06, "rests at {p:?}");
    assert!((p.z - 0.7).abs() < 1e-3, "z locked: {p:?}");
    let hit = crate::do_raycast(
        V3 {
            x: 5.0,
            y: 5.0,
            z: 0.0,
        },
        V3 {
            x: 0.0,
            y: -1.0,
            z: 0.0,
        },
        100.0,
    );
    assert_ne!(hit, 0);
    let cid = crate::st(|s| s.hits[hit as usize - 1].collider);
    assert_eq!(comp::collider_info(cid).map(|i| i.object), Some(ground));
    assert!(comp::collider_info(cid).unwrap().two_d);
}

// M9-API-208
//
// Component kinds save and load their properties and list what is on an object.
#[test]
fn kinds_save_and_load_physics_components() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    platypus_kinds::clear();
    crate::register_kinds();
    let a = make("a");
    let b = make("b");
    let bx = platypus_kinds::get("BoxCollider").unwrap();
    let h = (bx.create)(a.to_raw()).unwrap();
    let mut props = Map::new();
    props.insert(
        "size".into(),
        json!({ "$type": "Vector3", "value": [2.0, 1.0, 4.0] }),
    );
    props.insert("isTrigger".into(), json!(true));
    props.insert(
        "material".into(),
        json!({ "dynamicFriction": 0.2, "bounciness": 0.7 }),
    );
    assert!((bx.load)(h, &props).is_empty());
    let saved = (bx.save)(h);
    assert_eq!(saved["size"]["value"], json!([2.0, 1.0, 4.0]));
    assert_eq!(saved["isTrigger"], json!(true));
    assert_eq!(saved["material"]["bounciness"], json!(0.7));
    // Copy to another object.
    let h2 = (bx.create)(b.to_raw()).unwrap();
    assert!((bx.load)(h2, &saved).is_empty());
    assert_eq!((bx.save)(h2), saved);
    assert_eq!((bx.on)(b.to_raw()), vec![h2]);

    // A joint's reference to a Rigidbody keeps its placeholder.
    let rb = platypus_kinds::get("Rigidbody").unwrap();
    let r = (rb.create)(a.to_raw()).unwrap();
    let hinge = platypus_kinds::get("HingeJoint").unwrap();
    let j = (hinge.create)(b.to_raw()).unwrap();
    let mut jp = Map::new();
    jp.insert(
        "connectedBody".into(),
        json!({ "$ref": "Component", "kind": "Rigidbody", "handle": r }),
    );
    jp.insert("limits".into(), json!({ "min": -30.0, "max": 45.0 }));
    jp.insert("useLimits".into(), json!(true));
    assert!((hinge.load)(j, &jp).is_empty());
    let js = (hinge.save)(j);
    assert_eq!(js["connectedBody"]["handle"], json!(r));
    assert_eq!(js["limits"]["max"], json!(45.0));

    // Destroying a component through its kind removes its collider.
    (bx.destroy)(h);
    assert!((bx.on)(a.to_raw()).is_empty());
}

// M9-API-209
//
// Destroying an object's physics removes its body and colliders from the world.
#[test]
fn destroying_an_object_removes_its_physics() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let o = make("rock");
    comp::rb_attach(o, false);
    comp::col_attach(o, unit_box());
    let down = |_: ()| {
        crate::do_raycast(
            V3 {
                x: 0.0,
                y: 5.0,
                z: 0.0,
            },
            V3 {
                x: 0.0,
                y: -1.0,
                z: 0.0,
            },
            50.0,
        )
    };
    assert_ne!(down(()), 0);
    crate::object_destroyed(o);
    assert_eq!(down(()), 0);
    // An object removed from the scene loses its physics at the next step.
    let p = make("pebble");
    comp::col_attach(p, unit_box());
    platypus_scene::api::with_scene(|s| s.destroy(p));
    run(1);
    assert_eq!(down(()), 0);
}

// M9-API-210
//
// A raycast answers the GameObject it hit, only counts colliders on the layers of its mask, and
// skips a collider the origin starts inside (as Unity does).
#[test]
fn raycasts_filter_by_layer_and_name_the_hit_object() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let far = make("far");
    place(far, 10.0, 0.0, 0.0);
    comp::col_attach(far, unit_box());
    let near = make("near");
    place(near, 5.0, 0.0, 0.0);
    comp::col_attach(near, unit_box());
    platypus_scene::api::with_scene(|s| s.set_layer(near, 3));
    run(1);
    let cast = |x: f64, mask: u32| {
        let n = crate::do_raycast_masked(
            V3 { x, y: 0.0, z: 0.0 },
            V3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            100.0,
            mask,
        );
        let hit = crate::st(|s| s.hits.get(n as usize - 1).map(|h| (h.object, h.distance)));
        hit.map(|(o, d)| (o.unwrap(), (d * 100.0).round() / 100.0))
    };
    // Every layer: the near box, its face at 4.5 m.
    assert_eq!(cast(0.0, u32::MAX), Some((near, 4.5)));
    // Layer 0 only (Default): the far box, at 9.5 m.
    assert_eq!(cast(0.0, 1), Some((far, 9.5)));
    // Layer 3 only: the near box again.
    assert_eq!(cast(0.0, 1 << 3), Some((near, 4.5)));
    // Starting inside the near box skips it, so the far box is the hit.
    assert_eq!(cast(5.0, u32::MAX), Some((far, 4.5)));
    // An infinite distance (the default) reaches far targets.
    let far_ray = |dist: f64| {
        crate::do_raycast_masked(
            V3 {
                x: -1.0e6,
                y: 0.0,
                z: 0.0,
            },
            V3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            dist,
            u32::MAX,
        )
    };
    assert_ne!(far_ray(f64::INFINITY), 0);
    assert_ne!(far_ray(1.0e30), 0);
    assert_eq!(far_ray(100.0), 0);
    // A mask that matches nothing is a miss.
    assert_eq!(
        crate::do_raycast_masked(
            V3 {
                x: 0.0,
                y: 0.0,
                z: 0.0
            },
            V3 {
                x: 1.0,
                y: 0.0,
                z: 0.0
            },
            100.0,
            1 << 7
        ),
        0
    );
}

// M9-API-214
//
// `raycastAll` answers every collider on the ray nearest first (honouring the layer mask and the
// start-inside rule); `overlapSphere` answers the colliders a sphere touches, by layer.
#[test]
fn raycast_all_and_overlap_sphere_list_every_collider() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let far = make("far");
    place(far, 10.0, 0.0, 0.0);
    comp::col_attach(far, unit_box());
    let near = make("near");
    place(near, 5.0, 0.0, 0.0);
    comp::col_attach(near, unit_box());
    platypus_scene::api::with_scene(|s| s.set_layer(near, 3));
    let off = make("off");
    place(off, 0.0, 20.0, 0.0);
    comp::col_attach(off, unit_box());
    run(1);
    let x_axis = V3 {
        x: 1.0,
        y: 0.0,
        z: 0.0,
    };
    let at = |x: f64, y: f64| V3 { x, y, z: 0.0 };
    let all = |from: V3, mask: u32| -> Vec<(Option<ObjectId>, f64)> {
        crate::do_raycast_all(from, x_axis, 1.0e30, mask)
            .into_iter()
            .map(|n| {
                crate::st(|s| {
                    let h = &s.hits[n as usize - 1];
                    (h.object, (h.distance * 100.0).round() / 100.0)
                })
            })
            .collect()
    };
    // Both boxes, nearest first.
    assert_eq!(
        all(at(0.0, 0.0), u32::MAX),
        vec![(Some(near), 4.5), (Some(far), 9.5)]
    );
    // The layer mask drops one; starting inside the near box skips it.
    assert_eq!(all(at(0.0, 0.0), 1), vec![(Some(far), 9.5)]);
    assert_eq!(all(at(5.0, 0.0), u32::MAX), vec![(Some(far), 4.5)]);
    assert!(all(at(0.0, 5.0), u32::MAX).is_empty());

    // A sphere at the origin of radius 5 reaches the near box (face at 4.5) but not the far one.
    let touched = |center: V3, radius: f64, mask: u32| -> Vec<Option<ObjectId>> {
        crate::do_overlap_sphere(center, radius, mask)
            .into_iter()
            .map(crate::object_of_collider)
            .collect()
    };
    assert_eq!(touched(at(0.0, 0.0), 5.0, u32::MAX), vec![Some(near)]);
    assert_eq!(touched(at(0.0, 0.0), 10.0, u32::MAX).len(), 2);
    // Centred inside a box counts as touching it.
    assert_eq!(touched(at(10.0, 0.0), 0.1, u32::MAX), vec![Some(far)]);
    // The layer mask keeps only layer 0 or only layer 3.
    assert_eq!(touched(at(0.0, 0.0), 10.0, 1), vec![Some(far)]);
    assert_eq!(touched(at(0.0, 0.0), 10.0, 1 << 3), vec![Some(near)]);
    assert!(touched(at(0.0, 0.0), 2.0, u32::MAX).is_empty());
}

// RDY-PHY-01
//
// Two ignored colliders pass through each other, also after one is rebuilt; lifting it restores
// the contact.
#[test]
fn ignored_collider_components_fall_through_each_other() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let ground = make("ground");
    place(ground, 0.0, 0.0, 0.0);
    let gc = comp::col_attach(ground, unit_box());
    let o = make("box");
    place(o, 0.0, 3.0, 0.0);
    comp::rb_attach(o, false);
    let bc = comp::col_attach(o, unit_box());
    comp::set_ignored(gc, bc, true);
    // A rebuilt collider keeps being ignored.
    comp::col_edit(bc, |s| *s = unit_box());
    run(120);
    assert!(pos(o).y < -2.0, "fell to {}", pos(o).y);
    comp::set_ignored(gc, bc, false);
    place(o, 0.0, 3.0, 0.0);
    run(240);
    assert!((pos(o).y - 1.0).abs() < 0.05, "rests at {}", pos(o).y);
}

// RDY-PHY-04
//
// A 2D impulse changes the velocity a script reads at once, not after the next step.
#[test]
fn an_impulse_changes_the_velocity_at_once() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let o = make("ball");
    let rb = comp::rb_attach(o, true);
    comp::col_attach(o, ColShape::Circle2D { radius: 0.5 });
    let h = crate::Rigidbody2DRef(rb);
    run(2);
    h.add_force(V2 { x: 0.0, y: 5.0 }, 1.0);
    assert!(h.velocity().y > 1.0, "{:?}", h.velocity());
}

// RDY-PHY-02, RDY-PHY-05
//
// A collider-less Rigidbody2D hung from a HingeJoint2D (limits -45..225 degrees) swings out when
// pushed: it has the inertia of the box its scale stands for.
#[test]
fn a_hinge_joint_2d_lets_a_collider_less_body_swing() {
    let mut scene = setup();
    let _scope = platypus_scene::api::enter(&mut scene);
    let frame = make("frame");
    place(frame, 0.0, 0.0, 0.0);
    let frame_rb = comp::rb_attach(frame, true);
    comp::rb_set_kind(frame_rb, "fixed");
    let seat = make("seat");
    place(seat, 0.0, -1.5, 0.0);
    platypus_scene::api::with_scene(|s| {
        let mut t = s.local(seat).unwrap();
        t.scale = Vec3::new(0.05, 3.0, 1.0);
        s.set_local(seat, t);
    });
    let rb = comp::rb_attach(seat, true);
    let joint = crate::HingeJoint2DRef::attach(seat);
    joint.set_connected_body(crate::Rigidbody2DRef(frame_rb));
    joint.set_anchor(V2 { x: 0.0, y: 0.5 });
    joint.set_use_limits(true);
    joint.set_limits_min(-45.0);
    joint.set_limits_max(225.0);
    crate::Rigidbody2DRef(rb).add_force(V2 { x: 8.0, y: 0.0 }, 1.0);
    let mut far = 0.0f32;
    for _ in 0..90 {
        step();
        far = far.max(pos(seat).x);
    }
    assert!(far > 0.8, "the seat swung out {far} m");
}
