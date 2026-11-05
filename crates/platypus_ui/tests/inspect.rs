// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What the UI inspector asks of a tree (M3-INS-01..03).

use platypus_ui::opcodes::Op;
use platypus_ui::{ROOT, UiTree};
use web_ui_core::{NodeId, Value};

fn card(css: &str) -> UiTree {
    let mut tree = UiTree::new(400.0, 300.0);
    tree.add_stylesheet(css);
    let id = |n| NodeId(n);
    let ops = [
        Op::Create {
            id: id(1),
            kind: "box".into(),
        },
        Op::SetAttrs {
            id: id(1),
            pairs: vec![("className".into(), "card hot".into())],
        },
        Op::SetStyle {
            id: id(1),
            pairs: vec![("width".into(), Value::Number(100.0))],
        },
        Op::AppendChild {
            parent: ROOT,
            child: id(1),
        },
    ];
    assert!(tree.apply_ops(&ops).is_empty());
    tree.layout_if_needed();
    tree
}

fn decl<'a>(rules: &'a [platypus_ui::InspectedRule], prop: &str) -> Vec<(&'a str, &'a str, bool)> {
    rules
        .iter()
        .flat_map(|r| r.declarations.iter().map(move |d| (r, d)))
        .filter(|(_, d)| d.prop == prop)
        .map(|(r, d)| (r.selector.as_str(), d.value.as_str(), d.applied))
        .collect()
}

// M3-INS-01
#[test]
fn inspect_reports_node_boxes_classes_and_children() {
    let tree = card("");
    let i = tree.inspect(NodeId(1)).unwrap();
    assert_eq!(i.kind, "box");
    assert_eq!(i.classes, ["card", "hot"]);
    assert_eq!(i.parent, Some(ROOT));
    assert_eq!(i.rect.unwrap().width, 100.0);
    assert!(tree.inspect(NodeId(99)).is_none());
}

// M3-INS-02
#[test]
fn inspect_lists_matching_rules_and_which_declarations_win() {
    let tree = card(
        ".card { height: 10px; color: red } .card.hot { color: blue } \
         .other { color: green } box { height: 20px !important }",
    );
    let i = tree.inspect(NodeId(1)).unwrap();
    // The later, more specific rule wins `color`; the unmatched rule is absent.
    assert_eq!(
        decl(&i.rules, "color"),
        [(".card", "red", false), (".card.hot", "blue", true)]
    );
    // `!important` beats the normal rule even with lower specificity.
    assert_eq!(
        decl(&i.rules, "height"),
        [(".card", "10px", false), ("box", "20px", true)]
    );
    assert!(i.rules.iter().all(|r| r.selector != ".other"));
    // An inline style shadows the rules.
    let width: Vec<_> = i.inline.iter().filter(|p| p.prop == "width").collect();
    assert_eq!(width.len(), 1);
    assert!(width[0].applied);
}

// M3-INS-03
#[test]
fn inspect_set_style_changes_the_node_live() {
    let mut tree = card(".card { width: 50px }");
    assert!(
        tree.inspect_set_style(NodeId(1), "width", "200px")
            .is_empty()
    );
    tree.layout_if_needed();
    assert_eq!(tree.inspect(NodeId(1)).unwrap().rect.unwrap().width, 200.0);
    // An empty value removes the inline declaration; the rule applies again.
    assert!(tree.inspect_set_style(NodeId(1), "width", "").is_empty());
    tree.layout_if_needed();
    assert_eq!(tree.inspect(NodeId(1)).unwrap().rect.unwrap().width, 50.0);
}

fn computed<'a>(rules: &'a [platypus_ui::InspectedRule], prop: &str) -> Option<&'a str> {
    rules
        .iter()
        .flat_map(|r| &r.declarations)
        .find(|d| d.prop == prop)
        .and_then(|d| d.computed.as_deref())
}

// M3-INS-12
#[test]
fn inspect_says_what_calc_var_and_relative_units_come_to() {
    // The root is 400 x 300: 100% of the parent's height less 20px is 280px.
    let tree = card(
        ".card { --gap: 12px; height: calc(100% - 20px); width: var(--gap); padding-top: 2em; \
         color: red; margin-left: 5px }",
    );
    let i = tree.inspect(NodeId(1)).unwrap();
    assert_eq!(computed(&i.rules, "height"), Some("280px"));
    assert_eq!(computed(&i.rules, "width"), Some("12px"));
    assert_eq!(
        computed(&i.rules, "paddingTop"),
        Some("32px"),
        "2em of a 16px font"
    );
    // A plain value has no computed form to show.
    assert_eq!(computed(&i.rules, "color"), None);
    assert_eq!(computed(&i.rules, "marginLeft"), None);
}

// M3-INS-13
#[test]
fn inspect_reports_the_layout_a_flexbox_preview_draws() {
    let tree = card(
        ".card { display: flex; flex-direction: column; justify-content: center; \
         align-items: flex-end; flex-wrap: wrap; gap: 8px }",
    );
    let layout = tree.inspect(NodeId(1)).unwrap().layout;
    let get = |name: &str| {
        layout
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("display"), Some("flex"));
    assert_eq!(get("flexDirection"), Some("column"));
    assert_eq!(get("justifyContent"), Some("center"));
    assert_eq!(get("alignItems"), Some("flex-end"));
    assert_eq!(get("flexWrap"), Some("wrap"));
    assert_eq!(get("columnGap"), Some("8px"));
    assert_eq!(get("rowGap"), Some("8px"));
}

// M3-INS-14
#[test]
fn inspect_adds_and_removes_classes_and_lists_the_ones_stylesheets_name() {
    let mut tree = card(".card .hot, #main .other-one > box { color: red } .extra { width: 50px }");
    assert!(tree.inspect_set_class(NodeId(1), "extra", true).is_empty());
    assert_eq!(
        tree.inspect(NodeId(1)).unwrap().classes,
        ["card", "hot", "extra"]
    );
    assert!(tree.inspect_set_class(NodeId(1), "hot", false).is_empty());
    assert_eq!(tree.inspect(NodeId(1)).unwrap().classes, ["card", "extra"]);
    // Adding what is there twice keeps one.
    assert!(tree.inspect_set_class(NodeId(1), "extra", true).is_empty());
    assert_eq!(tree.inspect(NodeId(1)).unwrap().classes, ["card", "extra"]);
    let names = tree.inspect_class_names();
    for want in ["card", "hot", "other-one", "extra"] {
        assert!(names.iter().any(|n| n == want), "{want} in {names:?}");
    }
    assert!(!names.iter().any(|n| n == "main"), "an id is not a class");
}

// M3-INS-19
#[test]
fn inspect_layout_has_the_margins_and_paddings_a_box_model_overlay_draws() {
    let tree = card(".card { margin: 6px 8px; padding-top: 4px; padding-left: 10% }");
    let layout = tree.inspect(NodeId(1)).unwrap().layout;
    let get = |name: &str| {
        layout
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    };
    assert_eq!(get("marginTop"), Some("6px"));
    assert_eq!(get("marginRight"), Some("8px"));
    assert_eq!(get("marginLeft"), Some("8px"));
    assert_eq!(get("paddingTop"), Some("4px"));
    assert_eq!(get("paddingBottom"), Some("0px"));
    // 10% of the parent's content width (the root is 400 wide).
    assert_eq!(get("paddingLeft"), Some("40px"));
}

// M3-INS-20
#[test]
fn inspect_rule_matches_names_the_elements_a_rule_styles() {
    let tree = card(".hot { color: red } .card::before { color: blue }");
    let rules = tree.inspect(NodeId(1)).unwrap().rules;
    let find = |selector: &str| rules.iter().find(|r| r.selector == selector).unwrap();
    let hot = find(".hot");
    assert_eq!(tree.inspect_rule_matches(hot.sheet, hot.rule), [NodeId(1)]);
    // A pseudo-element's rule is about the element that owns it.
    let before = find(".card::before");
    assert_eq!(
        tree.inspect_rule_matches(before.sheet, before.rule),
        [NodeId(1)]
    );
    assert!(tree.inspect_rule_matches(hot.sheet, 99).is_empty());
    assert!(tree.inspect_rule_matches(9999, 0).is_empty());
}

// M3-INS-18
#[test]
fn inspect_brief_has_the_boxes_and_names_but_none_of_the_styles() {
    let tree = card(".card { height: 10px } .card::before { color: red }");
    let full = tree.inspect(NodeId(1)).unwrap();
    let brief = tree.inspect_brief(NodeId(1)).unwrap();
    assert!(!full.rules.is_empty() && !full.inline.is_empty() && !full.layout.is_empty());
    assert!(brief.rules.is_empty() && brief.inline.is_empty() && brief.layout.is_empty());
    assert_eq!(
        (
            brief.kind.as_str(),
            &brief.classes,
            brief.rect,
            brief.padding_box,
            &brief.children
        ),
        (
            full.kind.as_str(),
            &full.classes,
            full.rect,
            full.padding_box,
            &full.children
        )
    );
    assert!(tree.inspect_brief(NodeId(99)).is_none());
}

// M3-INS-15
#[test]
fn inspect_lists_the_rules_of_a_pseudo_element() {
    let tree = card(
        ".card::before { content: \"x\"; color: red } .card::after { color: blue } .card { color: green }",
    );
    let before = tree.inspect_pseudo(NodeId(1), "before");
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].selector, ".card::before");
    assert!(
        before[0]
            .declarations
            .iter()
            .any(|d| d.prop == "color" && d.value == "red")
    );
    let after = tree.inspect_pseudo(NodeId(1), "after");
    assert_eq!(after[0].declarations[0].value, "blue");
    // `inspect` lists the element's own rules (no pseudo tag) and then each pseudo-element's, tagged.
    let all = tree.inspect(NodeId(1)).unwrap().rules;
    let own: Vec<_> = all.iter().filter(|r| r.pseudo.is_empty()).collect();
    assert!(own.iter().all(|r| !r.selector.contains("::")));
    assert_eq!(
        all.iter()
            .filter(|r| !r.pseudo.is_empty())
            .map(|r| (r.pseudo.as_str(), r.selector.as_str()))
            .collect::<Vec<_>>(),
        [("before", ".card::before"), ("after", ".card::after")]
    );
    assert!(tree.inspect_pseudo(NodeId(1), "marker").is_empty());
    assert!(tree.inspect_pseudo(NodeId(1), "bogus").is_empty());
    assert!(tree.inspect_pseudo(NodeId(99), "before").is_empty());
}
