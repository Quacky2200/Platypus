// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What a UI inspector can ask of a tree: the node under a point, one node's boxes, attributes,
//! inline styles and the stylesheet rules that match it (with which declarations win the
//! cascade, and what `calc()` / `var()` / relative units come to), its layout, the rules of its
//! pseudo-elements, and live edits of an inline style or of its classes. Nothing is recorded
//! while the tree runs: rules are matched again when asked, so a build without an inspector pays
//! nothing.

use cssparser::ToCss;
use selectors::context::SelectorCaches;

use super::{TEXT_NODE, UiError, UiTree};
use crate::cascade::{Matched, Target, matched_declarations};
use crate::css::{PseudoElement, Stylesheet, substitute_vars};
use crate::draw::Rect;
use crate::layout::LayoutEngine;
use crate::opcodes::Op;
use crate::units::{UnitContext, needs_resolution_for, resolve};
use platypus_core::{NodeId, Value};

/// One declaration of a matched rule, and whether it decides the property for the node.
#[derive(Clone, Debug, PartialEq)]
pub struct InspectedDeclaration {
    pub prop: String,
    pub value: String,
    pub important: bool,
    /// False when a later rule, an inline style or an `!important` rule overrides it.
    pub applied: bool,
    /// What a value with `var()`, `calc()` or relative units comes to for this node (`calc(100% -
    /// 20px)` as `380px`); `None` for a plain value.
    pub computed: Option<String>,
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
    /// As [`InspectedDeclaration::computed`].
    pub computed: Option<String>,
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
    /// The layout the node was given, as CSS text by property name (`display`, `flexDirection`, ...).
    pub layout: Vec<(String, String)>,
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

/// `FlexStart` as `flex-start`.
fn kebab(debug: &str) -> String {
    let mut out = String::new();
    for (i, c) in debug.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('-');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// A gap as CSS text (a `calc()` gap reads as 0: the preview only needs the common cases).
fn length_text(l: taffy::LengthPercentage) -> String {
    use taffy::CompactLength;
    let raw = l.into_raw();
    match raw.tag() {
        CompactLength::PERCENT_TAG => format!("{}%", raw.value() * 100.0),
        CompactLength::LENGTH_TAG => format!("{}px", raw.value()),
        _ => "0px".to_owned(),
    }
}

/// The scan for `.name` tokens in a selector's text.
fn class_names_in(selector: &str, out: &mut std::collections::BTreeSet<String>) {
    let chars: Vec<char> = selector.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '.'
            && i + 1 < chars.len()
            && (chars[i + 1].is_alphabetic() || chars[i + 1] == '_' || chars[i + 1] == '-')
        {
            let start = i + 1;
            let mut j = start;
            while j < chars.len()
                && (chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '-')
            {
                j += 1;
            }
            out.insert(chars[start..j].iter().collect());
            i = j;
        } else {
            i += 1;
        }
    }
}

impl<L: LayoutEngine> UiTree<L> {
    /// The topmost element under (`x`, `y`), like `hit_test`. Named for the inspector, which
    /// skips its own overlay by giving it `pointer-events: none`.
    pub fn inspect_pick(&mut self, x: f32, y: f32) -> Option<NodeId> {
        self.hit_test(x, y)
    }

    /// The names of the classes the loaded stylesheets mention (what an "add class" field offers).
    pub fn inspect_class_names(&self) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        for (_, sheet) in &self.sheets {
            for rule in &sheet.rules {
                class_names_in(&rule.selectors.to_css_string(), &mut out);
            }
        }
        out.into_iter().collect()
    }

    /// What `text`, a value of `prop` on `id`, comes to once `var()`, math and relative units are
    /// resolved (percentages against the parent's box); `None` when it is plain or invalid.
    fn computed_value(&self, id: NodeId, prop: &str, text: &str) -> Option<String> {
        if !text.contains("var(") && !needs_resolution_for(prop, text) {
            return None;
        }
        let node = self.nodes.get(&id)?;
        let original = text.trim();
        let mut text = text.to_owned();
        if text.contains("var(") {
            let lookup = |name: &str| node.vars.get(name).cloned();
            text = substitute_vars(&text, &lookup)?;
        }
        if text.contains('%')
            && ["calc(", "min(", "max(", "clamp("]
                .iter()
                .any(|f| text.contains(f))
        {
            let parent = node.parent.and_then(|p| self.content_box(p));
            let horizontal = !matches!(
                prop,
                "height"
                    | "minHeight"
                    | "maxHeight"
                    | "top"
                    | "bottom"
                    | "marginTop"
                    | "marginBottom"
            );
            if let Some(p) = parent {
                text = percent_to_px(&text, if horizontal { p.width } else { p.height });
            }
        }
        let ctx = UnitContext {
            em: self.computed_text(id).font_size,
            rem: self.computed_text(super::ROOT).font_size,
            viewport: self.size,
        };
        let (resolved, _) = resolve(&text, &ctx)?;
        (resolved != original).then_some(resolved)
    }

    /// The matching rules of `id` for `target`, in cascade order, with which declarations win.
    /// `inline_has` says which properties the node's inline styles set (they beat normal rules).
    fn rules_for(
        &self,
        id: NodeId,
        target: Target,
        inline_has: &dyn Fn(&str) -> bool,
    ) -> Vec<InspectedRule> {
        let mut caches = SelectorCaches::default();
        let sheets: Vec<&Stylesheet> = self.sheets.iter().map(|(_, s)| s).collect();
        let (normal, important): (Vec<Matched>, Vec<Matched>) =
            matched_declarations(self, &sheets, &self.rule_index, id, target, &mut caches);
        // The cascade's last word per property: !important rules, else inline, else rules.
        let wins_important = |prop: &str, order| {
            important
                .iter()
                .rev()
                .find(|m| m.prop == prop)
                .is_some_and(|m| m.order == order)
        };
        let important_has = |prop: &str| important.iter().any(|m| m.prop == prop);
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
            let value = value_text(&m.value);
            let decl = InspectedDeclaration {
                prop: m.prop.clone(),
                computed: self.computed_value(id, &m.prop, &value),
                value,
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
        rules
    }

    /// The rules that match `id`'s pseudo-element (`before`, `after`, `marker`, `selection`,
    /// `placeholder`, `backdrop`); empty for another name or a node that does not exist.
    pub fn inspect_pseudo(&self, id: NodeId, pseudo: &str) -> Vec<InspectedRule> {
        let which = match pseudo {
            "before" => PseudoElement::Before,
            "after" => PseudoElement::After,
            "marker" => PseudoElement::Marker,
            "selection" => PseudoElement::Selection,
            "placeholder" => PseudoElement::Placeholder,
            "backdrop" => PseudoElement::Backdrop,
            _ => return Vec::new(),
        };
        match self.nodes.get(&id) {
            Some(n) if n.generated.is_none() && n.kind != TEXT_NODE => {
                self.rules_for(id, Target::Pseudo(which), &|_| false)
            }
            _ => Vec::new(),
        }
    }

    /// The layout the node got, as CSS text: what a flexbox preview draws.
    fn layout_text(&self, id: NodeId) -> Vec<(String, String)> {
        let Some(node) = self.nodes.get(&id) else {
            return Vec::new();
        };
        let s = &node.style;
        let wrap = match s.flex_wrap {
            taffy::FlexWrap::NoWrap => "nowrap".to_owned(),
            other => kebab(&format!("{other:?}")),
        };
        vec![
            ("display".into(), kebab(&format!("{:?}", s.display))),
            ("position".into(), kebab(&format!("{:?}", s.position))),
            (
                "flexDirection".into(),
                kebab(&format!("{:?}", s.flex_direction)),
            ),
            ("flexWrap".into(), wrap),
            (
                "justifyContent".into(),
                kebab(&format!("{:?}", s.justify_content.keyword)),
            ),
            (
                "alignItems".into(),
                kebab(&format!("{:?}", s.align_items.keyword)),
            ),
            (
                "alignContent".into(),
                kebab(&format!("{:?}", s.align_content.keyword)),
            ),
            (
                "alignSelf".into(),
                s.align_self
                    .map_or_else(|| "auto".to_owned(), |a| kebab(&format!("{:?}", a.keyword))),
            ),
            ("columnGap".into(), length_text(s.gap.width)),
            ("rowGap".into(), length_text(s.gap.height)),
            ("flexGrow".into(), s.flex_grow.to_string()),
            ("flexShrink".into(), s.flex_shrink.to_string()),
        ]
    }

    /// What the inspector shows for `id`, or `None` for a node that does not exist.
    pub fn inspect(&self, id: NodeId) -> Option<Inspected> {
        let node = self.nodes.get(&id)?;
        let important_inline = |prop: &str| node.inline.iter().any(|(p, _)| p == prop);
        let rules = if node.generated.is_some() || node.kind == TEXT_NODE {
            Vec::new()
        } else {
            self.rules_for(id, Target::Element, &important_inline)
        };
        let important_has = |prop: &str| {
            rules
                .iter()
                .any(|r| r.declarations.iter().any(|d| d.important && d.prop == prop))
        };

        // The latest inline value of a property wins; earlier ones are shadowed.
        let inline = node
            .inline
            .iter()
            .enumerate()
            .map(|(i, (prop, value))| {
                let value = value_text(value);
                InspectedInline {
                    prop: prop.clone(),
                    computed: self.computed_value(id, prop, &value),
                    value,
                    applied: !important_has(prop)
                        && !node.inline[i + 1..].iter().any(|(p, _)| p == prop),
                }
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
            layout: self.layout_text(id),
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

    /// Adds (`on`) or removes the class `name` on `id`, as a `className` prop would. Like a style
    /// edit it stays until the component renders another `className`.
    pub fn inspect_set_class(&mut self, id: NodeId, name: &str, on: bool) -> Vec<UiError> {
        let Some(attrs) = self.attrs(id) else {
            return Vec::new();
        };
        let mut classes = attrs.classes.clone();
        classes.retain(|c| c != name);
        if on && !name.trim().is_empty() {
            classes.push(name.trim().to_owned());
        }
        self.apply_ops(&[Op::SetAttrs {
            id,
            pairs: vec![("className".to_owned(), Value::String(classes.join(" ")))],
        }])
    }
}

/// `text` with each `N%` as `N/100 × base` pixels.
fn percent_to_px(text: &str, base: f32) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)) {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let n: String = chars[start..i].iter().collect();
            if chars.get(i) == Some(&'%')
                && let Ok(v) = n.parse::<f32>()
            {
                out.push_str(&format!("{}px", v / 100.0 * base));
                i += 1;
            } else {
                out.push_str(&n);
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}
