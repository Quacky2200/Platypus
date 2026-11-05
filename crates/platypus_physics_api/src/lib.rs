// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Physics for compiled scripts: rigid bodies, colliders and ray queries over `platypus_physics`,
//! declared with `#[ts_api]` (design: `docs/research/platypus-native-physics.md`).
//!
//! Bodies and colliders are numbers chosen here; the world is one per thread. The host calls
//! [`step`] once per fixed tick: it advances the world, moves the GameObjects bodies are attached
//! to, and queues the contacts for [`take_contacts`] (the host turns them into
//! `onCollisionEnter` and friends). A ray answers at once.

use platypus_core::Value;
use platypus_math::{Q, V3};
use platypus_physics::{Contact, Physics};
use platypus_scene::ObjectId;
use platypus_scene::api::TransformRef;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use ts_api::{Handle, ts_api};

mod comp;
pub mod components;
pub mod contacts;
pub mod joints;
mod kind_fields;
mod kinds;
pub mod physics2d;
#[cfg(test)]
mod tests;

pub use comp::{ColliderInfo, object_destroyed, start_pending};
pub use kinds::{component_objects, register_kinds};

macro_rules! number_handle {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct $name(pub u32);
        impl Handle for $name {
            fn to_raw(self) -> u64 {
                u64::from(self.0)
            }
            fn from_raw(raw: u64) -> Self {
                $name(raw as u32)
            }
        }
    )*};
}

// 0 is "none" in each (a script's `Body | null`).
number_handle!(
    BodyRef,
    ColliderRef,
    HitRef,
    JointRef,
    RigidbodyRef,
    BoxColliderRef,
    SphereColliderRef,
    CapsuleColliderRef,
    MeshColliderRef,
    CharacterControllerRef,
    FixedJointRef,
    CharacterJointRef,
    HingeJointRef,
    SpringJointRef,
    RopeJointRef,
    RopeRef,
    Rigidbody2DRef,
    BoxCollider2DRef,
    CircleCollider2DRef,
    CapsuleCollider2DRef,
    PolygonCollider2DRef,
    EdgeCollider2DRef,
    Hit2DRef
);

pub(crate) struct HitData {
    pub collider: u32,
    pub object: Option<ObjectId>,
    pub distance: f64,
    pub point: V3,
    pub normal: V3,
}

struct State {
    physics: Physics,
    next_body: u32,
    next_collider: u32,
    /// The GameObject each body moves.
    attached: HashMap<u32, ObjectId>,
    /// The pose each attached object had when the world last wrote it (or read it): the object
    /// moved by a script when it differs.
    synced: HashMap<u32, (glam::Vec3, glam::Quat)>,
    /// Bodies whose character controller ended its last move on the ground.
    grounded: HashSet<u32>,
    /// The colliders of each body (the body's removal drops them).
    owner: HashMap<u32, u32>,
    kinds: HashMap<u32, String>,
    /// Ray hits handed out since the last step.
    pub(crate) hits: Vec<HitData>,
    contacts: Vec<Contact>,
    /// The collisions handed to contact hooks since the last dispatch (`Collision` handles).
    pub(crate) collisions: Vec<contacts::Slot>,
    /// Contact points of those collisions, read from the world when a script first asks.
    pub(crate) points: Vec<platypus_physics::ContactPoint>,
    /// Physics components (a handle is the place plus one).
    comps: Vec<Option<comp::Comp>>,
    /// The body, Rigidbody and colliders of each GameObject with physics components.
    objects: HashMap<ObjectId, comp::ObjPhys>,
    /// The collider component of each rapier collider it made.
    col_owner: HashMap<u32, u32>,
    next_joint: u32,
    /// How deep 2D colliders are along z (`Physics2D.depth`).
    depth_2d: f64,
    gravity_2d: [f64; 2],
}

impl State {
    fn new() -> Self {
        State {
            physics: Physics::new(),
            next_body: 1,
            next_collider: 1,
            attached: HashMap::new(),
            synced: HashMap::new(),
            grounded: HashSet::new(),
            owner: HashMap::new(),
            kinds: HashMap::new(),
            hits: Vec::new(),
            contacts: Vec::new(),
            collisions: Vec::new(),
            points: Vec::new(),
            comps: Vec::new(),
            objects: HashMap::new(),
            col_owner: HashMap::new(),
            next_joint: 0,
            depth_2d: 10.0,
            gravity_2d: [0.0, -9.81],
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::new());
}

pub(crate) fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// A new empty world (a new game, tests).
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::new());
    AFTER_STEP.with(|a| a.borrow_mut().clear());
}

/// Runs a `physics.*` op; a bad one is reported and ignored (a script cannot do anything with it).
pub(crate) fn op(name: &str, args: Vec<Value>) {
    if let Err(e) = st(|s| s.physics.apply(name, &args)) {
        eprintln!("physics.{name}: {e}");
    }
}

pub(crate) fn num(x: f64) -> Value {
    Value::Number(x)
}

pub(crate) fn v3(v: V3) -> Value {
    Value::Array(vec![num(v.x), num(v.y), num(v.z)])
}

fn quat(q: Q) -> Value {
    Value::Array(vec![num(q.x), num(q.y), num(q.z), num(q.w)])
}

pub(crate) fn v3_from_f32(a: [f32; 3]) -> V3 {
    from3(a)
}

fn from3(a: [f32; 3]) -> V3 {
    V3 {
        x: f64::from(a[0]),
        y: f64::from(a[1]),
        z: f64::from(a[2]),
    }
}

/// Casts a ray at once, on every layer; the number is the hit (0: none), read through the
/// `RaycastHit` handle.
pub(crate) fn do_raycast(origin: V3, direction: V3, max_distance: f64) -> u32 {
    do_raycast_masked(origin, direction, max_distance, u32::MAX)
}

/// `do_raycast` for the colliders whose GameObject's layer is in `layer_mask` (a bit per layer).
pub(crate) fn do_raycast_masked(
    origin: V3,
    direction: V3,
    max_distance: f64,
    layer_mask: u32,
) -> u32 {
    st(|s| {
        let accept = |collider: u32| on_layers(s, collider, layer_mask);
        let hit = s.physics.raycast_now(
            [origin.x as f32, origin.y as f32, origin.z as f32],
            [direction.x as f32, direction.y as f32, direction.z as f32],
            max_distance.min(f64::from(f32::MAX)) as f32,
            &accept,
        );
        match hit {
            Some(hit) => push_hit(s, origin, direction, hit),
            None => 0,
        }
    })
}

/// `do_raycast_masked` for every collider on the ray, nearest first: the hit numbers.
pub(crate) fn do_raycast_all(
    origin: V3,
    direction: V3,
    max_distance: f64,
    layer_mask: u32,
) -> Vec<u32> {
    st(|s| {
        let accept = |collider: u32| on_layers(s, collider, layer_mask);
        let hits = s.physics.raycast_all(
            [origin.x as f32, origin.y as f32, origin.z as f32],
            [direction.x as f32, direction.y as f32, direction.z as f32],
            max_distance.min(f64::from(f32::MAX)) as f32,
            &accept,
        );
        hits.into_iter()
            .map(|hit| push_hit(s, origin, direction, hit))
            .collect()
    })
}

/// The colliders a sphere touches whose GameObject's layer is in `layer_mask`.
pub(crate) fn do_overlap_sphere(center: V3, radius: f64, layer_mask: u32) -> Vec<u32> {
    st(|s| {
        let accept = |collider: u32| on_layers(s, collider, layer_mask);
        s.physics.overlap_sphere(
            [center.x as f32, center.y as f32, center.z as f32],
            radius as f32,
            &accept,
        )
    })
}

/// Is the collider's GameObject (layer 0 without one) on a layer of `layer_mask`?
fn on_layers(s: &State, collider: u32, layer_mask: u32) -> bool {
    let layer = object_in(s, collider).map_or(0, layer_of);
    layer_mask & (1 << layer) != 0
}

/// Keeps a ray hit for the script to read; the number is the hit (never 0).
fn push_hit(
    s: &mut State,
    origin: V3,
    direction: V3,
    (collider, distance, normal): (u32, f32, [f32; 3]),
) -> u32 {
    let len =
        (direction.x * direction.x + direction.y * direction.y + direction.z * direction.z).sqrt();
    let d = f64::from(distance);
    let point = V3 {
        x: origin.x + direction.x / len * d,
        y: origin.y + direction.y / len * d,
        z: origin.z + direction.z / len * d,
    };
    let object = object_in(s, collider);
    s.hits.push(HitData {
        collider,
        object,
        distance: d,
        point,
        normal: from3(normal),
    });
    s.hits.len() as u32
}

/// The layer (0 to 31) of a GameObject; 0 (Default) when there is no scene.
fn layer_of(object: ObjectId) -> u32 {
    platypus_scene::api::with_scene(|sc| sc.layer(object)).map_or(0, u32::from)
}

/// A `force` op on a body: `"force"`, `"impulse"`, `"torque"`, `"torqueImpulse"` or `"impulseAt"`
/// (with the world point).
pub(crate) fn force_op(body: u32, what: &str, v: V3, point: Option<V3>) {
    let mut args = vec![num(f64::from(body)), Value::String(what.into()), v3(v)];
    if let Some(p) = point {
        args.push(v3(p));
    }
    op("force", args);
}

/// Advances the world one fixed tick, moves the objects bodies are attached to, and keeps the
/// contacts for [`take_contacts`].
pub fn step() {
    step_world();
    // What was asked to run after the next step runs now, once.
    let callbacks = AFTER_STEP.with(|a| std::mem::take(&mut *a.borrow_mut()));
    ts_api::callback::emit(&callbacks, ());
}

thread_local! {
    /// Closures `Physics.afterNextStep` was given, run once after the next step.
    static AFTER_STEP: std::cell::RefCell<Vec<ts_api::Callback<()>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

fn step_world() {
    comp::start_pending();
    comp::sync_from_scene();
    let (moves, grounded_now, contacts) = st(|s| {
        s.hits.clear();
        s.physics.step();
        let batch = s.physics.take_batch();
        let moves: Vec<(u32, ObjectId, [f32; 3], [f32; 4])> = batch
            .bodies
            .iter()
            .filter_map(|b| Some((b.id, *s.attached.get(&b.id)?, b.position, b.rotation)))
            .collect();
        (moves, batch.characters, batch.contacts)
    });
    let written = platypus_scene::api::with_scene(|scene| {
        let mut written = Vec::new();
        for (body, o, p, r) in moves {
            let (p, r) = (
                glam::Vec3::from_array(p),
                glam::Quat::from_xyzw(r[0], r[1], r[2], r[3]),
            );
            scene.set_world(o, p, r);
            let w = scene.world(o);
            written.push((body, (w.position, w.rotation)));
        }
        written
    })
    .unwrap_or_default();
    st(|s| s.synced.extend(written));
    st(|s| {
        for (id, g) in grounded_now {
            if g {
                s.grounded.insert(id);
            } else {
                s.grounded.remove(&id);
            }
        }
        s.contacts.extend(contacts);
    });
    comp::after_step();
}

/// The contacts since the last call, by collider number.
pub fn take_contacts() -> Vec<Contact> {
    st(|s| std::mem::take(&mut s.contacts))
}

/// The GameObject a collider's body moves, if it is attached to one.
pub fn object_of_collider(collider: u32) -> Option<ObjectId> {
    st(|s| object_in(s, collider))
}

/// `object_of_collider` on a state already borrowed: the object of the collider component, or
/// else of the body the collider is on.
fn object_in(s: &State, collider: u32) -> Option<ObjectId> {
    if let Some(&comp) = s.col_owner.get(&collider) {
        return s
            .comps
            .get((comp as usize).checked_sub(1)?)?
            .as_ref()
            .map(|c| c.object);
    }
    let body = s.physics.collider_body(collider)?;
    s.attached.get(&body).copied()
}

fn add_collider(
    body: u32,
    shape: &'static str,
    dims: Vec<f64>,
    indices: Option<Vec<f64>>,
) -> ColliderRef {
    ColliderRef(comp::add_collider_full(comp::ColliderArgs {
        body,
        name: shape,
        dims,
        indices,
        position: V3::ZERO,
        rotation: Q::IDENTITY,
        friction: 0.5,
        restitution: 0.0,
        sensor: false,
    }))
}

/// A new body of that kind at the origin.
pub(crate) fn new_body(kind: &str) -> u32 {
    let id = st(|s| {
        let id = s.next_body;
        s.next_body += 1;
        s.kinds.insert(id, kind.to_owned());
        id
    });
    op(
        "body",
        vec![
            num(f64::from(id)),
            Value::String(kind.to_owned()),
            Value::Array(vec![num(0.0); 3]),
            Value::Array(vec![num(0.0), num(0.0), num(0.0), num(1.0)]),
            Value::Array(vec![num(0.0); 3]),
            Value::Array(vec![
                num(1.0),
                num(0.0),
                num(0.0),
                Value::Bool(false),
                Value::Bool(false),
            ]),
        ],
    );
    id
}

/// Drops a body and its colliders.
pub(crate) fn remove_body(id: u32) {
    st(|s| {
        s.attached.remove(&id);
        s.kinds.remove(&id);
        s.grounded.remove(&id);
        let gone: Vec<u32> = s
            .owner
            .iter()
            .filter(|(_, b)| **b == id)
            .map(|(c, _)| *c)
            .collect();
        for c in gone {
            s.owner.remove(&c);
            s.col_owner.remove(&c);
        }
    });
    op("removeBody", vec![num(f64::from(id))]);
}

fn flat(points: &[V3]) -> Vec<f64> {
    points.iter().flat_map(|p| [p.x, p.y, p.z]).collect()
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[ts_api(
    namespace = "Physics", handles(
        PhysicsBody = BodyRef,
        PhysicsCollider = ColliderRef,
        RaycastHit = HitRef,
        PhysicsJoint = JointRef,
        Rigidbody = RigidbodyRef,
        GameObject = ObjectId,
        Transform = TransformRef
    ),
    values(Vector3 = V3 [x: f64, y: f64, z: f64], Quaternion = Q [x: f64, y: f64, z: f64, w: f64])
)]
pub mod physics_api {
    use super::*;
    use ts_api::Callback;

    /// Gravity in m/s² (y up; default 0, -9.81, 0).
    #[property]
    pub fn gravity() -> V3 {
        from3(st(|s| s.physics.gravity()))
    }

    pub fn set_gravity(g: V3) {
        op("gravity", vec![v3(g)]);
    }

    /// A body: `"dynamic"` (simulated), `"fixed"` (never moves) or `"kinematic"` (moved by script).
    pub fn create_body(kind: &str) -> BodyRef {
        BodyRef(new_body(kind))
    }

    /// A box collider on the world itself (floors, walls): half extents.
    pub fn add_box(half: V3) -> ColliderRef {
        add_collider(0, "box", vec![half.x, half.y, half.z], None)
    }

    pub fn add_ball(radius: f64) -> ColliderRef {
        add_collider(0, "ball", vec![radius], None)
    }

    /// Static level geometry from triangles: points and the indices of each triangle.
    pub fn add_mesh(points: &[V3], indices: &[f64]) -> ColliderRef {
        add_collider(0, "mesh", flat(points), Some(indices.to_vec()))
    }

    /// Casts a ray at once and answers the first collider hit within `maxDistance` (unlimited by
    /// default, as in Unity), or no hit.
    /// `layerMask` (a bit per layer, `LayerMask.getMask`) keeps the colliders whose GameObject
    /// is on those layers; a collider the origin is inside is not hit (as in Unity).
    #[optional(max_distance = 1e30, layer_mask = 4294967295)]
    #[nullable]
    pub fn raycast(origin: V3, direction: V3, max_distance: f64, layer_mask: u32) -> HitRef {
        HitRef(do_raycast_masked(
            origin,
            direction,
            max_distance,
            layer_mask,
        ))
    }

    /// Casts a ray at once and answers every collider it hits within `maxDistance` (unlimited by
    /// default), nearest first. Unity does not order them; this does. `layerMask` and the
    /// start-inside rule are those of `raycast`.
    #[optional(max_distance = 1e30, layer_mask = 4294967295)]
    pub fn raycast_all(
        origin: V3,
        direction: V3,
        max_distance: f64,
        layer_mask: u32,
    ) -> Vec<HitRef> {
        do_raycast_all(origin, direction, max_distance, layer_mask)
            .into_iter()
            .map(HitRef)
            .collect()
    }

    /// Every collider a sphere touches or is inside (triggers too), by creation order.
    /// `layerMask` keeps the colliders whose GameObject is on those layers.
    #[optional(layer_mask = 4294967295)]
    pub fn overlap_sphere(center: V3, radius: f64, layer_mask: u32) -> Vec<ColliderRef> {
        do_overlap_sphere(center, radius, layer_mask)
            .into_iter()
            .map(ColliderRef)
            .collect()
    }

    /// Joins two bodies (`b` 0: to the world). Kinds and `params`: `"fixed"`; `"ball"` (turns
    /// freely about the anchor); `"hinge"` [axis x y z, use limits (0/1), min, max (degrees),
    /// use motor (0/1), speed (degrees per second), strength]; `"spring"` [rest length,
    /// stiffness, damping]; `"rope"` [max length]. Anchors are in each body's space (the
    /// world's for no body). Joined bodies do not collide with each other.
    pub fn create_joint(
        kind: &str,
        a: BodyRef,
        b: BodyRef,
        anchor_a: V3,
        anchor_b: V3,
        params: &[f64],
    ) -> JointRef {
        JointRef(comp::create_joint_raw(
            kind, a.0, b.0, anchor_a, anchor_b, params,
        ))
    }

    /// Advances the world one fixed tick (the host does this once per tick).
    pub fn step() {
        super::step();
    }

    /// Calls `f` once, after the next physics step (the next fixed tick), then forgets it: what a
    /// physics query that "resolves after the next tick" waits for.
    pub fn after_next_step(f: Callback<()>) {
        super::AFTER_STEP.with(|a| a.borrow_mut().push(f));
    }

    impl BodyRef {
        /// Where it was after the last tick (or where it was last set).
        #[property]
        pub fn position(self) -> V3 {
            from3(st(|s| s.physics.position(self.0)).unwrap_or_default())
        }

        pub fn set_position(self, p: V3) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("position".into()),
                    v3(p),
                ],
            );
        }

        #[property]
        pub fn rotation(self) -> Q {
            let r = st(|s| s.physics.pose(self.0)).map_or([0.0, 0.0, 0.0, 1.0], |p| p.1);
            Q {
                x: f64::from(r[0]),
                y: f64::from(r[1]),
                z: f64::from(r[2]),
                w: f64::from(r[3]),
            }
        }

        pub fn set_rotation(self, q: Q) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("rotation".into()),
                    quat(q),
                ],
            );
        }

        #[property]
        pub fn velocity(self) -> V3 {
            from3(st(|s| s.physics.velocity(self.0)).unwrap_or_default())
        }

        pub fn set_velocity(self, v: V3) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("velocity".into()),
                    v3(v),
                ],
            );
        }

        /// Radians per second about each world axis.
        #[property]
        pub fn angular_velocity(self) -> V3 {
            from3(st(|s| s.physics.angular_velocity(self.0)).unwrap_or_default())
        }

        pub fn set_angular_velocity(self, v: V3) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("angularVelocity".into()),
                    v3(v),
                ],
            );
        }

        /// A character controller's ground contact after its last `move`.
        #[property]
        pub fn grounded(self) -> bool {
            st(|s| s.grounded.contains(&self.0))
        }

        /// A kinematic body's position at the end of the next tick (it pushes things on the way).
        pub fn move_to(self, p: V3) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("target".into()),
                    v3(p),
                ],
            );
        }

        /// An instant change in momentum (N·s): jumps, hits, explosions.
        pub fn apply_impulse(self, impulse: V3) {
            op(
                "force",
                vec![
                    num(f64::from(self.0)),
                    Value::String("impulse".into()),
                    v3(impulse),
                ],
            );
        }

        pub fn apply_torque_impulse(self, impulse: V3) {
            op(
                "force",
                vec![
                    num(f64::from(self.0)),
                    Value::String("torqueImpulse".into()),
                    v3(impulse),
                ],
            );
        }

        /// A force (N) applied every tick until `clearForces()`.
        pub fn add_force(self, force: V3) {
            op(
                "force",
                vec![
                    num(f64::from(self.0)),
                    Value::String("force".into()),
                    v3(force),
                ],
            );
        }

        pub fn add_torque(self, torque: V3) {
            op(
                "force",
                vec![
                    num(f64::from(self.0)),
                    Value::String("torque".into()),
                    v3(torque),
                ],
            );
        }

        pub fn clear_forces(self) {
            op("clearForces", vec![num(f64::from(self.0))]);
        }

        /// An instant impulse at a world point (it also spins the body).
        pub fn apply_impulse_at_point(self, impulse: V3, point: V3) {
            op(
                "force",
                vec![
                    num(f64::from(self.0)),
                    Value::String("impulseAt".into()),
                    v3(impulse),
                    v3(point),
                ],
            );
        }

        /// Total mass in kg (shared by its colliders).
        pub fn set_mass(self, kg: f64) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("mass".into()),
                    num(kg),
                ],
            );
        }

        pub fn set_gravity_scale(self, scale: f64) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("gravityScale".into()),
                    num(scale),
                ],
            );
        }

        pub fn set_damping(self, linear: f64, angular: f64) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("linearDamping".into()),
                    num(linear),
                ],
            );
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("angularDamping".into()),
                    num(angular),
                ],
            );
        }

        pub fn set_ccd(self, on: bool) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("ccd".into()),
                    Value::Bool(on),
                ],
            );
        }

        /// Keeps the body upright (no rotation about any axis).
        pub fn lock_rotation(self, locked: bool) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("constraints".into()),
                    Value::Array(vec![
                        Value::Bool(true),
                        Value::Bool(true),
                        Value::Bool(true),
                        Value::Bool(!locked),
                        Value::Bool(!locked),
                        Value::Bool(!locked),
                    ]),
                ],
            );
        }

        /// Switches between simulated (`"dynamic"`), immovable (`"fixed"`) and script-moved
        /// (`"kinematic"`).
        pub fn set_kind(self, kind: &str) {
            st(|s| s.kinds.insert(self.0, kind.to_owned()));
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("kind".into()),
                    Value::String(kind.to_owned()),
                ],
            );
        }

        #[property]
        pub fn kind(self) -> String {
            st(|s| s.kinds.get(&self.0).cloned().unwrap_or_default())
        }

        /// Disabled bodies are left out of the simulation (and keep their state).
        pub fn set_enabled(self, enabled: bool) {
            op(
                "set",
                vec![
                    num(f64::from(self.0)),
                    Value::String("enabled".into()),
                    Value::Bool(enabled),
                ],
            );
        }

        pub fn sleep(self) {
            op(
                "set",
                vec![num(f64::from(self.0)), Value::String("sleep".into())],
            );
        }

        pub fn wake_up(self) {
            op(
                "set",
                vec![num(f64::from(self.0)), Value::String("wake".into())],
            );
        }

        /// Makes the body move a GameObject each tick.
        pub fn attach_to(self, object: ObjectId) {
            st(|s| s.attached.insert(self.0, object));
        }

        /// A box collider (half extents).
        pub fn add_box(self, half: V3) -> ColliderRef {
            add_collider(self.0, "box", vec![half.x, half.y, half.z], None)
        }

        pub fn add_ball(self, radius: f64) -> ColliderRef {
            add_collider(self.0, "ball", vec![radius], None)
        }

        /// Along y.
        pub fn add_capsule(self, half_height: f64, radius: f64) -> ColliderRef {
            add_collider(self.0, "capsule", vec![half_height, radius], None)
        }

        pub fn add_cylinder(self, half_height: f64, radius: f64) -> ColliderRef {
            add_collider(self.0, "cylinder", vec![half_height, radius], None)
        }

        /// The convex hull of these points.
        pub fn add_convex(self, points: &[V3]) -> ColliderRef {
            add_collider(self.0, "convex", flat(points), None)
        }

        /// Makes the body a character: `move` then slides it along walls, climbs slopes and
        /// steps. Use a kinematic body with a capsule collider.
        #[optional(max_slope = 45, snap_to_ground, step_height)]
        pub fn make_character(self, max_slope: f64, snap_to_ground: f64, step_height: f64) {
            let opt = |x: f64| if x.is_nan() { Value::Null } else { num(x) };
            op(
                "character",
                vec![
                    num(f64::from(self.0)),
                    num(max_slope),
                    opt(snap_to_ground),
                    opt(step_height),
                ],
            );
        }

        /// Moves a character by `delta` metres this tick (moves in the same tick add up).
        pub fn move_by(self, delta: V3) {
            op("move", vec![num(f64::from(self.0)), v3(delta)]);
        }

        /// Removes the body and its colliders.
        pub fn remove(self) {
            remove_body(self.0);
        }
    }

    impl ColliderRef {
        pub fn set_friction(self, friction: f64) {
            op(
                "colliderSet",
                vec![
                    num(f64::from(self.0)),
                    Value::String("friction".into()),
                    num(friction),
                ],
            );
        }

        /// Bounciness, 0 to 1.
        pub fn set_restitution(self, restitution: f64) {
            op(
                "colliderSet",
                vec![
                    num(f64::from(self.0)),
                    Value::String("restitution".into()),
                    num(restitution),
                ],
            );
        }

        /// A trigger: reports overlaps but does not push anything.
        pub fn set_sensor(self, sensor: bool) {
            op(
                "colliderSet",
                vec![
                    num(f64::from(self.0)),
                    Value::String("sensor".into()),
                    Value::Bool(sensor),
                ],
            );
        }

        /// The body it is on (no body for a world collider).
        #[property]
        pub fn body(self) -> BodyRef {
            BodyRef(st(|s| s.owner.get(&self.0).copied()).unwrap_or(0))
        }

        /// The GameObject the collider belongs to (none for world geometry without one).
        #[property]
        #[nullable]
        pub fn game_object(self) -> ObjectId {
            object_of_collider(self.0).unwrap_or_else(none_object)
        }

        /// The Transform of the collider's GameObject.
        #[property]
        #[nullable]
        pub fn transform(self) -> TransformRef {
            TransformRef(object_of_collider(self.0).unwrap_or_else(none_object))
        }

        pub fn remove(self) {
            comp::remove_collider_id(self.0);
        }
    }

    impl JointRef {
        /// Removes the joint.
        pub fn remove(self) {
            comp::remove_joint_id(self.0);
        }
    }

    impl HitRef {
        /// The collider that was hit.
        #[property]
        pub fn collider(self) -> ColliderRef {
            ColliderRef(
                st(|s| {
                    s.hits
                        .get((self.0 as usize).wrapping_sub(1))
                        .map(|h| h.collider)
                })
                .unwrap_or(0),
            )
        }

        /// Metres along the ray.
        #[property]
        pub fn distance(self) -> f64 {
            st(|s| {
                s.hits
                    .get((self.0 as usize).wrapping_sub(1))
                    .map(|h| h.distance)
            })
            .unwrap_or(0.0)
        }

        #[property]
        pub fn point(self) -> V3 {
            st(|s| {
                s.hits
                    .get((self.0 as usize).wrapping_sub(1))
                    .map(|h| h.point)
            })
            .unwrap_or_default()
        }

        #[property]
        pub fn normal(self) -> V3 {
            st(|s| {
                s.hits
                    .get((self.0 as usize).wrapping_sub(1))
                    .map(|h| h.normal)
            })
            .unwrap_or_default()
        }

        /// The GameObject hit (none for a collider made without one, such as world geometry).
        #[property]
        #[nullable]
        pub fn game_object(self) -> ObjectId {
            hit_object(self).unwrap_or_else(none_object)
        }

        /// The hit GameObject's Transform.
        #[property]
        #[nullable]
        pub fn transform(self) -> TransformRef {
            TransformRef(hit_object(self).unwrap_or_else(none_object))
        }

        /// The Rigidbody the hit collider is on (none without one).
        #[property]
        #[nullable]
        pub fn rigidbody(self) -> RigidbodyRef {
            let (collider, object) = st(|s| {
                s.hits
                    .get((self.0 as usize).wrapping_sub(1))
                    .map_or((0, None), |h| (h.collider, h.object))
            });
            RigidbodyRef(crate::contacts::rigidbody_of(
                collider,
                object.unwrap_or_else(none_object),
                false,
            ))
        }
    }
}

fn hit_object(hit: HitRef) -> Option<ObjectId> {
    st(|s| {
        s.hits
            .get((hit.0 as usize).wrapping_sub(1))
            .and_then(|h| h.object)
    })
}

fn none_object() -> ObjectId {
    ObjectId::from_raw(0)
}

/// The manifests of this crate's script API.
pub fn manifests() -> Vec<String> {
    vec![
        physics_api::MANIFEST.to_json(),
        components::components_api::MANIFEST.to_json(),
        components::force_mode_api::MANIFEST.to_json(),
        components::constraints_api::MANIFEST.to_json(),
        joints::joints_api::MANIFEST.to_json(),
        physics2d::physics2d_api::MANIFEST.to_json(),
        contacts::contacts_api::MANIFEST.to_json(),
    ]
}

/// A body `set` op with no value (`"sleep"`, `"wake"`) or one.
pub(crate) fn body_set(body: u32, what: &str, value: Option<Value>) {
    let mut args = vec![num(f64::from(body)), Value::String(what.into())];
    args.extend(value);
    op("set", args);
}
