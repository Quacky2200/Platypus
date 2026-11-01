# UI tree, styles, layout, text and the draw list

Crate: [platypus_ui](../crates/platypus_ui/src). Entry type: `UiTree` in
[tree.rs](../crates/platypus_ui/src/tree.rs). Pipeline overview: [README.md](README.md). Design
background: [DESIGN.md](../DESIGN.md) section 5. CSS stylesheets are covered in [css.md](css.md).

```
Commit ops --apply_commit--> Node map + dirty sets
                                  |  layout_if_needed()
                                  v
        restyle_dirty -> shape dirty text -> Taffy compute -> absolute Rects
                                  |  draw_list()
                                  v
                          Vec<DrawCmd>  -->  platypus_render
```

## Node model

`UiTree<L: LayoutEngine = TaffyLayout>` stores `HashMap<NodeId, Box<Node>>` (boxed because a node
is large). A `Node` holds:

| Field | Meaning |
|---|---|
| `kind` | `"box"`, `"text"`, `"span"`, `"input"`, `"textarea"`, `"image"` (`IMAGE`), `"list"` (`LIST`), `"dialog"` (`DIALOG`), `"#text"` (`TEXT_NODE`), or `"root"` |
| `parent`, `children` | Tree links (ordered) |
| `style: taffy::Style` | Layout properties |
| `visual: Visual` | Paint-only properties ([style.rs](../crates/platypus_ui/src/style.rs)) |
| `text` | Characters of a `#text` node |
| `attrs: Attrs`, `state: ElementState` | `className`, `id`, `disabled`, `checked`, `tabIndex`, `dir`, text field props (`value`, `defaultValue`, `placeholder`, `readOnly`, `rows`), `src` (images), `open`/`modal`/`closedBy` (dialogs, `ClosedBy`), `data-*`; `:hover`/`:active`/`:focus`/`:focus-visible` ([attrs.rs](../crates/platypus_ui/src/attrs.rs)) |
| `inline` | Inline `style` declarations, in the order set (later wins) |
| `generated` | `Some(Before/After)` for stylesheet-generated content |
| `vars` | Computed custom properties (`Rc`, shared with the parent when unchanged) |

Ids: `ROOT` is node 0 (always the window size, see `root_style`). React allocates ids from 1 and
never reuses them. Generated `::before`/`::after`/`::marker`/`::backdrop` nodes use ids from `0x8000_0000`
(`FIRST_GENERATED_ID`), so they can never collide with React's.

## Commits and opcodes

[opcodes.rs](../crates/platypus_ui/src/opcodes.rs) defines eight opcodes (the
JS `opcodes.ts` that mirrored them is deleted; the native reconciler builds typed `Op`s directly):

| Opcode | Args | Effect |
|---|---|---|
| `CREATE` 0 | id, type | Detached element |
| `SET_STYLE` 1 | id, (prop, value)* | `null` resets a property |
| `APPEND_CHILD` 2 | parent, child | Also *moves* an attached node |
| `INSERT_BEFORE` 3 | parent, child, before | Also moves |
| `REMOVE_CHILD` 4 | parent, child | Destroys the whole subtree |
| `CREATE_TEXT` 5 | id, text | `#text` node |
| `SET_TEXT` 6 | id, text | |
| `SET_ATTRS` 7 | id, (name, value)* | `null` removes |

`UiTree::apply_commit` decodes ops in order via `apply_op`. A bad op yields a `UiError` and is
skipped; the rest still apply, so the tree is always valid (`attach` refuses cycles, text-node
parents and unknown ids). Per-commit bookkeeping is collected in local sets
(`children_changed`, `styles_changed`, `text_changed`, `text_style_changed`) and flushed **once
per commit**: styles are pushed to the layout engine, child lists are re-sent (inline containers
get none, see below), text blocks are marked dirty. Finally `needs_layout` is set.

`SET_STYLE` applies the value immediately through `apply_style` *and* records it in `node.inline`
so the cascade can re-apply it later (see [css.md](css.md)). Values containing `var(`, CSS-wide keywords
(`inherit` etc.) or `--custom` props are deferred to restyle because they depend on inherited variables.

### Node lifetime

`REMOVE_CHILD` destroys the subtree; a node created but never attached stays in the tree until
something removes it. `UiTree::clear_script_nodes` is the bulk version used by script hot reload
([scripting.md](scripting.md#hot-reload)): it removes every child of `ROOT` through a normal
commit, then destroys every remaining script-created node (id below `FIRST_GENERATED_ID`) that was
never attached. While it runs, the `clearing` flag skips `:outro` exit animations. The result is an
empty root, so a new bundle's reconciler can reuse ids from 1 (M4-HOT-09).

## Styles

[style.rs](../crates/platypus_ui/src/style.rs) `apply_style(&mut Style, &mut Visual, prop, &Value)` is
the single place where a property name (camelCase) and a `Value` become state. Numbers are logical
pixels; strings are CSS values parsed by Taffy's parser (layout) or Platypus's own parsers (paint).
Both inline styles and stylesheet declarations (after kebab-case to camelCase) go through it, so
the two paths cannot drift. On error nothing changes (`StyleError`).

- Layout properties land in `taffy::Style` (flex, grid, margin, padding, border widths, inset...).
- Paint properties land in `Visual`: colours (with `currentColor` flags), border radius
  (`Corners<CornerRadius>`, each a horizontal and vertical `Len`), `shadows`, background layer lists, border colours, styles and image, `outline_*`, `mix_blend`, `filter_blur`, `filter_matrix`,
  `drop_shadows`, `backdrop_blur`, `backdrop_matrix`, mask lists (`mask_images/sizes/positions/repeats/modes`), `opacity`, `pointer_events`, `order`, `z_index`, `isolate`, `contents`, `fixed`, `overflow`, `clip_path`, `clip_rect`,
  `scrollbar_width/color`, `cursor`, `transform: Option<Arc<Transforms>>` (all transform properties; `None` at defaults) and `text: TextProps`.
- [paint.rs](../crates/platypus_ui/src/paint.rs) holds the parsers: radii, shadow lists, `Filter` chains (`ColorMatrix`, `DropShadow`),
  backgrounds (`linear-gradient`, `radial-gradient`, `conic-gradient`, `repeating-*`, `url()`),
  `parse_layers` for comma lists, `BlendMode`, `MaskMode`, `BorderStyle`, `BorderImage`, `clip-path` shapes
  (`ClipPath` resolved to a `ClipShape` against the box), `Overflow`.
  [svg_path.rs](../crates/platypus_ui/src/svg_path.rs) turns a `path()` string into polygon contours.
  [svg.rs](../crates/platypus_ui/src/svg.rs) holds the inline `<svg>` helpers (see [svg.md](svg.md)).
- [color.rs](../crates/platypus_ui/src/color.rs): `Rgba::parse` for hex, `rgb()`, `hsl()`, named colours; modern colour functions and
  `color-mix()` live in [color_fn.rs](../crates/platypus_ui/src/color_fn.rs) (see [css.md](css.md#colours)).
- [text_style.rs](../crates/platypus_ui/src/text_style.rs): `TextProps` are the *specified* text
  properties of one node; `ComputedText::child(&TextProps)` applies CSS inheritance.
  `UiTree::computed_text(id)` folds root-to-node. `is_text_prop` lets the tree know a style change
  affects text shaping. `direction` (`ltr`/`rtl`, enum `Direction`) is an inherited text property.
  The `dir` attribute (`ltr`/`rtl`, case-insensitive; `auto` is rejected) is visible to selectors as
  `[dir=...]` and fills in `direction` only when no CSS or inline style sets it (like the HTML UA
  rule `[dir=rtl] { direction: rtl }`): in `restyle` after the cascade, and via `dir_fallback` when
  there are no stylesheets. A `dir` change restyles and invalidates `[dir]` selectors.

### Layout shorthands (M3d step 2a)

All in `apply_style`/`copy_prop` ([style.rs](../crates/platypus_ui/src/style.rs)); tests in
[layout_props.rs](../crates/platypus_ui/tests/layout_props.rs).

- **`placeItems` / `placeSelf` / `placeContent`:** `<align> [<justify>]`; one value sets both axes.
- **Grid:**
  - `gridTemplateAreas`: quoted rows (`"head head" "side main"`, `none` clears). Rows must have equal
    cell counts and each name must fill a rectangle; `.` cells are empty. Otherwise it is invalid.
  - `gridArea`: an area name, or four lines `row-start / column-start / row-end / column-end`
    (omitted ends copy a named start, else `auto`; more than four is invalid).
  - `gridTemplate`: `rows / columns`, or area strings with optional row sizes
    (`"a b" 40px "c c" 1fr / 1fr 2fr`; rows without a size are `auto`). `none` resets rows, columns,
    line names and areas.
  - `gridAutoRows` / `gridAutoColumns`: single track sizes only (no `repeat()`).
  - `gridRowStart/End` and `gridColumnStart/End` longhands (number, `span n` or a line name).
- **Logical properties** map for horizontal left-to-right writing: block = top/bottom, inline =
  left/right. `margin|padding|inset` + `Block`/`Inline` (+ `Start`/`End`) and `blockSize`,
  `inlineSize` with `min`/`max` forms (to height/width). The two-value forms (`marginBlock: "5px 7px"`)
  apply atomically: both halves are checked first, so an error changes nothing. More than two values
  is invalid.

### Order and z-index (M3d step 2b)

- **`order`** (i32): `layout_order` gives Taffy the children stably sorted by `order`
  (`sync_children`, the `children_changed` loop). Changing `order` re-syncs the parent, in restyle
  and in the inline `SET_STYLE` fast path.
- **`zIndex`** (integer or `auto`, which is `None`): `paint_order` sorts siblings by
  (`zIndex` or 0, `order`), stable. `collect` paints in that order and `hit` walks it in reverse.
  Each z-indexed element is its own stacking context, so z-index only orders siblings.
- **`isolation`** is accepted and has no effect, for the same reason.
- Tab order stays in DOM order.

### Contents and fixed (M3d step 2c)

Taffy has neither, so `Visual` carries `contents` and `fixed` flags.

- **`display: contents`** keeps `Style::display = Block`. `layout_order` flattens through contents
  children (`flatten_contents`), skips fixed children, then stably sorts by `order`.
  `set_layout_children` (used by `sync_children` and the `children_changed` loop) gives a contents
  node no Taffy children of its own: they go to its parent. The node paints nothing and is never a
  hit target, but stays in event paths.
- **`position: fixed`** sets `Position::Absolute`. `set_layout_children` appends every fixed node to
  the root's Taffy children, so insets resolve against the window. `UiTree.fixed` (kept by
  `update_fixed`) tracks them; `fixed_nodes(rendered_only)` walks the tree, and `fixed_paint_order`
  sorts by z-index.
- **Paint:** fixed nodes draw after the root, using their parent's computed text. **Hit:**
  `hit_test` tests fixed nodes first in reverse paint order, inheriting the parent's
  `pointer_events_of`.
- **Containing blocks of absolute boxes.** Taffy only knows `position`, so a static element for which
  `style::makes_containing_block` holds (a transform, `translate`/`rotate`/`scale`, `perspective`,
  `transform-style: preserve-3d`, a filter, a backdrop filter, `contain: layout|paint|strict|content`, or
  a `willChange` naming one of them) is given `relative` without insets in the style Taffy sees
  (`layout_style`, kept current by `sync_layout_style`). The tree keeps the authored style, so painting,
  z-order and scripts still see `static`. A transform coming or going by class, hover or inline style
  re-lays out. Fixed boxes are unchanged (the root lays them out).
- **Limits:** fixed ignores ancestor clips, opacity and filters. A `display: none` or `opacity: 0`
  ancestor still hides it.

Relative units (`em`, `rem`, viewport units) and `calc()`/`min()`/`max()`/`clamp()` in either path
are resolved to pixels at restyle before `apply_style` sees them (so are the absolute units `in`, `cm`, `mm`, `Q`, `pt`, `pc`, at 96 px per inch); see [css.md](css.md#relative-units-and-math-unitsrs).

`Visual` has a field-wise `PartialEq` so restyle can detect "no visible change".

## Layout

[layout.rs](../crates/platypus_ui/src/layout.rs) hides Taffy behind `trait LayoutEngine`
(`create_node`, `set_style`, `set_children`, `set_measured`, `set_text_block`, `mark_dirty`, `compute`, `layout`).
`taffy::Style` doubles as the backend-neutral style type. Why the trait: DESIGN.md section 5.3 keeps
Yoga as a fallback, and the JSON fixtures in `tests/layout` are generic over the engine (see
[testing.md](testing.md)). `TaffyLayout` keeps its own node storage and runs Taffy's algorithms over it; layout results are
cached per node, so only dirty subtrees recompute.

Why not wrap `TaffyTree`: its `resolve_calc_value` always returns 0, so `calc()` with `%` could not
work. `TaffyLayout` instead holds a map of `NodeData` (style, children, parent, measured flag,
unrounded/final layout, hoisted out-of-flow children, cache) and a per-pass `View` that implements
Taffy's tree traits, mirroring `TaffyTree`'s own view: dirty propagation up the parents
(`mark_dirty_up`, stopping at an already-clear cache), `compute_cached_layout` caching, `round_layout`
rounding, and hoisting of out-of-flow (absolute) children (to the nearest positioned ancestor; `position: static` is in the style table, and a `NodeLayout` position of a hoisted child is relative to that box, see `containing_block`). Besides Taffy's `Cache` (nine measure
entries, evicting), each node keeps `sizes`: up to `MAX_SIZES` (32) exact `ComputeSize` inputs with their
results, oldest dropped. Intrinsic sizing through nested containers (a scroller inside a row flex) asks one
node more ways than nine, so every pass re-measured whole subtrees (appending a child to a long column
re-measured every sibling's text). `cache_get` tries Taffy's cache, then `sizes`; `clear_caches` clears
both (`M2-LAY-07`). `View` resolves calc handles through
`crate::calc::resolve`, for containers (`resolve_calc_value`) and for measured leaves
(`compute_leaf_layout`; the measure callback sizes text content).

### Floats (`float`, `clear`; `M3-FLT-*`)

Taffy's `float_layout` feature does the box side: a `float: left | right` child of a block container
(`display: block` or `flow-root`; every Platypus element is `flex` unless set) is placed beside the line
flow, floats of one side stack and wrap down, margins count, `clear` adds clearance, and a block
formatting context root (`flow-root`, a scroller, a flex item) contains its floats. Flex and grid items
ignore `float`. Text is the part Taffy cannot do, so `layout.rs` adds two things:

- A text root (a measured leaf that is a text block, `set_text_block`) lays out as a block item whatever
  its `display`, so it stays in the container's formatting context instead of being pushed beside the
  floats like a new one. `get_block_child_style` gives the container a copy of its style with `display: block`.
- When a measured leaf is laid out inside a formatting context that has floats, `float_bands` asks the
  context's `find_content_slot` for each float segment and turns them into `text::Band`s (`y`, `x`,
  `width` in the leaf's content box). The measure callback gets them as `Around { bands, layout_pass }`;
  the final layout pass records them in `UiTree::text_bands` so `text::finish` (drawing) breaks the
  same way.

`text::break_in_bands` drives Parley's `BreakLines` line by line: `room(bands, y, height)` is the left
edge and width every band the line's height touches leaves in common (a float anywhere in the line
pushes the whole line, as in CSS), found by breaking the line once and again when its height reaches a
narrower band; a line whose word is wider than the room moves to the next band
(`set_line_y`). Alignment is then per line within its own room (Parley's `inline_min_coord` /
`inline_max_coord`), and hit testing and selection follow without changes. `text_height` takes the bottom
of the last line as the block's height, since Parley sums line heights and a pushed-down line leaves a gap.

Taffy's per-node cache is keyed on sizes only, not on the floats placed before a block, so while any
node sets `float` or `clear` (`TaffyLayout::floats`) blocks inside a formatting context skip the cache.

An image (or any box) floated in a block that also holds bare text is the "image in a paragraph" case;
the text is an anonymous text block beside it. A float between runs of one `text` element is not
supported (text children are flattened to runs; it would need in-flow inline boxes).

### Tables

There are no table elements. A table is a grid whose rows are `display: contents` boxes
(`examples/table`, tests `M3-TBL-*`): `grid-template-columns: auto auto 1fr auto` sizes the columns from
the content of every row (one grid, the rows' cells are its items), the `1fr` column takes the rest and
wraps its text, `grid-column: span n` / `grid-row: span n` on a cell are colspan and rowspan, cells take
borders, backgrounds and `text-align`, and row styling goes through descendant selectors
(`.row:nth-child(even) .cell`, `.row:hover .cell`) since a `contents` row paints nothing and has no rect
(clicks still bubble through it, so `onClick` on the row works). Cells must be `display: block`, or their
text blocks shrink-wrap and `text-align` does nothing. Auto columns share spare width equally up to their
content size (grid), not in proportion to it (HTML tables); see `docs/differences.md`.

### calc ([calc.rs](../crates/platypus_ui/src/calc.rs))

Math that still has `%` after relative units are resolved is parsed into an `Expr` tree (`Px`,
`Percent`, `Number`, add/sub/mul/div, `min`, `max`, `clamp`) and type-checked: lengths and
percentages combine, numbers only scale (`calc(10% + 2)` or `calc(2 / 10%)` is rejected). Parsed
expressions are interned in a global table keyed by lowercased text, so equal text gives equal styles.
A style stores the table index as Taffy's opaque calc pointer: `handle = (index + 1) << 3` (Taffy
uses the 3 low bits as a tag). `calc::resolve(handle, basis)` evaluates it against the percentage
basis; unknown handles give 0, division by zero gives 0. `style.rs` calls `calc::handle` for
`width`-like (`Dimension`), margin-like (`LengthPercentageAuto`) and padding-like
(`LengthPercentage`) values.

`layout_if_needed()` (tree.rs) is the heart of the frame:

1. `restyle_dirty()`: recompute cascaded styles for dirty nodes (no-op without stylesheets/vars).
2. For each dirty text root, `build_text_layout` shapes a new Parley layout.
3. `layout.compute(ROOT, w, h, measure)`. Measured leaves (text blocks, images) call back into
   `text::measure` / `image_size`.
4. Walk the tree top-down turning Taffy's parent-relative `NodeLayout` into absolute `Rect`s in
   `self.rects`. Children of a scrolled element are shifted by its (clamped) scroll offset.
   `display: contents` nodes get a zero rect at the parent origin; `position: fixed` nodes are placed
   from (0,0), ignoring ancestor scroll. An absolute box whose containing block is not its parent
   (`LayoutEngine::containing_block`: the nearest positioned ancestor, or one made so by a transform, filter or `contain`, or the root) is placed from that
   box's rect minus its scroll, because Taffy gives out-of-flow positions relative to the containing
   block (`UiTree::child_origin` picks the origin for each child). Outside list markers are the
   exception: their `right: 100%; top: 0` only cancel the containing block's size, so they are
   placed from their item. The pass also records `escapes` (box -> its containing block, when that is
   further up than its parent) and `escape_paths` for painting, hit testing and scrolling (see
   [Clipping](#backgrounds-borders-and-blending)). For
   text blocks, `text::finish` re-breaks lines at the final content width and applies `text-align`;
   `finished` remembers that width, and the pass skips `finish` when it is unchanged (a scroll only
   moves things). Measuring or re-shaping a block forgets it. `text_measure_count()` counts measures.
5. `compute_ink()` fills `ink`: each node's rect (joined with its text layout's extent) unioned bottom-up
   with its children's. Nodes that can paint outside their box get an unbounded rect, and so do their
   ancestors: transform, filter blur, drop shadows, backdrop blur, box shadows, outline, border-image,
   transitions/animations, SVG, `:outro` leavers, and text blocks inheriting a text shadow.
   `display: contents` nodes have no own ink; inline content and fixed nodes aren't walked.

`NodeLayout` also carries resolved `border`, `padding` and `scroll_max` (Taffy's scrollable
overflow), used for clipping, scrollbars and event data.

## Text

[text.rs](../crates/platypus_ui/src/text.rs) wraps Parley (`TextEngine` = `FontContext` +
`LayoutContext<Brush>`; `Brush` is a struct carrying the run's colour, decoration lines/style/colour/thickness/underline offset, stroke width/colour, `smooth`, `hidden` (`visibility: hidden`) and `node` (the source node of the run's characters, set by `build_with_text`, `None` from `Brush::of(&ComputedText)`); `node_at(layout, x, y)` maps a point to that node via `Cluster::from_point_exact`, for span hit testing). Fonts: bundled Inter (variable, upright and
italic) is always registered and put first for the generic sans-serif families; `with_system_fonts`
adds platform fonts for fallback (CJK, emoji). Tests use `UiTree::new` (no system fonts) to stay
deterministic. `register_font` first runs the data through `decode_web_font`, which decompresses WOFF and WOFF2
(recognised by their `wOFF`/`wOF2` signature, via the `wuff` crate) to plain TTF/OTF and passes
anything else through; bad web font data is an error. It accepts overrides for family/weight/style (like `@font-face`), then
replaces the `LayoutContext` because Parley caches family lookups, including misses.

`platypus_ui` enables Parley's `complex-scripts` feature, so word and line segmentation use ICU4X's
dictionary segmenters. Japanese/Chinese word moves (Ctrl+Arrow) and double-click selection follow
dictionary word boundaries, and Thai, Lao, Khmer and Burmese text without spaces wraps between
words (M3-TXT-08). It adds about 3.8 MB to the release binary.

`TextEngine::build` sets Parley's base direction from the block's `direction` (never Auto, as in
CSS), so rtl blocks start at the right and mixed Latin/Hebrew/Arabic runs order correctly. The
shared helpers `alignment()` and `base_direction()` are used by text blocks and fields;
`text-align: start/end` resolve by direction through Parley (M3-TXT-09). Not done: `dir="auto"`,
flex/grid row order in rtl (Taffy has no direction). Logical properties are mapped left-to-right only
(see "Layout shorthands").

How inline content works:

- A **text root** is a `text` element, or a `span`/`#text` that is *not* inside a text container
  (`is_text_root`; it becomes an anonymous text block).
- Children of `text`/`span` are **not** sent to Taffy as layout children (`is_inline_container`);
  they are flattened by `build_text_layout` into `(string, ComputedText)` runs, each carrying its
  node's inherited style, and handed to `TextEngine::build`. The root is a Taffy leaf marked
  `set_measured` so Taffy asks for its size.
- `text::measure` breaks lines for the available width (`MinContent` uses width 0, `MaxContent`
  none) and returns a ceil'd size. `text::finish` does the final break + alignment.
- `text::draw(layout, origin)` produces a `TextCmd`: `GlyphRun`s (font data, size, variation
  coordinates, synthetic bold/italic, colour, `stroke: Option<(width, colour)>`, `smooth`, positioned
  glyphs with their `advance`) plus `decorations: Vec<TextDecoration { rect, color, style }>`.
- `text::draw_ellipsized(layout, origin, Ellipsis { layout, width, rtl })` is `draw` for blocks with
  `text-overflow: ellipsis`: on lines wider than the content box it drops whole glyphs that would
  overlap the `…`, stops decorations where the kept glyphs end, and appends the `…` run on that
  line's baseline (before the glyphs for rtl). `UiTree` keeps one `…` layout per such block in
  `ellipses` and only uses it when `overflow-x` clips.

### Static text selection and `user-select` (M3d step 5h)

- **`userSelect`** (`auto`, `none`, `text`, `all`) is an inherited text property; `auto` keeps the
  parent's value.
- **State in the tree:** `UiTree::select_text(target, x, y, PointerSelect)` starts (Shift extends) a
  selection in the text block holding `target`: one press places a caret, two select a word, three
  a line, and `user-select: all` takes the whole block. It returns whether one started; otherwise
  any selection is cleared. `extend_text_selection(x, y)` follows a drag, `text_selection()` returns
  the block and the selected string, `clear_text_selection()` drops it. Each text block keeps a
  Parley `Selection` against the text it laid out (`layout_texts`, white space already collapsed).
  A selection survives restyles but not a change of that text.
- **`UiTree::to_local(id, x, y)`** maps a window point into the plane `id` is laid out in, undoing
  the transforms of it and its ancestors (fixed elements ignore ancestor transforms). Static
  selection, text fields and backdrop clicks use it, so presses work on rotated or scaled text.
  **`UiTree::to_window(id, x, y)`** is its inverse: a point in `id`'s plane to the window. The bench's
  `clickText` / `moveToText` use it to find text under transforms.
- **Drawing (`selection_cmds`):** highlight quads behind the text (default blue) and, when
  `::selection` sets a colour, the selected text drawn again in that colour, clipped to the highlight.
- **`::selection` / `::placeholder`:** `update_pseudo_style` stores what the rules set as a
  `PseudoStyle` (text props, background, opacity) in `pseudo_styles`, by element. `selection_colors`
  takes the colour and the background each from the nearest element (itself or an ancestor) with a
  rule that sets it. A field's `::placeholder` style goes to `Editor::set_placeholder_style`, which
  rebuilds the placeholder layout (still dimmed by default; `opacity` scales that).
- The I-beam cursor shows only over selectable text (see
  [input-and-events.md](input-and-events.md#cursor)).

### Text properties (M3d group 4)

Parsed in [text_style.rs](../crates/platypus_ui/src/text_style.rs) into `TextProps`; all inherited except
`textOverflow` (`ComputedText::child` resets it). Tests: [text_props.rs](../crates/platypus_ui/tests/text_props.rs),
unit tests in `text_style.rs`, goldens `M3-TXT-10..15`.

- **`textTransform`** (`TextTransform::apply`): applied to each run's text in `build_text_layout`;
  `capitalize` tracks word starts across inline runs.
- **`whiteSpace`** also has `pre-line` and `break-spaces`; **`wordBreak`** (`normal`, `break-all`,
  `keep-all`, `break-word`) and **`overflowWrap`** (alias `wordWrap`; `anywhere`, `break-word`) map to
  Parley's `WordBreak` / `OverflowWrap`.
- **`fontWeight`** is a `FontWeightSpec`: number 1-1000, `normal`/`bold`, `bolder`/`lighter` (relative
  to the parent's weight), or names (`thin` ... `black`). `fontSize` accepts keywords (`medium` 16px,
  `smaller`/`larger` x1.2 of the parent). The **`font`** shorthand (`parse_font_shorthand`) sets
  style, weight, size, line height and family; omitted parts go back to initial.
- **Font features:** `fontVariantNumeric` and `fontFeatureSettings` are merged by
  `ComputedText::font_features()` into a `font-feature-settings` string, passed to Parley as
  `FontFeatures::Source`.
- **Decorations:** `textDecoration` shorthand plus `textDecorationLine` (now with `overline`),
  `...Style` (`DecorationStyle`: solid, double, dotted, dashed, wavy), `...Color`, `...Thickness`,
  `textUnderlineOffset`; `textDecorationSkipInk` is parsed only. Colour/thickness default to the text
  colour and the font's metrics. Parley's own underline is still used for the editor's IME pre-edit.
- **Text stroke:** `WebkitTextStroke[Width|Color]` / `textStroke[...]` fill `GlyphRun.stroke`
  (colour defaults to the text colour); the renderer draws the outline over the fill.
- **`fontSmooth` / `WebkitFontSmoothing`:** `never`/`none` sets `GlyphRun.smooth = false` (crisp glyphs).
- **`caretColor`:** used for the text field caret quad in `tree.rs` (`auto` is the text colour).
- `dirty_text` is the set of roots to reshape; `text_build_count()` lets tests assert how often
  shaping ran (used in [text.rs tests](../crates/platypus_ui/tests/text.rs) to check that unrelated
  changes do not reshape text).

## Lists (M3d step 5g)

`list` is an element type whose element children are list items (so is any element with
`display: list-item`; see `is_list_item`). Its user-agent style (`base_style`) is a column with a
24px start padding that leaves room for outside markers. `update_list_indent` drops that indent
when the list has no markers (`list-style: none`), unless a rule or inline style set the start
padding (`is_start_padding`).

- `listStyleType` (counter styles, strings, `none`), `listStylePosition`, `listStyleImage` and the
  `listStyle` shorthand are inherited.
- **Markers are generated children.** `refresh_markers` visits the queued nodes (items added,
  removed or moved, and inherited style changes) and `update_marker` adds, updates or removes the
  item's first child. The marker is a `span` for text markers and an `image`
  (`insert_marker_image`) for `list-style-image`; the kind must match, so a change recreates it.
  The number comes from the item's position among its siblings, so removals renumber.
  `::marker` rules style it, and it comes before `::before`.
- **Outside** markers are absolutely placed to the left of the item; **inside** ones are inline
  first children (a no-break space keeps the gap). A `text` item cannot hold a box marker, so there
  an outside marker reads as inside.

## Dialogs and the top layer (M3d step 5j)

`dialog` takes `open`, `modal` and `closedBy` (`any`, `closerequest` (default), `none`)
attributes. `update_dialog` runs after restyle:

- A dialog without `open` is `display: none`; the authored `display` is kept in
  `Node.dialog_display` and restored when it opens.
- UA styles (`base_style` / `base_visual`): centred in its containing block (`translate: -50% -50%`).
- An open **modal** dialog is `position: fixed` (centred in the window) and joins `top_layer`, in
  opening order. `fixed_paint_order` paints the top layer after all other fixed nodes, each dialog
  over its `::backdrop`, whatever the `zIndex`. `top_modal()` is the topmost one,
  `top_modal_contains(dialog)` checks that a dialog is still an open modal, and
  `is_inclusive_ancestor(ancestor, id)` tests whether `id` is inside it.
- **`::backdrop`** is a generated fixed box covering the window under the dialog (`update_backdrop`,
  `backdrop_of`): browsers' default tint, then `::backdrop` rules. It always covers the window. A hit
  on it reports the dialog.
- Events and focus: [input-and-events.md](input-and-events.md#dialogs).

## Draw list

[draw.rs](../crates/platypus_ui/src/draw.rs): `Rect` (logical px, with `intersect/union/expand`,
rounded hit testing), `Radii { x, y }` (per-corner horizontal and vertical radii, used by every
shape), `ClipShape` (rect with elliptical corners, or an `Arc<Polygon>`; no longer `Copy`), and
`enum DrawCmd`:

| Command | Meaning |
|---|---|
| `BackdropBlur{rect,radii,blur,matrix}` | Blur and/or recolour (`backdrop-filter`) what is already drawn behind the box |
| `BoxShadow(ShadowShape)` | Outer shadow, before its box |
| `Quad(Quad)` | Background colour, `images: Vec<ImageFill>` layers (bottom first), uniform border, radii |
| `Border{rect,radii,widths,colors,styles}` | Border with per-side colours or styles; also the `outline` |
| `BorderImage{area,border,image,pixelated,tint}` | A `border-image` over the border box grown by the outset |
| `InsetShadow{shadow,border}` | Inside the padding box, over the background |
| `PushLayer{bounds,blur,opacity,blend,matrix,shadows}` / `PopLayer` | Offscreen group (filter blur and colour matrix, `LayerShadow` drop shadows, group opacity, blend mode) |
| `BeginMask(MaskMode)` | Inside a layer, after its content: the commands up to `PopLayer` draw the layer's mask |
| `PushClip(ClipShape)` / `PopClip` | Clip following commands |
| `PushTransform(Homography)` / `PopTransform` | What follows is drawn in a transformed plane; the homography maps that plane to the window and replaces (does not compose with) any outer one |
| `Text(TextCmd)` | Glyph runs and decorations |

`UiTree::draw_list()` runs `layout_if_needed` then `collect(ROOT)` recursively. Per node, in
CSS painting order: `PushTransform` if the element has a transform (see [Transforms](#transforms)), `PushClip` for the element's own `clip-path`/`clip: rect()`, `BackdropBlur`,
`PushLayer` if needed, outer shadows (last listed first, so the first shadow ends on top), the
`Quad` (or `Border`/`BorderImage` when the border needs them), inset shadows, text shadows then text (a text field draws its [editor content](#text-fields) here), then the children, scrollbars and `PopLayer`/`PopClip`/`PopTransform`.
`collect` skips `position: fixed` children and paints only the children of a `display: contents`
node. `draw_list` paints the fixed nodes after the root (see below).

**Culling.** `collect` and `collect_children` take `cull: Option<Rect>`, the area the clips around the
node leave visible (layout space). `collect` returns early when the node's `ink` doesn't come within
`CULL_MARGIN` (64 px, for glyph overhangs) of it, so a long scroller only emits what shows. Children get
`cull` intersected with the node's overflow clip rect, or `None` below a transformed node (another
space). `draw_list` (root and fixed nodes) and the drag image pass `None` (`M2-DRW-09`).

### Backgrounds, borders and blending

- **Layers.** `Visual` keeps `background_images`, `background_sizes/positions/repeats/clips/blends`
  as lists; a shorter list cycles (`i % len`). `background_layers` in tree.rs builds one
  `ImageFill` per layer, each with its `area`/`radii` from `clip_area` (`border-box`, `padding-box`,
  `content-box`). The colour is clipped like the bottom layer (an extra colour quad if needed).
  `BackgroundImage` is `Linear`, `Radial` or `Conic { from, at, stops, repeating }`; stops may have
  two positions. `BackgroundRepeat { x, y }` uses `RepeatAxis` (`Repeat`, `NoRepeat`, `Round`,
  `Space`).
- **`background-clip: text`.** Gradients go into `TextCmd.fill` (image plus the element rect);
  the glyphs sample it.
- **Borders.** `BorderStyle` is none, hidden, solid, dashed, dotted, double, groove, ridge, inset or
  outset; Platypus default is solid (not CSS none). Colours, styles and `currentColor` flags are per
  side (`border_colors`, `border_styles`, `border_currents`). `border_side` / `four_sides` /
  `set_border_style` handle the longhands; none and hidden zero the width. A uniform border stays
  in the `Quad`; anything else becomes `DrawCmd::Border`.
- **Radii.** `parse_radii` accepts `a b c d / e f g h`; `resolve_radii` resolves percentages per
  axis (width for x, height for y).
- **Outline.** `outline*` live in `Visual.outline_*`; `outline()` emits a `Border` after the
  element children.
- **Image tint** (s&box `background-image-tint` / `border-image-tint`, M3d step 9a). `Visual`
  holds `background_image_tint` and `border_image_tint` (`none` or a colour). The tint is copied
  into `ImageFill.tint` for background layers and `image` elements (not masks or gradients) and
  into `BorderImage.tint`; the renderer multiplies it into the sampled pixels
  ([rendering.md](rendering.md#image-tint)). Golden M3-GAME-01.
- **`border-image`.** `BorderImage` holds source, slice (with `fill`), width, outset and repeat
  (`BorderImageLength`, `BorderImageRepeat`). `pieces()` yields the `BorderImagePiece`s and
  `area()` the outset box.
- **Blend modes.** `BlendMode` has the 16 W3C modes (`index()` is the shader id). `mix-blend-mode`
  sets `PushLayer.blend`. `blended_background` wraps blended background layers in their own
  `PushLayer`s inside an isolated group.
- **Filters** (`Filter::parse`). `filter`/`backdrop-filter` take `blur`, `brightness`, `contrast`,
  `saturate`, `grayscale`, `sepia`, `hue-rotate`, `invert`, `opacity` and (`filter` only)
  `drop-shadow`. Blurs add in quadrature; colour functions multiply, in order, into one `ColorMatrix`
  (4x5, row-major, straight-alpha sRGB). Drop shadows (no inset or spread, `currentColor` default)
  become `LayerShadow`s with sigma = blur / 2. Platypus applies blur, then colours, then shadows. A
  layer is pushed for blur, a matrix, shadows, a mask, group opacity or a blend mode, and its
  `bounds` grow by `3 * blur` and by each shadow's offset and `3 * sigma`. `backdrop-filter` with
  `drop-shadow` is invalid.
- **Masks.** `mask-image` (and `-webkit-`; the `mask` shorthand takes only images), `mask-size`,
  `-position`, `-repeat` and `-mode` are lists like the background ones. `mask_layers` builds the
  `ImageFill`s (bottom first) over the border box. After the layer's content `collect` emits
  `BeginMask(mask_modes[0])` and one `Quad` of those fills; the top layer's mode applies to all.
  `match-source` is alpha.
- **`image-rendering`.** Parsed with the text props (`TextProps.pixelated`, `ComputedText.pixelated`;
  inherited, `pixelated` and `crisp-edges` set it) and copied into `ImageFill.pixelated` (backgrounds,
  masks, `image` elements) and `BorderImage` commands.
- **`visibility`.** `visible | hidden | collapse` (`collapse` = `hidden`, there are no tables), parsed
  with the text props (`TextProps.visible`, `ComputedText.visible`; inherited, children can set
  `visible` again). A hidden element keeps its layout space. In `collect`, `own` (the node's own
  commands: shadows, `Quad`, images, text-field content) is cleared when `!text.visible`, and its
  `BackdropBlur` and outline are skipped; children still draw. Text runs carry the flag in
  `Brush.hidden`, so `draw_lines` drops the glyph runs and gives decorations a transparent colour,
  while a visible `span` inside hidden text still draws. `hit` takes a `visible` flag (the
  inherited value, overridden by the node's own) and a hidden node returns nothing, though its
  visible children can hit; `focusable` also checks `computed_text(id).visible`. Test: M3-VIS-01.
- **`opacity`** takes a number or a percentage (`"40%"`, `number_or_percent`), clamped to 0-1.

Decisions worth knowing:

- `display: none` and `opacity <= 0` subtrees are skipped.
- **Group opacity**: a layer is only created if the node has children or more than one own command;
  otherwise the alpha is multiplied into the single command (`scale_alpha`). The layer's `bounds`
  are patched afterwards to the union of what was actually drawn, expanded by `3 * blur`.
- **Clipping** (committed): `clip-path` and legacy `clip` clip the element *and* its children;
  `overflow` clips only the children, to the padding box with inner corner radii (`overflow_clip`,
  `inner_corners`). `clip-path: polygon()` and `path()` (fill rule `nonzero` or `evenodd`) become
  `ClipShapeSpec::Polygon`/`Path`, flattened to a `Polygon` (`Polygon::contains` for hit tests;
  curves and arcs are split into segments); `url()` is unsupported. An axis that does not clip gets an effectively unbounded clip edge
  (`UNCLIPPED`). Hit testing uses the same clip functions, so clipped-away parts cannot be hit.
  `contain: paint` (also `strict`, `content`) clips like `overflow: clip`.
- **Absolute boxes and clips** ([escape.rs](../crates/platypus_ui/src/tree/escape.rs)). As in CSS, an
  `overflow` clip (and scrolling) reaches an absolute box only when the clipper is its containing block or
  lies between them; a dropdown in a static `overflow: hidden` bar shows in full. `collect` is wrapped to
  draw into a `DrawOut` (the draw list plus a trail of open clips, transforms and layers). For a box in
  `escapes` it pops the escaped overflow clips, draws, and reopens the rest (clip-paths, transforms)
  around it. Hit testing passes `clipped` down, so escaping boxes are still hit where the clip hides the
  point. Boxes without escapes take the plain path. Limit: a clip outside an opacity, blend or mask group
  that holds the box still clips it (`differences.md`).
- `currentColor` is resolved here (`current()`), using the node's computed text colour.

## Transforms

[transform.rs](../crates/platypus_ui/src/transform.rs) parses `transform` (every 2D and 3D function;
`parse_transform` gives `Vec<TransformFn>`, where `Fixed(Mat4)` is size-independent and
`Translate(Len, Len, f32)` may hold percentages), `translate`, `rotate`, `scale`, `transform-origin`,
`perspective`, `perspective-origin`, `transform-style` and `backface-visibility`. `apply_style` stores
them in `Visual.transform: Option<Arc<Transforms>>` (`TransformSpec` plus origin, perspective,
`preserve_3d`, `backface_hidden`); `copy_prop` handles the CSS-wide keywords.

- **Model.** Every element is a flat plane. `TransformSpec::matrix(rect, origin)` builds its `Mat4`
  (`m[row][col]`, column vectors, y down): translate to the origin, then `translate`, `rotate`,
  `scale`, then the `transform` functions, then translate back. The parent's `perspective` (about
  its `perspective-origin`) is multiplied in front by `UiTree::node_matrix`. `Mat4::flatten` drops z
  to a 3x3 `Homography` (rows and columns x, y, w), which maps lines to lines and inverts exactly,
  so drawing and hit testing only need homographies. `Homography::apply` returns `None` when w <= 0
  (behind the viewer); `map_rect` gives the bounds of a mapped rect (unlimited if a corner is behind
  the viewer).
- **`TransformContext { world, pending }`** is passed down `collect` and `hit`. `world` maps the
  current plane to the window; `pending` is 3D transform not yet flattened. `transform_step` combines
  `pending` with the node's matrix, returns `None` for a `backface-visibility: hidden` node facing
  away (not drawn or hit), and yields the node's plane (`world * flatten(acc)`) plus the children's
  context: normally `world = plane, pending = identity`; under `transform-style: preserve-3d`
  `world` stays and `pending = acc`, so the children's turns multiply in before flattening.
  Planes are still painted in tree order (no depth sorting; see [differences.md](differences.md)).
- **Draw list.** A node with its own transform is bracketed by `PushTransform(plane)` /
  `PopTransform`, outside its self clips. Everything inside is in the node's own coordinates; the
  homography carries the absolute plane-to-window map. Culling and layer bounds in the renderer map
  through it (see [rendering.md](rendering.md)).
- **Hit testing.** `hit` keeps the window point (`wx`, `wy`), asks `transform_step` for the node's
  plane and maps the point into it with `plane.inverse().apply(..)` (a failed inverse or a point
  behind the viewer is a miss). Its own box test and its `overflow` clip then run in that plane;
  children get the window point and the child context. `position: fixed` nodes start from
  `TransformContext::ROOT`.
- Tests: unit tests in `transform.rs` (M3-XF-01), `layout_props.rs` (hit testing, property forms,
  `preserve-3d`: M3-XF-02) and the `M3-XF-02` golden.

## Scrolling

- **Extent**: `NodeLayout.scroll_max` comes from Taffy's scrollable overflow (`scroll_width()` /
  `scroll_height()`): how far content reaches beyond the padding box. `ScrollInfo::of` derives
  `scrollWidth = clientWidth + max`.
- **Offsets**: `UiTree.scroll: HashMap<NodeId, (f32, f32)>`. `scroll_offset`, `scroll_max`,
  `scroll_axes(id, by_user)` (user: `scroll`/`auto`; scripts also `hidden`) and
  `scroll_to(id, x, y, by_user)`, which clamps to `0..=scroll_max` per scrollable axis, sets
  `needs_layout` and returns whether the offset changed. Taffy's layout is cached; only the
  absolute rects are recomputed. It cancels a running smooth scroll on that node; the old body is
  private `set_scroll`.
- **Smooth scrolling** (`M3-EVT-09`): `smooth_scroll_to(id, x, y, by_user)` starts an eased scroll
  (cubic-bezier(.42,0,.58,1); retargeting mid-flight eases out from the current position; duration
  `200 + distance * 0.25` ms, clamped to 250..500) and returns the clamped goal. `set_time` advances
  them (`step_smooth_scrolls`), `is_animating` includes them, `take_smooth_scrolled` lists containers
  moved, `scroll_goal` is where one is heading. `scrollBehavior: smooth` (`Visual.scroll_smooth`;
  `smooth_by_default`) makes script scrolls with `Auto` behavior smooth.
- **Scroll padding** (`M3-EVT-10`): `Visual.scroll_padding: [f32; 4]` (top, right, bottom, left, px) from
  `scrollPadding` (1 to 4 lengths or `auto` = 0) and the `Top/Right/Bottom/Left/Block/Inline`
  longhands; `UiTree::scroll_padding(id)` reads it. `Interaction::scroll_into_view` insets the
  container's view by it. Not inherited; px only.
- **Applied in `layout_if_needed`**: while building `rects`, an element's offset is first clamped
  to the current `scroll_max` (content may have shrunk), then subtracted from the origin passed to
  its children. Hit testing and drawing both read these shifted rects, and the `overflow` clip is
  on the padding box (unshifted), so scrolled content stays clipped and hittable consistently.
  `destroy_subtree` drops offsets of removed nodes.
- **`scroll_path(id)`**: the scroll ancestors of a node, innermost first, skipping those an absolute box
  escapes (see Clipping). Wheel, key, scrollbar and scroll-into-view scrolling use it.
- **Scrollbars**: `scrollbars(id)` returns up to two `Scrollbar { node, vertical, track, thumb,
  thumb_color, track_color }` in absolute px. Shown for `scroll`, or `auto` with content to scroll,
  unless `scrollbarWidth: none` (thickness: `auto` 8 px, `thin` 5 px). They overlay the content
  (no layout space, unless `scrollbarGutter: stable`, below), inset by `SCROLLBAR_MARGIN` 2 px
  (defined in style.rs); when both show, the corner is left free. Thumb
  length is proportional to `client / (client + max)` with a 24 px minimum. `scrollbarColor`
  (thumb, track) is inherited, default a translucent grey thumb and transparent track.
  `collect_scrollbars` draws them as rounded `Quad`s after the children, inside the element's layer
  so they are subject to its opacity but not its overflow clip. `scrollbar_at(path, x, y)` hit-tests
  the track expanded by the margin so thin bars are easy to grab.
- **`scrollbarGutter: stable`** (`Visual.scrollbar_gutter`; `auto` turns it off, `both-edges` is
  accepted and treated as `stable`). `set_overflow` in style.rs reserves the room in layout: for a
  box that clips or scrolls vertically it sets Taffy's `scrollbar_width` to the thickness plus both
  margins and switches Taffy's y overflow to `Scroll` (the only mode where Taffy reserves it).
  Changing `scrollbarWidth` re-runs `set_overflow` so the reserved size follows. The bar is still
  drawn as an overlay, now over the reserved strip, so content width does not jump when it appears.
- **`overscrollBehavior`** (and `-x/-y/-inline/-block`): `Visual.overscroll_contain: [bool; 2]`,
  true for `contain` and `none` (Platypus has no overscroll glow or history gesture, so they are the
  same). `UiTree::contains_overscroll(id, axis)` is true only when the node is user-scrollable on
  that axis; `Interaction` uses it to stop chaining. Test: M3-SCR-01 (`tests/scroll.rs`).
- Input handling, wheel chaining and the `scroll` event are in
  [input-and-events.md](input-and-events.md#scrolling-m3c-step-4b).

## Text fields

`<input>` and `<textarea>` are `box`-like elements (styled, bordered, clipped by CSS like any
other) whose text is owned by an `Editor` in [editor.rs](../crates/platypus_ui/src/editor.rs):
`UiTree.editors: HashMap<NodeId, Editor>`, created in `CREATE` for `is_editor_kind` and removed with
the node. Each `Editor` wraps a Parley `PlainEditor<Brush>` (text, selection, IME composition; DESIGN
D7) plus `multiline`, `rows`, an optional placeholder layout, the field's own `scroll` and the last
content-box `size`.

- **Sizing**: a field is a *measured leaf* like a text block (`set_measured`); the measure callback
  falls back to `Editor::intrinsic_size`: `known_width` or a default (`INPUT_WIDTH` 160 px,
  `TEXTAREA_WIDTH` 240 px) by one line (textarea: `rows`, default `TEXTAREA_ROWS` 2) of the editor's
  line height. Explicit `width`/`height` win. Attribute changes mark the node dirty.
- **Attributes** (`SET_ATTRS` in `apply_commit`): a `value` attribute (controlled) replaces the text
  if it differs (`Editor::set_value`; skipped while composing; the caret goes to the end at the next
  layout, like browsers). `defaultValue` only fills a field that never had a value or default.
  `placeholder` and `rows` are passed on. `readOnly` and `disabled` are read from `Attrs` when keys
  arrive.
- **Password** (`type="password"` on an `<input>`, `Editor::set_password`, `M3-INP-03`): the Parley
  editor holds one `•` (U+2022) per character and the real text lives in `Editor::secret`, which is what
  `editor_value` and `input` events report and `value` sets. After each edit (`key`, `insert`)
  `absorb_edit` reads what changed from the selection before it and the caret after it, updates
  `secret` and masks the editor again. As in browsers there is no copy or cut (no clipboard request,
  `editor_selection` is `None`), Ctrl moves and deletes to the ends, and a double click selects all.
  An IME pre-edit shows as typed until it is committed. Changing `type` re-shows the text masked
  or plain and keeps the value.
- **Other input types** (`Editor::set_type`, `M3-INP-04`): `number` keeps only `0-9 + - . e E` from
  typed and pasted text (`accept`), and ArrowUp/ArrowDown set the value one `step` along (`stepped`:
  1 by default, on the grid that starts at `min` or 0, within `min`..`max`, an empty field counts as
  0). `search` clears on Escape. `maxLength` (any field) cuts typed and pasted text to fit, counting
  characters (not masks) and giving room for a replaced selection. `email`, `tel`, `url` and the
  rest are plain one-line fields. Not done: spinner buttons, validation (`required`, `pattern`,
  `:invalid`), `inputMode`, and the non-text types (`checkbox`, `radio`, `range`, `color`, dates).
- **Style sync**: `refresh_editors` runs at the end of `layout_if_needed`. For every field it takes
  `computed_text(id)` (so `color`, `font*`, `line-height`, spacing come from the cascade), calls
  `Editor::set_style` (restyles only if the `ComputedText` changed, which also drops the placeholder
  layout), computes `content_box(id)` (rect minus border and padding) and calls `Editor::refresh`:
  sets the wrap width (textarea only; an input never wraps), moves the caret to the end if a script
  just set the text, refreshes the Parley layout, builds the placeholder layout in the text colour at
  half alpha, and runs `keep_caret_visible`.
- **Direction**: the editor's base direction and alignment come from the field's `direction` and
  `text-align`. A single-line input aligns short text with a negative horizontal `scroll`, so
  drawing, hit tests and the IME area follow. The placeholder is aligned the same way
  (`placeholder_x`) and re-laid out when the field's size changes. Arrow keys move visually
  (Parley behaviour).
- **Caret in view**: `Editor::scroll` is the field's own offset, one axis only (x for input, y for
  textarea), adjusted to show the caret rectangle and clamped to `content + caret width - view`.
  It is not a CSS scroll offset and creates no scrollbars.
- **Blink**: `UiTree::set_time(now_ms)` (the app calls it every frame) feeds `caret_on`: on for the
  first `CARET_BLINK_MS` (530 ms) after the last `touch` (any key, press, paste, compose), then
  alternating. Editing calls set `needs_layout`.
- **Drawing** (`editor_cmds`, after the field's own quad and shadows, before children): `PushClip`
  to the padding box with inner corner radii; if focused and not disabled, selection quads
  (`SELECTION`, blue at 0.4 alpha); the text (or the placeholder while empty) as one `Text` command
  at the content origin minus `scroll`; the caret quad (1.5 px wide, text colour) when in the "on"
  half of the blink; `PopClip`. Unfocused fields show no selection and no caret.
- **Cursor**: `auto` is `text` over fields too.
- **API used by [Interaction](input-and-events.md#text-fields-m3c-step-5)**: `is_editor`,
  `editor_value`, `editor_selection`, `editor_key` (refuses disabled), `editor_insert` (refuses
  disabled and read-only), `editor_pointer` (absolute px to content-box px), `editor_compose`,
  `editor_end_compose`, `editor_ime_area` (caret or pre-edit box in absolute px, shifted by the
  field scroll).

## Gotchas

- `needs_layout` is a coarse flag: any commit, state change or scroll sets it, and Taffy's own
  cache keeps the recompute cheap. Hover changes call `set_state`, which records a `Key` change so
  the restyle only touches nodes the stylesheets depend on.
- `hit_test` also calls `layout_if_needed`, so it is safe right after a commit.
- The `uses_vars` flag is set once an inline style uses variables and never cleared.

## Images

`image` is a measured leaf (kept measured in the touched-node pass too). Its `src` attribute is a
path inside the game folder or a `data:` URL. The tree does not load pixels. `UiTree` keeps
`image_sizes: HashMap<String, Option<(f32, f32)>>` (natural sizes; `None` = failed) and
`image_requests`. `request_image` queues a `src` once when an image's `src` changes. The app calls
`take_image_requests()`, loads each through the renderer, and reports back with
`set_image_size(src, Option<(u32, u32)>)`, which marks every image using that `src` dirty.

`Measure` now carries the known `(width, height)`. `image_size(natural, known)` keeps the aspect
ratio when only one axis is known. `draw_image` emits `PushClip(inner_corners(rect, radii,
border + padding))`, a transparent `Quad` over the content box with an `ImageFill` (`Url`,
size from `ObjectFit::background_size`, position from `objectPosition`, no repeat), then `PopClip`.
Until the size is known it fits as if the natural size matched the box. `paint::ObjectFit` is
`fill`, `contain`, `cover`, `none` or `scale-down`; `Visual.object_fit` / `object_position`
(default `50% 50%`) come from the `objectFit` / `objectPosition` style props.

## Typed ops

`platypus_ui::opcodes::Op` is the decoded form of a commit op (`Create`, `SetStyle`, `AppendChild`, ...). `UiTree::apply_commit` decodes each `[opcode, ...args]` array with `Op::decode` (arity and argument types checked there, errors in op order) and applies it; `UiTree::apply_ops(&[Op])` applies already-typed ops as one batch. There is one applier (`apply_op`), used by the native reconciler (`platypus_ui_native`, [ui-native.md](ui-native.md)); the QuickJS React `__platypus_commit` path is gone. A non-string style or attribute name now rejects the whole op instead of skipping the one pair. Test `M2-UI-OP-01`.
