// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Hit testing (M3-EVT-01): the topmost element under a point, respecting paint
//! order, rounded corners, `pointerEvents` and `display: none`.

use crate::common;

use common::{Dom, px};
use platypus_ui::UiTree;
use web_ui_core::NodeId;

fn hit(tree: &mut UiTree, x: f32, y: f32) -> Option<u32> {
    tree.hit_test(x, y).map(|id| id.0)
}

#[test]
fn topmost_child_wins_and_misses_fall_through() {
    let mut d = Dom::new();
    let outer = d.add(
        0,
        "box",
        &[
            ("width", px(200.0)),
            ("height", px(200.0)),
            ("padding", px(20.0)),
        ],
    );
    let inner = d.add(outer, "box", &[("width", px(50.0)), ("height", px(50.0))]);
    // An absolutely positioned later sibling overlapping `inner`: painted later, so on top.
    let over = d.add(
        outer,
        "box",
        &[
            ("position", "absolute".into()),
            ("left", px(40.0)),
            ("top", px(40.0)),
            ("width", px(50.0)),
            ("height", px(50.0)),
        ],
    );
    let mut tree = d.build("");
    assert_eq!(hit(&mut tree, 25.0, 25.0), Some(inner));
    assert_eq!(
        hit(&mut tree, 45.0, 45.0),
        Some(over),
        "later sibling is on top"
    );
    assert_eq!(hit(&mut tree, 150.0, 150.0), Some(outer));
    assert_eq!(hit(&mut tree, 300.0, 300.0), None, "only the root");
    assert_eq!(
        tree.event_path(NodeId(over)),
        vec![NodeId(over), NodeId(outer)]
    );
}

#[test]
fn children_outside_their_parent_still_hit() {
    let mut d = Dom::new();
    let parent = d.add(0, "box", &[("width", px(50.0)), ("height", px(50.0))]);
    let child = d.add(
        parent,
        "box",
        &[
            ("position", "absolute".into()),
            ("left", px(100.0)),
            ("width", px(20.0)),
            ("height", px(20.0)),
        ],
    );
    let mut tree = d.build("");
    assert_eq!(hit(&mut tree, 110.0, 10.0), Some(child));
}

#[test]
fn rounded_corners_are_respected() {
    let mut d = Dom::new();
    let circle = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("borderRadius", "50%".into()),
        ],
    );
    let mut tree = d.build("");
    assert_eq!(hit(&mut tree, 50.0, 50.0), Some(circle));
    assert_eq!(hit(&mut tree, 50.0, 2.0), Some(circle), "edge midpoint");
    assert_eq!(hit(&mut tree, 5.0, 5.0), None, "outside the top-left curve");
    assert_eq!(
        hit(&mut tree, 95.0, 95.0),
        None,
        "outside the bottom-right curve"
    );
}

// M3-BRD-02
#[test]
fn elliptical_corners_are_respected() {
    let mut d = Dom::new();
    let pill = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(40.0)),
            ("borderRadius", "50%".into()),
        ],
    );
    let mut tree = d.build("");
    assert_eq!(hit(&mut tree, 50.0, 20.0), Some(pill));
    assert_eq!(
        hit(&mut tree, 50.0, 1.0),
        Some(pill),
        "the top of the ellipse"
    );
    assert_eq!(
        hit(&mut tree, 12.0, 5.0),
        None,
        "inside a circle's corner, outside the ellipse"
    );
}

#[test]
fn pointer_events_none_is_inherited_and_can_be_reenabled() {
    let mut d = Dom::new();
    let base = d.add(0, "box", &[("width", px(300.0)), ("height", px(300.0))]);
    let overlay = d.add(
        base,
        "box",
        &[
            ("position", "absolute".into()),
            ("inset", px(0.0)),
            ("pointerEvents", "none".into()),
        ],
    );
    let passive = d.add(
        overlay,
        "box",
        &[("width", px(100.0)), ("height", px(100.0))],
    );
    let button = d.add(
        overlay,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("pointerEvents", "auto".into()),
        ],
    );
    let mut tree = d.build("");
    let _ = passive;
    assert_eq!(
        hit(&mut tree, 50.0, 50.0),
        Some(base),
        "overlay and its child pass through"
    );
    assert_eq!(
        hit(&mut tree, 150.0, 50.0),
        Some(button),
        "child opted back in"
    );
    assert_eq!(hit(&mut tree, 250.0, 250.0), Some(base));
}

#[test]
fn display_none_subtrees_are_skipped() {
    let mut d = Dom::new();
    let base = d.add(0, "box", &[("width", px(100.0)), ("height", px(100.0))]);
    let hidden = d.add(
        base,
        "box",
        &[
            ("position", "absolute".into()),
            ("inset", px(0.0)),
            ("display", "none".into()),
        ],
    );
    d.add(
        hidden,
        "box",
        &[("width", px(100.0)), ("height", px(100.0))],
    );
    let mut tree = d.build("");
    assert_eq!(hit(&mut tree, 50.0, 50.0), Some(base));
}

#[test]
fn text_hits_its_text_element_and_generated_content_its_owner() {
    let mut d = Dom::new();
    let label = d.add(0, "text", &[]);
    d.text(label, "Hello");
    let icon = d.add(0, "box", &[("width", px(100.0)), ("height", px(40.0))]);
    let mut tree =
        d.build(r#"box::before { content: ""; width: 30px; height: 30px; background-color: red }"#);
    let text_rect = tree.rect(NodeId(label)).unwrap();
    assert_eq!(
        hit(&mut tree, text_rect.x + 5.0, text_rect.y + 5.0),
        Some(label)
    );
    let icon_rect = tree.rect(NodeId(icon)).unwrap();
    assert_eq!(
        hit(&mut tree, icon_rect.x + 10.0, icon_rect.y + 10.0),
        Some(icon),
        "the ::before box belongs to its element"
    );
}

// M3-EVT-07
#[test]
fn a_span_is_hit_over_its_own_characters() {
    let mut d = Dom::new();
    let para = d.add(0, "text", &[("fontSize", px(20.0)), ("width", px(400.0))]);
    d.text(para, "Read ");
    let link = d.add(para, "span", &[]);
    d.text(link, "the docs");
    d.text(para, " now");
    let quiet = d.add(para, "span", &[("pointerEvents", "none".into())]);
    d.text(quiet, " later");
    let mut tree = d.build("");
    let r = tree.rect(NodeId(para)).unwrap();
    let y = r.y + r.height / 2.0;
    assert_eq!(hit(&mut tree, r.x + 3.0, y), Some(para), "plain text");
    // "Read " is about 50px wide at 20px; "the docs" follows it.
    assert_eq!(hit(&mut tree, r.x + 80.0, y), Some(link));
    assert_eq!(
        tree.event_path(NodeId(link)),
        vec![NodeId(link), NodeId(para)]
    );
    assert_eq!(
        hit(&mut tree, r.x + 390.0, y),
        Some(para),
        "past the end of the line"
    );
    let layout = tree.text_layout(NodeId(para)).unwrap();
    let end = layout.width();
    assert_eq!(
        hit(&mut tree, r.x + end - 3.0, y),
        Some(para),
        "a span with pointerEvents: none passes to its text"
    );
}

#[test]
fn opacity_zero_still_hits() {
    // CSS: transparent elements are still targets (unlike `visibility: hidden`).
    let mut d = Dom::new();
    let ghost = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("opacity", px(0.0)),
        ],
    );
    let mut tree = d.build("");
    assert_eq!(hit(&mut tree, 50.0, 50.0), Some(ghost));
}
