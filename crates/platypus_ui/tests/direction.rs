// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Right-to-left text (M3-TXT-09): CSS `direction`, the `dir` attribute, `text-align`
//! start/end, and mixed-direction lines.

use crate::common;

use common::{Dom, n, op, px};
use parley::PositionedLayoutItem;
use platypus_ui::UiTree;
use platypus_ui::opcodes::SET_ATTRS;
use web_ui_core::{Commit, NodeId, Value};

/// A 200px-wide text block holding `text`, with inline `style` and optional `dir`.
fn block(text: &str, style: &[(&str, Value)], dir: Option<&str>, css: &str) -> (UiTree, NodeId) {
    let mut d = Dom::new();
    let mut s = vec![("width", px(200.0)), ("fontSize", px(16.0))];
    s.extend_from_slice(style);
    let id = d.add(0, "text", &s);
    d.text(id, text);
    if let Some(dir) = dir {
        d.ops
            .push(op(SET_ATTRS, &[n(id), "dir".into(), dir.into()]));
    }
    (d.build(css), NodeId(id))
}

/// Left edges of the first line's glyph runs, and whether each run is right-to-left.
fn runs(tree: &UiTree, id: NodeId) -> Vec<(f32, bool)> {
    let layout = tree.text_layout(id).unwrap();
    let line = layout.lines().next().unwrap();
    line.items()
        .filter_map(|item| match item {
            PositionedLayoutItem::GlyphRun(g) => Some((g.offset(), g.run().is_rtl())),
            _ => None,
        })
        .collect()
}

fn left(tree: &UiTree, id: NodeId) -> f32 {
    runs(tree, id)[0].0
}

// M3-TXT-09
#[test]
fn rtl_blocks_start_at_the_right() {
    let (ltr, id) = block("abc", &[], None, "");
    assert!(left(&ltr, id) < 1.0, "ltr starts at the left");

    let (rtl, id) = block("abc", &[("direction", "rtl".into())], None, "");
    assert!(left(&rtl, id) > 150.0, "rtl starts at the right");

    let (end, id) = block(
        "abc",
        &[("direction", "rtl".into()), ("textAlign", "end".into())],
        None,
        "",
    );
    assert!(left(&end, id) < 1.0, "end is the left in rtl");

    let (right, id) = block("abc", &[("textAlign", "left".into())], Some("rtl"), "");
    assert!(left(&right, id) < 1.0, "left stays physical");
}

// M3-TXT-09
#[test]
fn dir_sets_direction_unless_css_does() {
    let (attr, id) = block("abc", &[], Some("rtl"), "");
    assert!(left(&attr, id) > 150.0, "dir=rtl");

    let (inline, id) = block("abc", &[("direction", "ltr".into())], Some("rtl"), "");
    assert!(left(&inline, id) < 1.0, "inline direction beats dir");

    let (sheet, id) = block("abc", &[], Some("rtl"), "text { direction: ltr; }");
    assert!(left(&sheet, id) < 1.0, "a stylesheet beats dir too");

    let (selector, id) = block("abc", &[], Some("rtl"), "[dir=rtl] { color: red; }");
    assert!(left(&selector, id) > 150.0, "[dir] selectors match");
    assert_eq!(selector.computed_text(id).color.r, 1.0);
}

// M3-TXT-09
#[test]
fn direction_inherits_and_follows_dir_changes() {
    let mut d = Dom::new();
    let outer = d.add(0, "box", &[("direction", "rtl".into())]);
    let id = d.add(outer, "text", &[("width", px(200.0))]);
    d.text(id, "abc");
    let mut tree = d.build("");
    let id = NodeId(id);
    assert!(left(&tree, id) > 150.0, "inherited from the parent box");

    let set_dir = |tree: &mut UiTree, value: Value| {
        let errors = tree.apply_commit(&Commit {
            ops: vec![op(SET_ATTRS, &[n(id.0), "dir".into(), value])],
        });
        assert!(errors.is_empty(), "{errors:?}");
        tree.layout_if_needed();
    };
    set_dir(&mut tree, "ltr".into());
    assert!(
        left(&tree, id) < 1.0,
        "dir=ltr on the child wins over inheritance"
    );
    set_dir(&mut tree, Value::Null);
    assert!(left(&tree, id) > 150.0, "removing dir inherits again");
}

// M3-TXT-09
#[test]
fn mixed_lines_order_runs_by_the_paragraph_direction() {
    // Hebrew letters are strongly right-to-left; Parley shapes and reorders them.
    let (ltr, id) = block("abc אבג", &[], None, "");
    // Runs come in visual order; the space may be a run of its own.
    let r = runs(&ltr, id);
    assert!(
        !r[0].1 && r.last().unwrap().1,
        "Latin on the left, the RTL run on the right: {r:?}"
    );

    let (rtl, id) = block("abc אבג", &[("direction", "rtl".into())], None, "");
    let r = runs(&rtl, id);
    assert!(
        r[0].1 && !r.last().unwrap().1,
        "in an rtl paragraph the Hebrew run is on the left: {r:?}"
    );
}
