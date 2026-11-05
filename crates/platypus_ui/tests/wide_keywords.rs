// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! CSS-wide keywords (M3d step 1): `initial`, `inherit`, `unset` and `revert` on any
//! property, inline or from stylesheets, for inherited and non-inherited properties.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::opcodes::SET_ATTRS;
use platypus_ui::{DrawCmd, Rgba, UiTree};
use web_ui_core::{NodeId, Value};

fn rgb(s: &str) -> Rgba {
    Rgba::parse(s).unwrap()
}

/// A green, 30px-font parent with a red background and 10px padding, holding one
/// 20x20 child with `child_style`.
fn family(child_style: &[(&str, Value)], css: &str) -> (UiTree, NodeId, NodeId) {
    let mut d = Dom::new();
    let parent = d.add(
        0,
        "box",
        &[
            ("color", "#00ff00".into()),
            ("fontSize", px(30.0)),
            ("backgroundColor", "#ff0000".into()),
            ("padding", px(10.0)),
            ("width", px(100.0)),
            ("height", px(100.0)),
        ],
    );
    let mut style = vec![("width", px(20.0)), ("height", px(20.0))];
    style.extend_from_slice(child_style);
    let child = d.add(parent, "box", &style);
    d.ops.push(op(
        SET_ATTRS,
        &[n(child), "className".into(), "child".into()],
    ));
    let mut tree = d.build(css);
    tree.layout_if_needed();
    (tree, NodeId(parent), NodeId(child))
}

#[test]
fn inherit_copies_the_parents_value_even_for_properties_that_dont_inherit() {
    let (tree, _, child) = family(
        &[
            ("backgroundColor", "inherit".into()),
            ("padding", "inherit".into()),
            ("width", "auto".into()),
        ],
        "",
    );
    assert_eq!(tree.visual(child).unwrap().background, rgb("#ff0000"));
    assert_eq!(
        tree.rect(child).unwrap().width,
        20.0,
        "an empty box with the parent's 10px padding on each side"
    );
}

#[test]
fn initial_sets_the_initial_value_and_stops_inheritance() {
    let (tree, _, child) = family(
        &[("color", "initial".into()), ("fontSize", "initial".into())],
        "",
    );
    let text = tree.computed_text(child);
    assert_eq!(text.color, Rgba::new(0.0, 0.0, 0.0, 1.0));
    assert_eq!(text.font_size, 16.0);
}

#[test]
fn unset_inherits_inherited_properties_and_resets_others() {
    let (tree, _, child) = family(
        &[("color", "unset".into()), ("width", "unset".into())],
        ".child { color: blue; width: 50px }",
    );
    assert_eq!(
        tree.computed_text(child).color,
        rgb("#00ff00"),
        "the inline unset beats the rule and inherits"
    );
    assert_eq!(
        tree.rect(child).unwrap().width,
        0.0,
        "width back to auto: an empty flex item"
    );
}

#[test]
fn revert_acts_like_unset() {
    let (tree, _, child) = family(&[("color", "revert".into())], ".child { color: blue }");
    assert_eq!(tree.computed_text(child).color, rgb("#00ff00"));
}

#[test]
fn keywords_work_in_stylesheets_without_warnings() {
    let mut d = Dom::new();
    let parent = d.add(0, "box", &[("backgroundColor", "#ff0000".into())]);
    let child = d.add(parent, "box", &[("width", px(5.0)), ("height", px(5.0))]);
    let mut tree = d.build("");
    let added = tree.add_stylesheet(
        "box box { background-color: inherit; border-radius: initial; cursor: unset }",
    );
    assert!(added.warnings.is_empty(), "{:?}", added.warnings);
    tree.layout_if_needed();
    assert_eq!(
        tree.visual(NodeId(child)).unwrap().background,
        rgb("#ff0000")
    );
}

#[test]
fn inherit_at_the_top_level_means_initial() {
    let mut d = Dom::new();
    let a = d.add(0, "box", &[("backgroundColor", "inherit".into())]);
    let mut tree = d.build("");
    tree.layout_if_needed();
    assert_eq!(
        tree.visual(NodeId(a)).unwrap().background,
        Rgba::TRANSPARENT
    );
}

#[test]
fn generated_content_inherits_from_its_element() {
    let mut d = Dom::new();
    d.add(0, "box", &[("backgroundColor", "#0000ff".into())]);
    let mut tree = d.build(
        r#"box::before { content: ""; width: 10px; height: 10px; background-color: inherit }"#,
    );
    let blue_quads = tree
        .draw_list()
        .into_iter()
        .filter(|c| matches!(c, DrawCmd::Quad(q) if q.background == rgb("#0000ff")))
        .count();
    assert_eq!(blue_quads, 2, "the box and its ::before");
}
