// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Incremental layout (M2-LAY-07): appending to a long column lays out the new child,
//! not every sibling again, so streaming content in stays cheap as it grows.

use crate::common;

use common::Dom;
use platypus_ui::UiTree;
use web_ui_core::Commit;

const CSS: &str = ".row { flex-direction: row; width: 100%; height: 100% }
    .side { width: 120px }
    .scroller { flex-grow: 1; min-width: 0; flex-direction: column; overflow-y: auto }
    .page { flex-direction: column; flex-shrink: 0; padding: 10px }
    .chunk { flex-direction: column; flex-shrink: 0 }
    .p { margin-bottom: 8px; line-height: 1.5 }";

fn chunk(d: &mut Dom, page: u32) {
    let c = d.add(page, "box", &[]);
    d.set_class(c, "chunk");
    for i in 0..10 {
        let t = d.add(c, "text", &[]);
        d.set_class(t, "p");
        d.text(
            t,
            &format!("Paragraph {i}, long enough to wrap onto a second line here."),
        );
    }
}

/// Text measures spent appending one chunk to a page holding `chunks` of them,
/// inside a row (the page's scroller is sized from its content) or alone.
fn measures_to_append(chunks: usize, in_row: bool) -> u64 {
    let mut d = Dom::new();
    let scroller = if in_row {
        let row = d.add(0, "box", &[]);
        d.set_class(row, "row");
        let side = d.add(row, "box", &[]);
        d.set_class(side, "side");
        d.add(row, "box", &[])
    } else {
        d.add(0, "box", &[("height", "100%".into())])
    };
    d.set_class(scroller, "scroller");
    let page = d.add(scroller, "box", &[]);
    d.set_class(page, "page");
    for _ in 0..chunks {
        chunk(&mut d, page);
    }
    let next = d.next;
    let mut tree = UiTree::new(800.0, 600.0);
    tree.add_stylesheet(CSS);
    tree.apply_commit(&Commit { ops: d.ops });
    tree.layout_if_needed();

    let mut more = Dom {
        ops: Vec::new(),
        next,
    };
    chunk(&mut more, page);
    tree.apply_commit(&Commit { ops: more.ops });
    let before = tree.text_measure_count();
    tree.layout_if_needed();
    tree.text_measure_count() - before
}

// M2-LAY-07
#[test]
fn appending_to_a_long_column_measures_only_the_new_text() {
    for in_row in [false, true] {
        let short = measures_to_append(1, in_row);
        let long = measures_to_append(20, in_row);
        assert!(short > 0, "the new chunk is measured");
        assert_eq!(
            long, short,
            "the page's length doesn't matter (in a row: {in_row})"
        );
    }
}
