// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Component kinds (`platypus_kinds`) of the physics components, so scene files with them load and
//! `instantiate` copies them. [`register_kinds`] registers them on this thread (the host calls it
//! with the other engine kinds). Properties are saved under the TypeScript names; a collider's
//! `material` is `{ dynamicFriction, bounciness }`, and a reference to a Rigidbody is the
//! `{"$ref":"Component","kind":"Rigidbody","handle":N}` placeholder.

use crate::comp::{
    self, ColShape, Data, Jt, RopeD, ShapeTag, with_col, with_comp, with_joint, with_rb, with_rope,
};
use platypus_api::Handle;
use platypus_kinds::{Kind, Props};
use platypus_math::{V2, V3};
use platypus_scene::ObjectId;
use serde_json::{Map, Value, json};

const CLASSES: [&str; 19] = [
    "Rigidbody",
    "BoxCollider",
    "SphereCollider",
    "CapsuleCollider",
    "MeshCollider",
    "CharacterController",
    "FixedJoint",
    "CharacterJoint",
    "HingeJoint",
    "HingeJoint2D",
    "SpringJoint",
    "RopeJoint",
    "Rope",
    "Rigidbody2D",
    "BoxCollider2D",
    "CircleCollider2D",
    "CapsuleCollider2D",
    "PolygonCollider2D",
    "EdgeCollider2D",
];

fn tag_of(class: &str) -> Option<ShapeTag> {
    Some(match class {
        "BoxCollider" => ShapeTag::Box,
        "SphereCollider" => ShapeTag::Sphere,
        "CapsuleCollider" => ShapeTag::Capsule,
        "MeshCollider" => ShapeTag::Mesh,
        "CharacterController" => ShapeTag::Character,
        "BoxCollider2D" => ShapeTag::Box2D,
        "CircleCollider2D" => ShapeTag::Circle2D,
        "CapsuleCollider2D" => ShapeTag::Capsule2D,
        "PolygonCollider2D" => ShapeTag::Polygon2D,
        "EdgeCollider2D" => ShapeTag::Edge2D,
        _ => return None,
    })
}

fn joint_kind(class: &str) -> Option<&'static str> {
    Some(match class {
        "FixedJoint" => "fixed",
        "CharacterJoint" => "ball",
        "HingeJoint" => "hinge",
        "HingeJoint2D" => "hinge2d",
        "SpringJoint" => "spring",
        "RopeJoint" => "rope",
        _ => return None,
    })
}

fn default_shape(tag: ShapeTag) -> ColShape {
    let v = |x, y| V2 { x, y };
    match tag {
        ShapeTag::Box => ColShape::Box {
            size: V3 {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
        },
        ShapeTag::Sphere => ColShape::Sphere { radius: 0.5 },
        ShapeTag::Capsule => ColShape::Capsule {
            radius: 0.5,
            height: 2.0,
        },
        ShapeTag::Mesh => ColShape::Mesh {
            points: Vec::new(),
            indices: Vec::new(),
            convex: false,
        },
        ShapeTag::Character => ColShape::character(0.5, 2.0),
        ShapeTag::Box2D => ColShape::Box2D { size: v(1.0, 1.0) },
        ShapeTag::Circle2D => ColShape::Circle2D { radius: 0.5 },
        ShapeTag::Capsule2D => ColShape::Capsule2D {
            size: v(1.0, 2.0),
            vertical: true,
        },
        ShapeTag::Polygon2D => ColShape::Polygon2D {
            points: vec![v(-0.5, -0.5), v(0.5, -0.5), v(0.0, 0.5)],
        },
        ShapeTag::Edge2D => ColShape::Edge2D {
            points: vec![v(-0.5, 0.0), v(0.5, 0.0)],
        },
    }
}

fn is_class(class: &str, d: &Data) -> bool {
    match d {
        Data::Rb(r) => class == if r.two_d { "Rigidbody2D" } else { "Rigidbody" },
        Data::Col(c) => tag_of(class) == Some(c.shape.tag()),
        Data::Joint(j) => joint_kind(class) == Some(j.kind),
        Data::Rope(_) => class == "Rope",
    }
}

fn create(class: &str, object: ObjectId) -> Result<u64, String> {
    let h = match class {
        "Rigidbody" => comp::rb_attach(object, false),
        "Rigidbody2D" => comp::rb_attach(object, true),
        "CharacterController" => crate::components::character_attach(object),
        "Rope" => comp::add_comp(object, Data::Rope(comp::rope_new())),
        c => match (tag_of(c), joint_kind(c)) {
            (Some(tag), _) => comp::col_attach(object, default_shape(tag)),
            (_, Some(kind)) => {
                let h = comp::add_comp(object, Data::Joint(Jt::new(kind)));
                comp::joint_build(h);
                h
            }
            _ => return Err(format!("no physics component `{c}`")),
        },
    };
    Ok(u64::from(h))
}

fn destroy(h: u32) {
    match with_comp(h, |c| match &c.data {
        Data::Rb(_) => 0,
        Data::Col(_) => 1,
        Data::Joint(_) => 2,
        Data::Rope(_) => 3,
    }) {
        Some(0) => comp::rb_destroy(h),
        Some(1) => comp::col_destroy(h),
        Some(2) => comp::joint_destroy(h),
        Some(3) => comp::rope_destroy(h),
        _ => {}
    }
}

// ---- JSON values ----------------------------------------------------------------------------

fn vec3_json(v: V3) -> Value {
    json!({ "$type": "Vector3", "value": [v.x, v.y, v.z] })
}

fn vec2_json(v: V2) -> Value {
    json!({ "$type": "Vector2", "value": [v.x, v.y] })
}

fn parts(v: &Value) -> Option<Vec<f64>> {
    let a = match v {
        Value::Array(a) => a,
        Value::Object(o) => o.get("value")?.as_array()?,
        _ => return None,
    };
    a.iter().map(Value::as_f64).collect()
}

fn vec3(v: &Value) -> Option<V3> {
    let p = parts(v)?;
    (p.len() == 3).then(|| V3 {
        x: p[0],
        y: p[1],
        z: p[2],
    })
}

fn vec2(v: &Value) -> Option<V2> {
    let p = parts(v)?;
    (p.len() == 2).then(|| V2 { x: p[0], y: p[1] })
}

fn vec3s(v: &Value) -> Option<Vec<V3>> {
    v.as_array()?.iter().map(vec3).collect()
}

fn vec2s(v: &Value) -> Option<Vec<V2>> {
    v.as_array()?.iter().map(vec2).collect()
}

fn reference(rb: u32) -> Value {
    if rb == 0 {
        Value::Null
    } else {
        json!({ "$ref": "Component", "kind": "Rigidbody", "handle": rb })
    }
}

fn parse_reference(v: &Value) -> Option<u32> {
    if v.is_null() {
        return Some(0);
    }
    v.get("handle")?.as_u64().map(|h| h as u32)
}

// ---- save -----------------------------------------------------------------------------------

fn save(h: u32) -> Map<String, Value> {
    let mut m = Map::new();
    with_comp(h, |c| match &c.data {
        Data::Rb(r) => {
            m.insert("mass".into(), json!(r.mass));
            m.insert("drag".into(), json!(r.drag));
            m.insert("angularDrag".into(), json!(r.angular_drag));
            if r.two_d {
                m.insert("gravityScale".into(), json!(r.gravity_scale));
                let t = if r.kind == "fixed" { "static" } else { r.kind };
                m.insert("bodyType".into(), json!(t));
                m.insert("freezeRotation".into(), json!(r.freeze_rotation));
                m.insert(
                    "collisionDetectionMode".into(),
                    json!(if r.continuous {
                        "continuous"
                    } else {
                        "discrete"
                    }),
                );
            } else {
                m.insert("useGravity".into(), json!(r.gravity_scale != 0.0));
                m.insert("isKinematic".into(), json!(r.kind == "kinematic"));
                m.insert("constraints".into(), json!(r.constraints));
                m.insert(
                    "collisionDetection".into(),
                    json!(if r.continuous {
                        "continuous"
                    } else {
                        "discrete"
                    }),
                );
            }
        }
        Data::Col(k) => {
            if k.shape.is_2d() {
                m.insert(
                    "offset".into(),
                    vec2_json(V2 {
                        x: k.center.x,
                        y: k.center.y,
                    }),
                );
            } else {
                m.insert("center".into(), vec3_json(k.center));
            }
            m.insert("isTrigger".into(), json!(k.trigger));
            m.insert(
                "material".into(),
                json!({ "dynamicFriction": k.friction, "bounciness": k.bounciness }),
            );
            match &k.shape {
                ColShape::Box { size } => {
                    m.insert("size".into(), vec3_json(*size));
                }
                ColShape::Sphere { radius } => {
                    m.insert("radius".into(), json!(radius));
                }
                ColShape::Capsule { radius, height } => {
                    m.insert("radius".into(), json!(radius));
                    m.insert("height".into(), json!(height));
                }
                ColShape::Mesh {
                    points,
                    indices,
                    convex,
                } => {
                    m.insert(
                        "points".into(),
                        Value::Array(points.iter().map(|p| vec3_json(*p)).collect()),
                    );
                    m.insert("indices".into(), json!(indices));
                    m.insert("convex".into(), json!(convex));
                }
                ColShape::Character {
                    radius,
                    height,
                    slope_limit,
                    step_offset,
                    ..
                } => {
                    m.insert("radius".into(), json!(radius));
                    m.insert("height".into(), json!(height));
                    m.insert("slopeLimit".into(), json!(slope_limit));
                    m.insert("stepOffset".into(), json!(step_offset));
                }
                ColShape::Box2D { size } => {
                    m.insert("size".into(), vec2_json(*size));
                }
                ColShape::Circle2D { radius } => {
                    m.insert("radius".into(), json!(radius));
                }
                ColShape::Capsule2D { size, vertical } => {
                    m.insert("size".into(), vec2_json(*size));
                    m.insert(
                        "direction".into(),
                        json!(if *vertical { "vertical" } else { "horizontal" }),
                    );
                }
                ColShape::Polygon2D { points } | ColShape::Edge2D { points } => {
                    m.insert(
                        "points".into(),
                        Value::Array(points.iter().map(|p| vec2_json(*p)).collect()),
                    );
                }
            }
        }
        Data::Joint(j) => {
            m.insert("connectedBody".into(), reference(j.connected));
            m.insert("anchor".into(), vec3_json(j.anchor));
            m.insert("connectedAnchor".into(), vec3_json(j.connected_anchor));
            m.insert("autoConfigureConnectedAnchor".into(), json!(j.auto_anchor));
            match j.kind {
                "hinge" | "hinge2d" => {
                    m.insert("axis".into(), vec3_json(j.axis));
                    m.insert("useLimits".into(), json!(j.use_limits));
                    m.insert("limits".into(), json!({ "min": j.min, "max": j.max }));
                    m.insert("useMotor".into(), json!(j.use_motor));
                    m.insert(
                        "motor".into(),
                        json!({ "targetVelocity": j.target_velocity, "force": j.force }),
                    );
                }
                "spring" => {
                    m.insert("spring".into(), json!(j.spring));
                    m.insert("damper".into(), json!(j.damper));
                    m.insert(
                        "restLength".into(),
                        if j.rest_length.is_nan() {
                            Value::Null
                        } else {
                            json!(j.rest_length)
                        },
                    );
                }
                "rope" => {
                    m.insert("maxDistance".into(), json!(j.max_distance));
                }
                _ => {}
            }
        }
        Data::Rope(r) => {
            m.insert("length".into(), json!(r.length));
            m.insert("segments".into(), json!(r.segments));
            m.insert("radius".into(), json!(r.radius));
            m.insert("segmentMass".into(), json!(r.segment_mass));
            m.insert("end".into(), reference(r.end));
        }
    });
    m
}

// ---- load -----------------------------------------------------------------------------------

fn warn_shape(k: &str) -> String {
    format!("property `{k}` has a value of the wrong shape")
}

fn load(h: u32, props: &Props) -> Vec<String> {
    let mut w = Vec::new();
    let Some(kind) = with_comp(h, |c| match &c.data {
        Data::Rb(_) => 0,
        Data::Col(_) => 1,
        Data::Joint(_) => 2,
        Data::Rope(_) => 3,
    }) else {
        return w;
    };
    match kind {
        0 => load_rb(h, props, &mut w),
        1 => load_col(h, props, &mut w),
        2 => load_joint(h, props, &mut w),
        _ => load_rope(h, props, &mut w),
    }
    w
}

fn load_rb(h: u32, props: &Props, w: &mut Vec<String>) {
    for (k, v) in props {
        let ok = match k.as_str() {
            "mass" => v.as_f64().map(|x| comp::rb_set_mass(h, x)),
            "drag" => v.as_f64().map(|x| comp::rb_set_damping(h, Some(x), None)),
            "angularDrag" => v.as_f64().map(|x| comp::rb_set_damping(h, None, Some(x))),
            "useGravity" => v
                .as_bool()
                .map(|x| comp::rb_set_gravity_scale(h, if x { 1.0 } else { 0.0 })),
            "gravityScale" => v.as_f64().map(|x| comp::rb_set_gravity_scale(h, x)),
            "isKinematic" => v
                .as_bool()
                .map(|x| comp::rb_set_kind(h, if x { "kinematic" } else { "dynamic" })),
            "bodyType" => v.as_str().map(|t| {
                comp::rb_set_kind(
                    h,
                    match t {
                        "static" => "fixed",
                        "kinematic" => "kinematic",
                        _ => "dynamic",
                    },
                )
            }),
            "constraints" => v.as_f64().map(|x| {
                with_rb(h, |r| r.constraints = x as u32);
                comp::rb_resend_axes(h);
            }),
            "freezeRotation" => v.as_bool().map(|x| {
                with_rb(h, |r| r.freeze_rotation = x);
                comp::rb_resend_axes(h);
            }),
            "collisionDetection" | "collisionDetectionMode" => v
                .as_str()
                .map(|m| comp::rb_set_continuous(h, m == "continuous")),
            other => {
                w.push(format!("no property `{other}` yet; dropped"));
                continue;
            }
        };
        if ok.is_none() {
            w.push(warn_shape(k));
        }
    }
}

fn load_col(h: u32, props: &Props, w: &mut Vec<String>) {
    for (k, v) in props {
        let ok: Option<()> = match k.as_str() {
            "center" => vec3(v).map(|c| comp::col_set_center(h, c)),
            "offset" => vec2(v).map(|o| {
                comp::col_set_center(
                    h,
                    V3 {
                        x: o.x,
                        y: o.y,
                        z: 0.0,
                    },
                )
            }),
            "isTrigger" => v.as_bool().map(|b| comp::col_set_sensor(h, b)),
            "material" => (|| {
                let f = v.get("dynamicFriction")?.as_f64()?;
                let b = v.get("bounciness")?.as_f64()?;
                comp::col_set_material(h, Some(f), Some(b));
                Some(())
            })(),
            other => {
                let before = with_col(h, |c| c.shape.tag());
                let done = load_shape(h, other, v);
                match done {
                    Some(true) => Some(()),
                    Some(false) => {
                        w.push(format!("no property `{other}` yet; dropped"));
                        continue;
                    }
                    None => {
                        let _ = before;
                        None
                    }
                }
            }
        };
        if ok.is_none() {
            w.push(warn_shape(k));
        }
    }
    comp::col_make(h);
    if matches!(with_col(h, |c| c.shape.tag()), Some(ShapeTag::Character)) {
        crate::components::character_configure(h);
    }
}

/// Sets one shape property without remaking the collider: `Some(true)` set, `Some(false)` the
/// shape has no such property, `None` the value does not fit.
fn load_shape(h: u32, key: &str, v: &Value) -> Option<bool> {
    with_col(h, |c| -> Option<bool> {
        match (&mut c.shape, key) {
            (ColShape::Box { size }, "size") => *size = vec3(v)?,
            (ColShape::Sphere { radius }, "radius") => *radius = v.as_f64()?,
            (ColShape::Capsule { radius, .. }, "radius") => *radius = v.as_f64()?,
            (ColShape::Capsule { height, .. }, "height") => *height = v.as_f64()?,
            (ColShape::Mesh { points, .. }, "points") => *points = vec3s(v)?,
            (ColShape::Mesh { indices, .. }, "indices") => {
                *indices = v
                    .as_array()?
                    .iter()
                    .map(Value::as_f64)
                    .collect::<Option<Vec<_>>>()?
            }
            (ColShape::Mesh { convex, .. }, "convex") => *convex = v.as_bool()?,
            (ColShape::Character { radius, .. }, "radius") => *radius = v.as_f64()?,
            (ColShape::Character { height, .. }, "height") => *height = v.as_f64()?,
            (ColShape::Character { slope_limit, .. }, "slopeLimit") => *slope_limit = v.as_f64()?,
            (ColShape::Character { step_offset, .. }, "stepOffset") => *step_offset = v.as_f64()?,
            (ColShape::Box2D { size }, "size") => *size = vec2(v)?,
            (ColShape::Circle2D { radius }, "radius") => *radius = v.as_f64()?,
            (ColShape::Capsule2D { size, .. }, "size") => *size = vec2(v)?,
            (ColShape::Capsule2D { vertical, .. }, "direction") => {
                *vertical = v.as_str()? != "horizontal"
            }
            (ColShape::Polygon2D { points } | ColShape::Edge2D { points }, "points") => {
                *points = vec2s(v)?
            }
            _ => return Some(false),
        }
        Some(true)
    })
    .flatten()
}

fn load_joint(h: u32, props: &Props, w: &mut Vec<String>) {
    for (k, v) in props {
        let ok: Option<()> = with_joint(h, |j| -> Option<()> {
            match k.as_str() {
                "connectedBody" => j.connected = parse_reference(v)?,
                "anchor" => j.anchor = vec3(v)?,
                "connectedAnchor" => j.connected_anchor = vec3(v)?,
                "autoConfigureConnectedAnchor" => j.auto_anchor = v.as_bool()?,
                "axis" => j.axis = vec3(v)?,
                "useLimits" => j.use_limits = v.as_bool()?,
                "limits" => {
                    j.min = v.get("min")?.as_f64()?;
                    j.max = v.get("max")?.as_f64()?;
                }
                "useMotor" => j.use_motor = v.as_bool()?,
                "motor" => {
                    j.target_velocity = v.get("targetVelocity")?.as_f64()?;
                    j.force = v.get("force")?.as_f64()?;
                }
                "spring" => j.spring = v.as_f64()?,
                "damper" => j.damper = v.as_f64()?,
                "restLength" => j.rest_length = v.as_f64().unwrap_or(f64::NAN),
                "maxDistance" => j.max_distance = v.as_f64()?,
                _ => {
                    w.push(format!("no property `{k}` yet; dropped"));
                }
            }
            Some(())
        })
        .flatten();
        if ok.is_none() {
            w.push(warn_shape(k));
        }
    }
    comp::joint_build(h);
}

fn load_rope(h: u32, props: &Props, w: &mut Vec<String>) {
    for (k, v) in props {
        let ok: Option<()> = with_rope(h, |r: &mut RopeD| -> Option<()> {
            match k.as_str() {
                "length" => r.length = v.as_f64()?,
                "segments" => r.segments = v.as_f64()?,
                "radius" => r.radius = v.as_f64()?,
                "segmentMass" => r.segment_mass = v.as_f64()?,
                "end" => r.end = parse_reference(v)?,
                _ => {
                    w.push(format!("no property `{k}` yet; dropped"));
                }
            }
            Some(())
        })
        .flatten();
        if ok.is_none() {
            w.push(warn_shape(k));
        }
    }
}

fn kind(class: &'static str) -> Kind {
    Kind {
        name: class.into(),
        create: Box::new(move |o| create(class, ObjectId::from_raw(o))),
        on: Box::new(move |o| {
            comp::comps_on(ObjectId::from_raw(o), |d| is_class(class, d))
                .into_iter()
                .map(u64::from)
                .collect()
        }),
        save: Box::new(|h| save(h as u32)),
        load: Box::new(|h, props| load(h as u32, props)),
        awake: Box::new(|_| {}),
        object: Box::new(|h| with_comp(h as u32, |c| c.object.to_raw())),
        destroy: Box::new(|h| destroy(h as u32)),
    }
}

/// Every physics component with its object and class name (Rigidbody, colliders, joints, ropes),
/// in the order they were added.
pub fn component_objects() -> Vec<(ObjectId, &'static str)> {
    comp::all_comps(|object, d| {
        CLASSES
            .iter()
            .find(|c| is_class(c, d))
            .map(|c| (object, *c))
    })
}

/// Registers the physics component kinds on this thread (a later registration replaces an
/// earlier one).
pub fn register_kinds() {
    for class in CLASSES {
        platypus_kinds::register(kind(class));
    }
    crate::kind_fields::register();
}
