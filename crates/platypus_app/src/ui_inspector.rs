// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The host half of `UiInspector` (`platypus_engine_api::ui_inspector`): once per frame, for a
//! bundle built with `platypus-build --add-ui-inspector`, it finds the node under the pointer,
//! applies the style edits scripts queued, and publishes what the screen UI says about the nodes
//! scripts watch (with their ancestors and children, which an elements tree shows). A bundle
//! without the flag never calls [`enable`], so none of this runs.
//! Design: `docs/research/platypus-ui-inspector.md`.

use std::collections::BTreeSet;

use platypus_core::NodeId;
use platypus_engine_api::ui_inspector as api;
use platypus_ui::{Inspected, UiTree};

use super::{App, Surface};

/// Turns the inspector on for this run.
pub(crate) fn enable() {
    api::enable();
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
            .map(|d| (d.prop.clone(), d.value.clone(), d.applied))
            .collect(),
        rules: i
            .rules
            .iter()
            .map(|r| api::RuleSnapshot {
                sheet: r.sheet,
                selector: r.selector.clone(),
                declarations: r
                    .declarations
                    .iter()
                    .map(|d| api::DeclSnapshot {
                        prop: d.prop.clone(),
                        value: d.value.clone(),
                        important: d.important,
                        applied: d.applied,
                    })
                    .collect(),
            })
            .collect(),
    }
}

impl App {
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
        // The pointer over the screen UI (a world panel has its own tree, not inspected here).
        let hovered = match (self.pointer_px, self.pointer_on) {
            (Some((px, py)), Surface::Screen) => {
                let scale = self.scale();
                self.ui
                    .inspect_pick(px / scale, py / scale)
                    .map_or(0, |n| n.0)
            }
            _ => 0,
        };
        api::set_hovered(hovered);
        let nodes = wanted(&self.ui, &api::watched())
            .into_iter()
            .filter_map(|id| self.ui.inspect(NodeId(id)))
            .map(|i| snapshot(&i))
            .collect();
        api::publish(nodes);
    }
}
