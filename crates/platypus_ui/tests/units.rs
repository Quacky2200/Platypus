// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Relative units and math in styles (M3d step 1): `em`, `rem`, viewport units and
//! `calc()` / `min()` / `max()` / `clamp()` resolve at restyle, from inline styles and
//! stylesheets, and update when the font size or the window size changes.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::UiTree;
use platypus_ui::opcodes::SET_STYLE;
use web_ui_core::{Commit, NodeId, Value};

fn width(tree: &mut UiTree, id: u32) -> f32 {
    tree.layout_if_needed();
    tree.rect(NodeId(id)).unwrap().width
}

fn set(tree: &mut UiTree, id: u32, prop: &str, value: Value) {
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_STYLE, &[n(id), prop.into(), value])],
    });
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn em_uses_the_elements_own_font_size_and_rem_the_roots() {
    let mut d = Dom::new();
    let parent = d.add(
        0,
        "box",
        &[("fontSize", px(20.0)), ("flexDirection", "column".into())],
    );
    let a = d.add(
        parent,
        "box",
        &[("width", "10em".into()), ("height", px(5.0))],
    );
    let b = d.add(
        parent,
        "box",
        &[
            ("fontSize", "2em".into()),
            ("width", "3em".into()),
            ("height", px(5.0)),
        ],
    );
    let c = d.add(
        parent,
        "box",
        &[("width", "4rem".into()), ("height", px(5.0))],
    );
    let mut tree = d.build("");
    assert_eq!(width(&mut tree, a), 200.0, "10 x the inherited 20px");
    assert_eq!(
        width(&mut tree, b),
        120.0,
        "its own font size is 2 x 20 = 40px"
    );
    assert_eq!(width(&mut tree, c), 64.0, "rem: the root's 16px");
}

#[test]
fn stylesheets_resolve_units_and_math_too() {
    let mut d = Dom::new();
    let wrap = d.add(0, "box", &[("flexDirection", "column".into())]);
    for _ in 0..4 {
        d.add(wrap, "box", &[("height", px(5.0))]);
    }
    let mut tree = d.build(
        ":root { font-size: 10px }
         box:nth-child(1) { width: calc(10px + 2rem) }
         box:nth-child(2) { width: min(50vw, 30rem) }
         box:nth-child(3) { width: clamp(1rem, 10vw, 25px) }
         box:nth-child(4) { padding: 0.5rem; width: max(1em, 3px) }",
    );
    let kids: Vec<u32> = tree.children(NodeId(wrap)).iter().map(|c| c.0).collect();
    assert_eq!(width(&mut tree, kids[0]), 30.0);
    assert_eq!(
        width(&mut tree, kids[1]),
        200.0,
        "50vw of 400 is 200, under 300"
    );
    assert_eq!(width(&mut tree, kids[2]), 25.0, "10vw = 40, clamped to 25");
    assert_eq!(width(&mut tree, kids[3]), 10.0, "1em of the 10px root font");
}

#[test]
fn viewport_units_follow_the_window() {
    let mut d = Dom::new();
    let a = d.add(
        0,
        "box",
        &[("width", "50vw".into()), ("height", "10vh".into())],
    );
    let mut tree = d.build("");
    assert_eq!(width(&mut tree, a), 200.0);
    tree.set_size(800.0, 600.0);
    assert_eq!(width(&mut tree, a), 400.0, "restyled for the new size");
    assert_eq!(tree.rect(NodeId(a)).unwrap().height, 60.0);
}

#[test]
fn em_values_follow_a_parents_font_size_change() {
    let mut d = Dom::new();
    let parent = d.add(0, "box", &[("fontSize", px(10.0))]);
    let child = d.add(
        parent,
        "box",
        &[("width", "5em".into()), ("height", px(5.0))],
    );
    let mut tree = d.build("");
    assert_eq!(width(&mut tree, child), 50.0);
    set(&mut tree, parent, "fontSize", px(30.0));
    assert_eq!(width(&mut tree, child), 150.0);
}

#[test]
fn rem_values_follow_the_root_font_size() {
    let mut d = Dom::new();
    let a = d.add(0, "box", &[("width", "10rem".into()), ("height", px(5.0))]);
    let mut tree = d.build("");
    assert_eq!(width(&mut tree, a), 160.0);
    let added = tree.add_stylesheet(":root { font-size: 20px }");
    assert!(added.warnings.is_empty());
    assert_eq!(width(&mut tree, a), 200.0);
}

#[test]
fn paint_and_text_properties_take_relative_units() {
    let mut d = Dom::new();
    let a = d.add(
        0,
        "box",
        &[
            ("fontSize", px(10.0)),
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("borderRadius", "2em".into()),
            ("letterSpacing", "0.1em".into()),
        ],
    );
    let mut tree = d.build("");
    tree.layout_if_needed();
    let v = tree.visual(NodeId(a)).unwrap();
    assert_eq!(v.border_radius.top_left.x, platypus_ui::Len::Px(20.0));
    assert!((tree.computed_text(NodeId(a)).letter_spacing - 1.0).abs() < 1e-4);
}

#[test]
fn invalid_math_is_ignored_like_invalid_css() {
    let mut d = Dom::new();
    let a = d.add(0, "box", &[("width", px(50.0)), ("height", px(5.0))]);
    let mut tree = d.build("");
    let added = tree.add_stylesheet("box { width: calc(10px + 2) }");
    assert_eq!(added.warnings, ["invalid math in `width: calc(10px + 2)`"]);
    assert_eq!(width(&mut tree, a), 50.0, "the inline 50px stands");
    // Inline styles with invalid math are rejected and the old value stays.
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(
            SET_STYLE,
            &[n(a), "width".into(), "calc(1px * 2px)".into()],
        )],
    });
    assert_eq!(errors.len(), 1);
    assert_eq!(width(&mut tree, a), 50.0);
}

// M3d step 1b: math with percentages is evaluated by layout.
#[test]
fn math_with_percentages_resolves_against_the_containing_block() {
    let mut d = Dom::new();
    let wrap = d.add(
        0,
        "box",
        &[("flexDirection", "column".into()), ("width", px(300.0))],
    );
    let a = d.add(
        wrap,
        "box",
        &[("width", "calc(100% - 20px)".into()), ("height", px(5.0))],
    );
    let b = d.add(
        wrap,
        "box",
        &[("width", "min(100%, 120px)".into()), ("height", px(5.0))],
    );
    let c = d.add(
        wrap,
        "box",
        &[
            ("width", "clamp(50px, 50% - 2em, 200px)".into()),
            ("fontSize", px(10.0)),
            ("height", px(5.0)),
        ],
    );
    let e = d.add(
        wrap,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(5.0)),
            ("marginLeft", "calc(10% + 5px)".into()),
        ],
    );
    let mut tree = d.build("");
    assert_eq!(width(&mut tree, a), 280.0);
    assert_eq!(width(&mut tree, b), 120.0);
    assert_eq!(width(&mut tree, c), 130.0, "50% of 300 minus 2em (20px)");
    assert_eq!(tree.rect(NodeId(e)).unwrap().x, 35.0, "10% of 300 plus 5");
}

#[test]
fn percentage_math_follows_the_window_and_works_in_stylesheets() {
    let mut d = Dom::new();
    let a = d.add(0, "box", &[("height", px(5.0))]);
    let mut tree = d.build("box { width: calc(50% + 10px); padding: calc(5% - 10px) 0 }");
    assert_eq!(width(&mut tree, a), 210.0, "half of 400, plus 10");
    tree.set_size(800.0, 400.0);
    assert_eq!(width(&mut tree, a), 410.0);
    let r = tree.rect(NodeId(a)).unwrap();
    assert_eq!(
        r.height, 60.0,
        "padding: 5% of 800 minus 10 = 30, top and bottom (border-box: it outgrows the 5px)"
    );
}

#[test]
fn flex_basis_takes_percentage_math() {
    let mut d = Dom::new();
    let row = d.add(0, "box", &[("width", px(200.0)), ("height", px(10.0))]);
    let a = d.add(
        row,
        "box",
        &[
            ("flexBasis", "calc(25% + 10px)".into()),
            ("flexShrink", px(0.0)),
        ],
    );
    let mut tree = d.build("");
    assert_eq!(width(&mut tree, a), 60.0);
}

#[test]
fn box_sizing_defaults_to_border_box_and_content_box_adds_padding() {
    let mut d = Dom::new();
    let wrap = d.add(
        0,
        "box",
        &[
            ("flexDirection", "column".into()),
            ("alignItems", "flex-start".into()),
        ],
    );
    let style = |sizing: Option<&str>| {
        let mut s = vec![
            ("width", px(100.0)),
            ("height", px(10.0)),
            ("padding", px(10.0)),
        ];
        if let Some(b) = sizing {
            s.push(("boxSizing", b.into()));
        }
        s
    };
    let default = d.add(wrap, "box", &style(None));
    let border = d.add(wrap, "box", &style(Some("border-box")));
    let content = d.add(wrap, "box", &style(Some("content-box")));
    let mut tree = d.build("");
    assert_eq!(width(&mut tree, default), 100.0, "border-box by default");
    assert_eq!(width(&mut tree, border), 100.0);
    assert_eq!(
        width(&mut tree, content),
        120.0,
        "content-box adds the padding"
    );
}
