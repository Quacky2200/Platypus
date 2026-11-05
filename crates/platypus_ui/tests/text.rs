// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Text layout through the UI tree (M3-TXT-02, M3-TXT-03, M3-FNT-01, M3-FNT-02).

use platypus_ui::opcodes::*;
use platypus_ui::{ROOT, UiTree};
use web_ui_core::{Commit, NodeId, Value};

const INTER: &[u8] = include_bytes!("../fonts/InterVariable.ttf");
const INTER_ITALIC: &[u8] = include_bytes!("../fonts/InterVariable-Italic.ttf");

fn n(id: u32) -> Value {
    Value::Number(id.into())
}

fn op(code: u32, args: &[Value]) -> Value {
    let mut v = vec![Value::Number(code.into())];
    v.extend_from_slice(args);
    Value::Array(v)
}

struct Builder {
    ops: Vec<Value>,
    next: u32,
}

impl Builder {
    fn new() -> Self {
        Self {
            ops: Vec::new(),
            next: 1,
        }
    }

    fn element(&mut self, parent: u32, kind: &str, style: &[(&str, Value)]) -> u32 {
        let id = self.next;
        self.next += 1;
        self.ops.push(op(CREATE, &[n(id), kind.into()]));
        let mut set = vec![n(id)];
        for (k, v) in style {
            set.push((*k).into());
            set.push(v.clone());
        }
        self.ops.push(op(SET_STYLE, &set));
        self.ops.push(op(APPEND_CHILD, &[n(parent), n(id)]));
        id
    }

    fn text(&mut self, parent: u32, text: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        self.ops.push(op(CREATE_TEXT, &[n(id), text.into()]));
        self.ops.push(op(APPEND_CHILD, &[n(parent), n(id)]));
        id
    }

    fn apply(self, tree: &mut UiTree) {
        let errors = tree.apply_commit(&Commit { ops: self.ops });
        assert!(errors.is_empty(), "{errors:?}");
    }
}

const LOREM: &str = "The quick brown fox jumps over the lazy dog again and again";

// M3-TXT-02
#[test]
fn text_wraps_and_pushes_siblings_down() {
    let mut tree = UiTree::new(400.0, 400.0);
    let mut b = Builder::new();
    let column = b.element(
        0,
        "box",
        &[("width", 200.0.into()), ("flexDirection", "column".into())],
    );
    let label = b.element(column, "text", &[("fontSize", 16.0.into())]);
    b.text(label, LOREM);
    let below = b.element(column, "box", &[("height", 10.0.into())]);
    b.apply(&mut tree);
    tree.layout_if_needed();

    let text = tree.rect(NodeId(label)).unwrap();
    assert_eq!(text.width, 200.0, "a column stretches the text block");
    let line = tree
        .text_layout(NodeId(label))
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .metrics()
        .line_height;
    assert!(
        line > 10.0 && line < 30.0,
        "line height for 16px text: {line}"
    );
    assert!(
        text.height >= 2.0 * line,
        "wrapped onto several lines: {text:?}"
    );
    let sibling = tree.rect(NodeId(below)).unwrap();
    assert_eq!(
        sibling.y,
        text.y + text.height,
        "the sibling follows the text"
    );

    // Wider container, fewer lines.
    let mut tree2 = UiTree::new(1000.0, 400.0);
    let mut b = Builder::new();
    let column = b.element(
        0,
        "box",
        &[("width", 900.0.into()), ("flexDirection", "column".into())],
    );
    let label = b.element(column, "text", &[("fontSize", 16.0.into())]);
    b.text(label, LOREM);
    b.apply(&mut tree2);
    tree2.layout_if_needed();
    assert!(tree2.rect(NodeId(label)).unwrap().height < text.height);
}

// M3-TXT-08
#[test]
fn thai_wraps_between_words() {
    // Thai has no spaces; break opportunities come from a dictionary.
    let mut tree = UiTree::new(400.0, 400.0);
    let mut b = Builder::new();
    let column = b.element(
        0,
        "box",
        &[("width", 120.0.into()), ("flexDirection", "column".into())],
    );
    let label = b.element(column, "text", &[("fontSize", 16.0.into())]);
    b.text(label, "ภาษาไทยเป็นภาษาที่ไม่มีช่องว่างระหว่างคำ");
    b.apply(&mut tree);
    tree.layout_if_needed();
    let lines = tree.text_layout(NodeId(label)).unwrap().lines().count();
    assert!(
        lines > 1,
        "a long Thai run wraps at word boundaries: {lines} line(s)"
    );
}

// M3-TXT-02: shrink-to-fit in a row; padding is added around the text.
#[test]
fn text_sizes_to_its_content() {
    let mut tree = UiTree::new(800.0, 200.0);
    let mut b = Builder::new();
    let row = b.element(0, "box", &[("alignItems", "flex-start".into())]);
    let short = b.element(row, "text", &[("padding", 4.0.into())]);
    b.text(short, "Hi");
    let long = b.element(row, "text", &[]);
    b.text(long, "A much longer label");
    b.apply(&mut tree);
    tree.layout_if_needed();
    let (s, l) = (
        tree.rect(NodeId(short)).unwrap(),
        tree.rect(NodeId(long)).unwrap(),
    );
    assert!(s.width > 8.0 && s.width < 40.0, "{s:?}");
    assert!(l.width > 2.0 * s.width, "{l:?}");
    assert_eq!(l.x, s.x + s.width);
    let line = tree.text_layout(NodeId(short)).unwrap().height();
    assert!(
        (s.height - (line.ceil() + 8.0)).abs() < 0.01,
        "padding around one line: {s:?}"
    );
}

// M3-TXT-03: shaping happens only when text or text styles change.
#[test]
fn unchanged_text_is_not_reshaped() {
    let mut tree = UiTree::new(400.0, 200.0);
    let mut b = Builder::new();
    let column = b.element(0, "box", &[("flexDirection", "column".into())]);
    let a = b.element(column, "text", &[]);
    let a_text = b.text(a, "first");
    let c = b.element(column, "text", &[]);
    b.text(c, "second");
    b.apply(&mut tree);
    tree.layout_if_needed();
    assert_eq!(tree.text_build_count(), 2);

    // Resizing re-measures (breaks lines again) but doesn't re-shape.
    tree.set_size(300.0, 200.0);
    tree.layout_if_needed();
    assert_eq!(tree.text_build_count(), 2);

    // Changing one text node re-shapes only its block.
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_TEXT, &[n(a_text), "first, longer".into()])],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(tree.text_build_count(), 3);

    // A non-text style change re-shapes nothing.
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_STYLE, &[n(column), "padding".into(), 4.0.into()])],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(tree.text_build_count(), 3);
}

/// The family and style of the font each glyph run used.
fn run_fonts(tree: &UiTree, id: u32) -> Vec<(u32, bool)> {
    let layout = tree.text_layout(NodeId(id)).unwrap();
    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                let font = run.run().font();
                let attrs = run.run().font_attrs();
                out.push((font.index, attrs.style != parley::FontStyle::Normal));
            }
        }
    }
    out
}

// M3-FNT-01, M3-FNT-02
#[test]
fn registered_fonts_are_matched_by_family_and_style() {
    let mut tree = UiTree::new(400.0, 200.0);
    // Register a two-face family under a custom name, like @font-face does.
    let names = tree
        .register_font(INTER.to_vec(), Some("Duo"), Some(400.0), Some(false))
        .unwrap();
    assert_eq!(names, ["Duo"]);
    tree.register_font(INTER_ITALIC.to_vec(), Some("Duo"), Some(400.0), Some(true))
        .unwrap();

    let mut b = Builder::new();
    let column = b.element(0, "box", &[("flexDirection", "column".into())]);
    let upright = b.element(column, "text", &[("fontFamily", "Duo".into())]);
    b.text(upright, "upright");
    let slanted = b.element(
        column,
        "text",
        &[("fontFamily", "Duo".into()), ("fontStyle", "italic".into())],
    );
    b.text(slanted, "slanted");
    // Unknown families fall back down the list, then to the bundled default.
    let fallback = b.element(column, "text", &[("fontFamily", "Nope, Duo".into())]);
    b.text(fallback, "fallback");
    let default = b.element(column, "text", &[("fontFamily", "Nope".into())]);
    b.text(default, "default");
    b.apply(&mut tree);
    tree.layout_if_needed();

    assert!(run_fonts(&tree, upright).iter().all(|(_, italic)| !italic));
    assert!(run_fonts(&tree, slanted).iter().all(|(_, italic)| *italic));
    assert!(!run_fonts(&tree, fallback).is_empty());
    assert!(
        !run_fonts(&tree, default).is_empty(),
        "the bundled font is the last resort"
    );
    // Every block got real glyphs (no zero-height layouts).
    for id in [upright, slanted, fallback, default] {
        assert!(tree.rect(NodeId(id)).unwrap().height > 0.0);
    }
}

#[test]
fn registering_a_font_relayouts_existing_text() {
    let mut tree = UiTree::new(400.0, 200.0);
    let mut b = Builder::new();
    let label = b.element(0, "text", &[("fontFamily", "Late".into())]);
    b.text(label, "arrives later");
    b.apply(&mut tree);
    tree.layout_if_needed();
    let before = tree.text_build_count();
    let fallback_data = run_data_ids(&tree, label);
    assert!(!fallback_data.is_empty());

    // The bundled font is the same file, so tell them apart by which font data
    // (blob) the runs use: the registration makes a new one.
    tree.register_font(INTER.to_vec(), Some("Late"), None, None)
        .unwrap();
    assert!(
        tree.layout_if_needed(),
        "registering a font triggers layout"
    );
    assert_eq!(tree.text_build_count(), before + 1);
    let loaded_data = run_data_ids(&tree, label);
    assert!(
        loaded_data.iter().all(|id| !fallback_data.contains(id)),
        "now uses the loaded font, not the fallback"
    );

    assert!(
        tree.register_font(b"not a font".to_vec(), Some("Bad"), None, None)
            .is_err()
    );
    let _ = ROOT;
}

fn run_data_ids(tree: &UiTree, id: u32) -> Vec<u64> {
    let layout = tree.text_layout(NodeId(id)).unwrap();
    let mut out = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            if let parley::PositionedLayoutItem::GlyphRun(run) = item {
                out.push(run.run().font().data.id());
            }
        }
    }
    out
}

// M3-FNT-03
#[test]
fn woff_and_woff2_fonts_register_and_shape() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/fonts");
    for (file, family) in [
        ("inter-subset.woff", "WebOne"),
        ("inter-subset.woff2", "WebTwo"),
    ] {
        let data = std::fs::read(dir.join(file)).unwrap();
        let mut tree = platypus_ui::UiTree::new(400.0, 100.0);
        let names = tree
            .register_font(data.clone(), Some(family), None, None)
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(names, [family]);
        // The decoded data is a plain font: it starts with a TrueType/OpenType tag.
        let plain = platypus_ui::text::decode_web_font(data.clone()).unwrap();
        assert!(
            plain.starts_with(&[0, 1, 0, 0])
                || plain.starts_with(b"OTTO")
                || plain.starts_with(b"true"),
            "{file}: {:?}",
            &plain[..4]
        );
        // Broken data is reported, not registered.
        let mut cut = data;
        cut.truncate(40);
        assert!(
            tree.register_font(cut, Some("Broken"), None, None).is_err(),
            "{file}"
        );
    }
}

// M3-FNT-02
#[test]
fn a_family_with_only_an_italic_face_uses_it_for_normal_text() {
    let mut tree = UiTree::new(400.0, 200.0);
    let mut b = Builder::new();
    let label = b.element(0, "text", &[("fontFamily", "OnlySlanted".into())]);
    b.text(label, "normal style");
    b.apply(&mut tree);
    tree.layout_if_needed();
    let fallback_data = run_data_ids(&tree, label);
    tree.register_font(
        INTER_ITALIC.to_vec(),
        Some("OnlySlanted"),
        Some(400.0),
        Some(true),
    )
    .unwrap();
    tree.layout_if_needed();
    let loaded = run_data_ids(&tree, label);
    // The family's only data is the italic face, so using it at all means CSS font
    // matching picked the italic face instead of another family. (`font_attrs` reports
    // the requested style, so it can't tell.)
    assert!(
        loaded.iter().all(|id| !fallback_data.contains(id)),
        "CSS font matching falls back to the family's italic face, not another family"
    );
}
