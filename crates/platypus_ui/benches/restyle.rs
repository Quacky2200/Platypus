// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! M3-BEN-01: CSS restyle cost on an editor-like UI of ~2,250 nodes with a
//! realistic stylesheet. Run with `cargo bench -p platypus_ui --bench restyle`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use platypus_ui::opcodes::*;
use platypus_ui::{ElementState, UiTree};
use web_ui_core::{Commit, NodeId, Value};

pub const STYLESHEET: &str = r#"
:root { --accent: #3fa9f5; --row-h: 22px; --pad: 8px }
.app { width: 100%; height: 100%; flex-direction: column }
.toolbar { height: 40px; gap: 4px; padding: 4px; background-color: #222 }
.toolbar > .tool { width: 32px; border-radius: 4px; background-color: #444 }
.toolbar > .tool:hover { background-color: var(--accent) }
.toolbar > .tool:first-child { margin-right: 12px }
.body { flex-grow: 1 }
.sidebar { width: 260px; flex-direction: column }
.sidebar .row { height: var(--row-h); align-items: center; gap: 6px }
.sidebar .row:hover { background-color: #2a2f38 }
.sidebar .row.selected { background-color: var(--accent) }
.sidebar .row:nth-child(even) { background-color: #1d2128 }
.sidebar .row .icon { width: 16px; height: 16px; background-color: #888 }
.sidebar .row .label { flex-grow: 1; height: 12px; background-color: #666 }
.sidebar .row.selected .label { background-color: white }
.main { flex-grow: 1; display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--pad); padding: var(--pad) }
.card { height: 120px; padding: 8px; flex-direction: column; border-radius: 8px; box-shadow: 0 2px 6px rgba(0,0,0,.4) }
.card:hover { box-shadow: 0 6px 16px rgba(0,0,0,.6) }
.card:nth-child(4n+1) { border: 1px solid var(--accent) }
.card > .thumb { flex-grow: 1; background-color: #357 }
.card:has(.thumb.wide) { grid-column: span 2 }
.inspector { width: 300px; flex-direction: column; gap: 4px; padding: 8px }
.inspector > .field { height: 24px; gap: 8px }
.inspector > .field + .field { border-top-width: 1px; border-color: #333 }
.field > .name { width: 100px }
.field > .input { flex-grow: 1; border: 1px solid #555 }
.field:focus-within > .input { border: 1px solid var(--accent) }
.field > .input:disabled { opacity: 0.5 }
#status { height: 20px }
"#;

struct Builder {
    ops: Vec<Value>,
    next: u32,
}

impl Builder {
    fn add(&mut self, parent: u32, class: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        let n = |v: u32| Value::Number(v.into());
        self.ops
            .push(Value::Array(vec![n(CREATE), n(id), "box".into()]));
        self.ops.push(Value::Array(vec![
            n(SET_ATTRS),
            n(id),
            "className".into(),
            class.into(),
        ]));
        self.ops
            .push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
        id
    }
}

/// The editor tree; returns (commit, a sidebar row in the middle, a card, an input).
pub fn editor() -> (Commit, u32, u32, u32, usize) {
    let mut b = Builder {
        ops: Vec::new(),
        next: 1,
    };
    let app = b.add(0, "app");
    let toolbar = b.add(app, "toolbar");
    for _ in 0..20 {
        b.add(toolbar, "tool");
    }
    let body = b.add(app, "body");
    let sidebar = b.add(body, "sidebar");
    let mut middle_row = 0;
    for i in 0..600 {
        let row = b.add(sidebar, "row");
        b.add(row, "icon");
        b.add(row, "label");
        if i == 300 {
            middle_row = row;
        }
    }
    let main = b.add(body, "main");
    let mut card = 0;
    for i in 0..150 {
        let c = b.add(main, "card");
        b.add(c, if i % 10 == 0 { "thumb wide" } else { "thumb" });
        if i == 75 {
            card = c;
        }
    }
    let inspector = b.add(body, "inspector");
    let mut input = 0;
    for _ in 0..40 {
        let field = b.add(inspector, "field");
        b.add(field, "name");
        input = b.add(field, "input");
    }
    let nodes = (b.next - 1) as usize;
    (Commit { ops: b.ops }, middle_row, card, input, nodes)
}

fn set_class(id: u32, class: &str) -> Commit {
    let n = |v: u32| Value::Number(v.into());
    Commit {
        ops: vec![Value::Array(vec![
            n(SET_ATTRS),
            n(id),
            "className".into(),
            class.into(),
        ])],
    }
}

fn styled_tree(commit: &Commit) -> UiTree {
    let mut tree = UiTree::new(1280.0, 720.0);
    tree.apply_commit(commit);
    let added = tree.add_stylesheet(STYLESHEET);
    assert!(added.warnings.is_empty(), "{:?}", added.warnings);
    tree.layout_if_needed();
    tree
}

fn benches(c: &mut Criterion) {
    let (commit, row, card, input, nodes) = editor();
    eprintln!("restyle tree: {nodes} nodes");

    // Full style + layout of the whole tree after adding the stylesheet.
    c.bench_function("style_whole_tree", |b| {
        b.iter_batched(
            || {
                let mut tree = UiTree::new(1280.0, 720.0);
                tree.apply_commit(&commit);
                tree
            },
            |mut tree| {
                tree.add_stylesheet(STYLESHEET);
                tree.layout_if_needed();
                black_box(tree)
            },
            BatchSize::LargeInput,
        )
    });

    // M3-BEN-01: one class change in the middle of a 600-row list.
    let mut tree = styled_tree(&commit);
    let mut selected = false;
    c.bench_function("restyle_one_class_change", |b| {
        b.iter(|| {
            selected = !selected;
            tree.apply_commit(&set_class(
                row,
                if selected { "row selected" } else { "row" },
            ));
            black_box(tree.layout_if_needed());
        })
    });

    // Hovering a card (state change).
    let mut hovered = false;
    c.bench_function("restyle_hover_state", |b| {
        b.iter(|| {
            hovered = !hovered;
            tree.set_state(
                NodeId(card),
                ElementState {
                    hover: hovered,
                    ..Default::default()
                },
            );
            black_box(tree.layout_if_needed());
        })
    });

    // Focusing an input (:focus-within on its field).
    let mut focused = false;
    c.bench_function("restyle_focus_within", |b| {
        b.iter(|| {
            focused = !focused;
            tree.set_state(
                NodeId(input),
                ElementState {
                    focus: focused,
                    ..Default::default()
                },
            );
            black_box(tree.layout_if_needed());
        })
    });
}

criterion_group!(restyle, benches);
criterion_main!(restyle);
