// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Clipping (M3-EVT-05, M3-EVT-01): `overflow` clips children to the padding box with
//! inner radii, `clipPath` / `clip` clip the element itself, the draw list brackets
//! the right commands with `PushClip` / `PopClip`, and hit testing matches.

use crate::common;

use common::{Dom, px};
use platypus_ui::{ClipShape, DrawCmd, Rect, UiTree};
use web_ui_core::{NodeId, Value};

/// Clips and quads in order, e.g. `["clip 100x100", "quad 50", "unclip"]`.
fn shape(tree: &mut UiTree) -> Vec<String> {
    tree.draw_list()
        .iter()
        .filter_map(|c| match c {
            DrawCmd::PushClip(c) => Some(format!("clip {}x{}", c.rect.width, c.rect.height)),
            DrawCmd::PopClip => Some("unclip".into()),
            DrawCmd::Quad(q) => Some(format!("quad {}", q.rect.width)),
            DrawCmd::BoxShadow(_) => Some("shadow".into()),
            _ => None,
        })
        .collect()
}

fn clips(tree: &mut UiTree) -> Vec<ClipShape> {
    tree.draw_list()
        .into_iter()
        .filter_map(|c| match c {
            DrawCmd::PushClip(c) => Some(c),
            _ => None,
        })
        .collect()
}

fn bg() -> (&'static str, Value) {
    ("backgroundColor", "red".into())
}

/// A 100x100 box with `style`, holding a 150x150 child.
fn parent_with_big_child(style: &[(&str, Value)]) -> (UiTree, u32, u32) {
    let mut d = Dom::new();
    let mut s = vec![("width", px(100.0)), ("height", px(100.0)), bg()];
    s.extend_from_slice(style);
    let parent = d.add(0, "box", &s);
    let child = d.add(
        parent,
        "box",
        &[
            ("width", px(150.0)),
            ("height", px(150.0)),
            ("flexShrink", px(0.0)),
            bg(),
        ],
    );
    (d.build(""), parent, child)
}

// M3-EVT-05
#[test]
fn overflow_hidden_clips_children_to_the_padding_box_with_inner_radii() {
    let (mut tree, _, _) = parent_with_big_child(&[
        ("overflow", "hidden".into()),
        ("borderRadius", px(20.0)),
        ("border", "4px solid black".into()),
        ("boxShadow", "0 2px 4px black".into()),
    ]);
    assert_eq!(
        shape(&mut tree),
        ["shadow", "quad 100", "clip 92x92", "quad 150", "unclip"],
        "the box and its shadow aren't clipped, its child is"
    );
    let clip = clips(&mut tree)[0].clone();
    assert_eq!(clip.rect, Rect::new(4.0, 4.0, 92.0, 92.0));
    assert_eq!(clip.radii_x, [16.0; 4], "outer radius minus the border");
}

// M3-EVT-05
#[test]
fn every_clipping_overflow_value_clips_and_visible_does_not() {
    for value in ["hidden", "clip", "scroll", "auto"] {
        let (mut tree, _, _) = parent_with_big_child(&[("overflow", value.into())]);
        assert_eq!(clips(&mut tree).len(), 1, "{value}");
    }
    let (mut tree, _, _) = parent_with_big_child(&[("overflow", "visible".into())]);
    assert!(clips(&mut tree).is_empty());
    let (mut tree, _, _) = parent_with_big_child(&[]);
    assert!(clips(&mut tree).is_empty(), "visible by default");
}

// M3-EVT-05
#[test]
fn overflow_clip_on_one_axis_leaves_the_other_unbounded() {
    let (mut tree, _, _) =
        parent_with_big_child(&[("overflowX", "clip".into()), ("borderRadius", px(10.0))]);
    let clip = clips(&mut tree)[0].clone();
    assert_eq!((clip.rect.x, clip.rect.width), (0.0, 100.0));
    assert!(clip.rect.height > 1.0e6, "y isn't clipped");
    assert_eq!(clip.radii_x, [0.0; 4], "no corners on a one-axis clip");

    // `hidden` on one axis makes `visible` on the other compute to `auto`.
    let (mut tree, _, _) = parent_with_big_child(&[("overflowX", "hidden".into())]);
    assert_eq!(clips(&mut tree)[0].rect, Rect::new(0.0, 0.0, 100.0, 100.0));
}

// M3-EVT-05
#[test]
fn clip_path_clips_the_element_and_its_children() {
    let (mut tree, _, _) = parent_with_big_child(&[
        ("clipPath", "circle(50%)".into()),
        ("boxShadow", "0 2px 4px black".into()),
    ]);
    assert_eq!(
        shape(&mut tree),
        ["clip 100x100", "shadow", "quad 100", "quad 150", "unclip"],
        "clip-path wraps the shadow too"
    );
    let clip = clips(&mut tree)[0].clone();
    // 50% of the normalized diagonal of a square is half its side.
    assert_eq!(clip.radii_x, [50.0; 4]);
    assert_eq!(clip.radii_y, [50.0; 4]);
}

// M3-EVT-05
#[test]
fn clip_path_shapes_resolve_against_the_reference_box() {
    let cases: [(&str, Rect, [f32; 4], [f32; 4]); 6] = [
        (
            "inset(10px 20% round 5px)",
            Rect::new(20.0, 10.0, 60.0, 80.0),
            [5.0; 4],
            [5.0; 4],
        ),
        (
            "circle(20px at 30px 40px)",
            Rect::new(10.0, 20.0, 40.0, 40.0),
            [20.0; 4],
            [20.0; 4],
        ),
        (
            "circle(closest-side at 20% 50%)",
            Rect::new(0.0, 30.0, 40.0, 40.0),
            [20.0; 4],
            [20.0; 4],
        ),
        (
            "ellipse(40px 20px)",
            Rect::new(10.0, 30.0, 80.0, 40.0),
            [40.0; 4],
            [20.0; 4],
        ),
        (
            "ellipse(closest-side farthest-side at left top)",
            Rect::new(0.0, -100.0, 0.0, 200.0),
            [0.0; 4],
            [100.0; 4],
        ),
        (
            "padding-box",
            Rect::new(5.0, 5.0, 90.0, 90.0),
            [7.0; 4],
            [7.0; 4],
        ),
    ];
    for (value, rect, rx, ry) in cases {
        let (mut tree, _, _) = parent_with_big_child(&[
            ("clipPath", value.into()),
            ("border", "5px solid black".into()),
            ("borderRadius", px(12.0)),
        ]);
        let clip = clips(&mut tree)[0].clone();
        assert_eq!(
            (clip.rect, clip.radii_x, clip.radii_y),
            (rect, rx, ry),
            "{value}"
        );
    }
}

#[test]
fn invalid_clip_values_are_rejected() {
    for value in [
        "polygon(0 0, 10px 0)",
        "polygon(0 0, 10px, 0 10px)",
        "path('L 10 10')",
        "path(M 0 0 L 10 0 L 0 10)",
        // `url(#id)` is accepted: it refers to an SVG `clipPath` (M3-SVG-02).
        "circle(",
        "inset()",
        "banana",
        "border-box border-box",
    ] {
        let mut d = Dom::new();
        d.add(0, "box", &[]);
        let mut tree = d.build("");
        let errors = tree.apply_commit(&web_ui_core::Commit {
            ops: vec![common::op(
                platypus_ui::opcodes::SET_STYLE,
                &[common::n(1), "clipPath".into(), value.into()],
            )],
        });
        assert!(!errors.is_empty(), "{value} should be rejected");
    }
}

// M3-EVT-05
#[test]
fn legacy_clip_rect_applies_to_absolutely_positioned_elements_only() {
    let mut d = Dom::new();
    let style = |position: &str| {
        vec![
            ("position", Value::from(position)),
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("left", px(10.0)),
            ("clip", "rect(5px, auto, 50px, 20px)".into()),
            bg(),
        ]
    };
    d.add(0, "box", &style("absolute"));
    d.add(0, "box", &style("relative"));
    let mut tree = d.build("");
    let clips = clips(&mut tree);
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].rect, Rect::new(30.0, 5.0, 80.0, 45.0));
}

// M3-EVT-01, M3-EVT-05
#[test]
fn hit_testing_ignores_clipped_away_parts() {
    // A child sticking out of an overflow: hidden parent can't be hit outside it.
    let (mut tree, _, child) = parent_with_big_child(&[("overflow", "hidden".into())]);
    assert_eq!(tree.hit_test(50.0, 50.0), Some(NodeId(child)));
    assert_eq!(
        tree.hit_test(120.0, 120.0),
        None,
        "clipped part of the child"
    );

    // Inner rounded corners clip too.
    let (mut tree, _, _) =
        parent_with_big_child(&[("overflow", "hidden".into()), ("borderRadius", px(40.0))]);
    assert_eq!(tree.hit_test(3.0, 3.0), None, "outside the rounded clip");

    // clip-path removes the element itself outside the shape.
    let (mut tree, _, child) = parent_with_big_child(&[("clipPath", "circle(50%)".into())]);
    assert_eq!(tree.hit_test(50.0, 50.0), Some(NodeId(child)));
    assert_eq!(tree.hit_test(5.0, 5.0), None, "outside the circle");
    assert_eq!(
        tree.hit_test(120.0, 50.0),
        None,
        "children are clipped by it too"
    );
}

// M3-CLIP-01
#[test]
fn polygon_and_path_clips_resolve_against_the_reference_box() {
    let mut d = Dom::new();
    let a = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("clipPath", "polygon(50% 0, 100% 100%, 0 100%)".into()),
        ],
    );
    let mut tree = d.build("");
    let clip = clips(&mut tree)[0].clone();
    assert_eq!(
        clip.rect,
        Rect::new(0.0, 0.0, 100.0, 100.0),
        "the bounding box"
    );
    let polygon = clip.polygon.as_ref().expect("a polygon");
    assert_eq!(
        polygon.contours[0],
        vec![(50.0, 0.0), (100.0, 100.0), (0.0, 100.0)]
    );
    // Hit testing follows the triangle.
    assert_eq!(tree.hit_test(50.0, 80.0).map(|n| n.0), Some(a));
    assert_eq!(tree.hit_test(10.0, 10.0), None, "outside the triangle");

    // path() is in px from the box's corner; evenodd leaves the inner square open.
    let mut d = Dom::new();
    let b = d.add(
        0,
        "box",
        &[
            ("marginLeft", px(20.0)),
            ("width", px(100.0)),
            ("height", px(100.0)),
            (
                "clipPath",
                "path(evenodd, 'M0 0 H100 V100 H0 Z M25 25 H75 V75 H25 Z')".into(),
            ),
        ],
    );
    let mut tree = d.build("");
    let clip = clips(&mut tree)[0].clone();
    assert_eq!(clip.rect, Rect::new(20.0, 0.0, 100.0, 100.0));
    assert_eq!(tree.hit_test(30.0, 10.0).map(|n| n.0), Some(b), "the frame");
    assert_eq!(tree.hit_test(70.0, 50.0), None, "the hole");
}

#[test]
fn clip_shapes_contain_points_like_they_draw() {
    let ellipse = ClipShape {
        rect: Rect::new(0.0, 0.0, 100.0, 50.0),
        radii_x: [50.0; 4],
        radii_y: [25.0; 4],
        polygon: None,
    };
    assert!(ellipse.contains(50.0, 25.0));
    assert!(ellipse.contains(2.0, 25.0), "left tip");
    assert!(!ellipse.contains(10.0, 5.0), "outside the curve");
    assert!(!ellipse.contains(100.0, 25.0), "right edge excluded");
    let square = ClipShape::rect(Rect::new(0.0, 0.0, 10.0, 10.0));
    assert!(square.contains(0.0, 0.0) && !square.contains(10.0, 5.0));
}
