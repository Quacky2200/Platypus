// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Element descriptions: what a component returns. Plain data; the reconciler diffs it against
//! the previous tree.

use crate::hooks::{Ctx, Dep};
use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;
use web_ui_core::{NodeId, Value};

/// An event handler. Capture handlers are registered under the name `"<kind>:capture"`.
pub type Handler = Rc<dyn Fn(&mut Event)>;
/// A component's render function: its props are captured by the closure.
pub type RenderFn = Rc<dyn Fn(&mut Ctx) -> Element>;
/// An effect's cleanup.
pub type Cleanup = Box<dyn FnOnce()>;

/// A reference to a host element's node (`useRef` + `ref=`): set when the element is attached,
/// cleared when it is removed.
#[derive(Clone, Default)]
pub struct NodeRef(Rc<RefState>);

#[derive(Default)]
struct RefState {
    node: Cell<Option<NodeId>>,
    /// Told about every change (the script bridge writes the node into the script's ref object).
    on_change: Option<Box<dyn Fn(Option<NodeId>)>>,
}

impl NodeRef {
    pub fn new() -> Self {
        Self::default()
    }

    /// A reference that also calls `f` whenever the node it points to changes.
    pub fn with_callback(f: impl Fn(Option<NodeId>) + 'static) -> Self {
        NodeRef(Rc::new(RefState {
            node: Cell::new(None),
            on_change: Some(Box::new(f)),
        }))
    }

    pub fn get(&self) -> Option<NodeId> {
        self.0.node.get()
    }

    pub(crate) fn set(&self, id: Option<NodeId>) {
        self.0.node.set(id);
        if let Some(f) = &self.0.on_change {
            f(id);
        }
    }

    pub(crate) fn same(&self, other: &NodeRef) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// The props of a host element. Style and attribute values are the raw values `platypus_ui` parses.
#[derive(Clone, Default)]
pub struct HostProps {
    pub style: Vec<(String, Value)>,
    pub attrs: Vec<(String, Value)>,
    pub handlers: Vec<(String, Handler)>,
    pub node_ref: Option<NodeRef>,
}

#[derive(Clone)]
pub enum Element {
    /// Renders nothing (`null`, `false`).
    None,
    Text(String),
    Host {
        ty: String,
        key: Option<String>,
        props: HostProps,
        children: Vec<Element>,
    },
    Fragment {
        key: Option<String>,
        children: Vec<Element>,
    },
    /// A component: `id` says which function it is (two elements with different ids never share
    /// state), `render` is called with the hooks.
    Component {
        key: Option<String>,
        id: u32,
        render: RenderFn,
        /// `memo`: the props as dependencies. An update with equal ones renders nothing (unless
        /// the component's own state changed).
        memo: Option<Vec<Dep>>,
    },
    /// Children mounted under another node (`createPortal`).
    Portal {
        target: NodeId,
        children: Vec<Element>,
    },
    /// A context provider.
    Provider {
        key: Option<String>,
        context: u32,
        value: Rc<dyn Any>,
        children: Vec<Element>,
    },
}

impl Element {
    pub fn text(s: impl Into<String>) -> Element {
        Element::Text(s.into())
    }

    pub fn host(ty: &str, props: HostProps, children: Vec<Element>) -> Element {
        Element::Host {
            ty: ty.to_owned(),
            key: None,
            props,
            children,
        }
    }

    pub fn fragment(children: Vec<Element>) -> Element {
        Element::Fragment {
            key: None,
            children,
        }
    }

    pub fn component(id: u32, render: impl Fn(&mut Ctx) -> Element + 'static) -> Element {
        Element::Component {
            key: None,
            id,
            render: Rc::new(render),
            memo: None,
        }
    }

    pub fn portal(target: NodeId, children: Vec<Element>) -> Element {
        Element::Portal { target, children }
    }

    /// The same element with a key.
    pub fn keyed(mut self, k: &str) -> Element {
        match &mut self {
            Element::Host { key, .. }
            | Element::Fragment { key, .. }
            | Element::Component { key, .. }
            | Element::Provider { key, .. } => *key = Some(k.to_owned()),
            Element::None | Element::Text(_) | Element::Portal { .. } => {}
        }
        self
    }

    pub(crate) fn key(&self) -> Option<&str> {
        match self {
            Element::Host { key, .. }
            | Element::Fragment { key, .. }
            | Element::Component { key, .. }
            | Element::Provider { key, .. } => key.as_deref(),
            _ => None,
        }
    }
}

/// A UI event travelling through the handlers. Capture runs from the outermost ancestor to the
/// target, bubbling back out (when `bubbles`), along the **component tree** (portal content
/// bubbles to its logical parent, as in React).
pub struct Event {
    pub kind: String,
    pub target: NodeId,
    pub bubbles: bool,
    pub data: Vec<f64>,
    pub strings: Vec<String>,
    pub prevented: bool,
    /// The element whose handlers are running (`currentTarget`); set by the dispatch.
    pub current: Option<NodeId>,
    pub(crate) stopped: bool,
}

impl Event {
    pub fn new(kind: &str, target: NodeId) -> Event {
        Event {
            kind: kind.to_owned(),
            target,
            bubbles: true,
            data: Vec::new(),
            strings: Vec::new(),
            prevented: false,
            current: None,
            stopped: false,
        }
    }

    /// Stops the event before the next element; the current element's other handlers still run.
    pub fn stop_propagation(&mut self) {
        self.stopped = true;
    }

    pub fn prevent_default(&mut self) {
        self.prevented = true;
    }
}
