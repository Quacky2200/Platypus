// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Saving objects to scene data and building them back: the format of `js/engine/src/scenes.ts`
//! (`platypus.scene` version 1), over the component kinds in [`platypus_kinds`].
//!
//! Objects and components get file ids; a property that points at another object or component is
//! saved as `{"$ref":"GameObject"|"Component","id":N}` and resolved after everything is built.
//! `instantiate` saves and loads through the same data, keeping references to things outside the
//! copy as `{"$ref":..,"live":..}`.

use glam::{Quat, Vec3};
use platypus_kinds::Kind;
use platypus_scene::api::with_scene;
use platypus_scene::{ObjectId, Scene, Transform};
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use ts_api::Handle;

/// `HideFlags.DontSave`: any of these bits keeps an object out of saved scenes.
const DONT_SAVE: u32 = 52;

thread_local! {
    static WARNINGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The prefab file an object was placed from (kept for tools; objects are saved in full).
    static PREFAB_OF: RefCell<HashMap<ObjectId, String>> = RefCell::new(HashMap::new());
    /// Components of kinds this program doesn't register, kept per object (see [`Opaque`]).
    static OPAQUE_OF: RefCell<HashMap<ObjectId, Vec<Opaque>>> = RefCell::new(HashMap::new());
    static NEXT_OPAQUE: std::cell::Cell<u64> = const { std::cell::Cell::new(OPAQUE_BASE) };
}

/// Stand-in handles of opaque components start here, far from any kind's own handles.
const OPAQUE_BASE: u64 = 1 << 48;

/// A component whose kind isn't registered in this program (a game's script class, in the
/// editor, which hasn't compiled the game's code): kept as the file had it and written back by
/// the saver, so opening and saving a scene doesn't drop it.
#[derive(Clone, Debug, PartialEq)]
pub struct Opaque {
    /// The kind's name (`type` in the file).
    pub ty: String,
    /// A stand-in handle, so references to it get file ids like a real component's.
    pub handle: u64,
    /// Its place among the object's components in the file.
    pub index: usize,
    /// The component's data without `id` and `type`; references are placeholders.
    pub data: Map<String, Value>,
}

/// The opaque components on an object (kinds this program doesn't register), in file order.
pub fn opaque_components(object: ObjectId) -> Vec<Opaque> {
    OPAQUE_OF.with(|o| o.borrow().get(&object).cloned().unwrap_or_default())
}

/// Drops an object's opaque component with that stand-in handle; whether there was one.
pub fn remove_opaque(object: ObjectId, handle: u64) -> bool {
    OPAQUE_OF.with(|o| {
        let mut o = o.borrow_mut();
        let Some(list) = o.get_mut(&object) else {
            return false;
        };
        let before = list.len();
        list.retain(|c| c.handle != handle);
        before != list.len()
    })
}

/// Replaces an object's opaque component of the same stand-in handle (an edit of its data).
pub(crate) fn set_opaque(object: ObjectId, opaque: Opaque) {
    OPAQUE_OF.with(|o| {
        if let Some(c) = o
            .borrow_mut()
            .get_mut(&object)
            .and_then(|l| l.iter_mut().find(|c| c.handle == opaque.handle))
        {
            *c = opaque;
        }
    });
}

fn add_opaque(object: ObjectId, opaque: Opaque) {
    OPAQUE_OF.with(|o| o.borrow_mut().entry(object).or_default().push(opaque));
}

/// Forgets opaque components of objects that are gone.
fn prune_opaque() {
    OPAQUE_OF.with(|o| {
        let mut o = o.borrow_mut();
        if !o.is_empty() {
            o.retain(|object, _| sc(|s| s.contains(*object)).unwrap_or(false));
        }
    });
}

/// Reports a problem with a scene file or a component's data. The text goes to stderr and to
/// [`take_warnings`].
pub fn warn(message: impl Into<String>) {
    let message = format!("scene: {}", message.into());
    eprintln!("{message}");
    WARNINGS.with(|w| w.borrow_mut().push(message));
}

/// The warnings since the last call.
pub fn take_warnings() -> Vec<String> {
    WARNINGS.with(|w| std::mem::take(&mut *w.borrow_mut()))
}

pub(crate) fn sc<R>(f: impl FnOnce(&mut Scene) -> R) -> Option<R> {
    with_scene(f)
}

pub(crate) fn set_prefab(object: ObjectId, path: &str) {
    PREFAB_OF.with(|p| p.borrow_mut().insert(object, path.to_owned()));
}

fn prefab_of(object: ObjectId) -> Option<String> {
    PREFAB_OF.with(|p| p.borrow().get(&object).cloned())
}

/// The components on an object, kind by kind in registration order.
pub(crate) fn components_of(raw: u64) -> Vec<(Rc<Kind>, u64)> {
    let mut out = Vec::new();
    for k in platypus_kinds::all() {
        for h in (k.on)(raw) {
            out.push((k.clone(), h));
        }
    }
    out
}

fn unsaved(object: ObjectId) -> bool {
    sc(|s| s.hide_flags(object) & DONT_SAVE != 0).unwrap_or(true)
        || platypus_kinds::object_extras()
            .iter()
            .any(|e| (e.hidden)(object.to_raw()))
}

fn children(object: ObjectId) -> Vec<ObjectId> {
    sc(|s| s.children(object).to_vec()).unwrap_or_default()
}

#[derive(Hash, PartialEq, Eq, Clone)]
enum Key {
    Object(u64),
    Component(String, u64),
}

/// Saving: file ids for objects and components.
pub(crate) struct Saver {
    /// `instantiate`: things outside what is saved stay live references, else they become null.
    keep_external: bool,
    /// Assets stay live placeholders (the copy shares the handles) instead of going to the
    /// `assets` table: `instantiate` and prefabs made in memory.
    live_assets: bool,
    ids: HashMap<Key, u64>,
    /// The `assets` table of the file being saved, and the table id of each shared asset.
    assets: RefCell<Map<String, Value>>,
    asset_ids: RefCell<HashMap<(String, u64), String>>,
}

impl Saver {
    pub fn new(keep_external: bool) -> Self {
        Saver {
            keep_external,
            live_assets: keep_external,
            ids: HashMap::new(),
            assets: RefCell::new(Map::new()),
            asset_ids: RefCell::new(HashMap::new()),
        }
    }

    /// Keeps asset placeholders live (a prefab in memory).
    pub fn keep_assets_live(&mut self) {
        self.live_assets = true;
    }

    /// The `assets` table collected while saving (empty when no shared asset was used).
    pub fn take_assets(&self) -> Map<String, Value> {
        std::mem::take(&mut *self.assets.borrow_mut())
    }

    /// An asset placeholder as the file holds it: a table id (shared kinds) or the inline
    /// definition. A kind that does not exist, or an asset that cannot be saved, is null.
    fn asset(&self, kind_name: &str, handle: u64) -> Value {
        let Some(kind) = platypus_kinds::get_asset(kind_name) else {
            warn(format!("no asset kind {kind_name}; value dropped"));
            return Value::Null;
        };
        if kind.shared
            && let Some(id) = self.asset_ids.borrow().get(&(kind_name.to_owned(), handle))
        {
            return json!({ "$asset": id });
        }
        let Some(def) = (kind.describe)(handle) else {
            warn(format!(
                "a {kind_name} that was not loaded from a file cannot be saved; value dropped"
            ));
            return Value::Null;
        };
        if kind.shared {
            let id = format!(
                "{}{}",
                kind_name.to_lowercase(),
                self.asset_ids.borrow().len() + 1
            );
            self.asset_ids
                .borrow_mut()
                .insert((kind_name.to_owned(), handle), id.clone());
            let def = self.value(&def);
            self.assets.borrow_mut().insert(id.clone(), def);
            json!({ "$asset": id })
        } else {
            let mut def = self.value(&def);
            if let Some(m) = def.as_object_mut() {
                m.insert("$type".into(), json!(kind_name));
            }
            def
        }
    }

    fn id(&mut self, key: Key) -> u64 {
        let next = self.ids.len() as u64 + 1;
        *self.ids.entry(key).or_insert(next)
    }

    /// Gives every object and component under `roots` a file id, so references work both ways.
    pub fn number(&mut self, roots: &[ObjectId]) {
        for r in roots {
            self.walk(*r);
        }
    }

    fn walk(&mut self, object: ObjectId) {
        if unsaved(object) {
            return;
        }
        self.id(Key::Object(object.to_raw()));
        for (kind, h) in components_of(object.to_raw()) {
            self.id(Key::Component(kind.name.clone(), h));
        }
        for o in opaque_components(object) {
            self.id(Key::Component(o.ty, o.handle));
        }
        for c in children(object) {
            self.walk(c);
        }
    }

    /// A property value with its object and component placeholders turned into file ids.
    fn value(&self, v: &Value) -> Value {
        match v {
            Value::Array(a) => Value::Array(a.iter().map(|x| self.value(x)).collect()),
            Value::Object(o) if o.contains_key("$handle") && !self.live_assets => {
                let kind = o.get("$handle").and_then(Value::as_str).unwrap_or_default();
                let id = o.get("id").and_then(Value::as_u64).unwrap_or(0);
                self.asset(kind, id)
            }
            Value::Object(o) => match o.get("$ref").and_then(Value::as_str) {
                Some("GameObject") => {
                    let raw = o.get("object").and_then(Value::as_u64).unwrap_or(0);
                    self.reference("GameObject", Key::Object(raw), json!({ "object": raw }))
                }
                Some("Component") => {
                    let kind = o.get("kind").and_then(Value::as_str).unwrap_or_default();
                    let h = o.get("handle").and_then(Value::as_u64).unwrap_or(0);
                    self.reference(
                        "Component",
                        Key::Component(kind.to_owned(), h),
                        json!({ "kind": kind, "handle": h }),
                    )
                }
                _ => Value::Object(o.iter().map(|(k, x)| (k.clone(), self.value(x))).collect()),
            },
            other => other.clone(),
        }
    }

    fn reference(&self, what: &str, key: Key, live: Value) -> Value {
        if let Some(id) = self.ids.get(&key) {
            return json!({ "$ref": what, "id": id });
        }
        if self.keep_external {
            let mut m = live.as_object().cloned().unwrap_or_default();
            m.insert("$ref".into(), json!(what));
            m.insert("live".into(), json!(true));
            return Value::Object(m);
        }
        Value::Null
    }

    /// One object with its components and children; `None` for an object that is not saved.
    pub fn save_object(&mut self, object: ObjectId) -> Option<Value> {
        if unsaved(object) {
            return None;
        }
        let (name, t, enabled, layer, tag) = sc(|s| {
            (
                s.name(object).unwrap_or_default().to_owned(),
                s.local(object).unwrap_or(Transform::IDENTITY),
                s.enabled(object),
                s.layer(object),
                s.tag_name(object).to_owned(),
            )
        })?;
        let f = f64::from;
        let mut d = Map::new();
        d.insert("id".into(), json!(self.id(Key::Object(object.to_raw()))));
        d.insert("name".into(), json!(name));
        // Defaults are left out: smaller files, faster loads.
        let p = t.position;
        if p != Vec3::ZERO {
            d.insert("position".into(), json!([f(p.x), f(p.y), f(p.z)]));
        }
        let r = t.rotation;
        if r != Quat::IDENTITY {
            d.insert("rotation".into(), json!([f(r.x), f(r.y), f(r.z), f(r.w)]));
        }
        let sz = t.scale;
        if sz != Vec3::ONE {
            d.insert("scale".into(), json!([f(sz.x), f(sz.y), f(sz.z)]));
        }
        if !enabled {
            d.insert("enabled".into(), json!(false));
        }
        if layer != 0 {
            d.insert("layer".into(), json!(layer));
        }
        if tag != "Untagged" {
            d.insert("tag".into(), json!(tag));
        }
        for extra in platypus_kinds::object_extras() {
            if let Some(v) = (extra.save)(object.to_raw()) {
                d.insert(extra.key.clone(), v);
            }
        }
        if let Some(path) = prefab_of(object) {
            d.insert("prefab".into(), json!(path));
        }
        let mut components = Vec::new();
        for (kind, h) in components_of(object.to_raw()) {
            let props = (kind.save)(h);
            let mut properties = Map::new();
            for (k, v) in &props {
                properties.insert(k.clone(), self.value(v));
            }
            components.push(json!({
                "id": self.id(Key::Component(kind.name.clone(), h)),
                "type": kind.name,
                "properties": properties,
            }));
        }
        // Opaque ones go back where they were in the file.
        for o in opaque_components(object) {
            let mut c = Map::new();
            c.insert(
                "id".into(),
                json!(self.id(Key::Component(o.ty.clone(), o.handle))),
            );
            c.insert("type".into(), json!(o.ty));
            for (k, v) in &o.data {
                c.insert(k.clone(), self.value(v));
            }
            components.insert(o.index.min(components.len()), Value::Object(c));
        }
        if !components.is_empty() {
            d.insert("components".into(), Value::Array(components));
        }
        let kids: Vec<Value> = children(object)
            .into_iter()
            .filter_map(|c| self.save_object(c))
            .collect();
        if !kids.is_empty() {
            d.insert("children".into(), Value::Array(kids));
        }
        Some(Value::Object(d))
    }
}

enum Thing {
    Object(ObjectId),
    Component(String, u64),
}

fn number_of(v: &Value, i: usize) -> f32 {
    v.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32
}

/// Builds objects (under `parent`, or as roots tagged `tag`), sets their components' properties,
/// then wakes the components: Unity's order, so components can refer to each other in any order.
pub(crate) fn load_objects(
    list: &[Value],
    parent: Option<ObjectId>,
    tag: u32,
    assets: Option<&Map<String, Value>>,
) -> Vec<ObjectId> {
    let mut loader = Loader {
        assets: assets.cloned().unwrap_or_default(),
        asset_cache: RefCell::new(HashMap::new()),
        things: HashMap::new(),
        pending: Vec::new(),
        woken: Vec::new(),
        disabled: Vec::new(),
        opaque: Vec::new(),
    };
    prune_opaque();
    let previous = sc(|s| {
        let p = s.new_tag();
        s.set_new_tag(tag);
        p
    })
    .unwrap_or(0);
    let made: Vec<ObjectId> = list.iter().map(|d| loader.build(d, parent)).collect();
    sc(|s| s.set_new_tag(previous));
    for (object, mut opaque) in std::mem::take(&mut loader.opaque) {
        opaque.data = opaque
            .data
            .iter()
            .map(|(k, v)| (k.clone(), loader.resolve(v)))
            .collect();
        add_opaque(object, opaque);
    }
    for (kind, handle, props) in std::mem::take(&mut loader.pending) {
        let props: Map<String, Value> = props
            .iter()
            .map(|(k, v)| (k.clone(), loader.resolve(v)))
            .collect();
        for w in (kind.load)(handle, &props) {
            warn(format!("{}: {w}", kind.name));
        }
        loader.woken.push((kind, handle));
    }
    // Wake kind by kind, each in the order the components were made.
    let mut done: Vec<String> = Vec::new();
    for (kind, _) in &loader.woken {
        if done.contains(&kind.name) {
            continue;
        }
        done.push(kind.name.clone());
        let handles: Vec<u64> = loader
            .woken
            .iter()
            .filter(|(k, _)| k.name == kind.name)
            .map(|(_, h)| *h)
            .collect();
        (kind.awake)(&handles);
    }
    for o in loader.disabled {
        sc(|s| s.set_enabled(o, false));
    }
    made
}

struct Loader {
    /// The file's `assets` table, and the handle each entry became (one per entry).
    assets: Map<String, Value>,
    asset_cache: RefCell<HashMap<String, u64>>,
    things: HashMap<u64, Thing>,
    pending: Vec<(Rc<Kind>, u64, Map<String, Value>)>,
    /// Components whose properties are set, waiting to wake.
    woken: Vec<(Rc<Kind>, u64)>,
    disabled: Vec<ObjectId>,
    /// Components of unregistered kinds, their data not resolved yet.
    opaque: Vec<(ObjectId, Opaque)>,
}

impl Loader {
    fn build(&mut self, d: &Value, under: Option<ObjectId>) -> ObjectId {
        let name = d
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("GameObject");
        let object = sc(|s| s.create(name)).expect("a scene is current");
        if let Some(p) = under {
            sc(|s| s.set_parent(object, Some(p), false));
        }
        let mut local = Transform::IDENTITY;
        if let Some(p) = d.get("position") {
            local.position = Vec3::new(number_of(p, 0), number_of(p, 1), number_of(p, 2));
        }
        if let Some(r) = d.get("rotation") {
            local.rotation = Quat::from_xyzw(
                number_of(r, 0),
                number_of(r, 1),
                number_of(r, 2),
                number_of(r, 3),
            );
        }
        if let Some(sz) = d.get("scale") {
            local.scale = Vec3::new(number_of(sz, 0), number_of(sz, 1), number_of(sz, 2));
        }
        sc(|s| s.set_local(object, local));
        if let Some(layer) = d.get("layer").and_then(Value::as_u64) {
            sc(|s| s.set_layer(object, layer as u8));
        }
        if let Some(tag) = d.get("tag").and_then(Value::as_str) {
            sc(|s| s.set_tag_name(object, tag));
        }
        if d.get("enabled") == Some(&Value::Bool(false)) {
            self.disabled.push(object);
        }
        if let Some(id) = d.get("id").and_then(Value::as_u64) {
            self.things.insert(id, Thing::Object(object));
        }
        for extra in platypus_kinds::object_extras() {
            if let Some(v) = d.get(&extra.key) {
                (extra.load)(object.to_raw(), v);
            }
        }
        if let Some(path) = d.get("prefab").and_then(Value::as_str) {
            set_prefab(object, path);
        }
        for (index, c) in d
            .get("components")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
        {
            let ty = c.get("type").and_then(Value::as_str).unwrap_or_default();
            let Some(kind) = platypus_kinds::get(ty) else {
                // Kept as it is, so saving writes it back (its references resolve below).
                warn(format!(
                    "{name}: unknown component {ty} (it is not registered with this game); kept as data"
                ));
                let handle = NEXT_OPAQUE.with(|n| {
                    let h = n.get();
                    n.set(h + 1);
                    h
                });
                if let Some(id) = c.get("id").and_then(Value::as_u64) {
                    self.things
                        .insert(id, Thing::Component(ty.to_owned(), handle));
                }
                let mut data = c.as_object().cloned().unwrap_or_default();
                data.remove("id");
                data.remove("type");
                self.opaque.push((
                    object,
                    Opaque {
                        ty: ty.to_owned(),
                        handle,
                        index,
                        data,
                    },
                ));
                continue;
            };
            let handle = match (kind.create)(object.to_raw()) {
                Ok(h) => h,
                Err(e) => {
                    warn(format!("{name}: {ty}: {e}"));
                    continue;
                }
            };
            if let Some(id) = c.get("id").and_then(Value::as_u64) {
                self.things
                    .insert(id, Thing::Component(ty.to_owned(), handle));
            }
            let props = c
                .get("properties")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            self.pending.push((kind, handle, props));
        }
        for child in d
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.build(child, Some(object));
        }
        object
    }

    /// An `assets` table entry made into an asset: its placeholder (null when it cannot be made).
    fn shared_asset(&self, id: &str) -> Value {
        let Some(def) = self.assets.get(id) else {
            warn(format!("unknown asset {id}"));
            return Value::Null;
        };
        let type_name = def.get("type").and_then(Value::as_str).unwrap_or_default();
        let Some(kind) = platypus_kinds::get_asset(type_name) else {
            warn(format!("asset {id}: unknown type {type_name:?}"));
            return Value::Null;
        };
        if let Some(h) = self.asset_cache.borrow().get(id) {
            return platypus_kinds::asset_ref(&kind.name, *h);
        }
        match (kind.create)(&self.resolve(def)) {
            Ok(h) => {
                self.asset_cache.borrow_mut().insert(id.to_owned(), h);
                platypus_kinds::asset_ref(&kind.name, h)
            }
            Err(e) => {
                warn(format!("asset {id}: {e}"));
                Value::Null
            }
        }
    }

    /// File ids in a property value back to placeholders for the kinds.
    fn resolve(&self, v: &Value) -> Value {
        match v {
            Value::Array(a) => Value::Array(a.iter().map(|x| self.resolve(x)).collect()),
            Value::Object(o) if o.get("$asset").is_some_and(Value::is_string) => {
                self.shared_asset(o["$asset"].as_str().unwrap_or_default())
            }
            Value::Object(o)
                if o.get("$type")
                    .and_then(Value::as_str)
                    .and_then(platypus_kinds::get_asset)
                    .is_some_and(|k| !k.shared) =>
            {
                let name = o["$type"].as_str().unwrap_or_default();
                let kind = platypus_kinds::get_asset(name).expect("checked");
                let def = Value::Object(
                    o.iter()
                        .filter(|(k, _)| *k != "$type")
                        .map(|(k, x)| (k.clone(), self.resolve(x)))
                        .collect(),
                );
                match (kind.create)(&def) {
                    Ok(h) => platypus_kinds::asset_ref(&kind.name, h),
                    Err(e) => {
                        warn(format!("{name}: {e}"));
                        Value::Null
                    }
                }
            }
            Value::Object(o) => {
                let Some(what) = o.get("$ref").and_then(Value::as_str) else {
                    return Value::Object(
                        o.iter()
                            .map(|(k, x)| (k.clone(), self.resolve(x)))
                            .collect(),
                    );
                };
                if o.get("live").is_some() {
                    // Outside the copy (`instantiate`): the placeholder is already right.
                    let mut m = o.clone();
                    m.remove("live");
                    return Value::Object(m);
                }
                let id = o.get("id").and_then(Value::as_u64).unwrap_or(0);
                match (what, self.things.get(&id)) {
                    ("GameObject", Some(Thing::Object(o))) => {
                        platypus_kinds::object_ref(o.to_raw())
                    }
                    ("Component", Some(Thing::Component(k, h))) => {
                        platypus_kinds::component_ref(k, *h)
                    }
                    _ => Value::Null,
                }
            }
            other => other.clone(),
        }
    }
}
