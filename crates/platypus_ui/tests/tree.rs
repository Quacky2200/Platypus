// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

use platypus_ui::opcodes::*;
use platypus_ui::{DrawCmd, Quad, ROOT, Rect, Rgba, UiTree};
use web_ui_core::{Commit, NodeId, Value};

fn op(code: u32, args: &[Value]) -> Value {
    let mut v = vec![Value::Number(code.into())];
    v.extend_from_slice(args);
    Value::Array(v)
}

fn n(id: u32) -> Value {
    Value::Number(id.into())
}

fn create(id: u32) -> Value {
    op(CREATE, &[n(id), "box".into()])
}

fn style(id: u32, props: &[(&str, Value)]) -> Value {
    let mut args = vec![n(id)];
    for (k, v) in props {
        args.push((*k).into());
        args.push(v.clone());
    }
    op(SET_STYLE, &args)
}

fn append(parent: u32, child: u32) -> Value {
    op(APPEND_CHILD, &[n(parent), n(child)])
}

fn commit(ops: Vec<Value>) -> Commit {
    Commit { ops }
}

fn apply_ok(tree: &mut UiTree, ops: Vec<Value>) {
    let errors = tree.apply_commit(&commit(ops));
    assert!(errors.is_empty(), "unexpected errors: {errors:?}");
}

fn ids(list: &[NodeId]) -> Vec<u32> {
    list.iter().map(|n| n.0).collect()
}

// M2-UI-01
#[test]
fn builds_the_tree_from_ops() {
    let mut tree = UiTree::new(100.0, 100.0);
    apply_ok(
        &mut tree,
        vec![
            create(1),
            create(2),
            create(3),
            append(1, 2),
            append(1, 3),
            append(0, 1),
        ],
    );
    assert_eq!(tree.len(), 3);
    assert_eq!(ids(tree.children(ROOT)), [1]);
    assert_eq!(ids(tree.children(NodeId(1))), [2, 3]);
    assert_eq!(tree.parent(NodeId(3)), Some(NodeId(1)));
    assert_eq!(tree.kind(NodeId(2)), Some("box"));

    // Insert before, then move an existing child to the end.
    apply_ok(
        &mut tree,
        vec![
            create(4),
            op(INSERT_BEFORE, &[n(1), n(4), n(2)]),
            append(1, 2),
        ],
    );
    assert_eq!(ids(tree.children(NodeId(1))), [4, 3, 2]);

    // Removing destroys the whole subtree.
    apply_ok(
        &mut tree,
        vec![create(5), append(3, 5), op(REMOVE_CHILD, &[n(1), n(3)])],
    );
    assert_eq!(ids(tree.children(NodeId(1))), [4, 2]);
    assert!(!tree.contains(NodeId(3)) && !tree.contains(NodeId(5)));
}

// M2-UI-02
#[test]
fn bad_ops_are_reported_and_skipped() {
    let mut tree = UiTree::new(100.0, 100.0);
    let errors = tree.apply_commit(&commit(vec![
        create(1),
        append(0, 99),                     // unknown child
        append(42, 1),                     // unknown parent
        create(1),                         // duplicate id
        op(77, &[]),                       // unknown opcode
        Value::String("not an op".into()), // not an array
        op(CREATE, &[n(2)]),               // missing type
        create(2),
        append(1, 2),
        append(2, 1),                                                  // cycle
        op(INSERT_BEFORE, &[n(0), n(2), n(1)]), // `before` not a child of root
        op(REMOVE_CHILD, &[n(0), n(2)]),        // not a child of root
        style(1, &[("width", 10.0.into()), ("colour", "red".into())]), // one bad prop
        append(0, 1),
    ]));
    assert_eq!(errors.len(), 10, "{errors:#?}");
    assert!(
        errors
            .iter()
            .any(|e| e.0.contains("unknown style property `colour`"))
    );
    // Everything valid still applied, including the good half of the style op.
    assert_eq!(ids(tree.children(ROOT)), [1]);
    assert_eq!(ids(tree.children(NodeId(1))), [2]);
    tree.layout_if_needed();
    assert_eq!(tree.rect(NodeId(1)).unwrap().width, 10.0);
}

// M2-UI-03
#[test]
fn resizing_the_root_relayouts() {
    let mut tree = UiTree::new(200.0, 100.0);
    apply_ok(
        &mut tree,
        vec![
            create(1),
            style(1, &[("width", "50%".into()), ("height", "100%".into())]),
            append(0, 1),
        ],
    );
    tree.layout_if_needed();
    assert_eq!(
        tree.rect(NodeId(1)),
        Some(Rect::new(0.0, 0.0, 100.0, 100.0))
    );

    tree.set_size(400.0, 300.0);
    assert!(tree.layout_if_needed());
    assert_eq!(
        tree.rect(NodeId(1)),
        Some(Rect::new(0.0, 0.0, 200.0, 300.0))
    );
}

// M2-LAY-04
#[test]
fn layout_only_runs_when_something_changed() {
    let mut tree = UiTree::new(100.0, 100.0);
    apply_ok(&mut tree, vec![create(1), append(0, 1)]);
    assert!(tree.layout_if_needed());
    assert!(!tree.layout_if_needed(), "nothing changed");
    tree.set_size(100.0, 100.0);
    assert!(!tree.layout_if_needed(), "same size is not a change");
    apply_ok(&mut tree, vec![style(1, &[("width", 50.0.into())])]);
    assert!(tree.layout_if_needed());
}

fn quads(cmds: &[DrawCmd]) -> Vec<&Quad> {
    cmds.iter()
        .filter_map(|c| match c {
            DrawCmd::Quad(q) => Some(q),
            _ => None,
        })
        .collect()
}

/// A compact description of the command sequence, for order assertions.
fn shape(cmds: &[DrawCmd]) -> Vec<String> {
    cmds.iter()
        .map(|c| match c {
            DrawCmd::BackdropBlur { blur, .. } => format!("backdrop {blur}"),
            DrawCmd::BoxShadow(s) => format!("shadow {}", s.offset_y),
            DrawCmd::Quad(q) => format!("quad {}", q.rect.width),
            DrawCmd::InsetShadow { shadow, .. } => format!("inset {}", shadow.spread),
            DrawCmd::Border { rect, .. } => format!("border {}", rect.width),
            DrawCmd::BorderImage { area, .. } => format!("border-image {}", area.width),
            DrawCmd::PushLayer { blur, opacity, .. } => {
                format!("push blur={blur} opacity={opacity}")
            }
            DrawCmd::PopLayer => "pop".into(),
            DrawCmd::BeginMask(_) => "mask".into(),
            DrawCmd::PushTransform(_) => "transform".into(),
            DrawCmd::PopTransform => "untransform".into(),
            DrawCmd::PushClip(c) => format!("clip {}x{}", c.rect.width, c.rect.height),
            DrawCmd::PopClip => "unclip".into(),
            DrawCmd::Text(t) => format!("text {}", t.runs.len()),
        })
        .collect()
}

fn sized(id: u32, w: f64, h: f64, extra: &[(&str, Value)]) -> Value {
    let mut props = vec![("width", w.into()), ("height", h.into())];
    props.extend_from_slice(extra);
    style(id, &props)
}

// M2-UI-04
#[test]
fn draw_list_is_in_paint_order_and_skips_invisible_boxes() {
    let mut tree = UiTree::new(100.0, 100.0);
    let red: Value = "#ff0000".into();
    apply_ok(
        &mut tree,
        vec![
            create(1),
            sized(1, 100.0, 100.0, &[("backgroundColor", red.clone())]),
            create(2), // no background or border: invisible, but its children still draw
            sized(2, 50.0, 50.0, &[]),
            create(3),
            sized(
                3,
                10.0,
                10.0,
                &[("borderWidth", 2.0.into()), ("borderColor", "blue".into())],
            ),
            create(4), // display: none hides its subtree
            style(
                4,
                &[("display", "none".into()), ("backgroundColor", red.clone())],
            ),
            create(5),
            sized(5, 5.0, 5.0, &[("backgroundColor", red.clone())]),
            create(6),
            sized(6, 20.0, 20.0, &[("backgroundColor", red.clone())]),
            append(2, 3),
            append(4, 5),
            append(1, 2),
            append(1, 4),
            append(1, 6),
            append(0, 1),
        ],
    );
    let cmds = tree.draw_list();
    let q = quads(&cmds);
    assert_eq!(
        cmds.len(),
        q.len(),
        "plain boxes only produce quads: {cmds:#?}"
    );
    assert_eq!(q.len(), 3);
    // Parent first, then children in order: 1, 3 (inside 2), 6.
    assert_eq!(q[0].rect, Rect::new(0.0, 0.0, 100.0, 100.0));
    assert_eq!(q[0].background, Rgba::new(1.0, 0.0, 0.0, 1.0));
    assert_eq!(q[1].rect, Rect::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(q[1].border, [2.0; 4]);
    assert_eq!(q[1].background.a, 0.0);
    assert_eq!(q[2].rect.x, 50.0, "box 6 follows box 2 in the row");
}

// M2-VIS-07
#[test]
fn effects_are_emitted_in_css_paint_order() {
    let mut tree = UiTree::new(200.0, 200.0);
    apply_ok(
        &mut tree,
        vec![
            create(1),
            sized(
                1,
                100.0,
                100.0,
                &[
                    ("backgroundColor", "white".into()),
                    (
                        "boxShadow",
                        "0 1px 2px black, inset 0 0 0 3px red, 0 2px 4px black".into(),
                    ),
                    ("filter", "blur(4px)".into()),
                    ("backdropFilter", "blur(10px)".into()),
                    ("borderRadius", "8px 0".into()),
                ],
            ),
            create(2),
            sized(2, 10.0, 10.0, &[("backgroundColor", "black".into())]),
            append(1, 2),
            append(0, 1),
        ],
    );
    let cmds = tree.draw_list();
    assert_eq!(
        shape(&cmds),
        [
            "backdrop 10",
            "push blur=4 opacity=1",
            "shadow 2", // last shadow in the list is painted first (bottom-most)
            "shadow 1",
            "quad 100",
            "inset 3",
            "quad 10", // the child, inside the layer
            "pop",
        ]
    );
    let DrawCmd::Quad(q) = &cmds[4] else { panic!() };
    assert_eq!(q.radii, platypus_ui::Radii::circular([8.0, 0.0, 8.0, 0.0]));
    // The layer covers the larger shadow (box offset 2 down, grown by its blur of 4:
    // x -4..104, y -2..106) plus 3x the filter blur (12) on every side.
    let DrawCmd::PushLayer { bounds, .. } = cmds[1] else {
        panic!()
    };
    assert_eq!(
        bounds,
        Rect::new(-4.0 - 12.0, -2.0 - 12.0, 108.0 + 24.0, 108.0 + 24.0)
    );
}

// M2-VIS-08
#[test]
fn opacity_groups_children_but_scales_simple_leaves() {
    let mut tree = UiTree::new(200.0, 200.0);
    apply_ok(
        &mut tree,
        vec![
            create(1), // parent with a child: one layer, children keep full alpha
            sized(
                1,
                100.0,
                100.0,
                &[("opacity", 0.5.into()), ("backgroundColor", "red".into())],
            ),
            create(2),
            sized(2, 10.0, 10.0, &[("backgroundColor", "lime".into())]),
            create(3), // leaf with a single quad: alpha is scaled, no layer
            sized(
                3,
                10.0,
                10.0,
                &[("opacity", 0.25.into()), ("backgroundColor", "blue".into())],
            ),
            create(4), // leaf with a shadow and a quad: needs a layer so they don't double-blend
            sized(
                4,
                10.0,
                10.0,
                &[
                    ("opacity", 0.5.into()),
                    ("backgroundColor", "blue".into()),
                    ("boxShadow", "0 0 4px black".into()),
                ],
            ),
            create(5), // fully transparent: skipped entirely
            sized(
                5,
                10.0,
                10.0,
                &[("opacity", 0.0.into()), ("backgroundColor", "blue".into())],
            ),
            append(1, 2),
            append(0, 1),
            append(0, 3),
            append(0, 4),
            append(0, 5),
        ],
    );
    let cmds = tree.draw_list();
    assert_eq!(
        shape(&cmds),
        [
            "push blur=0 opacity=0.5",
            "quad 100",
            "quad 10",
            "pop",
            "quad 10",
            "push blur=0 opacity=0.5",
            "shadow 0",
            "quad 10",
            "pop",
        ]
    );
    let q = quads(&cmds);
    assert_eq!(
        q[0].background.a, 1.0,
        "inside the layer, colours stay opaque"
    );
    assert_eq!(q[1].background.a, 1.0);
    assert_eq!(
        q[2].background.a, 0.25,
        "a simple leaf just scales its alpha"
    );
}

#[test]
fn backgrounds_and_shadows_without_a_fill_still_draw() {
    let mut tree = UiTree::new(100.0, 100.0);
    apply_ok(
        &mut tree,
        vec![
            create(1),
            sized(
                1,
                50.0,
                50.0,
                &[
                    ("backgroundImage", "linear-gradient(red, blue)".into()),
                    ("backgroundSize", "cover".into()),
                ],
            ),
            create(2), // transparent box that only casts a shadow
            sized(
                2,
                20.0,
                20.0,
                &[("boxShadow", "0 0 8px rgba(0,0,0,0.5)".into())],
            ),
            append(0, 1),
            append(0, 2),
        ],
    );
    let cmds = tree.draw_list();
    assert_eq!(shape(&cmds), ["quad 50", "shadow 0"]);
    let DrawCmd::Quad(q) = &cmds[0] else { panic!() };
    let fill = q.images.first().expect("gradient fill");
    assert_eq!(fill.size, platypus_ui::BackgroundSize::Cover);
    assert!(matches!(
        *fill.image,
        platypus_ui::BackgroundImage::Linear { .. }
    ));
}

fn text_node(id: u32, text: &str) -> Value {
    op(CREATE_TEXT, &[n(id), text.into()])
}

fn create_kind(id: u32, kind: &str) -> Value {
    op(CREATE, &[n(id), kind.into()])
}

// M3-TXT-07
#[test]
fn text_nodes_and_text_blocks() {
    let mut tree = UiTree::new(200.0, 100.0);
    // <box><text>Hello <span>big</span> world</text>loose</box>
    apply_ok(
        &mut tree,
        vec![
            create(1),
            create_kind(2, "text"),
            text_node(3, "Hello "),
            create_kind(4, "span"),
            text_node(5, "big"),
            text_node(6, " world"),
            text_node(7, "loose"),
            append(4, 5),
            append(2, 3),
            append(2, 4),
            append(2, 6),
            append(1, 2),
            append(1, 7),
            append(0, 1),
        ],
    );
    assert_eq!(tree.kind(NodeId(3)), Some(platypus_ui::TEXT_NODE));
    assert_eq!(tree.text(NodeId(5)), Some("big"));
    assert_eq!(tree.text_content(NodeId(2)), "Hello big world");

    // The <text> element is a text block; its inline content belongs to it.
    assert!(tree.is_text_root(NodeId(2)));
    for inline in [3, 4, 5, 6] {
        assert!(
            !tree.is_text_root(NodeId(inline)),
            "{inline} is inline content"
        );
        assert_eq!(tree.text_root_of(NodeId(inline)), Some(NodeId(2)));
    }
    // A bare string in a box is its own anonymous text block; boxes aren't text.
    assert!(tree.is_text_root(NodeId(7)));
    assert_eq!(tree.text_root_of(NodeId(1)), None);

    let mut dirty = tree.take_dirty_text();
    dirty.sort();
    assert_eq!(dirty, [NodeId(2), NodeId(7)], "new text blocks need layout");
    assert!(tree.take_dirty_text().is_empty());

    // Changing a span's text dirties only its block.
    apply_ok(&mut tree, vec![op(SET_TEXT, &[n(5), "huge".into()])]);
    assert_eq!(tree.text_content(NodeId(2)), "Hello huge world");
    assert_eq!(tree.take_dirty_text(), [NodeId(2)]);

    // Inline content isn't part of block layout: no rects for spans and text runs.
    tree.layout_if_needed();
    assert!(tree.rect(NodeId(2)).is_some());
    assert!(tree.rect(NodeId(4)).is_none() && tree.rect(NodeId(3)).is_none());
}

#[test]
fn text_node_errors() {
    let mut tree = UiTree::new(100.0, 100.0);
    let errors = tree.apply_commit(&commit(vec![
        text_node(1, "a"),
        create(2),
        append(1, 2),                      // text nodes can't have children
        op(SET_TEXT, &[n(2), "x".into()]), // not a text node
        op(SET_TEXT, &[n(9), "x".into()]), // unknown node
        op(CREATE_TEXT, &[n(3)]),          // missing text
    ]));
    assert_eq!(errors.len(), 4, "{errors:#?}");
    assert!(errors[0].0.contains("can't have children"));
}

// M3-TXT-06
#[test]
fn text_styles_inherit_and_current_color_follows_color() {
    let mut tree = UiTree::new(200.0, 100.0);
    apply_ok(
        &mut tree,
        vec![
            create(1),
            style(
                1,
                &[
                    ("color", "#00ff00".into()),
                    ("fontSize", 20.0.into()),
                    ("width", 100.0.into()),
                    ("height", 50.0.into()),
                ],
            ),
            create(2),
            sized(
                2,
                40.0,
                40.0,
                &[
                    ("border", "2px solid".into()),  // colour omitted: currentColor
                    ("boxShadow", "0 0 4px".into()), // colour omitted: currentColor
                    ("backgroundColor", "currentColor".into()),
                    ("fontSize", "1.5em".into()),
                ],
            ),
            append(1, 2),
            append(0, 1),
        ],
    );
    let computed = tree.computed_text(NodeId(2));
    assert_eq!(computed.color, Rgba::new(0.0, 1.0, 0.0, 1.0));
    assert_eq!(computed.font_size, 30.0);

    let green = Rgba::new(0.0, 1.0, 0.0, 1.0);
    let cmds = tree.draw_list();
    let quad = quads(&cmds)[0].clone();
    assert_eq!(quad.background, green);
    assert_eq!(quad.border_color, green);
    let shadow = cmds.iter().find_map(|c| match c {
        DrawCmd::BoxShadow(s) => Some(s.color),
        _ => None,
    });
    assert_eq!(shadow, Some(green));

    // Changing the ancestor's colour changes what currentColor resolves to.
    apply_ok(&mut tree, vec![style(1, &[("color", "#0000ff".into())])]);
    let cmds = tree.draw_list();
    assert_eq!(quads(&cmds)[0].background, Rgba::new(0.0, 0.0, 1.0, 1.0));
}

// M3-TXT-06: inherited text properties dirty the text blocks below them.
#[test]
fn text_style_changes_dirty_inheriting_text_blocks() {
    let mut tree = UiTree::new(200.0, 100.0);
    apply_ok(
        &mut tree,
        vec![
            create(1),
            create_kind(2, "text"),
            text_node(3, "a"),
            create(4),
            text_node(5, "b"),
            append(2, 3),
            append(1, 2),
            append(4, 5),
            append(1, 4),
            append(0, 1),
        ],
    );
    tree.take_dirty_text();

    // A non-text style change dirties nothing.
    apply_ok(&mut tree, vec![style(1, &[("width", 120.0.into())])]);
    assert!(tree.take_dirty_text().is_empty());

    // A font size on the ancestor dirties both text blocks below it.
    apply_ok(&mut tree, vec![style(1, &[("fontSize", 24.0.into())])]);
    let mut dirty = tree.take_dirty_text();
    dirty.sort();
    assert_eq!(dirty, [NodeId(2), NodeId(5)]);
    assert_eq!(tree.computed_text(NodeId(3)).font_size, 24.0);
}

// M3-TXT-04/05: text draw commands, decorations and shadows.
#[test]
fn text_blocks_draw_glyphs_decorations_and_shadows() {
    let mut tree = UiTree::new(300.0, 100.0);
    apply_ok(
        &mut tree,
        vec![
            create_kind(1, "text"),
            style(
                1,
                &[
                    ("color", "#ff0000".into()),
                    ("padding", 10.0.into()),
                    ("textDecoration", "underline".into()),
                    ("textShadow", "2px 3px 0 blue, 0 0 4px black".into()),
                ],
            ),
            text_node(2, "Hi there"),
            append(1, 2),
            append(0, 1),
        ],
    );
    let cmds = tree.draw_list();
    // Shadows are painted under the text, last one lowest: blurred (in a layer), sharp, then the text.
    assert_eq!(
        shape(&cmds),
        ["push blur=2 opacity=1", "text 1", "pop", "text 1", "text 1"]
    );
    let DrawCmd::Text(text) = &cmds[4] else {
        panic!()
    };
    let DrawCmd::Text(sharp_shadow) = &cmds[3] else {
        panic!()
    };
    let run = &text.runs[0];
    assert_eq!(run.color, Rgba::new(1.0, 0.0, 0.0, 1.0));
    assert_eq!(run.glyphs.len(), 8, "one glyph per character in Inter");
    // Glyphs start at the content box (padding 10) and sit on a baseline below it.
    assert!((run.glyphs[0].x - 10.0).abs() < 1.5, "{:?}", run.glyphs[0]);
    assert!(
        run.glyphs[0].y > 20.0 && run.glyphs[0].y < 40.0,
        "{:?}",
        run.glyphs[0]
    );
    assert!(
        run.glyphs.windows(2).all(|w| w[1].x > w[0].x),
        "glyphs advance left to right"
    );
    // The underline spans the run, just below the baseline, in the text colour.
    let platypus_ui::TextDecoration {
        rect: line, color, ..
    } = text.decorations[0];
    assert_eq!(color, Rgba::new(1.0, 0.0, 0.0, 1.0));
    assert!(
        line.y > run.glyphs[0].y && line.y < run.glyphs[0].y + 5.0,
        "{line:?}"
    );
    assert!(line.width > 40.0);
    // The sharp shadow is the same text moved by (2, 3) and tinted blue.
    assert_eq!(sharp_shadow.runs[0].color, Rgba::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(sharp_shadow.runs[0].glyphs[0].x, run.glyphs[0].x + 2.0);
    assert_eq!(sharp_shadow.runs[0].glyphs[0].y, run.glyphs[0].y + 3.0);
}

#[test]
fn text_alignment_moves_lines() {
    let mut tree = UiTree::new(300.0, 100.0);
    apply_ok(
        &mut tree,
        vec![
            create_kind(1, "text"),
            style(1, &[("width", 300.0.into())]),
            text_node(2, "short"),
            create_kind(3, "text"),
            style(
                3,
                &[("width", 300.0.into()), ("textAlign", "center".into())],
            ),
            text_node(4, "short"),
            create_kind(5, "text"),
            style(5, &[("width", 300.0.into()), ("textAlign", "right".into())]),
            text_node(6, "short"),
            create(7),
            style(7, &[("flexDirection", "column".into())]),
            append(1, 2),
            append(3, 4),
            append(5, 6),
            append(7, 1),
            append(7, 3),
            append(7, 5),
            append(0, 7),
        ],
    );
    let cmds = tree.draw_list();
    let xs: Vec<f32> = cmds
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Text(t) => Some(t.runs[0].glyphs[0].x),
            _ => None,
        })
        .collect();
    assert_eq!(xs.len(), 3);
    assert!(xs[0] < 2.0, "start: {xs:?}");
    assert!(xs[1] > 100.0 && xs[1] < 150.0, "centred: {xs:?}");
    assert!(xs[2] > 200.0, "right: {xs:?}");
}

// M4-HOT-09
#[test]
fn clearing_script_nodes_lets_a_reloaded_bundle_reuse_ids() {
    let mut tree = UiTree::new(200.0, 200.0);
    // An attached box with a child, a detached box, and an `:outro` rule that would
    // otherwise keep removed elements around.
    apply_ok(
        &mut tree,
        vec![
            create(1),
            style(
                1,
                &[
                    ("width", Value::Number(50.0)),
                    ("height", Value::Number(50.0)),
                ],
            ),
            append(0, 1),
            create(2),
            append(1, 2),
            create(3),
        ],
    );
    tree.add_stylesheet("box:outro { opacity: 0; transition: opacity 1s; }");
    tree.layout_if_needed();
    assert_eq!(tree.len(), 3);

    tree.clear_script_nodes();
    assert!(tree.is_empty(), "only the root is left");
    assert!(tree.children(NodeId(0)).is_empty());

    // The new bundle's reconciler starts at 1 again.
    apply_ok(&mut tree, vec![create(1), append(0, 1)]);
    tree.layout_if_needed();
    assert_eq!(ids(tree.children(NodeId(0))), [1]);
}

// M2-UI-OP-01
//
// `apply_ops` takes typed ops and gives the same tree as the script's arrays, and `Op::decode`
// checks arity and argument types once.
#[test]
fn typed_ops_build_the_same_tree_as_script_arrays() {
    let mut a = UiTree::new(100.0, 100.0);
    apply_ok(
        &mut a,
        vec![create(1), style(1, &[("width", 10.0.into())]), append(0, 1)],
    );
    let mut b = UiTree::new(100.0, 100.0);
    let errors = b.apply_ops(&[
        Op::Create {
            id: NodeId(1),
            kind: "box".into(),
        },
        Op::SetStyle {
            id: NodeId(1),
            pairs: vec![("width".into(), 10.0.into())],
        },
        Op::AppendChild {
            parent: ROOT,
            child: NodeId(1),
        },
    ]);
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(a.children(ROOT), b.children(ROOT));
    assert_eq!(
        Op::decode(&style(3, &[("width", 1.0.into())])).unwrap(),
        Op::SetStyle {
            id: NodeId(3),
            pairs: vec![("width".into(), 1.0.into())]
        }
    );
    assert!(Op::decode(&op(SET_STYLE, &[n(1), "width".into()])).is_err());
    assert!(Op::decode(&op(CREATE, &[n(1)])).is_err());
    assert!(Op::decode(&op(77, &[])).is_err());
}
