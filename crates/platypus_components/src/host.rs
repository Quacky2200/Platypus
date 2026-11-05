// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The engine side of compiled components: a class table (from the generated bindings'
//! `classes()`), one rooted instance per component and its lifecycle, with no script engine in the
//! loop.
//!
//! Order and rules (Unity's): `onAwake` when spawned, `onStart` once
//! before a component's first other hook, `onUpdate` for all and then `onLateUpdate` for all,
//! disabled or inactive-object components skipped, a component that throws is reported and
//! disabled. After every hook the microtask queue runs (the event-loop contract of `event.rs`).

use platypus_ts_rt::*;

pub const HOOK_AWAKE: usize = 0;
pub const HOOK_START: usize = 1;
pub const HOOK_UPDATE: usize = 2;
pub const HOOK_LATE_UPDATE: usize = 3;
pub const HOOK_FIXED_UPDATE: usize = 4;
pub const HOOK_PRE_RENDER: usize = 5;
pub const HOOK_DESTROY: usize = 6;
pub const HOOK_APP_FOCUS: usize = 7;
pub const HOOK_APP_PAUSE: usize = 8;
const HOOK_NAMES: [&str; 9] = [
    "onAwake",
    "onStart",
    "onUpdate",
    "onLateUpdate",
    "onFixedUpdate",
    "onPreRender",
    "onDestroy",
    "onApplicationFocus",
    "onApplicationPause",
];

type Create = unsafe extern "C" fn(f64, u64) -> *mut u8;
type Hook = unsafe extern "C" fn(*mut u8, f64);
/// A contact hook: (this, the `Collision` handle, the other side's `Collider` handle or 0). The
/// compiled thunk passes the script whichever of the two its hook asks for.
type ContactHook = unsafe extern "C" fn(*mut u8, u64, u64);

pub const CONTACT_ENTER: usize = 0;
pub const CONTACT_EXIT: usize = 1;
pub const CONTACT_STAY: usize = 2;
pub const TRIGGER_ENTER: usize = 3;
pub const TRIGGER_EXIT: usize = 4;
pub const TRIGGER_STAY: usize = 5;
const CONTACT_NAMES: [&str; 6] = [
    "onCollisionEnter",
    "onCollisionExit",
    "onCollisionStay",
    "onTriggerEnter",
    "onTriggerExit",
    "onTriggerStay",
];

/// One registered class: its constructor thunk and the hooks it has (a missing hook costs nothing).
#[derive(Clone)]
pub struct NativeClass {
    pub name: &'static str,
    /// The class struct id in the compiled program: what `getComponent(Class)` passes.
    pub sid: u64,
    /// The class and its ancestors (`getComponent(Base)` finds subclasses).
    pub bases: &'static [u64],
    pub create: Create,
    pub hooks: [Option<Hook>; 9],
    /// `onCollisionEnter/Exit/Stay` and `onTriggerEnter/Exit/Stay` (see the `CONTACT_*` indices).
    pub contact_hooks: [Option<ContactHook>; 6],
    /// The `@property` table: name, byte offset in the object, type name (`PROPS_<Class>`).
    pub props: &'static [(&'static str, usize, &'static str)],
}

/// The host's hook list, the order of [`HostClass::hooks`]: the lifecycle hooks, then the contact
/// hooks. The compiler is told the same list (`hostClasses.hooks` in the manifest).
pub const HOOK_LIST: [&str; 15] = [
    HOOK_NAMES[0],
    HOOK_NAMES[1],
    HOOK_NAMES[2],
    HOOK_NAMES[3],
    HOOK_NAMES[4],
    HOOK_NAMES[5],
    HOOK_NAMES[6],
    HOOK_NAMES[7],
    HOOK_NAMES[8],
    CONTACT_NAMES[0],
    CONTACT_NAMES[1],
    CONTACT_NAMES[2],
    CONTACT_NAMES[3],
    CONTACT_NAMES[4],
    CONTACT_NAMES[5],
];

impl NativeClass {
    /// Types the hook addresses of a host class by their place in [`HOOK_LIST`], once at load.
    pub fn from_host(c: &HostClass) -> Self {
        let at = |i: usize| c.hooks.get(i).copied().flatten();
        let mut hooks: [Option<Hook>; 9] = [None; 9];
        for (i, h) in hooks.iter_mut().enumerate() {
            // SAFETY: a lifecycle hook thunk takes (this, dt).
            *h = at(i).map(|a| unsafe { std::mem::transmute::<usize, Hook>(a) });
        }
        let mut contact_hooks: [Option<ContactHook>; 6] = [None; 6];
        for (i, h) in contact_hooks.iter_mut().enumerate() {
            // SAFETY: a contact hook thunk takes (this, collision, collider).
            *h = at(9 + i).map(|a| unsafe { std::mem::transmute::<usize, ContactHook>(a) });
        }
        NativeClass {
            name: c.name,
            sid: c.sid,
            bases: c.bases,
            create: c.create,
            hooks,
            contact_hooks,
            props: c.props,
        }
    }
}

/// A `@property` value as the engine sees it (scene files, the editor).
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    /// `null` or `undefined`.
    Null,
    Num(f64),
    Bool(bool),
    Str(String),
    /// Another component, by id.
    Component(u32),
    /// A GameObject, by raw handle.
    Object(u64),
    /// A value class (`Vector3`, `Color`, ...) by name and field values.
    Value(String, Vec<f64>),
}

/// The value classes a `@property` may hold: name and field count. The compiled layout is a header
/// word and one f64 per field.
const VALUE_CLASSES: [(&str, usize); 9] = [
    ("Vector2", 2),
    ("Vector3", 3),
    ("Vector4", 4),
    ("Quaternion", 4),
    ("Color", 4),
    ("Rect", 4),
    ("RectInt", 4),
    ("Vector2Int", 2),
    ("Vector3Int", 3),
];

fn value_class(kind: &str) -> Option<(&'static str, usize)> {
    let name = kind.strip_prefix("object:")?;
    VALUE_CLASSES.iter().copied().find(|(n, _)| *n == name)
}

/// A component in the thread registry: what `getComponent` searches.
struct Live {
    id: u32,
    object: u64,
    sid: u64,
    bases: &'static [u64],
    /// Index in `Registry::classes`.
    class: usize,
    ptr: *mut u8,
}

impl Live {
    /// Is this a component of class `sid`, or of a subclass?
    fn is(&self, sid: u64) -> bool {
        self.sid == sid || self.bases.contains(&sid)
    }
}

/// A component `addComponent` made from script, waiting for the host to run its `onAwake`.
struct Pending {
    id: u32,
    class: usize,
    object: u64,
    root: usize,
    ptr: *mut u8,
}

#[derive(Default)]
struct Registry {
    /// (sid, class index, constructor, bases) of the classes of the current host.
    creators: Vec<(u64, usize, Create, &'static [u64])>,
    /// The class table of the current host: scene loading reads and writes components by name.
    classes: Vec<NativeClass>,
    live: Vec<Live>,
    pending: Vec<Pending>,
    /// Components script asked to remove (`removeComponent`, `destroy()`): their `onDestroy` runs
    /// when the calling hook returns.
    removals: Vec<u32>,
    next_id: u32,
}

thread_local! {
    static REG: std::cell::RefCell<Registry> = std::cell::RefCell::new(Registry { next_id: 1, ..Registry::default() });
}

/// `go.getComponent(Class)`: the first live component of that class on the object, or null.
#[unsafe(no_mangle)]
pub extern "C" fn ts_rt_host_get(object: u64, sid: f64) -> *mut u8 {
    REG.with(|r| {
        r.borrow()
            .live
            .iter()
            .find(|l| l.object == object && l.is(sid as u64))
            .map_or(std::ptr::null_mut(), |l| l.ptr)
    })
}

/// How many components of class `sid` (or a subclass) are on the object.
#[unsafe(no_mangle)]
pub extern "C" fn ts_rt_host_count(object: u64, sid: f64) -> f64 {
    REG.with(|r| {
        r.borrow()
            .live
            .iter()
            .filter(|l| l.object == object && l.is(sid as u64))
            .count() as f64
    })
}

/// A new array of the object components of class `sid` (or a subclass), in the order they were
/// added. May collect first.
#[unsafe(no_mangle)]
pub extern "C" fn ts_rt_host_all_gc(object: u64, sid: f64) -> *mut Array {
    let ptrs: Vec<u64> = REG.with(|r| {
        r.borrow()
            .live
            .iter()
            .filter(|l| l.object == object && l.is(sid as u64))
            .map(|l| l.ptr as u64)
            .collect()
    });
    component_array(&ptrs)
}

/// The first component of class `sid` (or a subclass) on any object, or null.
#[unsafe(no_mangle)]
pub extern "C" fn ts_rt_host_find_any(sid: f64) -> *mut u8 {
    REG.with(|r| {
        r.borrow()
            .live
            .iter()
            .find(|l| l.is(sid as u64))
            .map_or(std::ptr::null_mut(), |l| l.ptr)
    })
}

/// `removeComponent` / `destroy()` on a component: it is gone for `getComponent` at once and its
/// `onDestroy` runs when the calling hook returns.
#[unsafe(no_mangle)]
pub extern "C" fn ts_rt_host_remove(ptr: *mut u8) {
    platypus_kinds::components_changed();
    REG.with(|r| {
        let mut r = r.borrow_mut();
        if let Some(i) = r.live.iter().position(|l| l.ptr == ptr) {
            let id = r.live[i].id;
            r.live.remove(i);
            // Added and removed in the same hook: it never wakes.
            if let Some(j) = r.pending.iter().position(|p| p.id == id) {
                let p = r.pending.remove(j);
                ts_rt_root_remove(p.root);
            } else {
                r.removals.push(id);
            }
        }
    });
}

/// Every live script component's object (a raw handle, 0: none) and class name, in the order they
/// were made.
pub fn live_components() -> Vec<(u64, &'static str)> {
    REG.with(|r| {
        let r = r.borrow();
        r.live
            .iter()
            .map(|l| (l.object, r.classes[l.class].name))
            .collect()
    })
}

/// An array holding these pointers (an array of objects).
fn component_array(ptrs: &[u64]) -> *mut Array {
    let arr = ts_rt_arr_new_gc(ptrs.len() as i64, 1);
    // The array is not reachable from anything yet; the pushes below do not allocate.
    let root = ts_rt_root_add(arr as *mut u8);
    for p in ptrs {
        // SAFETY: `arr` is a live array with room for every pointer.
        unsafe { ts_rt_arr_push(arr, *p) };
    }
    ts_rt_root_remove(root);
    arr
}

/// `go.addComponent(Class)`: makes the component now (fields default) and queues its `onAwake` for
/// the host, which runs it when the calling hook returns. May collect (it runs the constructor).
#[unsafe(no_mangle)]
pub extern "C" fn ts_rt_host_add_gc(object: u64, sid: f64) -> *mut u8 {
    let sid = sid as u64;
    let found = REG.with(|r| {
        let mut r = r.borrow_mut();
        let (_, class, create, bases) = *r.creators.iter().find(|c| c.0 == sid)?;
        let id = r.next_id;
        r.next_id += 1;
        Some((id, class, create, bases))
    });
    let Some((id, class, create, bases)) = found else {
        panic!("addComponent: the class is not registered with a NativeHost");
    };
    // SAFETY: the thunk was generated for this class; the registry is not borrowed while it runs.
    let ptr = unsafe { create(id as f64, object) };
    let root = ts_rt_root_add(ptr);
    platypus_kinds::components_changed();
    REG.with(|r| {
        let mut r = r.borrow_mut();
        r.live.push(Live {
            id,
            object,
            sid,
            bases,
            class,
            ptr,
        });
        r.pending.push(Pending {
            id,
            class,
            object,
            root,
            ptr,
        });
    });
    ptr
}

struct Entry {
    id: u32,
    class: usize,
    /// The engine object (a raw handle) the component is on, if any.
    object: Option<u64>,
    root: usize,
    this: *mut u8,
    started: bool,
    enabled: bool,
}

/// A component whose hook threw: it is disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub id: u32,
    pub class: &'static str,
    pub hook: &'static str,
}

/// One side of a contact, as the host dispatches it: `object` is the object being told, with its
/// collider `own`, `other` the object on the other side (0: none) with its collider `collider`
/// (rapier collider numbers), and `velocity` the other side's velocity minus this one's going in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactEvent {
    pub started: bool,
    pub object: u64,
    pub own: u64,
    pub other: u64,
    pub collider: u64,
    pub velocity: [f64; 3],
    pub sensor: bool,
}

pub struct NativeHost {
    classes: Vec<NativeClass>,
    entries: Vec<Entry>,
    /// Contacts that began and have not ended: `onCollisionStay` / `onTriggerStay` run for these
    /// on every dispatch.
    touching: Vec<ContactEvent>,
}

impl Drop for NativeHost {
    fn drop(&mut self) {
        for e in self.entries.drain(..) {
            ts_rt_root_remove(e.root);
        }
        REG.with(|r| {
            let mut r = r.borrow_mut();
            for p in r.pending.drain(..) {
                ts_rt_root_remove(p.root);
            }
            r.creators.clear();
            r.classes.clear();
            r.live.clear();
        });
        for c in &self.classes {
            platypus_kinds::unregister(c.name);
        }
    }
}

impl NativeHost {
    /// The host over a program's host classes (`Game::host_classes`, or the AOT `classes()`).
    pub fn new(classes: Vec<HostClass>) -> Self {
        Self::with_classes(classes.iter().map(NativeClass::from_host).collect())
    }

    pub fn with_classes(classes: Vec<NativeClass>) -> Self {
        REG.with(|r| {
            let mut r = r.borrow_mut();
            r.creators = classes
                .iter()
                .enumerate()
                .map(|(i, c)| (c.sid, i, c.create, c.bases))
                .collect();
            r.classes = classes.clone();
            r.live.clear();
            r.pending.clear();
        });
        for c in &classes {
            register_kind(c.name);
        }
        NativeHost {
            classes,
            entries: Vec::new(),
            touching: Vec::new(),
        }
    }

    pub fn class_names(&self) -> Vec<&'static str> {
        self.classes.iter().map(|c| c.name).collect()
    }

    /// The classes a `register(..)` call has named so far, in the order of the calls (the others
    /// only exist for `addComponent`), with their `spawn` option: the ones that get a starting
    /// object. Call it once the program's startup is done.
    pub fn registered_class_names(&self) -> Vec<(&'static str, bool)> {
        registered_classes()
            .into_iter()
            .filter_map(|(n, spawn)| {
                self.classes
                    .iter()
                    .find(|c| c.name == n)
                    .map(|c| (c.name, spawn))
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The ids of the live components, in creation order.
    pub fn ids(&self) -> Vec<u32> {
        self.entries.iter().map(|e| e.id).collect()
    }

    /// The live components in creation order: id, class name and the object each is on. A hot
    /// reload reads this to carry components over to the new program.
    pub fn components(&self) -> Vec<(u32, &'static str, Option<u64>)> {
        self.entries
            .iter()
            .map(|e| (e.id, self.classes[e.class].name, e.object))
            .collect()
    }

    pub fn contains(&self, id: u32) -> bool {
        self.entries.iter().any(|e| e.id == id)
    }

    pub fn is_enabled(&self, id: u32) -> bool {
        self.entries.iter().any(|e| e.id == id && e.enabled)
    }

    /// Creates a component of `class` on `object` (a raw handle, or none) and runs its `onAwake`.
    /// An unknown class is an `Err`; a throwing `onAwake` is a fault and the component is dropped.
    pub fn spawn(&mut self, class: &str, object: Option<u64>) -> Result<u32, String> {
        let Some(ci) = self.classes.iter().position(|c| c.name == class) else {
            return Err(format!("no component class `{class}` is registered"));
        };
        let id = REG.with(|r| {
            let mut r = r.borrow_mut();
            let id = r.next_id;
            r.next_id += 1;
            id
        });
        // SAFETY: the thunk was generated for this class.
        let this = unsafe { (self.classes[ci].create)(id as f64, object.unwrap_or(0)) };
        let root = ts_rt_root_add(this);
        platypus_kinds::components_changed();
        REG.with(|r| {
            r.borrow_mut().live.push(Live {
                id,
                object: object.unwrap_or(0),
                sid: self.classes[ci].sid,
                bases: self.classes[ci].bases,
                class: ci,
                ptr: this,
            })
        });
        self.entries.push(Entry {
            id,
            class: ci,
            object,
            root,
            this,
            started: false,
            enabled: true,
        });
        let i = self.entries.len() - 1;
        if let Some(f) = self.call(i, HOOK_AWAKE, 0.0) {
            self.drop_entry(i);
            return Err(format!("{}.{} threw", f.class, f.hook));
        }
        self.adopt();
        Ok(id)
    }

    /// Runs what script asked for in a hook that has returned: components added get their `onAwake`
    /// and components removed their `onDestroy`. The host does this after every hook it runs; call
    /// it after calling compiled code directly.
    pub fn settle(&mut self) -> Vec<Fault> {
        self.adopt()
    }

    /// Takes the components `addComponent` made from script and runs their `onAwake`. A component
    /// made by an `onAwake` is taken in turn.
    fn adopt(&mut self) -> Vec<Fault> {
        let mut faults = Vec::new();
        // Components script removed: `onDestroy` runs, then they are forgotten.
        while let Some(id) = REG.with(|r| {
            let mut r = r.borrow_mut();
            (!r.removals.is_empty()).then(|| r.removals.remove(0))
        }) {
            self.destroy(id);
        }
        loop {
            let next = REG.with(|r| {
                let mut r = r.borrow_mut();
                (!r.pending.is_empty()).then(|| r.pending.remove(0))
            });
            let Some(p) = next else {
                return faults;
            };
            self.entries.push(Entry {
                id: p.id,
                class: p.class,
                object: (p.object != 0).then_some(p.object),
                root: p.root,
                this: p.ptr,
                started: false,
                enabled: true,
            });
            let i = self.entries.len() - 1;
            if let Some(f) = self.call(i, HOOK_AWAKE, 0.0) {
                faults.push(f);
            }
        }
    }

    /// One frame: `onStart` for new components, `onUpdate` for all, then `onLateUpdate` for all.
    /// `active` says whether an object is active in the scene.
    pub fn update(&mut self, dt: f64, active: &dyn Fn(u64) -> bool) -> Vec<Fault> {
        let mut faults = self.run(HOOK_UPDATE, dt, active);
        faults.extend(self.run(HOOK_LATE_UPDATE, dt, active));
        faults
    }

    /// One fixed tick.
    pub fn fixed_update(&mut self, dt: f64, active: &dyn Fn(u64) -> bool) -> Vec<Fault> {
        self.run(HOOK_FIXED_UPDATE, dt, active)
    }

    /// Just before the frame is drawn.
    pub fn pre_render(&mut self, active: &dyn Fn(u64) -> bool) -> Vec<Fault> {
        self.run(HOOK_PRE_RENDER, 0.0, active)
    }

    /// Runs `onDestroy` and forgets the component.
    pub fn destroy(&mut self, id: u32) {
        if let Some(i) = self.entries.iter().position(|e| e.id == id) {
            if self.entries[i].enabled {
                let _ = self.call(i, HOOK_DESTROY, 0.0);
            }
            self.drop_entry(i);
            self.adopt();
        }
    }

    /// The window gained or lost focus: `onApplicationFocus(hasFocus)` on the components of active
    /// objects.
    pub fn application_focus(&mut self, focused: bool, active: &dyn Fn(u64) -> bool) -> Vec<Fault> {
        self.flag_hook(HOOK_APP_FOCUS, focused, active)
    }

    /// The game paused in the background or resumed: `onApplicationPause(paused)`.
    pub fn application_pause(&mut self, paused: bool, active: &dyn Fn(u64) -> bool) -> Vec<Fault> {
        self.flag_hook(HOOK_APP_PAUSE, paused, active)
    }

    fn flag_hook(&mut self, hook: usize, flag: bool, active: &dyn Fn(u64) -> bool) -> Vec<Fault> {
        let mut faults = Vec::new();
        for i in 0..self.entries.len() {
            let e = &self.entries[i];
            if !e.enabled || e.object.is_some_and(|o| !active(o)) {
                continue;
            }
            if let Some(f) = self.call(i, hook, f64::from(u8::from(flag))) {
                faults.push(f);
            }
        }
        faults.extend(self.adopt());
        faults
    }

    /// Tells the components of each event's object about it: `onCollisionEnter` / `Exit` for
    /// solid contacts and `onTriggerEnter` / `Exit` for sensors, then `onCollisionStay` /
    /// `onTriggerStay` for every contact still touching (including the ones that just began).
    /// Call it after each fixed tick, with `[]` if there is nothing new. Components of inactive
    /// objects and disabled components are skipped (they miss the event, as in Unity); the
    /// `Stay` set keeps track either way.
    pub fn dispatch_contacts(
        &mut self,
        events: &[ContactEvent],
        active: &dyn Fn(u64) -> bool,
    ) -> Vec<Fault> {
        let mut faults = Vec::new();
        // The `Collision` handles of the last dispatch are done with.
        platypus_physics_api::contacts::clear_collisions();
        for ev in events {
            let same = |t: &ContactEvent| {
                t.object == ev.object && t.collider == ev.collider && t.other == ev.other
            };
            if ev.started {
                if !self.touching.iter().any(same) {
                    self.touching.push(*ev);
                }
            } else {
                self.touching.retain(|t| !same(t));
            }
            let hook = match (ev.sensor, ev.started) {
                (false, true) => CONTACT_ENTER,
                (false, false) => CONTACT_EXIT,
                (true, true) => TRIGGER_ENTER,
                (true, false) => TRIGGER_EXIT,
            };
            // Enter carries the velocity going in; Exit has no contact points.
            let velocity = ev.started.then_some(ev.velocity);
            faults.extend(self.contact_hook(hook, ev, velocity, ev.started, active));
        }
        for ev in self.touching.clone() {
            let hook = if ev.sensor {
                TRIGGER_STAY
            } else {
                CONTACT_STAY
            };
            faults.extend(self.contact_hook(hook, &ev, None, true, active));
        }
        faults
    }

    /// Takes the contacts the physics step queued (`platypus_physics_api::take_contacts`) and
    /// dispatches them: each contact tells the components of both objects (a collider with no
    /// object attached tells nobody).
    pub fn dispatch_physics(&mut self, active: &dyn Fn(u64) -> bool) -> Vec<Fault> {
        use ts_api::Handle;
        let raw = |c: u32| platypus_physics_api::object_of_collider(c).map(|o| o.to_raw());
        let mut events = Vec::new();
        for c in platypus_physics_api::take_contacts() {
            let (oa, ob) = (raw(c.a), raw(c.b));
            // `velocity` is b's minus a's: a is told the other way round.
            let v = c.velocity.map(f64::from);
            for (me, other, own, coll, velocity) in
                [(oa, ob, c.a, c.b, v), (ob, oa, c.b, c.a, v.map(|x| -x))]
            {
                if let Some(object) = me {
                    events.push(ContactEvent {
                        started: c.started,
                        object,
                        own: u64::from(own),
                        other: other.unwrap_or(0),
                        collider: u64::from(coll),
                        velocity,
                        sensor: c.sensor,
                    });
                }
            }
        }
        self.dispatch_contacts(&events, active)
    }

    /// Runs one contact hook on every component of the event's object that has it, with one
    /// `Collision` for all of them (`velocity`: the one going in, or `None` for the bodies' current
    /// velocities; `touching`: it has contact points).
    fn contact_hook(
        &mut self,
        hook: usize,
        ev: &ContactEvent,
        velocity: Option<[f64; 3]>,
        touching: bool,
        active: &dyn Fn(u64) -> bool,
    ) -> Vec<Fault> {
        let mut faults = Vec::new();
        if !active(ev.object) {
            return faults;
        }
        // Hooks may add or remove components: walk a snapshot of ids.
        let ids: Vec<u32> = self
            .entries
            .iter()
            .filter(|e| e.object == Some(ev.object))
            .map(|e| e.id)
            .collect();
        let mut collision = None;
        for id in ids {
            let Some(i) = self.entries.iter().position(|e| e.id == id) else {
                continue;
            };
            let e = &self.entries[i];
            if !e.enabled || self.classes[e.class].contact_hooks[hook].is_none() {
                continue;
            }
            let collision = *collision.get_or_insert_with(|| {
                platypus_physics_api::contacts::collision(
                    ev.own as u32,
                    ev.collider as u32,
                    ev.other,
                    velocity,
                    touching && !ev.sensor,
                )
            });
            if !e.started {
                self.entries[i].started = true;
                let f = self.call(i, HOOK_START, 0.0);
                faults.extend(self.adopt());
                if let Some(f) = f {
                    faults.push(f);
                    continue;
                }
                // `adopt` may have removed components, so the index is looked up again.
                if !self.entries.iter().any(|e| e.id == id) {
                    continue;
                }
            }
            let Some(i) = self.entries.iter().position(|e| e.id == id) else {
                continue;
            };
            if let Some(f) = self.call_contact(i, hook, collision, ev) {
                faults.push(f);
            }
            faults.extend(self.adopt());
        }
        faults
    }

    fn call_contact(
        &mut self,
        i: usize,
        hook: usize,
        collision: u64,
        ev: &ContactEvent,
    ) -> Option<Fault> {
        let e = &self.entries[i];
        let class = &self.classes[e.class];
        let f = class.contact_hooks[hook]?;
        let collider = platypus_physics_api::contacts::collider_component(ev.collider as u32);
        // SAFETY: `this` is rooted and has the class the thunk was generated for.
        unsafe { f(e.this, collision, collider) };
        ts_rt_microtasks_run();
        if ts_rt_exc_pending() {
            ts_rt_exc_take();
            let id = e.id;
            let name = class.name;
            self.entries[i].enabled = false;
            return Some(Fault {
                id,
                class: name,
                hook: CONTACT_NAMES[hook],
            });
        }
        None
    }

    /// Destroys every component on `objects` (the engine destroyed them).
    pub fn destroy_on_objects(&mut self, objects: &[u64]) {
        self.touching
            .retain(|t| !objects.contains(&t.object) && !objects.contains(&t.other));
        let ids: Vec<u32> = self
            .entries
            .iter()
            .filter(|e| e.object.is_some_and(|o| objects.contains(&o)))
            .map(|e| e.id)
            .collect();
        for id in ids {
            self.destroy(id);
        }
    }

    fn run(&mut self, hook: usize, dt: f64, active: &dyn Fn(u64) -> bool) -> Vec<Fault> {
        let mut faults = Vec::new();
        // Components spawned by a hook join the next pass, so the list is walked by index.
        let n = self.entries.len();
        for i in 0..n {
            let e = &self.entries[i];
            if !e.enabled || e.object.is_some_and(|o| !active(o)) {
                continue;
            }
            if !e.started {
                self.entries[i].started = true;
                let f = self.call(i, HOOK_START, 0.0);
                faults.extend(self.adopt());
                if let Some(f) = f {
                    faults.push(f);
                    continue;
                }
            }
            if let Some(f) = self.call(i, hook, dt) {
                faults.push(f);
            }
            faults.extend(self.adopt());
        }
        faults
    }

    /// Calls a hook if the class has it. A pending exception disables the component.
    fn call(&mut self, i: usize, hook: usize, dt: f64) -> Option<Fault> {
        let e = &self.entries[i];
        let class = &self.classes[e.class];
        let f = class.hooks[hook]?;
        // SAFETY: `this` is rooted and has the class the thunk was generated for.
        unsafe { f(e.this, dt) };
        ts_rt_microtasks_run();
        if ts_rt_exc_pending() {
            ts_rt_exc_take();
            let id = e.id;
            self.entries[i].enabled = false;
            return Some(Fault {
                id,
                class: class.name,
                hook: HOOK_NAMES[hook],
            });
        }
        None
    }

    fn drop_entry(&mut self, i: usize) {
        let e = self.entries.remove(i);
        REG.with(|r| r.borrow_mut().live.retain(|l| l.id != e.id));
        platypus_kinds::components_changed();
        ts_rt_root_remove(e.root);
    }

    /// The `@property` fields of a component: name and type name.
    pub fn properties(&self, id: u32) -> Result<Vec<(&'static str, &'static str)>, String> {
        property_table(id)
    }

    /// Reads a `@property` through the reflection table, with no script running.
    pub fn get_property(&self, id: u32, name: &str) -> Result<PropValue, String> {
        get_property(id, name)
    }

    /// Writes a `@property` through the reflection table.
    pub fn set_property(&mut self, id: u32, name: &str, v: PropValue) -> Result<(), String> {
        set_property(id, name, v)
    }

    /// Every supported `@property` of a component: what a scene file stores.
    pub fn save_properties(&self, id: u32) -> Result<Vec<(&'static str, PropValue)>, String> {
        save_properties(id)
    }

    /// Applies saved values; a value that does not fit is an error in the returned list and the
    /// rest are still applied.
    pub fn load_properties(&mut self, id: u32, values: &[(&str, PropValue)]) -> Vec<String> {
        load_properties(id, values)
    }
}

/// (pointer, class index) of a live component.
fn locate(id: u32) -> Result<(*mut u8, usize), String> {
    REG.with(|r| {
        r.borrow()
            .live
            .iter()
            .find(|l| l.id == id)
            .map(|l| (l.ptr, l.class))
    })
    .ok_or_else(|| format!("no component {id}"))
}

fn class_of(class: usize) -> NativeClass {
    REG.with(|r| r.borrow().classes[class].clone())
}

fn property_table(id: u32) -> Result<Vec<(&'static str, &'static str)>, String> {
    let (_, class) = locate(id)?;
    Ok(class_of(class).props.iter().map(|p| (p.0, p.2)).collect())
}

fn get_property(id: u32, name: &str) -> Result<PropValue, String> {
    let (this, ci) = locate(id)?;
    let class = class_of(ci);
    let &(_, off, kind) = class
        .props
        .iter()
        .find(|p| p.0 == name)
        .ok_or_else(|| format!("{} has no property `{name}`", class.name))?;
    let (base, nullable) = split_kind(kind);
    // SAFETY: the table offsets were generated for this class and its fields are 8 bytes.
    let word = unsafe { *(this.add(off) as *const u64) };
    match base {
        "number" => Ok(if nullable && word == NULL_NUM_BITS {
            PropValue::Null
        } else {
            PropValue::Num(f64::from_bits(word))
        }),
        "boolean" => Ok(match (nullable, word) {
            (true, 0) => PropValue::Null,
            (true, w) => PropValue::Bool(w == 2),
            (false, w) => PropValue::Bool(w != 0),
        }),
        "string" => Ok(if word == 0 {
            PropValue::Null
        } else {
            // SAFETY: a string field holds a live string or null.
            PropValue::Str(unsafe { Str::from_raw(word as *const Str) }.to_string_lossy())
        }),
        "handle:GameObject" => Ok(if word == 0 {
            PropValue::Null
        } else {
            PropValue::Object(word)
        }),
        b if let Some((n, count)) = value_class(b) => {
            if word == 0 {
                return Ok(PropValue::Null);
            }
            // SAFETY: a value class object is a header word and `count` f64 fields.
            let fields = (0..count)
                .map(|i| unsafe { *((word as *const f64).add(1 + i)) })
                .collect();
            Ok(PropValue::Value(n.to_string(), fields))
        }
        b if b.starts_with("object:") => {
            if word == 0 {
                return Ok(PropValue::Null);
            }
            REG.with(|r| {
                r.borrow()
                    .live
                    .iter()
                    .find(|l| l.ptr as u64 == word)
                    .map(|l| PropValue::Component(l.id))
            })
            .ok_or_else(|| format!("property `{name}` holds an object that is not a component"))
        }
        _ => Err(format!(
            "property `{name}` has the unsupported type `{kind}`"
        )),
    }
}

fn set_property(id: u32, name: &str, v: PropValue) -> Result<(), String> {
    let (this, ci) = locate(id)?;
    let class = class_of(ci);
    let &(_, off, kind) = class
        .props
        .iter()
        .find(|p| p.0 == name)
        .ok_or_else(|| format!("{} has no property `{name}`", class.name))?;
    let (base, nullable) = split_kind(kind);
    let word: u64 = match (base, &v) {
        ("number", PropValue::Null) if nullable => NULL_NUM_BITS,
        (_, PropValue::Null) if nullable => 0,
        ("number", PropValue::Num(n)) => n.to_bits(),
        ("boolean", PropValue::Bool(b)) => match (nullable, b) {
            (true, true) => 2,
            (true, false) => 1,
            (false, b) => *b as u64,
        },
        ("string", PropValue::Str(s)) => {
            // SAFETY: the bytes are valid UTF-8, hence WTF-8; this never collects.
            unsafe { ts_rt_str_new(s.as_ptr(), s.len() as i64) as u64 }
        }
        (b, PropValue::Value(n, fields)) if let Some((vn, count)) = value_class(b) => {
            if vn != n || fields.len() != count {
                return Err(format!("property `{name}` is `{kind}`, not {n}"));
            }
            // SAFETY: the object's fields are written in place, as `get_property` reads them.
            let word = unsafe { *(this.add(off) as *const u64) };
            if word == 0 {
                return Err(format!(
                    "property `{name}` is null; its value cannot be set"
                ));
            }
            for (i, f) in fields.iter().enumerate() {
                unsafe { *((word as *mut f64).add(1 + i)) = *f };
            }
            return Ok(());
        }
        ("handle:GameObject", PropValue::Object(o)) => *o,
        ("handle:GameObject", PropValue::Null) => 0,
        (b, PropValue::Component(c)) if b.starts_with("object:") => {
            let want = &b["object:".len()..];
            let (other, oi) = locate(*c)?;
            let (sid, bases) = REG.with(|r| {
                let r = r.borrow();
                let l = r.live.iter().find(|l| l.id == *c).expect("located above");
                (l.sid, l.bases)
            });
            let want_sid = REG.with(|r| {
                r.borrow()
                    .classes
                    .iter()
                    .find(|k| k.name == want)
                    .map(|k| k.sid)
            });
            if want_sid.is_none_or(|w| w != sid && !bases.contains(&w)) {
                return Err(format!(
                    "property `{name}` is `{kind}`, but component {c} is a {}",
                    class_of(oi).name
                ));
            }
            other as u64
        }
        _ => return Err(format!("property `{name}` is `{kind}`, not {v:?}")),
    };
    // SAFETY: as in `get_property`.
    unsafe { *(this.add(off) as *mut u64) = word };
    Ok(())
}

fn save_properties(id: u32) -> Result<Vec<(&'static str, PropValue)>, String> {
    let mut out = Vec::new();
    for (name, kind) in property_table(id)? {
        let base = split_kind(kind).0;
        if matches!(base, "number" | "boolean" | "string" | "handle:GameObject")
            || base.starts_with("object:")
        {
            // A field holding something that is not a component or value (a plain object) is left out.
            if let Ok(v) = get_property(id, name) {
                out.push((name, v));
            }
        }
    }
    Ok(out)
}

fn load_properties(id: u32, values: &[(&str, PropValue)]) -> Vec<String> {
    let mut errors = Vec::new();
    for (name, v) in values {
        if let Err(e) = set_property(id, name, v.clone()) {
            errors.push(e);
        }
    }
    errors
}

fn kind_value(v: PropValue) -> serde_json::Value {
    use serde_json::Value;
    match v {
        PropValue::Null => Value::Null,
        PropValue::Num(n) => serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number),
        PropValue::Bool(b) => Value::Bool(b),
        PropValue::Str(s) => Value::String(s),
        PropValue::Object(o) => platypus_kinds::object_ref(o),
        PropValue::Value(n, v) => serde_json::json!({ "$type": n, "value": v }),
        PropValue::Component(c) => {
            let name = locate(c)
                .map(|(_, ci)| class_of(ci).name)
                .unwrap_or_default();
            platypus_kinds::component_ref(name, u64::from(c))
        }
    }
}

fn prop_value(v: &serde_json::Value) -> Option<PropValue> {
    use serde_json::Value;
    Some(match v {
        Value::Null => PropValue::Null,
        Value::Number(n) => PropValue::Num(n.as_f64()?),
        Value::Bool(b) => PropValue::Bool(*b),
        Value::String(s) => PropValue::Str(s.clone()),
        Value::Object(o) if o.contains_key("$type") => {
            let n = o.get("$type")?.as_str()?;
            let fields = o
                .get("value")?
                .as_array()?
                .iter()
                .map(Value::as_f64)
                .collect::<Option<Vec<f64>>>()?;
            PropValue::Value(n.to_string(), fields)
        }
        Value::Object(o) => match o.get("$ref").and_then(Value::as_str)? {
            "GameObject" => PropValue::Object(o.get("object")?.as_u64()?),
            "Component" => PropValue::Component(o.get("handle")?.as_u64()? as u32),
            _ => return None,
        },
        Value::Array(_) => return None,
    })
}

/// Registers the scene-file kind of a class: components are made the way `addComponent` makes
/// them (asleep, woken when the host settles) and saved and loaded through the reflection table.
fn register_kind(name: &'static str) {
    use serde_json::{Map, Value};
    let sid = REG
        .with(|r| {
            r.borrow()
                .classes
                .iter()
                .find(|c| c.name == name)
                .map(|c| c.sid)
        })
        .expect("the class is in the table");
    platypus_kinds::register(platypus_kinds::Kind {
        name: name.to_string(),
        create: Box::new(move |object| {
            // A scene or prefab names its classes; one that no `register(..)` call named cannot
            // be made from a name (`addComponent(Class)` does not need the registration).
            if !is_registered(name) {
                return Err(format!(
                    "the component class `{name}` was never registered: call `register({name})`"
                ));
            }
            let ptr = ts_rt_host_add_gc(object, sid as f64);
            REG.with(|r| {
                r.borrow()
                    .live
                    .iter()
                    .find(|l| l.ptr == ptr)
                    .map(|l| u64::from(l.id))
            })
            .ok_or_else(|| "the component was not registered".to_string())
        }),
        on: Box::new(move |object| {
            REG.with(|r| {
                r.borrow()
                    .live
                    .iter()
                    .filter(|l| l.object == object && l.sid == sid)
                    .map(|l| u64::from(l.id))
                    .collect()
            })
        }),
        save: Box::new(|handle| {
            let mut m = Map::new();
            for (k, v) in save_properties(handle as u32).unwrap_or_default() {
                m.insert(k.to_string(), kind_value(v));
            }
            m
        }),
        load: Box::new(|handle, props: &Map<String, Value>| {
            let mut warnings = Vec::new();
            for (k, v) in props {
                match prop_value(v) {
                    Some(p) => {
                        if let Err(e) = set_property(handle as u32, k, p) {
                            warnings.push(e);
                        }
                    }
                    None => warnings.push(format!(
                        "property `{k}` has a value of an unsupported shape"
                    )),
                }
            }
            warnings
        }),
        // Components made by `create` wake when the host settles (after the calling hook).
        awake: Box::new(|_| {}),
        object: Box::new(|handle| {
            REG.with(|r| {
                r.borrow()
                    .live
                    .iter()
                    .find(|l| l.id == handle as u32)
                    .map(|l| l.object)
            })
        }),
        destroy: Box::new(|handle| {
            if let Ok((ptr, _)) = locate(handle as u32) {
                ts_rt_host_remove(ptr);
            }
        }),
    });
}

/// `"number?"` is (`"number"`, nullable).
fn split_kind(kind: &str) -> (&str, bool) {
    match kind.strip_suffix('?') {
        Some(b) => (b, true),
        None => (kind, false),
    }
}

const NULL_NUM_BITS: u64 = 0x7FF8_DEAD_BEEF_0001;
