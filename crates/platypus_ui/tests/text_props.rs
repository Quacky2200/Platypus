// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Text properties from M3d group 4: `text-transform`, `text-overflow`, wrapping,
//! fonts, decorations and strokes.

use crate::common;

use common::{Dom, px};
use platypus_ui::{DrawCmd, TextCmd, UiTree};
use web_ui_core::Value;

/// The text command of the only text block in the tree.
fn text_cmd(tree: &mut UiTree) -> TextCmd {
    tree.draw_list()
        .into_iter()
        .find_map(|c| match c {
            DrawCmd::Text(t) => Some(t),
            _ => None,
        })
        .expect("a text command")
}

fn glyph_ids(cmd: &TextCmd) -> Vec<u32> {
    cmd.runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.id))
        .collect()
}

/// A text element with `style` holding `text`.
fn label(text: &str, style: &[(&str, Value)]) -> UiTree {
    let mut d = Dom::new();
    let t = d.add(0, "text", style);
    d.text(t, text);
    d.build("")
}

// M3-TXT-10
#[test]
fn text_transform_changes_the_shaped_text() {
    let upper = glyph_ids(&text_cmd(&mut label(
        "Hello world",
        &[("textTransform", "uppercase".into())],
    )));
    assert_eq!(upper, glyph_ids(&text_cmd(&mut label("HELLO WORLD", &[]))));
    let caps = glyph_ids(&text_cmd(&mut label(
        "hello wORLD",
        &[("textTransform", "capitalize".into())],
    )));
    assert_eq!(caps, glyph_ids(&text_cmd(&mut label("Hello WORLD", &[]))));
}

// M3-TXT-10
#[test]
fn ellipsis_cuts_overflowing_lines_where_overflow_clips() {
    let style = |overflow: &str| {
        vec![
            ("width", px(80.0)),
            ("whiteSpace", "nowrap".into()),
            ("textOverflow", "ellipsis".into()),
            ("overflow", overflow.into()),
        ]
    };
    let text = "The quick brown fox jumps";
    let full = text_cmd(&mut label(text, &style("visible")));
    let cut = text_cmd(&mut label(text, &style("hidden")));
    let glyphs: Vec<_> = cut.runs.iter().flat_map(|r| &r.glyphs).collect();
    let right = glyphs
        .iter()
        .map(|g| g.x + g.advance)
        .fold(f32::MIN, f32::max);
    assert!(right <= 80.5, "everything fits in the box: {right}");
    assert!(
        glyphs.len() < glyph_ids(&full).len(),
        "visible overflow isn't cut"
    );
    // The last glyph is the ellipsis, shaped on its own.
    let dots = glyph_ids(&text_cmd(&mut label("\u{2026}", &[])));
    assert_eq!(cut.runs.last().unwrap().glyphs.len(), dots.len());
    assert_eq!(glyphs.last().unwrap().id, dots[0]);

    // Text that fits is left alone.
    let short = text_cmd(&mut label("Fox", &style("hidden")));
    assert_eq!(
        glyph_ids(&short),
        glyph_ids(&text_cmd(&mut label("Fox", &[])))
    );
}

// M3-TXT-10
#[test]
fn rtl_ellipsis_goes_on_the_left() {
    let cut = text_cmd(&mut label(
        "The quick brown fox jumps",
        &[
            ("width", px(80.0)),
            ("direction", "rtl".into()),
            ("whiteSpace", "nowrap".into()),
            ("textOverflow", "ellipsis".into()),
            ("overflow", "hidden".into()),
        ],
    ));
    let glyphs: Vec<_> = cut.runs.iter().flat_map(|r| &r.glyphs).collect();
    let left = glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
    assert!(left >= -0.5, "nothing left of the box: {left}");
    let dots = cut.runs.last().unwrap().glyphs[0].x;
    assert!(
        glyphs.iter().all(|g| g.x >= dots - 0.01),
        "the ellipsis is leftmost"
    );
}

/// The height of a text block `width` px wide.
fn height(text: &str, width: f64, style: &[(&str, Value)]) -> f32 {
    let mut d = Dom::new();
    let col = d.add(0, "box", &[("alignItems", "flex-start".into())]);
    let mut s = vec![("width", px(width))];
    s.extend_from_slice(style);
    let t = d.add(col, "text", &s);
    d.text(t, text);
    let mut tree = d.build("");
    tree.layout_if_needed();
    tree.rect(web_ui_core::NodeId(t)).unwrap().height
}

// M3-TXT-11
#[test]
fn long_words_break_with_overflow_wrap_and_word_break() {
    let word = "Supercalifragilisticexpialidocious";
    let one_line = height(word, 60.0, &[]);
    assert!(
        height(word, 60.0, &[("overflowWrap", "anywhere".into())]) > one_line * 2.0,
        "anywhere breaks the word"
    );
    assert!(height(word, 60.0, &[("wordWrap", "break-word".into())]) > one_line * 2.0);
    assert!(height(word, 60.0, &[("wordBreak", "break-word".into())]) > one_line * 2.0);
    assert!(height(word, 60.0, &[("wordBreak", "break-all".into())]) > one_line * 2.0);
    // break-all breaks inside words even when they'd fit on the next line.
    let text = "aa bbbbbbbbbbbbbbbb";
    let normal = height(text, 90.0, &[]);
    let all = height(text, 90.0, &[("wordBreak", "break-all".into())]);
    assert!(all >= normal, "{all} vs {normal}");
}

// M3-TXT-11
#[test]
fn pre_line_keeps_newlines_but_collapses_spaces() {
    let one = height("a b", 200.0, &[]);
    assert_eq!(
        height("a     b\nc", 200.0, &[]),
        one,
        "normal collapses the newline"
    );
    assert!(
        (height("a     b\nc", 200.0, &[("whiteSpace", "pre-line".into())]) - 2.0 * one).abs() < 1.0,
        "pre-line: two lines"
    );
    let wide = |ws: &str| {
        let mut d = Dom::new();
        let t = d.add(0, "text", &[("whiteSpace", ws.into())]);
        d.text(t, "a      b");
        let mut tree = d.build("");
        tree.layout_if_needed();
        tree.rect(web_ui_core::NodeId(t)).unwrap().width
    };
    assert!(
        wide("pre-line") < wide("break-spaces"),
        "pre-line collapses the run of spaces"
    );
}

/// The width a text element shrinks to.
fn width(text: &str, style: &[(&str, Value)]) -> f32 {
    let mut d = Dom::new();
    let col = d.add(0, "box", &[("alignItems", "flex-start".into())]);
    let t = d.add(col, "text", style);
    d.text(t, text);
    let mut tree = d.build("");
    tree.layout_if_needed();
    tree.rect(web_ui_core::NodeId(t)).unwrap().width
}

// M3-TXT-12
#[test]
fn tabular_nums_give_digits_one_width() {
    let tabular = [("fontVariantNumeric", Value::from("tabular-nums"))];
    assert!(
        width("1111", &[]) < width("8888", &[]),
        "Inter's digits are proportional"
    );
    assert_eq!(width("1111", &tabular), width("8888", &tabular));
    let font = [("font", Value::from("bold 30px sans-serif"))];
    assert!(
        width("Hi", &font) > width("Hi", &[]) * 1.5,
        "the shorthand applies"
    );
}

/// The marker texts of `list`'s items, in order.
fn markers(tree: &mut UiTree, items: &[u32]) -> Vec<String> {
    tree.layout_if_needed();
    items
        .iter()
        .filter_map(|i| {
            let marker = *tree.children(web_ui_core::NodeId(*i)).first()?;
            let text = *tree.children(marker).first()?;
            Some(tree.text(text)?.to_owned())
        })
        .collect()
}

// M3-LST-01
#[test]
fn list_items_get_numbered_markers() {
    let mut d = Dom::new();
    let list = d.add(0, "list", &[("listStyleType", "decimal".into())]);
    let items: Vec<u32> = (0..3)
        .map(|_| d.add(list, "box", &[("height", px(20.0))]))
        .collect();
    let mut tree = d.build("");
    assert_eq!(markers(&mut tree, &items), vec!["1.", "2.", "3."]);

    // Removing the first renumbers the rest.
    tree.apply_commit(&web_ui_core::Commit {
        ops: vec![common::op(
            platypus_ui::opcodes::REMOVE_CHILD,
            &[common::n(list), common::n(items[0])],
        )],
    });
    assert_eq!(markers(&mut tree, &items[1..]), vec!["1.", "2."]);

    // Other types, and `none` removes the markers.
    for (kind, first) in [
        ("upper-roman", "I."),
        ("lower-alpha", "a."),
        ("disc", "\u{2022}"),
        ("'-'", "-"),
    ] {
        let errors = tree.apply_commit(&web_ui_core::Commit {
            ops: vec![common::op(
                platypus_ui::opcodes::SET_STYLE,
                &[common::n(list), "listStyleType".into(), kind.into()],
            )],
        });
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            markers(&mut tree, &items[1..2]),
            vec![first.to_owned()],
            "{kind}"
        );
    }
    tree.apply_commit(&web_ui_core::Commit {
        ops: vec![common::op(
            platypus_ui::opcodes::SET_STYLE,
            &[common::n(list), "listStyle".into(), "none".into()],
        )],
    });
    tree.layout_if_needed();
    assert!(
        tree.children(web_ui_core::NodeId(items[1])).is_empty(),
        "no marker"
    );
}

// M3-LST-01
#[test]
fn outside_markers_hang_left_and_inside_ones_take_space() {
    let build = |position: &str| {
        let mut d = Dom::new();
        let list = d.add(0, "list", &[("listStylePosition", position.into())]);
        let item = d.add(list, "box", &[("height", px(20.0))]);
        let child = d.add(item, "box", &[("width", px(10.0)), ("height", px(10.0))]);
        let mut tree = d.build("");
        tree.layout_if_needed();
        let marker = tree.children(web_ui_core::NodeId(item))[0];
        (
            tree.rect(web_ui_core::NodeId(item)).unwrap(),
            tree.rect(marker).unwrap(),
            tree.rect(web_ui_core::NodeId(child)).unwrap(),
        )
    };
    let (item, marker, child) = build("outside");
    assert_eq!(item.x, 24.0, "the list's default left padding");
    assert!(
        marker.x + marker.width <= item.x + 0.5,
        "{marker:?} hangs left of {item:?}"
    );
    assert_eq!(child.x, item.x, "content isn't pushed over");
    let (item, marker, child) = build("inside");
    assert!(
        marker.x >= item.x && child.x > marker.x,
        "inside: {marker:?} {child:?}"
    );
}

// M3-LST-01
#[test]
fn marker_rules_style_the_marker() {
    let mut d = Dom::new();
    let list = d.add(0, "list", &[]);
    let item = d.add(list, "box", &[]);
    d.set_class(item, "item");
    let mut tree = d.build(".item::marker { color: red; font-size: 30px }");
    tree.layout_if_needed();
    let marker = tree.children(web_ui_core::NodeId(item))[0];
    let text = tree.computed_text(marker);
    assert_eq!(text.color, platypus_ui::Rgba::new(1.0, 0.0, 0.0, 1.0));
    assert_eq!(text.font_size, 30.0);
}

// M3-LST-01
#[test]
fn lists_without_markers_have_no_indent() {
    let item_x = |list_style: &[(&str, Value)]| {
        let mut d = Dom::new();
        let list = d.add(0, "list", list_style);
        let item = d.add(list, "box", &[("height", px(20.0))]);
        let mut tree = d.build("");
        tree.layout_if_needed();
        (tree, list, item)
    };
    let x = |t: &(UiTree, u32, u32)| t.0.rect(web_ui_core::NodeId(t.2)).unwrap().x;
    assert_eq!(x(&item_x(&[])), 24.0, "markers: indented");
    assert_eq!(
        x(&item_x(&[("listStyle", "none".into())])),
        0.0,
        "no markers: flush"
    );
    assert_eq!(
        x(&item_x(&[("listStyleType", "none".into())])),
        0.0,
        "the longhand too"
    );
    assert_eq!(
        x(&item_x(&[
            ("listStyle", "none".into()),
            ("paddingLeft", px(10.0))
        ])),
        10.0,
        "explicit padding wins"
    );
    assert_eq!(
        x(&item_x(&[("listStyle", "none url(dot.png)".into())])),
        24.0,
        "an image marker still needs the room"
    );
    // Switching at runtime updates the indent.
    let (mut tree, list, item) = item_x(&[]);
    let errors = tree.apply_commit(&web_ui_core::Commit {
        ops: vec![common::op(
            platypus_ui::opcodes::SET_STYLE,
            &[common::n(list), "listStyleType".into(), "none".into()],
        )],
    });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    assert_eq!(tree.rect(web_ui_core::NodeId(item)).unwrap().x, 0.0);
    // And from a stylesheet.
    let mut d = Dom::new();
    let list = d.add(0, "list", &[]);
    let item = d.add(list, "box", &[("height", px(20.0))]);
    d.set_class(list, "plain");
    let mut tree = d.build(".plain { list-style: none }");
    tree.layout_if_needed();
    assert_eq!(tree.rect(web_ui_core::NodeId(item)).unwrap().x, 0.0);
}
