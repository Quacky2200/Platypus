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
    /// What `calc()`, `var()` or relative units come to (`""` for a plain value).
    pub computed: String,
}

/// An inline style declaration of a node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InlineSnapshot {
    pub prop: String,
    pub value: String,
    pub applied: bool,
    pub computed: String,
}

/// One style property the engine supports, for autocomplete.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StyleEntry {
    pub name: String,
    /// The keywords it takes (`flex`, `column`, ...).
    pub keywords: Vec<String>,
    /// What else it takes: `number`, `length`, `string`, space separated.
    pub kinds: String,
    pub doc: String,
}

/// A rule that matches a node.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RuleSnapshot {
    pub sheet: u32,
    /// The rule's position in its stylesheet (what `highlightRule` takes).
    pub rule: u32,
    pub selector: String,
    /// `""` for the element's own rule, else the pseudo-element it styles (`before`, `after`, ...).
    pub pseudo: String,
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
    pub inline: Vec<InlineSnapshot>,
    pub rules: Vec<RuleSnapshot>,
    /// The layout the node was given (`display`, `flexDirection`, ...), as CSS text.
    pub layout: Vec<(String, String)>,
}

/// What the inspector outlines besides the pointer's node: while a rule or an element row is hovered,
/// the elements it is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Highlight {
    #[default]
    None,
    /// One element.
    Node(u32),
    /// Every element a stylesheet rule matches.
    Rule { sheet: u32, rule: u32 },
}

/// A class added to or removed from a node.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassEdit {
    pub id: u32,
    pub name: String,
    pub on: bool,
}

/// A forced interaction state turned on or off (`hover`, `active`, `focus`, `focus-visible`).
#[derive(Clone, Debug, PartialEq)]
pub struct StateChange {
    pub id: u32,
    pub state: String,
    pub on: bool,
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
    /// Which pseudo-element's rules the selected node publishes (`""`: the element's own).
    pseudo: String,
    /// States forced on, by node.
    forced: HashMap<u32, BTreeSet<String>>,
    state_changes: Vec<StateChange>,
    class_edits: Vec<ClassEdit>,
    /// Nodes scripts only want the boxes of (a widget finding where it is on screen).
    probes: BTreeSet<u32>,
    highlight: Highlight,
    /// The border boxes `[x, y, width, height]` of what `highlight` names.
    highlights: Vec<[f32; 4]>,
    /// The node whose box model (margin, border, padding, content) is drawn whatever the pointer is over
    /// (a hovered `margin` line).
    box_focus: u32,
    styles: Vec<StyleEntry>,
    units: Vec<String>,
    class_names: Vec<String>,
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

/// The style properties and length units the engine supports, once, for autocomplete.
pub fn set_style_table(styles: Vec<StyleEntry>, units: Vec<String>) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.styles = styles;
        s.units = units;
    });
}

/// The class names the loaded stylesheets mention.
pub fn set_class_names(names: Vec<String>) {
    STATE.with(|s| s.borrow_mut().class_names = names);
}

/// Which pseudo-element's rules the selected node should publish (`""`: the element's own).
pub fn pseudo() -> String {
    STATE.with(|s| s.borrow().pseudo.clone())
}

/// The states scripts forced on, by node: the host keeps them applied over the real ones.
pub fn forced() -> Vec<(u32, Vec<String>)> {
    STATE.with(|s| {
        s.borrow()
            .forced
            .iter()
            .map(|(id, set)| (*id, set.iter().cloned().collect()))
            .collect()
    })
}

/// The forced states turned on or off since the last call.
pub fn take_state_changes() -> Vec<StateChange> {
    STATE.with(|s| std::mem::take(&mut s.borrow_mut().state_changes))
}

/// The class edits scripts queued since the last call.
pub fn take_class_edits() -> Vec<ClassEdit> {
    STATE.with(|s| std::mem::take(&mut s.borrow_mut().class_edits))
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

/// What scripts asked to have highlighted.
pub fn highlight() -> Highlight {
    STATE.with(|s| s.borrow().highlight)
}

/// Replaces the boxes of what is highlighted (the host does, each frame).
pub fn publish_highlights(rects: Vec<[f32; 4]>) {
    STATE.with(|s| s.borrow_mut().highlights = rects);
}

/// The nodes scripts probed: the host publishes their boxes (and nothing else) every frame.
pub fn probes() -> Vec<u32> {
    STATE.with(|s| s.borrow().probes.iter().copied().collect())
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

    /// Asks for a node's boxes (`x`, `y`, `width`, `height`), whatever it is: a widget that needs
    /// the pointer's position inside itself probes the element it is made of (`e.target.id` of its
    /// `onPointerEnter`) and reads `x` / `y` from the next frame on. At most 256 nodes are kept.
    pub fn probe(id: f64) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.enabled && id >= 1.0 {
                if s.probes.len() >= 256 {
                    s.probes.clear();
                }
                s.probes.insert(id as u32);
            }
        });
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
            .and_then(|i| node(id, |n| n.inline.get(i).map(|d| d.prop.clone())).flatten())
            .unwrap_or_default()
    }

    pub fn inline_value(id: f64, i: f64) -> String {
        index(i)
            .and_then(|i| node(id, |n| n.inline.get(i).map(|d| d.value.clone())).flatten())
            .unwrap_or_default()
    }

    /// False when a later inline declaration or an `!important` rule overrides it.
    pub fn inline_applied(id: f64, i: f64) -> bool {
        index(i)
            .and_then(|i| node(id, |n| n.inline.get(i).is_some_and(|d| d.applied)))
            .unwrap_or(false)
    }

    /// What an inline value with `calc()`, `var()` or relative units comes to for the node
    /// (`"280px"`), `""` for a plain value.
    pub fn inline_computed(id: f64, i: f64) -> String {
        index(i)
            .and_then(|i| node(id, |n| n.inline.get(i).map(|d| d.computed.clone())).flatten())
            .unwrap_or_default()
    }

    /// As `inlineComputed`, for a declaration of a matching rule.
    pub fn decl_computed(id: f64, rule: f64, i: f64) -> String {
        decl(id, rule, i).map(|d| d.computed).unwrap_or_default()
    }

    /// One part of the layout the node was given, as CSS text: `display`, `position`,
    /// `flexDirection`, `flexWrap`, `justifyContent`, `alignItems`, `alignContent`, `alignSelf`,
    /// `columnGap`, `rowGap`, `flexGrow`, `flexShrink`. `""` for another name.
    pub fn layout(id: f64, name: &str) -> String {
        node(id, |n| {
            n.layout
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        })
        .flatten()
        .unwrap_or_default()
    }

    /// Which pseudo-element's rules `ruleCount` and the rule getters answer for the selected node:
    /// `""` (the element's own), `before`, `after`, `marker`, `selection`, `placeholder` or
    /// `backdrop`. Other nodes keep answering their own.
    pub fn set_pseudo(name: &str) {
        STATE.with(|s| s.borrow_mut().pseudo = name.to_string());
    }

    /// The pseudo-element view `setPseudo` chose.
    pub fn pseudo() -> String {
        super::pseudo()
    }

    /// Forces an interaction state on a node, or lets go of it: `hover`, `active`, `focus` or
    /// `focus-visible`. The host keeps it applied over the real state until it is turned off.
    pub fn force_state(id: f64, state: &str, on: bool) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.enabled || id < 1.0 {
                return;
            }
            let id = id as u32;
            let known = matches!(state, "hover" | "active" | "focus" | "focus-visible");
            if !known {
                return;
            }
            let set = s.forced.entry(id).or_default();
            let changed = if on {
                set.insert(state.to_string())
            } else {
                set.remove(state)
            };
            if set.is_empty() {
                s.forced.remove(&id);
            }
            if changed {
                s.state_changes.push(super::StateChange {
                    id,
                    state: state.to_string(),
                    on,
                });
            }
        });
    }

    /// The states forced on a node, space separated (`"hover active"`).
    pub fn forced_states(id: f64) -> String {
        STATE.with(|s| {
            s.borrow()
                .forced
                .get(&(id.max(0.0) as u32))
                .map(|set| set.iter().cloned().collect::<Vec<_>>().join(" "))
                .unwrap_or_default()
        })
    }

    /// Adds (`on`) or removes a class of a node, like a `className` prop. Dropped when the
    /// inspector is off.
    pub fn set_class(id: f64, name: &str, on: bool) {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if s.enabled && id >= 1.0 && !name.trim().is_empty() {
                s.class_edits.push(super::ClassEdit {
                    id: id as u32,
                    name: name.trim().to_string(),
                    on,
                });
            }
        });
    }

    /// Every style property the engine supports (what autocomplete offers), in the table's order.
    pub fn style_props() -> Vec<String> {
        STATE.with(|s| s.borrow().styles.iter().map(|e| e.name.clone()).collect())
    }

    /// The keywords a property takes (`flex`, `none` for `display`), empty when it has none.
    pub fn style_keywords(prop: &str) -> Vec<String> {
        STATE.with(|s| {
            s.borrow()
                .styles
                .iter()
                .find(|e| e.name == prop)
                .map(|e| e.keywords.clone())
                .unwrap_or_default()
        })
    }

    /// What else a property takes: `number`, `length`, `string`, space separated; `""` for an
    /// unknown property.
    pub fn style_kinds(prop: &str) -> String {
        STATE.with(|s| {
            s.borrow()
                .styles
                .iter()
                .find(|e| e.name == prop)
                .map(|e| e.kinds.clone())
                .unwrap_or_default()
        })
    }

    /// The property's documentation.
    pub fn style_doc(prop: &str) -> String {
        STATE.with(|s| {
            s.borrow()
                .styles
                .iter()
                .find(|e| e.name == prop)
                .map(|e| e.doc.clone())
                .unwrap_or_default()
        })
    }

    /// The units a length takes besides a bare number (`px`, `%`, `em`, ...).
    pub fn style_units() -> Vec<String> {
        STATE.with(|s| s.borrow().units.clone())
    }

    /// The class names the loaded stylesheets mention.
    pub fn class_names() -> Vec<String> {
        STATE.with(|s| s.borrow().class_names.clone())
    }

    /// How many stylesheet rules match the node, lowest priority first.
    pub fn rule_count(id: f64) -> f64 {
        node(id, |n| n.rules.len() as f64).unwrap_or(0.0)
    }

    /// The `i`th matching rule's selector list as written.
    pub fn rule_selector(id: f64, i: f64) -> String {
        rule(id, i).map(|r| r.selector).unwrap_or_default()
    }

    /// `""` when the `i`th matching rule is the element's own, else the pseudo-element it styles
    /// (`before`, `after`, `marker`, `selection`, `placeholder`, `backdrop`).
    pub fn rule_pseudo(id: f64, i: f64) -> String {
        rule(id, i).map(|r| r.pseudo).unwrap_or_default()
    }

    /// The id of the stylesheet the rule came from.
    pub fn rule_sheet(id: f64, i: f64) -> f64 {
        rule(id, i).map_or(0.0, |r| f64::from(r.sheet))
    }

    /// The `i`th matching rule's position in its stylesheet (with `ruleSheet`, what `highlightRule` takes).
    pub fn rule_index(id: f64, i: f64) -> f64 {
        rule(id, i).map_or(0.0, |r| f64::from(r.rule))
    }

    /// Outlines one element until `clearHighlight`.
    pub fn highlight_node(id: f64) {
        STATE.with(|s| s.borrow_mut().highlight = super::Highlight::Node(id.max(0.0) as u32));
    }

    /// Outlines every element a rule matches (a rule of a pseudo-element: the elements it belongs to)
    /// until `clearHighlight`.
    pub fn highlight_rule(sheet: f64, rule: f64) {
        STATE.with(|s| {
            s.borrow_mut().highlight = super::Highlight::Rule {
                sheet: sheet.max(0.0) as u32,
                rule: rule.max(0.0) as u32,
            }
        });
    }

    pub fn clear_highlight() {
        STATE.with(|s| s.borrow_mut().highlight = super::Highlight::None);
    }

    /// How many boxes the highlight outlines.
    pub fn highlight_count() -> f64 {
        STATE.with(|s| s.borrow().highlights.len() as f64)
    }

    /// The `i`th highlighted box's left, top, width or height (`which` 0..3), window pixels.
    pub fn highlight_rect(i: f64, which: f64) -> f64 {
        STATE.with(|s| {
            index(i)
                .zip(index(which))
                .and_then(|(i, w)| {
                    s.borrow()
                        .highlights
                        .get(i)
                        .and_then(|r| r.get(w))
                        .map(|v| f64::from(*v))
                })
                .unwrap_or(0.0)
        })
    }

    /// Draws the box model of a node (0: none) whatever the pointer is over: what a hovered `margin` or
    /// `padding` line asks for.
    pub fn show_box(id: f64) {
        STATE.with(|s| s.borrow_mut().box_focus = id.max(0.0) as u32);
    }

    /// The node `showBox` named, 0 for none.
    pub fn box_focus() -> f64 {
        STATE.with(|s| f64::from(s.borrow().box_focus))
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
            inline: vec![InlineSnapshot {
                prop: "width".into(),
                value: "100".into(),
                applied: true,
                computed: String::new(),
            }],
            rules: vec![RuleSnapshot {
                sheet: 3,
                rule: 5,
                selector: ".card".into(),
                pseudo: "before".into(),
                declarations: vec![DeclSnapshot {
                    prop: "height".into(),
                    value: "calc(100% - 20px)".into(),
                    important: true,
                    applied: false,
                    computed: "280px".into(),
                }],
            }],
            layout: vec![
                ("display".into(), "flex".into()),
                ("flexDirection".into(), "column".into()),
            ],
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
        assert_eq!(ui_inspector_api::rule_pseudo(7.0, 0.0), "before");
        assert_eq!(ui_inspector_api::rule_pseudo(7.0, 3.0), "");
        assert_eq!(
            ui_inspector_api::decl_value(7.0, 0.0, 0.0),
            "calc(100% - 20px)"
        );
        assert_eq!(ui_inspector_api::decl_computed(7.0, 0.0, 0.0), "280px");
        assert_eq!(ui_inspector_api::inline_computed(7.0, 0.0), "");
        assert_eq!(ui_inspector_api::layout(7.0, "flexDirection"), "column");
        assert_eq!(ui_inspector_api::layout(7.0, "nope"), "");
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

    // M3-INS-16
    #[test]
    fn states_classes_and_the_pseudo_view_reach_the_host() {
        reset();
        // Off: nothing is queued or forced.
        ui_inspector_api::force_state(7.0, "hover", true);
        ui_inspector_api::set_class(7.0, "hot", true);
        assert!(
            forced().is_empty() && take_state_changes().is_empty() && take_class_edits().is_empty()
        );

        enable();
        ui_inspector_api::force_state(7.0, "hover", true);
        ui_inspector_api::force_state(7.0, "focus-visible", true);
        ui_inspector_api::force_state(7.0, "hover", true); // again: no second change
        ui_inspector_api::force_state(7.0, "bogus", true);
        assert_eq!(ui_inspector_api::forced_states(7.0), "focus-visible hover");
        assert_eq!(
            forced(),
            [(7, vec!["focus-visible".to_string(), "hover".to_string()])]
        );
        ui_inspector_api::force_state(7.0, "hover", false);
        let changes = take_state_changes();
        assert_eq!(
            changes
                .iter()
                .map(|c| (c.state.as_str(), c.on))
                .collect::<Vec<_>>(),
            [("hover", true), ("focus-visible", true), ("hover", false)]
        );
        ui_inspector_api::force_state(7.0, "focus-visible", false);
        assert!(forced().is_empty(), "nothing left forced");

        ui_inspector_api::set_class(7.0, " hot ", true);
        ui_inspector_api::set_class(7.0, "", true);
        assert_eq!(
            take_class_edits(),
            [ClassEdit {
                id: 7,
                name: "hot".into(),
                on: true
            }]
        );

        ui_inspector_api::set_pseudo("before");
        assert_eq!(ui_inspector_api::pseudo(), "before");
        assert_eq!(pseudo(), "before");

        ui_inspector_api::probe(9.0);
        ui_inspector_api::probe(4.0);
        ui_inspector_api::probe(0.0);
        assert_eq!(probes(), [4, 9]);
        reset();
        ui_inspector_api::probe(9.0);
        assert!(probes().is_empty(), "off: nothing is probed");
    }

    // M3-INS-21
    #[test]
    fn highlights_and_the_box_focus_reach_the_host_and_their_boxes_come_back() {
        reset();
        enable();
        publish(vec![NodeSnapshot {
            id: 7,
            rules: vec![RuleSnapshot {
                sheet: 3,
                rule: 5,
                selector: ".card".into(),
                ..Default::default()
            }],
            ..Default::default()
        }]);
        assert_eq!(ui_inspector_api::rule_index(7.0, 0.0), 5.0);
        assert_eq!(highlight(), Highlight::None);
        ui_inspector_api::highlight_rule(
            ui_inspector_api::rule_sheet(7.0, 0.0),
            ui_inspector_api::rule_index(7.0, 0.0),
        );
        assert_eq!(highlight(), Highlight::Rule { sheet: 3, rule: 5 });
        ui_inspector_api::highlight_node(9.0);
        assert_eq!(highlight(), Highlight::Node(9));
        publish_highlights(vec![[10.0, 20.0, 30.0, 40.0], [1.0, 2.0, 3.0, 4.0]]);
        assert_eq!(ui_inspector_api::highlight_count(), 2.0);
        assert_eq!(ui_inspector_api::highlight_rect(0.0, 2.0), 30.0);
        assert_eq!(ui_inspector_api::highlight_rect(1.0, 1.0), 2.0);
        assert_eq!(
            ui_inspector_api::highlight_rect(2.0, 0.0),
            0.0,
            "past the end"
        );
        ui_inspector_api::clear_highlight();
        assert_eq!(highlight(), Highlight::None);
        ui_inspector_api::show_box(7.0);
        assert_eq!(ui_inspector_api::box_focus(), 7.0);
        ui_inspector_api::show_box(0.0);
        assert_eq!(ui_inspector_api::box_focus(), 0.0);
    }

    // M3-INS-17
    #[test]
    fn the_style_table_feeds_autocomplete() {
        reset();
        set_style_table(
            vec![
                StyleEntry {
                    name: "display".into(),
                    keywords: vec!["flex".into(), "none".into()],
                    kinds: String::new(),
                    doc: "How the box lays out its children.".into(),
                },
                StyleEntry {
                    name: "width".into(),
                    keywords: vec!["auto".into()],
                    kinds: "number length string".into(),
                    doc: String::new(),
                },
            ],
            vec!["px".into(), "%".into()],
        );
        set_class_names(vec!["card".into(), "hot".into()]);
        assert_eq!(ui_inspector_api::style_props(), ["display", "width"]);
        assert_eq!(
            ui_inspector_api::style_keywords("display"),
            ["flex", "none"]
        );
        assert!(ui_inspector_api::style_keywords("nope").is_empty());
        assert_eq!(
            ui_inspector_api::style_kinds("width"),
            "number length string"
        );
        assert_eq!(
            ui_inspector_api::style_doc("display"),
            "How the box lays out its children."
        );
        assert_eq!(ui_inspector_api::style_units(), ["px", "%"]);
        assert_eq!(ui_inspector_api::class_names(), ["card", "hot"]);
    }
}
