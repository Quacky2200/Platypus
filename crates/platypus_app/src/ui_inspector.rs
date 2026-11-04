// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The host half of `UiInspector` (`platypus_engine_api::ui_inspector`): once per frame, for a
//! bundle built with `platypus-build --add-ui-inspector`, it finds the node under the pointer,
//! applies the style and class edits scripts queued, keeps the states scripts forced
//! (`:hover` ...) applied, and publishes what the screen UI says about the nodes scripts watch
//! (with their ancestors and children, which an elements tree shows). A bundle without the flag
//! never calls [`enable`], so none of this runs.
//! Design: `docs/research/platypus-ui-inspector.md`.

use std::cell::Cell;
use std::collections::BTreeSet;

use platypus_core::NodeId;
use platypus_engine_api::ui_inspector as api;
use platypus_ui::tables::style::{LENGTH_UNITS, Part, STYLE_PROPS};
use platypus_ui::{ElementState, Inspected, UiTree};

use super::{App, Surface};

/// How often the class names the stylesheets mention are read again, in published frames.
const CLASS_NAMES_EVERY: u32 = 60;
/// The most boxes a rule's highlight draws (a rule on every list row stays readable and cheap).
const MAX_HIGHLIGHTS: usize = 200;

thread_local! {
    static FRAMES: Cell<u32> = const { Cell::new(0) };
}

/// Turns the inspector on for this run, and tells scripts what style properties exist.
pub(crate) fn enable() {
    api::enable();
    let styles = STYLE_PROPS
        .iter()
        .map(|p| {
            let mut kinds = Vec::new();
            if p.accepts_number() {
                kinds.push("number");
            }
            if p.ty.iter().any(|t| matches!(t, Part::Length)) {
                kinds.push("length");
            }
            if p.accepts_string() {
                kinds.push("string");
            }
            api::StyleEntry {
                name: p.name.to_owned(),
                keywords: p.keywords().into_iter().map(str::to_owned).collect(),
                kinds: kinds.join(" "),
                doc: p.doc.to_owned(),
            }
        })
        .collect();
    api::set_style_table(
        styles,
        LENGTH_UNITS.iter().map(|u| (*u).to_owned()).collect(),
    );
}

/// The ids to publish for `watched`: each one, its ancestors up to the root, and the children of
/// all of those.
fn wanted(ui: &UiTree, watched: &[u32]) -> BTreeSet<u32> {
    let mut spine = BTreeSet::new();
    for &id in watched {
        let mut at = Some(NodeId(id));
        while let Some(n) = at.filter(|n| spine.insert(n.0)) {
            at = ui.parent(n);
        }
    }
    let mut out = spine.clone();
    for &id in &spine {
        out.extend(ui.children(NodeId(id)).iter().map(|c| c.0));
    }
    out
}

fn rule_snapshots(rules: &[platypus_ui::InspectedRule]) -> Vec<api::RuleSnapshot> {
    rules
        .iter()
        .map(|r| api::RuleSnapshot {
            sheet: r.sheet,
            rule: r.rule as u32,
            selector: r.selector.clone(),
            pseudo: r.pseudo.clone(),
            declarations: r
                .declarations
                .iter()
                .map(|d| api::DeclSnapshot {
                    prop: d.prop.clone(),
                    value: d.value.clone(),
                    important: d.important,
                    applied: d.applied,
                    computed: d.computed.clone().unwrap_or_default(),
                })
                .collect(),
        })
        .collect()
}

/// What scripts read about a node.
fn snapshot(i: &Inspected) -> api::NodeSnapshot {
    let rect = |r: Option<platypus_ui::Rect>| r.map_or([0.0; 4], |r| [r.x, r.y, r.width, r.height]);
    api::NodeSnapshot {
        id: i.id.0,
        kind: i.kind.clone(),
        parent: i.parent.map_or(0, |p| p.0),
        children: i.children.iter().map(|c| c.0).collect(),
        element_id: i.element_id.clone().unwrap_or_default(),
        classes: i.classes.clone(),
        rect: rect(i.rect),
        padding: rect(i.padding_box),
        inline: i
            .inline
            .iter()
            .map(|d| api::InlineSnapshot {
                prop: d.prop.clone(),
                value: d.value.clone(),
                applied: d.applied,
                computed: d.computed.clone().unwrap_or_default(),
            })
            .collect(),
        rules: rule_snapshots(&i.rules),
        layout: i.layout.clone(),
    }
}

/// `state` as a field of [`ElementState`].
fn state_field<'a>(s: &'a mut ElementState, state: &str) -> Option<&'a mut bool> {
    match state {
        "hover" => Some(&mut s.hover),
        "active" => Some(&mut s.active),
        "focus" => Some(&mut s.focus),
        "focus-visible" => Some(&mut s.focus_visible),
        _ => None,
    }
}

/// Sets `state` on `id`, and for `hover` and `active` on its ancestors too, as a pointer over
/// the node would (and as browsers' devtools do when they force it).
fn set_state(ui: &mut UiTree, id: NodeId, state: &str, on: bool) {
    let mut at = Some(id);
    while let Some(n) = at {
        let mut s = ui.state(n);
        if let Some(f) = state_field(&mut s, state) {
            *f = on;
        }
        ui.set_state(n, s);
        at = if matches!(state, "hover" | "active") {
            ui.parent(n)
        } else {
            None
        };
    }
}

/// Whether `id` is part of the inspector's own UI (it or an ancestor has a `uii` class: the overlay's and
/// the editor panel's roots do), which is neither picked nor outlined.
fn in_inspector(ui: &UiTree, id: NodeId) -> bool {
    let mut at = Some(id);
    while let Some(n) = at {
        if ui
            .attrs(n)
            .is_some_and(|a| a.classes.iter().any(|c| c == "uii"))
        {
            return true;
        }
        at = ui.parent(n);
    }
    false
}

impl App {
    /// A mouse press: in pick mode the inspector takes it (and, with it, the release) instead of
    /// the game, and remembers the node under the pointer. True when it was taken.
    pub(super) fn inspector_takes_press(&mut self) -> bool {
        if !api::enabled() || self.pointer_on != Surface::Screen {
            return false;
        }
        let Some((px, py)) = self.pointer_px else {
            return false;
        };
        let scale = self.scale();
        let hit = self.ui.inspect_pick(px / scale, py / scale);
        // The inspector's own buttons and card take presses as usual, and are never picked.
        if hit.is_some_and(|n| in_inspector(&self.ui, n)) {
            return false;
        }
        api::press(hit.map_or(0, |n| n.0))
    }

    /// Called before drawing each frame of a compiled game (`sync_native_render`).
    pub(super) fn sync_ui_inspector(&mut self) {
        if !api::enabled() {
            return;
        }
        for edit in api::take_requests() {
            for e in self
                .ui
                .inspect_set_style(NodeId(edit.id), &edit.prop, &edit.value)
            {
                tracing::warn!("UiInspector.setStyle({}, {}): {e}", edit.id, edit.prop);
            }
        }
        for edit in api::take_class_edits() {
            for e in self
                .ui
                .inspect_set_class(NodeId(edit.id), &edit.name, edit.on)
            {
                tracing::warn!("UiInspector.setClass({}, {}): {e}", edit.id, edit.name);
            }
        }
        // A state let go of is cleared once; the ones still forced are put back every frame, since
        // the pointer's own moves rewrite them.
        for change in api::take_state_changes().into_iter().filter(|c| !c.on) {
            set_state(&mut self.ui, NodeId(change.id), &change.state, false);
        }
        for (id, states) in api::forced() {
            for state in states {
                set_state(&mut self.ui, NodeId(id), &state, true);
            }
        }
        // The pointer over the screen UI (a world panel has its own tree, not inspected here).
        let hovered = match (self.pointer_px, self.pointer_on) {
            (Some((px, py)), Surface::Screen) => {
                let scale = self.scale();
                self.ui
                    .inspect_pick(px / scale, py / scale)
                    .filter(|n| !in_inspector(&self.ui, *n))
                    .map_or(0, |n| n.0)
            }
            _ => 0,
        };
        api::set_hovered(hovered);
        let frames = FRAMES.with(|f| {
            f.set(f.get().wrapping_add(1));
            f.get()
        });
        if frames % CLASS_NAMES_EVERY == 1 {
            api::set_class_names(self.ui.inspect_class_names());
        }
        // The selected node shows a pseudo-element's rules when scripts ask for that view.
        let selected = api::ui_inspector_api::selected() as u32;
        let pseudo = api::pseudo();
        // Only the hovered and the selected node need their styles; the rest of what is published
        // (the tree around them, the probed widgets) needs names and boxes.
        let mut ids = wanted(&self.ui, &api::watched());
        ids.extend(api::probes());
        let nodes = ids
            .into_iter()
            .filter_map(|id| {
                let full = id == selected || id == hovered;
                let inspected = if full {
                    self.ui.inspect(NodeId(id))?
                } else {
                    self.ui.inspect_brief(NodeId(id))?
                };
                let mut snap = snapshot(&inspected);
                if id == selected && !pseudo.is_empty() {
                    snap.rules = rule_snapshots(&self.ui.inspect_pseudo(NodeId(id), &pseudo));
                    snap.inline.clear();
                }
                Some(snap)
            })
            .collect();
        api::publish(nodes);
        // The boxes of what a hovered rule or row is about.
        let marked = match api::highlight() {
            api::Highlight::None => Vec::new(),
            api::Highlight::Node(id) => vec![NodeId(id)],
            api::Highlight::Rule { sheet, rule } => {
                let mut ids = self.ui.inspect_rule_matches(sheet, rule as usize);
                ids.truncate(MAX_HIGHLIGHTS);
                ids
            }
        };
        api::publish_highlights(
            marked
                .into_iter()
                .filter_map(|id| self.ui.rect(id))
                .map(|r| [r.x, r.y, r.width, r.height])
                .collect(),
        );
    }
}
