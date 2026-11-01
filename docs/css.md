# CSS: stylesheets, selectors, cascade and invalidation

Files: [css.rs](../crates/platypus_ui/src/css.rs) (parsing), [cascade.rs](../crates/platypus_ui/src/cascade.rs)
(matching, cascade, rule index, dependencies), and the restyle code in
[tree.rs](../crates/platypus_ui/src/tree.rs). Plan and backlog: TODO.md "M3b" and "M3d". Prefer
standard CSS over s&box extensions (CLAUDE.md).

## Overview

```
game.css / addStyleSheet(css)
   |  parse_stylesheet (cssparser)         css.rs
   v
Stylesheet { rules: [selectors + declarations], font_faces, imports, warnings }
   |  UiTree::add_stylesheet_with_imports: inline @imports, rebase url()s, drop invalid declarations
   v
UiTree.sheets  --restyle_everything-->  RuleIndex + Deps  (cascade.rs)
   |
   |  per dirty node: matched_declarations -> ordered declarations -> apply_style
   v
Node.style (Taffy) + Node.visual
```

Inline `style` props and stylesheet rules end up in the same `apply_style` call
([ui-tree.md](ui-tree.md)), so every property works in both.

## Parsing ([css.rs](../crates/platypus_ui/src/css.rs))

- Uses `cssparser` for syntax and Servo's `selectors` crate for selector parsing. `PlatypusSelectors`
  is the `SelectorImpl`; `CssString` is the string newtype for names and values (with a stable
  `precomputed_hash`, which the ancestor bloom filter needs).
- `parse_stylesheet(css) -> Stylesheet`. Rules keep declarations as **CSS text**
  (`Declaration { prop, value, important }`), with `prop` converted by `camel_case` (kebab to
  camelCase; `--custom` names kept). Values are re-parsed later by `apply_style`, which is why
  they can contain `var()`.
  `space_after_calls` spaces minified function lists (`translateY(0)scale(1)` becomes
  `translateY(0) scale(1)`: a `)` followed by a letter or `#`, outside strings) in every value but
  `--*` ones, since the property parsers split on spaces and transitions compare the text. Test: M3-CSS-13.
- Failures never abort the sheet: an invalid selector, unsupported at-rule, or unknown pseudo-class yields a message in `Stylesheet.warnings` and the next rule
  still applies. `@font-face` becomes a `FontFace`; its `src` is the first `url(...)` entry
  Platypus can load (`first_url`): TrueType, OpenType, WOFF or WOFF2, judged by `format()` or else the
  file extension. `local()`, EOT and SVG entries are skipped, so a typical
  `url(a.eot), url(a.woff2) format("woff2"), url(a.ttf)` list picks the WOFF2. Tests: M3-FNT-03.
- **Nesting** (M3d step 10a, `&` as in CSS Nesting). A rule body is read by `parse_rule_body` with a
  `BodyParser` (declarations, nested style rules, and at-rules: `@media` is supported, any other is skipped with a warning).
  Each nested rule's selector is resolved against the parent's text by `resolve_nested`: a part
  containing `&` has it replaced by the parent, any other part (including ones starting with a
  combinator, `> .icon`) becomes `parent part`; a parent selector list is wrapped in `:is(...)` so
  `&` keeps its meaning. Nested rules are **flattened** into ordinary `StyleRule`s that follow
  their parent (the parent only if it has declarations of its own), so the cascade and the rule
  index see no nesting. Depth is capped at `MAX_NESTING` (16); a bad nested selector warns and
  drops only that rule. Tests: M3-CSS-11 (`css.rs` unit, `tests/css.rs`).
- **`@import`** (M3d step 10b). `@import "x.css"` / `@import url(x.css)` is collected in
  `Stylesheet.imports` as `(url, media)`; with a `supports()`/`layer` condition, or no URL, it warns
  and is skipped. A media query list after the URL is kept (see "`@media`" below). See `@import` under "URL rebasing" below.
- Supported selector features (all parsed by `selectors`): type, class, id, attribute selectors,
  combinators (descendant, `>`, `+`, `~`), `:first-child`/`:last-child`/`:only-child`,
  `:nth-child(.. of S)`, `:nth-last-child`, `*-of-type`, `:root`, `:empty`, `:not`, `:is`, `:where`,
  `:has` (relative selectors). Platypus-specific pseudo-classes (`PseudoClass`): `:hover`, `:active`,
  `:focus`, `:focus-visible`, `:focus-within`, `:enabled`, `:disabled`, `:checked`, `:valid`, `:invalid`, `:required`, `:optional` (text fields,
  see `docs/ui-tree.md` "Text fields"), `:blank`.
  Pseudo-elements: `::before`, `::after` only.
- Element type selectors use the Platypus element names: `box`, `text`, `span`, `root`.

## `@media` ([media.rs](../crates/platypus_ui/src/media.rs))

- **Where it works.** Top-level blocks (`parse_stylesheet` handles `Item::Media` by parsing the
  block as its own sheet), inside a style rule (`BodyParser`; the block's declarations and nested
  rules belong to the parent selector), and on `@import` (`@import "x.css" (min-width: 600px)`).
  An invalid query warns and skips the block.
- **Storage.** Each `StyleRule` has `media: Vec<Arc<MediaList>>`: the conditions of the enclosing
  `@media`s, all of which must match. `expand_imports` inserts an import's media list into every
  rule (and `@starting-style` rule) it brings in. `matched_declarations` skips a rule whose
  conditions don't all match, so the rule index and dependency tracking stay media-unaware.
- **Syntax.** `MediaList::parse` reads a comma list (any query matching is enough) of
  `[not|only] type [and condition]` or a bare condition. Conditions combine with `and`, `or`,
  `not` and parentheses (`Condition`). Types `screen` and `all` match; `print` and unknown types
  don't. Range features take `min-`/`max-` prefixes or the range syntax (`400px <= width < 800px`).
  An unknown feature is `Condition::Unknown` and never matches.
- **Features.** `width`, `height`, `aspect-ratio` (`w/h`), `resolution` (`dppx`, `x`, `dpi`,
  `dpcm`), `orientation`, `prefers-color-scheme`, `prefers-reduced-motion`, `pointer`/`any-pointer`,
  `hover`/`any-hover`. Lengths are px; `em`/`rem` are 16px.
- **Environment.** `MediaEnv { width, height, resolution, dark, reduced_motion, pointer, hover }`
  lives on `UiTree` (`media_env`, `set_media_env`). The size follows `set_size` (UI pixels).
  The app sets `resolution` (the DPI factor) and `dark` (`Manifest::dark`: `project.json` `theme`,
  else the system's preference, [platform.md](platform.md#system-theme)) every frame in
  `PlatformApp::frame`; reduced motion stays off and the pointer stays `fine` with hover until the
  app reports otherwise.
- **Re-evaluation.** `UiTree.media_results` holds the last result of every rule's conditions.
  `check_media` (called from `set_size` and `set_media_env`) recomputes them and calls
  `restyle_everything` only if some result flipped, so resizing within one breakpoint costs
  nothing. `restyle_everything` also refreshes the cached results.
- Tests: M3-MQ-01 (`media.rs` unit: parsing and matching), M3-MQ-02 (`tests/css.rs`: `@media`
  rules applying and un-applying).

## Matching ([cascade.rs](../crates/platypus_ui/src/cascade.rs))

`NodeRef<'a, L>` implements `selectors::Element` over `UiTree`: parent/sibling navigation skips
`#text` nodes and generated nodes (`is_element`), `has_class`/`has_id`/`attr_matches` read
[Attrs](../crates/platypus_ui/src/attrs.rs), and `match_non_ts_pseudo_class` reads `ElementState`
and attrs. `:focus-within` scans the subtree for a focused node.

### Cascade order

`matched_declarations(tree, sheets, index, id, target, caches)` returns two lists, `normal` and
`important`, each sorted by `(specificity, (sheet, rule, declaration))`, lowest priority first.
A rule's specificity is the best among its matching selectors. `restyle` then builds the final
order:

```
normal rule declarations  ->  inline style declarations  ->  !important rule declarations
```

and applies them in that order (later wins). This gives "inline beats rules, `!important` beats
inline", matching browsers. Notes: there is no origin/layer system, and a value like `0.5` is turned into a number by `css_value` so `opacity: .5` equals
the inline form `opacity: 0.5`; `opacity: 40%` is also accepted.

### Rule index

`RuleIndex::build` files each selector once, by the **rightmost compound's** most selective key:
id, else class (first one), else type, else "universal". Selectors with a pseudo-element go to a
separate `pseudo` list. For an element, `candidates(kind, attrs)` gathers universal + type bucket +
id bucket + every class bucket, so most elements test only a few selectors. Candidates are
deduplicated and sorted back into rule order.

## Invalidation and restyle

Goal: after one class or state change, restyle only what can be affected (M3-BEN-01 targets
< 1 ms on a 2,246-node tree).

**Keys.** A `Key` is something a selector can depend on that changes at runtime: `Class(name)`,
`Id`, `Attr(name)`, `Hover`, `Active`, `Focus`, `FocusVisible`, `Disabled`, `Checked`.

**Deps.** `Deps::add_sheet` walks every selector right-to-left (`iter_raw_match_order`), tracking
the *position* of each compound relative to the subject:

| Position | Reached by | Set | Affects when the key changes |
|---|---|---|---|
| Subject | the rightmost compound | (not stored) | only the node itself |
| Ancestor | left of `>` or descendant combinator | `deps.ancestor` | the node's subtree |
| Sibling | left of `+` / `~`, or inside `:nth-child(.. of S)` | `deps.sibling` | its later siblings (and their subtrees) |
| Has | inside `:has()` | `deps.has` | its ancestors |

Plus flags: `focus_within`, `uses_has`, and `structural` (any of `:nth-*`, `:first-child`, `+`, `~`,
`:empty`, `:blank`, `:has`). `:not/:is/:where` recurse with the same position.

**Recording changes.** Attribute edits (`SET_ATTRS`, `attr_keys_changed`) and state changes
(`UiTree::set_state`) push `(node, Vec<Key>)` into `key_changes`. Child-list changes put the
parent in `structure_changed`.

**Resolving them.** At the start of `layout_if_needed`, `restyle_dirty`:

1. Returns early (cheap) when there are no stylesheets and no variable usage: inline styles were
   already applied directly in `apply_op`.
2. `invalidate_keys` marks the node, then its subtree / later siblings / ancestors according to
   which `Deps` set contains a changed key. `invalidate_structure` (only if `deps.structural`)
   marks the parent and its children's subtrees (and ancestors when `:has` is used).
3. Walks the tree in document order with an explicit stack, calling `restyle(id)` for each node in
   `style_dirty` and stopping once the set is empty. Document order matters: parents are restyled
   before children, so inherited variables are ready.

`restyle` recomputes `style` and `visual` from scratch for that node, sets the layout style only
if it changed, marks text dirty if text properties changed, and if the node's custom properties
changed it dirties all descendants.

Adding or removing a stylesheet calls `restyle_everything`: rebuilds the index and deps and dirties
every node. New nodes are inserted into `style_dirty` at `CREATE`.

## CSS-wide keywords

`WideKeyword` ([style.rs](../crates/platypus_ui/src/style.rs)) is `Initial`, `Inherit` or `Unset`;
`WideKeyword::parse(&Value)` also maps `revert` and `revert-layer` to `Unset`, since there is no
user-agent stylesheet to revert to. They work on every property: inline, in stylesheets and in
`::before`/`::after` rules.

`apply_wide_keyword(style, visual, prop, keyword, parent)` applies one, using `is_inherited(prop)`
(text properties, `cursor`, `pointerEvents`, `scrollbarColor`):

| | inherited property | other property |
|---|---|---|
| `initial` | set to its explicit initial value (stops inheriting) | reset |
| `inherit` | reset (unset properties inherit) | copy the parent's value (reset at the root) |
| `unset` | reset | reset |

`reset` now delegates to `copy_prop(style, visual, from_style, from_visual, prop)`, which copies one
property from another style: the defaults for a reset, the parent's for `inherit`. Text properties
are set back to inheriting.

Keywords need the parent's computed style, so they resolve in `restyle`, in cascade order like any
declaration. The `::before`/`::after` path does the same, with the owning element as the parent.
An inline `SET_STYLE` keyword is deferred to restyle (like `var(`), and `add_stylesheet` keeps
keyword declarations without warnings. Tests: [wide_keywords.rs](../crates/platypus_ui/tests/wide_keywords.rs).

## Custom properties and `var()`

- Declarations named `--x` are collected per node into `Node.vars`, an `Rc<HashMap>` that starts as
  the parent's map (inheritance) and is copy-on-write (`Rc::make_mut`) only if this node changes a
  value. Inline `--x` set through `SET_STYLE` takes part in the cascade like any declaration.
- `substitute_vars(value, lookup)` in css.rs expands `var(--name, fallback)`, supporting nested
  fallbacks and variables that reference variables (cycle and depth protected). It returns `None`
  when a variable is undefined without a fallback; `restyle` then **skips that declaration**
  ("invalid at computed time").
- Because validity depends on the element, `add_stylesheet_with_base` does not pre-validate
  declarations containing `var(`, `--` props or `content`; it validates everything else with a
  throwaway `apply_style` and drops bad declarations with a warning, so they are not re-reported on
  every restyle.

## Relative units and math ([units.rs](../crates/platypus_ui/src/units.rs))

`em`, `rem`, `vw`/`vh`/`vmin`/`vmax` (and the `d`/`s`/`l` variants), the absolute units `in`, `cm`, `mm`, `Q`, `pt`, `pc` (96 px per inch, so `1in` is `96px` and `1pt` is `96/72` px; they need no context) and `calc()`, `min()`,
`max()`, `clamp()` are resolved to plain pixel text during restyle, like `var()`, so every property
parser stays unchanged (`2em` becomes `32px`).

- `resolve(text, &UnitContext) -> Option<(String, Uses)>` rewrites a value. `UnitContext` holds
  `em` (the element's own font size; the parent's for `fontSize` itself), `rem` (the root's) and the
  viewport size. `None` means invalid math: the declaration is ignored, like invalid CSS.
  Quoted strings and `url(...)` pass through untouched.
- `needs_resolution(text)` is the cheap pre-check; `needs_resolution_for(prop, text)` also skips a
  plain `fontSize: 2em`, which the font-size parser handles itself (relative to the parent).
- `Uses { em, rem, viewport }` records which kinds a declaration used.
- Math that mixes in `%` (`calc(100% - 20px)`) needs layout, so it is kept as written with its units
  resolved and parsed by [calc.rs](../crates/platypus_ui/src/calc.rs), then evaluated by layout once the
  percentage basis is known (see [ui-tree.md](ui-tree.md#layout)). Properties that accept it: sizes
  (and min/max), insets, margins, padding, borders, gaps and `flexBasis`. Paint lengths (radii,
  shadows, etc.) do not take percentage math yet. Shorthands (`margin`, `padding`, ...) split at the
  top level, so `calc(10% - 2px)` stays one value. Mixing a length with a bare number
  (`calc(10% + 2)`) is invalid.

## Box sizing

`boxSizing: border-box | content-box`. The default is `border-box` (Taffy's and s&box's default),
unlike CSS, whose default is `content-box`. `width`/`height` therefore include padding and border
unless a rule sets `content-box`.

In `restyle`, after `var()` substitution, `fontSize` is resolved first (its `em` is the parent's
size), then `em` is set to the node's own computed size for all other properties. The node's `Uses`
go into `UiTree::unit_uses` (cleared on removal). Dependencies:

- `set_size` restyles every node whose uses include `viewport`.
- A font-size change on a node restyles descendants using `em`; on `ROOT` also those using `rem`.
  The inline fast path (no stylesheets) does the same.
- Generated `::before`/`::after` content resolves with its element's font size.

Validation: stylesheet declarations with relative units are checked at `add_stylesheet` time with
typical units (16px, 1000x1000 viewport); invalid math is dropped with a warning. Inline values go
through the same check: an invalid one is reported and the previous value stays. Tests:
[units.rs](../crates/platypus_ui/tests/units.rs).

## Colours

Code: [color.rs](../crates/platypus_ui/src/color.rs), [color_fn.rs](../crates/platypus_ui/src/color_fn.rs).

`Rgba::parse` accepts hex (`#rgb`, `#rgba`, `#rrggbb`, `#rrggbbaa`), `rgb()`/`hsl()` (and the `a`
variants), named colours and `transparent`. Anything else ending in `)` goes to `color_fn::parse`:

- `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()` and `color(srgb | srgb-linear | display-p3 ...)`.
  Channels may be `none` (treated as 0), and alpha follows `/`.
- Conversion follows the CSS Color 4 sample code: linear-light sRGB as the hub, the standard matrices,
  Lab/LCH on D50 (Bradford-adapted) and OKLab/OKLCH. Out-of-gamut results are clipped to sRGB.
- `color-mix(in <space>, <color> [p%], <color> [p%])` with `srgb`, `srgb-linear`, `lab`, `oklab`,
  `lch` and `oklch`. A missing percentage is the rest of 100% (50/50 if both are missing); weights
  are normalised, and a total under 100% scales the result's alpha. Mixing is premultiplied by
  alpha; hues in `lch`/`oklch` take the shorter way round and are not premultiplied.
  This is the standard form of s&box's `mix()`, `darken()` and `lighten()`, which are also
  supported for s&box compatibility. They work like their SCSS namesakes:
  - `darken(c, amount)` and `lighten(c, amount)` move HSL lightness by `amount` (`20%` or `0.2`)
    and keep the colour's alpha.
  - `mix(a, b, weight)` takes `weight` (default 50%) of `a`, mixed in sRGB.
- Angles (gradient directions) accept `deg`, `rad`, `grad` and `turn`.
- Gradients: `linear-`, `radial-` and `conic-gradient()` and their `repeating-` forms. A stop may
  have two positions (`red 10% 30%`); `parse_stops` takes the position parser. The direction check splits the first argument at the top level, so a first stop such as
  `oklch(0.7 0.1 200)` or `rgb(1 2 3)` is a colour, not an angle.

Invalid functions (wrong channel count, unknown space) are rejected like any invalid value. Tests are
unit tests in [color_fn.rs](../crates/platypus_ui/src/color_fn.rs) and
[paint.rs](../crates/platypus_ui/src/paint.rs) (`M3d` step 1).

## `::before` / `::after`

Handled in `UiTree::update_pseudo`, called at the end of `restyle` for every element when any
sheet exists:

1. `matched_declarations(.., Target::Pseudo(p))` finds the rules for the pseudo-element (matched with
   `MatchingMode::ForStatelessPseudoElement`).
2. The last `content` declaration wins (`parse_content`: quoted strings, numbers, `attr(name)`,
   concatenation). No `content` means no generated node; an existing one is destroyed.
3. Otherwise `insert_generated` creates a `span` containing a `#text` child, with ids from
   `next_generated` (>= `0x8000_0000`), inserted as first (`Before`) or last (`After`) child.
   `attach` keeps an existing `::after` as the last child when React appends siblings.
4. The span gets the pseudo's declarations (including `var()` resolved from the originating
   element's variables); the text is updated in place if the content changed.

Generated nodes are excluded from selector matching, from the tab order, and from hit-test results:
a hit on generated content is reported as the originating element (`hit` returns the parent).

## `::marker`, `::backdrop`, `::selection`, `::placeholder`

Besides `::before`/`::after`, stylesheets can target these pseudo-elements:

- **`::marker`** (lists, M3d step 5g): a generated child like `::before`, but present whenever
  the item has a marker from `list-style`, not only with `content`. It comes before `::before`.
  See [ui-tree.md](ui-tree.md#lists-m3d-step-5g).
- **`::backdrop`**: styles the generated box under a modal `dialog`. See
  [ui-tree.md](ui-tree.md#dialogs-and-the-top-layer-m3d-step-5j).
- **`::selection`** and **`::placeholder`** create no node. `pseudo_declarations` collects the
  matched declarations (with `var()` and relative units resolved) and `update_pseudo_style` stores
  them as a `PseudoStyle` per element. `::selection` supports colour and background (each from the
  nearest element with a rule that sets it); `::placeholder` supports text properties and opacity.
  See [ui-tree.md](ui-tree.md#static-text-selection-and-user-select-m3d-step-5h).

`dialog[open]` and `dialog[modal]` work as ordinary attribute selectors.

## `@font-face`, loading and URL rebasing

The tree does not know the game folder, so font loading is the app's job:

- `AddedStylesheet { id, warnings, font_faces }` is returned from `add_stylesheet*`. In
  [main.rs](../crates/platypus_app/src/main.rs), `add_stylesheet()` logs the warnings (target `css`) and
  calls `load_font` for each `FontFace`, which reads the file via `resolve_asset` (or a `data:` URL)
  and calls `UiTree::register_font(data, family, weight, italic)`. Text re-lays out when fonts load.
- **`game.css`**: `platypus-build build` writes it next to `game.pbundle`. The app loads `bundle.with_extension("css")` and
  passes `base` = its folder relative to the game folder (`"build"`).
- **Rebasing**: the build emits `url()`s relative to the CSS file. With a non-empty `base`,
  `rebase_sheet` rewrites every declaration (rules, `@starting-style`, keyframes) through `css::rebase_urls` (relative URLs
  become `"build/..."`; `data:` URLs and anything with a scheme or leading slash are untouched) and
  prefixes `@font-face` sources. The renderer later resolves the rewritten path against the game
  folder.
- **`@import`**. `UiTree::add_stylesheet_with_imports(css, base, load)` is the real entry point
  (`add_stylesheet_with_base` calls it with a loader that finds nothing). `expand_imports` parses
  the sheet and rebases its URLs (`rebase_sheet`); then for each import it joins the path onto the
  importing sheet's folder (`join_path` resolves `.`/`..` and rejects absolute paths, paths that
  leave the game folder, and anything with a scheme), calls `load(path)` for the text and recurses
  with the imported file's own folder as `base`. Imported rules, `@font-face`s, keyframes and
  `@starting-style` rules come **before** the importing sheet's own, as in CSS. Cycles, nesting
  over 16 deep and failed loads are skipped with a warning. The app's `add_stylesheet` passes a
  loader that reads the file through `resolve_asset` from the game folder. Bun already inlines
  `@import` when it bundles `game.css`, so this matters for runtime sheets (`addStyleSheet`).
  Tests: M3-CSS-12 (`tests/css.rs`).
- **`addStyleSheet(css)`** (CSS-in-JS) goes through the `addStyleSheet` op with `base = ""`, so its
  `url()`s are already game-folder-relative. JS ids map to tree ids in `App.sheets`; re-adding an
  id replaces the old sheet.
- Small assets are inlined as `data:` URLs by Bun; larger ones are copied with hashed names
  (see the Bun CSS gotchas in [CLAUDE.md](../CLAUDE.md)).

## Known limits

- `@import` with `supports()` or `layer` is skipped. `@keyframes`, `@starting-style` and `:outro` are parsed here and run as described in [animation.md](animation.md).
- Pseudo-elements: `::before`, `::after`, `::marker`, `::selection`, `::placeholder` and `::backdrop`; `content` is limited to text.
- Descendant-combinator invalidation is conservative: a changed ancestor key restyles the whole subtree.
