// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Scenes (Unity's `SceneManager`): a scene is a set of root objects loaded, saved and unloaded
//! together. The state is per thread. Objects remember the scene they were made in (a tag on the
//! `platypus_scene::Scene` object); an object with no tag belongs to the active scene.

use crate::data::{Saver, load_objects, sc, warn};
use platypus_scene::ObjectId;
use serde_json::{Map, Value, json};
use std::cell::RefCell;
use ts_api::Handle;
use ts_api::callback::{HandleArg, Listeners, emit};

/// The tag of objects kept across scene loads (`dontDestroyOnLoad`).
const KEEP: u32 = u32::MAX;

/// A scene by number (0 is "no scene").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneRef(pub u32);

impl Handle for SceneRef {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        SceneRef(raw as u32)
    }
}

/// A prefab by number (0 is "no prefab").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrefabRef(pub u32);

impl Handle for PrefabRef {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        PrefabRef(raw as u32)
    }
}

#[derive(Clone, Debug)]
pub struct SceneInfo {
    pub handle: u32,
    pub name: String,
    /// Where it was loaded from (none: made in code or from text).
    pub path: Option<String>,
    /// The lighting it was saved with, applied when it is made active.
    pub render_settings: Option<Value>,
}

/// Something the game may want to react to; read with [`take_events`].
#[derive(Clone, Debug, PartialEq)]
pub enum SceneEvent {
    Loaded {
        scene: u32,
        additive: bool,
    },
    Unloaded {
        scene: u32,
    },
    ActiveChanged {
        previous: u32,
        next: u32,
    },
    /// A scene or prefab file could not be read or parsed (`loadSceneAsync` / `Prefab.load` reject).
    LoadFailed {
        path: String,
        error: String,
    },
    /// A prefab file asked for with [`request_prefab`] was read.
    PrefabLoaded {
        path: String,
        prefab: u32,
    },
}

struct Prefab {
    name: String,
    root: Value,
    src: Option<String>,
    /// The file's `assets` table (shared textures, meshes) its `$asset` ids name.
    assets: Option<Map<String, Value>>,
}

struct State {
    scenes: Vec<SceneInfo>,
    active: u32,
    next: u32,
    events: Vec<SceneEvent>,
    /// Events not yet given to listeners (`take_events` is a separate reader).
    pending: Vec<SceneEvent>,
    on_loaded: Listeners<(HandleArg<SceneRef>, String)>,
    on_unloaded: Listeners<(HandleArg<SceneRef>,)>,
    on_active: Listeners<(HandleArg<SceneRef>, HandleArg<SceneRef>)>,
    on_load_failed: Listeners<(String, String)>,
    on_prefab_loaded: Listeners<(String, HandleArg<PrefabRef>)>,
    prefab_requests: Vec<(u64, String)>,
    requests: Vec<(u64, String, bool)>,
    next_request: u64,
    prefabs: Vec<Prefab>,
}

impl State {
    fn push(&mut self, e: SceneEvent) {
        self.events.push(e.clone());
        self.pending.push(e);
    }

    fn new() -> Self {
        State {
            scenes: vec![SceneInfo {
                handle: 1,
                name: "Main".into(),
                path: None,
                render_settings: None,
            }],
            active: 1,
            next: 2,
            events: Vec::new(),
            pending: Vec::new(),
            on_loaded: Listeners::default(),
            on_unloaded: Listeners::default(),
            on_active: Listeners::default(),
            on_load_failed: Listeners::default(),
            on_prefab_loaded: Listeners::default(),
            prefab_requests: Vec::new(),
            requests: Vec::new(),
            next_request: 1,
            prefabs: Vec::new(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::new());
}

fn st<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// Forgets every scene and prefab (a new game, tests): one empty `Main` scene is active.
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::new());
}

/// Makes objects created from now on belong to the active scene. Called by every entry point.
pub fn sync() {
    crate::ensure_kinds();
    let a = active();
    sc(|s| s.set_new_tag(a));
}

pub fn scene_count() -> usize {
    st(|s| s.scenes.len())
}

/// The scene at an index, or 0.
pub fn scene_at(index: usize) -> u32 {
    st(|s| s.scenes.get(index).map_or(0, |i| i.handle))
}

pub fn scene_by_name(name: &str) -> u32 {
    st(|s| {
        s.scenes
            .iter()
            .find(|i| i.name == name)
            .map_or(0, |i| i.handle)
    })
}

pub fn active() -> u32 {
    st(|s| s.active)
}

pub fn info(scene: u32) -> Option<SceneInfo> {
    st(|s| s.scenes.iter().find(|i| i.handle == scene).cloned())
}

pub fn is_loaded(scene: u32) -> bool {
    info(scene).is_some()
}

/// Makes a loaded scene active (false if it is not loaded).
pub fn set_active(scene: u32) -> bool {
    if !is_loaded(scene) {
        return false;
    }
    let previous = active();
    if previous != scene {
        st(|s| {
            s.active = scene;
            s.push(SceneEvent::ActiveChanged {
                previous,
                next: scene,
            });
        });
        sync();
        // The lighting the scene was saved with.
        if let Some(r) = info(scene).and_then(|i| i.render_settings) {
            platypus_render_api::apply_render_settings(&r);
        }
        dispatch();
    }
    true
}

/// Gives the events since the last call to the script listeners (after the world is consistent,
/// so a listener may call back into the manager).
pub fn dispatch() {
    let (events, loaded, unloaded, active, failed, prefabs) = st(|s| {
        (
            std::mem::take(&mut s.pending),
            s.on_loaded.snapshot(),
            s.on_unloaded.snapshot(),
            s.on_active.snapshot(),
            s.on_load_failed.snapshot(),
            s.on_prefab_loaded.snapshot(),
        )
    });
    for e in events {
        match e {
            SceneEvent::Loaded { scene, additive } => {
                let mode = if additive { "additive" } else { "single" };
                emit(&loaded, (HandleArg(SceneRef(scene)), mode.to_string()));
            }
            SceneEvent::Unloaded { scene } => emit(&unloaded, (HandleArg(SceneRef(scene)),)),
            SceneEvent::LoadFailed { path, error } => emit(&failed, (path, error)),
            SceneEvent::PrefabLoaded { path, prefab } => {
                emit(&prefabs, (path, HandleArg(PrefabRef(prefab))))
            }
            SceneEvent::ActiveChanged { previous, next } => emit(
                &active,
                (HandleArg(SceneRef(previous)), HandleArg(SceneRef(next))),
            ),
        }
    }
}

/// Calls `f(scene, mode)` after a scene loads (`"single"` or `"additive"`); the token is for [`off`].
pub fn on_loaded(f: ts_api::Callback<(HandleArg<SceneRef>, String)>) -> u32 {
    st(|s| s.on_loaded.add(f))
}

pub fn on_unloaded(f: ts_api::Callback<(HandleArg<SceneRef>,)>) -> u32 {
    st(|s| s.on_unloaded.add(f))
}

pub fn on_active_changed(f: ts_api::Callback<(HandleArg<SceneRef>, HandleArg<SceneRef>)>) -> u32 {
    st(|s| s.on_active.add(f))
}

/// Calls `f(path, error)` when a scene file asked for with [`request_load`] fails to load.
pub fn on_load_failed(f: ts_api::Callback<(String, String)>) -> u32 {
    st(|s| s.on_load_failed.add(f))
}

/// Calls `f(path, prefab)` when a prefab file asked for with [`request_prefab`] was read.
pub fn on_prefab_loaded(f: ts_api::Callback<(String, HandleArg<PrefabRef>)>) -> u32 {
    st(|s| s.on_prefab_loaded.add(f))
}

/// Asks for a scene file and settles with the scene that file made: `resolve(scene)` in the tick
/// the host loads it, or `reject(message)` when it cannot be read or parsed. The listeners it adds
/// go when either happens.
pub fn load_scene_promise(
    path: &str,
    additive: bool,
    resolve: ts_api::Callback<(HandleArg<SceneRef>,)>,
    reject: ts_api::Callback<(String,)>,
) {
    let tokens = std::rc::Rc::new(std::cell::Cell::new((0u32, 0u32)));
    let (want, on_ok) = (path.to_owned(), tokens.clone());
    let loaded = on_loaded(ts_api::Callback::from_fn(
        move |(scene, _mode): (HandleArg<SceneRef>, String)| {
            if info(scene.0.0).and_then(|i| i.path).as_deref() == Some(want.as_str()) {
                let (a, b) = on_ok.get();
                off(a);
                off(b);
                resolve.call((scene,));
            }
        },
    ));
    let (want, on_err) = (path.to_owned(), tokens.clone());
    let failed = on_load_failed(ts_api::Callback::from_fn(
        move |(failed_path, error): (String, String)| {
            if failed_path == want {
                let (a, b) = on_err.get();
                off(a);
                off(b);
                reject.call((format!("could not load scene {want}: {error}"),));
            }
        },
    ));
    tokens.set((loaded, failed));
    request_load(path, additive);
}

/// [`load_scene_promise`] for a prefab file.
pub fn load_prefab_promise(
    path: &str,
    resolve: ts_api::Callback<(HandleArg<PrefabRef>,)>,
    reject: ts_api::Callback<(String,)>,
) {
    let tokens = std::rc::Rc::new(std::cell::Cell::new((0u32, 0u32)));
    let (want, on_ok) = (path.to_owned(), tokens.clone());
    let loaded = on_prefab_loaded(ts_api::Callback::from_fn(
        move |(loaded_path, prefab): (String, HandleArg<PrefabRef>)| {
            if loaded_path == want {
                let (a, b) = on_ok.get();
                off(a);
                off(b);
                resolve.call((prefab,));
            }
        },
    ));
    let (want, on_err) = (path.to_owned(), tokens.clone());
    let failed = on_load_failed(ts_api::Callback::from_fn(
        move |(failed_path, error): (String, String)| {
            if failed_path == want {
                let (a, b) = on_err.get();
                off(a);
                off(b);
                reject.call((format!("could not load prefab {want}: {error}"),));
            }
        },
    ));
    tokens.set((loaded, failed));
    request_prefab(path);
}

/// Removes a listener by its token; false when it is unknown.
pub fn off(token: u32) -> bool {
    st(|s| {
        s.on_loaded.remove(token)
            | s.on_unloaded.remove(token)
            | s.on_active.remove(token)
            | s.on_load_failed.remove(token)
            | s.on_prefab_loaded.remove(token)
    })
}

/// A new empty scene, loaded alongside the others (0 if the name is taken).
pub fn create_scene(name: &str) -> u32 {
    if scene_by_name(name) != 0 {
        warn(format!("a scene named {name} is already loaded"));
        return 0;
    }
    st(|s| {
        let handle = s.next;
        s.next += 1;
        s.scenes.push(SceneInfo {
            handle,
            name: name.to_owned(),
            path: None,
            render_settings: None,
        });
        handle
    })
}

fn tag_of(object: ObjectId) -> u32 {
    let tag = sc(|s| {
        let mut o = object;
        while let Some(p) = s.parent(o) {
            o = p;
        }
        s.tag(o)
    })
    .unwrap_or(0);
    if tag == 0 { active() } else { tag }
}

/// The scene an object is in (0 for an object that does not exist, `u32::MAX` for kept ones).
pub fn scene_of(object: ObjectId) -> u32 {
    if sc(|s| s.contains(object)) != Some(true) {
        return 0;
    }
    tag_of(object)
}

/// The root objects of a scene, in order.
pub fn roots(scene: u32) -> Vec<ObjectId> {
    let a = active();
    sc(|s| {
        s.roots()
            .iter()
            .copied()
            .filter(|r| match s.tag(*r) {
                0 => scene == a,
                t => t == scene,
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Everything in a scene as `SceneData` JSON text.
pub fn serialize(scene: u32) -> Option<String> {
    scene_value(scene).map(|v| v.to_string())
}

/// Gives a loaded scene a new name and file (the editor's Save As).
pub fn rename_scene(scene: u32, name: &str, path: Option<&str>) {
    st(|s| {
        if let Some(info) = s.scenes.iter_mut().find(|i| i.handle == scene) {
            info.name = name.to_owned();
            info.path = path.map(str::to_owned);
        }
    });
}

/// Loads a scene file's text in place of the loaded scenes, remembering its path.
pub fn load_scene_file(text: &str, path: &str) -> Result<u32, String> {
    load_scene_from(text, false, Some(path.to_owned()))
}

/// Everything in a scene as `SceneData`.
pub fn scene_value(scene: u32) -> Option<Value> {
    crate::ensure_kinds();
    let info = info(scene)?;
    let mut saver = Saver::new(false);
    let roots = roots(scene);
    saver.number(&roots);
    let objects: Vec<Value> = roots
        .into_iter()
        .filter_map(|r| saver.save_object(r))
        .collect();
    let mut data = json!({
        "format": "platypus.scene",
        "version": 1,
        "name": info.name,
        "objects": objects,
    });
    if scene == active()
        && (info.render_settings.is_some() || platypus_render_api::settings_changed())
    {
        data["renderSettings"] = platypus_render_api::save_render_settings();
    }
    let assets = saver.take_assets();
    if !assets.is_empty() {
        data["assets"] = Value::Object(assets);
    }
    Some(data)
}

fn unload(scene: u32) {
    for o in roots(scene) {
        platypus_scene::api::scene_api::destroy(o);
    }
    st(|s| {
        s.scenes.retain(|i| i.handle != scene);
        s.push(SceneEvent::Unloaded { scene });
    });
}

/// Loads scene data (text). `additive` adds it to the loaded scenes; otherwise every other scene
/// is unloaded first (not the objects kept with [`dont_destroy_on_load`]) and it becomes active.
pub fn load_scene(text: &str, additive: bool) -> Result<u32, String> {
    load_scene_from(text, additive, None)
}

/// As [`load_scene`], remembering the file the text came from before the listeners run (they
/// match a `loadSceneAsync` by `path`).
fn load_scene_from(text: &str, additive: bool, path: Option<String>) -> Result<u32, String> {
    sync();
    let data: Value = serde_json::from_str(text).map_err(|e| format!("not a scene: {e}"))?;
    // `iron.scene`: files saved before the rename to Platypus.
    if !matches!(
        data.get("format").and_then(Value::as_str),
        Some("platypus.scene" | "iron.scene")
    ) {
        return Err("not a scene (format isn't platypus.scene)".into());
    }
    if data.get("version").and_then(Value::as_u64) != Some(1) {
        return Err(format!("scene version {} isn't supported", data["version"]));
    }
    let name = data.get("name").and_then(Value::as_str).unwrap_or("Scene");
    let (handle, old) = st(|s| {
        let handle = s.next;
        s.next += 1;
        let old: Vec<u32> = if additive {
            Vec::new()
        } else {
            s.scenes.iter().map(|i| i.handle).collect()
        };
        s.scenes.push(SceneInfo {
            handle,
            name: name.to_owned(),
            path,
            render_settings: data.get("renderSettings").cloned(),
        });
        (handle, old)
    });
    for o in old {
        unload(o);
    }
    let objects = data
        .get("objects")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let assets = data.get("assets").and_then(Value::as_object);
    load_objects(&objects, None, handle, assets);
    if !additive {
        set_active(handle);
    }
    st(|s| {
        s.push(SceneEvent::Loaded {
            scene: handle,
            additive,
        })
    });
    dispatch();
    Ok(handle)
}

/// Destroys a scene's objects and drops it. The last scene cannot be unloaded.
pub fn unload_scene(scene: u32) -> bool {
    if !is_loaded(scene) || scene_count() == 1 {
        return false;
    }
    unload(scene);
    if active() == scene {
        set_active(scene_at(0));
    }
    dispatch();
    true
}

/// Moves a root object (with its children) to another loaded scene.
pub fn move_to_scene(object: ObjectId, scene: u32) -> bool {
    if !is_loaded(scene) || sc(|s| s.parent(object).is_some()) != Some(false) {
        return false;
    }
    sc(|s| s.set_tag(object, scene));
    true
}

/// Moves every object of `source` into `destination` and unloads `source`.
pub fn merge_scenes(source: u32, destination: u32) {
    for o in roots(source) {
        move_to_scene(o, destination);
    }
    unload_scene(source);
}

/// Keeps the root of an object when scenes unload.
pub fn dont_destroy_on_load(object: ObjectId) {
    sc(|s| {
        let mut o = object;
        while let Some(p) = s.parent(o) {
            o = p;
        }
        s.set_tag(o, KEEP);
    });
}

/// What happened since the last call.
pub fn take_events() -> Vec<SceneEvent> {
    st(|s| std::mem::take(&mut s.events))
}

/// Asks the host to read a scene file (it is the app that has the game folder). The host calls
/// [`finish_load`] with the text. Returns the request id.
pub fn request_load(path: &str, additive: bool) -> u64 {
    st(|s| {
        let id = s.next_request;
        s.next_request += 1;
        s.requests.push((id, path.to_owned(), additive));
        id
    })
}

/// The files the script asked for, as (request id, path).
pub fn take_load_requests() -> Vec<(u64, String)> {
    st(|s| s.requests.iter().map(|(i, p, _)| (*i, p.clone())).collect())
}

/// Asks the host to read a prefab file (`Prefab.load`); the host calls [`finish_prefab`].
pub fn request_prefab(path: &str) -> u64 {
    st(|s| {
        let id = s.next_request;
        s.next_request += 1;
        s.prefab_requests.push((id, path.to_owned()));
        id
    })
}

/// The prefab files the script asked for, as (request id, path).
pub fn take_prefab_requests() -> Vec<(u64, String)> {
    st(|s| s.prefab_requests.clone())
}

/// The host read the file of a prefab request (`text`), or could not (`Err`): the listeners hear
/// the prefab or the failure.
pub fn finish_prefab(id: u64, text: Result<&str, &str>) -> Result<u32, String> {
    let path = st(|s| {
        let i = s.prefab_requests.iter().position(|r| r.0 == id)?;
        Some(s.prefab_requests.remove(i).1)
    })
    .ok_or_else(|| format!("no prefab request {id}"))?;
    match text
        .map_err(str::to_owned)
        .and_then(|t| prefab_from_data(t, Some(&path)))
    {
        Ok(prefab) => {
            st(|s| s.push(SceneEvent::PrefabLoaded { path, prefab }));
            dispatch();
            Ok(prefab)
        }
        Err(e) => {
            fail(&path, &e);
            Err(e)
        }
    }
}

/// The host could not read the file of a request: the listeners of [`on_load_failed`] hear it.
pub fn fail_load(id: u64, error: &str) {
    let path = st(|s| {
        let i = s.requests.iter().position(|r| r.0 == id)?;
        Some(s.requests.remove(i).1)
    });
    if let Some(path) = path {
        fail(&path, error);
    }
}

fn fail(path: &str, error: &str) {
    st(|s| {
        s.push(SceneEvent::LoadFailed {
            path: path.to_owned(),
            error: error.to_owned(),
        })
    });
    dispatch();
}

/// The host read the file of a request: the scene loads (the new scene's handle).
pub fn finish_load(id: u64, text: &str) -> Result<u32, String> {
    let (path, additive) = st(|s| {
        let i = s.requests.iter().position(|r| r.0 == id)?;
        let r = s.requests.remove(i);
        Some((r.1, r.2))
    })
    .ok_or_else(|| format!("no load request {id}"))?;
    let handle = match load_scene_from(text, additive, Some(path.clone())) {
        Ok(h) => h,
        Err(e) => {
            fail(&path, &e);
            return Err(e);
        }
    };
    Ok(handle)
}

fn place(data: &mut Value, position: Option<[f64; 3]>, rotation: Option<[f64; 4]>) {
    if let Some(p) = position {
        data["position"] = json!(p);
    }
    if let Some(r) = rotation {
        data["rotation"] = json!(r);
    }
}

/// Copies an object with its children and components (Unity's `Instantiate`): references inside
/// the copy point at the copies, others at the originals. `position` and `rotation` (world, when
/// there is no parent) default to the original's.
pub fn instantiate(
    original: ObjectId,
    position: Option<[f64; 3]>,
    rotation: Option<[f64; 4]>,
    parent: Option<ObjectId>,
) -> Option<ObjectId> {
    sync();
    let mut saver = Saver::new(true);
    saver.number(&[original]);
    let Some(mut data) = saver.save_object(original) else {
        warn("parts of a model cannot be copied on their own; copy the model's root");
        return None;
    };
    if parent.is_none() {
        // Where the original is in the world.
        let w = sc(|s| s.world(original))?;
        let f = f64::from;
        data["position"] = json!([f(w.position.x), f(w.position.y), f(w.position.z)]);
        data["rotation"] = json!([
            f(w.rotation.x),
            f(w.rotation.y),
            f(w.rotation.z),
            f(w.rotation.w)
        ]);
        data["scale"] = json!([f(w.scale.x), f(w.scale.y), f(w.scale.z)]);
    }
    place(&mut data, position, rotation);
    // A root copy goes into the original's scene.
    let tag = if parent.is_some() {
        active()
    } else {
        tag_of(original)
    };
    load_objects(&[data], parent, tag, None).into_iter().next()
}

/// Captures an object and everything under it as it is now. References to objects outside it are
/// dropped. Returns the prefab's number (1-based), 0 if the object cannot be a prefab.
pub fn prefab_from_object(object: ObjectId) -> u32 {
    // Assets stay live handles in a prefab made in memory (it is not a file).
    let mut saver = Saver::new(false);
    saver.keep_assets_live();
    saver.number(&[object]);
    let Some(mut root) = saver.save_object(object) else {
        warn("parts of a model cannot be prefabs on their own; use the model's root");
        return 0;
    };
    // The prefab's own placement starts from the origin.
    if let Some(o) = root.as_object_mut() {
        o.remove("position");
        o.remove("rotation");
    }
    let name = sc(|s| s.name(object).unwrap_or_default().to_owned()).unwrap_or_default();
    st(|s| {
        s.prefabs.push(Prefab {
            name,
            root,
            src: None,
            assets: None,
        });
        s.prefabs.len() as u32
    })
}

/// An object and everything under it as a prefab file (`.prefab.json`): assets go to the file's
/// `assets` table like a scene's, references outside it are dropped, and its placement starts
/// at the origin. `None` for a part of a model.
pub fn prefab_file_json(object: ObjectId) -> Option<Value> {
    crate::ensure_kinds();
    sync();
    let mut saver = Saver::new(false);
    saver.number(&[object]);
    let mut root = saver.save_object(object)?;
    if let Some(o) = root.as_object_mut() {
        o.remove("position");
        o.remove("rotation");
        o.remove("prefab");
    }
    let name = sc(|s| s.name(object).unwrap_or_default().to_owned()).unwrap_or_default();
    let mut data = json!({ "format": "platypus.prefab", "version": 1, "name": name, "root": root });
    let assets = saver.take_assets();
    if !assets.is_empty() {
        data["assets"] = Value::Object(assets);
    }
    Some(data)
}

/// A prefab from saved data (`prefab_json`'s output).
pub fn prefab_from_data(text: &str, src: Option<&str>) -> Result<u32, String> {
    let d: Value = serde_json::from_str(text).map_err(|e| format!("not prefab data: {e}"))?;
    let format = d.get("format").and_then(Value::as_str);
    if !matches!(format, Some("platypus.prefab" | "iron.prefab")) || d.get("root").is_none() {
        return Err("not prefab data".into());
    }
    let name = d
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Prefab")
        .to_owned();
    Ok(st(|s| {
        s.prefabs.push(Prefab {
            name,
            root: d["root"].clone(),
            src: src.map(str::to_owned),
            assets: d.get("assets").and_then(Value::as_object).cloned(),
        });
        s.prefabs.len() as u32
    }))
}

pub fn prefab_name(prefab: u32) -> String {
    st(|s| {
        s.prefabs
            .get((prefab as usize).wrapping_sub(1))
            .map(|p| p.name.clone())
            .unwrap_or_default()
    })
}

/// A prefab as JSON text, for a `.prefab.json` file.
pub fn prefab_json(prefab: u32) -> String {
    st(|s| {
        s.prefabs
            .get((prefab as usize).wrapping_sub(1))
            .map(|p| {
                let mut d = json!({ "format": "platypus.prefab", "version": 1, "name": p.name, "root": p.root });
                if let Some(a) = &p.assets {
                    d["assets"] = Value::Object(a.clone());
                }
                d.to_string()
            })
            .unwrap_or_default()
    })
}

/// Places a copy: in the active scene (or under `parent`), at `position` / `rotation`.
pub fn instantiate_prefab(
    prefab: u32,
    position: Option<[f64; 3]>,
    rotation: Option<[f64; 4]>,
    parent: Option<ObjectId>,
) -> Option<ObjectId> {
    sync();
    let (mut root, assets) = st(|s| {
        s.prefabs.get((prefab as usize).wrapping_sub(1)).map(|p| {
            let mut r = p.root.clone();
            if let (Some(src), Some(o)) = (&p.src, r.as_object_mut()) {
                o.insert("prefab".into(), json!(src));
            }
            (r, p.assets.clone())
        })
    })?;
    place(&mut root, position, rotation);
    load_objects(&[root], parent, active(), assets.as_ref())
        .into_iter()
        .next()
}
