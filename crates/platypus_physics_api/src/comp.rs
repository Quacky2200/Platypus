// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The physics components (`Rigidbody`, the colliders, joints, `Rope`, and their 2D twins) as
//! data in Rust. A component is a number (its place in `State::comps`); the properties live
//! here and every change reaches the rapier world at once. One body per GameObject is shared by
//! its Rigidbody and colliders, made static at the object's pose the first time something needs it.
//!
//! The script classes are declared in `components.rs`, `joints.rs` and `physics2d.rs`; this file
//! is what they call.

use crate::{State, num, op, st, v3, v3_from_f32};
use platypus_core::Value;
use platypus_math::{Q, V2, V3};
use platypus_scene::ObjectId;

/// The physics of one GameObject.
pub(crate) struct ObjPhys {
    pub body: u32,
    /// The Rigidbody or Rigidbody2D component, if any.
    pub rigidbody: Option<u32>,
    /// Collider components, in the order they were added.
    pub colliders: Vec<u32>,
}

#[derive(Clone)]
pub(crate) struct Rb {
    pub mass: f64,
    pub drag: f64,
    pub angular_drag: f64,
    pub gravity_scale: f64,
    pub kind: &'static str,
    /// `RigidbodyConstraints` flags (3D).
    pub constraints: u32,
    /// Keeps a 2D body from turning.
    pub freeze_rotation: bool,
    pub continuous: bool,
    pub two_d: bool,
}

impl Rb {
    pub fn new(two_d: bool) -> Self {
        Rb {
            mass: 1.0,
            drag: 0.0,
            angular_drag: 0.05,
            gravity_scale: 1.0,
            kind: "dynamic",
            constraints: 0,
            freeze_rotation: false,
            continuous: false,
            two_d,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ShapeTag {
    Box,
    Sphere,
    Capsule,
    Mesh,
    Character,
    Box2D,
    Circle2D,
    Capsule2D,
    Polygon2D,
    Edge2D,
}

#[derive(Clone)]
pub(crate) enum ColShape {
    Box {
        size: V3,
    },
    Sphere {
        radius: f64,
    },
    Capsule {
        radius: f64,
        height: f64,
    },
    Mesh {
        points: Vec<V3>,
        indices: Vec<f64>,
        convex: bool,
    },
    Character {
        radius: f64,
        height: f64,
        slope_limit: f64,
        step_offset: f64,
        last: V3,
        velocity: V3,
        fall_speed: f64,
    },
    Box2D {
        size: V2,
    },
    Circle2D {
        radius: f64,
    },
    Capsule2D {
        size: V2,
        vertical: bool,
    },
    Polygon2D {
        points: Vec<V2>,
    },
    Edge2D {
        points: Vec<V2>,
    },
}

impl ColShape {
    pub fn tag(&self) -> ShapeTag {
        match self {
            ColShape::Box { .. } => ShapeTag::Box,
            ColShape::Sphere { .. } => ShapeTag::Sphere,
            ColShape::Capsule { .. } => ShapeTag::Capsule,
            ColShape::Mesh { .. } => ShapeTag::Mesh,
            ColShape::Character { .. } => ShapeTag::Character,
            ColShape::Box2D { .. } => ShapeTag::Box2D,
            ColShape::Circle2D { .. } => ShapeTag::Circle2D,
            ColShape::Capsule2D { .. } => ShapeTag::Capsule2D,
            ColShape::Polygon2D { .. } => ShapeTag::Polygon2D,
            ColShape::Edge2D { .. } => ShapeTag::Edge2D,
        }
    }

    pub fn is_2d(&self) -> bool {
        matches!(
            self,
            ColShape::Box2D { .. }
                | ColShape::Circle2D { .. }
                | ColShape::Capsule2D { .. }
                | ColShape::Polygon2D { .. }
                | ColShape::Edge2D { .. }
        )
    }

    pub fn character(radius: f64, height: f64) -> Self {
        ColShape::Character {
            radius,
            height,
            slope_limit: 45.0,
            step_offset: 0.3,
            last: V3::ZERO,
            velocity: V3::ZERO,
            fall_speed: 0.0,
        }
    }
}

pub(crate) struct Col {
    pub center: V3,
    pub trigger: bool,
    pub friction: f64,
    pub bounciness: f64,
    /// The rapier collider (0: none, such as a mesh without triangles).
    pub cid: u32,
    /// The world scale the shape was made for.
    pub built: V3,
    pub shape: ColShape,
}

/// Joint settings by kind; fields a kind does not use are left alone.
#[derive(Clone)]
pub(crate) struct Jt {
    pub kind: &'static str,
    /// The connected Rigidbody component (0: the world).
    pub connected: u32,
    pub anchor: V3,
    pub connected_anchor: V3,
    pub auto_anchor: bool,
    pub axis: V3,
    pub use_limits: bool,
    pub min: f64,
    pub max: f64,
    pub use_motor: bool,
    pub target_velocity: f64,
    pub force: f64,
    pub spring: f64,
    pub damper: f64,
    /// NaN: the distance when the joint is made.
    pub rest_length: f64,
    pub max_distance: f64,
    /// The rapier joint (0: not made yet).
    pub joint: u32,
}

impl Jt {
    pub fn new(kind: &'static str) -> Self {
        Jt {
            kind,
            connected: 0,
            anchor: V3::ZERO,
            connected_anchor: V3::ZERO,
            auto_anchor: true,
            // A 2D hinge turns about z.
            axis: if kind == "hinge2d" {
                V3 {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                }
            } else {
                V3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0,
                }
            },
            use_limits: false,
            min: -90.0,
            max: 90.0,
            use_motor: false,
            target_velocity: 0.0,
            force: 1.0,
            spring: 10.0,
            damper: 0.2,
            rest_length: f64::NAN,
            max_distance: 2.0,
            joint: 0,
        }
    }
}

pub(crate) struct RopeD {
    pub length: f64,
    pub segments: f64,
    pub radius: f64,
    pub segment_mass: f64,
    /// The Rigidbody component the far end holds (0: none).
    pub end: u32,
    pub links: Vec<ObjectId>,
    pub joints: Vec<u32>,
    pub started: bool,
}

pub(crate) enum Data {
    Rb(Rb),
    Col(Col),
    Joint(Jt),
    Rope(RopeD),
}

pub(crate) struct Comp {
    pub object: ObjectId,
    pub data: Data,
}

// ---- component slab -------------------------------------------------------------------------

pub(crate) fn add_comp(object: ObjectId, data: Data) -> u32 {
    platypus_kinds::components_changed();
    st(|s| {
        s.comps.push(Some(Comp { object, data }));
        s.comps.len() as u32
    })
}

pub(crate) fn with_comp<R>(h: u32, f: impl FnOnce(&mut Comp) -> R) -> Option<R> {
    st(|s| {
        s.comps
            .get_mut((h as usize).checked_sub(1)?)?
            .as_mut()
            .map(f)
    })
}

pub(crate) fn with_rb<R>(h: u32, f: impl FnOnce(&mut Rb) -> R) -> Option<R> {
    with_comp(h, |c| match &mut c.data {
        Data::Rb(r) => Some(f(r)),
        _ => None,
    })
    .flatten()
}

pub(crate) fn with_col<R>(h: u32, f: impl FnOnce(&mut Col) -> R) -> Option<R> {
    with_comp(h, |c| match &mut c.data {
        Data::Col(r) => Some(f(r)),
        _ => None,
    })
    .flatten()
}

pub(crate) fn with_joint<R>(h: u32, f: impl FnOnce(&mut Jt) -> R) -> Option<R> {
    with_comp(h, |c| match &mut c.data {
        Data::Joint(r) => Some(f(r)),
        _ => None,
    })
    .flatten()
}

pub(crate) fn with_rope<R>(h: u32, f: impl FnOnce(&mut RopeD) -> R) -> Option<R> {
    with_comp(h, |c| match &mut c.data {
        Data::Rope(r) => Some(f(r)),
        _ => None,
    })
    .flatten()
}

pub(crate) fn comp_object(h: u32) -> ObjectId {
    with_comp(h, |c| c.object).unwrap_or(ObjectId {
        index: u32::MAX,
        generation: u32::MAX,
    })
}

/// The component handles of one kind on an object, in the order they were added.
pub(crate) fn comps_on(object: ObjectId, pick: impl Fn(&Data) -> bool) -> Vec<u32> {
    st(|s| {
        s.comps
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let c = c.as_ref()?;
                (c.object == object && pick(&c.data)).then_some(i as u32 + 1)
            })
            .collect()
    })
}

/// `f` over every live component, keeping what it returns.
pub(crate) fn all_comps<T>(f: impl Fn(ObjectId, &Data) -> Option<T>) -> Vec<T> {
    st(|s| {
        s.comps
            .iter()
            .flatten()
            .filter_map(|c| f(c.object, &c.data))
            .collect()
    })
}

pub(crate) fn find_rb(object: ObjectId, two_d: bool) -> u32 {
    comps_on(object, |d| matches!(d, Data::Rb(r) if r.two_d == two_d))
        .first()
        .copied()
        .unwrap_or(0)
}

pub(crate) fn find_col(object: ObjectId, tag: ShapeTag) -> u32 {
    comps_on(
        object,
        |d| matches!(d, Data::Col(c) if c.shape.tag() == tag),
    )
    .first()
    .copied()
    .unwrap_or(0)
}

// ---- the object's body ----------------------------------------------------------------------

pub(crate) fn world_transform(object: ObjectId) -> Option<platypus_scene::Transform> {
    platypus_scene::api::with_scene(|sc| sc.contains(object).then(|| sc.world(object))).flatten()
}

pub(crate) fn world_scale(object: ObjectId) -> V3 {
    world_transform(object).map_or(
        V3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
        |t| v3_from_f32(t.scale.to_array()),
    )
}

/// The object's body, made static at the object's pose the first time.
pub(crate) fn physics_for(object: ObjectId) -> u32 {
    if let Some(b) = st(|s| s.objects.get(&object).map(|p| p.body)) {
        return b;
    }
    let body = crate::new_body("fixed");
    if let Some(t) = world_transform(object) {
        st(|s| {
            s.physics
                .set_pose(body, t.position.to_array(), t.rotation.to_array())
        });
    }
    st(|s| {
        s.attached.insert(body, object);
        if let Some(t) = world_transform(object) {
            s.synced.insert(body, (t.position, t.rotation));
        }
        s.objects.insert(
            object,
            ObjPhys {
                body,
                rigidbody: None,
                colliders: Vec::new(),
            },
        );
    });
    body
}

/// Drops the body once nothing on the object needs it.
pub(crate) fn release_physics(object: ObjectId) {
    let body = st(|s| {
        let p = s.objects.get(&object)?;
        (p.rigidbody.is_none() && p.colliders.is_empty()).then_some(p.body)
    });
    if let Some(body) = body {
        crate::remove_body(body);
        st(|s| s.objects.remove(&object));
    }
}

/// Sends the body's mass again (the colliders changed, which share it out). A body without
/// colliders has no shape to turn, so it also gets the size of the box it stands for (its
/// object's scale) to work out a rotational inertia from; otherwise a hinged chain or door
/// would stay put when pushed.
pub(crate) fn send_mass(object: ObjectId) {
    let info = st(|s| {
        let p = s.objects.get(&object)?;
        let rb = p.rigidbody?;
        Some((p.body, rb, p.colliders.is_empty()))
    });
    if let Some((body, rb, bare)) = info
        && let Some(mass) = with_rb(rb, |r| r.mass)
    {
        let mut args = vec![
            num(f64::from(body)),
            Value::String("mass".into()),
            num(mass),
        ];
        if bare {
            args.push(v3(world_scale(object)));
        }
        op("set", args);
    }
}

// ---- rigidbody ------------------------------------------------------------------------------

pub(crate) fn body_of(object: ObjectId) -> u32 {
    st(|s| s.objects.get(&object).map(|p| p.body)).unwrap_or(0)
}

fn set_body(body: u32, what: &str, v: Value) {
    op(
        "set",
        vec![num(f64::from(body)), Value::String(what.into()), v],
    );
}

pub(crate) fn rb_axes(body: u32, r: &Rb) {
    let bools: Vec<Value> = if r.two_d {
        vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(!r.freeze_rotation),
        ]
    } else {
        let free = |flag: u32| Value::Bool(r.constraints & flag == 0);
        vec![free(2), free(4), free(8), free(16), free(32), free(64)]
    };
    set_body(body, "constraints", Value::Array(bools));
}

/// Makes a Rigidbody / Rigidbody2D component for an object.
pub(crate) fn rb_attach(object: ObjectId, two_d: bool) -> u32 {
    let h = add_comp(object, Data::Rb(Rb::new(two_d)));
    let body = physics_for(object);
    st(|s| {
        if let Some(p) = s.objects.get_mut(&object) {
            p.rigidbody = Some(h);
        }
    });
    let r = with_rb(h, |r| r.clone()).expect("just made");
    set_body(body, "kind", Value::String(r.kind.into()));
    st(|s| s.kinds.insert(body, r.kind.to_owned()));
    if !two_d {
        // Constraints default to none: nothing to send.
    } else {
        rb_axes(body, &r);
    }
    send_mass(object);
    set_body(body, "linearDamping", num(r.drag));
    set_body(body, "angularDamping", num(r.angular_drag));
    h
}

pub(crate) fn rb_body(h: u32) -> u32 {
    with_comp(h, |c| c.object).map_or(0, body_of)
}

pub(crate) fn rb_set_kind(h: u32, kind: &'static str) {
    with_rb(h, |r| r.kind = kind);
    let body = rb_body(h);
    if body != 0 {
        st(|s| s.kinds.insert(body, kind.to_owned()));
        set_body(body, "kind", Value::String(kind.into()));
    }
}

pub(crate) fn rb_set_mass(h: u32, kg: f64) {
    with_rb(h, |r| r.mass = kg.max(1e-6));
    let object = comp_object(h);
    send_mass(object);
}

pub(crate) fn rb_set_damping(h: u32, drag: Option<f64>, angular: Option<f64>) {
    let (d, a) = with_rb(h, |r| {
        if let Some(d) = drag {
            r.drag = d;
        }
        if let Some(a) = angular {
            r.angular_drag = a;
        }
        (r.drag, r.angular_drag)
    })
    .unwrap_or((0.0, 0.0));
    let body = rb_body(h);
    if body != 0 {
        set_body(body, "linearDamping", num(d));
        set_body(body, "angularDamping", num(a));
    }
}

pub(crate) fn rb_set_gravity_scale(h: u32, scale: f64) {
    with_rb(h, |r| r.gravity_scale = scale);
    let body = rb_body(h);
    if body != 0 {
        set_body(body, "gravityScale", num(scale));
    }
}

pub(crate) fn rb_set_continuous(h: u32, on: bool) {
    with_rb(h, |r| r.continuous = on);
    let body = rb_body(h);
    if body != 0 {
        set_body(body, "ccd", Value::Bool(on));
    }
}

pub(crate) fn rb_resend_axes(h: u32) {
    let body = rb_body(h);
    if let (true, Some(r)) = (body != 0, with_rb(h, |r| r.clone())) {
        rb_axes(body, &r);
    }
}

pub(crate) fn rb_destroy(h: u32) {
    let Some(object) = with_comp(h, |c| c.object) else {
        return;
    };
    let body = body_of(object);
    st(|s| {
        if let Some(p) = s.objects.get_mut(&object)
            && p.rigidbody == Some(h)
        {
            p.rigidbody = None;
        }
        if let Some(slot) = s.comps.get_mut(h as usize - 1) {
            *slot = None;
        }
    });
    platypus_kinds::components_changed();
    // The colliders stay, now static.
    if body != 0 {
        st(|s| s.kinds.insert(body, "fixed".into()));
        set_body(body, "kind", Value::String("fixed".into()));
    }
    release_physics(object);
}

// ---- colliders ------------------------------------------------------------------------------

/// The rapier shape for a component's shape at an object scale: its name, dimensions, extra
/// triangle indices and the turn of the shape.
struct Spec {
    name: &'static str,
    dims: Vec<f64>,
    indices: Option<Vec<f64>>,
    rotation: Q,
}

fn flat3(points: impl Iterator<Item = V3>) -> Vec<f64> {
    points.flat_map(|p| [p.x, p.y, p.z]).collect()
}

fn spec(shape: &ColShape, scale: V3, depth: f64) -> Option<Spec> {
    let identity = Q::IDENTITY;
    let abs = f64::abs;
    let half_depth = depth / 2.0;
    // An outline as a convex slab (2D shapes are extruded along z).
    let slab = |points: &[V2]| -> Vec<f64> {
        flat3(points.iter().flat_map(|p| {
            [
                V3 {
                    x: p.x * scale.x,
                    y: p.y * scale.y,
                    z: -half_depth,
                },
                V3 {
                    x: p.x * scale.x,
                    y: p.y * scale.y,
                    z: half_depth,
                },
            ]
        }))
    };
    Some(match shape {
        ColShape::Box { size } => Spec {
            name: "box",
            dims: vec![
                abs(size.x * scale.x * 0.5),
                abs(size.y * scale.y * 0.5),
                abs(size.z * scale.z * 0.5),
            ],
            indices: None,
            rotation: identity,
        },
        ColShape::Sphere { radius } => Spec {
            name: "ball",
            dims: vec![radius * abs(scale.x).max(abs(scale.y)).max(abs(scale.z))],
            indices: None,
            rotation: identity,
        },
        ColShape::Capsule { radius, height } | ColShape::Character { radius, height, .. } => {
            let r = radius * abs(scale.x).max(abs(scale.z));
            let half = (height * abs(scale.y) / 2.0 - r).max(0.0);
            Spec {
                name: "capsule",
                dims: vec![half, r],
                indices: None,
                rotation: identity,
            }
        }
        ColShape::Mesh {
            points,
            indices,
            convex,
        } => {
            let pts = flat3(points.iter().map(|p| V3 {
                x: p.x * scale.x,
                y: p.y * scale.y,
                z: p.z * scale.z,
            }));
            if pts.is_empty() || (!convex && indices.is_empty()) {
                return None;
            }
            Spec {
                name: if *convex { "convex" } else { "mesh" },
                dims: pts,
                indices: (!convex).then(|| indices.clone()),
                rotation: identity,
            }
        }
        ColShape::Box2D { size } => Spec {
            name: "box",
            dims: vec![
                abs(size.x * scale.x / 2.0),
                abs(size.y * scale.y / 2.0),
                half_depth,
            ],
            indices: None,
            rotation: identity,
        },
        ColShape::Circle2D { radius } => Spec {
            name: "cylinder",
            dims: vec![half_depth, radius * abs(scale.x).max(abs(scale.y))],
            indices: None,
            // Cylinders stand along y; this one's axis runs along z (Quaternion.euler(90, 0, 0)).
            rotation: Q {
                x: std::f64::consts::FRAC_1_SQRT_2,
                y: 0.0,
                z: 0.0,
                w: std::f64::consts::FRAC_1_SQRT_2,
            },
        },
        ColShape::Capsule2D { size, vertical } => {
            let (w, h) = (size.x, size.y);
            let r = (if *vertical { w } else { h }) / 2.0;
            let half = ((if *vertical { h } else { w }) / 2.0 - r).max(0.0);
            // Two half circles joined: 8 segments each.
            let mut pts = Vec::new();
            for end in 0..2 {
                for i in 0..=8 {
                    let a = std::f64::consts::PI * (f64::from(i) / 8.0)
                        + if end == 1 { std::f64::consts::PI } else { 0.0 };
                    let along = if end == 1 { -half } else { half };
                    pts.push(if *vertical {
                        V2 {
                            x: a.cos() * r,
                            y: along + a.sin() * r,
                        }
                    } else {
                        V2 {
                            x: along + a.sin() * r,
                            y: -a.cos() * r,
                        }
                    });
                }
            }
            Spec {
                name: "convex",
                dims: slab(&pts),
                indices: None,
                rotation: identity,
            }
        }
        ColShape::Polygon2D { points } => {
            if points.len() < 3 {
                return None;
            }
            Spec {
                name: "convex",
                dims: slab(points),
                indices: None,
                rotation: identity,
            }
        }
        ColShape::Edge2D { points } => {
            if points.len() < 2 {
                return None;
            }
            let mut pts = Vec::new();
            let mut indices = Vec::new();
            for (i, p) in points.iter().enumerate() {
                for z in [-half_depth, half_depth] {
                    pts.push(V3 {
                        x: p.x * scale.x,
                        y: p.y * scale.y,
                        z,
                    });
                }
                if i > 0 {
                    let (a, b, c, e) = ((i - 1) * 2, (i - 1) * 2 + 1, i * 2, i * 2 + 1);
                    indices.extend([a, c, b, b, c, e].map(|v| v as f64));
                }
            }
            Spec {
                name: "mesh",
                dims: flat3(pts.into_iter()),
                indices: Some(indices),
                rotation: identity,
            }
        }
    })
}

pub(crate) struct ColliderArgs {
    pub body: u32,
    pub name: &'static str,
    pub dims: Vec<f64>,
    pub indices: Option<Vec<f64>>,
    pub position: V3,
    pub rotation: Q,
    pub friction: f64,
    pub restitution: f64,
    pub sensor: bool,
}

/// Makes a rapier collider; the number is its id.
pub(crate) fn add_collider_full(a: ColliderArgs) -> u32 {
    let id = st(|s| {
        let id = s.next_collider;
        s.next_collider += 1;
        id
    });
    let opts = vec![
        num(a.friction),
        num(a.restitution),
        num(1.0),
        Value::Bool(a.sensor),
        Value::Bool(true),
        a.indices
            .map(|i| Value::Array(i.into_iter().map(num).collect()))
            .unwrap_or(Value::Null),
    ];
    op(
        "collider",
        vec![
            num(f64::from(id)),
            if a.body == 0 {
                Value::Null
            } else {
                num(f64::from(a.body))
            },
            Value::String(a.name.to_owned()),
            Value::Array(a.dims.into_iter().map(num).collect()),
            v3(a.position),
            Value::Array(vec![
                num(a.rotation.x),
                num(a.rotation.y),
                num(a.rotation.z),
                num(a.rotation.w),
            ]),
            Value::Array(opts),
        ],
    );
    st(|s| s.owner.insert(id, a.body));
    id
}

pub(crate) fn remove_collider_id(cid: u32) {
    if cid == 0 {
        return;
    }
    st(|s| {
        s.owner.remove(&cid);
        s.col_owner.remove(&cid);
    });
    op("removeCollider", vec![num(f64::from(cid))]);
}

fn scaled(c: V3, s: V3) -> V3 {
    V3 {
        x: c.x * s.x,
        y: c.y * s.y,
        z: c.z * s.z,
    }
}

/// (Re)makes a collider component's shape on its object's body.
pub(crate) fn col_make(h: u32) {
    let Some((object, old, shape, center, trigger, fr, bo)) = with_comp(h, |c| match &c.data {
        Data::Col(k) => Some((
            c.object,
            k.cid,
            k.shape.clone(),
            k.center,
            k.trigger,
            k.friction,
            k.bounciness,
        )),
        _ => None,
    })
    .flatten() else {
        return;
    };
    remove_collider_id(old);
    let body = physics_for(object);
    let scale = world_scale(object);
    let depth = st(|s| s.depth_2d);
    let cid = match spec(&shape, scale, depth) {
        Some(sp) => {
            let id = add_collider_full(ColliderArgs {
                body,
                name: sp.name,
                dims: sp.dims,
                indices: sp.indices,
                position: scaled(center, scale),
                rotation: sp.rotation,
                friction: fr,
                restitution: bo,
                sensor: trigger,
            });
            st(|s| s.col_owner.insert(id, h));
            id
        }
        None => 0,
    };
    with_col(h, |k| {
        k.cid = cid;
        k.built = scale;
    });
    st(|s| {
        if let Some(p) = s.objects.get_mut(&object)
            && !p.colliders.contains(&h)
        {
            p.colliders.push(h);
        }
    });
    send_ignored(h);
    send_mass(object);
}

/// The rapier collider of a collider component (0: it has none).
fn collider_id(h: u32) -> u32 {
    with_col(h, |k| k.cid).unwrap_or(0)
}

/// Tells the world whether the colliders of components `a` and `b` collide, once both have one.
fn send_ignore(a: u32, b: u32, ignore: bool) {
    let (ca, cb) = (collider_id(a), collider_id(b));
    if ca != 0 && cb != 0 {
        let mut args = vec![num(f64::from(ca)), num(f64::from(cb))];
        if !ignore {
            args.push(Value::Bool(false));
        }
        op("ignore", args);
    }
}

/// `Physics2D.ignoreCollision` / `Physics.IgnoreCollision`: the pair does not collide (or does
/// again). It holds across rebuilds of either collider (a scale change, a new size).
pub(crate) fn set_ignored(a: u32, b: u32, ignore: bool) {
    if a == 0 || b == 0 || a == b {
        return;
    }
    let pair = (a.min(b), a.max(b));
    st(|s| {
        if ignore {
            s.ignored.insert(pair)
        } else {
            s.ignored.remove(&pair)
        }
    });
    send_ignore(a, b, ignore);
}

/// Sends the ignored pairs `h` is in again: its collider was rebuilt (a new rapier collider).
fn send_ignored(h: u32) {
    let pairs: Vec<(u32, u32)> = st(|s| {
        s.ignored
            .iter()
            .filter(|&&(a, b)| a == h || b == h)
            .copied()
            .collect()
    });
    for (a, b) in pairs {
        send_ignore(a, b, true);
    }
}

pub(crate) fn col_attach(object: ObjectId, shape: ColShape) -> u32 {
    let h = add_comp(
        object,
        Data::Col(Col {
            center: V3::ZERO,
            trigger: false,
            friction: 0.6,
            bounciness: 0.0,
            cid: 0,
            built: V3::ZERO,
            shape,
        }),
    );
    col_make(h);
    h
}

pub(crate) fn col_destroy(h: u32) {
    let Some((object, cid)) = with_comp(h, |c| match &c.data {
        Data::Col(k) => Some((c.object, k.cid)),
        _ => None,
    })
    .flatten() else {
        return;
    };
    remove_collider_id(cid);
    st(|s| {
        s.ignored.retain(|&(a, b)| a != h && b != h);
        if let Some(p) = s.objects.get_mut(&object) {
            p.colliders.retain(|c| *c != h);
        }
        s.comps[h as usize - 1] = None;
    });
    platypus_kinds::components_changed();
    send_mass(object);
    release_physics(object);
}

/// Edits a collider's shape and remakes it.
pub(crate) fn col_edit(h: u32, f: impl FnOnce(&mut ColShape)) {
    with_col(h, |k| f(&mut k.shape));
    col_make(h);
}

pub(crate) fn col_set_center(h: u32, c: V3) {
    let Some((object, cid)) = with_comp(h, |cm| match &mut cm.data {
        Data::Col(k) => {
            k.center = c;
            Some((cm.object, k.cid))
        }
        _ => None,
    })
    .flatten() else {
        return;
    };
    if cid != 0 {
        let s = world_scale(object);
        let p = scaled(c, s);
        op(
            "colliderSet",
            vec![
                num(f64::from(cid)),
                Value::String("offset".into()),
                v3(p),
                Value::Array(vec![num(0.0), num(0.0), num(0.0), num(1.0)]),
            ],
        );
        // 2D circles lie turned: remake to keep their rotation.
        if matches!(with_col(h, |k| k.shape.tag()), Some(ShapeTag::Circle2D)) {
            col_make(h);
        }
    }
}

pub(crate) fn col_set_sensor(h: u32, on: bool) {
    let cid = with_col(h, |k| {
        k.trigger = on;
        k.cid
    })
    .unwrap_or(0);
    if cid != 0 {
        op(
            "colliderSet",
            vec![
                num(f64::from(cid)),
                Value::String("sensor".into()),
                Value::Bool(on),
            ],
        );
    }
}

pub(crate) fn col_set_material(h: u32, friction: Option<f64>, bounciness: Option<f64>) {
    let (cid, f, b) = with_col(h, |k| {
        if let Some(f) = friction {
            k.friction = f;
        }
        if let Some(b) = bounciness {
            k.bounciness = b;
        }
        (k.cid, k.friction, k.bounciness)
    })
    .unwrap_or((0, 0.0, 0.0));
    if cid != 0 {
        for (name, v) in [("friction", f), ("restitution", b)] {
            op(
                "colliderSet",
                vec![num(f64::from(cid)), Value::String(name.into()), num(v)],
            );
        }
    }
}

/// The Rigidbody (or Rigidbody2D) component moving a collider's object.
pub(crate) fn attached_rigidbody(h: u32, two_d: bool) -> u32 {
    let object = comp_object(h);
    st(|s| s.objects.get(&object).and_then(|p| p.rigidbody)).map_or(0, |rb| {
        if with_rb(rb, |r| r.two_d == two_d).unwrap_or(false) {
            rb
        } else {
            0
        }
    })
}

// ---- joints ---------------------------------------------------------------------------------

/// (Re)makes a joint component's rapier joint with its current settings.
pub(crate) fn joint_build(h: u32) {
    let Some((object, j)) = with_comp(h, |c| match &c.data {
        Data::Joint(j) => Some((c.object, j.clone())),
        _ => None,
    })
    .flatten() else {
        return;
    };
    if j.joint != 0 {
        remove_joint_id(j.joint);
        with_joint(h, |j| j.joint = 0);
    }
    let body = body_of(object);
    if body == 0 {
        return;
    }
    let tr = world_transform(object);
    let scale = world_scale(object);
    let anchor = scaled(j.anchor, scale);
    let other_object = (j.connected != 0).then(|| comp_object(j.connected));
    let other_body = other_object.map_or(0, body_of);
    let mut connected_anchor = j.connected_anchor;
    if j.auto_anchor
        && let Some(t) = tr
    {
        // The anchor in world space, then in the connected body's unscaled space.
        let local = glam::Vec3::new(j.anchor.x as f32, j.anchor.y as f32, j.anchor.z as f32);
        let world = t.position + t.rotation * (local * t.scale);
        let ca = match other_object.and_then(world_transform) {
            Some(o) => o.rotation.inverse() * (world - o.position),
            None => world,
        };
        connected_anchor = v3_from_f32(ca.to_array());
        with_joint(h, |jj| jj.connected_anchor = connected_anchor);
    }
    let params: Vec<f64> = match j.kind {
        "hinge" | "hinge2d" => vec![
            j.axis.x,
            j.axis.y,
            j.axis.z,
            f64::from(u8::from(j.use_limits)),
            j.min,
            j.max,
            f64::from(u8::from(j.use_motor)),
            j.target_velocity,
            j.force,
        ],
        "spring" => {
            let rest = if j.rest_length.is_nan() {
                // The distance between the anchors now.
                let world = tr.map_or(glam::Vec3::ZERO, |t| {
                    let l =
                        glam::Vec3::new(j.anchor.x as f32, j.anchor.y as f32, j.anchor.z as f32);
                    t.position + t.rotation * (l * t.scale)
                });
                let ca = glam::Vec3::new(
                    connected_anchor.x as f32,
                    connected_anchor.y as f32,
                    connected_anchor.z as f32,
                );
                let other = match other_object.and_then(world_transform) {
                    Some(o) => o.position + o.rotation * ca,
                    None => ca,
                };
                f64::from(world.distance(other))
            } else {
                j.rest_length
            };
            vec![rest, j.spring, j.damper]
        }
        "rope" => vec![j.max_distance],
        _ => Vec::new(),
    };
    let world_kind = if j.kind == "hinge2d" { "hinge" } else { j.kind };
    let id = create_joint_raw(
        world_kind,
        body,
        other_body,
        anchor,
        connected_anchor,
        &params,
    );
    with_joint(h, |j| j.joint = id);
}

pub(crate) fn create_joint_raw(
    kind: &str,
    a: u32,
    b: u32,
    anchor_a: V3,
    anchor_b: V3,
    params: &[f64],
) -> u32 {
    let id = st(|s| {
        s.next_joint += 1;
        s.next_joint
    });
    op(
        "joint",
        vec![
            num(f64::from(id)),
            Value::String(kind.to_owned()),
            num(f64::from(a)),
            if b == 0 {
                Value::Null
            } else {
                num(f64::from(b))
            },
            v3(anchor_a),
            v3(anchor_b),
            Value::Array(params.iter().copied().map(num).collect()),
        ],
    );
    id
}

pub(crate) fn remove_joint_id(id: u32) {
    if id != 0 {
        op("removeJoint", vec![num(f64::from(id))]);
    }
}

pub(crate) fn joint_destroy(h: u32) {
    if let Some(j) = with_joint(h, |j| j.joint) {
        remove_joint_id(j);
    }
    st(|s| {
        if let Some(slot) = s.comps.get_mut(h as usize - 1) {
            *slot = None;
        }
    });
    platypus_kinds::components_changed();
}

// ---- rope -----------------------------------------------------------------------------------

/// Builds the chain of capsule links the first time the rope is stepped (its options are set by
/// then).
fn rope_start(h: u32) {
    let Some((object, r)) = with_comp(h, |c| match &mut c.data {
        Data::Rope(r) if !r.started => {
            r.started = true;
            Some((
                c.object,
                (r.length, r.segments, r.radius, r.segment_mass, r.end),
            ))
        }
        _ => None,
    })
    .flatten() else {
        return;
    };
    let (length, segments, radius, mass, end) = r;
    let start = world_transform(object).map_or(glam::Vec3::ZERO, |t| t.position);
    let to_end = match (end != 0)
        .then(|| comp_object(end))
        .and_then(world_transform)
    {
        Some(t) => t.position - start,
        None => glam::Vec3::NEG_Y * length as f32,
    };
    let dir = if to_end.length() > 1e-6 {
        to_end.normalize()
    } else {
        glam::Vec3::NEG_Y
    };
    let n = segments.round().max(1.0) as usize;
    let seg = (length / n as f64) as f32;
    // Segments' local -y points along the rope.
    let rotation = glam::Quat::from_rotation_arc(glam::Vec3::NEG_Y, dir);
    let own_rb = st(|s| {
        s.objects
            .get(&object)
            .and_then(|p| p.rigidbody.map(|_| p.body))
    });
    let mut prev: u32 = own_rb.unwrap_or(0);
    let mut prev_anchor = if prev != 0 {
        V3::ZERO
    } else {
        v3_from_f32(start.to_array())
    };
    let name = platypus_scene::api::with_scene(|sc| sc.name(object).unwrap_or("").to_owned())
        .unwrap_or_default();
    let mut links = Vec::new();
    let mut joints = Vec::new();
    let mut last_body = prev;
    for i in 0..n {
        let center = start + dir * (seg * (i as f32 + 0.5));
        let link = platypus_scene::api::with_scene(|sc| {
            let id = sc.create(&format!("{name} rope {i}"));
            sc.set_local(
                id,
                platypus_scene::Transform {
                    position: center,
                    rotation,
                    // The capsule is 1 m wide and 2 m tall.
                    scale: glam::Vec3::new(radius as f32 * 2.0, seg / 2.0, radius as f32 * 2.0),
                },
            );
            id
        });
        let Some(link) = link else { break };
        col_attach(
            link,
            ColShape::Capsule {
                radius: 0.5,
                height: 2.0,
            },
        );
        let rb = rb_attach(link, false);
        rb_set_mass(rb, mass);
        rb_set_damping(rb, Some(0.1), Some(0.5));
        let body = body_of(link);
        let half = f64::from(seg) / 2.0;
        joints.push(create_joint_raw(
            "ball",
            body,
            prev,
            V3 {
                x: 0.0,
                y: half,
                z: 0.0,
            },
            prev_anchor,
            &[],
        ));
        prev = body;
        last_body = body;
        prev_anchor = V3 {
            x: 0.0,
            y: -half,
            z: 0.0,
        };
        links.push(link);
    }
    if end != 0 {
        let eb = body_of(comp_object(end));
        if eb != 0 && last_body != 0 {
            joints.push(create_joint_raw(
                "ball",
                eb,
                last_body,
                V3::ZERO,
                prev_anchor,
                &[],
            ));
        }
    }
    with_rope(h, |r| {
        r.links = links;
        r.joints = joints;
    });
}

pub(crate) fn rope_destroy(h: u32) {
    let Some((links, joints)) = with_rope(h, |r| {
        (std::mem::take(&mut r.links), std::mem::take(&mut r.joints))
    }) else {
        return;
    };
    for j in joints {
        remove_joint_id(j);
    }
    for l in links {
        destroy_object(l);
    }
    st(|s| {
        if let Some(slot) = s.comps.get_mut(h as usize - 1) {
            *slot = None;
        }
    });
    platypus_kinds::components_changed();
}

/// Removes an object's physics components and body, then the object.
fn destroy_object(object: ObjectId) {
    object_destroyed(object);
    platypus_scene::api::with_scene(|sc| sc.destroy(object));
}

/// Where the rope runs: the first link's top, then every link's bottom (where they join).
pub(crate) fn rope_points(h: u32) -> Vec<V3> {
    let links = with_rope(h, |r| r.links.clone()).unwrap_or_default();
    let mut out = Vec::new();
    for (i, l) in links.iter().enumerate() {
        let Some(t) = world_transform(*l) else {
            continue;
        };
        // A link's local y runs back up the rope; its scaled capsule spans -1..1.
        if i == 0 {
            out.push(v3_from_f32(
                (t.position + t.rotation * (glam::Vec3::Y * t.scale.y)).to_array(),
            ));
        }
        out.push(v3_from_f32(
            (t.position - t.rotation * (glam::Vec3::Y * t.scale.y)).to_array(),
        ));
    }
    out
}

// ---- the world's frame ----------------------------------------------------------------------

/// Builds what components left for when the world starts moving: joints with their bodies,
/// ropes with their options. The host may call it earlier (after setting up a scene); `step`
/// does.
pub fn start_pending() {
    let pending: Vec<u32> = st(|s| {
        s.comps
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let c = c.as_ref()?;
                let wants = match &c.data {
                    Data::Joint(j) => j.joint == 0,
                    Data::Rope(r) => !r.started,
                    _ => false,
                };
                wants.then_some(i as u32 + 1)
            })
            .collect()
    });
    for h in pending {
        match with_comp(h, |c| matches!(c.data, Data::Rope(_))) {
            Some(true) => rope_start(h),
            Some(false) => joint_build(h),
            None => {}
        }
    }
}

/// Moves bodies to where scripts put their objects and remakes the colliders whose object was
/// scaled since they were made. Objects that are gone lose their physics.
pub(crate) fn sync_from_scene() {
    let bodies: Vec<(u32, ObjectId)> = st(|s| s.attached.iter().map(|(b, o)| (*b, *o)).collect());
    let mut gone = Vec::new();
    let mut rescale = Vec::new();
    for (body, object) in bodies {
        let Some(t) = world_transform(object) else {
            gone.push(object);
            continue;
        };
        // The object moved by a script when it differs from where the world last left it; an
        // object never synced yet takes the body's pose (a body attached to an object).
        let last = st(|s| s.synced.get(&body).copied());
        let pose = st(|s| s.physics.pose(body));
        match (last, pose) {
            (Some((lp, lr)), Some(_)) => {
                let dp = lp.distance_squared(t.position);
                let dr = lr.dot(t.rotation).abs();
                if dp > 1e-8 || dr < 1.0 - 1e-9 {
                    st(|s| {
                        s.physics
                            .set_pose(body, t.position.to_array(), t.rotation.to_array());
                        s.synced.insert(body, (t.position, t.rotation));
                    });
                }
            }
            (None, Some((p, r))) => {
                let (p, r) = (glam::Vec3::from_array(p), glam::Quat::from_array(r));
                platypus_scene::api::with_scene(|sc| sc.set_world(object, p, r));
                let w = world_transform(object).unwrap_or(t);
                st(|s| s.synced.insert(body, (w.position, w.rotation)));
            }
            _ => {}
        }
        let cols = st(|s| s.objects.get(&object).map(|p| p.colliders.clone()));
        for c in cols.unwrap_or_default() {
            let built = with_col(c, |k| k.built).unwrap_or(V3::ZERO);
            let now = v3_from_f32(t.scale.to_array());
            let d = (built.x - now.x).abs() + (built.y - now.y).abs() + (built.z - now.z).abs();
            if d > 1e-6 {
                rescale.push(c);
            }
        }
    }
    for object in gone {
        object_destroyed(object);
    }
    for c in rescale {
        col_make(c);
    }
}

/// Removes every physics component of an object, its body and its colliders (the host calls this
/// for destroyed GameObjects; `step` also notices objects that are gone).
pub fn object_destroyed(object: ObjectId) {
    let handles = comps_on(object, |_| true);
    for h in handles {
        match with_comp(h, |c| match &c.data {
            Data::Rb(_) => 0,
            Data::Col(_) => 1,
            Data::Joint(_) => 2,
            Data::Rope(_) => 3,
        }) {
            Some(1) => col_destroy(h),
            Some(2) => joint_destroy(h),
            Some(3) => rope_destroy(h),
            Some(0) => rb_destroy(h),
            _ => {}
        }
    }
    // Anything left on the object's body (low-level colliders, attached bodies).
    let bodies: Vec<u32> = st(|s| {
        s.attached
            .iter()
            .filter_map(|(b, o)| (*o == object).then_some(*b))
            .collect()
    });
    for b in bodies {
        crate::remove_body(b);
    }
    st(|s| s.objects.remove(&object));
}

/// What a contact event needs to know about one side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColliderInfo {
    /// The GameObject the collider's body moves.
    pub object: ObjectId,
    /// The collider component (`BoxCollider`, ... as its class name) and its handle.
    pub class: &'static str,
    pub component: u32,
    /// A 2D collider (`onCollisionEnter2D`).
    pub two_d: bool,
    /// The Rigidbody / Rigidbody2D component on the object (0: none).
    pub rigidbody: u32,
}

pub fn class_name(shape: &ColShape) -> &'static str {
    match shape {
        ColShape::Box { .. } => "BoxCollider",
        ColShape::Sphere { .. } => "SphereCollider",
        ColShape::Capsule { .. } => "CapsuleCollider",
        ColShape::Mesh { .. } => "MeshCollider",
        ColShape::Character { .. } => "CharacterController",
        ColShape::Box2D { .. } => "BoxCollider2D",
        ColShape::Circle2D { .. } => "CircleCollider2D",
        ColShape::Capsule2D { .. } => "CapsuleCollider2D",
        ColShape::Polygon2D { .. } => "PolygonCollider2D",
        ColShape::Edge2D { .. } => "EdgeCollider2D",
    }
}

/// The component behind a collider number from a contact (`None` for colliders made without a
/// component, such as `Physics.addBox`).
pub fn collider_info(collider: u32) -> Option<ColliderInfo> {
    let h = st(|s: &mut State| s.col_owner.get(&collider).copied())?;
    let (object, class, two_d) = with_comp(h, |c| match &c.data {
        Data::Col(k) => Some((c.object, class_name(&k.shape), k.shape.is_2d())),
        _ => None,
    })
    .flatten()?;
    let rigidbody = st(|s| s.objects.get(&object).and_then(|p| p.rigidbody)).unwrap_or(0);
    Some(ColliderInfo {
        object,
        class,
        component: h,
        two_d,
        rigidbody,
    })
}

/// After a step: how fast each character controller actually moved.
pub(crate) fn after_step() {
    let chars: Vec<(u32, ObjectId)> = st(|s| {
        s.comps
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let c = c.as_ref()?;
                matches!(&c.data, Data::Col(k) if matches!(k.shape, ColShape::Character { .. }))
                    .then_some((i as u32 + 1, c.object))
            })
            .collect()
    });
    for (h, object) in chars {
        let Some(t) = world_transform(object) else {
            continue;
        };
        let now = v3_from_f32(t.position.to_array());
        with_col(h, |k| {
            if let ColShape::Character { last, velocity, .. } = &mut k.shape {
                *velocity = V3 {
                    x: (now.x - last.x) * 60.0,
                    y: (now.y - last.y) * 60.0,
                    z: (now.z - last.z) * 60.0,
                };
                *last = now;
            }
        });
    }
}

pub(crate) fn rope_new() -> RopeD {
    RopeD {
        length: 3.0,
        segments: 12.0,
        radius: 0.04,
        segment_mass: 0.05,
        end: 0,
        links: Vec::new(),
        joints: Vec::new(),
        started: false,
    }
}
