// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// SPDX-License-Identifier: Apache-2.0

//! CSS floats: `float` and `clear` on blocks, and text lines that wrap around them.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::opcodes::SET_STYLE;
use platypus_ui::{DrawCmd, Rect, UiTree};
use web_ui_core::{Commit, NodeId, Value};

const WORDS: &str = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi \
    omicron pi rho sigma tau upsilon phi chi psi omega alpha beta gamma delta epsilon zeta";

fn rect(tree: &UiTree, id: u32) -> Rect {
    tree.rect(NodeId(id)).unwrap()
}

/// The left edge and right edge of each line of the first text command, top to bottom.
fn lines(tree: &mut UiTree) -> Vec<(f32, f32, f32)> {
    let cmd = tree
        .draw_list()
        .into_iter()
        .find_map(|c| match c {
            DrawCmd::Text(t) => Some(t),
            _ => None,
        })
        .expect("a text command");
    let mut rows: Vec<(f32, f32, f32)> = Vec::new();
    for g in cmd.runs.iter().flat_map(|r| &r.glyphs) {
        match rows.iter_mut().find(|r| (r.0 - g.y).abs() < 1.0) {
            Some(r) => {
                r.1 = r.1.min(g.x);
                r.2 = r.2.max(g.x + g.advance);
            }
            None => rows.push((g.y, g.x, g.x + g.advance)),
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows
}

/// A block container (boxes are flex containers by default) of the given width.
fn block(width: f64) -> Vec<(&'static str, Value)> {
    vec![("display", "block".into()), ("width", px(width))]
}

/// A 300px wide block holding a float of `float` styles, then a text of `WORDS`. Returns the
/// tree and the ids (container, float, text).
fn page(float: &[(&str, Value)], text: &[(&str, Value)]) -> (UiTree, [u32; 3]) {
    let mut d = Dom::new();
    let root = d.add(0, "box", &block(300.0));
    let f = d.add(root, "box", float);
    let t = d.add(root, "text", text);
    d.text(t, WORDS);
    (d.build(""), [root, f, t])
}

fn float_box(side: &str, w: f64, h: f64) -> Vec<(&'static str, Value)> {
    vec![("float", side.into()), ("width", px(w)), ("height", px(h))]
}

// M3-FLT-01
#[test]
fn text_lines_start_after_a_left_float_and_return_to_the_edge_below_it() {
    let (mut tree, [_, f, t]) = page(&float_box("left", 100.0, 60.0), &[]);
    assert_eq!(rect(&tree, f), Rect::new(0.0, 0.0, 100.0, 60.0));
    // The text's box is the full width, from the top; only its lines avoid the float.
    assert_eq!(rect(&tree, t).x, 0.0);
    assert_eq!(rect(&tree, t).y, 0.0);
    assert_eq!(rect(&tree, t).width, 300.0);
    let rows = lines(&mut tree);
    assert!(rows.len() > 4, "{rows:?}");
    let float_bottom = 60.0;
    for (top, left, right) in &rows {
        // A line whose top is above the float's bottom is beside it.
        if *top < float_bottom - 20.0 {
            assert!(*left >= 100.0 - 0.5, "line at {top} starts at {left}");
        }
        assert!(*right <= 300.5);
    }
    let last = rows.last().unwrap();
    assert!(last.1 < 1.0, "the last line is back at the edge: {last:?}");
}

// M3-FLT-02
#[test]
fn a_right_float_narrows_lines_from_the_right() {
    let (mut tree, [_, f, _]) = page(&float_box("right", 120.0, 60.0), &[]);
    assert_eq!(rect(&tree, f), Rect::new(180.0, 0.0, 120.0, 60.0));
    let rows = lines(&mut tree);
    let first = rows[0];
    assert!(first.1 < 1.0, "starts at the left edge: {first:?}");
    assert!(first.2 <= 180.5, "ends before the float: {first:?}");
    assert!(
        rows.iter().any(|r| r.2 > 200.0),
        "later lines use the width"
    );
}

// M3-FLT-03
#[test]
fn floats_on_both_sides_leave_the_middle() {
    let mut d = Dom::new();
    let root = d.add(0, "box", &block(300.0));
    d.add(root, "box", &float_box("left", 80.0, 80.0));
    d.add(root, "box", &float_box("right", 80.0, 80.0));
    let t = d.add(root, "text", &[]);
    d.text(t, WORDS);
    let mut tree = d.build("");
    let first = lines(&mut tree)[0];
    // A line's trailing space hangs past its room.
    assert!(first.1 >= 79.5 && first.2 <= 226.0, "{first:?}");
}

// M3-FLT-04
#[test]
fn floats_of_one_side_sit_next_to_each_other_then_wrap_down() {
    let mut d = Dom::new();
    let root = d.add(0, "box", &block(300.0));
    let a = d.add(root, "box", &float_box("left", 120.0, 40.0));
    let b = d.add(root, "box", &float_box("left", 120.0, 40.0));
    let c = d.add(root, "box", &float_box("left", 120.0, 40.0));
    let tree = d.build("");
    assert_eq!(rect(&tree, a), Rect::new(0.0, 0.0, 120.0, 40.0));
    assert_eq!(rect(&tree, b), Rect::new(120.0, 0.0, 120.0, 40.0));
    // No room for a third: it goes below the first.
    assert_eq!(rect(&tree, c), Rect::new(0.0, 40.0, 120.0, 40.0));
}

// M3-FLT-05
#[test]
fn clear_moves_a_block_below_the_floats() {
    let mut d = Dom::new();
    let root = d.add(0, "box", &block(300.0));
    d.add(root, "box", &float_box("left", 100.0, 70.0));
    let free = d.add(root, "box", &[("height", px(20.0))]);
    let cleared = d.add(
        root,
        "box",
        &[("height", px(20.0)), ("clear", "left".into())],
    );
    let tree = d.build("");
    // An ordinary block ignores the float's box (its text would wrap).
    assert_eq!(rect(&tree, free).y, 0.0);
    assert_eq!(rect(&tree, cleared).y, 70.0);
}

// M3-FLT-06
#[test]
fn a_float_sticks_out_of_a_plain_block_but_a_flow_root_contains_it() {
    let tall = float_box("left", 100.0, 500.0);
    let mut d = Dom::new();
    let outer = d.add(0, "box", &{
        let mut s = vec![("display", Value::from("flow-root")), ("width", px(300.0))];
        s.push(("alignSelf", "flex-start".into()));
        s
    });
    let plain = d.add(outer, "box", &[("display", "block".into())]);
    d.add(plain, "box", &tall);
    d.add(plain, "box", &[("height", px(30.0))]);
    let inner = d.add(outer, "box", &[("display", "flow-root".into())]);
    d.add(inner, "box", &tall);
    let tree = d.build("");
    // Inside a plain block the float hangs below the block's own content.
    assert_eq!(rect(&tree, plain).height, 30.0);
    assert_eq!(rect(&tree, inner).height, 500.0);
    // The root of the formatting context holds all of it.
    assert_eq!(rect(&tree, outer).height, 530.0);
}

// M3-FLT-07
#[test]
fn a_word_wider_than_the_room_beside_a_float_moves_below_it() {
    let mut d = Dom::new();
    let root = d.add(0, "box", &block(300.0));
    d.add(root, "box", &float_box("left", 270.0, 50.0));
    let t = d.add(root, "text", &[]);
    d.text(t, "extraordinarily short");
    let mut tree = d.build("");
    let rows = lines(&mut tree);
    assert!(rows[0].0 >= 49.5, "starts below the float: {rows:?}");
    assert!(rows[0].1 < 1.0);
}

// M3-FLT-08
#[test]
fn text_beside_a_float_is_aligned_within_its_room() {
    let (mut tree, _) = page(
        &float_box("left", 100.0, 200.0),
        &[("textAlign", "right".into())],
    );
    for (_, _, right) in lines(&mut tree) {
        assert!(
            right > 290.0 && right <= 306.0,
            "right-aligned lines: {right}"
        );
    }
}

// M3-FLT-09
#[test]
fn floats_are_ignored_in_flex_and_grid_containers() {
    let mut d = Dom::new();
    let row = d.add(
        0,
        "box",
        &[("display", "flex".into()), ("width", px(300.0))],
    );
    let a = d.add(row, "box", &float_box("right", 50.0, 20.0));
    let b = d.add(row, "box", &float_box("right", 50.0, 20.0));
    let tree = d.build("");
    assert_eq!(rect(&tree, a).x, 0.0);
    assert_eq!(rect(&tree, b).x, 50.0);
}

// M3-FLT-10
#[test]
fn removing_or_resizing_a_float_relayouts_the_text_beside_it() {
    let (mut tree, [_, f, _]) = page(&float_box("left", 100.0, 60.0), &[]);
    let beside = lines(&mut tree);
    assert!(beside[0].1 >= 99.5);
    let set = |tree: &mut UiTree, props: &[(&str, Value)]| {
        let mut args = vec![n(f)];
        for (k, v) in props {
            args.push((*k).into());
            args.push(v.clone());
        }
        let errors = tree.apply_commit(&Commit {
            ops: vec![op(SET_STYLE, &args)],
        });
        assert!(errors.is_empty(), "{errors:?}");
        tree.layout_if_needed();
    };
    set(&mut tree, &[("float", "none".into())]);
    assert!(lines(&mut tree)[0].1 < 1.0, "text is back at the edge");
    set(&mut tree, &[("float", "right".into()), ("width", px(60.0))]);
    let rows = lines(&mut tree);
    assert!(rows[0].1 < 1.0 && rows[0].2 <= 240.5, "{:?}", rows[0]);
}

// M3-FLT-11
#[test]
fn margins_keep_text_away_from_a_float() {
    let mut style = float_box("left", 100.0, 60.0);
    style.push(("marginRight", px(20.0)));
    let (mut tree, _) = page(&style, &[]);
    assert!(lines(&mut tree)[0].1 >= 119.5);
}

// M3-FLT-12
#[test]
fn text_with_padding_wraps_around_a_float_inside_its_padding_box() {
    let (mut tree, [_, _, t]) = page(
        &float_box("left", 100.0, 60.0),
        &[("padding", px(10.0)), ("backgroundColor", "#eee".into())],
    );
    assert_eq!(rect(&tree, t).width, 300.0);
    let first = lines(&mut tree)[0];
    assert!(first.1 >= 99.5, "{first:?}");
    // The box is taller than its lines only by its padding.
    let rows = lines(&mut tree);
    assert!(rect(&tree, t).height > rows.len() as f32 * 15.0);
}

// M3-FLT-14
#[test]
fn a_later_paragraph_stays_clear_of_the_floats_bottom_margin() {
    // Two paragraphs after a float whose margin box ends at 6 + 60 + 40 = 106. The second
    // paragraph starts below the first, and its lines that reach into the margin stay narrow.
    let mut d = Dom::new();
    let root = d.add(0, "box", &block(300.0));
    // Its height comes from its text (five lines in 100px), as a pull-quote's does.
    let f = vec![
        ("display", "block".into()),
        ("float", "right".into()),
        ("width", px(100.0)),
        ("padding", "8px 0px 8px 8px".into()),
        ("margin", "6px 0 40px 20px".into()),
    ];
    let fl = d.add(root, "box", &f);
    let q = d.add(fl, "text", &[]);
    d.text(
        q,
        "one two three four five six seven eight nine ten eleven twelve",
    );
    let a = d.add(root, "text", &[("marginBottom", px(10.0))]);
    d.text(
        a,
        "alpha beta gamma delta epsilon zeta eta theta iota kappa",
    );
    let b = d.add(root, "text", &[]);
    d.text(b, WORDS);
    let mut tree = d.build("");
    let second = rect(&tree, b);
    let cmds: Vec<_> = tree
        .draw_list()
        .into_iter()
        .filter_map(|c| match c {
            DrawCmd::Text(t) => Some(t),
            _ => None,
        })
        .collect();
    let mut tops: Vec<(f32, f32)> = Vec::new();
    for g in cmds[1].runs.iter().flat_map(|r| &r.glyphs) {
        if !tops.iter().any(|t| (t.0 - g.y).abs() < 1.0) {
            tops.push((g.y, 0.0));
        }
        let t = tops.iter_mut().find(|t| (t.0 - g.y).abs() < 1.0).unwrap();
        t.1 = t.1.max(g.x + g.advance);
    }
    let fr = rect(&tree, fl);
    let bottom = fr.y + fr.height + 40.0;
    assert!(bottom > 130.0, "{fr:?}");
    for (y, right) in &tops {
        if *y < bottom - 14.0 {
            assert!(
                *right <= fr.x - 20.0 + 0.5,
                "line at {y} (text at {}) ends at {right}, float {fr:?}",
                second.y
            );
        }
    }
}

// M3-FLT-15
#[test]
fn the_taller_of_two_floats_keeps_narrowing_lines_after_the_shorter_ends() {
    let mut d = Dom::new();
    let root = d.add(0, "box", &block(300.0));
    d.add(root, "box", &float_box("left", 80.0, 40.0));
    let mut tall = float_box("right", 80.0, 120.0);
    tall.push(("marginBottom", px(20.0)));
    d.add(root, "box", &tall);
    let t = d.add(root, "text", &[]);
    d.text(t, WORDS);
    let mut tree = d.build("");
    let rows = lines(&mut tree);
    for (top, left, right) in &rows {
        if *top < 130.0 {
            // (a trailing space may hang a few px past the room)
            assert!(*right <= 226.0, "line at {top} ends at {right}");
        }
        if *top > 65.0 && *top < 130.0 {
            assert!(*left < 1.0, "line at {top} starts at {left}");
        }
    }
    assert!(rows.iter().any(|r| r.0 > 45.0), "{rows:?}");
}

// M3-FLT-13
#[test]
fn an_image_floats_inside_a_paragraph_of_bare_text() {
    // The text is a child of the block itself (an anonymous text block), the way a
    // paragraph holding an image is written.
    let mut d = Dom::new();
    let p = d.add(0, "box", &block(300.0));
    let img = d.add(
        p,
        "image",
        &[
            ("float", "right".into()),
            ("width", px(90.0)),
            ("height", px(60.0)),
            ("margin", px(6.0)),
        ],
    );
    d.text(p, WORDS);
    let mut tree = d.build("");
    assert_eq!(rect(&tree, img), Rect::new(204.0, 6.0, 90.0, 60.0));
    let rows = lines(&mut tree);
    assert!(rows[0].2 <= 204.0 + 6.0, "{:?}", rows[0]);
    // A line that touches the float's margin box stays beside it; later ones are wide.
    assert!(
        rows.iter().any(|r| r.2 > 250.0),
        "below the image: {rows:?}"
    );
}
