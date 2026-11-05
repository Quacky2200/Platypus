// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Inline SVG (M3-SVG-02): `svg` elements size from their attributes, serialize their
//! content with attributes and computed CSS presentation properties, restyle and
//! transition like other elements, and are hit as one box.

use crate::common;

use common::{Dom, n, op};
use platypus_ui::opcodes::{SET_ATTRS, SET_STYLE};
use platypus_ui::{BackgroundImage, DrawCmd, ElementState, UiTree};
use web_ui_core::{Commit, NodeId, Value};

fn attrs(d: &mut Dom, id: u32, pairs: &[(&str, Value)]) {
    let mut a = vec![n(id)];
    for (k, v) in pairs {
        a.push((*k).into());
        a.push(v.clone());
    }
    d.ops.push(op(SET_ATTRS, &a));
}

/// An `svg` (viewBox 0 0 24 24, 48 wide) holding a `.dot` circle and SVG text.
fn icon(css: &str) -> (UiTree, u32, u32) {
    let mut d = Dom::new();
    // (Not stretched by the root's flex layout.)
    let svg = d.add(0, "svg", &[("alignSelf", "flex-start".into())]);
    attrs(
        &mut d,
        svg,
        &[("viewBox", "0 0 24 24".into()), ("width", 48.0.into())],
    );
    let dot = d.add(svg, "circle", &[]);
    attrs(
        &mut d,
        dot,
        &[
            ("cx", 12.0.into()),
            ("cy", 12.0.into()),
            ("r", 10.0.into()),
            ("strokeWidth", 2.0.into()),
            ("className", "dot".into()),
        ],
    );
    let label = d.add(svg, "text", &[]);
    attrs(&mut d, label, &[("x", 2.0.into()), ("y", 20.0.into())]);
    d.text(label, "A&B");
    (d.build(css), svg, dot)
}

/// The serialized markup of the first inline SVG drawn.
fn markup(tree: &mut UiTree) -> String {
    tree.draw_list()
        .into_iter()
        .find_map(|c| match c {
            DrawCmd::Quad(q) => q.images.first().and_then(|f| match &*f.image {
                BackgroundImage::Url(u) => u.strip_prefix("data:image/svg+xml,").map(str::to_owned),
                _ => None,
            }),
            _ => None,
        })
        .expect("an inline SVG")
}

// M3-SVG-02
#[test]
fn svg_elements_size_from_their_attributes() {
    let (tree, svg, _) = icon("");
    let r = tree.rect(NodeId(svg)).unwrap();
    assert_eq!(
        (r.width, r.height),
        (48.0, 48.0),
        "the viewBox keeps it square"
    );
}

// M3-SVG-02
#[test]
fn content_serializes_with_attributes_and_css() {
    let (mut tree, _, _) = icon(".dot { fill: red; stroke: currentColor } svg { color: blue }");
    let m = markup(&mut tree);
    assert!(m.starts_with("<svg "), "{m}");
    assert!(m.contains(r#"viewBox="0 0 24 24""#), "{m}");
    assert!(m.contains(r#"width="48""#), "{m}");
    assert!(
        m.contains(r#"color="rgba(0,0,255,1)""#),
        "currentColor: {m}"
    );
    assert!(
        m.contains(r#"<circle cx="12" cy="12" r="10" stroke-width="2" fill="red" stroke="currentColor"></circle>"#),
        "{m}"
    );
    assert!(
        m.contains(r#"<text x="2" y="20">A&amp;B</text>"#),
        "SVG text: {m}"
    );
}

// M3-SVG-02
#[test]
fn hover_rules_and_transitions_restyle_svg_content() {
    let (mut tree, svg, _) =
        icon(".dot { fill: #ff0000; transition: fill 1s linear } svg:hover .dot { fill: #0000ff }");
    tree.set_time(0.0);
    tree.layout_if_needed();
    assert!(markup(&mut tree).contains(r##"fill="#ff0000""##));
    tree.set_state(
        NodeId(svg),
        ElementState {
            hover: true,
            ..Default::default()
        },
    );
    tree.layout_if_needed();
    tree.set_time(500.0);
    let m = markup(&mut tree);
    assert!(
        m.contains(r#"fill="rgba(127.5, 0, 127.5, 1)""#),
        "halfway: {m}"
    );
}

/// How opaque the inline SVG is drawn (the product of the layers around it), and its root
/// element's markup; `None` when it isn't drawn.
fn svg_opacity(tree: &mut UiTree) -> Option<(f32, String)> {
    let mut layers: Vec<f32> = Vec::new();
    let mut seen = None;
    for c in tree.draw_list() {
        match c {
            DrawCmd::PushLayer { opacity, .. } => layers.push(opacity),
            DrawCmd::PopLayer => {
                layers.pop();
            }
            DrawCmd::Quad(q) if !q.images.is_empty() => {
                if let BackgroundImage::Url(u) = &*q.images[0].image {
                    let root = u.split('>').next().unwrap_or_default().to_owned();
                    seen = Some((layers.iter().product(), root));
                }
            }
            _ => {}
        }
    }
    seen
}

// M3-SVG-04
#[test]
fn an_svg_fades_once_when_its_parent_is_hovered() {
    let mut d = Dom::new();
    let arrow = d.add(0, "box", &[("alignSelf", "flex-start".into())]);
    attrs(&mut d, arrow, &[("className", "arrow".into())]);
    let svg = d.add(arrow, "svg", &[]);
    attrs(
        &mut d,
        svg,
        &[
            ("viewBox", "0 0 24 24".into()),
            ("width", 24.0.into()),
            ("className", "lit".into()),
        ],
    );
    let line = d.add(svg, "polyline", &[]);
    attrs(
        &mut d,
        line,
        &[
            ("points", "6,4 18,12 6,20".into()),
            ("stroke", "#ffffff".into()),
        ],
    );
    let mut tree =
        d.build(".lit { opacity: 0; transition: opacity 0.1s } .arrow:hover .lit { opacity: 1 }");
    tree.set_time(0.0);
    tree.layout_if_needed();
    assert!(svg_opacity(&mut tree).is_none(), "hidden at rest");

    let hover = |tree: &mut UiTree, on: bool| {
        tree.set_state(
            NodeId(arrow),
            ElementState {
                hover: on,
                ..Default::default()
            },
        );
        tree.layout_if_needed();
    };
    hover(&mut tree, true);
    tree.set_time(50.0);
    let (halfway, root) = svg_opacity(&mut tree).expect("drawn while fading in");
    assert!(halfway > 0.6 && halfway < 0.95, "eased halfway: {halfway}");
    // The box's layer carries the opacity; the markup mustn't apply it a second time.
    assert!(!root.contains("opacity="), "opacity applied twice: {root}");
    tree.set_time(200.0);
    assert_eq!(
        svg_opacity(&mut tree).map(|(o, _)| o),
        Some(1.0),
        "fully shown"
    );

    hover(&mut tree, false);
    tree.set_time(250.0);
    let (leaving, _) = svg_opacity(&mut tree).expect("drawn while fading out");
    assert!(leaving < 0.5, "fading out: {leaving}");
    tree.set_time(400.0);
    assert!(svg_opacity(&mut tree).is_none(), "hidden again");
}

// M3-SVG-02
#[test]
fn svg_is_hit_as_one_box_and_its_text_isnt_platypus_text() {
    let (mut tree, svg, dot) = icon("");
    assert_eq!(tree.hit_test(24.0, 24.0), Some(NodeId(svg)));
    assert!(tree.rect(NodeId(dot)).is_none(), "content isn't laid out");
    assert!(!tree.is_text_root(NodeId(4)), "SVG text isn't a text block");
    // Attribute changes resize it.
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_ATTRS, &[n(svg), "width".into(), 96.0.into()])],
    });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    assert_eq!(tree.rect(NodeId(svg)).unwrap().width, 96.0);
    // CSS sizes on the svg element win, like an image.
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_STYLE, &[n(svg), "width".into(), 20.0.into()])],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(tree.rect(NodeId(svg)).unwrap().width, 20.0);
}
