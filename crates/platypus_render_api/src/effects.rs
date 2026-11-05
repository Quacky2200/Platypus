// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The hand-written parts of the particle, line, cloth and primitive components: JSON helpers
//! for their kinds, what the host calls (`report_particles`, `take_particle_controls`,
//! `pre_render_lines`) and the geometry of `LineRenderer`'s tube.

use super::*;
use platypus_scene::api::{TransformRef, scene_api};
use std::collections::HashMap;

// ---- JSON helpers ----

/// A number when both ends are the same, else `[min, max]`.
pub(crate) fn minmax_json(m: [f64; 2]) -> Value {
    if m[0] == m[1] {
        json!(m[0])
    } else {
        json!([m[0], m[1]])
    }
}

pub(crate) fn minmax_of(v: &Value) -> Option<[f64; 2]> {
    if let Some(n) = v.as_f64() {
        return Some([n, n]);
    }
    let a = v.as_array()?;
    Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
}

pub(crate) fn color_json(c: Rgba) -> Value {
    json!({ "$type": "Color", "value": [c.r, c.g, c.b, c.a] })
}

pub(crate) fn v3_json(v: V3) -> Value {
    json!({ "$type": "Vector3", "value": [v.x, v.y, v.z] })
}

/// `startColor`: null (white), one colour, or two to mix between.
pub(crate) fn start_color_json(a: Rgba, b: Option<Rgba>, default: bool) -> Value {
    match b {
        Some(b) => json!([color_json(a), color_json(b)]),
        None if default => Value::Null,
        None => color_json(a),
    }
}

/// The two colours of a saved `startColor` (the second only for a pair).
pub(crate) fn start_color_of(v: &Value) -> Option<(Rgba, Option<Rgba>)> {
    const WHITE: Rgba = Rgba {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    };
    match v {
        Value::Null => Some((WHITE, None)),
        Value::Array(a) if a.len() == 2 => Some((color_of(&a[0])?, Some(color_of(&a[1])?))),
        other => Some((color_of(other)?, None)),
    }
}

pub(crate) fn shape_json(kind: &str, radius: f64, angle: f64, size: V3) -> Value {
    match kind {
        "point" => json!({ "type": "point" }),
        "cone" => json!({ "type": "cone", "angle": angle, "radius": radius }),
        "box" => json!({ "type": "box", "size": { "x": size.x, "y": size.y, "z": size.z } }),
        other => json!({ "type": other, "radius": radius }),
    }
}

/// A saved `shape`: its type and the parameters it has.
/// A saved particle shape: its type, radius, angle and box size (those it has).
pub(crate) type Shape = (String, Option<f64>, Option<f64>, Option<V3>);

pub(crate) fn shape_of(v: &Value) -> Option<Shape> {
    let t = v.get("type")?.as_str()?;
    if !["point", "sphere", "hemisphere", "cone", "box"].contains(&t) {
        return None;
    }
    let size = v.get("size").and_then(|s| {
        Some(V3 {
            x: s.get("x")?.as_f64()?,
            y: s.get("y")?.as_f64()?,
            z: s.get("z")?.as_f64()?,
        })
    });
    Some((
        t.to_string(),
        v.get("radius").and_then(Value::as_f64),
        v.get("angle").and_then(Value::as_f64),
        size,
    ))
}

// ---- ParticleSystem: the host side ----

thread_local! {
    static CONTROLS: RefCell<Vec<(ParticleSystemRef, String, f64)>> = const { RefCell::new(Vec::new()) };
    static STOPPED: RefCell<HashMap<u32, Vec<Callback<()>>>> = RefCell::new(HashMap::new());
}

pub(crate) fn control(r: ParticleSystemRef, action: &str, count: f64) {
    CONTROLS.with(|c| c.borrow_mut().push((r, action.to_string(), count)));
}

/// What scripts asked of the particle systems since the last call, in order: `play`, `stop`,
/// `stopClear`, `pause`, `clear` and `emit` (with a count).
pub fn take_particle_controls() -> Vec<(ParticleSystemRef, String, f64)> {
    CONTROLS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

pub(crate) fn add_stopped(r: ParticleSystemRef, f: Callback<()>) {
    STOPPED.with(|s| s.borrow_mut().entry(r.0).or_default().push(f));
}

pub(crate) fn reset_effects() {
    CONTROLS.with(|c| c.borrow_mut().clear());
    STOPPED.with(|s| s.borrow_mut().clear());
    EFFECTS.with(|e| e.borrow_mut().clear());
    LINES_SEEN.with(|s| s.set(None));
}

/// The engine's report for a system: how many particles are alive and whether it still emits or
/// has particles left. When it stops, `onStopped` callbacks run and `stopAction: "destroy"`
/// destroys its object.
pub fn report_particles(r: ParticleSystemRef, count: u32, alive: bool) {
    let Some((was, object, destroy)) = with_particle_system(r, |d| {
        let was = d.alive;
        d.particle_count = count;
        d.alive = alive;
        (was, d.object, d.stop_action == "destroy")
    }) else {
        return;
    };
    if was && !alive {
        let callbacks = STOPPED.with(|s| s.borrow().get(&r.0).cloned().unwrap_or_default());
        for c in callbacks {
            c.call(());
        }
        if destroy {
            scene_api::destroy(object);
        }
    }
}

// ---- LineRenderer ----

thread_local! {
    /// Per line renderer: the mesh it draws into, its mesh renderer and what is drawn.
    static EFFECTS: RefCell<HashMap<u32, LineEffect>> = RefCell::new(HashMap::new());
    /// The line renderers' version when [`pre_render_lines`] last looked.
    static LINES_SEEN: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// A line renderer's mesh, its mesh renderer and what is drawn in it.
type LineEffect = (MeshRef, MeshRendererRef, Option<Drawn>);

/// What a line renderer's tube was built from: width, sides, smoothing and the points.
#[derive(Clone, PartialEq)]
pub(crate) struct Drawn(f64, i32, i32, Vec<V3>);

impl Drawn {
    fn matches(&self, d: &LineData) -> bool {
        self.0 == d.width && self.1 == d.sides && self.2 == d.smoothing && self.3 == d.positions
    }
}

/// Whether a mesh renderer belongs to a `LineRenderer` (scene files leave it out: the line makes
/// it again).
pub(crate) fn internal_renderer(r: MeshRendererRef) -> bool {
    EFFECTS.with(|e| e.borrow().values().any(|x| x.1 == r))
}

/// The mesh a cloth started from, for the simulated copy a renderer draws (scene files save the
/// original).
pub(crate) fn cloth_source(simulated: u32) -> Option<u32> {
    CLOTHS.with(|c| {
        c.borrow()
            .values()
            .find(|(_, s)| s.0 == simulated)
            .map(|(o, _)| *o)
    })
}

/// Called when a line renderer is made: it draws through a mesh renderer on its own object.
pub(crate) fn line_attached(r: LineRendererRef) {
    let Some((object, material)) = with_line_renderer(r, |d| (d.object, d.material)) else {
        return;
    };
    let mesh = MeshRef::create(
        &[V3::ZERO, V3::ZERO, V3::ZERO],
        &[0.0, 1.0, 2.0],
        &[],
        &[],
        &[],
        &[],
    );
    let renderer = attach_mesh_renderer(object);
    with_mesh_renderer(renderer, |d| {
        d.mesh = mesh.0;
        d.material = material;
    });
    EFFECTS.with(|e| e.borrow_mut().insert(r.0, (mesh, renderer, None)));
}

pub(crate) fn line_removed(r: LineRendererRef) {
    if let Some((mesh, renderer, _)) = EFFECTS.with(|e| e.borrow_mut().remove(&r.0)) {
        remove_mesh_renderer(renderer);
        mesh.destroy();
    }
}

/// Rebuilds the tube if the points or its look changed (what `redraw()` does in the TypeScript).
pub(crate) fn line_redraw(r: LineRendererRef) {
    let Some((mesh, renderer)) = EFFECTS.with(|e| e.borrow().get(&r.0).map(|x| (x.0, x.1))) else {
        return;
    };
    // Unchanged (the usual case): nothing is copied.
    let fresh = read_line_renderer(r, |d| {
        let drawn = EFFECTS.with(|e| {
            e.borrow()
                .get(&r.0)
                .and_then(|x| x.2.as_ref())
                .is_some_and(|k| k.matches(d))
        });
        (!drawn && d.positions.len() >= 2).then(|| d.clone())
    });
    // The renderer follows `material` (written only when it differs: a write is a change).
    if let Some(material) = read_line_renderer(r, |d| d.material)
        && read_mesh_renderer(renderer, |m| m.material) != Some(material)
    {
        with_mesh_renderer(renderer, |m| m.material = material);
    }
    let Some(Some(d)) = fresh else {
        return;
    };
    let key = Drawn(d.width, d.sides, d.smoothing, d.positions.clone());
    // World-space points into this object's space (usually it stays at the origin).
    let t = TransformRef(d.object);
    let local: Vec<V3> = d
        .positions
        .iter()
        .map(|p| t.inverse_transform_point(*p))
        .collect();
    let tube = tube_mesh(
        &local,
        d.width / 2.0,
        d.sides.max(3) as usize,
        d.smoothing.max(0) as usize,
    );
    set_mesh_data(
        mesh,
        &tube.vertices,
        &tube.triangles,
        &tube.normals,
        &tube.uv,
    );
    EFFECTS.with(|e| {
        if let Some(x) = e.borrow_mut().get_mut(&r.0) {
            x.2 = Some(key);
        }
    });
}

/// Rebuilds every line renderer that changed. The host calls it once a frame before drawing
/// (`onPreRender`); nothing to do while no line renderer changed.
pub fn pre_render_lines() {
    let version = line_renderers_version();
    if LINES_SEEN.with(|s| s.replace(Some(version))) == Some(version) {
        return;
    }
    let mut lines = Vec::new();
    for_each_line_renderer(|r, _| lines.push(r));
    for r in lines {
        line_redraw(r);
    }
}

/// The new default material of a line renderer: white, matte-ish (smoothness 0.3).
pub(crate) fn create_line_material() -> MaterialRef {
    let m = create_material();
    with_material(m, |d| d.smoothness = 0.3);
    m
}

fn catmull_rom(a: V3, b: V3, c: V3, d: V3, t: f64) -> V3 {
    let (t2, t3) = (t * t, t * t * t);
    let k = |p0: f64, p1: f64, p2: f64, p3: f64| {
        0.5 * (2.0 * p1
            + (-p0 + p2) * t
            + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
            + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
    };
    V3 {
        x: k(a.x, b.x, c.x, d.x),
        y: k(a.y, b.y, c.y, d.y),
        z: k(a.z, b.z, c.z, d.z),
    }
}

/// Tube geometry through `points`: rings of `sides` vertices, a frame carried along the path so
/// rings do not twist, `smoothing` extra points between each pair (Catmull-Rom).
pub struct Tube {
    pub vertices: Vec<V3>,
    pub normals: Vec<V3>,
    pub uv: Vec<V2>,
    pub triangles: Vec<f64>,
}

pub fn tube_mesh(points: &[V3], radius: f64, sides: usize, smoothing: usize) -> Tube {
    let mut path: Vec<V3> = Vec::new();
    for i in 0..points.len() - 1 {
        let a = points[i.saturating_sub(1)];
        let b = points[i];
        let c = points[i + 1];
        let d = points[(i + 2).min(points.len() - 1)];
        for s in 0..=smoothing {
            path.push(catmull_rom(a, b, c, d, s as f64 / (smoothing + 1) as f64));
        }
    }
    path.push(points[points.len() - 1]);
    let n = path.len();
    let tangents: Vec<V3> = (0..n)
        .map(|i| {
            path[(i + 1).min(n - 1)]
                .sub(path[i.saturating_sub(1)])
                .normalized()
        })
        .collect();
    let mut normal = if tangents[0].y.abs() < 0.9 {
        V3::UP
    } else {
        V3::RIGHT
    };
    normal = normal
        .sub(tangents[0].scale(normal.dot(tangents[0])))
        .normalized();
    let mut vertices = Vec::new();
    let mut normals = Vec::new();
    let mut uv = Vec::new();
    let mut along = 0.0;
    for i in 0..n {
        let t = tangents[i];
        if i > 0 {
            along += path[i].sub(path[i - 1]).magnitude();
            // Remove the part of the normal along the new tangent.
            let fixed = normal.sub(t.scale(normal.dot(t)));
            if fixed.magnitude() > 1e-6 {
                normal = fixed.normalized();
            }
        }
        let binormal = t.cross(normal);
        for s in 0..=sides {
            let a = s as f64 / sides as f64 * std::f64::consts::PI * 2.0;
            let dir = normal.scale(a.cos()).add(binormal.scale(a.sin()));
            vertices.push(path[i].add(dir.scale(radius)));
            normals.push(dir);
            uv.push(V2 {
                x: s as f64 / sides as f64,
                y: along,
            });
        }
    }
    let mut triangles = Vec::new();
    let ring = sides + 1;
    for i in 0..n - 1 {
        for s in 0..sides {
            let a = (i * ring + s) as f64;
            let b = a + ring as f64;
            triangles.extend([a, a + 1.0, b, a + 1.0, b + 1.0, b]);
        }
    }
    Tube {
        vertices,
        normals,
        uv,
        triangles,
    }
}

// ---- Cloth ----

thread_local! {
    /// Per cloth: the mesh it started with (as a mesh renderer's `mesh`) and its simulated copy.
    static CLOTHS: RefCell<HashMap<u32, (u32, MeshRef)>> = RefCell::new(HashMap::new());
}

/// Starts the simulation again from the mesh with the current settings: the object's mesh
/// renderer draws a copy the engine fills in (`simulated_mesh`). Needs a `MeshRenderer` on the
/// same object.
pub(crate) fn cloth_rebuild(r: ClothRef) {
    let Some(object) = with_cloth(r, |d| d.object) else {
        return;
    };
    let Some(renderer) = mesh_renderers_on(object).first().copied() else {
        eprintln!("render: Cloth needs a MeshRenderer on the same object");
        return;
    };
    let simulated = match CLOTHS.with(|c| c.borrow().get(&r.0).copied()) {
        Some((_, s)) => s,
        None => {
            let original = with_mesh_renderer(renderer, |d| d.mesh).unwrap_or(0);
            let s = new_engine_mesh();
            CLOTHS.with(|c| c.borrow_mut().insert(r.0, (original, s)));
            s
        }
    };
    with_cloth(r, |d| {
        d.rebuild_requests += 1.0;
        d.source_mesh = CLOTHS.with(|c| c.borrow().get(&r.0).map_or(0, |x| x.0));
        d.simulated_mesh = simulated.0;
    });
    with_mesh_renderer(renderer, |d| d.mesh = simulated.0);
}

/// Puts the original mesh back and frees the copy.
pub(crate) fn cloth_removed(r: ClothRef) {
    let Some((original, simulated)) = CLOTHS.with(|c| c.borrow_mut().remove(&r.0)) else {
        return;
    };
    if let Some(object) = with_cloth(r, |d| d.object)
        && let Some(renderer) = mesh_renderers_on(object).first().copied()
    {
        with_mesh_renderer(renderer, |d| d.mesh = original);
    }
    simulated.destroy();
}

// ---- Primitives ----

/// Adds the collider of a primitive to its object.
pub type ColliderHook = Rc<dyn Fn(ObjectId, &str)>;

thread_local! {
    static COLLIDER_HOOK: RefCell<Option<ColliderHook>> = RefCell::new(None);
}

use std::rc::Rc;

/// Lets the physics side add the matching collider to the objects `Primitives.create` makes (a
/// cube gets a box collider, ...): `hook(object, primitive_name)`.
pub fn set_primitive_collider_hook(hook: Option<ColliderHook>) {
    COLLIDER_HOOK.with(|h| *h.borrow_mut() = hook);
}

/// An object with a built-in mesh and the matching collider (Unity's `CreatePrimitive`).
pub fn create_primitive(kind: &str, name: &str) -> ObjectId {
    let label = if name.is_empty() {
        let mut c = kind.chars();
        c.next()
            .map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
    } else {
        name.to_string()
    };
    let object = scene_api::create();
    object.set_name(&label);
    let r = attach_mesh_renderer(object);
    with_mesh_renderer(r, |d| d.mesh = primitive_mesh(kind).0);
    if let Some(h) = COLLIDER_HOOK.with(|h| h.borrow().clone()) {
        h(object, kind);
    }
    object
}

/// The built-in meshes (Unity's `PrimitiveType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    Cube,
    Sphere,
    Capsule,
    Cylinder,
    Plane,
    Quad,
}

#[ts_api(
    namespace = "Primitives", handles(GameObject = ObjectId),
    enums(extern PrimitiveType = Primitive [Cube, Sphere, Capsule, Cylinder, Plane, Quad])
)]
pub mod primitives_api {
    use super::primitive_type_api::{primitive_type_from_index, primitive_type_name};
    use super::*;

    /// A GameObject with a built-in mesh (`cube`, `sphere`, `capsule`, `cylinder`, `plane`,
    /// `quad`) and its collider, once the physics side has registered a hook. Unity's
    /// `GameObject.CreatePrimitive`.
    #[optional(name)]
    pub fn create(kind: Primitive, name: &str) -> ObjectId {
        super::create_primitive(primitive_type_name(kind), name)
    }
}

/// The names of the built-in meshes: `PrimitiveType.Cube` is `"cube"`, and `PrimitiveType.name(i)`
/// / `.parse(s)` convert the index.
#[ts_api(
    namespace = "PrimitiveType", enums(
        /// A built-in mesh (Unity's `PrimitiveType`).
        PrimitiveType = Primitive [Cube, Sphere, Capsule, Cylinder, Plane, Quad]
    ),
    aliases(PrimitiveMesh = PrimitiveType)
)]
pub mod primitive_type_api {
    use super::*;

    #[property]
    #[js_name = "Cube"]
    pub fn cube() -> Primitive {
        Primitive::Cube
    }
    #[property]
    #[js_name = "Sphere"]
    pub fn sphere() -> Primitive {
        Primitive::Sphere
    }
    #[property]
    #[js_name = "Capsule"]
    pub fn capsule() -> Primitive {
        Primitive::Capsule
    }
    #[property]
    #[js_name = "Cylinder"]
    pub fn cylinder() -> Primitive {
        Primitive::Cylinder
    }
    #[property]
    #[js_name = "Plane"]
    pub fn plane() -> Primitive {
        Primitive::Plane
    }
    #[property]
    #[js_name = "Quad"]
    pub fn quad() -> Primitive {
        Primitive::Quad
    }
}

pub(crate) fn effect_manifests() -> Vec<String> {
    vec![
        primitives_api::MANIFEST.to_json(),
        primitive_type_api::MANIFEST.to_json(),
    ]
}
