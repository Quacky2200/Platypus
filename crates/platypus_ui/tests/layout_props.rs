// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Layout properties from M3d group 2: `place-*` shorthands, grid extras
//! (`grid-template-areas`, `grid-area`, `grid-template`, auto tracks, line
//! longhands) and logical properties.

use crate::common;

use common::{Dom, px};
use platypus_ui::{Rect, UiTree};
use web_ui_core::{NodeId, Value};

fn rect(tree: &mut UiTree, id: u32) -> Rect {
    tree.layout_if_needed();
    tree.rect(NodeId(id)).unwrap()
}

/// A 300x200 grid container with `container` styles and one child per entry of
/// `children`.
fn grid(container: &[(&str, Value)], children: &[&[(&str, Value)]]) -> (UiTree, Vec<u32>) {
    let mut d = Dom::new();
    let mut style = vec![
        ("display", Value::from("grid")),
        ("width", px(300.0)),
        ("height", px(200.0)),
    ];
    style.extend_from_slice(container);
    let g = d.add(0, "box", &style);
    let kids = children.iter().map(|s| d.add(g, "box", s)).collect();
    (d.build(""), kids)
}

#[test]
fn template_areas_and_grid_area_place_items_by_name() {
    let (mut tree, k) = grid(
        &[
            ("gridTemplateAreas", "\"head head\" \"side main\"".into()),
            ("gridTemplateColumns", "100px 1fr".into()),
            ("gridTemplateRows", "50px 1fr".into()),
        ],
        &[
            &[("gridArea", "main".into())],
            &[("gridArea", "head".into())],
            &[("gridArea", "side".into())],
        ],
    );
    assert_eq!(
        rect(&mut tree, k[0]),
        Rect::new(100.0, 50.0, 200.0, 150.0),
        "main"
    );
    assert_eq!(
        rect(&mut tree, k[1]),
        Rect::new(0.0, 0.0, 300.0, 50.0),
        "head spans both columns"
    );
    assert_eq!(
        rect(&mut tree, k[2]),
        Rect::new(0.0, 50.0, 100.0, 150.0),
        "side"
    );
}

#[test]
fn grid_template_shorthand_with_areas_and_sizes() {
    let (mut tree, k) = grid(
        &[("gridTemplate", "\"a b\" 40px \"c c\" 1fr / 1fr 2fr".into())],
        &[&[("gridArea", "b".into())], &[("gridArea", "c".into())]],
    );
    assert_eq!(rect(&mut tree, k[0]), Rect::new(100.0, 0.0, 200.0, 40.0));
    assert_eq!(rect(&mut tree, k[1]), Rect::new(0.0, 40.0, 300.0, 160.0));

    let (mut tree, k) = grid(
        &[("gridTemplate", "60px 1fr / 1fr 1fr 1fr".into())],
        &[&[], &[], &[], &[]],
    );
    assert_eq!(
        rect(&mut tree, k[3]),
        Rect::new(0.0, 60.0, 100.0, 140.0),
        "4th item wraps to row 2"
    );
}

#[test]
fn grid_area_with_lines_and_line_longhands() {
    let (mut tree, k) = grid(
        &[
            ("gridTemplateColumns", "repeat(3, 100px)".into()),
            ("gridTemplateRows", "repeat(2, 100px)".into()),
        ],
        &[
            &[("gridArea", "2 / 1 / 3 / 3".into())],
            &[
                ("gridColumnStart", "3".into()),
                ("gridRowStart", "1".into()),
                ("gridRowEnd", "span 2".into()),
            ],
        ],
    );
    assert_eq!(rect(&mut tree, k[0]), Rect::new(0.0, 100.0, 200.0, 100.0));
    assert_eq!(rect(&mut tree, k[1]), Rect::new(200.0, 0.0, 100.0, 200.0));
}

#[test]
fn auto_tracks_size_implicit_rows() {
    let (mut tree, k) = grid(
        &[
            ("gridTemplateColumns", "1fr 1fr".into()),
            ("gridAutoRows", "30px".into()),
            ("alignContent", "start".into()),
        ],
        &[&[], &[], &[]],
    );
    assert_eq!(rect(&mut tree, k[2]).y, 30.0);
    assert_eq!(rect(&mut tree, k[2]).height, 30.0);
}

#[test]
fn place_shorthands_set_both_axes() {
    let (mut tree, k) = grid(
        &[
            ("placeItems", "center".into()),
            ("gridTemplateColumns", "1fr".into()),
        ],
        &[&[("width", px(50.0)), ("height", px(20.0))]],
    );
    assert_eq!(rect(&mut tree, k[0]), Rect::new(125.0, 90.0, 50.0, 20.0));

    let (mut tree, k) = grid(
        &[],
        &[&[
            ("width", px(50.0)),
            ("height", px(20.0)),
            ("placeSelf", "end start".into()),
        ]],
    );
    assert_eq!(
        rect(&mut tree, k[0]),
        Rect::new(0.0, 180.0, 50.0, 20.0),
        "align end, justify start"
    );
}

#[test]
fn bad_grid_values_are_rejected() {
    let mut d = Dom::new();
    let a = d.add(0, "box", &[]);
    let mut tree = d.build("");
    for (prop, value) in [
        ("gridTemplateAreas", "\"a b\" \"b a\""),
        ("gridTemplateAreas", "\"a a\" \"b\""),
        ("gridArea", "1 / 2 / 3 / 4 / 5"),
        ("placeItems", "center start end"),
        ("marginInline", "1px 2px 3px"),
    ] {
        let errors = tree.apply_commit(&web_ui_core::Commit {
            ops: vec![common::op(
                platypus_ui::opcodes::SET_STYLE,
                &[common::n(a), prop.into(), value.into()],
            )],
        });
        assert!(!errors.is_empty(), "{prop}: {value} should be rejected");
    }
}

#[test]
fn logical_properties_map_left_to_right() {
    let mut d = Dom::new();
    let col = d.add(
        0,
        "box",
        &[
            ("flexDirection", "column".into()),
            ("alignItems", "flex-start".into()),
        ],
    );
    let a = d.add(
        col,
        "box",
        &[
            ("inlineSize", px(80.0)),
            ("blockSize", px(20.0)),
            ("marginBlock", "5px 7px".into()),
            ("marginInlineStart", px(11.0)),
            ("paddingInline", px(4.0)),
        ],
    );
    let b = d.add(
        col,
        "box",
        &[("inlineSize", px(10.0)), ("blockSize", px(10.0))],
    );
    let mut tree = d.build("");
    assert_eq!(rect(&mut tree, a), Rect::new(11.0, 5.0, 80.0, 20.0));
    assert_eq!(rect(&mut tree, b).y, 32.0, "5 + 20 + 7");
}

/// A 300x100 flex row of 100px boxes, one per entry of `children`.
fn row(children: &[&[(&str, Value)]]) -> (UiTree, Vec<u32>) {
    let mut d = Dom::new();
    let r = d.add(0, "box", &[("width", px(300.0)), ("height", px(100.0))]);
    let kids = children
        .iter()
        .map(|s| {
            let mut style = vec![("width", px(100.0)), ("height", px(100.0))];
            style.extend_from_slice(s);
            d.add(r, "box", &style)
        })
        .collect();
    (d.build(""), kids)
}

fn set_style(tree: &mut UiTree, id: u32, prop: &str, value: Value) {
    let errors = tree.apply_commit(&web_ui_core::Commit {
        ops: vec![common::op(
            platypus_ui::opcodes::SET_STYLE,
            &[common::n(id), prop.into(), value],
        )],
    });
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn order_reorders_flex_items_stably() {
    let (mut tree, k) = row(&[&[("order", 1.0.into())], &[], &[("order", "-1".into())]]);
    assert_eq!(rect(&mut tree, k[2]).x, 0.0);
    assert_eq!(rect(&mut tree, k[1]).x, 100.0);
    assert_eq!(rect(&mut tree, k[0]).x, 200.0);

    set_style(&mut tree, k[0], "order", Value::Null);
    assert_eq!(rect(&mut tree, k[2]).x, 0.0, "still first");
    assert_eq!(rect(&mut tree, k[0]).x, 100.0, "then document order");
    assert_eq!(rect(&mut tree, k[1]).x, 200.0);

    // From a stylesheet too, through restyle.
    let mut d = Dom::new();
    let r = d.add(0, "box", &[("width", px(200.0))]);
    let a = d.add(r, "box", &[("width", px(100.0)), ("height", px(10.0))]);
    let b = d.add(r, "box", &[("width", px(100.0)), ("height", px(10.0))]);
    d.set_class(a, "late");
    let mut tree = d.build(".late { order: 2 }");
    assert_eq!(rect(&mut tree, b).x, 0.0);
    assert_eq!(rect(&mut tree, a).x, 100.0);
}

/// Background colours of the painted quads, in paint order.
fn paint_reds(tree: &mut UiTree) -> Vec<u8> {
    tree.draw_list()
        .iter()
        .filter_map(|c| match c {
            platypus_ui::DrawCmd::Quad(q) => Some((q.background.r * 255.0).round() as u8),
            _ => None,
        })
        .collect()
}

#[test]
fn z_index_sets_paint_and_hit_order_among_siblings() {
    // Three stacked boxes at the same spot; reds 1, 2, 3 identify them.
    let mut d = Dom::new();
    let r = d.add(0, "box", &[("width", px(100.0)), ("height", px(100.0))]);
    let boxes: Vec<u32> = [("2", 1.0), ("auto", 2.0), ("-1", 3.0)]
        .iter()
        .map(|(z, red)| {
            d.add(
                r,
                "box",
                &[
                    ("position", "absolute".into()),
                    ("width", px(50.0)),
                    ("height", px(50.0)),
                    ("zIndex", (*z).into()),
                    ("backgroundColor", format!("rgb({red} 0 0)").into()),
                ],
            )
        })
        .collect();
    let mut tree = d.build("");
    tree.layout_if_needed();
    assert_eq!(paint_reds(&mut tree), vec![3, 2, 1], "-1, auto, then 2");
    assert_eq!(tree.hit_test(10.0, 10.0).map(|n| n.0), Some(boxes[0]));

    set_style(&mut tree, boxes[0], "zIndex", "auto".into());
    tree.layout_if_needed();
    assert_eq!(
        paint_reds(&mut tree),
        vec![3, 1, 2],
        "ties keep document order"
    );
    assert_eq!(tree.hit_test(10.0, 10.0).map(|n| n.0), Some(boxes[1]));
}

#[test]
fn order_props_accept_and_reject_values() {
    let (mut tree, k) = row(&[&[]]);
    set_style(&mut tree, k[0], "isolation", "isolate".into());
    set_style(&mut tree, k[0], "isolation", "auto".into());
    set_style(&mut tree, k[0], "zIndex", 5.0.into());
    for (prop, value) in [
        ("order", Value::from("auto")),
        ("order", Value::from(1.5)),
        ("zIndex", Value::from("high")),
        ("isolation", Value::from("separate")),
    ] {
        let errors = tree.apply_commit(&web_ui_core::Commit {
            ops: vec![common::op(
                platypus_ui::opcodes::SET_STYLE,
                &[common::n(k[0]), prop.into(), value.clone()],
            )],
        });
        assert!(!errors.is_empty(), "{prop}: {value:?} should be rejected");
    }
}

fn red(n: u8) -> (&'static str, Value) {
    ("backgroundColor", format!("rgb({n} 0 0)").into())
}

#[test]
fn display_contents_children_join_the_parent_layout() {
    let mut d = Dom::new();
    let r = d.add(0, "box", &[("width", px(300.0)), ("height", px(200.0))]);
    let size = [("width", px(100.0)), ("height", px(100.0))];
    let a = d.add(r, "box", &size);
    let wrap = d.add(
        r,
        "box",
        &[
            ("display", "contents".into()),
            ("flexDirection", "column".into()),
            ("padding", px(30.0)),
            red(9),
        ],
    );
    let b = d.add(wrap, "box", &size);
    let mut c_style = size.to_vec();
    c_style.push(("order", "-1".into()));
    let c = d.add(wrap, "box", &c_style);
    let mut tree = d.build("");

    assert_eq!(
        rect(&mut tree, c).x,
        0.0,
        "order applies across the wrapper"
    );
    assert_eq!(rect(&mut tree, a).x, 100.0);
    assert_eq!(rect(&mut tree, b), Rect::new(200.0, 0.0, 100.0, 100.0));
    assert!(
        !paint_reds(&mut tree).contains(&9),
        "the wrapper paints no box"
    );
    assert_eq!(tree.hit_test(250.0, 50.0).map(|n| n.0), Some(b));
    assert_eq!(
        tree.event_path(NodeId(b))[1],
        NodeId(wrap),
        "still in the DOM"
    );

    set_style(&mut tree, wrap, "display", Value::Null);
    assert_eq!(rect(&mut tree, wrap).x, 100.0, "a box again");
    assert_eq!(
        rect(&mut tree, b),
        Rect::new(130.0, 100.0, 100.0, 70.0),
        "second in the padded column; both shrink to fit"
    );
}

#[test]
fn position_fixed_uses_the_window_and_paints_on_top() {
    let mut d = Dom::new();
    let clip = d.add(
        0,
        "box",
        &[
            ("margin", px(50.0)),
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("overflow", "hidden".into()),
            red(1),
        ],
    );
    let fixed = d.add(
        clip,
        "box",
        &[
            ("position", "fixed".into()),
            ("left", px(10.0)),
            ("top", px(10.0)),
            ("width", px(60.0)),
            ("height", px(60.0)),
            red(2),
        ],
    );
    let cover = d.add(
        0,
        "box",
        &[
            ("position", "absolute".into()),
            ("width", px(400.0)),
            ("height", px(400.0)),
            red(3),
        ],
    );
    let mut tree = d.build("");

    assert_eq!(rect(&mut tree, fixed), Rect::new(10.0, 10.0, 60.0, 60.0));
    assert_eq!(paint_reds(&mut tree), vec![1, 3, 2], "fixed paints last");
    assert_eq!(
        tree.hit_test(15.0, 15.0).map(|n| n.0),
        Some(fixed),
        "outside its clipping parent, above a later sibling"
    );
    assert_eq!(tree.hit_test(300.0, 300.0).map(|n| n.0), Some(cover));
    set_style(&mut tree, fixed, "position", Value::Null);
    assert_eq!(rect(&mut tree, fixed).x, 50.0, "back in its parent's flow");
    assert_eq!(paint_reds(&mut tree), vec![1, 2, 3]);
}

#[test]
fn contents_and_fixed_from_stylesheets() {
    let mut d = Dom::new();
    let r = d.add(0, "box", &[("width", px(200.0)), ("height", px(50.0))]);
    let wrap = d.add(r, "box", &[("padding", px(20.0))]);
    let a = d.add(wrap, "box", &[("width", px(50.0)), ("height", px(50.0))]);
    let f = d.add(wrap, "box", &[("width", px(5.0)), ("height", px(5.0))]);
    d.set_class(wrap, "w");
    d.set_class(f, "f");
    let mut tree = d.build(".w { display: contents } .f { position: fixed; right: 0; bottom: 0 }");
    assert_eq!(rect(&mut tree, a).x, 0.0, "no wrapper padding");
    assert_eq!(rect(&mut tree, f), Rect::new(395.0, 395.0, 5.0, 5.0));
}

// M3-XF-02
#[test]
fn hit_testing_follows_transforms() {
    let mut d = Dom::new();
    let rotated = d.add(
        0,
        "box",
        &[
            ("position", "absolute".into()),
            ("left", px(100.0)),
            ("top", px(100.0)),
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("transform", "rotate(45deg)".into()),
        ],
    );
    let mut tree = d.build("");
    let hit = |tree: &mut UiTree, x: f32, y: f32| tree.hit_test(x, y).map(|n| n.0);
    assert_eq!(hit(&mut tree, 150.0, 150.0), Some(rotated), "the centre");
    assert_eq!(hit(&mut tree, 103.0, 103.0), None, "a corner turned away");
    assert_eq!(
        hit(&mut tree, 150.0, 85.0),
        Some(rotated),
        "a tip beyond the layout box"
    );
    assert_eq!(
        tree.rect(NodeId(rotated)).unwrap().x,
        100.0,
        "layout is unchanged"
    );

    // Children move with their transformed parent; 3D turns squash it.
    let mut d = Dom::new();
    let card = d.add(
        0,
        "box",
        &[
            ("position", "absolute".into()),
            ("left", px(0.0)),
            ("top", px(0.0)),
            ("width", px(100.0)),
            ("height", px(50.0)),
            ("transform", "translateX(200px) rotateY(60deg)".into()),
        ],
    );
    let child = d.add(card, "box", &[("width", px(100.0)), ("height", px(50.0))]);
    let mut tree = d.build("");
    assert_eq!(hit(&mut tree, 250.0, 25.0), Some(child));
    assert_eq!(
        hit(&mut tree, 220.0, 25.0),
        None,
        "squashed to half its width"
    );
    assert_eq!(hit(&mut tree, 50.0, 25.0), None, "moved away");

    // A back-facing `backface-visibility: hidden` element can't be hit.
    let mut d = Dom::new();
    d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("transform", "rotateY(180deg)".into()),
            ("backfaceVisibility", "hidden".into()),
        ],
    );
    let mut tree = d.build("");
    assert_eq!(hit(&mut tree, 50.0, 50.0), None);
    let _ = card;
}

// M3-XF-02
#[test]
fn transform_properties_parse_and_reset() {
    let (mut tree, k) = row(&[&[]]);
    for (prop, value) in [
        (
            "transform",
            "rotate(0.5turn) translate(12px, 50%) scale(2, 0.5)",
        ),
        ("transform", "translateY(3in)"),
        (
            "transform",
            "matrix3d(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)",
        ),
        ("translate", "10px 20px"),
        ("rotate", "x 30deg"),
        ("scale", "1.5"),
        ("transformOrigin", "left top"),
        ("perspective", "500px"),
        ("perspectiveOrigin", "center"),
        ("transformStyle", "preserve-3d"),
        ("backfaceVisibility", "hidden"),
        ("transform", "inherit"),
        ("transform", "none"),
    ] {
        set_style(&mut tree, k[0], prop, value.into());
    }
    for (prop, value) in [
        ("transform", "rotate(10)"),
        ("transformStyle", "3d"),
        ("perspective", "-1px"),
    ] {
        let errors = tree.apply_commit(&web_ui_core::Commit {
            ops: vec![common::op(
                platypus_ui::opcodes::SET_STYLE,
                &[common::n(k[0]), prop.into(), value.into()],
            )],
        });
        assert!(!errors.is_empty(), "{prop}: {value}");
    }
}

// M3-XF-02
#[test]
fn preserve_3d_combines_turns_before_flattening() {
    // A parent turned 60° about y and a child turned back -60°: in 3D they cancel;
    // flattened one after the other, the child is squashed twice.
    let child_hit = |style: &str| {
        let mut d = Dom::new();
        let parent = d.add(
            0,
            "box",
            &[
                ("width", px(100.0)),
                ("height", px(50.0)),
                ("transform", "rotateY(60deg)".into()),
                ("transformStyle", style.into()),
            ],
        );
        let child = d.add(
            parent,
            "box",
            &[
                ("width", px(100.0)),
                ("height", px(50.0)),
                ("transform", "rotateY(-60deg)".into()),
            ],
        );
        let mut tree = d.build("");
        tree.hit_test(95.0, 25.0).map(|n| n.0) == Some(child)
    };
    assert!(child_hit("preserve-3d"), "the turns cancel: full width");
    assert!(!child_hit("flat"), "flattened twice: squashed");
}

// M3-VIS-01
#[test]
fn visibility_hidden_keeps_space_but_draws_and_hits_nothing() {
    let mut d = Dom::new();
    let col = d.add(0, "box", &[("flexDirection", "column".into())]);
    let hidden = d.add(
        col,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(40.0)),
            ("backgroundColor", "red".into()),
            ("visibility", "hidden".into()),
        ],
    );
    // A child that opts back in.
    let shown = d.add(
        hidden,
        "box",
        &[
            ("width", px(20.0)),
            ("height", px(20.0)),
            ("backgroundColor", "blue".into()),
            ("visibility", "visible".into()),
        ],
    );
    let after = d.add(col, "box", &[("width", px(100.0)), ("height", px(10.0))]);
    let mut tree = d.build("");
    assert_eq!(
        rect(&mut tree, after).y,
        40.0,
        "the hidden box keeps its space"
    );
    let colours: Vec<_> = tree
        .draw_list()
        .iter()
        .filter_map(|c| match c {
            platypus_ui::DrawCmd::Quad(q) => Some(q.background),
            _ => None,
        })
        .collect();
    assert_eq!(colours.len(), 1, "only the visible child: {colours:?}");
    assert_eq!(colours[0].b, 1.0);
    assert_eq!(
        tree.hit_test(50.0, 30.0).map(|n| n.0),
        Some(col),
        "the hidden box isn't hit: the column behind it is"
    );
    assert_eq!(tree.hit_test(10.0, 10.0).map(|n| n.0), Some(shown));
}

// M3-VIS-01
#[test]
fn hidden_text_and_spans() {
    let mut d = Dom::new();
    let t = d.add(0, "text", &[("visibility", "hidden".into())]);
    d.text(t, "gone ");
    let span = d.add(t, "span", &[("visibility", "visible".into())]);
    d.text(span, "here");
    let mut tree = d.build("");
    let glyphs: usize = tree
        .draw_list()
        .iter()
        .filter_map(|c| match c {
            platypus_ui::DrawCmd::Text(t) => {
                Some(t.runs.iter().map(|r| r.glyphs.len()).sum::<usize>())
            }
            _ => None,
        })
        .sum();
    assert_eq!(glyphs, 4, "only the visible span's glyphs");
}
