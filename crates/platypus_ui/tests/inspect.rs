// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What the UI inspector asks of a tree (M3-INS-01..03).

use platypus_core::{NodeId, Value};
use platypus_ui::opcodes::Op;
use platypus_ui::{ROOT, UiTree};

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
