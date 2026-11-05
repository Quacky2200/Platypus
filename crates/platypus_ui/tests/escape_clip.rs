// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Absolute boxes and the clips of their ancestors (M3-CLIP-02..04): an `overflow` clip
//! reaches an absolute descendant only when the clipping element is its containing block or
//! lies between them, as in CSS. Drawing, hit testing and scrolling all follow that.

use crate::common;

use common::{Dom, px};
use platypus_ui::{DrawCmd, Input, Interaction, UiTree};
use web_ui_core::{NodeId, Value};

/// What is open around the first quad `width` wide, outermost first: `clip WxH`, `layer`,
/// `transform`.
fn around(tree: &mut UiTree, width: f32) -> Vec<String> {
    let mut open = Vec::new();
    for cmd in tree.draw_list() {
        match cmd {
            DrawCmd::PushClip(c) => open.push(format!("clip {}x{}", c.rect.width, c.rect.height)),
            DrawCmd::PushLayer { .. } => open.push("layer".into()),
            DrawCmd::PushTransform(_) => open.push("transform".into()),
            DrawCmd::PopClip | DrawCmd::PopLayer | DrawCmd::PopTransform => {
                open.pop().expect("balanced");
            }
            DrawCmd::Quad(q) if q.rect.width == width => return open,
            _ => {}
        }
    }
    panic!("no quad {width} wide");
}

fn bg(color: &str) -> (&'static str, Value) {
    ("backgroundColor", color.into())
}

/// A menu bar (see [`menu`]) and its parts.
struct Menu {
    tree: UiTree,
    outer: u32,
    bar: u32,
    dropdown: u32,
}

/// A 300x300 box (styled `outer`) > a 100x40 `overflow: hidden` bar (styled `bar`) > a 30x20
/// red item, a static wrapper (styled `wrap`) holding a 90x120 absolute dropdown at
/// `left: 0; top: 30px`, and a 50x20 green item after it.
fn menu(outer: &[(&str, Value)], bar: &[(&str, Value)], wrap: &[(&str, Value)]) -> Menu {
    let mut d = Dom::new();
    let mut s = vec![("width", px(300.0)), ("height", px(300.0)), bg("gray")];
    s.extend_from_slice(outer);
    let outer = d.add(0, "box", &s);
    let mut s = vec![
        ("width", px(100.0)),
        ("height", px(40.0)),
        ("flexShrink", px(0.0)),
        ("overflow", "hidden".into()),
        bg("white"),
    ];
    s.extend_from_slice(bar);
    let bar = d.add(outer, "box", &s);
    d.add(
        bar,
        "box",
        &[("width", px(30.0)), ("height", px(20.0)), bg("red")],
    );
    let wrap = d.add(bar, "box", wrap);
    let dropdown = d.add(
        wrap,
        "box",
        &[
            ("position", "absolute".into()),
            ("left", px(0.0)),
            ("top", px(30.0)),
            ("width", px(90.0)),
            ("height", px(120.0)),
            bg("blue"),
        ],
    );
    d.add(
        bar,
        "box",
        &[("width", px(50.0)), ("height", px(20.0)), bg("green")],
    );
    Menu {
        tree: d.build(""),
        outer,
        bar,
        dropdown,
    }
}

fn relative() -> (&'static str, Value) {
    ("position", "relative".into())
}

// M3-CLIP-02
#[test]
fn an_absolute_box_escapes_the_clip_of_a_static_ancestor() {
    // Held by the relative outer box: the bar's clip doesn't reach it, its siblings stay
    // clipped, before and after it.
    let mut m = menu(&[relative()], &[], &[]);
    assert_eq!(around(&mut m.tree, 90.0), Vec::<String>::new());
    assert_eq!(around(&mut m.tree, 30.0), ["clip 100x40"]);
    assert_eq!(around(&mut m.tree, 50.0), ["clip 100x40"]);
    // A clipping containing block still clips it.
    let mut m = menu(&[relative(), ("overflow", "hidden".into())], &[], &[]);
    assert_eq!(around(&mut m.tree, 90.0), ["clip 300x300"]);
    assert_eq!(around(&mut m.tree, 50.0), ["clip 300x300", "clip 100x40"]);
    // A positioned bar is its containing block, and clips it.
    let mut m = menu(&[relative()], &[relative()], &[]);
    assert_eq!(around(&mut m.tree, 90.0), ["clip 100x40"]);
    // So is a transformed bar (rounded corners too).
    let mut m = menu(
        &[relative()],
        &[
            ("transform", "rotate(0deg)".into()),
            ("borderRadius", px(8.0)),
        ],
        &[],
    );
    assert_eq!(around(&mut m.tree, 90.0), ["transform", "clip 100x40"]);
    // With no positioned ancestor the root holds it: no clip at all.
    let mut m = menu(&[], &[], &[]);
    assert_eq!(around(&mut m.tree, 90.0), Vec::<String>::new());
}

// M3-CLIP-02
#[test]
fn clip_paths_and_layers_still_apply_to_an_escaping_box() {
    // A clip-path between the dropdown and its containing block clips everything inside.
    let mut m = menu(&[relative()], &[], &[("clipPath", "inset(-500px)".into())]);
    let clipped = around(&mut m.tree, 90.0);
    assert_eq!(clipped.len(), 1, "{clipped:?}");
    assert_ne!(clipped, ["clip 100x40"]);
    // An opacity group outside the bar still holds the dropdown, without the bar's clip.
    let mut d = Dom::new();
    let outer = d.add(
        0,
        "box",
        &[relative(), ("width", px(300.0)), ("height", px(300.0))],
    );
    let group = d.add(outer, "box", &[("opacity", px(0.5))]);
    let bar = d.add(
        group,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(40.0)),
            ("overflow", "hidden".into()),
            bg("white"),
        ],
    );
    d.add(
        bar,
        "box",
        &[
            ("position", "absolute".into()),
            ("top", px(30.0)),
            ("width", px(90.0)),
            ("height", px(120.0)),
            bg("blue"),
        ],
    );
    let mut t = d.build("");
    assert_eq!(around(&mut t, 90.0), ["layer"]);
    // Inside a group inside the clip, the clip stays (the group is clipped when composited).
    let mut m = menu(&[relative()], &[], &[("opacity", px(0.5))]);
    assert_eq!(around(&mut m.tree, 90.0), ["clip 100x40", "layer"]);
}

// M3-CLIP-03
#[test]
fn hit_testing_finds_an_escaping_box_outside_the_clip() {
    // (20, 100) is below the 40px bar, inside the dropdown (30..150).
    let mut m = menu(&[relative()], &[], &[]);
    assert_eq!(m.tree.hit_test(20.0, 100.0), Some(NodeId(m.dropdown)));
    // Over the bar too it is on top.
    assert_eq!(m.tree.hit_test(20.0, 35.0), Some(NodeId(m.dropdown)));
    // Clipped by a positioned bar: the point falls through to the outer box.
    let mut m = menu(&[relative()], &[relative()], &[]);
    assert_eq!(m.tree.hit_test(20.0, 100.0), Some(NodeId(m.outer)));
    // Its siblings stay clipped: in a 60px bar, (70, 10) is on the clipped-off green item.
    let mut m = menu(&[relative()], &[("width", px(60.0))], &[]);
    assert_eq!(m.tree.hit_test(70.0, 10.0), Some(NodeId(m.outer)));
    assert_eq!(m.tree.hit_test(20.0, 100.0), Some(NodeId(m.dropdown)));
}

// M3-CLIP-04
#[test]
fn an_escaping_box_does_not_scroll_with_a_static_scroller() {
    let mut m = menu(
        &[relative()],
        &[
            ("overflow", "auto".into()),
            ("flexDirection", "column".into()),
        ],
        &[("flexShrink", px(0.0)), ("height", px(60.0))],
    );
    let before = m.tree.rect(NodeId(m.dropdown)).unwrap();
    assert!(m.tree.scroll_to(NodeId(m.bar), 0.0, 10.0, false));
    m.tree.layout_if_needed();
    assert_eq!(m.tree.rect(NodeId(m.dropdown)), Some(before));
    // Its scroll chain skips the bar; the wheel over it scrolls nothing.
    let path = m.tree.scroll_path(NodeId(m.dropdown));
    assert!(!path.contains(&NodeId(m.bar)), "{path:?}");
    assert!(path.contains(&NodeId(m.outer)), "{path:?}");
    let mut ui = Interaction::new();
    let mut send = |tree: &mut UiTree, input: Input| ui.handle(tree, input, &mut |_| false);
    send(&mut m.tree, Input::PointerMove { x: 20.0, y: 100.0 });
    send(&mut m.tree, Input::Wheel { dx: 0.0, dy: 30.0 });
    assert_eq!(m.tree.scroll_offset(NodeId(m.bar)), (0.0, 10.0));
    // Over the bar's own items the wheel scrolls it.
    send(&mut m.tree, Input::PointerMove { x: 95.0, y: 5.0 });
    send(&mut m.tree, Input::Wheel { dx: 0.0, dy: 30.0 });
    assert_ne!(m.tree.scroll_offset(NodeId(m.bar)), (0.0, 10.0));
}
