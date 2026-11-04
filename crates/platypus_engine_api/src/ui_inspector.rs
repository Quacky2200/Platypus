// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `UiInspector`: what a UI inspector written in TSX (the game's overlay, the editor's panel)
//! asks of the screen UI. The host does the looking: while a build made with
//! `platypus-build --add-ui-inspector` runs it calls [`enable`], then each frame sets the node
//! under the pointer ([`set_hovered`]), asks which nodes scripts watch ([`watched`]) and publishes
//! what the tree says about them ([`publish`]). Scripts read that snapshot with scalar getters
//! (a node's boxes, classes, inline styles and matching rules) and queue style edits the host
//! applies ([`take_requests`]). Without [`enable`] every getter answers "nothing" and edits are
//! dropped, so a game that was built without the inspector pays for none of it.
//! Design: `docs/research/platypus-ui-inspector.md`.

use platypus_api::platypus_api;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

/// One declaration of a rule that matches a node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeclSnapshot {
    pub prop: String,
    pub value: String,
    pub important: bool,
    /// False when the cascade overrides it.
    pub applied: bool,
}

/// A rule that matches a node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuleSnapshot {
    pub sheet: u32,
    pub selector: String,
    pub declarations: Vec<DeclSnapshot>,
}

/// What the host publishes for one node (the fields of `platypus_ui::Inspected`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeSnapshot {
    pub id: u32,
    pub kind: String,
    /// 0 for the root.
    pub parent: u32,
    pub children: Vec<u32>,
    pub element_id: String,
    pub classes: Vec<String>,
    /// Border box `[x, y, width, height]`, absolute logical pixels.
    pub rect: [f32; 4],
    /// Padding box, same layout.
    pub padding: [f32; 4],
    /// `(prop, value, applied)`.
    pub inline: Vec<(String, String, bool)>,
    pub rules: Vec<RuleSnapshot>,
}

/// A style edit a script made.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleEdit {
    pub id: u32,
    pub prop: String,
    /// Empty removes the inline declaration.
    pub value: String,
}

#[derive(Default)]
struct State {
    enabled: bool,
    hovered: u32,
    selected: u32,
    expanded: BTreeSet<u32>,
    nodes: HashMap<u32, NodeSnapshot>,
    edits: Vec<StyleEdit>,
    /// Pick mode: the host swallows a press on the screen UI and reports the node under it.
    picking: bool,
    /// The node a swallowed press hit (0: none yet).
    clicked: u32,
    /// A press was swallowed, so its release is too.
    swallowed: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Turns the inspector on (the host does, for a bundle with `uiInspector`).
pub fn enable() {
    STATE.with(|s| s.borrow_mut().enabled = true);
}

pub fn enabled() -> bool {
    STATE.with(|s| s.borrow().enabled)
}

/// Forgets everything, and turns it off (a test, or a reload).
pub fn reset() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}

/// The node under the pointer (0: none).
pub fn set_hovered(id: u32) {
    STATE.with(|s| s.borrow_mut().hovered = id);
}

/// A mouse press on the screen UI, `hit` the node under it. In pick mode the host does not send
/// the press to the game: it is kept for `takeClick` and its release is swallowed too. Returns
/// whether the press was taken.
pub fn press(hit: u32) -> bool {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let take = s.enabled && s.picking;
        if take {
            s.clicked = hit;
            s.swallowed = true;
        }
        take
    })
}

/// A mouse release; true when its press was swallowed by [`press`] (the host then drops it).
pub fn release() -> bool {
    STATE.with(|s| std::mem::take(&mut s.borrow_mut().swallowed))
}

/// The nodes scripts want data for: the hovered and the selected node and the expanded ones. The
/// host adds their ancestors and children, which an elements tree shows.
pub fn watched() -> Vec<u32> {
    STATE.with(|s| {
        let s = s.borrow();
        let mut v: Vec<u32> = [s.hovered, s.selected]
            .into_iter()
            .chain(s.expanded.iter().copied())
            .filter(|&id| id != 0)
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    })
}

/// Replaces the snapshot scripts read.
pub fn publish(nodes: Vec<NodeSnapshot>) {
    STATE.with(|s| {
        s.borrow_mut().nodes = nodes.into_iter().map(|n| (n.id, n)).collect();
    });
}

/// The edits scripts queued since the last call.
pub fn take_requests() -> Vec<StyleEdit> {
    STATE.with(|s| std::mem::take(&mut s.borrow_mut().edits))
}

fn node<R>(id: f64, read: impl FnOnce(&NodeSnapshot) -> R) -> Option<R> {
    if !(0.0..=f64::from(u32::MAX)).contains(&id) {
        return None;
    }
    STATE.with(|s| s.borrow().nodes.get(&(id as u32)).map(read))
}

fn index(i: f64) -> Option<usize> {
    (i >= 0.0 && i.is_finite()).then_some(i as usize)
}

fn rule(id: f64, i: f64) -> Option<RuleSnapshot> {
    let i = index(i)?;
    node(id, |n| n.rules.get(i).cloned()).flatten()
}

fn decl(id: f64, i: f64, j: f64) -> Option<DeclSnapshot> {
    let j = index(j)?;
    rule(id, i)?.declarations.get(j).cloned()
}

/// Looks at the screen UI of a game built with `platypus-build --add-ui-inspector`: which node
/// the pointer is over, a node's boxes, classes, inline styles and the stylesheet rules that match
/// it (and which of their declarations win), and live style edits. Node ids are the UI tree's;
/// an unknown id (or an inspector that is off) answers 0, `""` or `false`.
#[platypus_api(namespace = "UiInspector")]
pub mod ui_inspector_api {
    use super::{STATE, StyleEdit, decl, index, node, rule};

    /// Whether the host runs the inspector (the build had `--add-ui-inspector`).
    pub fn enabled() -> bool {
        super::enabled()
    }

    /// The node under the pointer, 0 for none.
    pub fn hovered() -> f64 {
        STATE.with(|s| f64::from(s.borrow().hovered))
    }

    /// The node the inspector shows in detail, 0 for none.
    pub fn selected() -> f64 {
        STATE.with(|s| f64::from(s.borrow().selected))
    }

    /// Selects a node (0 clears it); the host publishes its data from the next frame.
    pub fn select(id: f64) {
        STATE.with(|s| s.borrow_mut().selected = id.max(0.0) as u32);
    }

    /// Pick mode: while on, a mouse press on the screen UI does not reach the game; the node under
    /// it comes back from `takeClick`. Ignored when the inspector is off.
    pub fn pick(on: bool) {
        STATE.with(|s| s.borrow_mut().picking = on);
    }

    /// The node the last swallowed press hit (0 when there was none since the last call).
    pub fn take_click() -> f64 {
        STATE.with(|s| f64::from(std::mem::take(&mut s.borrow_mut().clicked)))
    }

    /// Asks for the children of a node too (an open row of the elements tree).
    pub fn expand(id: f64, open: bool) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let id = id.max(0.0) as u32;
            if open {
                s.expanded.insert(id);
            } else {
                s.expanded.remove(&id);
            }
        });
    }

    /// The element type (`box`, `text`, ...), `""` when the node is not published.
    pub fn kind(id: f64) -> String {
        node(id, |n| n.kind.clone()).unwrap_or_default()
    }

    /// The parent's id, 0 for the root or an unknown node.
    pub fn parent(id: f64) -> f64 {
        node(id, |n| f64::from(n.parent)).unwrap_or(0.0)
    }

    pub fn child_count(id: f64) -> f64 {
        node(id, |n| n.children.len() as f64).unwrap_or(0.0)
    }

    /// The `i`th child's id, 0 when there is none.
    pub fn child_at(id: f64, i: f64) -> f64 {
        index(i)
            .and_then(|i| node(id, |n| n.children.get(i).map(|&c| f64::from(c))).flatten())
            .unwrap_or(0.0)
    }

    /// The `id` attribute, `""` when none.
    pub fn element_id(id: f64) -> String {
        node(id, |n| n.element_id.clone()).unwrap_or_default()
    }

    /// The classes, space separated.
    pub fn class_name(id: f64) -> String {
        node(id, |n| n.classes.join(" ")).unwrap_or_default()
    }

    /// The border box's left edge, absolute logical pixels.
    pub fn x(id: f64) -> f64 {
        node(id, |n| f64::from(n.rect[0])).unwrap_or(0.0)
    }

    pub fn y(id: f64) -> f64 {
        node(id, |n| f64::from(n.rect[1])).unwrap_or(0.0)
    }

    pub fn width(id: f64) -> f64 {
        node(id, |n| f64::from(n.rect[2])).unwrap_or(0.0)
    }

    pub fn height(id: f64) -> f64 {
        node(id, |n| f64::from(n.rect[3])).unwrap_or(0.0)
    }

    /// The padding box's left edge, top edge, width or height (`which` 0..3).
    pub fn padding_box(id: f64, which: f64) -> f64 {
        index(which)
            .and_then(|w| node(id, |n| n.padding.get(w).map(|&v| f64::from(v))).flatten())
            .unwrap_or(0.0)
    }

    pub fn inline_count(id: f64) -> f64 {
        node(id, |n| n.inline.len() as f64).unwrap_or(0.0)
    }

    pub fn inline_prop(id: f64, i: f64) -> String {
        index(i)
            .and_then(|i| node(id, |n| n.inline.get(i).map(|d| d.0.clone())).flatten())
            .unwrap_or_default()
    }

    pub fn inline_value(id: f64, i: f64) -> String {
        index(i)
            .and_then(|i| node(id, |n| n.inline.get(i).map(|d| d.1.clone())).flatten())
            .unwrap_or_default()
    }

    /// False when a later inline declaration or an `!important` rule overrides it.
    pub fn inline_applied(id: f64, i: f64) -> bool {
        index(i)
            .and_then(|i| node(id, |n| n.inline.get(i).is_some_and(|d| d.2)))
            .unwrap_or(false)
    }

    /// How many stylesheet rules match the node, lowest priority first.
    pub fn rule_count(id: f64) -> f64 {
        node(id, |n| n.rules.len() as f64).unwrap_or(0.0)
    }

    /// The `i`th matching rule's selector list as written.
    pub fn rule_selector(id: f64, i: f64) -> String {
        rule(id, i).map(|r| r.selector).unwrap_or_default()
    }

    /// The id of the stylesheet the rule came from.
    pub fn rule_sheet(id: f64, i: f64) -> f64 {
        rule(id, i).map_or(0.0, |r| f64::from(r.sheet))
    }

    pub fn decl_count(id: f64, rule: f64) -> f64 {
        super::rule(id, rule).map_or(0.0, |r| r.declarations.len() as f64)
    }

    pub fn decl_prop(id: f64, rule: f64, i: f64) -> String {
        decl(id, rule, i).map(|d| d.prop).unwrap_or_default()
    }

    pub fn decl_value(id: f64, rule: f64, i: f64) -> String {
        decl(id, rule, i).map(|d| d.value).unwrap_or_default()
    }

    pub fn decl_important(id: f64, rule: f64, i: f64) -> bool {
        decl(id, rule, i).is_some_and(|d| d.important)
    }

    /// False when a later rule, an inline style or an `!important` rule overrides it.
    pub fn decl_applied(id: f64, rule: f64, i: f64) -> bool {
        decl(id, rule, i).is_some_and(|d| d.applied)
    }

    /// Sets (or, with `""`, removes) one inline style declaration, like a `style` prop. Dropped
    /// when the inspector is off.
    pub fn set_style(id: f64, prop: &str, value: &str) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.enabled && id >= 1.0 {
                s.edits.push(StyleEdit {
                    id: id as u32,
                    prop: prop.to_string(),
                    value: value.to_string(),
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> NodeSnapshot {
        NodeSnapshot {
            id: 7,
            kind: "box".into(),
            parent: 1,
            children: vec![8, 9],
            element_id: "main".into(),
            classes: vec!["card".into(), "hot".into()],
            rect: [10.0, 20.0, 100.0, 40.0],
            padding: [11.0, 21.0, 98.0, 38.0],
            inline: vec![("width".into(), "100".into(), true)],
            rules: vec![RuleSnapshot {
                sheet: 3,
                selector: ".card".into(),
                declarations: vec![DeclSnapshot {
                    prop: "height".into(),
                    value: "40px".into(),
                    important: true,
                    applied: false,
                }],
            }],
        }
    }

    // M3-INS-05
    #[test]
    fn scripts_read_the_published_snapshot_and_unknown_nodes_answer_nothing() {
        reset();
        enable();
        publish(vec![card()]);
        assert_eq!(ui_inspector_api::kind(7.0), "box");
        assert_eq!(ui_inspector_api::class_name(7.0), "card hot");
        assert_eq!(ui_inspector_api::child_at(7.0, 1.0), 9.0);
        assert_eq!(ui_inspector_api::child_at(7.0, 2.0), 0.0);
        assert_eq!(ui_inspector_api::width(7.0), 100.0);
        assert_eq!(ui_inspector_api::padding_box(7.0, 1.0), 21.0);
        assert!(ui_inspector_api::inline_applied(7.0, 0.0));
        assert_eq!(ui_inspector_api::rule_selector(7.0, 0.0), ".card");
        assert_eq!(ui_inspector_api::rule_sheet(7.0, 0.0), 3.0);
        assert_eq!(ui_inspector_api::decl_value(7.0, 0.0, 0.0), "40px");
        assert!(ui_inspector_api::decl_important(7.0, 0.0, 0.0));
        assert!(!ui_inspector_api::decl_applied(7.0, 0.0, 0.0));
        assert_eq!(ui_inspector_api::kind(99.0), "");
        assert_eq!(ui_inspector_api::rule_count(-1.0), 0.0);
        assert_eq!(ui_inspector_api::decl_prop(7.0, 4.0, 0.0), "");
        reset();
        assert!(!enabled());
    }

    // M3-INS-06
    #[test]
    fn the_host_learns_what_to_publish_and_gets_the_edits() {
        reset();
        enable();
        set_hovered(5);
        ui_inspector_api::select(7.0);
        ui_inspector_api::expand(2.0, true);
        ui_inspector_api::expand(3.0, true);
        ui_inspector_api::expand(3.0, false);
        assert_eq!(watched(), [2, 5, 7]);
        assert_eq!(ui_inspector_api::hovered(), 5.0);
        ui_inspector_api::set_style(7.0, "width", "200px");
        ui_inspector_api::set_style(0.0, "width", "1px");
        assert_eq!(
            take_requests(),
            [StyleEdit {
                id: 7,
                prop: "width".into(),
                value: "200px".into()
            }]
        );
        assert!(take_requests().is_empty());
        // Off, edits are dropped.
        reset();
        ui_inspector_api::set_style(7.0, "width", "200px");
        assert!(take_requests().is_empty());
    }

    // M3-INS-09
    #[test]
    fn pick_mode_swallows_a_press_and_its_release_and_reports_the_node() {
        reset();
        enable();
        // Not picking: the game gets the press.
        assert!(!press(7));
        assert!(!release());
        ui_inspector_api::pick(true);
        assert!(press(7));
        assert_eq!(ui_inspector_api::take_click(), 7.0);
        assert_eq!(ui_inspector_api::take_click(), 0.0, "taken once");
        assert!(release(), "the release goes with it");
        assert!(!release());
        // Leaving pick mode (the script does after a click) lets presses through again.
        ui_inspector_api::pick(false);
        assert!(!press(8));
        // Off, nothing is swallowed even if a script asked.
        reset();
        ui_inspector_api::pick(true);
        assert!(!press(7));
    }
}
