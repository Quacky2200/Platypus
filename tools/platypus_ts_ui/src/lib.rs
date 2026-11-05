// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The bridge between compiled TypeScript UI code and the native reconciler (`platypus_ui_native`).
//!
//! Compiled code builds element descriptions through the `Ui` namespace (`Ui.begin("box")`,
//! `Ui.child(h, x)`, ...): the functions here keep them in a per-thread arena as plain handles.
//! `element_from(handle)` turns a handle into a `platypus_ui_native::Element`. Closures the script
//! hands over (render functions, event handlers, effects) are rooted with `ts_rt_root_add` until
//! the description holding them is dropped, and called later with no arguments.
//!
//! Hooks (`Ui.stateNew`, `Ui.load`, `Ui.store`, `Ui.memoSlot`, `Ui.effect`) act on the component
//! being rendered; the reconciler makes it current around each render. Hook state lives in
//! `ScriptCell`s, one per slot, holding a number, a boolean or a rooted script pointer.
//! Design: `docs/research/platypus-ui-native.md`; the TypeScript side is `tools/platypus_ts/src/lower/jsx.rs`
//! and the prelude in `ui.ts`.

mod mount;
pub use mount::{INSPECTOR_TARGET, root_manifest_json, take_mounts, take_window_mounts};

use platypus_ts_rt::{
    Array, Closure, ts_rt_exc_pending, ts_rt_exc_take, ts_rt_root_add, ts_rt_root_remove,
};
use platypus_ui_native::{
    Cleanup, Ctx, Dep, Element, Event, Handler, HostProps, NodeRef, RenderFn, Scheduler,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use ts_api::{return_string, script_str};

/// A script closure the host keeps alive: rooted for as long as this value lives.
struct Rooted {
    ptr: *mut Closure,
    root: usize,
}

impl Rooted {
    fn new(ptr: *mut Closure) -> Rooted {
        Rooted {
            ptr,
            root: ts_rt_root_add(ptr as *mut u8),
        }
    }

    fn call_void(&self) {
        // SAFETY: `ptr` is a live closure (rooted) of the signature the manifest promised.
        unsafe {
            let f: unsafe extern "C" fn(*mut u8) = std::mem::transmute((*self.ptr).func);
            f((*self.ptr).env);
        }
        check_exception();
    }

    /// Calls a closure that takes a handle (`(el: PlatypusElement | null) => void`; 0 is null).
    fn call_with_handle(&self, h: u64) {
        // SAFETY: as `call_void`, for a closure taking one 64-bit handle.
        unsafe {
            let f: unsafe extern "C" fn(*mut u8, u64) = std::mem::transmute((*self.ptr).func);
            f((*self.ptr).env, h);
        }
        check_exception();
    }

    fn call_u64(&self) -> u64 {
        // SAFETY: as `call_void`, for a closure returning a handle.
        let r = unsafe {
            let f: unsafe extern "C" fn(*mut u8) -> u64 = std::mem::transmute((*self.ptr).func);
            f((*self.ptr).env)
        };
        check_exception();
        r
    }
}

impl Drop for Rooted {
    fn drop(&mut self) {
        ts_rt_root_remove(self.root);
    }
}

fn check_exception() {
    if ts_rt_exc_pending() {
        ts_rt_exc_take();
        ERRORS.with(|e| {
            e.borrow_mut()
                .push("a UI callback threw an exception".into())
        });
    }
}

/// Problems the script caused since the last call (an exception in a callback).
pub fn take_errors() -> Vec<String> {
    ERRORS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}

enum Built {
    Host {
        ty: String,
        key: Option<String>,
        props: HostProps,
        children: Vec<u64>,
    },
    Text(String),
    Fragment {
        key: Option<String>,
        children: Vec<u64>,
    },
    Component {
        key: Option<String>,
        id: u32,
        render: Rc<Rooted>,
        memo: Option<Vec<Dep>>,
    },
    Provider {
        key: Option<String>,
        context: u32,
        value: Rc<ScriptCell>,
        children: Vec<u64>,
    },
}

#[derive(Default)]
struct Builder {
    items: Vec<Option<Built>>,
    free: Vec<usize>,
}

impl Builder {
    fn add(&mut self, b: Built) -> u64 {
        match self.free.pop() {
            Some(i) => {
                self.items[i] = Some(b);
                i as u64 + 1
            }
            None => {
                self.items.push(Some(b));
                self.items.len() as u64
            }
        }
    }

    fn take(&mut self, h: u64) -> Option<Built> {
        let i = (h as usize).checked_sub(1)?;
        let b = self.items.get_mut(i)?.take();
        if b.is_some() {
            self.free.push(i);
        }
        b
    }

    fn get(&mut self, h: u64) -> Option<&mut Built> {
        let i = (h as usize).checked_sub(1)?;
        self.items.get_mut(i)?.as_mut()
    }
}

/// A hook slot's value: a number, a boolean or a rooted script pointer, as bits.
struct ScriptCell {
    id: Cell<u64>,
    kind: Cell<u8>,
    bits: Cell<u64>,
    root: Cell<Option<usize>>,
    /// None for a provider's value, which nothing re-renders for.
    sched: Option<Scheduler>,
    /// Memo: the dependencies it was computed with, and whether the last render found it stale.
    deps: RefCell<Option<Vec<Dep>>>,
    stale: Cell<bool>,
}

const NUM: u8 = 1;
const BOOL: u8 = 2;
const PTR: u8 = 3;

impl ScriptCell {
    fn new(sched: Option<Scheduler>) -> ScriptCell {
        ScriptCell {
            id: Cell::new(0),
            kind: Cell::new(0),
            bits: Cell::new(0),
            root: Cell::new(None),
            sched,
            deps: RefCell::new(None),
            stale: Cell::new(false),
        }
    }

    fn set(&self, kind: u8, bits: u64) {
        let changed = self.kind.get() != kind || self.bits.get() != bits;
        if kind == PTR {
            if let Some(r) = self.root.take() {
                ts_rt_root_remove(r);
            }
            if bits != 0 {
                self.root.set(Some(ts_rt_root_add(bits as *mut u8)));
            }
        }
        self.kind.set(kind);
        self.bits.set(bits);
        // A set during a render only keeps the value; one from a handler asks for a render.
        if changed
            && CX.with(|c| c.get()).is_null()
            && let Some(s) = &self.sched
        {
            s.mark();
        }
    }
}

impl Drop for ScriptCell {
    fn drop(&mut self) {
        if let Some(r) = self.root.take() {
            ts_rt_root_remove(r);
        }
    }
}

thread_local! {
    static BUILDER: RefCell<Builder> = RefCell::new(Builder::default());
    static ERRORS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// The component being rendered (a `*mut Ctx`), or null.
    static CX: Cell<*mut ()> = const { Cell::new(std::ptr::null_mut()) };
    static EVENT: Cell<*mut Event> = const { Cell::new(std::ptr::null_mut()) };
    static DEPS: RefCell<Option<Vec<Dep>>> = const { RefCell::new(None) };
    static CLEANUP: RefCell<Option<Rooted>> = const { RefCell::new(None) };
    static CELLS: RefCell<HashMap<u64, Weak<ScriptCell>>> = RefCell::new(HashMap::new());
    static NEXT_CELL: Cell<u64> = const { Cell::new(1) };
    static NEXT_CONTEXT: Cell<u32> = const { Cell::new(1) };
}

fn with_cx<R>(f: impl FnOnce(&mut Ctx) -> R) -> R {
    let p = CX.with(|c| c.get());
    assert!(!p.is_null(), "a hook was called outside a component");
    // SAFETY: the pointer was set by the render closure below, which outlives this call.
    f(unsafe { &mut *(p as *mut Ctx<'static>) })
}

fn with_event<R>(f: impl FnOnce(&mut Event) -> R, default: R) -> R {
    let p = EVENT.with(|c| c.get());
    if p.is_null() {
        return default;
    }
    // SAFETY: set by the handler closure for the duration of its call.
    f(unsafe { &mut *p })
}

/// The cell of a slot whose component is mounted (or being rendered).
fn cell_of(slot: f64) -> Rc<ScriptCell> {
    live_cell(slot).expect("a hook slot that is gone")
}

/// The cell of a slot, or `None` once its component has unmounted (a setter kept by a promise,
/// a timer or a store's listener can outlive it).
fn live_cell(slot: f64) -> Option<Rc<ScriptCell>> {
    CELLS.with(|c| c.borrow().get(&(slot as u64)).and_then(Weak::upgrade))
}

/// A store into a slot: as in React, setting the state of an unmounted component does nothing.
fn store(slot: f64, kind: u8, bits: u64) {
    if let Some(cell) = live_cell(slot) {
        cell.set(kind, bits);
    }
}

/// The cell of the next hook of the component being rendered (made on its first render).
fn next_cell() -> Rc<ScriptCell> {
    with_cx(|cx| {
        let sched = cx.scheduler();
        let cell = cx.use_shared(move || ScriptCell::new(Some(sched)));
        register_cell(&cell);
        cell
    })
}

/// Gives a cell its id (once) and makes it findable by `cell_of`.
fn register_cell(cell: &Rc<ScriptCell>) {
    if cell.id.get() == 0 {
        let id = NEXT_CELL.with(|n| {
            let v = n.get();
            n.set(v + 1);
            v
        });
        cell.id.set(id);
        CELLS.with(|c| c.borrow_mut().insert(id, Rc::downgrade(cell)));
    }
}

fn component_render(rc: Rc<Rooted>) -> RenderFn {
    Rc::new(move |cx: &mut Ctx| {
        let prev = CX.with(|c| c.replace(cx as *mut Ctx as *mut ()));
        DEPS.with(|d| *d.borrow_mut() = None);
        let h = rc.call_u64();
        CX.with(|c| c.set(prev));
        element_from(h)
    })
}

/// The element a handle stands for (and everything under it); the handles are used up.
pub fn element_from(h: u64) -> Element {
    if h == 0 {
        return Element::None;
    }
    let Some(b) = BUILDER.with(|b| b.borrow_mut().take(h)) else {
        return Element::None;
    };
    match b {
        Built::Text(s) => Element::Text(s),
        Built::Host {
            ty,
            key,
            props,
            children,
        } => Element::Host {
            ty,
            key,
            props,
            children: children.into_iter().map(element_from).collect(),
        },
        Built::Fragment { key, children } => Element::Fragment {
            key,
            children: children.into_iter().map(element_from).collect(),
        },
        Built::Component {
            key,
            id,
            render,
            memo,
        } => Element::Component {
            key,
            id,
            render: component_render(render),
            memo,
        },
        Built::Provider {
            key,
            context,
            value,
            children,
        } => Element::Provider {
            key,
            context,
            value,
            children: children.into_iter().map(element_from).collect(),
        },
    }
}

fn text(p: *const u8) -> String {
    // SAFETY: the compiled code passes a live script string (or null).
    unsafe { script_str(p) }.into_owned()
}

fn with_built<R>(h: u64, f: impl FnOnce(&mut Built) -> R) -> Option<R> {
    BUILDER.with(|b| b.borrow_mut().get(h).map(f))
}

fn add(b: Built) -> u64 {
    BUILDER.with(|x| x.borrow_mut().add(b))
}

// ---- elements ----

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_begin(ty: *const u8) -> u64 {
    add(Built::Host {
        ty: text(ty),
        key: None,
        props: HostProps::default(),
        children: Vec::new(),
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_fragment() -> u64 {
    add(Built::Fragment {
        key: None,
        children: Vec::new(),
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_component(id: f64, render: *mut Closure) -> u64 {
    // A dependency list built just before is `memo`'s props.
    let memo = DEPS.with(|d| d.borrow_mut().take());
    add(Built::Component {
        key: None,
        id: id as u32,
        render: Rc::new(Rooted::new(render)),
        memo,
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_key(h: u64, k: *const u8) -> u64 {
    let k = text(k);
    with_built(h, |b| match b {
        Built::Host { key, .. }
        | Built::Fragment { key, .. }
        | Built::Component { key, .. }
        | Built::Provider { key, .. } => *key = Some(k),
        Built::Text(_) => {}
    });
    h
}

/// `key={3}`: a number key is its text.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_key_num(h: u64, k: f64) -> u64 {
    let k = format_number(k);
    with_built(h, |b| match b {
        Built::Host { key, .. }
        | Built::Fragment { key, .. }
        | Built::Component { key, .. }
        | Built::Provider { key, .. } => *key = Some(k),
        Built::Text(_) => {}
    });
    h
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_on(h: u64, kind: *const u8, f: *mut Closure) -> u64 {
    let name = text(kind);
    let rc = Rooted::new(f);
    let handler: Handler = Rc::new(move |ev: &mut Event| {
        let prev = EVENT.with(|c| c.replace(ev as *mut Event));
        rc.call_void();
        EVENT.with(|c| c.set(prev));
    });
    with_built(h, |b| {
        if let Built::Host { props, .. } = b {
            props.handlers.push((name, handler));
        }
    });
    h
}

fn style_value(h: u64, k: *const u8, v: platypus_core::Value) -> u64 {
    let k = text(k);
    with_built(h, |b| {
        if let Built::Host { props, .. } = b {
            props.style.retain(|(n, _)| *n != k);
            props.style.push((k, v));
        }
    });
    h
}

fn attr_value(h: u64, k: *const u8, v: platypus_core::Value) -> u64 {
    let k = text(k);
    with_built(h, |b| {
        if let Built::Host { props, .. } = b {
            props.attrs.retain(|(n, _)| *n != k);
            props.attrs.push((k, v));
        }
    });
    h
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_style_num(h: u64, k: *const u8, v: f64) -> u64 {
    style_value(h, k, platypus_core::Value::Number(v))
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_style_str(h: u64, k: *const u8, v: *const u8) -> u64 {
    style_value(h, k, platypus_core::Value::String(text(v)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_attr_num(h: u64, k: *const u8, v: f64) -> u64 {
    attr_value(h, k, platypus_core::Value::Number(v))
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_attr_str(h: u64, k: *const u8, v: *const u8) -> u64 {
    attr_value(h, k, platypus_core::Value::String(text(v)))
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_attr_bool(h: u64, k: *const u8, v: bool) -> u64 {
    attr_value(h, k, platypus_core::Value::Bool(v))
}

fn push_child(h: u64, child: u64) -> u64 {
    if child != 0 {
        with_built(h, |b| match b {
            Built::Host { children, .. }
            | Built::Fragment { children, .. }
            | Built::Provider { children, .. } => children.push(child),
            _ => {}
        });
    }
    h
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_child_el(h: u64, child: u64) -> u64 {
    push_child(h, child)
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_child_str(h: u64, s: *const u8) -> u64 {
    let t = add(Built::Text(text(s)));
    push_child(h, t)
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_child_num(h: u64, n: f64) -> u64 {
    let t = add(Built::Text(format_number(n)));
    push_child(h, t)
}

/// `{flag}` as a child renders nothing (`true`, `false`), as in React.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_child_bool(h: u64, _b: bool) -> u64 {
    h
}

#[unsafe(no_mangle)]
/// # Safety
/// `items` is null or a script array of handles.
pub unsafe extern "C" fn ts_ui_child_arr(h: u64, items: *const Array) -> u64 {
    if !items.is_null() {
        // SAFETY: a script array of handles: `len` 8-byte elements.
        let list: Vec<u64> = unsafe {
            let a = &*items;
            // An empty array may have no buffer (a null pointer is not a valid slice).
            if a.len == 0 || a.data.is_null() {
                Vec::new()
            } else {
                std::slice::from_raw_parts(a.data as *const u64, a.len as usize).to_vec()
            }
        };
        for c in list {
            push_child(h, c);
        }
    }
    h
}

/// JavaScript's number-to-string for the common cases (integers print without a point).
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// A script object the host keeps alive and writes a field of.
struct RootedObj {
    ptr: *mut u8,
    root: usize,
}

impl Drop for RootedObj {
    fn drop(&mut self) {
        ts_rt_root_remove(self.root);
    }
}

/// `ref={r}`: the node is written into the ref object (`current`, its first field, as a handle:
/// the node id plus one, 0 for none) when the element attaches and clears.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ref(h: u64, obj: u64) -> u64 {
    if obj != 0 {
        let held = RootedObj {
            ptr: obj as *mut u8,
            root: ts_rt_root_add(obj as *mut u8),
        };
        let node_ref = NodeRef::with_callback(move |n| {
            let v = n.map_or(0, |n| u64::from(n.raw()) + 1);
            // SAFETY: the object is a `Ref<T>`: a header and then the 8-byte `current` field.
            unsafe { *(held.ptr as *mut u64).add(1) = v };
        });
        with_built(h, |b| {
            if let Built::Host { props, .. } = b {
                props.node_ref = Some(node_ref);
            }
        });
    }
    h
}

/// `ref={(el) => ..}`: a callback ref, called with the node when the element attaches and with
/// null (0) when it detaches, as React does.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ref_fn(h: u64, f: *mut Closure) -> u64 {
    if !f.is_null() {
        let rc = Rooted::new(f);
        let node_ref = NodeRef::with_callback(move |n| {
            rc.call_with_handle(n.map_or(0, |n| u64::from(n.raw()) + 1))
        });
        with_built(h, |b| {
            if let Built::Host { props, .. } = b {
                props.node_ref = Some(node_ref);
            }
        });
    }
    h
}

/// A `UiNode` handle as the node id.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_node_id(n: u64) -> f64 {
    n.wrapping_sub(1) as f64
}

/// What a script asked of a node it holds a ref to (`ref.current?.focus()`). The host that owns
/// the `UiTree` and its interaction state applies them (see `take_node_commands`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NodeCommand {
    Focus(u32),
    Blur(u32),
    /// Scroll the node's content to this offset (NaN: that axis stays), as the behaviour says.
    ScrollTo(u32, f64, f64, platypus_ui::ScrollBehavior),
    /// Scroll the node's content by this much.
    ScrollBy(u32, f64, f64, platypus_ui::ScrollBehavior),
    /// `scrollIntoView({ block, inline, behavior })`.
    ScrollIntoView {
        id: u32,
        block: platypus_ui::ScrollAlign,
        inline: platypus_ui::ScrollAlign,
        behavior: platypus_ui::ScrollBehavior,
    },
    /// `dataTransfer.setDragImage(element, x, y)`: the element to show under the pointer (0 for
    /// none) and where it is held.
    DragImage(u32, f64, f64),
    /// `setPointerCapture(pointerId)`: the pointer's events go to the node until released.
    SetPointerCapture(u32),
    /// `releasePointerCapture(pointerId)`.
    ReleasePointerCapture(u32),
}

thread_local! {
    static NODE_COMMANDS: RefCell<Vec<NodeCommand>> = const { RefCell::new(Vec::new()) };
}

/// The node commands since the last call, in order.
pub fn take_node_commands() -> Vec<NodeCommand> {
    NODE_COMMANDS.with(|c| std::mem::take(&mut *c.borrow_mut()))
}

fn node_command(n: u64, f: impl FnOnce(u32) -> NodeCommand) {
    if n != 0 {
        NODE_COMMANDS.with(|c| c.borrow_mut().push(f((n - 1) as u32)));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_node_focus(n: u64) {
    node_command(n, NodeCommand::Focus);
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_node_blur(n: u64) {
    node_command(n, NodeCommand::Blur);
}

/// `setPointerCapture(pointerId)`; there is one pointer, so the id is not checked.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_node_set_capture(n: u64, _pointer: f64) {
    node_command(n, NodeCommand::SetPointerCapture);
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_node_release_capture(n: u64, _pointer: f64) {
    node_command(n, NodeCommand::ReleasePointerCapture);
}

/// A `behavior` argument: `"auto"`, `"instant"` or `"smooth"` (left out or unknown: auto).
fn behavior(b: *const u8) -> platypus_ui::ScrollBehavior {
    platypus_ui::ScrollBehavior::parse(&text(b)).unwrap_or_default()
}

/// `scrollTo(x, y)` or `scrollTo({ left, top, behavior })`: a left-out axis (NaN) stays.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_node_scroll_to(n: u64, x: f64, y: f64, b: *const u8) {
    let b = behavior(b);
    node_command(n, |id| NodeCommand::ScrollTo(id, x, y, b));
}

/// `scrollBy(dx, dy)` or `scrollBy({ left, top, behavior })`: a left-out axis is 0.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_node_scroll_by(n: u64, dx: f64, dy: f64, b: *const u8) {
    let b = behavior(b);
    let or0 = |v: f64| if v.is_nan() { 0.0 } else { v };
    node_command(n, |id| NodeCommand::ScrollBy(id, or0(dx), or0(dy), b));
}

/// `scrollIntoView({ block, inline, behavior })` with the DOM's defaults (`block: "start"`,
/// `inline: "nearest"`, `behavior: "auto"`) for keys left out.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_node_scroll_into_view(
    n: u64,
    block: *const u8,
    inline: *const u8,
    b: *const u8,
) {
    let align =
        |s: *const u8, default| platypus_ui::ScrollAlign::parse(&text(s)).unwrap_or(default);
    let (block, inline) = (
        align(block, platypus_ui::ScrollAlign::Start),
        align(inline, platypus_ui::ScrollAlign::Nearest),
    );
    let behavior = behavior(b);
    node_command(n, |id| NodeCommand::ScrollIntoView {
        id,
        block,
        inline,
        behavior,
    });
}

/// `dataTransfer.setDragImage`: `id` 0 shows no drag image.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_drag_image(id: f64, x: f64, y: f64) {
    NODE_COMMANDS.with(|c| c.borrow_mut().push(NodeCommand::DragImage(id as u32, x, y)));
}

// ---- context ----

fn provider(context: f64, kind: u8, bits: u64) -> u64 {
    let cell = Rc::new(ScriptCell::new(None));
    register_cell(&cell);
    cell.set(kind, bits);
    add(Built::Provider {
        key: None,
        context: context as u32,
        value: cell,
        children: Vec::new(),
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_provider_num(context: f64, v: f64) -> u64 {
    provider(context, NUM, v.to_bits())
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_provider_bool(context: f64, v: bool) -> u64 {
    provider(context, BOOL, u64::from(v))
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_provider_ptr(context: f64, v: u64) -> u64 {
    provider(context, PTR, v)
}

/// The slot of the nearest provider's value for context `id`, or 0 when there is none.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_context_slot(id: f64) -> f64 {
    with_cx(|cx| {
        cx.find_context(id as u32)
            .and_then(|v| v.downcast::<ScriptCell>().ok())
            .map_or(0.0, |c| c.id.get() as f64)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_context_id() -> f64 {
    NEXT_CONTEXT.with(|n| {
        let v = n.get();
        n.set(v + 1);
        f64::from(v)
    })
}

// ---- hooks ----

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_deps_begin() {
    DEPS.with(|d| *d.borrow_mut() = Some(Vec::new()));
}

fn push_dep(d: Dep) {
    DEPS.with(|x| x.borrow_mut().get_or_insert_with(Vec::new).push(d));
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_dep_num(v: f64) {
    push_dep(Dep::Num(v));
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_dep_bool(v: bool) {
    push_dep(Dep::Bool(v));
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_dep_str(v: *const u8) {
    push_dep(Dep::Str(text(v)));
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_dep_ptr(v: u64) {
    push_dep(Dep::Id(v));
}

fn state_new(kind: u8, bits: u64) -> f64 {
    let cell = next_cell();
    if cell.kind.get() == 0 {
        cell.set(kind, bits);
    }
    cell.id.get() as f64
}

/// `useState(() => init)`: the hook's slot, made before the initial value is computed.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_state_slot() -> f64 {
    next_cell().id.get() as f64
}

/// Has the slot never been given a value (the first render)?
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_state_empty(slot: f64) -> bool {
    cell_of(slot).kind.get() == 0
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_state_num(init: f64) -> f64 {
    state_new(NUM, init.to_bits())
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_state_bool(init: bool) -> f64 {
    state_new(BOOL, u64::from(init))
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_state_ptr(init: u64) -> f64 {
    state_new(PTR, init)
}

/// An engine handle (`useState(camera)`): its 64-bit id, kept like a number (not traced).
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_state_handle(init: u64) -> f64 {
    state_new(NUM, init)
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_load_handle(slot: f64) -> u64 {
    cell_of(slot).bits.get()
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_store_handle(slot: f64, v: u64) {
    store(slot, NUM, v);
}

/// Is the slot's component still mounted? An updater (`setN(n => n + 1)`) or a reducer's
/// `dispatch` called after it unmounted then runs nothing, as in React.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_state_live(slot: f64) -> bool {
    live_cell(slot).is_some()
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_load_num(slot: f64) -> f64 {
    f64::from_bits(cell_of(slot).bits.get())
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_load_bool(slot: f64) -> bool {
    cell_of(slot).bits.get() != 0
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_load_ptr(slot: f64) -> u64 {
    cell_of(slot).bits.get()
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_store_num(slot: f64, v: f64) {
    store(slot, NUM, v.to_bits());
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_store_bool(slot: f64, v: bool) {
    store(slot, BOOL, u64::from(v));
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_store_ptr(slot: f64, v: u64) {
    store(slot, PTR, v);
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_memo_slot() -> f64 {
    let deps = DEPS.with(|d| d.borrow_mut().take());
    let cell = next_cell();
    let stale = match (&deps, &*cell.deps.borrow()) {
        _ if cell.kind.get() == 0 => true,
        (None, _) => true,
        (Some(d), Some(old)) => d != old,
        (Some(_), None) => true,
    };
    *cell.deps.borrow_mut() = deps;
    cell.stale.set(stale);
    cell.id.get() as f64
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_stale(slot: f64) -> bool {
    cell_of(slot).stale.get()
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_effect(f: *mut Closure) {
    let rc = Rooted::new(f);
    let deps = DEPS.with(|d| d.borrow_mut().take());
    with_cx(|cx| {
        cx.use_effect(deps.as_deref(), move || {
            rc.call_void();
            CLEANUP
                .with(|c| c.borrow_mut().take())
                .map(|c| Box::new(move || c.call_void()) as Cleanup)
        });
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_set_cleanup(f: *mut Closure) {
    CLEANUP.with(|c| {
        *c.borrow_mut() = if f.is_null() {
            None
        } else {
            Some(Rooted::new(f))
        }
    });
}

// ---- events ----

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ev_num(i: f64) -> f64 {
    with_event(|e| e.data.get(i as usize).copied().unwrap_or(0.0), 0.0)
}

/// # Safety
/// `out_len` is a valid pointer (the compiled code passes a stack slot).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ts_ui_ev_str(i: f64, out_len: *mut i64) -> *const u8 {
    let s = with_event(
        |e| e.strings.get(i as usize).cloned().unwrap_or_default(),
        String::new(),
    );
    // SAFETY: per the contract above.
    unsafe { return_string(s, out_len) }
}

/// How many strings the event carries (the dragged files of a drag event).
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ev_str_count() -> f64 {
    with_event(|e| e.strings.len() as f64, 0.0)
}

/// # Safety
/// `out_len` is a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ts_ui_ev_kind(out_len: *mut i64) -> *const u8 {
    let s = with_event(|e| e.kind.clone(), String::new());
    // SAFETY: per the contract above.
    unsafe { return_string(s, out_len) }
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ev_target() -> f64 {
    with_event(|e| f64::from(e.target.raw()), 0.0)
}

/// `currentTarget`: the element whose handler runs, as a `UiNode` handle (0 outside a dispatch).
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ev_current() -> u64 {
    with_event(|e| e.current.map_or(0, |n| u64::from(n.raw()) + 1), 0)
}

/// `defaultPrevented`: whether a handler of this event called `preventDefault()` so far.
#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ev_prevented() -> bool {
    with_event(|e| e.prevented, false)
}

type KindResolver = fn(u32) -> String;

thread_local! {
    static TARGET_KIND: Cell<Option<KindResolver>> = const { Cell::new(None) };
}

/// Tells `e.target.type` what element kind (`"input"`, `"box"`, ..) a node id is; the host that
/// owns the tree sets it (without one the type is empty).
pub fn set_target_kind_resolver(f: fn(u32) -> String) {
    TARGET_KIND.with(|c| c.set(Some(f)));
}

/// # Safety
/// `out_len` is a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ts_ui_ev_target_kind(out_len: *mut i64) -> *const u8 {
    let id = with_event(|e| e.target.raw(), 0);
    let s = TARGET_KIND
        .with(|c| c.get())
        .map(|f| f(id))
        .unwrap_or_default();
    // SAFETY: per the contract above.
    unsafe { return_string(s, out_len) }
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ev_stop() {
    with_event(Event::stop_propagation, ());
}

#[unsafe(no_mangle)]
pub extern "C" fn ts_ui_ev_prevent() {
    with_event(Event::prevent_default, ());
}

/// Keeps every bridge function linked into a program that only names them as symbols.
pub fn link_anchor() -> usize {
    [
        ts_ui_begin as *const () as usize,
        ts_ui_fragment as *const () as usize,
        ts_ui_component as *const () as usize,
        ts_ui_key as *const () as usize,
        ts_ui_key_num as *const () as usize,
        ts_ui_child_bool as *const () as usize,
        ts_ui_node_focus as *const () as usize,
        ts_ui_ref_fn as *const () as usize,
        ts_ui_node_blur as *const () as usize,
        ts_ui_node_scroll_to as *const () as usize,
        ts_ui_node_scroll_by as *const () as usize,
        ts_ui_node_scroll_into_view as *const () as usize,
        ts_ui_state_slot as *const () as usize,
        ts_ui_state_empty as *const () as usize,
        ts_ui_state_live as *const () as usize,
        ts_ui_on as *const () as usize,
        ts_ui_style_num as *const () as usize,
        ts_ui_style_str as *const () as usize,
        ts_ui_attr_num as *const () as usize,
        ts_ui_attr_str as *const () as usize,
        ts_ui_attr_bool as *const () as usize,
        ts_ui_child_el as *const () as usize,
        ts_ui_child_str as *const () as usize,
        ts_ui_child_num as *const () as usize,
        ts_ui_child_arr as *const () as usize,
        ts_ui_deps_begin as *const () as usize,
        ts_ui_dep_num as *const () as usize,
        ts_ui_dep_bool as *const () as usize,
        ts_ui_dep_str as *const () as usize,
        ts_ui_dep_ptr as *const () as usize,
        ts_ui_state_num as *const () as usize,
        ts_ui_state_bool as *const () as usize,
        ts_ui_state_ptr as *const () as usize,
        ts_ui_load_num as *const () as usize,
        ts_ui_load_bool as *const () as usize,
        ts_ui_load_ptr as *const () as usize,
        ts_ui_state_handle as *const () as usize,
        ts_ui_load_handle as *const () as usize,
        ts_ui_store_handle as *const () as usize,
        ts_ui_store_num as *const () as usize,
        ts_ui_store_bool as *const () as usize,
        ts_ui_store_ptr as *const () as usize,
        ts_ui_memo_slot as *const () as usize,
        ts_ui_stale as *const () as usize,
        ts_ui_effect as *const () as usize,
        ts_ui_set_cleanup as *const () as usize,
        ts_ui_ref as *const () as usize,
        ts_ui_node_id as *const () as usize,
        ts_ui_provider_num as *const () as usize,
        ts_ui_provider_bool as *const () as usize,
        ts_ui_provider_ptr as *const () as usize,
        ts_ui_context_slot as *const () as usize,
        ts_ui_context_id as *const () as usize,
        ts_ui_ev_num as *const () as usize,
        ts_ui_ev_str as *const () as usize,
        ts_ui_ev_str_count as *const () as usize,
        ts_ui_drag_image as *const () as usize,
        ts_ui_ev_kind as *const () as usize,
        ts_ui_ev_target as *const () as usize,
        ts_ui_ev_current as *const () as usize,
        ts_ui_ev_prevented as *const () as usize,
        ts_ui_node_set_capture as *const () as usize,
        ts_ui_node_release_capture as *const () as usize,
        ts_ui_ev_target_kind as *const () as usize,
        ts_ui_ev_stop as *const () as usize,
        ts_ui_ev_prevent as *const () as usize,
    ]
    .iter()
    .fold(0, |a, p| a ^ p)
}

/// The `Ui` namespace as the compiler reads it (`platypus_ts` manifest JSON).
pub fn manifest_json() -> String {
    use serde_json::{Value, json};
    fn f(name: &str, symbol: &str, params: &[(&str, &str)], returns: &str) -> Value {
        json!({
            "name": name,
            "symbol": symbol,
            "params": params.iter().map(|(n, t)| json!({"name": n, "type": t})).collect::<Vec<_>>(),
            "returns": returns,
        })
    }
    let el = "handle:UiElement";
    let els = "handles:UiElement";
    let functions = vec![
        f("begin", "ts_ui_begin", &[("type", "string")], el),
        f("fragment", "ts_ui_fragment", &[], el),
        f(
            "component",
            "ts_ui_component",
            &[("id", "f64"), ("render", "fn:handle:UiElement")],
            el,
        ),
        f("key", "ts_ui_key", &[("h", el), ("key", "string")], el),
        f("key", "ts_ui_key_num", &[("h", el), ("key", "f64")], el),
        f(
            "on",
            "ts_ui_on",
            &[("h", el), ("kind", "string"), ("f", "fn:void")],
            el,
        ),
        f(
            "style",
            "ts_ui_style_num",
            &[("h", el), ("key", "string"), ("v", "f64")],
            el,
        ),
        f(
            "style",
            "ts_ui_style_str",
            &[("h", el), ("key", "string"), ("v", "string")],
            el,
        ),
        f(
            "attr",
            "ts_ui_attr_num",
            &[("h", el), ("key", "string"), ("v", "f64")],
            el,
        ),
        f(
            "attr",
            "ts_ui_attr_str",
            &[("h", el), ("key", "string"), ("v", "string")],
            el,
        ),
        f(
            "attr",
            "ts_ui_attr_bool",
            &[("h", el), ("key", "string"), ("v", "bool")],
            el,
        ),
        f("child", "ts_ui_child_el", &[("h", el), ("c", el)], el),
        f(
            "child",
            "ts_ui_child_str",
            &[("h", el), ("s", "string")],
            el,
        ),
        f("child", "ts_ui_child_num", &[("h", el), ("n", "f64")], el),
        f("child", "ts_ui_child_arr", &[("h", el), ("items", els)], el),
        f("child", "ts_ui_child_bool", &[("h", el), ("b", "bool")], el),
        f("ref", "ts_ui_ref", &[("h", el), ("r", "ptr")], el),
        // `Ui.ref` with a function: the lowering picks it by the value's type.
        f(
            "refFn",
            "ts_ui_ref_fn",
            &[("h", el), ("f", "fn(handle:UiNode):void")],
            el,
        ),
        f("nodeId", "ts_ui_node_id", &[("n", "handle:UiNode")], "f64"),
        f(
            "provider",
            "ts_ui_provider_num",
            &[("ctx", "f64"), ("v", "f64")],
            el,
        ),
        f(
            "provider",
            "ts_ui_provider_bool",
            &[("ctx", "f64"), ("v", "bool")],
            el,
        ),
        f(
            "provider",
            "ts_ui_provider_ptr",
            &[("ctx", "f64"), ("v", "ptr")],
            el,
        ),
        f("contextSlot", "ts_ui_context_slot", &[("id", "f64")], "f64"),
        f("contextId", "ts_ui_context_id", &[], "f64"),
        f("depsBegin", "ts_ui_deps_begin", &[], "void"),
        f("dep", "ts_ui_dep_num", &[("v", "f64")], "void"),
        f("dep", "ts_ui_dep_bool", &[("v", "bool")], "void"),
        f("dep", "ts_ui_dep_str", &[("v", "string")], "void"),
        f("dep", "ts_ui_dep_ptr", &[("v", "ptr")], "void"),
        f("stateSlot", "ts_ui_state_slot", &[], "f64"),
        f(
            "stateEmpty",
            "ts_ui_state_empty",
            &[("slot", "f64")],
            "bool",
        ),
        f("stateLive", "ts_ui_state_live", &[("slot", "f64")], "bool"),
        f("stateNew", "ts_ui_state_num", &[("init", "f64")], "f64"),
        f("stateNew", "ts_ui_state_bool", &[("init", "bool")], "f64"),
        f("stateNew", "ts_ui_state_ptr", &[("init", "ptr")], "f64"),
        f(
            "store",
            "ts_ui_store_num",
            &[("slot", "f64"), ("v", "f64")],
            "void",
        ),
        f(
            "store",
            "ts_ui_store_bool",
            &[("slot", "f64"), ("v", "bool")],
            "void",
        ),
        f(
            "store",
            "ts_ui_store_ptr",
            &[("slot", "f64"), ("v", "ptr")],
            "void",
        ),
        f("memoSlot", "ts_ui_memo_slot", &[], "f64"),
        f("stale", "ts_ui_stale", &[("slot", "f64")], "bool"),
        f("effect", "ts_ui_effect", &[("f", "fn:void")], "void"),
        f("evNum", "ts_ui_ev_num", &[("i", "f64")], "f64"),
        f("evStr", "ts_ui_ev_str", &[("i", "f64")], "string"),
        f("evStrCount", "ts_ui_ev_str_count", &[], "f64"),
        f("evKind", "ts_ui_ev_kind", &[], "string"),
        f(
            "dragImage",
            "ts_ui_drag_image",
            &[("id", "f64"), ("x", "f64"), ("y", "f64")],
            "void",
        ),
        f("evTarget", "ts_ui_ev_target", &[], "f64"),
        f("evCurrent", "ts_ui_ev_current", &[], "handle:UiNode"),
        f("evPrevented", "ts_ui_ev_prevented", &[], "bool"),
        f("evTargetKind", "ts_ui_ev_target_kind", &[], "string"),
        f("evStop", "ts_ui_ev_stop", &[], "void"),
        f("evPrevent", "ts_ui_ev_prevent", &[], "void"),
        // Special forms the compiler lowers itself (`Ui.load<T>(slot)`, `Ui.runEffect(f)`); listed
        // so the namespace has them.
        f("load", "ts_ui_load_num", &[("slot", "f64")], "f64"),
        f("runEffect", "ts_ui_set_cleanup", &[("f", "ptr")], "void"),
        f(
            "styleObj",
            "ts_ui_style_num",
            &[("h", el), ("obj", "ptr")],
            el,
        ),
        f(
            "attrObj",
            "ts_ui_attr_num",
            &[("h", el), ("obj", "ptr")],
            el,
        ),
    ];
    // Methods of a node a `ref` holds (`ref.current?.focus()`).
    let node = |name: &str, symbol: &str, extra: &[(&str, &str)]| {
        let mut params = vec![("this", "handle:UiNode")];
        params.extend_from_slice(extra);
        let mut v = f(name, symbol, &params, "void");
        v["kind"] = json!("method");
        v
    };
    // Parameters from `from` on may be left out (NaN, an empty string); the string ones are
    // literal unions in the declarations (`ts`, see `aliases`).
    let optional = |v: &mut Value, from: usize| {
        for p in v["params"].as_array_mut().into_iter().flatten().skip(from) {
            p["optional"] = json!(true);
            match p["name"].as_str() {
                Some("behavior") => p["ts"] = json!("ScrollBehavior"),
                Some("block" | "inline") => p["ts"] = json!("ScrollLogicalPosition"),
                _ => {}
            }
        }
    };
    // `scrollTo(x, y)` / `scrollTo({ left, top, behavior })`, also `scrollBy`.
    let scroll = |name: &str, symbol: &str, options: &str| {
        let mut v = node(
            name,
            symbol,
            &[("left", "f64"), ("top", "f64"), ("behavior", "string")],
        );
        optional(&mut v, 1);
        v["optionsName"] = json!(options);
        v
    };
    let mut into_view = node(
        "scrollIntoView",
        "ts_ui_node_scroll_into_view",
        &[
            ("block", "string"),
            ("inline", "string"),
            ("behavior", "string"),
        ],
    );
    optional(&mut into_view, 1);
    into_view["optionsName"] = json!("ScrollIntoViewOptions");
    into_view["optionsOnly"] = json!(true);
    let classes = vec![json!({
        "name": "UiNode",
        "functions": [
            node("focus", "ts_ui_node_focus", &[]),
            node("blur", "ts_ui_node_blur", &[]),
            node(
                "setPointerCapture",
                "ts_ui_node_set_capture",
                &[("pointerId", "f64")],
            ),
            node(
                "releasePointerCapture",
                "ts_ui_node_release_capture",
                &[("pointerId", "f64")],
            ),
            scroll("scrollTo", "ts_ui_node_scroll_to", "ScrollToOptions"),
            scroll("scrollBy", "ts_ui_node_scroll_by", "ScrollToOptions"),
            into_view,
        ],
    })];
    // The DOM's names for the scroll options' strings.
    let aliases = json!([
        { "name": "ScrollBehavior", "ts": "\"auto\" | \"instant\" | \"smooth\"" },
        { "name": "ScrollLogicalPosition", "ts": "\"start\" | \"center\" | \"end\" | \"nearest\"" },
    ]);
    json!({ "namespace": "Ui", "functions": functions, "classes": classes, "aliases": aliases })
        .to_string()
}

include!(concat!(env!("OUT_DIR"), "/symbols.rs"));
