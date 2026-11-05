// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! glTF models (`js/engine/src/models.ts`), Unity-prefab style. `Model.load(path)` asks the host to
//! read a `.glb` / `.gltf` ([`RenderCommand::Model`]); the host uploads its meshes and textures
//! ([`engine_mesh`], [`engine_texture`]) and reports the description with [`model_loaded`] (the
//! JSON of the TypeScript: nodes, roots, materials, skins). `instantiate` builds the node tree as
//! GameObjects with mesh renderers; asked before loading finishes, the nodes appear once it has.
//!
//! The file's animations become clips (`Model.animations`, built by `platypus_animation::gltf`), and a
//! copy gets an `Animation` playing the first. The colliders option needs the physics side to
//! call [`set_model_collider_hook`].

use super::*;
use platypus_engine_api::animation::{AnimationRef, ClipRef};
use platypus_engine_api::assets;
use platypus_math::Q;
use platypus_scene::api::scene_api;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

numbered_handle!(
    /// A model by number (1-based: 0 is "no model").
    ModelRef
);

struct ModelData {
    src: String,
    loaded: bool,
    failed: bool,
    description: Option<Value>,
    materials: Vec<MaterialRef>,
    meshes: Vec<MeshRef>,
    /// The file's animations as clips (looping), paths from the copy's root.
    animations: Vec<ClipRef>,
    /// Copies asked for before it loaded: the root and the `colliders` option.
    pending: Vec<(ObjectId, String)>,
}

#[derive(Default)]
struct State {
    models: Vec<ModelData>,
    cache: HashMap<String, ModelRef>,
    /// Objects that are copies of a model (the root), with the file and the colliders option.
    roots: HashMap<u64, (String, String)>,
    /// Objects that are the model's own nodes (not saved: the root rebuilds them).
    parts: HashSet<u64>,
    /// `whenLoaded()` promises of models still loading: (model, resolve, reject).
    waiters: Vec<ModelWaiter>,
}

type ModelWaiter = (
    u32,
    Callback<(ts_api::callback::HandleArg<ModelRef>,)>,
    Callback<(String,)>,
);

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Adds the collider of a model's mesh to the object holding it: `hook(object, mesh, convex)`.
pub type ModelColliderHook = Rc<dyn Fn(ObjectId, MeshRef, bool)>;

thread_local! {
    static COLLIDER_HOOK: RefCell<Option<ModelColliderHook>> = RefCell::new(None);
}

/// Lets the physics side add `mesh` / `convex` colliders when a model is placed with the
/// `colliders` option.
pub fn set_model_collider_hook(hook: Option<ModelColliderHook>) {
    COLLIDER_HOOK.with(|h| *h.borrow_mut() = hook);
}

pub(crate) fn reset_models() {
    st(|s| *s = State::default());
}

pub(crate) fn forget_objects(objects: &[ObjectId]) {
    st(|s| {
        for o in objects {
            s.roots.remove(&o.to_raw());
            s.parts.remove(&o.to_raw());
        }
    });
}

fn with_model<R>(m: ModelRef, f: impl FnOnce(&mut ModelData) -> R) -> Option<R> {
    st(|s| s.models.get_mut((m.0 as usize).checked_sub(1)?).map(f))
}

/// The files the host has to read: every model asked for and not loaded yet.
pub fn model_src(m: ModelRef) -> Option<String> {
    with_model(m, |d| d.src.clone())
}

/// A texture the host made for a model (its size known): the description names it by number.
pub fn engine_texture(width: u32, height: u32) -> TextureRef {
    new_loaded_texture(width, height)
}

/// A mesh the host made for a model: the description names it by number.
pub fn engine_mesh() -> MeshRef {
    new_engine_mesh()
}

fn n(v: &Value, i: usize) -> f64 {
    v.get(i).and_then(Value::as_f64).unwrap_or(0.0)
}

fn material_of(m: &Value) -> MaterialRef {
    let r = create_material();
    let tex = |v: Option<&Value>| TextureRef(v.and_then(Value::as_u64).unwrap_or(0) as u32);
    with_material(r, |d| {
        if let Some(c) = m.get("color") {
            d.color = Rgba {
                r: n(c, 0),
                g: n(c, 1),
                b: n(c, 2),
                a: n(c, 3),
            };
        }
        if let Some(e) = m.get("emission") {
            d.emission = Rgba {
                r: n(e, 0),
                g: n(e, 1),
                b: n(e, 2),
                a: 1.0,
            };
        }
        d.smoothness = m
            .get("smoothness")
            .and_then(Value::as_f64)
            .unwrap_or(d.smoothness);
        d.metallic = m.get("metallic").and_then(Value::as_f64).unwrap_or(0.0);
        d.unlit = m.get("unlit").and_then(Value::as_bool).unwrap_or(false);
        d.main_texture = tex(m.get("texture").and_then(|t| t.get(0))).0;
        d.rendering_mode = match m.get("alpha").and_then(Value::as_str) {
            Some("mask") => "cutout",
            Some("blend") => "fade",
            _ => "opaque",
        }
        .to_string();
        d.alpha_cutoff = m.get("alphaCutoff").and_then(Value::as_f64).unwrap_or(0.5);
        d.double_sided = m
            .get("doubleSided")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        d.normal_map = tex(m.get("normal").and_then(|t| t.get(0))).0;
        d.normal_scale = m
            .get("normal")
            .and_then(|t| t.get(1))
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        d.metallic_gloss_map = tex(m.get("metallicRoughness")).0;
        d.occlusion_map = tex(m.get("occlusion").and_then(|t| t.get(0))).0;
        d.occlusion_strength = m
            .get("occlusion")
            .and_then(|t| t.get(1))
            .and_then(Value::as_f64)
            .unwrap_or(1.0);
        d.emission_map = tex(m.get("emissive")).0;
        d.gltf_packing = true;
    });
    r
}

/// The glTF's animations as clips, paths by node names from the copy's root.
fn clips_of(d: &Value) -> Vec<ClipRef> {
    let nodes = d["nodes"].as_array().cloned().unwrap_or_default();
    let mut paths: HashMap<usize, String> = HashMap::new();
    let mut stack: Vec<(usize, String)> = d["roots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_u64().map(|r| (r as usize, String::new())))
        .collect();
    while let Some((index, prefix)) = stack.pop() {
        let Some(node) = nodes.get(index) else {
            continue;
        };
        let name = match node["name"].as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => format!("Node {index}"),
        };
        let path = prefix + &name;
        for c in node["children"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
        {
            stack.push((c as usize, format!("{path}/")));
        }
        paths.insert(index, path);
    }
    let numbers = |v: &Value| -> Vec<f64> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect()
    };
    d["animations"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, a)| {
            let name = match a["name"].as_str() {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => format!("Animation {i}"),
            };
            // The channels' keys as numbers, kept while the clip borrows them.
            let found: Vec<(&Value, &String)> = a["channels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|ch| Some((ch, paths.get(&(ch["node"].as_u64()? as usize))?)))
                .collect();
            let keys: Vec<[Vec<f64>; 2]> = found
                .iter()
                .map(|(ch, _)| [numbers(&ch["times"]), numbers(&ch["values"])])
                .collect();
            let channels: Vec<platypus_animation::gltf::Channel> = found
                .iter()
                .zip(&keys)
                .map(
                    |((ch, path), [times, values])| platypus_animation::gltf::Channel {
                        path,
                        target: ch["path"].as_str().unwrap_or(""),
                        interpolation: ch["interpolation"].as_str().unwrap_or("linear"),
                        times,
                        values,
                    },
                )
                .collect();
            let id = platypus_animation::with_world(|w| {
                platypus_animation::gltf::clip_from_channels(w, &name, &channels)
            });
            ClipRef(u64::from(id.0))
        })
        .collect()
}

/// The host read the file: its description (the TypeScript's JSON), or `None` when it failed.
/// Models asked for meanwhile are built now.
pub fn model_loaded(m: ModelRef, description: Option<&str>) {
    let parsed: Option<Value> = description.and_then(|t| serde_json::from_str(t).ok());
    let pending = with_model(m, |d| {
        match &parsed {
            Some(desc) => {
                d.loaded = true;
                d.materials = desc["materials"]
                    .as_array()
                    .map(|a| a.iter().map(material_of).collect())
                    .unwrap_or_default();
                d.meshes = desc["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|node| node["primitives"].as_array().into_iter().flatten())
                    .filter_map(|p| p["mesh"].as_u64())
                    .map(|id| MeshRef(id as u32))
                    .collect();
                d.animations = clips_of(desc);
                d.description = Some(desc.clone());
            }
            None => d.failed = true,
        }
        std::mem::take(&mut d.pending)
    })
    .unwrap_or_default();
    if parsed.is_some() {
        for (root, colliders) in pending {
            if platypus_scene::api::with_scene(|s| s.contains(root)).unwrap_or(false) {
                build(m, root, &colliders);
            }
        }
    }
    // The promises that were waiting hear it.
    let waiting: Vec<ModelWaiter> = st(|s| {
        let (mine, rest) = std::mem::take(&mut s.waiters)
            .into_iter()
            .partition(|w| w.0 == m.0);
        s.waiters = rest;
        mine
    });
    for (_, resolve, reject) in waiting {
        if parsed.is_some() {
            resolve.call((ts_api::callback::HandleArg(m),));
        } else {
            reject.call(("model failed to load".to_string(),));
        }
    }
}

fn mark_part(o: ObjectId) {
    st(|s| s.parts.insert(o.to_raw()));
}

/// Builds the model's nodes under `root` (now, or once it has loaded).
pub fn build_into(m: ModelRef, root: ObjectId, colliders: &str) {
    let src = model_src(m).unwrap_or_default();
    st(|s| s.roots.insert(root.to_raw(), (src, colliders.to_string())));
    let ready = with_model(m, |d| {
        if d.description.is_some() {
            true
        } else {
            if !d.failed {
                d.pending.push((root, colliders.to_string()));
            }
            false
        }
    })
    .unwrap_or(false);
    if ready {
        build(m, root, colliders);
    }
}

fn build(m: ModelRef, root: ObjectId, colliders: &str) {
    let Some((d, materials, animations)) = with_model(m, |d| {
        (
            d.description.clone(),
            d.materials.clone(),
            d.animations.clone(),
        )
    }) else {
        return;
    };
    let Some(d) = d else { return };
    let nodes = d["nodes"].as_array().cloned().unwrap_or_default();
    let skins = d["skins"].as_array().cloned().unwrap_or_default();
    let mut objects: HashMap<usize, ObjectId> = HashMap::new();
    struct Skinned {
        holder: ObjectId,
        mesh: MeshRef,
        material: MaterialRef,
        skin: usize,
    }
    let mut skinned: Vec<Skinned> = Vec::new();
    let hook = COLLIDER_HOOK.with(|h| h.borrow().clone());

    fn make_object(name: &str, parent: ObjectId, pos: V3, rot: Q, scale: V3) -> ObjectId {
        let go = scene_api::create();
        go.set_name(name);
        scene_api::set_parent(go, parent, false);
        scene_api::set_local_position(go, pos);
        scene_api::set_local_rotation(go, rot);
        scene_api::set_local_scale(go, scale);
        mark_part(go);
        go
    }

    // Depth-first, children after their parent, as the TypeScript's recursion.
    let mut stack: Vec<(usize, ObjectId)> = d["roots"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r.as_u64().map(|r| (r as usize, root)))
        .collect();
    stack.reverse();
    while let Some((index, parent)) = stack.pop() {
        let Some(node) = nodes.get(index) else {
            continue;
        };
        let name = match node["name"].as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => format!("Node {index}"),
        };
        let go = make_object(
            &name,
            parent,
            V3 {
                x: n(&node["position"], 0),
                y: n(&node["position"], 1),
                z: n(&node["position"], 2),
            },
            Q {
                x: n(&node["rotation"], 0),
                y: n(&node["rotation"], 1),
                z: n(&node["rotation"], 2),
                w: node["rotation"]
                    .get(3)
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0),
            },
            V3 {
                x: node["scale"].get(0).and_then(Value::as_f64).unwrap_or(1.0),
                y: node["scale"].get(1).and_then(Value::as_f64).unwrap_or(1.0),
                z: node["scale"].get(2).and_then(Value::as_f64).unwrap_or(1.0),
            },
        );
        objects.insert(index, go);
        let prims = node["primitives"].as_array().cloned().unwrap_or_default();
        for (i, p) in prims.iter().enumerate() {
            // One renderer per part: on the node itself, or on child objects when there are several.
            let holder = if prims.len() == 1 {
                go
            } else {
                make_object(
                    &format!("{name} {i}"),
                    go,
                    V3::ZERO,
                    Q::IDENTITY,
                    V3 {
                        x: 1.0,
                        y: 1.0,
                        z: 1.0,
                    },
                )
            };
            let mesh = MeshRef(p["mesh"].as_u64().unwrap_or(0) as u32);
            let material = p["material"]
                .as_u64()
                .and_then(|m| materials.get(m as usize).copied())
                .unwrap_or_else(create_material);
            let skin = node["skin"]
                .as_u64()
                .map(|s| s as usize)
                .filter(|s| *s < skins.len());
            match skin {
                Some(skin) => skinned.push(Skinned {
                    holder,
                    mesh,
                    material,
                    skin,
                }),
                None => {
                    let r = attach_mesh_renderer(holder);
                    with_mesh_renderer(r, |d| {
                        d.mesh = mesh.0;
                        d.material = material.0;
                    });
                }
            }
            if (colliders == "mesh" || colliders == "convex")
                && let Some(h) = &hook
            {
                h(holder, mesh, colliders == "convex");
            }
        }
        for c in node["children"]
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .filter_map(|c| c.as_u64())
        {
            stack.push((c as usize, go));
        }
    }
    // Skinned parts wait for every bone to exist.
    for s in skinned {
        let joints: Vec<usize> = skins[s.skin]["joints"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|j| j.as_u64().map(|j| j as usize))
            .collect();
        let inverse: Vec<f64> = skins[s.skin]["inverseBind"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_f64)
            .collect();
        s.mesh.set_bindposes(&inverse);
        let bones: Vec<ObjectId> = joints
            .iter()
            .filter_map(|j| objects.get(j).copied())
            .collect();
        let r = attach_skinned_mesh_renderer(s.holder);
        with_skinned_mesh_renderer(r, |d| {
            d.mesh = s.mesh.0;
            d.material = s.material.0;
            d.root_bone = bones.first().copied();
            d.bones = bones;
        });
    }
    // The file's animations play on the copy: the first by itself.
    if let Some(first) = animations.first()
        && AnimationRef::find_on(root).0 == 0
    {
        let a = AnimationRef::attach(root);
        for c in &animations {
            a.add_clip(*c, "");
        }
        a.set_clip(*first);
    }
}

#[ts_api(
    namespace = "Model", handles(Model = ModelRef, GameObject = ObjectId, Mesh = MeshRef, Material = MaterialRef,
        AnimationClip = ClipRef),
    values(Vector3 = V3 [x: f64, y: f64, z: f64], Quaternion = Q [x: f64, y: f64, z: f64, w: f64])
)]
pub mod model_api {
    use super::*;

    impl ModelRef {
        /// A promise of the model once it has loaded (resolved at once when it has); rejects with an
        /// `Error` when loading failed.
        #[promise]
        pub fn when_loaded(
            self,
            resolve: Callback<(ts_api::callback::HandleArg<ModelRef>,)>,
            reject: Callback<(String,)>,
        ) {
            match with_model(self, |d| (d.loaded, d.failed)) {
                Some((_, true)) => reject.call(("model failed to load".to_string(),)),
                Some((true, _)) => resolve.call((ts_api::callback::HandleArg(self),)),
                Some(_) => st(|s| s.waiters.push((self.0, resolve, reject))),
                None => reject.call(("the model was freed".to_string(),)),
            }
        }

        /// Loads a `.glb`, `.gltf` or `.fbx` from the game folder, or `"guid:<guid>"` (once per file).
        pub fn load(source: &str) -> ModelRef {
            super::load_model(source)
        }

        /// The file.
        #[property]
        pub fn src(self) -> String {
            with_model(self, |d| d.src.clone()).unwrap_or_default()
        }

        #[property]
        pub fn is_loaded(self) -> bool {
            with_model(self, |d| d.loaded).unwrap_or(false)
        }

        /// True when loading failed (the host logs why).
        #[property]
        pub fn failed(self) -> bool {
            with_model(self, |d| d.failed).unwrap_or(false)
        }

        /// How many materials it has (after loading).
        #[property]
        pub fn material_count(self) -> f64 {
            with_model(self, |d| d.materials.len() as f64).unwrap_or(0.0)
        }

        pub fn material_at(self, index: f64) -> MaterialRef {
            with_model(self, |d| d.materials.get(index as usize).copied())
                .flatten()
                .unwrap_or(MaterialRef(0))
        }

        /// Its materials, in the file's order (after loading).
        #[property]
        pub fn materials(self) -> Vec<MaterialRef> {
            with_model(self, |d| d.materials.clone()).unwrap_or_default()
        }

        /// The file's animations as clips that loop, with paths from a copy's root (after
        /// loading). A copy plays the first by itself (its `Animation`).
        #[property]
        pub fn animations(self) -> Vec<ClipRef> {
            with_model(self, |d| d.animations.clone()).unwrap_or_default()
        }

        /// How many meshes it has, one per glTF primitive (after loading).
        #[property]
        pub fn mesh_count(self) -> f64 {
            with_model(self, |d| d.meshes.len() as f64).unwrap_or(0.0)
        }

        pub fn mesh_at(self, index: f64) -> MeshRef {
            with_model(self, |d| d.meshes.get(index as usize).copied())
                .flatten()
                .unwrap_or(MeshRef(0))
        }

        /// Places a copy: a GameObject named after the file, holding the model's nodes as children.
        /// Works before loading finishes too: the nodes appear once it has. A position, rotation or
        /// scale left out (NaN) is the default; `colliders` is `mesh` or `convex` (needs the
        /// physics side), else none.
        #[optional(position, rotation, scale, parent, colliders)]
        pub fn instantiate(
            self,
            position: V3,
            rotation: Q,
            scale: V3,
            parent: ObjectId,
            colliders: &str,
        ) -> ObjectId {
            super::instantiate_model(self, position, rotation, scale, parent, colliders)
        }
    }
}

fn load_model(source: &str) -> ModelRef {
    let src = assets::resolve(source);
    if let Some(m) = st(|s| s.cache.get(&src).copied()) {
        return m;
    }
    let m = st(|s| {
        s.models.push(ModelData {
            src: src.clone(),
            loaded: false,
            failed: false,
            description: None,
            materials: Vec::new(),
            meshes: Vec::new(),
            animations: Vec::new(),
            pending: Vec::new(),
        });
        let m = ModelRef(s.models.len() as u32);
        s.cache.insert(src.clone(), m);
        m
    });
    push_command(RenderCommand::Model(m));
    m
}

fn instantiate_model(
    m: ModelRef,
    position: V3,
    rotation: Q,
    scale: V3,
    parent: ObjectId,
    colliders: &str,
) -> ObjectId {
    let src = model_src(m).unwrap_or_default();
    let stem = src.rsplit('/').next().unwrap_or("Model");
    // The file's name without its extension (`.glb`, `.gltf`, `.fbx`, any case).
    let name = stem
        .rsplit_once('.')
        .filter(|(base, ext)| {
            !base.is_empty() && ["glb", "gltf", "fbx"].contains(&ext.to_lowercase().as_str())
        })
        .map_or(stem, |(base, _)| base);
    let root = scene_api::create();
    root.set_name(name);
    if parent.to_raw() != 0 {
        scene_api::set_parent(root, parent, false);
    }
    if !position.x.is_nan() {
        scene_api::set_local_position(root, position);
    }
    if !rotation.x.is_nan() {
        scene_api::set_local_rotation(root, rotation);
    }
    if !scale.x.is_nan() {
        scene_api::set_local_scale(root, scale);
    }
    build_into(m, root, colliders);
    root
}

pub(crate) fn model_manifests() -> Vec<String> {
    vec![model_api::MANIFEST.to_json()]
}

/// Registers how scene files carry a model copy: `"model": { "src", "colliders"?, "guid"? }`.
pub(crate) fn register_model_extra() {
    platypus_kinds::register_object_extra(platypus_kinds::ObjectExtra {
        key: "model".into(),
        save: Box::new(|o| {
            let (src, colliders) = st(|s| s.roots.get(&o).cloned())?;
            let guid = assets::assets_api::path_to_guid(&src);
            let mut m = Map::new();
            m.insert("src".into(), json!(src));
            if !colliders.is_empty() && colliders != "none" {
                m.insert("colliders".into(), json!(colliders));
            }
            if !guid.is_empty() {
                m.insert("guid".into(), json!(guid));
            }
            Some(Value::Object(m))
        }),
        load: Box::new(|o, v| {
            let mut src = v
                .get("src")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if let Some(g) = v.get("guid").and_then(Value::as_str) {
                let p = assets::assets_api::guid_to_path(g);
                if !p.is_empty() {
                    src = p;
                }
            }
            let colliders = v
                .get("colliders")
                .and_then(Value::as_str)
                .unwrap_or_default();
            build_into(load_model(&src), ObjectId::from_raw(o), colliders);
        }),
        hidden: Box::new(|o| st(|s| s.parts.contains(&o))),
    });
}
