// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! M2-BEN-01..03: layout and commit costs on an editor-like UI of ~2,000 nodes.
//!
//! Run with `cargo bench -p platypus_ui`.

use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use platypus_ui::UiTree;
use platypus_ui::opcodes::*;
use web_ui_core::{Commit, Value};

struct Builder {
    ops: Vec<Value>,
    next: u32,
}

impl Builder {
    fn add(&mut self, parent: u32, style: &[(&str, Value)]) -> u32 {
        let id = self.next;
        self.next += 1;
        let n = |v: u32| Value::Number(v.into());
        self.ops
            .push(Value::Array(vec![n(CREATE), n(id), "box".into()]));
        let mut set = vec![n(SET_STYLE), n(id)];
        for (k, v) in style {
            set.push((*k).into());
            set.push(v.clone());
        }
        self.ops.push(Value::Array(set));
        self.ops
            .push(Value::Array(vec![n(APPEND_CHILD), n(parent), n(id)]));
        id
    }
}

/// An editor-like layout: toolbar, a file tree of 600 rows, a grid of cards and
/// an inspector of fields. Returns the commit and the id of one inspector input.
fn editor() -> (Commit, u32, usize) {
    let mut b = Builder {
        ops: Vec::new(),
        next: 1,
    };
    let app = b.add(
        0,
        &[
            ("width", "100%".into()),
            ("height", "100%".into()),
            ("flexDirection", "column".into()),
        ],
    );

    let toolbar = b.add(
        app,
        &[
            ("height", 40.0.into()),
            ("gap", 4.0.into()),
            ("padding", 4.0.into()),
            ("backgroundColor", "#222".into()),
        ],
    );
    for _ in 0..20 {
        b.add(
            toolbar,
            &[
                ("width", 32.0.into()),
                ("borderRadius", 4.0.into()),
                ("backgroundColor", "#444".into()),
            ],
        );
    }

    let body = b.add(app, &[("flexGrow", 1.0.into())]);
    let sidebar = b.add(
        body,
        &[("width", 260.0.into()), ("flexDirection", "column".into())],
    );
    for i in 0..600u32 {
        let depth = f64::from(i % 5);
        let row = b.add(
            sidebar,
            &[
                ("height", 22.0.into()),
                ("paddingLeft", (depth * 12.0).into()),
                ("alignItems", "center".into()),
                ("gap", 6.0.into()),
            ],
        );
        b.add(
            row,
            &[
                ("width", 16.0.into()),
                ("height", 16.0.into()),
                ("backgroundColor", "#888".into()),
            ],
        );
        b.add(
            row,
            &[
                ("flexGrow", 1.0.into()),
                ("height", 12.0.into()),
                ("backgroundColor", "#666".into()),
            ],
        );
    }

    let main = b.add(
        body,
        &[
            ("flexGrow", 1.0.into()),
            ("display", "grid".into()),
            ("gridTemplateColumns", "repeat(4, 1fr)".into()),
            ("gap", 8.0.into()),
            ("padding", 8.0.into()),
        ],
    );
    for _ in 0..150 {
        let card = b.add(
            main,
            &[
                ("height", 120.0.into()),
                ("padding", 8.0.into()),
                ("flexDirection", "column".into()),
                ("borderRadius", 8.0.into()),
                ("boxShadow", "0 2px 6px rgba(0,0,0,.4)".into()),
            ],
        );
        b.add(
            card,
            &[("flexGrow", 1.0.into()), ("backgroundColor", "#357".into())],
        );
    }

    let inspector = b.add(
        body,
        &[
            ("width", 300.0.into()),
            ("flexDirection", "column".into()),
            ("gap", 4.0.into()),
            ("padding", 8.0.into()),
        ],
    );
    let mut input = 0;
    for _ in 0..40 {
        let field = b.add(inspector, &[("height", 24.0.into()), ("gap", 8.0.into())]);
        b.add(field, &[("width", 100.0.into())]);
        input = b.add(
            field,
            &[
                ("flexGrow", 1.0.into()),
                ("border", "1px solid #555".into()),
            ],
        );
    }
    let nodes = (b.next - 1) as usize;
    (Commit { ops: b.ops }, input, nodes)
}

fn set_width(id: u32, width: f64) -> Commit {
    let n = |v: u32| Value::Number(v.into());
    Commit {
        ops: vec![Value::Array(vec![
            n(SET_STYLE),
            n(id),
            "width".into(),
            width.into(),
        ])],
    }
}

fn benches(c: &mut Criterion) {
    let (commit, input, nodes) = editor();
    eprintln!("editor tree: {nodes} nodes, {} ops", commit.ops.len());

    // M2-BEN-03: applying an op buffer that creates the whole tree.
    c.bench_function("apply_commit_2000_nodes", |b| {
        b.iter_batched(
            || (UiTree::new(1280.0, 720.0), commit.clone()),
            |(mut tree, commit)| {
                let errors = tree.apply_commit(&commit);
                black_box((tree, errors))
            },
            BatchSize::LargeInput,
        )
    });

    // M2-BEN-01: full layout of a freshly built tree (cold caches).
    c.bench_function("full_layout_2000_nodes", |b| {
        b.iter_batched(
            || {
                let mut tree = UiTree::new(1280.0, 720.0);
                tree.apply_commit(&commit);
                tree
            },
            |mut tree| {
                tree.layout_if_needed();
                black_box(tree)
            },
            BatchSize::LargeInput,
        )
    });

    // M2-BEN-02: relayout after changing one leaf's width (warm caches).
    let mut tree = UiTree::new(1280.0, 720.0);
    tree.apply_commit(&commit);
    tree.layout_if_needed();
    let mut toggle = false;
    c.bench_function("relayout_one_leaf_change", |b| {
        b.iter(|| {
            toggle = !toggle;
            tree.apply_commit(&set_width(input, if toggle { 120.0 } else { 140.0 }));
            black_box(tree.layout_if_needed());
        })
    });

    // For reference: a window resize relayouts everything with warm caches.
    let mut wide = true;
    c.bench_function("relayout_after_resize", |b| {
        b.iter(|| {
            wide = !wide;
            tree.set_size(if wide { 1280.0 } else { 1200.0 }, 720.0);
            black_box(tree.layout_if_needed());
        })
    });

    // For reference: building the draw list (layout already done).
    c.bench_function("draw_list_2000_nodes", |b| {
        b.iter(|| black_box(tree.draw_list()))
    });
}

criterion_group!(layout, benches);
criterion_main!(layout);
