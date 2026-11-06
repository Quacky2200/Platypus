# table

A table built from existing layout: a CSS grid whose rows are `display: contents`. Columns are sized from
every row's content, the long note wraps inside its `1fr` column, rows stripe and highlight on hover
through descendant selectors (`.row:hover .cell`), headers sort, a click selects a row, and
`grid-column` / `grid-row` spans act as `colspan` / `rowspan`. There are no table elements: this covers
what was tested (content-sized columns, header, striping, borders, alignment, wrapping, spans).

What it does not do: `display: table*` on arbitrary elements, `border-collapse`, `<col>` widths,
`position: sticky` headers, and a box for the row itself (a `contents` row has no rect, so it cannot take a
background, border or `getBoundingClientRect`; style its cells instead).

## Build and run

```bash
cd tools && cargo run -q -p platypus_build -- build ../examples/table
```

Then from the repo root (`cd ..`):

```bash
cargo run -p platypus_app -- examples/table --screenshot out.png --frames 30
```

## Structure

| File | What |
|---|---|
| `src/main.tsx` | `App`: header row with sorting, data rows, a footer row with a column span |
| `src/ui.css` | The grid, striping, hover, spans; shared by the golden `M3-TBL-01` and the tests in `crates/web_ui_dom/tests/table_grid.rs` |
