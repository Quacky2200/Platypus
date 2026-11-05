// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The scene's `#[ts_api]` namespace: handles, values and the current scene. IDs are in
//! TESTS.md (M9-API-*).
use glam::Quat;
use platypus_scene::api::{enter, scene_api as api};
use platypus_scene::{ObjectId, Scene};
use ts_api::Handle;

// M9-API-12
//
// A handle is the index and generation offset so that no live object is 0 (0 is "no object" in a
// script's `GameObject | null`), and it converts back exactly.
#[test]
fn an_object_handle_is_never_zero_and_round_trips() {
    for id in [
        ObjectId {
            index: 0,
            generation: 0,
        },
        ObjectId {
            index: 7,
            generation: 3,
        },
        ObjectId {
            index: u32::MAX,
            generation: u32::MAX - 1,
        },
    ] {
        let raw = id.to_raw();
        assert_ne!(raw, 0, "{id:?}");
        assert_eq!(ObjectId::from_raw(raw), id);
    }
    // And the "no object" handle names no slot.
    let mut scene = Scene::new();
    let first = scene.create("a");
    assert!(!scene.contains(ObjectId::from_raw(0)));
    assert_ne!(first.to_raw(), 0);
}

// M9-API-13
#[test]
fn the_shims_create_move_and_destroy_objects_in_the_current_scene() {
    let mut scene = Scene::new();
    {
        let _scope = enter(&mut scene);
        let a = api::platypus_Scene_create();
        let b = api::platypus_Scene_create();
        assert_ne!(a, 0);
        assert_ne!(a, b);
        assert_eq!(api::platypus_Scene_count(), 2);
        assert!(api::platypus_Scene_exists(a));

        // A Vector3 argument is three f64s; the result comes back through a pointer.
        api::platypus_Scene_setLocalPosition(a, 1.0, 2.0, 3.0);
        let mut out = [0.0f64; 3];
        unsafe { api::platypus_Scene_localPosition(a, out.as_mut_ptr()) };
        assert_eq!(out, [1.0, 2.0, 3.0]);

        // Parenting: the world position includes the parent's.
        api::platypus_Scene_setLocalPosition(b, 10.0, 0.0, 0.0);
        api::platypus_Scene_setParent(a, b, false);
        assert_eq!(api::platypus_Scene_parent(a), b);
        unsafe { api::platypus_Scene_position(a, out.as_mut_ptr()) };
        assert_eq!(out, [11.0, 2.0, 3.0]);

        // Setting the world position works back through the parent.
        api::platypus_Scene_setPosition(a, 10.0, 5.0, 0.0);
        unsafe { api::platypus_Scene_localPosition(a, out.as_mut_ptr()) };
        assert_eq!(out, [0.0, 5.0, 0.0]);

        // Destroying the parent destroys the child; the ids stop working.
        api::platypus_Scene_destroy(b);
        assert!(!api::platypus_Scene_exists(a));
        assert!(!api::platypus_Scene_exists(b));
        assert_eq!(api::platypus_Scene_count(), 0);
        unsafe { api::platypus_Scene_position(a, out.as_mut_ptr()) };
        assert_eq!(out, [0.0, 0.0, 0.0]);
        // The slot is reused with a new generation: the old handle still misses.
        let c = api::platypus_Scene_create();
        assert!(api::platypus_Scene_exists(c));
        assert!(!api::platypus_Scene_exists(a));
    }
    // Without a current scene every function answers its default.
    assert_eq!(api::platypus_Scene_create(), 0);
    assert_eq!(api::platypus_Scene_count(), 0);
}

// M9-API-14
#[test]
fn rotation_is_a_quaternion_value_and_flags_are_booleans() {
    let mut scene = Scene::new();
    let _scope = enter(&mut scene);
    let a = api::platypus_Scene_create();
    let q = Quat::from_rotation_y(0.5);
    api::platypus_Scene_setLocalRotation(a, q.x.into(), q.y.into(), q.z.into(), q.w.into());
    let mut out = [0.0f64; 4];
    unsafe { api::platypus_Scene_localRotation(a, out.as_mut_ptr()) };
    let back = Quat::from_xyzw(out[0] as f32, out[1] as f32, out[2] as f32, out[3] as f32);
    assert!(back.abs_diff_eq(q, 1e-6), "{back:?} vs {q:?}");
    assert!(api::platypus_Scene_enabled(a));
    api::platypus_Scene_setEnabled(a, false);
    assert!(!api::platypus_Scene_enabled(a));
    assert!(!api::platypus_Scene_active(a));
    api::platypus_Scene_setLayer(a, 7);
    assert_eq!(api::platypus_Scene_layer(a), 7);
    api::platypus_Scene_setLayer(a, 999);
    assert_eq!(api::platypus_Scene_layer(a), 31);
}

// M9-API-15
#[test]
fn the_manifest_declares_the_handle_and_both_value_classes() {
    let m = api::MANIFEST;
    assert_eq!(m.name, "Scene");
    assert_eq!(m.handles, &["GameObject", "Transform"]);
    let names: Vec<&str> = m.values.iter().map(|v| v.name).collect();
    assert_eq!(names, ["Vector3", "Quaternion", "Matrix4x4"]);
    let d = m.to_dts();
    assert!(d.contains("function create(): GameObject;"), "{d}");
    assert!(
        d.contains("function setPosition(object: GameObject, position: Vector3): void;"),
        "{d}"
    );
    assert!(
        d.contains("function rotation(object: GameObject): Quaternion;"),
        "{d}"
    );
    assert!(d.contains("declare class Quaternion {"), "{d}");
}

// M9-API-19
//
// `GameObject` and `Transform` as classes: the shims take the receiver first, a transform shares
// its object's handle, and the declaration file has accessors and methods.
#[test]
fn game_object_and_transform_classes_work_through_their_shims() {
    let mut scene = Scene::new();
    let _scope = enter(&mut scene);
    let go = api::platypus_GameObject_create();
    assert!(api::platypus_GameObject_exists(go));
    let t = api::platypus_GameObject_transform(go);
    assert_eq!(t, go, "a transform is the object's own handle");
    assert_eq!(api::platypus_Transform_gameObject(t), go);
    api::platypus_Transform_setLocalScale(t, 2.0, 2.0, 2.0);
    api::platypus_Transform_setPosition(t, 1.0, 2.0, 3.0);
    let mut out = [0.0f64; 3];
    unsafe { api::platypus_Transform_position(t, out.as_mut_ptr()) };
    assert_eq!(out, [1.0, 2.0, 3.0]);
    api::platypus_GameObject_setEnabled(go, false);
    assert!(!api::platypus_GameObject_activeInHierarchy(go));
    assert_eq!(api::platypus_Transform_parent(t), 0, "a root has no parent");
    api::platypus_GameObject_destroy(go);
    assert!(!api::platypus_GameObject_exists(go));

    let d = api::MANIFEST.to_dts();
    assert!(d.contains("declare class GameObject {"), "{d}");
    assert!(d.contains("static create(): GameObject;"), "{d}");
    assert!(d.contains("get transform(): Transform;"), "{d}");
    assert!(d.contains("get name(): string;"), "{d}");
    assert!(d.contains("set name(value: string);"), "{d}");
    assert!(d.contains("static find(name: string): GameObject;"), "{d}");
    assert!(d.contains("get position(): Vector3;"), "{d}");
    assert!(d.contains("set position(value: Vector3);"), "{d}");
    assert!(
        d.contains("setParent(parent: Transform, keepWorld: boolean): void;"),
        "{d}"
    );
}

// M9-API-211
//
// Unity's tags: objects are "Untagged" until tagged; `find_with_tag` lists the active objects with a
// tag in scene order, and tagging "Untagged" clears the tag.
#[test]
fn objects_are_found_by_tag() {
    let mut scene = Scene::new();
    let a = scene.create("a");
    let b = scene.create("b");
    let c = scene.create("c");
    assert_eq!(scene.tag_name(a), "Untagged");
    scene.set_tag_name(a, "Enemy");
    scene.set_tag_name(c, "Enemy");
    scene.set_tag_name(b, "Player");
    assert_eq!(scene.find_with_tag("Enemy"), vec![a, c]);
    assert_eq!(scene.find_with_tag("Player"), vec![b]);
    // An inactive object is not found (Unity's FindGameObjectsWithTag).
    scene.set_enabled(a, false);
    assert_eq!(scene.find_with_tag("Enemy"), vec![c]);
    scene.set_tag_name(c, "Untagged");
    assert!(scene.find_with_tag("Enemy").is_empty());
    assert_eq!(scene.tag_name(c), "Untagged");
}

// M9-API-213
//
// Unity's Transform hierarchy: children by index and path, the root, sibling order, `isChildOf`,
// detaching, vectors through a scaled parent, `rotateAround`, and `setParent` keeping the world
// scale.
#[test]
fn transforms_walk_and_reshape_the_hierarchy_like_unity() {
    use platypus_math::V3;
    use platypus_scene::api::TransformRef as T;
    let v = |x, y, z| V3 { x, y, z };
    let near = |a: V3, b: V3| (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs() < 1e-4;

    let mut scene = Scene::new();
    let (arm, hand, ring, other) = (
        scene.create("arm"),
        scene.create("hand"),
        scene.create("ring"),
        scene.create("other"),
    );
    let _scope = enter(&mut scene);
    let (arm_t, hand_t, ring_t) = (T(arm), T(hand), T(ring));
    hand_t.set_parent(arm_t, false);
    ring_t.set_parent(hand_t, false);

    assert_eq!(arm_t.child_count(), 1.0);
    assert_eq!(arm_t.get_child(0.0), hand_t);
    assert_eq!(ring_t.root(), arm_t);
    assert_eq!(arm_t.find("hand/ring"), ring_t);
    assert_eq!(arm_t.find("hand/nope").0, ObjectId::from_raw(0));
    assert!(ring_t.is_child_of(arm_t) && !arm_t.is_child_of(ring_t));
    assert!(
        ring_t.is_child_of(ring_t),
        "a transform is a child of itself, like Unity"
    );

    // Sibling order.
    T(other).set_parent(arm_t, false);
    assert_eq!(T(other).sibling_index(), 1.0);
    T(other).set_as_first_sibling();
    assert_eq!(arm_t.get_child(0.0), T(other));
    assert_eq!(hand_t.sibling_index(), 1.0);
    hand_t.set_sibling_index(0.0);
    assert_eq!(arm_t.get_child(0.0), hand_t);
    T(other).set_as_last_sibling();
    assert_eq!(T(other).sibling_index(), 1.0);

    // Vectors go through the scale, directions do not.
    arm_t.set_local_scale(v(2.0, 2.0, 2.0));
    hand_t.set_local_position(v(1.0, 0.0, 0.0));
    assert!(near(hand_t.position(), v(2.0, 0.0, 0.0)));
    assert!(near(
        hand_t.transform_vector(v(1.0, 0.0, 0.0)),
        v(2.0, 0.0, 0.0)
    ));
    assert!(near(
        hand_t.inverse_transform_vector(v(2.0, 0.0, 0.0)),
        v(1.0, 0.0, 0.0)
    ));
    assert!(near(
        hand_t.inverse_transform_direction(v(0.0, 1.0, 0.0)),
        v(0.0, 1.0, 0.0)
    ));

    // `rotateAround` orbits a point and turns with it.
    arm_t.set_local_scale(v(1.0, 1.0, 1.0));
    ring_t.set_local_position(v(0.0, 0.0, 0.0));
    hand_t.set_local_position(v(1.0, 0.0, 0.0));
    hand_t.rotate_around(v(0.0, 0.0, 0.0), v(0.0, 1.0, 0.0), 90.0);
    assert!(near(hand_t.position(), v(0.0, 0.0, -1.0)));

    // Re-parenting with keepWorld keeps the world scale as well as the pose.
    arm_t.set_local_scale(v(2.0, 2.0, 2.0));
    ring_t.set_parent(T(other), true);
    assert!(near(ring_t.lossy_scale(), v(2.0, 2.0, 2.0)));
    ring_t.set_parent(T(ObjectId::from_raw(0)), true);
    assert!(near(ring_t.lossy_scale(), v(2.0, 2.0, 2.0)));
    assert!(near(ring_t.local_scale(), v(2.0, 2.0, 2.0)));
    // Matrices: the translation column is the world position, and the two undo each other.
    ring_t.set_parent(hand_t, false);
    ring_t.set_local_position(v(0.0, 3.0, 0.0));
    let m = ring_t.local_to_world_matrix();
    let at = ring_t.position();
    assert!(near(v(m.m[12], m.m[13], m.m[14]), at));
    let id = ring_t.world_to_local_matrix().mul(m);
    for (i, want) in platypus_math::M4::IDENTITY.m.iter().enumerate() {
        assert!((id.m[i] - want).abs() < 1e-6, "element {i}: {}", id.m[i]);
    }

    // `find`: `..` goes up, `.` stays, and a leading `/` starts at the roots.
    assert_eq!(ring_t.find("..").0, hand);
    assert_eq!(ring_t.find("../../other"), T(other));
    assert_eq!(arm_t.find("./hand/ring"), ring_t);
    assert_eq!(hand_t.find("/arm/hand"), hand_t);
    assert_eq!(arm_t.find("/nope").0, ObjectId::from_raw(0));
    assert_eq!(
        arm_t.find("..").0,
        ObjectId::from_raw(0),
        "a root has no parent"
    );

    ring_t.detach_children();
    arm_t.detach_children();
    assert_eq!(arm_t.child_count(), 0.0);
    assert_eq!(hand_t.root(), hand_t);
}
