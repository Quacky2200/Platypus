// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Selector matching against the UI tree and the CSS cascade.

use std::fmt;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use platypus_core::{NodeId, Value};
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::context::{
    MatchingContext, MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, QuirksMode,
    SelectorCaches,
};
use selectors::matching::{ElementSelectorFlags, matches_selector};
use selectors::parser::{Combinator, Component, Selector};
use selectors::{Element, OpaqueElement};

use crate::attrs::Attrs;
use crate::css::{CssString, PlatypusSelectors, PseudoClass, PseudoElement, Stylesheet};
use crate::layout::LayoutEngine;
use crate::tree::{ROOT, TEXT_NODE, UiTree};

/// A UI element as `selectors` sees it. Text nodes are never elements.
pub(crate) struct NodeRef<'a, L: LayoutEngine> {
    pub tree: &'a UiTree<L>,
    pub id: NodeId,
}

impl<L: LayoutEngine> Clone for NodeRef<'_, L> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<L: LayoutEngine> Copy for NodeRef<'_, L> {}

impl<L: LayoutEngine> fmt::Debug for NodeRef<'_, L> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<{} {}>",
            self.tree.kind(self.id).unwrap_or("?"),
            self.id
        )
    }
}

impl<'a, L: LayoutEngine> NodeRef<'a, L> {
    fn at(&self, id: NodeId) -> Self {
        NodeRef {
            tree: self.tree,
            id,
        }
    }

    fn is_element(tree: &UiTree<L>, id: NodeId) -> bool {
        tree.kind(id).is_some_and(|k| k != TEXT_NODE) && !tree.is_generated(id)
    }

    fn sibling(&self, step: isize) -> Option<Self> {
        let parent = self.tree.parent(self.id)?;
        let siblings = self.tree.children(parent);
        let mut i = siblings.iter().position(|c| *c == self.id)? as isize;
        loop {
            i += step;
            let id = *siblings.get(usize::try_from(i).ok()?)?;
            if Self::is_element(self.tree, id) {
                return Some(self.at(id));
            }
        }
    }

    /// No element children, and text children are all empty (or all whitespace for `:blank`).
    fn is_empty_like(&self, allow_whitespace: bool) -> bool {
        self.tree.children(self.id).iter().all(|c| {
            if Self::is_element(self.tree, *c) {
                return false;
            }
            match self.tree.text(*c) {
                Some(t) if allow_whitespace => t.trim().is_empty(),
                Some(t) => t.is_empty(),
                None => true,
            }
        })
    }

    fn focus_within(&self) -> bool {
        let mut stack = vec![self.id];
        while let Some(id) = stack.pop() {
            if self.tree.state(id).focus {
                return true;
            }
            stack.extend_from_slice(self.tree.children(id));
        }
        false
    }
}

impl<'a, L: LayoutEngine> Element for NodeRef<'a, L> {
    type Impl = PlatypusSelectors;

    fn opaque(&self) -> OpaqueElement {
        // Attributes live in a boxed node, so their address is stable and unique.
        match self.tree.attrs(self.id) {
            Some(attrs) => OpaqueElement::new(attrs),
            None => OpaqueElement::new(self.tree),
        }
    }

    fn parent_element(&self) -> Option<Self> {
        self.tree.parent(self.id).map(|p| self.at(p))
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.sibling(-1)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.sibling(1)
    }

    fn first_element_child(&self) -> Option<Self> {
        self.tree
            .children(self.id)
            .iter()
            .find(|c| Self::is_element(self.tree, **c))
            .map(|c| self.at(*c))
    }

    fn is_html_element_in_html_document(&self) -> bool {
        // Element names are matched case-insensitively, like HTML.
        true
    }

    fn has_local_name(&self, name: &CssString) -> bool {
        self.tree.kind(self.id) == Some(name.0.as_str())
    }

    fn has_namespace(&self, _ns: &CssString) -> bool {
        true
    }

    fn is_same_type(&self, other: &Self) -> bool {
        self.tree.kind(self.id) == other.tree.kind(other.id)
    }

    fn attr_matches(
        &self,
        _ns: &NamespaceConstraint<&CssString>,
        local_name: &CssString,
        operation: &AttrSelectorOperation<&CssString>,
    ) -> bool {
        let Some(attrs) = self.tree.attrs(self.id) else {
            return false;
        };
        let class_list;
        let value = match local_name.0.as_str() {
            "class" if !attrs.classes.is_empty() => {
                class_list = attrs.classes.join(" ");
                Some(class_list.as_str())
            }
            "disabled" if attrs.disabled => Some(""),
            "checked" if attrs.checked => Some(""),
            "required" if attrs.required => Some(""),
            name => attrs.get(name),
        };
        value.is_some_and(|v| operation.eval_str(v))
    }

    fn match_non_ts_pseudo_class(
        &self,
        pc: &PseudoClass,
        _context: &mut MatchingContext<PlatypusSelectors>,
    ) -> bool {
        let state = self.tree.state(self.id);
        let disabled = self.tree.attrs(self.id).is_some_and(|a| a.disabled);
        match pc {
            PseudoClass::Hover => state.hover,
            PseudoClass::Active => state.active,
            PseudoClass::Focus => state.focus,
            PseudoClass::FocusVisible => state.focus_visible,
            PseudoClass::FocusWithin => self.focus_within(),
            PseudoClass::Enabled => !disabled,
            PseudoClass::Disabled => disabled,
            PseudoClass::Checked => self.tree.attrs(self.id).is_some_and(|a| a.checked),
            PseudoClass::Valid => self.tree.validity(self.id).is_some_and(|v| v.valid()),
            PseudoClass::Invalid => self.tree.validity(self.id).is_some_and(|v| !v.valid()),
            PseudoClass::Required => {
                self.tree.is_editor(self.id) && self.tree.attrs(self.id).is_some_and(|a| a.required)
            }
            PseudoClass::Optional => {
                self.tree.is_editor(self.id)
                    && self.tree.attrs(self.id).is_some_and(|a| !a.required)
            }
            PseudoClass::Blank => self.is_empty_like(true),
            PseudoClass::Outro => state.outro,
        }
    }

    fn match_pseudo_element(
        &self,
        _pe: &PseudoElement,
        _context: &mut MatchingContext<PlatypusSelectors>,
    ) -> bool {
        false
    }

    fn apply_selector_flags(&self, _flags: ElementSelectorFlags) {}

    fn is_link(&self) -> bool {
        false
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, id: &CssString, case: CaseSensitivity) -> bool {
        self.tree
            .attrs(self.id)
            .and_then(|a| a.id.as_deref())
            .is_some_and(|v| case.eq(v.as_bytes(), id.0.as_bytes()))
    }

    fn has_class(&self, name: &CssString, case: CaseSensitivity) -> bool {
        self.tree.attrs(self.id).is_some_and(|a| {
            a.classes
                .iter()
                .any(|c| case.eq(c.as_bytes(), name.0.as_bytes()))
        })
    }

    fn has_custom_state(&self, _name: &CssString) -> bool {
        false
    }

    fn imported_part(&self, _name: &CssString) -> Option<CssString> {
        None
    }

    fn is_part(&self, _name: &CssString) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.is_empty_like(false)
    }

    fn is_root(&self) -> bool {
        self.id == ROOT
    }

    fn add_element_unique_hashes(&self, _filter: &mut BloomFilter) -> bool {
        false
    }
}

/// Which rules to match: the element itself or one of its pseudo-elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Element,
    Pseudo(PseudoElement),
}

/// A declaration that applies to an element, with its cascade position.
#[derive(Clone, Debug)]
pub(crate) struct Matched {
    pub specificity: u32,
    /// Stylesheet order, then rule order, then declaration order.
    pub order: (usize, usize, usize),
    pub prop: String,
    pub value: Value,
}

/// Collects the declarations of every rule in `sheets` that matches `id`,
/// sorted into cascade order (lowest priority first). Normal and `!important`
/// declarations are returned separately (inline styles go between them).
pub(crate) fn matched_declarations<L: LayoutEngine>(
    tree: &UiTree<L>,
    sheets: &[&Stylesheet],
    index: &RuleIndex,
    id: NodeId,
    target: Target,
    caches: &mut SelectorCaches,
) -> (Vec<Matched>, Vec<Matched>) {
    let element = NodeRef { tree, id };
    let mode = match target {
        Target::Element => MatchingMode::Normal,
        Target::Pseudo(_) => MatchingMode::ForStatelessPseudoElement,
    };
    let mut candidates = Vec::new();
    match target {
        Target::Element => {
            index.candidates(tree.kind(id).unwrap_or(""), tree.attrs(id), &mut candidates)
        }
        Target::Pseudo(_) => candidates.extend_from_slice(&index.pseudo),
    }
    candidates.sort_by_key(|e| (e.sheet, e.rule, e.selector));
    candidates.dedup_by_key(|e| (e.sheet, e.rule, e.selector));

    // Best specificity per matching rule.
    let mut matched_rules: Vec<((usize, usize), u32)> = Vec::new();
    let env = tree.media_env();
    for entry in candidates {
        let rule = &sheets[entry.sheet].rules[entry.rule];
        if !rule.media.iter().all(|m| m.matches(&env)) {
            continue;
        }
        let selector = &rule.selectors.slice()[entry.selector];
        if let Target::Pseudo(p) = target
            && selector.pseudo_element() != Some(&p)
        {
            continue;
        }
        let mut context = MatchingContext::new(
            mode,
            None,
            caches,
            QuirksMode::NoQuirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        if !matches_selector(selector, 0, None, &element, &mut context) {
            continue;
        }
        let key = (entry.sheet, entry.rule);
        let specificity = selector.specificity();
        match matched_rules.last_mut() {
            Some((k, best)) if *k == key => *best = (*best).max(specificity),
            _ => matched_rules.push((key, specificity)),
        }
    }

    let mut normal = Vec::new();
    let mut important = Vec::new();
    for ((sheet_index, rule_index), specificity) in matched_rules {
        let rule = &sheets[sheet_index].rules[rule_index];
        for (decl_index, decl) in rule.declarations.iter().enumerate() {
            let m = Matched {
                specificity,
                order: (sheet_index, rule_index, decl_index),
                prop: decl.prop.clone(),
                value: css_value(&decl.value),
            };
            if decl.important {
                important.push(m);
            } else {
                normal.push(m);
            }
        }
    }
    let key = |m: &Matched| (m.specificity, m.order);
    normal.sort_by_key(key);
    important.sort_by_key(key);
    (normal, important)
}

/// CSS text as a Platypus style value: plain numbers become numbers (so `opacity: .5`
/// and `flex-grow: 1` work like their inline equivalents); everything else stays text.
pub(crate) fn css_value(text: &str) -> Value {
    match text.trim().parse::<f64>() {
        Ok(n) => Value::Number(n),
        Err(_) => Value::String(text.trim().to_owned()),
    }
}

// --- Rule index and invalidation dependencies --------------------------------------

/// Something a selector can depend on that can change at runtime.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    Class(String),
    Id(String),
    Attr(String),
    Hover,
    Active,
    Focus,
    FocusVisible,
    Disabled,
    Checked,
    /// What `:valid`, `:invalid`, `:required` and `:optional` read: a text field's text and
    /// its constraint attributes.
    Validity,
    Outro,
}

/// Which keys matter in which positions, so a change only restyles what it can
/// affect (a simplified version of browsers' invalidation sets).
#[derive(Clone, Debug, Default)]
pub(crate) struct Deps {
    /// Keys in compounds left of `>` or a space: changing them affects descendants.
    pub ancestor: HashSet<Key>,
    /// Keys left of `+` / `~`, or inside `:nth-child(.. of S)`: affect later siblings.
    pub sibling: HashSet<Key>,
    /// Keys inside `:has()`: affect ancestors.
    pub has: HashSet<Key>,
    pub focus_within: bool,
    /// Some selector uses `:has()`, so child changes can affect ancestors.
    pub uses_has: bool,
    /// Some selector depends on position among siblings or on children
    /// (`:nth-child`, `:first-child`, `+`, `~`, `:empty`, `:blank`, `:has`).
    pub structural: bool,
    /// Some selector uses `:outro`, so removed elements linger for their exit.
    pub uses_outro: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Subject,
    Ancestor,
    Sibling,
    Has,
}

impl Deps {
    pub fn add_sheet(&mut self, sheet: &Stylesheet) {
        for rule in &sheet.rules {
            for selector in rule.selectors.slice() {
                self.add_selector(selector, Position::Subject);
            }
        }
    }

    fn add_selector(&mut self, selector: &Selector<PlatypusSelectors>, start: Position) {
        let mut position = start;
        for component in selector.iter_raw_match_order() {
            if let Component::Combinator(c) = component {
                position = match (position, c) {
                    (Position::Has, _) => Position::Has,
                    (_, Combinator::Child | Combinator::Descendant) => Position::Ancestor,
                    (_, Combinator::NextSibling | Combinator::LaterSibling) => {
                        self.structural = true;
                        Position::Sibling
                    }
                    // `.x::before`: the compound before the pseudo is the element itself.
                    (p, _) => p,
                };
                continue;
            }
            self.add_component(component, position);
        }
    }

    fn add_component(&mut self, component: &Component<PlatypusSelectors>, position: Position) {
        let key = match component {
            Component::Class(c) => Some(Key::Class(c.0.clone())),
            Component::ID(id) => Some(Key::Id(id.0.clone())),
            Component::AttributeInNoNamespaceExists { local_name, .. }
            | Component::AttributeInNoNamespace { local_name, .. } => Some(attr_key(&local_name.0)),
            Component::AttributeOther(a) => Some(attr_key(&a.local_name.0)),
            Component::NonTSPseudoClass(pc) => match pc {
                PseudoClass::Hover => Some(Key::Hover),
                PseudoClass::Active => Some(Key::Active),
                PseudoClass::Focus => Some(Key::Focus),
                PseudoClass::FocusVisible => Some(Key::FocusVisible),
                PseudoClass::Enabled | PseudoClass::Disabled => Some(Key::Disabled),
                PseudoClass::Checked => Some(Key::Checked),
                PseudoClass::Valid
                | PseudoClass::Invalid
                | PseudoClass::Required
                | PseudoClass::Optional => Some(Key::Validity),
                PseudoClass::Outro => {
                    self.uses_outro = true;
                    Some(Key::Outro)
                }
                PseudoClass::FocusWithin => {
                    self.focus_within = true;
                    None
                }
                PseudoClass::Blank => {
                    self.structural = true;
                    None
                }
            },
            Component::Empty | Component::Nth(_) => {
                self.structural = true;
                None
            }
            Component::NthOf(data) => {
                self.structural = true;
                for s in data.selectors() {
                    self.add_selector(s, Position::Sibling);
                }
                None
            }
            Component::Negation(list) | Component::Is(list) | Component::Where(list) => {
                for s in list.slice() {
                    self.add_selector(s, position);
                }
                None
            }
            Component::Has(relative) => {
                self.structural = true;
                self.uses_has = true;
                for r in relative.iter() {
                    self.add_selector(&r.selector, Position::Has);
                }
                None
            }
            _ => None,
        };
        if let Some(key) = key {
            match position {
                Position::Subject => {}
                Position::Ancestor => {
                    self.ancestor.insert(key);
                }
                Position::Sibling => {
                    self.sibling.insert(key);
                }
                Position::Has => {
                    self.has.insert(key);
                }
            }
        }
    }
}

fn attr_key(name: &str) -> Key {
    match name {
        "class" => Key::Attr("class".into()),
        "id" => Key::Attr("id".into()),
        "disabled" => Key::Disabled,
        "checked" => Key::Checked,
        other => Key::Attr(other.to_owned()),
    }
}

/// One selector of one rule, filed under the most specific key of its subject compound.
#[derive(Clone, Copy, Debug)]
pub(crate) struct IndexEntry {
    pub sheet: usize,
    pub rule: usize,
    pub selector: usize,
}

/// Rules bucketed by the id, class or element type their rightmost compound
/// requires, so each element only tests rules that could match it.
#[derive(Clone, Debug, Default)]
pub(crate) struct RuleIndex {
    by_id: HashMap<String, Vec<IndexEntry>>,
    by_class: HashMap<String, Vec<IndexEntry>>,
    by_type: HashMap<String, Vec<IndexEntry>>,
    universal: Vec<IndexEntry>,
    /// Same for `::before` / `::after` rules (bucketed by the originating compound).
    pseudo: Vec<IndexEntry>,
}

impl RuleIndex {
    pub fn build(sheets: &[&Stylesheet]) -> Self {
        let mut index = RuleIndex::default();
        for (sheet_index, sheet) in sheets.iter().enumerate() {
            for (rule_index, rule) in sheet.rules.iter().enumerate() {
                for (selector_index, selector) in rule.selectors.slice().iter().enumerate() {
                    let entry = IndexEntry {
                        sheet: sheet_index,
                        rule: rule_index,
                        selector: selector_index,
                    };
                    if selector.pseudo_element().is_some() {
                        index.pseudo.push(entry);
                        continue;
                    }
                    let (mut id, mut class, mut ty) = (None, None, None);
                    for component in selector.iter() {
                        match component {
                            Component::ID(i) => id = Some(i.0.clone()),
                            Component::Class(c) => class = class.or(Some(c.0.clone())),
                            Component::LocalName(n) => ty = Some(n.lower_name.0.clone()),
                            _ => {}
                        }
                    }
                    match (id, class, ty) {
                        (Some(id), _, _) => index.by_id.entry(id).or_default().push(entry),
                        (_, Some(class), _) => index.by_class.entry(class).or_default().push(entry),
                        (_, _, Some(ty)) => index.by_type.entry(ty).or_default().push(entry),
                        _ => index.universal.push(entry),
                    }
                }
            }
        }
        index
    }

    /// Entries that could match an element with this kind and attributes.
    fn candidates(&self, kind: &str, attrs: Option<&Attrs>, out: &mut Vec<IndexEntry>) {
        out.extend_from_slice(&self.universal);
        if let Some(v) = self.by_type.get(kind) {
            out.extend_from_slice(v);
        }
        if let Some(attrs) = attrs {
            if let Some(v) = attrs.id.as_ref().and_then(|id| self.by_id.get(id)) {
                out.extend_from_slice(v);
            }
            for class in &attrs.classes {
                if let Some(v) = self.by_class.get(class) {
                    out.extend_from_slice(v);
                }
            }
        }
    }
}
