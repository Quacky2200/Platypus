// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// SPDX-License-Identifier: Apache-2.0

//! Tables built from a grid whose rows are `display: contents` boxes (`examples/table`):
//! the existing layout covers what a table needs, so there are no table elements.

use crate::common;

use common::{Dom, px};
use platypus_ui::{Rect, UiTree};
use web_ui_core::NodeId;

/// The stylesheet of `examples/table`.
const CSS: &str = include_str!("../../../examples/table/src/ui.css");

/// A table of `rows` (cells as text) under a block `width` wide. Returns the tree, the table
/// and its cells by row.
fn table(width: f64, rows: &[&[&str]], classes: &[&str]) -> (UiTree, u32, Vec<Vec<u32>>) {
    let mut d = Dom::new();
    let page = d.add(
        0,
        "box",
        &[("display", "block".into()), ("width", px(width))],
    );
    let t = d.add(page, "box", &[]);
    d.set_class(t, "table");
    let mut cells = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let row_el = d.add(t, "box", &[]);
        d.set_class(row_el, if r == 0 { "row head" } else { "row" });
        let mut ids = Vec::new();
        for (c, text) in row.iter().enumerate() {
            let cell = d.add(row_el, "box", &[]);
            d.set_class(cell, classes.get(c).copied().unwrap_or("cell"));
            d.text(cell, text);
            ids.push(cell);
        }
        cells.push(ids);
    }
    (d.build(CSS), t, cells)
}

fn rect(tree: &UiTree, id: u32) -> Rect {
    tree.rect(NodeId(id)).unwrap()
}

const NOTE: &str = "A long note that has to wrap inside its column because the table is narrow";

// M3-TBL-02
#[test]
fn columns_are_sized_from_the_content_of_every_row() {
    let rows: [&[&str]; 3] = [
        &["Item", "Kind", "Notes", "Qty", "Price"],
        &["Iron sword", "Weapon", "Short", "1", "120"],
        &["Potion", "Consumable", "Heals.", "12", "15"],
    ];
    let (tree, t, cells) = table(400.0, &rows, &[]);
    assert_eq!(rect(&tree, t).width, 400.0);
    for c in 0..5 {
        let x = rect(&tree, cells[0][c]).x;
        let w = rect(&tree, cells[0][c]).width;
        for row in &cells {
            assert_eq!(rect(&tree, row[c]).x, x, "column {c} lines up");
            assert_eq!(rect(&tree, row[c]).width, w);
        }
    }
    // The widest cell sets a content column: "Consumable" is wider than "Weapon" and "Kind".
    let kind = rect(&tree, cells[0][1]).width;
    let item = rect(&tree, cells[0][0]).width;
    assert!(kind > 80.0 && item > 60.0, "{item} {kind}");
    // The `1fr` Notes column takes the rest.
    let widths: f32 = (0..5).map(|c| rect(&tree, cells[0][c]).width).sum();
    assert!(
        (widths - (400.0 - 2.0)).abs() < 1.5,
        "{widths} {:?}",
        (0..5).map(|c| rect(&tree, cells[0][c])).collect::<Vec<_>>()
    );
}

// M3-TBL-03
#[test]
fn a_long_cell_wraps_and_its_row_grows_with_it() {
    let rows: [&[&str]; 3] = [
        &["Item", "Kind", "Notes", "Qty", "Price"],
        &["Rope", "Tool", "50 m", "200", "2"],
        &["Shield", "Armour", NOTE, "3", "80"],
    ];
    let (tree, _, cells) = table(400.0, &rows, &[]);
    let short = rect(&tree, cells[1][2]);
    let long = rect(&tree, cells[2][2]);
    assert!(long.height > short.height * 2.0, "{short:?} {long:?}");
    // Every cell of the long row is as tall as it, whatever its content.
    for cell in &cells[2] {
        assert_eq!(rect(&tree, *cell).height, long.height);
        assert_eq!(rect(&tree, *cell).y, long.y);
    }
    // And the table ends below it.
    assert!(rect(&tree, cells[2][0]).y > rect(&tree, cells[1][0]).y);
}

// M3-TBL-04
#[test]
fn numbers_align_to_the_right_edge_of_their_cell() {
    let rows: [&[&str]; 3] = [
        &["Item", "Kind", "Notes", "Qty", "Price"],
        &["Rope", "Tool", "50 m", "200", "2"],
        &["Gem", "Treasure", "Very shiny", "7", "4000"],
    ];
    let (mut tree, _, cells) = table(
        500.0,
        &rows,
        &["cell", "cell", "cell", "cell num", "cell num"],
    );
    // The text block of the Qty cells ends at the cell's padding edge in both rows.
    let right = |tree: &UiTree, cell: u32| {
        let r = rect(tree, cell);
        r.x + r.width
    };
    let texts = tree.draw_list();
    let ends: Vec<f32> = texts
        .iter()
        .filter_map(|c| match c {
            platypus_ui::DrawCmd::Text(t) => Some(
                t.runs
                    .iter()
                    .flat_map(|r| &r.glyphs)
                    .map(|g| g.x + g.advance)
                    .fold(0.0_f32, f32::max),
            ),
            _ => None,
        })
        .collect();
    let qty_edge = right(&tree, cells[1][3]) - 12.0;
    assert!(
        ends.iter().any(|e| (e - qty_edge).abs() < 1.0),
        "a Qty value ends at {qty_edge}: {ends:?}"
    );
}

// M3-TBL-05
#[test]
fn spans_act_as_colspan_and_rowspan() {
    let mut d = Dom::new();
    let page = d.add(
        0,
        "box",
        &[("display", "block".into()), ("width", px(400.0))],
    );
    let t = d.add(page, "box", &[]);
    d.set_class(t, "table");
    // Row 1: a cell over two rows and a cell over three columns; row 2: the rest.
    let r1 = d.add(t, "box", &[]);
    d.set_class(r1, "row");
    let tall = d.add(r1, "box", &[]);
    d.set_class(tall, "cell span-rows");
    d.text(tall, "A");
    let wide = d.add(r1, "box", &[]);
    d.set_class(wide, "cell span-3");
    d.text(wide, "Wide");
    let last = d.add(r1, "box", &[]);
    d.set_class(last, "cell");
    d.text(last, "Z");
    let r2 = d.add(t, "box", &[]);
    d.set_class(r2, "row");
    let mut below = Vec::new();
    for text in ["b", "c", "d", "e"] {
        let c = d.add(r2, "box", &[]);
        d.set_class(c, "cell");
        d.text(c, text);
        below.push(c);
    }
    let tree = d.build(CSS);
    let (tall, wide, b0) = (rect(&tree, tall), rect(&tree, wide), rect(&tree, below[0]));
    // The tall cell covers both rows; the wide one starts after it and covers three columns.
    assert!(tall.height > b0.height * 1.8, "{tall:?} {b0:?}");
    assert!(wide.x >= tall.x + tall.width - 0.5);
    let c = |i: usize| rect(&tree, below[i]);
    assert!((wide.width - (c(0).width + c(1).width + c(2).width)).abs() < 1.5);
    // The second row's cells start after the tall cell's column: b, c, d, e skip column 0.
    assert!(c(0).x >= tall.x + tall.width - 0.5);
}

// M3-TBL-06
#[test]
fn a_display_contents_row_is_hovered_through_its_cells() {
    let rows: [&[&str]; 2] = [&["Item", "Kind"], &["Rope", "Tool"]];
    let (mut tree, _, cells) = table(400.0, &rows, &[]);
    let cell = rect(&tree, cells[1][1]);
    let row = tree.parent(NodeId(cells[1][1])).unwrap();
    let mut pointer = platypus_ui::Pointer::new();
    pointer.move_to(&mut tree, cell.x + 5.0, cell.y + 5.0);
    assert!(tree.state(row).hover, "the row is hovered with its cell");
    // `.row:hover .cell` paints the cells of the row, not those of the other rows.
    tree.layout_if_needed();
    let colour = |tree: &UiTree, id: u32| tree.visual(NodeId(id)).unwrap().background;
    assert_ne!(colour(&tree, cells[1][0]), colour(&tree, cells[0][0]));
    assert_eq!(colour(&tree, cells[1][0]), colour(&tree, cells[1][1]));
}
