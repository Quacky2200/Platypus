// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Joint components (`FixedJoint`, `CharacterJoint`, `HingeJoint`, `SpringJoint`, `RopeJoint`)
//! and `Rope` for compiled scripts, as in `physics-components.ts`. A joint is made (and remade
//! when a setting changes) as soon as its object has a Rigidbody; one set up before that is made
//! on the first `step` (`start_pending`). The anchors are worked out from where the bodies are
//! at that moment unless `autoConfigureConnectedAnchor` is off.
//!
//! Differences to the TypeScript: a joint is made when a property is set rather than in
//! `onStart`; `HingeJoint.limits` and `.motor` are the flat `limitsMin`, `limitsMax`,
//! `motorTargetVelocity` and `motorForce`; `Rope.points()` and `.links` are `pointCount` /
//! `pointAt(i)` and `linkCount` / `linkAt(i)`; a rope has no line or capsule drawing yet.

use crate::comp::{self, Data, Jt, RopeD, with_joint, with_rope};
use crate::*;
use platypus_math::V2;
use ts_api::ts_api;

fn jg<R: Default>(h: u32, f: impl FnOnce(&Jt) -> R) -> R {
    with_joint(h, |j| f(j)).unwrap_or_default()
}

fn jset(h: u32, f: impl FnOnce(&mut Jt)) {
    with_joint(h, f);
    comp::joint_build(h);
}

fn joint_attach(object: ObjectId, kind: &'static str) -> u32 {
    let h = comp::add_comp(object, Data::Joint(Jt::new(kind)));
    comp::joint_build(h);
    h
}

fn find_joint(object: ObjectId, kind: &'static str) -> u32 {
    comp::comps_on(object, |d| matches!(d, Data::Joint(j) if j.kind == kind))
        .first()
        .copied()
        .unwrap_or(0)
}

fn rg<R: Default>(h: u32, f: impl FnOnce(&RopeD) -> R) -> R {
    with_rope(h, |r| f(r)).unwrap_or_default()
}

pub(crate) fn count_joints() -> u32 {
    st(|s| {
        s.comps
            .iter()
            .flatten()
            .filter(|c| matches!(&c.data, Data::Joint(_)))
            .count() as u32
    })
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[ts_api(
    namespace = "PhysicsJoints", handles(
        GameObject = ObjectId,
        Rigidbody = RigidbodyRef,
        FixedJoint = FixedJointRef,
        CharacterJoint = CharacterJointRef,
        HingeJoint = HingeJointRef,
        HingeJoint2D = HingeJoint2DRef,
        Rigidbody2D = Rigidbody2DRef,
        SpringJoint = SpringJointRef,
        RopeJoint = RopeJointRef,
        Rope = RopeRef
    ),
    values(Vector2 = V2 [x: f64, y: f64], Vector3 = V3 [x: f64, y: f64, z: f64])
)]
pub mod joints_api {
    use super::*;

    /// How many joint components exist.
    pub fn joint_count() -> u32 {
        count_joints()
    }

    impl FixedJointRef {
        /// Adds a FixedJoint to a GameObject. Holds two bodies rigidly together.
        pub fn attach(object: ObjectId) -> FixedJointRef {
            FixedJointRef(joint_attach(object, "fixed"))
        }

        /// The FixedJoint on an object, or none.
        pub fn find_on(object: ObjectId) -> FixedJointRef {
            FixedJointRef(find_joint(object, "fixed"))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The Rigidbody this joins to (none: the world).
        #[property]
        pub fn connected_body(self) -> RigidbodyRef {
            RigidbodyRef(jg(self.0, |j| j.connected))
        }

        pub fn set_connected_body(self, rb: RigidbodyRef) {
            jset(self.0, |j| j.connected = rb.0);
        }

        /// In this object's space.
        #[property]
        pub fn anchor(self) -> V3 {
            jg(self.0, |j| j.anchor)
        }

        pub fn set_anchor(self, a: V3) {
            jset(self.0, |j| j.anchor = a);
        }

        /// In the connected body's space (or the world's).
        #[property]
        pub fn connected_anchor(self) -> V3 {
            jg(self.0, |j| j.connected_anchor)
        }

        pub fn set_connected_anchor(self, a: V3) {
            jset(self.0, |j| j.connected_anchor = a);
        }

        #[property]
        pub fn auto_configure_connected_anchor(self) -> bool {
            with_joint(self.0, |j| j.auto_anchor).unwrap_or(true)
        }

        pub fn set_auto_configure_connected_anchor(self, on: bool) {
            jset(self.0, |j| j.auto_anchor = on);
        }

        pub fn destroy(self) {
            comp::joint_destroy(self.0);
        }
    }

    impl CharacterJointRef {
        /// Adds a CharacterJoint to a GameObject. Lets the bodies turn freely about the anchor, ball-and-socket style (ragdoll limbs, chains).
        pub fn attach(object: ObjectId) -> CharacterJointRef {
            CharacterJointRef(joint_attach(object, "ball"))
        }

        /// The CharacterJoint on an object, or none.
        pub fn find_on(object: ObjectId) -> CharacterJointRef {
            CharacterJointRef(find_joint(object, "ball"))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The Rigidbody this joins to (none: the world).
        #[property]
        pub fn connected_body(self) -> RigidbodyRef {
            RigidbodyRef(jg(self.0, |j| j.connected))
        }

        pub fn set_connected_body(self, rb: RigidbodyRef) {
            jset(self.0, |j| j.connected = rb.0);
        }

        /// In this object's space.
        #[property]
        pub fn anchor(self) -> V3 {
            jg(self.0, |j| j.anchor)
        }

        pub fn set_anchor(self, a: V3) {
            jset(self.0, |j| j.anchor = a);
        }

        /// In the connected body's space (or the world's).
        #[property]
        pub fn connected_anchor(self) -> V3 {
            jg(self.0, |j| j.connected_anchor)
        }

        pub fn set_connected_anchor(self, a: V3) {
            jset(self.0, |j| j.connected_anchor = a);
        }

        #[property]
        pub fn auto_configure_connected_anchor(self) -> bool {
            with_joint(self.0, |j| j.auto_anchor).unwrap_or(true)
        }

        pub fn set_auto_configure_connected_anchor(self, on: bool) {
            jset(self.0, |j| j.auto_anchor = on);
        }

        pub fn destroy(self) {
            comp::joint_destroy(self.0);
        }
    }

    impl HingeJointRef {
        /// Adds a HingeJoint to a GameObject. A hinge about `axis` (doors, wheels, levers), with optional limits and a motor.
        pub fn attach(object: ObjectId) -> HingeJointRef {
            HingeJointRef(joint_attach(object, "hinge"))
        }

        /// The HingeJoint on an object, or none.
        pub fn find_on(object: ObjectId) -> HingeJointRef {
            HingeJointRef(find_joint(object, "hinge"))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The Rigidbody this joins to (none: the world).
        #[property]
        pub fn connected_body(self) -> RigidbodyRef {
            RigidbodyRef(jg(self.0, |j| j.connected))
        }

        pub fn set_connected_body(self, rb: RigidbodyRef) {
            jset(self.0, |j| j.connected = rb.0);
        }

        /// In this object's space.
        #[property]
        pub fn anchor(self) -> V3 {
            jg(self.0, |j| j.anchor)
        }

        pub fn set_anchor(self, a: V3) {
            jset(self.0, |j| j.anchor = a);
        }

        /// In the connected body's space (or the world's).
        #[property]
        pub fn connected_anchor(self) -> V3 {
            jg(self.0, |j| j.connected_anchor)
        }

        pub fn set_connected_anchor(self, a: V3) {
            jset(self.0, |j| j.connected_anchor = a);
        }

        #[property]
        pub fn auto_configure_connected_anchor(self) -> bool {
            with_joint(self.0, |j| j.auto_anchor).unwrap_or(true)
        }

        pub fn set_auto_configure_connected_anchor(self, on: bool) {
            jset(self.0, |j| j.auto_anchor = on);
        }

        pub fn destroy(self) {
            comp::joint_destroy(self.0);
        }

        #[property]
        pub fn axis(self) -> V3 {
            jg(self.0, |j| j.axis)
        }

        pub fn set_axis(self, a: V3) {
            jset(self.0, |j| j.axis = a);
        }

        #[property]
        pub fn use_limits(self) -> bool {
            jg(self.0, |j| j.use_limits)
        }

        pub fn set_use_limits(self, on: bool) {
            jset(self.0, |j| j.use_limits = on);
        }

        /// The hinge's range in degrees.
        #[property]
        pub fn limits_min(self) -> f64 {
            jg(self.0, |j| j.min)
        }

        pub fn set_limits_min(self, d: f64) {
            jset(self.0, |j| j.min = d);
        }

        #[property]
        pub fn limits_max(self) -> f64 {
            jg(self.0, |j| j.max)
        }

        pub fn set_limits_max(self, d: f64) {
            jset(self.0, |j| j.max = d);
        }

        #[property]
        pub fn use_motor(self) -> bool {
            jg(self.0, |j| j.use_motor)
        }

        pub fn set_use_motor(self, on: bool) {
            jset(self.0, |j| j.use_motor = on);
        }

        /// Degrees per second.
        #[property]
        pub fn motor_target_velocity(self) -> f64 {
            jg(self.0, |j| j.target_velocity)
        }

        pub fn set_motor_target_velocity(self, v: f64) {
            jset(self.0, |j| j.target_velocity = v);
        }

        /// How hard the motor pushes towards that speed.
        #[property]
        pub fn motor_force(self) -> f64 {
            jg(self.0, |j| j.force)
        }

        pub fn set_motor_force(self, f: f64) {
            jset(self.0, |j| j.force = f);
        }
    }

    /// A hinge in the XY plane (Unity's `HingeJoint2D`): this object's `Rigidbody2D` turns about z
    /// around `anchor`, a point in this object's space. `connectedBody` is another `Rigidbody2D`,
    /// or the world when none. `useLimits` keeps the angle between `limitsMin` and `limitsMax`
    /// degrees.
    impl HingeJoint2DRef {
        /// Adds a HingeJoint2D to a GameObject.
        pub fn attach(object: ObjectId) -> HingeJoint2DRef {
            HingeJoint2DRef(joint_attach(object, "hinge2d"))
        }

        /// The HingeJoint2D on an object, or none.
        pub fn find_on(object: ObjectId) -> HingeJoint2DRef {
            HingeJoint2DRef(find_joint(object, "hinge2d"))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The Rigidbody2D this joins to (none: the world).
        #[property]
        pub fn connected_body(self) -> Rigidbody2DRef {
            Rigidbody2DRef(jg(self.0, |j| j.connected))
        }

        pub fn set_connected_body(self, rb: Rigidbody2DRef) {
            jset(self.0, |j| j.connected = rb.0);
        }

        /// In this object's space (scaled by its scale).
        #[property]
        pub fn anchor(self) -> V2 {
            jg(self.0, |j| V2 {
                x: j.anchor.x,
                y: j.anchor.y,
            })
        }

        pub fn set_anchor(self, a: V2) {
            jset(self.0, |j| {
                j.anchor = V3 {
                    x: a.x,
                    y: a.y,
                    z: 0.0,
                }
            });
        }

        /// In the connected body's space (or the world's).
        #[property]
        pub fn connected_anchor(self) -> V2 {
            jg(self.0, |j| V2 {
                x: j.connected_anchor.x,
                y: j.connected_anchor.y,
            })
        }

        pub fn set_connected_anchor(self, a: V2) {
            jset(self.0, |j| {
                j.connected_anchor = V3 {
                    x: a.x,
                    y: a.y,
                    z: 0.0,
                }
            });
        }

        #[property]
        pub fn auto_configure_connected_anchor(self) -> bool {
            with_joint(self.0, |j| j.auto_anchor).unwrap_or(true)
        }

        pub fn set_auto_configure_connected_anchor(self, on: bool) {
            jset(self.0, |j| j.auto_anchor = on);
        }

        #[property]
        pub fn use_limits(self) -> bool {
            jg(self.0, |j| j.use_limits)
        }

        pub fn set_use_limits(self, on: bool) {
            jset(self.0, |j| j.use_limits = on);
        }

        /// The hinge's range in degrees.
        #[property]
        pub fn limits_min(self) -> f64 {
            jg(self.0, |j| j.min)
        }

        pub fn set_limits_min(self, d: f64) {
            jset(self.0, |j| j.min = d);
        }

        #[property]
        pub fn limits_max(self) -> f64 {
            jg(self.0, |j| j.max)
        }

        pub fn set_limits_max(self, d: f64) {
            jset(self.0, |j| j.max = d);
        }

        #[property]
        pub fn use_motor(self) -> bool {
            jg(self.0, |j| j.use_motor)
        }

        pub fn set_use_motor(self, on: bool) {
            jset(self.0, |j| j.use_motor = on);
        }

        /// Degrees per second.
        #[property]
        pub fn motor_target_velocity(self) -> f64 {
            jg(self.0, |j| j.target_velocity)
        }

        pub fn set_motor_target_velocity(self, v: f64) {
            jset(self.0, |j| j.target_velocity = v);
        }

        /// How hard the motor pushes towards that speed.
        #[property]
        pub fn motor_force(self) -> f64 {
            jg(self.0, |j| j.force)
        }

        pub fn set_motor_force(self, f: f64) {
            jset(self.0, |j| j.force = f);
        }

        pub fn destroy(self) {
            comp::joint_destroy(self.0);
        }
    }

    impl SpringJointRef {
        /// Adds a SpringJoint to a GameObject. Pulls the bodies towards `restLength` apart like a spring.
        pub fn attach(object: ObjectId) -> SpringJointRef {
            SpringJointRef(joint_attach(object, "spring"))
        }

        /// The SpringJoint on an object, or none.
        pub fn find_on(object: ObjectId) -> SpringJointRef {
            SpringJointRef(find_joint(object, "spring"))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The Rigidbody this joins to (none: the world).
        #[property]
        pub fn connected_body(self) -> RigidbodyRef {
            RigidbodyRef(jg(self.0, |j| j.connected))
        }

        pub fn set_connected_body(self, rb: RigidbodyRef) {
            jset(self.0, |j| j.connected = rb.0);
        }

        /// In this object's space.
        #[property]
        pub fn anchor(self) -> V3 {
            jg(self.0, |j| j.anchor)
        }

        pub fn set_anchor(self, a: V3) {
            jset(self.0, |j| j.anchor = a);
        }

        /// In the connected body's space (or the world's).
        #[property]
        pub fn connected_anchor(self) -> V3 {
            jg(self.0, |j| j.connected_anchor)
        }

        pub fn set_connected_anchor(self, a: V3) {
            jset(self.0, |j| j.connected_anchor = a);
        }

        #[property]
        pub fn auto_configure_connected_anchor(self) -> bool {
            with_joint(self.0, |j| j.auto_anchor).unwrap_or(true)
        }

        pub fn set_auto_configure_connected_anchor(self, on: bool) {
            jset(self.0, |j| j.auto_anchor = on);
        }

        pub fn destroy(self) {
            comp::joint_destroy(self.0);
        }

        /// Stiffness.
        #[property]
        pub fn spring(self) -> f64 {
            jg(self.0, |j| j.spring)
        }

        pub fn set_spring(self, k: f64) {
            jset(self.0, |j| j.spring = k);
        }

        /// Damping.
        #[property]
        pub fn damper(self) -> f64 {
            jg(self.0, |j| j.damper)
        }

        pub fn set_damper(self, d: f64) {
            jset(self.0, |j| j.damper = d);
        }

        /// The length it rests at; NaN (the default) means the distance when it is made.
        #[property]
        pub fn rest_length(self) -> f64 {
            with_joint(self.0, |j| j.rest_length).unwrap_or(f64::NAN)
        }

        pub fn set_rest_length(self, l: f64) {
            jset(self.0, |j| j.rest_length = l);
        }
    }

    impl RopeJointRef {
        /// Adds a RopeJoint to a GameObject. Keeps the bodies at most `maxDistance` apart, slack within it: a rope without segments.
        pub fn attach(object: ObjectId) -> RopeJointRef {
            RopeJointRef(joint_attach(object, "rope"))
        }

        /// The RopeJoint on an object, or none.
        pub fn find_on(object: ObjectId) -> RopeJointRef {
            RopeJointRef(find_joint(object, "rope"))
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// The Rigidbody this joins to (none: the world).
        #[property]
        pub fn connected_body(self) -> RigidbodyRef {
            RigidbodyRef(jg(self.0, |j| j.connected))
        }

        pub fn set_connected_body(self, rb: RigidbodyRef) {
            jset(self.0, |j| j.connected = rb.0);
        }

        /// In this object's space.
        #[property]
        pub fn anchor(self) -> V3 {
            jg(self.0, |j| j.anchor)
        }

        pub fn set_anchor(self, a: V3) {
            jset(self.0, |j| j.anchor = a);
        }

        /// In the connected body's space (or the world's).
        #[property]
        pub fn connected_anchor(self) -> V3 {
            jg(self.0, |j| j.connected_anchor)
        }

        pub fn set_connected_anchor(self, a: V3) {
            jset(self.0, |j| j.connected_anchor = a);
        }

        #[property]
        pub fn auto_configure_connected_anchor(self) -> bool {
            with_joint(self.0, |j| j.auto_anchor).unwrap_or(true)
        }

        pub fn set_auto_configure_connected_anchor(self, on: bool) {
            jset(self.0, |j| j.auto_anchor = on);
        }

        pub fn destroy(self) {
            comp::joint_destroy(self.0);
        }

        #[property]
        pub fn max_distance(self) -> f64 {
            jg(self.0, |j| j.max_distance)
        }

        pub fn set_max_distance(self, d: f64) {
            jset(self.0, |j| j.max_distance = d);
        }
    }

    impl RopeRef {
        /// Adds a rope to a GameObject: a chain of capsule segments with Rigidbodies joined by
        /// ball joints, hanging from this object (its Rigidbody, or the world where it is) to
        /// `end` (a Rigidbody it holds, optional). Made on the first `step`.
        pub fn attach(object: ObjectId) -> RopeRef {
            RopeRef(comp::add_comp(
                object,
                Data::Rope(RopeD {
                    length: 3.0,
                    segments: 12.0,
                    radius: 0.04,
                    segment_mass: 0.05,
                    end: 0,
                    links: Vec::new(),
                    joints: Vec::new(),
                    started: false,
                }),
            ))
        }

        pub fn find_on(object: ObjectId) -> RopeRef {
            RopeRef(
                comp::comps_on(object, |d| matches!(d, Data::Rope(_)))
                    .first()
                    .copied()
                    .unwrap_or(0),
            )
        }

        #[property]
        pub fn game_object(self) -> ObjectId {
            comp::comp_object(self.0)
        }

        /// Total length in metres.
        #[property]
        pub fn length(self) -> f64 {
            rg(self.0, |r| r.length)
        }

        pub fn set_length(self, l: f64) {
            with_rope(self.0, |r| r.length = l);
        }

        #[property]
        pub fn segments(self) -> f64 {
            rg(self.0, |r| r.segments)
        }

        pub fn set_segments(self, n: f64) {
            with_rope(self.0, |r| r.segments = n);
        }

        #[property]
        pub fn radius(self) -> f64 {
            rg(self.0, |r| r.radius)
        }

        pub fn set_radius(self, r: f64) {
            with_rope(self.0, |x| x.radius = r);
        }

        /// Each segment's mass in kg.
        #[property]
        pub fn segment_mass(self) -> f64 {
            rg(self.0, |r| r.segment_mass)
        }

        pub fn set_segment_mass(self, m: f64) {
            with_rope(self.0, |r| r.segment_mass = m);
        }

        /// What the far end holds, if anything.
        #[property]
        pub fn end(self) -> RigidbodyRef {
            RigidbodyRef(rg(self.0, |r| r.end))
        }

        pub fn set_end(self, rb: RigidbodyRef) {
            with_rope(self.0, |r| r.end = rb.0);
        }

        /// How many segment objects the rope has made (they hang in the world).
        #[property]
        pub fn link_count(self) -> u32 {
            rg(self.0, |r| r.links.len() as u32)
        }

        pub fn link_at(self, i: u32) -> ObjectId {
            with_rope(self.0, |r| r.links.get(i as usize).copied())
                .flatten()
                .unwrap_or(ObjectId {
                    index: u32::MAX,
                    generation: u32::MAX,
                })
        }

        /// Where the rope runs: the first link's top, then every link's bottom.
        #[property]
        pub fn point_count(self) -> u32 {
            comp::rope_points(self.0).len() as u32
        }

        pub fn point_at(self, i: u32) -> V3 {
            comp::rope_points(self.0)
                .get(i as usize)
                .copied()
                .unwrap_or_default()
        }

        pub fn destroy(self) {
            comp::rope_destroy(self.0);
        }
    }
}
