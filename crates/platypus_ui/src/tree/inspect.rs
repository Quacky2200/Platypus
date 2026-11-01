// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What a UI inspector can ask of a tree: the node under a point, one node's boxes, attributes,
//! inline styles and the stylesheet rules that match it (with which declarations win the
//! cascade), and live edits of an inline style. Nothing is recorded while the tree runs: rules
//! are matched again when asked, so a build without an inspector pays nothing.

use cssparser::ToCss;
use selectors::context::SelectorCaches;

use super::{TEXT_NODE, UiError, UiTree};
use crate::cascade::{Target, matched_declarations};
use crate::css::Stylesheet;
use crate::draw::Rect;
use crate::layout::LayoutEngine;
use crate::opcodes::Op;
use platypus_core::{NodeId, Value};

/// One declaration of a matched rule, and whether it decides the property for the node.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectedDeclaration {
    pub prop: String,
    pub value: String,
    pub important: bool,
    /// False when a later rule, an inline style or an `!important` rule overrides it.
    pub applied: bool,
}

/// A stylesheet rule that matches the node.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectedRule {
    /// The stylesheet's id (`add_stylesheet`'s), to tell which sheet it came from.
    pub sheet: u32,
    /// The rule's position in its stylesheet.
    pub rule: usize,
    /// The rule's selector list as written (`.card > text:first-child`).
    pub selector: String,
    pub declarations: Vec<InspectedDeclaration>,
}

/// An inline style declaration, and whether it decides the property.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectedInline {
    pub prop: String,
    pub value: String,
    pub applied: bool,
}

/// Everything the inspector shows for one node.
#[derive(Clone, Debug, PartialEq)]
pub struct Inspected {
    pub id: NodeId,
    pub kind: String,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub element_id: Option<String>,
    pub classes: Vec<String>,
    /// Absolute border box from the last layout.
    pub rect: Option<Rect>,
    /// Absolute padding box (inside the border).
    pub padding_box: Option<Rect>,
    pub inline: Vec<InspectedInline>,
    /// Matching rules in cascade order, lowest priority first.
    pub rules: Vec<InspectedRule>,
}

fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().map(value_text).collect::<Vec<_>>().join(" "),
    }
}

impl<L: LayoutEngine> UiTree<L> {
    /// The topmost element under (`x`, `y`), like `hit_test`. Named for the inspector, which
    /// skips its own overlay by giving it `pointer-events: none`.
    pub fn inspect_pick(&mut self, x: f32, y: f32) -> Option<NodeId> {
        self.hit_test(x, y)
    }

    /// What the inspector shows for `id`, or `None` for a node that does not exist.
    pub fn inspect(&self, id: NodeId) -> Option<Inspected> {
        let node = self.nodes.get(&id)?;
        let mut caches = SelectorCaches::default();
        let sheets: Vec<&Stylesheet> = self.sheets.iter().map(|(_, s)| s).collect();
        let (normal, important) = if node.generated.is_some() || node.kind == TEXT_NODE {
            (Vec::new(), Vec::new())
        } else {
            matched_declarations(
                self,
                &sheets,
                &self.rule_index,
                id,
                Target::Element,
                &mut caches,
            )
        };

        // The cascade's last word per property: !important rules, else inline, else rules.
        let wins_important = |prop: &str, order| {
            important
                .iter()
                .rev()
                .find(|m| m.prop == prop)
                .is_some_and(|m| m.order == order)
        };
        let important_has = |prop: &str| important.iter().any(|m| m.prop == prop);
        let inline_has = |prop: &str| node.inline.iter().any(|(p, _)| p == prop);

        let mut rules: Vec<InspectedRule> = Vec::new();
        for m in normal.iter().chain(important.iter()) {
            let (sheet_index, rule_index, _) = m.order;
            let is_important = important.iter().any(|i| i.order == m.order);
            let applied = if is_important {
                wins_important(&m.prop, m.order)
            } else {
                !important_has(&m.prop)
                    && !inline_has(&m.prop)
                    && normal
                        .iter()
                        .rev()
                        .find(|n| n.prop == m.prop)
                        .is_some_and(|n| n.order == m.order)
            };
            let decl = InspectedDeclaration {
                prop: m.prop.clone(),
                value: value_text(&m.value),
                important: is_important,
                applied,
            };
            let sheet = self.sheets[sheet_index].0;
            match rules
                .iter_mut()
                .find(|r| r.sheet == sheet && r.rule == rule_index)
            {
                Some(r) => r.declarations.push(decl),
                None => rules.push(InspectedRule {
                    sheet,
                    rule: rule_index,
                    selector: sheets[sheet_index].rules[rule_index]
                        .selectors
                        .to_css_string(),
                    declarations: vec![decl],
                }),
            }
        }

        // The latest inline value of a property wins; earlier ones are shadowed.
        let inline = node
            .inline
            .iter()
            .enumerate()
            .map(|(i, (prop, value))| InspectedInline {
                prop: prop.clone(),
                value: value_text(value),
                applied: !important_has(prop)
                    && !node.inline[i + 1..].iter().any(|(p, _)| p == prop),
            })
            .collect();

        Some(Inspected {
            id,
            kind: node.kind.clone(),
            parent: node.parent,
            children: node.children.clone(),
            element_id: node.attrs.id.clone(),
            classes: node.attrs.classes.clone(),
            rect: self.rect(id),
            padding_box: self.padding_box(id),
            inline,
            rules,
        })
    }

    /// Sets (or, with an empty `value`, removes) one inline style declaration of `id`, as a
    /// `style` prop would. The reconciler only diffs the props it renders, so the edit stays
    /// until the component changes that property itself.
    pub fn inspect_set_style(&mut self, id: NodeId, prop: &str, value: &str) -> Vec<UiError> {
        let value = if value.trim().is_empty() {
            Value::Null
        } else {
            crate::cascade::css_value(value)
        };
        self.apply_ops(&[Op::SetStyle {
            id,
            pairs: vec![(prop.to_owned(), value)],
        }])
    }
}
