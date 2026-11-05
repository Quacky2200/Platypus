# SVG (M3d step 5.1)

Platypus draws SVG two ways. Both end in the same place: resvg rasterizes the SVG on the CPU at the
size it is drawn, and the result is a texture drawn like any image. Known gaps against browsers are
in [differences.md](differences.md).

| Path | You write | Source of the SVG text |
|---|---|---|
| SVG file as an image | `background-image: url(icon.svg)`, `border-image`, `<image src="icon.svg">` | The file in the game folder (or a `data:image/svg+xml` URL) |
| Inline SVG | `<svg>` with `<path>`, `<g>`, ... as React children | Serialized by `UiTree` from the live tree |

## SVG files as images (step 5.1a)

Code: [images.rs](../crates/web_ui_render/src/ui/images.rs), call site in
[ui/mod.rs](../crates/web_ui_render/src/ui/mod.rs). Uses resvg 0.48 (default features off, only
`text` on, so no system fonts).

- `is_svg(url)`: a `data:image/svg+xml` URL, or a path ending in `.svg` (query and fragment ignored).
- `ImageCache::load` parses an SVG once (`svg_tree`, a `usvg::Tree`) and rasterizes it at its natural
  size (`svg_natural`). That is what `UiRenderer::image_size` reports, so `image` elements measure
  correctly.
- When the planner draws an image fill whose URL is an SVG, it calls `ImageCache::svg_at` with the
  tile's physical size (tile size in logical px times the scale factor). `svg_at` rasterizes again at
  that size, so the image stays sharp when scaled or on a HiDPI display.
- Raster sizes go through `bucket`: rounded up to a multiple of 8 px, clamped to 8..=4096. Small size
  changes (a transition, a resize drag) reuse the same raster instead of making one per frame.
- Cache keys: the parsed tree is under the URL, each raster under `<id>#WxH`.
- `svg_tree` builds `usvg::Options` once with the bundled Inter (`platypus_ui::text::INTER` and
  `INTER_ITALIC`, now public) as the only font, so `<text>` renders without system fonts.
- `rasterize` renders with tiny-skia and converts to straight alpha; `upload` makes the texture.
- `ImageCache::set_root` clears the parsed trees along with the entries.

Tint (`background-image-tint`) and `image-rendering` apply to the raster like any image.

## Inline `<svg>` (step 5.1b)

Code: [svg.rs](../crates/platypus_ui/src/svg.rs) (helpers), [tree.rs](../crates/platypus_ui/src/tree.rs)
(`svg_markup`, measuring, drawing), [attrs.rs](../crates/platypus_ui/src/attrs.rs),
[style.rs](../crates/platypus_ui/src/style.rs), and for the script side the native reconciler
([ui-native.md](ui-native.md); the JS `attrs.ts` / `host.ts` / `style.ts` are deleted).

**Why serialize.** The SVG elements are ordinary tree nodes, so selectors, `:hover`, inheritance,
custom properties, transitions and animations all work on them through the usual cascade. Platypus does
not lay them out; it turns the subtree back into markup when drawing.

### Element types and attributes

- `SVG_KINDS` lists the element types: `svg g rect circle ellipse line polyline polygon path tspan
  defs use symbol linearGradient radialGradient stop clipPath mask pattern title`. `text` is the
  existing text element; inside an `svg` it is SVG text.
- JS side: `SVG_TYPES` in `attrs.ts` mirrors that list. `isAttrOf(name, value, type)` makes every
  primitive prop (string, number, boolean, null) of an SVG element or `text` an attribute, except
  `children`, `style`, `key`, `ref` and `on*` handlers. `attrEntries` and `attrDiff` take the element
  type. The element types are registered in `host.ts`, and `style.ts` has the SVG CSS property types.
- Rust side: `Attrs.svg` holds the extra attributes as React named them (`viewBox`, `strokeWidth`),
  in the order set. `Attrs::set_svg` stores a value (null removes it; whole numbers print without a
  trailing `.0`). SVG elements and `text` accept any attribute name instead of reporting a problem.
  `className` and `id` keep their usual meaning, so selectors see them.
- `attr_name` converts a prop to an attribute name: kebab-case (`strokeWidth` to `stroke-width`),
  except the camelCase names SVG keeps (`viewBox`, `gradientUnits`, `stdDeviation`, ...);
  `className` becomes `class`, `xlinkHref` becomes `href`. `escape` escapes attribute and text values.

### CSS on SVG content

`apply_style` wraps `apply_property`. For a property that `keeps_raw` it also stores the value as
written in `Visual.svg_props`:

- SVG-only properties (`is_svg_prop`): `fill`, `fill-opacity`, `fill-rule`, `stroke*`, `stop-color`,
  `stop-opacity`, `clip-rule`, `marker-*`, `paint-order`, `vector-effect`, `shape-rendering`,
  `text-anchor`, `dominant-baseline`, `flood-*`, and the geometry properties `cx cy r rx ry x y d`.
- Ordinary properties that also mean something in SVG (mirrored): `opacity`, `transform`,
  `transform-origin`, `visibility`, `display`, `clip-path`, `mask`, `font-size`, `font-weight`,
  `font-style`, `letter-spacing`. They still go through the normal path too.
- `clip-path` and `mask` accept `url(#id)` (pointing at a `clipPath` or `mask` inside the same SVG).

Stylesheet rules and inline `style` both work, with the normal specificity order. `<style>` elements
and `class` selectors written inside SVG markup are not read; Platypus stylesheets style the content.

### Measuring and drawing

- The `svg` kind is measured like an `image`: `natural_size` reads its `width`/`height`/`viewBox`
  attributes. It has no Taffy children, and descendants are not given rects. `text` inside an `svg`
  is not a text block (`in_svg`), so the text engine skips it.
- `svg_markup(id, size)` writes `<svg xmlns=... width height color font-size>` for the content box,
  then the children depth first. The root's `color` and `font-size` come from its computed CSS, so
  `currentColor` and `em` units work. For each element it writes: `id`, the `attrs.svg` pairs
  (the root's own `width`, `height`, `xmlns` and `color` are skipped), CSS `width`/`height` of
  shapes as attributes, then `svg_props`. `transform` goes through `svg_transform` (units dropped),
  `transform-origin` loses `px`, and `display` is only emitted when `none`. On the root `svg`
  element the mirrored properties its box already applies (`opacity`, `transform`,
  `transform-origin`, `visibility`, `display`, `clip-path`, `mask`: `svg::box_applies`) are not
  written, or they would apply twice (an `opacity` transition on an `svg` faded through 0.64 at
  its midpoint, not 0.8). Generated `::before` and `::after` nodes are skipped; text nodes are
  escaped.
- Drawing: `data_url(markup)` makes a `data:image/svg+xml` URL and the svg is pushed as a `Quad`
  with one image fill sized to the content box (no repeat). From there it is the image path above.
  The element still gets its background, border, radius, clip and opacity as a box.

### Cache and eviction

An inline SVG is a new data URL whenever its markup changes (an animated `stroke-dashoffset` makes
one per frame), so the cache would grow without bound. In the renderer:

- `key_for` keys `data:` SVGs by a 64-bit hash of the URL (`svg-data:<hash>`) instead of the long
  string, and records the frame in `used`. `svg_at` also stamps the sized raster key.
- `begin_frame` (called at the start of `UiRenderer::render`) counts frames; every 30th frame it
  drops entries, rasters and parsed trees unused for more than 120 frames (`SVG_DATA_TTL`).
- File-based SVGs are never evicted; they are keyed by URL and few.

### Limits

- Hit testing sees one box: the whole `svg`. Shapes are not targets and `pointer-events` on them is
  ignored.
- Rasterization is on the CPU, redone when markup, style or the bucketed size changes. Animating
  many large SVGs is costly. GPU path rendering and SMIL (`<animate>`) are TODO "Later".
- Text uses only the bundled Inter. `<image>` inside an SVG is not loaded from the game folder.

## Tests

| ID | What | Where |
|---|---|---|
| `M3-SVG-01` | SVG file as `background-image` (golden, fixture `tests/fixtures/images/icon.svg`) | `crates/web_ui_render/tests/ui.rs` |
| `M3-SVG-02` | `svg` measuring and serialization, `url(#id)` clips, `svg.rs` helper units | `crates/platypus_ui/tests/svg.rs`, unit tests in `crates/platypus_ui/src/svg.rs`, `crates/platypus_ui/tests/clip.rs` |
| `M3-SVG-03` | Inline SVG drawn through the renderer (golden) | `crates/web_ui_render/tests/ui.rs` |
