// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A native reconciler with React's re-render semantics, replacing `react-reconciler` for UI that
//! runs without a JavaScript engine. Design: `docs/research/platypus-ui-native.md`.
//!
//! A component is a function from hooks to an [`Element`] description. The [`Runtime`] keeps an
//! instance tree parallel to the descriptions, diffs each new description against it and emits
//! `platypus_ui` ops ([`platypus_ui::opcodes::Op`]) applied to the [`UiTree`] as one batch per update.
//! Keys, hooks (`state`, `ref`, `memo`, `callback`, `effect`, `context`), portals, refs and event
//! dispatch with capture and bubble along the component tree are built; `memo`, `forwardRef`,
//! `useReducer` and `useSyncExternalStore` come after the compiler side.

mod element;
mod hooks;

pub use element::{Cleanup, Element, Event, Handler, HostProps, NodeRef, RenderFn};
pub use hooks::{Context, Ctx, Dep, EffectFn, Scheduler, Setter};

use hooks::{Shared, Slot};
use platypus_ui::opcodes::Op;
use platypus_ui::{UiError, UiTree};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use web_ui_core::{NodeId, Value};

/// Updates a flush may chain before it gives up (an effect that always sets state).
const MAX_PASSES: usize = 100;

/// A UI event of the interaction layer as the reconciler's event: the same numbers and strings
/// `UiEvent::script_args` gives `__platypus_ui_event` (so a compiled handler reads what a QuickJS one
/// does), bubbling unless the event type does not (`tables::events::NON_BUBBLING`).
pub fn event_of(e: &platypus_ui::UiEvent) -> Option<Event> {
    let &target = e.path.first()?;
    let mut ev = Event::new(e.kind.name(), target);
    ev.bubbles = !platypus_ui::tables::events::NON_BUBBLING.contains(&e.kind.name());
    let args = e.script_args();
    if let Some(Value::Array(a)) = args.get(2) {
        ev.data = a.iter().filter_map(Value::as_f64).collect();
    }
    if let Some(Value::Array(a)) = args.get(3) {
        ev.strings = a
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
    }
    Some(ev)
}

/// A mounted root: a container node of a `UiTree` and the element rendered into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RootId(usize);

enum Kind {
    Root {
        container: NodeId,
        nodes: Vec<NodeId>,
        child: Option<usize>,
    },
    Nothing,
    Text {
        node: NodeId,
        text: String,
    },
    Host {
        node: NodeId,
        ty: String,
        key: Option<String>,
        props: HostProps,
        children: Vec<usize>,
        /// The node's children in the tree, in order.
        nodes: Vec<NodeId>,
    },
    Fragment {
        key: Option<String>,
        children: Vec<usize>,
    },
    Component {
        key: Option<String>,
        id: u32,
        render: RenderFn,
        memo: Option<Vec<Dep>>,
        hooks: Vec<Slot>,
        child: Option<usize>,
    },
    Portal {
        target: NodeId,
        children: Vec<usize>,
        nodes: Vec<NodeId>,
    },
    Provider {
        key: Option<String>,
        context: u32,
        value: Rc<dyn Any>,
        children: Vec<usize>,
    },
}

struct Inst {
    parent: Option<usize>,
    kind: Kind,
}

enum CleanupItem {
    Run(Cleanup),
    /// The cleanup stored in this effect slot (component, slot index).
    Slot(usize, usize),
}

/// Everything one update decides, applied together: ops first, then refs, cleanups, effects.
#[derive(Default)]
struct Pass {
    ops: Vec<Op>,
    cleanups: Vec<CleanupItem>,
    effects: Vec<(usize, usize)>,
    refs: Vec<(NodeRef, Option<NodeId>)>,
    rendered: HashSet<usize>,
}

pub struct Runtime {
    arena: Vec<Option<Inst>>,
    shared: Rc<Shared>,
    next_node: u32,
    by_node: HashMap<NodeId, usize>,
    /// What a text field holds as far as we know: its `value` prop, or what was typed since.
    field_text: HashMap<usize, String>,
    /// Fields whose `input` handlers ran and whose value prop is yet to be put back.
    pending_restore: Vec<usize>,
    /// Ops applied so far (for tests and statistics).
    ops_total: usize,
    /// During a `refresh`: components (by id) that start over, and whether `memo` is ignored.
    remount: HashSet<u32>,
    force: bool,
}

fn diff_pairs(old: &[(String, Value)], new: &[(String, Value)]) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for (k, v) in new {
        if old
            .iter()
            .find(|(ok, _)| ok == k)
            .is_none_or(|(_, ov)| ov != v)
        {
            out.push((k.clone(), v.clone()));
        }
    }
    for (k, _) in old {
        if new.iter().all(|(nk, _)| nk != k) {
            out.push((k.clone(), Value::Null));
        }
    }
    out
}

impl Runtime {
    /// A runtime whose nodes get ids from `first_node` up. Pick a range the script-side React
    /// reconciler does not use if both draw into one tree.
    pub fn new(first_node: u32) -> Self {
        Runtime {
            arena: Vec::new(),
            shared: Rc::default(),
            next_node: first_node,
            by_node: HashMap::new(),
            field_text: HashMap::new(),
            pending_restore: Vec::new(),
            ops_total: 0,
            remount: HashSet::new(),
            force: false,
        }
    }

    /// How many ops this runtime has applied to trees.
    pub fn ops_applied(&self) -> usize {
        self.ops_total
    }

    /// Number of live instances (for tests).
    pub fn instance_count(&self) -> usize {
        self.arena.iter().flatten().count()
    }

    /// The tree node of the host element or text a click on `node` would reach.
    pub fn is_mounted(&self, node: NodeId) -> bool {
        self.by_node.contains_key(&node)
    }

    pub fn create_root(&mut self, container: NodeId) -> RootId {
        RootId(self.alloc(Inst {
            parent: None,
            kind: Kind::Root {
                container,
                nodes: Vec::new(),
                child: None,
            },
        }))
    }

    /// Renders `element` into the root (the first time, or again with new props) and applies the
    /// result, effects included.
    pub fn render(&mut self, root: RootId, element: Element, tree: &mut UiTree) -> Vec<UiError> {
        let mut pass = Pass::default();
        let old = match &mut self.inst_mut(root.0).kind {
            Kind::Root { child, .. } => child.take(),
            _ => unreachable!("a root"),
        };
        let child = self.reconcile_single(root.0, old, element, &mut pass);
        let desired = self.host_nodes(child);
        if let Kind::Root { child: c, .. } = &mut self.inst_mut(root.0).kind {
            *c = Some(child);
        }
        self.place_children(root.0, desired, &mut pass);
        let mut errors = self.commit(pass, tree);
        errors.extend(self.flush(tree));
        errors
    }

    /// Hot reload: renders the new code's element over the old tree. Every component renders again
    /// with its new render function (`memo` is ignored) and keeps its state, except those in
    /// `remount` (their hook signature changed), which start over.
    pub fn refresh(
        &mut self,
        root: RootId,
        element: Element,
        remount: &[u32],
        tree: &mut UiTree,
    ) -> Vec<UiError> {
        self.remount = remount.iter().copied().collect();
        self.force = true;
        let errors = self.render(root, element, tree);
        self.remount.clear();
        self.force = false;
        errors
    }

    /// Renders nothing into the root: everything unmounts and effects clean up.
    pub fn unmount_root(&mut self, root: RootId, tree: &mut UiTree) -> Vec<UiError> {
        self.render(root, Element::None, tree)
    }

    /// Runs the components whose state changed (and the effects they cause) until none is left.
    pub fn flush(&mut self, tree: &mut UiTree) -> Vec<UiError> {
        let mut errors = Vec::new();
        for _ in 0..MAX_PASSES {
            let mut dirty: Vec<usize> = std::mem::take(&mut *self.shared.dirty.borrow_mut());
            dirty.sort_by_key(|i| (self.depth(*i), *i));
            dirty.dedup();
            let dirty: Vec<usize> = dirty
                .into_iter()
                .filter(|i| {
                    matches!(
                        self.arena[*i].as_ref().map(|n| &n.kind),
                        Some(Kind::Component { .. })
                    )
                })
                .collect();
            if dirty.is_empty() {
                return errors;
            }
            let mut pass = Pass::default();
            for id in dirty {
                // An ancestor that rendered in this pass already used the new state.
                if self.arena[id].is_none() || pass.rendered.contains(&id) {
                    continue;
                }
                self.render_component(id, &mut pass);
                self.fix_placement(id, &mut pass);
            }
            errors.extend(self.commit(pass, tree));
        }
        errors.push(UiError(format!(
            "more than {MAX_PASSES} chained updates: a component sets state on every render"
        )));
        self.shared.dirty.borrow_mut().clear();
        errors
    }

    /// Dispatches an event from `event.target`: capture handlers from the outermost component-tree
    /// ancestor in, then bubble handlers back out. Returns whether a handler prevented the default.
    pub fn dispatch(&mut self, event: &mut Event, tree: &mut UiTree) -> bool {
        let prevented = self.dispatch_handlers(event);
        self.settle(tree);
        prevented
    }

    /// The element kind (`"input"`, `"box"`, ..) of a host node this runtime made.
    pub fn host_kind(&self, node: NodeId) -> Option<String> {
        match &self.inst(*self.by_node.get(&node)?).kind {
            Kind::Host { ty, .. } => Some(ty.clone()),
            _ => None,
        }
    }

    /// Like [`Runtime::dispatch`] without the tree: the handlers run and the default action's
    /// answer comes back at once, so a host that holds the tree (an interaction layer deciding
    /// what the default action is) can use it. Call [`Runtime::settle`] afterwards.
    pub fn dispatch_handlers(&mut self, event: &mut Event) -> bool {
        let Some(&start) = self.by_node.get(&event.target) else {
            return false;
        };
        // The host elements from the target up through the component tree.
        let mut chain = Vec::new();
        let mut cur = Some(start);
        while let Some(i) = cur {
            if matches!(self.inst(i).kind, Kind::Host { .. }) {
                chain.push(i);
            }
            cur = self.inst(i).parent;
        }
        // A text field's own text changes before any handler runs, like `e.target.value`.
        let is_field = matches!(&self.inst(start).kind,
            Kind::Host { ty, .. } if ty == "input" || ty == "textarea");
        if is_field && event.kind == "input" {
            self.field_text
                .insert(start, event.strings.first().cloned().unwrap_or_default());
        }
        let capture = format!("{}:capture", event.kind);
        // The element's handlers for `name`, with the element as `currentTarget`.
        let handlers = |rt: &Runtime, i: usize, name: &str, event: &mut Event| -> Vec<Handler> {
            match &rt.inst(i).kind {
                Kind::Host { props, node, .. } => {
                    event.current = Some(*node);
                    props
                        .handlers
                        .iter()
                        .filter(|(n, _)| n == name)
                        .map(|(_, h)| h.clone())
                        .collect()
                }
                _ => Vec::new(),
            }
        };
        'phases: {
            for &i in chain.iter().rev() {
                for h in handlers(self, i, &capture, event) {
                    h(event);
                }
                if event.stopped {
                    break 'phases;
                }
            }
            // The target's own handlers run for every event; the ancestors' only when it bubbles.
            {
                let name = event.kind.clone();
                let last = if event.bubbles { chain.len() } else { 1 };
                for &i in chain.iter().take(last) {
                    for h in handlers(self, i, &name, event) {
                        h(event);
                    }
                    if event.stopped {
                        break 'phases;
                    }
                }
            }
        }
        // As in the DOM, `currentTarget` is null once the dispatch is over.
        event.current = None;
        if is_field && event.kind == "input" {
            self.pending_restore.push(start);
        }
        event.prevented
    }

    /// Commits what [`Runtime::dispatch_handlers`] changed (a discrete event commits before it
    /// returns) and puts controlled fields back.
    pub fn settle(&mut self, tree: &mut UiTree) -> Vec<UiError> {
        let errors = self.flush(tree);
        for i in std::mem::take(&mut self.pending_restore) {
            self.restore_controlled(i, tree);
        }
        errors
    }

    /// A field with a `value` prop shows that value: if the handlers did not accept what was
    /// typed (no state change, or a different one), the prop wins and the tree's text goes back.
    fn restore_controlled(&mut self, i: usize, tree: &mut UiTree) {
        let Some(Inst {
            kind: Kind::Host { node, props, .. },
            ..
        }) = self.arena[i].as_ref()
        else {
            return;
        };
        let Some(Value::String(want)) = props
            .attrs
            .iter()
            .find(|(k, _)| k == "value")
            .map(|(_, v)| v.clone())
        else {
            return;
        };
        if self.field_text.get(&i) != Some(&want) {
            let op = Op::SetAttrs {
                id: *node,
                pairs: vec![("value".into(), Value::String(want.clone()))],
            };
            tree.apply_ops(std::slice::from_ref(&op));
            self.ops_total += 1;
            self.field_text.insert(i, want);
        }
    }

    // ---- arena ----

    fn alloc(&mut self, inst: Inst) -> usize {
        self.arena.push(Some(inst));
        self.arena.len() - 1
    }

    fn inst(&self, i: usize) -> &Inst {
        self.arena[i].as_ref().expect("a live instance")
    }

    fn inst_mut(&mut self, i: usize) -> &mut Inst {
        self.arena[i].as_mut().expect("a live instance")
    }

    fn new_node(&mut self) -> NodeId {
        let id = NodeId(self.next_node);
        self.next_node += 1;
        id
    }

    fn depth(&self, mut i: usize) -> usize {
        let mut d = 0;
        while let Some(p) = self.arena[i].as_ref().and_then(|n| n.parent) {
            d += 1;
            i = p;
        }
        d
    }

    fn key_of(&self, i: usize) -> Option<&str> {
        match &self.inst(i).kind {
            Kind::Host { key, .. }
            | Kind::Fragment { key, .. }
            | Kind::Component { key, .. }
            | Kind::Provider { key, .. } => key.as_deref(),
            _ => None,
        }
    }

    fn compatible(&self, i: usize, el: &Element) -> bool {
        match (&self.inst(i).kind, el) {
            (Kind::Nothing, Element::None) | (Kind::Text { .. }, Element::Text(_)) => true,
            (Kind::Host { ty, .. }, Element::Host { ty: t, .. }) => ty == t,
            (Kind::Fragment { .. }, Element::Fragment { .. }) => true,
            (Kind::Component { id, .. }, Element::Component { id: c, .. }) => {
                id == c && !self.remount.contains(id)
            }
            (Kind::Portal { target, .. }, Element::Portal { target: t, .. }) => target == t,
            (Kind::Provider { context, .. }, Element::Provider { context: c, .. }) => context == c,
            _ => false,
        }
    }

    fn find_provider(&self, from: usize, context: u32) -> Option<Rc<dyn Any>> {
        let mut cur = self.inst(from).parent;
        while let Some(p) = cur {
            let inst = self.inst(p);
            if let Kind::Provider {
                context: c, value, ..
            } = &inst.kind
                && *c == context
            {
                return Some(value.clone());
            }
            cur = inst.parent;
        }
        None
    }

    // ---- nodes of an instance ----

    /// The tree nodes an instance puts directly into its container, in order.
    fn host_nodes(&self, i: usize) -> Vec<NodeId> {
        match &self.inst(i).kind {
            Kind::Host { node, .. } | Kind::Text { node, .. } => vec![*node],
            Kind::Fragment { children, .. } | Kind::Provider { children, .. } => {
                children.iter().flat_map(|c| self.host_nodes(*c)).collect()
            }
            Kind::Component { child, .. } => child.map(|c| self.host_nodes(c)).unwrap_or_default(),
            Kind::Portal { .. } | Kind::Nothing | Kind::Root { .. } => Vec::new(),
        }
    }

    /// The children of a container-owning instance, as the nodes they place.
    fn desired_of(&self, owner: usize) -> Vec<NodeId> {
        match &self.inst(owner).kind {
            Kind::Host { children, .. } | Kind::Portal { children, .. } => {
                children.iter().flat_map(|c| self.host_nodes(*c)).collect()
            }
            Kind::Root { child, .. } => child.map(|c| self.host_nodes(c)).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Brings the container's children in the tree to `desired`: removes what is gone, inserts and
    /// moves the rest. `owner` is the host, portal or root that owns the container.
    fn place_children(&mut self, owner: usize, desired: Vec<NodeId>, pass: &mut Pass) {
        let (container, mut current) = match &mut self.inst_mut(owner).kind {
            Kind::Host { node, nodes, .. } => (*node, std::mem::take(nodes)),
            Kind::Portal { target, nodes, .. } => (*target, std::mem::take(nodes)),
            Kind::Root {
                container, nodes, ..
            } => (*container, std::mem::take(nodes)),
            _ => unreachable!("a container"),
        };
        for gone in current.iter().filter(|n| !desired.contains(n)) {
            pass.ops.push(Op::RemoveChild {
                parent: container,
                child: *gone,
            });
        }
        current.retain(|n| desired.contains(n));
        if current.is_empty() {
            for n in &desired {
                pass.ops.push(Op::AppendChild {
                    parent: container,
                    child: *n,
                });
            }
            current = desired;
        } else {
            // Right to left: each node must sit just before the one placed after it.
            let mut before: Option<NodeId> = None;
            for &d in desired.iter().rev() {
                let pos = current.iter().position(|c| *c == d);
                let in_place = pos.is_some_and(|p| current.get(p + 1).copied() == before);
                if !in_place {
                    pass.ops.push(match before {
                        Some(b) => Op::InsertBefore {
                            parent: container,
                            child: d,
                            before: b,
                        },
                        None => Op::AppendChild {
                            parent: container,
                            child: d,
                        },
                    });
                    if let Some(p) = pos {
                        current.remove(p);
                    }
                    let at = before
                        .and_then(|b| current.iter().position(|c| *c == b))
                        .unwrap_or(current.len());
                    current.insert(at, d);
                }
                before = Some(d);
            }
        }
        match &mut self.inst_mut(owner).kind {
            Kind::Host { nodes, .. } | Kind::Portal { nodes, .. } | Kind::Root { nodes, .. } => {
                *nodes = current;
            }
            _ => unreachable!("a container"),
        }
    }

    /// After a component rendered on its own, its host nodes may have changed: re-place the
    /// container that holds them.
    fn fix_placement(&mut self, comp: usize, pass: &mut Pass) {
        let mut cur = self.inst(comp).parent;
        while let Some(p) = cur {
            if matches!(
                self.inst(p).kind,
                Kind::Host { .. } | Kind::Portal { .. } | Kind::Root { .. }
            ) {
                let desired = self.desired_of(p);
                self.place_children(p, desired, pass);
                return;
            }
            cur = self.inst(p).parent;
        }
    }

    // ---- mount, update, unmount ----

    fn mount(&mut self, el: Element, parent: Option<usize>, pass: &mut Pass) -> usize {
        match el {
            Element::None => self.alloc(Inst {
                parent,
                kind: Kind::Nothing,
            }),
            Element::Text(text) => {
                let node = self.new_node();
                pass.ops.push(Op::CreateText {
                    id: node,
                    text: text.clone(),
                });
                let id = self.alloc(Inst {
                    parent,
                    kind: Kind::Text { node, text },
                });
                self.by_node.insert(node, id);
                id
            }
            Element::Host {
                ty,
                key,
                props,
                children,
            } => {
                let node = self.new_node();
                pass.ops.push(Op::Create {
                    id: node,
                    kind: ty.clone(),
                });
                if !props.style.is_empty() {
                    pass.ops.push(Op::SetStyle {
                        id: node,
                        pairs: props.style.clone(),
                    });
                }
                if !props.attrs.is_empty() {
                    pass.ops.push(Op::SetAttrs {
                        id: node,
                        pairs: props.attrs.clone(),
                    });
                }
                if let Some(r) = &props.node_ref {
                    pass.refs.push((r.clone(), Some(node)));
                }
                let id = self.alloc(Inst {
                    parent,
                    kind: Kind::Host {
                        node,
                        ty,
                        key,
                        props,
                        children: Vec::new(),
                        nodes: Vec::new(),
                    },
                });
                self.by_node.insert(node, id);
                let kids: Vec<usize> = children
                    .into_iter()
                    .map(|c| self.mount(c, Some(id), pass))
                    .collect();
                self.set_children(id, kids);
                let desired = self.desired_of(id);
                self.place_children(id, desired, pass);
                id
            }
            Element::Fragment { key, children } => {
                let id = self.alloc(Inst {
                    parent,
                    kind: Kind::Fragment {
                        key,
                        children: Vec::new(),
                    },
                });
                let kids: Vec<usize> = children
                    .into_iter()
                    .map(|c| self.mount(c, Some(id), pass))
                    .collect();
                self.set_children(id, kids);
                id
            }
            Element::Provider {
                key,
                context,
                value,
                children,
            } => {
                let id = self.alloc(Inst {
                    parent,
                    kind: Kind::Provider {
                        key,
                        context,
                        value,
                        children: Vec::new(),
                    },
                });
                let kids: Vec<usize> = children
                    .into_iter()
                    .map(|c| self.mount(c, Some(id), pass))
                    .collect();
                self.set_children(id, kids);
                id
            }
            Element::Portal { target, children } => {
                let id = self.alloc(Inst {
                    parent,
                    kind: Kind::Portal {
                        target,
                        children: Vec::new(),
                        nodes: Vec::new(),
                    },
                });
                let kids: Vec<usize> = children
                    .into_iter()
                    .map(|c| self.mount(c, Some(id), pass))
                    .collect();
                self.set_children(id, kids);
                let desired = self.desired_of(id);
                self.place_children(id, desired, pass);
                id
            }
            Element::Component {
                key,
                id,
                render,
                memo,
            } => {
                let inst = self.alloc(Inst {
                    parent,
                    kind: Kind::Component {
                        key,
                        id,
                        render,
                        memo,
                        hooks: Vec::new(),
                        child: None,
                    },
                });
                self.render_component(inst, pass);
                inst
            }
        }
    }

    fn set_children(&mut self, owner: usize, kids: Vec<usize>) {
        match &mut self.inst_mut(owner).kind {
            Kind::Host { children, .. }
            | Kind::Fragment { children, .. }
            | Kind::Portal { children, .. }
            | Kind::Provider { children, .. } => *children = kids,
            _ => unreachable!("an instance with children"),
        }
    }

    fn take_children(&mut self, owner: usize) -> Vec<usize> {
        match &mut self.inst_mut(owner).kind {
            Kind::Host { children, .. }
            | Kind::Fragment { children, .. }
            | Kind::Portal { children, .. }
            | Kind::Provider { children, .. } => std::mem::take(children),
            _ => unreachable!("an instance with children"),
        }
    }

    /// Updates instance `i` in place to match `el` (already known to be compatible).
    fn update(&mut self, i: usize, el: Element, pass: &mut Pass) {
        match el {
            Element::None => {}
            Element::Text(new) => {
                let Kind::Text { node, text } = &mut self.inst_mut(i).kind else {
                    unreachable!("compatible")
                };
                if *text != new {
                    pass.ops.push(Op::SetText {
                        id: *node,
                        text: new.clone(),
                    });
                    *text = new;
                }
            }
            Element::Host {
                key,
                props,
                children,
                ..
            } => {
                let Kind::Host {
                    node,
                    key: k,
                    props: old,
                    ..
                } = &mut self.inst_mut(i).kind
                else {
                    unreachable!("compatible")
                };
                let node = *node;
                let style = diff_pairs(&old.style, &props.style);
                if !style.is_empty() {
                    pass.ops.push(Op::SetStyle {
                        id: node,
                        pairs: style,
                    });
                }
                let attrs = diff_pairs(&old.attrs, &props.attrs);
                if !attrs.is_empty() {
                    pass.ops.push(Op::SetAttrs {
                        id: node,
                        pairs: attrs,
                    });
                }
                match (&old.node_ref, &props.node_ref) {
                    (Some(a), Some(b)) if a.same(b) => {}
                    (a, b) => {
                        if let Some(a) = a {
                            pass.refs.push((a.clone(), None));
                        }
                        if let Some(b) = b {
                            pass.refs.push((b.clone(), Some(node)));
                        }
                    }
                }
                *k = key;
                let shown =
                    props
                        .attrs
                        .iter()
                        .find(|(n, _)| n == "value")
                        .and_then(|(_, v)| match v {
                            Value::String(t) => Some(t.clone()),
                            _ => None,
                        });
                *old = props;
                if let Some(t) = shown {
                    self.field_text.insert(i, t);
                }
                let old_kids = self.take_children(i);
                let kids = self.reconcile_children(i, old_kids, children, pass);
                self.set_children(i, kids);
                let desired = self.desired_of(i);
                self.place_children(i, desired, pass);
            }
            Element::Fragment { key, children } => {
                if let Kind::Fragment { key: k, .. } = &mut self.inst_mut(i).kind {
                    *k = key;
                }
                let old_kids = self.take_children(i);
                let kids = self.reconcile_children(i, old_kids, children, pass);
                self.set_children(i, kids);
            }
            Element::Provider {
                key,
                value,
                children,
                ..
            } => {
                if let Kind::Provider {
                    key: k, value: v, ..
                } = &mut self.inst_mut(i).kind
                {
                    *k = key;
                    *v = value;
                }
                let old_kids = self.take_children(i);
                let kids = self.reconcile_children(i, old_kids, children, pass);
                self.set_children(i, kids);
            }
            Element::Portal { children, .. } => {
                let old_kids = self.take_children(i);
                let kids = self.reconcile_children(i, old_kids, children, pass);
                self.set_children(i, kids);
                let desired = self.desired_of(i);
                self.place_children(i, desired, pass);
            }
            Element::Component {
                key, render, memo, ..
            } => {
                let dirty = self.shared.dirty.borrow().contains(&i) || self.force;
                if let Kind::Component {
                    key: k,
                    render: r,
                    memo: m,
                    ..
                } = &mut self.inst_mut(i).kind
                {
                    *k = key;
                    // `memo`: same props and no state change of its own: nothing to render.
                    if memo.is_some() && *m == memo && !dirty {
                        return;
                    }
                    *r = render;
                    *m = memo;
                }
                self.render_component(i, pass);
            }
        }
    }

    /// One child slot (a component's result, a root's element).
    fn reconcile_single(
        &mut self,
        parent: usize,
        old: Option<usize>,
        el: Element,
        pass: &mut Pass,
    ) -> usize {
        match old {
            Some(o) if self.compatible(o, &el) && self.key_of(o) == el.key() => {
                self.update(o, el, pass);
                o
            }
            Some(o) => {
                self.unmount(o, pass);
                self.mount(el, Some(parent), pass)
            }
            None => self.mount(el, Some(parent), pass),
        }
    }

    /// A list of children: matched by key, or by position when unkeyed.
    fn reconcile_children(
        &mut self,
        parent: usize,
        old: Vec<usize>,
        new: Vec<Element>,
        pass: &mut Pass,
    ) -> Vec<usize> {
        let keyed: HashMap<String, usize> = old
            .iter()
            .enumerate()
            .filter_map(|(j, o)| self.key_of(*o).map(|k| (k.to_owned(), j)))
            .collect();
        // Decide the matches first: what goes is unmounted before what stays updates.
        let mut used = vec![false; old.len()];
        let mut plan: Vec<(Option<usize>, Element)> = Vec::with_capacity(new.len());
        for (i, el) in new.into_iter().enumerate() {
            let cand = match el.key() {
                Some(k) => keyed.get(k).copied(),
                None => (i < old.len() && self.key_of(old[i]).is_none()).then_some(i),
            }
            .filter(|j| !used[*j] && self.compatible(old[*j], &el));
            if let Some(j) = cand {
                used[j] = true;
            }
            plan.push((cand, el));
        }
        for (j, o) in old.iter().enumerate() {
            if !used[j] {
                self.unmount(*o, pass);
            }
        }
        let mut out = Vec::with_capacity(plan.len());
        for (cand, el) in plan {
            match cand {
                Some(j) => {
                    self.update(old[j], el, pass);
                    out.push(old[j]);
                }
                None => out.push(self.mount(el, Some(parent), pass)),
            }
        }
        out
    }

    /// Removes an instance and everything under it. Cleanups are queued parent before child;
    /// the tree nodes themselves are removed by the container's `place_children`.
    fn unmount(&mut self, i: usize, pass: &mut Pass) {
        let Some(inst) = self.arena[i].take() else {
            return;
        };
        match inst.kind {
            Kind::Component { hooks, child, .. } => {
                for slot in hooks {
                    if let Slot::Effect {
                        cleanup: Some(c), ..
                    } = slot
                    {
                        pass.cleanups.push(CleanupItem::Run(c));
                    }
                }
                if let Some(c) = child {
                    self.unmount(c, pass);
                }
            }
            Kind::Host {
                node,
                props,
                children,
                ..
            } => {
                self.by_node.remove(&node);
                self.field_text.remove(&i);
                if let Some(r) = props.node_ref {
                    pass.refs.push((r, None));
                }
                for c in children {
                    self.unmount(c, pass);
                }
            }
            Kind::Text { node, .. } => {
                self.by_node.remove(&node);
            }
            Kind::Fragment { children, .. }
            | Kind::Portal { children, .. }
            | Kind::Provider { children, .. } => {
                for c in children {
                    self.unmount(c, pass);
                }
            }
            Kind::Root { child, .. } => {
                if let Some(c) = child {
                    self.unmount(c, pass);
                }
            }
            Kind::Nothing => {}
        }
    }

    // ---- components ----

    fn render_component(&mut self, id: usize, pass: &mut Pass) {
        pass.rendered.insert(id);
        let (render, mut hooks) = match &mut self.inst_mut(id).kind {
            Kind::Component { render, hooks, .. } => (render.clone(), std::mem::take(hooks)),
            _ => unreachable!("a component"),
        };
        let (el, used, effects) = {
            let lookup = |c: u32| self.find_provider(id, c);
            let mut cx = Ctx {
                slots: &mut hooks,
                idx: 0,
                shared: self.shared.clone(),
                inst: id,
                lookup: &lookup,
                effects: Vec::new(),
            };
            let el = render(&mut cx);
            (el, cx.idx, std::mem::take(&mut cx.effects))
        };
        assert!(
            used == hooks.len(),
            "a component rendered fewer hooks than before ({used} of {})",
            hooks.len()
        );
        let old_child = match &mut self.inst_mut(id).kind {
            Kind::Component {
                hooks: h, child, ..
            } => {
                *h = hooks;
                child.take()
            }
            _ => unreachable!("a component"),
        };
        let child = self.reconcile_single(id, old_child, el, pass);
        if let Kind::Component { child: c, .. } = &mut self.inst_mut(id).kind {
            *c = Some(child);
        }
        // Like React, cleanups of changed effects and new effects both run children before
        // parents, so they are queued after the children rendered.
        for idx in &effects {
            pass.cleanups.push(CleanupItem::Slot(id, *idx));
        }
        for idx in effects {
            pass.effects.push((id, idx));
        }
    }

    // ---- commit ----

    fn commit(&mut self, pass: Pass, tree: &mut UiTree) -> Vec<UiError> {
        self.ops_total += pass.ops.len();
        let errors = tree.apply_ops(&pass.ops);
        for (r, n) in &pass.refs {
            r.set(*n);
        }
        for item in pass.cleanups {
            match item {
                CleanupItem::Run(c) => c(),
                CleanupItem::Slot(i, idx) => {
                    let c = match self.arena[i].as_mut().map(|n| &mut n.kind) {
                        Some(Kind::Component { hooks, .. }) => match &mut hooks[idx] {
                            Slot::Effect { cleanup, .. } => cleanup.take(),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(c) = c {
                        c();
                    }
                }
            }
        }
        for (i, idx) in pass.effects {
            let f = match self.arena[i].as_mut().map(|n| &mut n.kind) {
                Some(Kind::Component { hooks, .. }) => match &mut hooks[idx] {
                    Slot::Effect { pending, .. } => pending.take(),
                    _ => None,
                },
                _ => None,
            };
            if let Some(f) = f {
                let cleanup = f();
                if let Some(Kind::Component { hooks, .. }) =
                    self.arena[i].as_mut().map(|n| &mut n.kind)
                    && let Slot::Effect { cleanup: c, .. } = &mut hooks[idx]
                {
                    *c = cleanup;
                }
            }
        }
        errors
    }
}
