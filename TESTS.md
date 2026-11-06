# Platypus — Test Plan

The tests we work towards, milestone by milestone. Each test has an ID (e.g. `M1-SCR-02`) that [TODO.md](TODO.md) refers to. A milestone is **done** when all of its tests pass in CI.

See [DESIGN.md](DESIGN.md) for the architecture these tests check.

---

## Test kinds

| Kind | Tool | Where | Runs in CI |
|---|---|---|---|
| **Unit** | `cargo test` | inside each crate (`#[cfg(test)]`) | Yes |
| **Integration** | `cargo test` | `crates/*/tests/` | Yes |
| **JS unit** | `bun test` | `js/editor-ui/test/` only (the editor sources; `js/engine`, `js/react` and their tests were removed with QuickJS on 2026-10-28) | No |
| **Headless render** | `cargo test` + wgpu offscreen | `crates/platypus_render/tests/` | Yes (software adapter if no GPU) |
| **Golden image** | headless render compared to a reference PNG with tolerance | `tests/golden/` | Yes |
| **Benchmark** | `criterion` | `crates/*/benches/` | Tracked, warns on regression; doesn't fail |
| **Manual** | a person runs the app | listed in TODO.md | No |

### Conventions
- Headless tests must not open a window. Rendering tests draw to an offscreen texture and read it back.
- If no GPU adapter is available, headless tests fall back to a software adapter (WARP on Windows, llvmpipe on Linux). Tests that truly need a GPU are marked `#[ignore]` and run locally.
- Golden images live in `tests/golden/<test-id>.png`. Updating them is an explicit step (`PLATYPUS_UPDATE_GOLDEN=1 cargo test`), never automatic. Each run also writes its renders to `tests/output/` (git-ignored).
- Test fixtures (tiny TS bundles, glTF files) live in `tests/fixtures/`.

---

## M1 — Hello Platypus (scaffold)

### Workspace and build
| ID | Kind | Checks |
|---|---|---|
| M1-BLD-01 | CI | `cargo build --workspace` succeeds on Windows and Linux. |
| M1-BLD-02 | CI | `cargo clippy --workspace -- -D warnings` and `cargo fmt --check` pass. |
| M1-BLD-03 | CI | `bun install && bun run build` produces `editor/build/game.js` (the examples are built by `platypus-build`, `M9-TS-1850`). (No longer applies: the Bun build and `bun run build` were removed with QuickJS on 2026-10-28.) |
| M1-BLD-04 | CI | `bun run typecheck` passes: `tsc --noEmit` for the JS packages, the editor and the fixtures, then `platypus-build check ../examples --tsc` for the examples (20 built, 0 failed). |

### Platform
| ID | Kind | Checks |
|---|---|---|
| M1-PLT-01 | Integration | The `Platform` layer can be created and destroyed without opening a window (headless mode). |
| M1-PLT-02 | Unit | winit events convert to Platypus input events (key down/up, mouse move, resize, close). |
| M1-PLT-03 | Manual | `cargo run -p platypus_app -- examples/hello` opens a window; closing it exits cleanly with code 0. |

### Rendering
| ID | Kind | Checks |
|---|---|---|
| M1-RND-01 | Headless | A wgpu device can be created (hardware or software fallback). |
| M1-RND-02 | Golden | Clearing an offscreen 64×64 target to a set colour reads back that exact colour. |
| M1-RND-03 | Headless | Resizing the surface/target to 0×0 (minimized window) doesn't panic. |

### Scripting
| ID | Kind | Checks |
|---|---|---|
| M1-SCR-01 | Unit | The QuickJS backend loads a bundle and evaluates `1 + 1` to `2`. |
| M1-SCR-02 | Unit | A JS syntax error returns `Err` with file/line info. It doesn't panic. |
| M1-SCR-03 | Unit | A thrown JS exception returns `Err` with the message and stack. |
| M1-SCR-04 | Unit | `console.log` from JS reaches the Rust log with a `[script]` tag. |
| M1-SCR-05 | Unit | `reset()` wipes globals: a global set before reset is `undefined` afterwards. |
| M1-SCR-06 | Unit | JS can queue ops and Rust receives them in order via `drain_ops()`. |
| M1-SCR-07 | Unit | Strings with non-ASCII text (`"héllo 世界 🎮"`) cross the boundary in both directions unchanged. |
| M1-SCR-08 | Integration | The `ScriptRuntime` trait test suite is generic, so every future backend runs the same tests. |

### Components (minimal)
| ID | Kind | Checks |
|---|---|---|
| M1-CMP-01 | Integration | Loading `tests/fixtures/hello.js` registers a `Hello` component class. |
| M1-CMP-02 | Integration | The engine calls `onStart()` once and `onUpdate(dt)` each tick, looked up by `(component id, method name)`. |
| M1-CMP-03 | Integration | After 3 ticks the component has logged `hello` once and `tick` three times. |
| M1-CMP-04 | Integration | A component that throws in `onUpdate` is reported and disabled; other components keep running. |

---

## M2 — Boxes (React → Taffy → screen)

### Script runtime additions
| ID | Kind | Checks |
|---|---|---|
| M2-SCR-01 | Unit (conformance) | `setTimeout` callbacks fire on the first `tick(now)` at or after their due time, in due-time order; `clearTimeout` cancels. |
| M2-SCR-02 | Unit (conformance) | Promise reactions and `queueMicrotask` callbacks run before control returns to Rust. |
| M2-SCR-03 | Unit (conformance) | `__platypus_commit(ops)` arrives as one commit per call, in order, via `drain_commits()`; `reset()` drops pending commits. |

### Reconciler (JS)
| ID | Kind | Checks |
|---|---|---|
| M2-REC-01 | JS unit | Rendering `<box/>` emits `create` + `appendChild` ops. |
| M2-REC-02 | JS unit | Changing a prop emits only a `setStyle`/`setProp` op for that node, not a recreate. |
| M2-REC-03 | JS unit | Removing a child emits `remove`; reordering emits `insertBefore`. |
| M2-REC-04 | JS unit | One React commit produces exactly one op buffer flush. |
| M2-REC-05 | JS unit | Node IDs are numbers and never reused within a session. |

### UI tree and layout (Rust)
| ID | Kind | Checks |
|---|---|---|
| M2-UI-01 | Unit | Applying an op buffer builds the expected tree (parents, children, order). |
| M2-UI-02 | Unit | Invalid ops (unknown node ID) are reported and skipped; the tree stays valid. |
| M2-UI-03 | Unit | Changing the root size relayouts; percentage widths follow the new size. |
| M2-UI-04 | Unit | The draw list has one quad per visible box in paint order (parents before children, siblings in order); `display: none` and `opacity: 0` subtrees are skipped. |
| M2-STY-01 | Unit | Layout values parse: `10` → 10px, `"50%"`, `"auto"`, `"space-between"`, `"1fr 2fr"`; unknown properties warn and are ignored. |
| M2-STY-02 | Unit | Colours parse: `#f80`, `#ff8800`, `#ff880080`, `rgb(255 136 0)`, `rgba(255,136,0,0.5)`, `transparent`; bad colours warn and keep the previous value. |
| M2-LAY-01 | Unit | Row flexbox: three 100px children in a 300px row land at x = 0, 100, 200. |
| M2-LAY-02 | Unit | `flexGrow`, `padding`, `margin`, `gap`, `justifyContent`, `alignItems` match the CSS spec on fixed cases. |
| M2-LAY-03 | Unit | Grid: a 3×3 grid places items in the right cells. |
| M2-LAY-04 | Unit | Layout only recomputes when something is dirty: a second `compute` with no changes does no work. |
| M2-LAY-05 | Unit | The same fixtures pass through the `LayoutEngine` trait, so a Yoga backend can run them later. |
| M2-UI-OP-01 | Integration | `UiTree::apply_ops` with typed `Op`s builds the same tree as the script arrays; `Op::decode` rejects bad arity, types and opcodes (`crates/web_ui_dom/tests/tree.rs`). |
| ~~M2-LAY-06~~ | Integration | A React render error unmounts the UI (with absolutely positioned boxes in it) and the tree still lays out: removing a node takes it off its containing block's out-of-flow list, so no cached layout visits it. (removed 2026-10-28: QuickJS removed) |
| M2-LAY-07 | Integration | Appending a child to a long column measures only the new text: the cost doesn't grow with the column's length, alone or inside a row flex item sized from its content. |
| M2-LAY-08 | Integration | An absolute box whose parent is static is placed in its containing block, as CSS says: in Fluid's menu bar (title without `position`), `left: 100%; bottom: 0` puts it at the absolute bar's right edge and bottom; with no insets it stays at its static position (the parent's content box) even when the containing block is the root. |
| M2-LAY-09 | Integration | In the same bar with a `position: relative` title, `left: 100%` / `bottom: 0`, `right: -8px` / `top: 0`, `right: 0` / `bottom: 0` and `left: 0` / `top: 0` place the 8x8 box against the title's padding box. |
| M2-LAY-10 | Integration | An empty 96x96 box in a 48x48 `overflow: hidden` box (a flex row, the default) shrinks to 48 wide like a CSS flex item (automatic minimum size 0); with `flexShrink: 0` or a `display: block` parent it keeps 96x96. |
| M2-LAY-11 | Integration | A static box with a transform (`rotate(0deg)`, `translate`), a filter (blur, colour, drop shadow), a backdrop filter, `perspective`, `transform-style: preserve-3d`, `will-change: transform` / `filter`, or `contain: paint` / `layout` / `content` is the containing block of its absolute child (`right: 0; bottom: 0` lands in its corner) and stays put (its `left` still ignored); `will-change: opacity`, `contain: size` / `style`, `opacity` and `transform: none` make none. |
| M2-LAY-12 | Integration | Adding and removing a transform or `will-change` by a class, or a filter by an inline style, moves the absolute child between the box and the root; a relative box keeps its insets with a transform. |

### Drawing
| ID | Kind | Checks |
|---|---|---|
| M2-DRW-01 | Golden | Three coloured boxes in a row render as expected. |
| M2-DRW-02 | Golden | Rounded corners and borders render as expected. |
| M2-DRW-03 | Headless | 2,000 boxes are drawn in a small, fixed number of draw calls (batched). |
| M2-DRW-04 | Golden | The same draw list at scale factor 2 renders at twice the pixel size. |
| M2-DRW-05 | Golden | Per-corner radii and borders, gradient fills, an image fill with `cover` and `contain`; repeated tiles; no-repeat edges don't bleed; missing or unsafe `url()`s fall back to the background colour. |
| M2-DRW-06 | Golden | Outer and inset box shadows with blur and spread, including on rounded boxes. |
| M2-DRW-07 | Golden | `filter: blur` blurs a box and its children; nothing outside the layer bounds changes. |
| M2-DRW-08 | Golden | `backdropFilter: blur` blurs only what's behind the box, clipped to its rounded shape. |
| M2-DRW-09 | Integration | Drawing skips subtrees scrolled out of a clipping container (beyond a margin), also below intermediate boxes; scrolling brings them back; a far-off box with a shadow is still drawn. |
| M2-DRW-10 | Integration | `backdropFilter: blur` reads only the backdrop inside the element's box: of two stacked glass boxes over a blue/orange split, each stays one even colour up to the shared edge (no colour bleeds in from the neighbour's side). |
| M2-DRW-11 | Integration | `radial-gradient(circle, transparent 44px, red 52px)` over white: clear inside 44px, a fade from transparent that never darkens (premultiplied interpolation), red outside 52px. |
| M2-DRW-12 | Pixel | A backdrop blur reflects the backdrop at the box edge instead of repeating the edge pixel: a white column 3 px inside the left edge reaches the edge pixel through two taps (about 0.092 of white, clamping gives 0.065). |
| M2-DRW-13 | Pixel | Absolute children placed by `left: 100%` and by `right: -8px` with `bottom: 0%` paint where their insets put them, outside a non-clipping parent. |
| M2-CPU-01 | Integration | The CPU renderer draws the same radial gradient: stops start at the first stop's radius, not the centre, and the fade from `transparent` is premultiplied. |
| M2-PLN-01 | Unit | The frame plan of two boxes: two instances in one `Draw` step after a `Clear` of the canvas, one texture, no layers or clips (no GPU). |
| M2-PLN-02 | Unit | A plan for a canvas seeded with the world has no `Clear` step. |
| M2-PLN-03 | Unit | A blurred, translucent layer: three textures (canvas, layer, blur scratch), a `Blur` step at the scaled sigma and a `Composite` onto the canvas at its opacity. |
| M2-PLN-04 | Unit | An `overflow: hidden` clip is one clip-table entry and its children's instances carry its index. |
| M2-PLN-05 | Unit | Text plans one glyph instance per visible glyph and queues the glyph pixels in the `GlyphCache` (not again on a second plan). |
| M2-PLN-06 | Unit | An image fill plans a `Draw` step naming its texture; an image the resources don't know is skipped. |
| M2-PLN-07 | Unit | `needs_backdrop` finds backdrop filters without planning. |
| M2-PLN-08 | Unit | A character the font lacks (glyph 0) is drawn as a red box of the font's own "NO GLYPH" letters stacked one per row, at least 0.34 em wide and as tall as the font's `.notdef`; all seven letters are cut out, every letter keeps its own pixels when the Y is redrawn (the L's foot runs under the Y), and the Y gets a tail down its middle. |
| M2-PLN-09 | Unit | Layers side by side (four rows at `opacity: 0.5`) take turns with one frame-sized texture (each cleared, then composited onto the canvas); two nested layers need two. |
| M2-GLES-01 | Integration | A GLES 3 context through EGL (ANGLE via `PLATYPUS_ANGLE_DIR`) clears an `SRGB8_ALPHA8` framebuffer and reads it back; skipped without an EGL driver. |
| M2-GLES-02 | Integration | Boxes with borders (solid, dashed, dotted, per-side colours), per-corner radii and linear, radial and conic gradients: the GLES executor matches the wgpu one pixel for pixel (within 6/255 on all but 0.2% of pixels). |
| M2-GLES-03 | Integration | Outer and inset box shadows match between the GLES and wgpu executors. |
| M2-GLES-04 | Integration | `overflow: hidden` clips with a rounded border (several children) match between the two executors. |
| M2-GLES-05 | Integration | Text at two sizes and colours, including dark text on light, matches between the two executors (the shared glyph cache, uploaded into the GLES pages). |
| M2-GLES-06 | Integration | Boxes and text at scale 2 and a rotated plane (the clip table's transforms) match between the two executors. |
| M2-GLES-07 | Integration | Background images match between the GLES and wgpu executors: an unrepeated PNG, a tiled one at a set size, `cover` with rounded corners, nearest-neighbour upscaling, and SVGs (`contain`, a set size over a colour). |
| M2-GLES-08 | Integration | Background images at scale 2 (a rounded tiled PNG and an SVG rasterized at twice the size) match between the two executors. |
| M2-GLES-09 | Integration | A translucent group (children composited once at 50%) and two blurred boxes match between the GLES and wgpu executors. |
| M2-GLES-10 | Integration | `drop-shadow()` (hard, and blurred) and eight colour filter functions (grayscale, brightness, contrast, hue-rotate, sepia, invert, saturate, opacity) match; a drop shadow at scale 1.5 (a shift between texels) too. |
| M2-GLES-11 | Integration | All 15 `mix-blend-mode`s over a gradient match between the two executors. |
| M2-GLES-12 | Integration | `backdrop-filter` blur on a rounded translucent box and `backdrop-filter: grayscale()` match between the two executors. |
| M2-GLES-13 | Integration | Alpha, luminance and repeated-dot `mask-image`s and a translucent layer inside a rounded `overflow: hidden` clip match between the two executors. |
| M2-GLES-14 | Integration | `render_over` seeds the canvas from a plain `RGBA8` target: a `backdrop-filter: blur()` box mixes the target's red and blue halves at their edge, far pixels round-trip unchanged, and an empty draw list leaves the target exactly as it was; `render_overlay` of the same box leaves the target untouched. |
| M2-GLES-14 | Integration | A new GLES renderer has compiled only the quad and blit programs; boxes and plain text add none and leave the colour glyph page unsized; a blurred box compiles the blur and composite programs once; with ANGLE linked in, the driver trim succeeds and the next frame's pixels are the same. |
| M2-GLES-15 | Unit | The GLES path trims the driver once, no sooner than a second after the first frame shown, and not with `PLATYPUS_GLES_TRIM=0`. |
| M2-GLES-16 | Integration | Two stacked `backdrop-filter: blur(10px) saturate(1.6)` glass boxes over a blue/orange split: the GLES executor reads only inside each box (even colour up to the shared edge) and matches the wgpu one. |
| M2-GLES-17 | Integration | A radial gradient from `transparent` 44px to red 52px matches between the two executors and fades without darkening. |
| M2-GLES-18 | Integration | An absolute dropdown in a static rounded `overflow: hidden` bar shows in full below it while one in a positioned bar is clipped at its height, the same in both executors. |
| M2-GLES-19 | Integration | After `release_canvases` (an idle window or layer gives back its frame-sized canvases and seed) the next `render_over` with a backdrop blur draws the same pixels, and `canvas_bytes` is 0 in between. |
| M2-KRN-01 | Integration | The CPU plan executor (`web_ui_render_cpu::plan`) draws boxes, solid, dashed and dotted borders, per-side colours and linear, radial and conic gradients as the GLES executor does (into a plain RGBA8 target; skipped without EGL). |
| M2-KRN-02 | Integration | Outer and inset box shadows match the GLES executor. |
| M2-KRN-03 | Integration | Children of a rounded `overflow: hidden` box are clipped as by the GLES executor. |
| M2-KRN-04 | Integration | Text at 1x, light on dark and dark on light, matches the GLES executor (the same glyph atlas, read on the CPU). |
| M2-KRN-05 | Integration | At scale 2, text, a rounded box, a rotated box and a box in a perspective plane match the GLES executor. |
| M2-KRN-06 | Integration | PNG and SVG background images (no-repeat, sized, cover, rounded, pixelated, contain, tiled), at 1x and 2x, match the GLES executor. |
| M2-KRN-07 | Integration | An opacity group and blurs of radius 8 or less (exact Gaussian taps) match the GLES executor. |
| M2-KRN-08 | Integration | Blurs past radius 8 (three box passes of the Gaussian's spread instead of its taps) stay within 10/255 of the GLES executor everywhere. |
| M2-KRN-09 | Integration | Drop shadows (hard, soft, at 1.5x) and the eight `filter` colour functions match the GLES executor. |
| M2-KRN-10 | Integration | The fifteen `mix-blend-mode`s match the GLES executor. |
| M2-KRN-11 | Integration | Backdrop blur and grayscale, and Fluid's stacked menu glass (no bleed between the boxes), match the GLES executor. |
| M2-KRN-12 | Integration | Alpha and luminance masks, a tiled mask and a translucent layer in a rounded clip match the GLES executor. |
| M2-KRN-13 | Unit | Two threads running jobs on one pool at once both finish, every band of each run once (the second caller draws its bands itself while the first's job runs). |
| M2-BLEND-01 | Unit | `project.json` `blending` is `srgb` when left out (and isn't written back), `linear` and `srgb` are kept, an unknown value warns and means `srgb`. |
| M2-BLEND-02 | Unit | The plan premultiplies colours in the frame's blending space (`rgba(128,128,128,0.5)`: 0.251 in sRGB, 0.108 in linear light) and encodes the clear colour for an sRGB canvas; `FramePlan::blending` says which. |
| M2-BLEND-03 | Integration | `examples/alpha-probe` (25% white fill and borders, square and rounded, 50% red, a 25% black border over it, glass over a backdrop blur, over `#3060c0`) gives the browser's values (100,136,208; 152,48,96; 114,36,72; the field inside borders) within 1 on wgpu and on GLES into sRGB and plain targets, by default. |
| M2-BLEND-04 | Integration | With `Blending::Linear` the probe keeps the linear-light values (142,157,210; 191,69,141; 167,59,123) on every executor. |
| M2-BLEND-05 | Integration | An opacity group, `filter: opacity()` and a gradient to transparent white each give (255 + field) / 2 in sRGB; an opaque gradient's midpoint is (128,0,128) in both modes; the opacity group in linear light is (190,199,228). |
| M2-BLEND-06 | Integration | A hard 50% black spread shadow halves the field in sRGB; `filter: blur()` averages sRGB values (white on black just inside the edge: 136 in sRGB, 193 in linear light). |
| M2-BLEND-07 | Integration | An image drawn texel for texel keeps its file's colours on sRGB and linear canvases, on wgpu and GLES. |
| M2-BLEND-08 | Integration | Text coverage blends in sRGB: white text on black and black text on white both carry less light than in linear light, and covered pixels stay the text colour. |
| M2-BLEND-09 | Integration | The CPU renderer gives the probe's sRGB values (it always blends in sRGB). |
| M2-BLEND-10 | Unit | The app draws screen UI over a copy of the world when blending in sRGB onto an sRGB-encoding target (not in linear, onto a plain target, or with nothing to draw), and the FPS counter joins the frame's list, under a scale transform when its scale differs. |
| M2-BLEND-11 | Integration | 25% white over a `#3060c0` world with `render_over` gives 100,136,208 in sRGB and 142,157,210 in linear light; `render_overlay` onto an sRGB target gives the linear-light value in either mode. |
| M2-THEME-01 | Unit | The settings portal's `color-scheme` maps 1 to dark, 2 to light and 0 (or an unknown number) to no preference, through `ReadOne`'s variant and `Read`'s doubly wrapped one alike (another type gives nothing); `gsettings` `'prefer-dark'` / `'prefer-light'` / `'default'` map the same and other output is ignored. |
| M2-THEME-02 | Unit | A stored system scheme wins over the window's (winit) theme, `set_system_scheme` reports only real changes, and no preference means no theme (light). |
| M2-THEME-03 | Unit | `project.json` `theme` is `system` when left out (and isn't written back) and then asks the system; `light` and `dark` are kept and force it without asking; an unknown value warns and means `system`. |
| M2-THEME-04 | Manual | On Linux (GNOME, KDE or Fluid), switching the desktop between light and dark restyles a running game's `prefers-color-scheme` rules in the main window and its secondary windows within a frame; `theme: "dark"` keeps it dark. |
| M2-BLEND-12 | Integration | GLES `set_linear_blur`: on sRGB canvases a `backdrop-filter: blur()` over a black/white edge averages in linear light (193 just inside the white, into sRGB and plain targets; 136 without it, as browsers), black and white stay so, and linear canvases give 193 too. |

### Box visuals (styles and draw commands, no GPU)
| ID | Kind | Checks |
|---|---|---|
| M2-VIS-01 | Unit | `hsl()/hsla()` and named colours parse (`rebeccapurple`, `hsl(120 100% 25%)`). |
| M2-VIS-02 | Unit | `border: "2px solid red"` sets all widths and the colour; `border: "none"` clears; unsupported styles (`dashed`) warn. |
| M2-VIS-03 | Unit | `borderRadius` 1–4 values map to the right corners; per-corner props; `"50%"` gives a circle on a square box. |
| M2-VIS-04 | Unit | `boxShadow` lists parse: `"0 4px 12px rgba(0,0,0,.4), inset 0 0 0 1px #fff"` → two shadows with the right fields; bad lists keep the old value. |
| M2-VIS-05 | Unit | `filter`/`backdropFilter` accept `blur(Npx)` and `none`; other functions warn. |
| M2-VIS-06 | Unit | Gradient and `url()` backgrounds parse; size/position/repeat keywords and lengths parse. |
| M2-VIS-07 | Unit | Draw order per node (CSS painting order; `filter` and opacity apply to the box's own shadows too): backdrop blur → push layer (if filter or group opacity) → outer shadows (last in the list first) → fill/border → inset shadows → children → pop layer. Layer bounds cover everything drawn inside plus 3× the blur. |
| M2-VIS-08 | Unit | With group opacity, a half-opaque parent emits one layer with opacity 0.5 instead of multiplying children's alpha. |

### Integration (React in QuickJS)
| ID | Kind | Checks |
|---|---|---|
| ~~M2-INT-01~~ | Integration | The `boxes` fixture bundle runs in `QuickJsRuntime`; applying its commits to `UiTree` gives the expected tree and layout rects. (removed 2026-10-28: QuickJS removed) |
| ~~M2-INT-02~~ | Integration | A `setTimeout`-driven state change produces only incremental ops (no recreation of unchanged nodes). (removed 2026-10-28: QuickJS removed) |
| M2-MAN-01 | Manual | `platypus-app examples/boxes`: resizing the window relayouts smoothly; the counter adds and removes boxes. |

### Performance
Benches: `cargo bench -p web_ui_dom` (layout) and `cargo bench -p platypus_script` (commits). The tree is an
editor-like UI of **2,246 nodes** (toolbar, a 600-row file tree, a grid of 150 cards with shadows, an
inspector of 40 fields), applied as one commit of 6,738 ops.

| ID | Kind | Initial target | Measured (2026-10-16, release, RTX 5080 desktop CPU) | Revised target |
|---|---|---|---|---|
| M2-BEN-01 | Benchmark | Full layout: < 2 ms | **3.46 ms** | < 5 ms (one-off at load) |
| M2-BEN-02 | Benchmark | Relayout after one leaf change: < 0.2 ms | **0.58 ms** | < 1 ms |
| M2-BEN-03 | Benchmark | Apply the whole tree's commit: < 1 ms | **3.10 ms** (was 5.1 ms; boxing nodes halved our share) | < 5 ms (one-off at load) |
| M2-BEN-04 | Benchmark | JS→Rust transfer of 1,000 ops: < 0.5 ms | **0.40 ms** (0.59 ms including building them in JS) | < 0.5 ms ✓ |
| — | Reference | Relayout after a window resize | 0.43 ms | — |
| — | Reference | Draw list for the whole tree | 0.19 ms | — |

**Where the time goes** (profiled with a timing layout-engine wrapper): about 85% of a relayout is Taffy
recomputing flex/grid for the changed node's ancestors (its pixel rounding is only ~0.1 ms); our own
bookkeeping is ~0.1 ms. Applying a commit is ~1.8 ms of Taffy node creation plus ~1.7 ms of ours (about
0.3 ms of that is style parsing).

**Decision: keep Taffy.** The initial targets were guesses made before measuring. The measured costs
fit comfortably in a frame: a worst-case incremental update of this 2,246-node UI (relayout + draw list)
is ~0.8 ms, about 5% of a 60 Hz frame, and full builds happen once at load. Yoga would do the same
ancestor relayout work, so a Yoga spike isn't justified now. Revisit at M5.e with the real editor UI
(virtualized lists will keep node counts down); the `M2-LAY-05` fixtures are ready if we do.

---

## M3 — Real UI: text, CSS, interaction

### M3a — Text and fonts
| ID | Kind | Checks |
|---|---|---|
| M3-TXT-01 | Golden | A text label renders crisply at 1× and 2×, including non-Latin text (CJK) and a colour emoji. |
| M3-TXT-02 | Unit | Text wraps to the container width; the measured height feeds back into layout (a `<text>` in a column pushes its siblings down). |
| M3-TXT-03 | Benchmark | Re-measuring unchanged text hits the cache (no shaping); changing one text node re-shapes only that node. |
| M3-TXT-04 | Golden | `color`, `fontSize`, `fontWeight: bold`, `fontStyle: italic`, `lineHeight`, `letterSpacing`, `textAlign`, `textDecoration`, `whiteSpace` render as expected. |
| M3-TXT-05 | Golden | `textShadow` with offset, blur and colour; multiple shadows stack in CSS order (first on top). |
| M3-TXT-06 | Unit | `color` and font properties inherit to descendants unless overridden; `currentColor` follows `color` (borders, shadows, gradients). |
| M3-TXT-07 | Unit / JS unit | React text children and nested `<span>`s produce text runs with the right styles; text updates send `SET_TEXT`; bare strings in `<box>` get an anonymous text wrapper. |
| M3-TXT-08 | Unit | Complex scripts segment with dictionaries: Ctrl+Shift+Right in `日本語の文章` selects `日本語`, and unspaced Thai wraps at word boundaries in a narrow box. |
| M3-TXT-09 | Unit / Integration / JS unit | Right-to-left text: `direction` parses and inherits; the `dir` attribute sets it unless CSS does (and `[dir]` selectors match); rtl blocks start at the right and `text-align: start/end` follow direction; mixed Latin/Hebrew runs order by the paragraph direction; rtl inputs and textareas align right (placeholder too) and arrow keys move visually; React sends `dir` as an attribute. |
| M3-TXT-10 | Unit + Integration + Golden | `text-transform` (capitalize across runs, `ß` → `SS`) and `text-overflow` (not inherited) parse; an ellipsis cuts lines only where overflow clips, keeps glyphs inside the box, ends with the `…` glyph, and goes on the left for rtl. |
| M3-TXT-11 | Integration | Long words break with `overflow-wrap: anywhere`, `word-wrap: break-word`, `word-break: break-word` and `break-all`; `pre-line` keeps newlines but collapses spaces, unlike `break-spaces`. |
| M3-TXT-12 | Unit + Integration | The `font` shorthand (resets left-out parts; bare numbers are weights), size keywords, named weights and `bolder`/`lighter`; `font-variant-numeric` and `font-feature-settings` become features, and `tabular-nums` gives digits one width. |
| M3-TXT-13 | Unit + Golden | The `text-decoration` shorthand (lines, style, colour, thickness; resets what it omits) and longhands parse; solid, double, dotted, dashed, wavy, overline and thick line-through decorations render. |
| M3-TXT-14 | Unit + Golden | `text-stroke` / `-webkit-text-stroke` and longhands parse and inherit; hollow outlined text and filled text with a stroke render. |
| M3-TXT-15 | Unit + Integration + Golden | `caret-color` and `font-smooth` / `-webkit-font-smoothing` parse and inherit; a field draws its caret in `caretColor` (or the text colour for `auto`); unsmoothed text has no partly covered pixels. |
| M3-FNT-01 | Integration | A font loaded with `loadFont` (and with `@font-face`) is used by `fontFamily`; text relayouts when it arrives; missing fonts fall back down the list, then to the bundled default; unsafe paths are rejected. |
| M3-FNT-02 | Unit | Picking weight/style from a family with several faces chooses the closest match (CSS font matching); a family with only an italic face uses it for normal text instead of falling back to another family. |

### M3b — CSS stylesheets and selectors
| ID | Kind | Checks |
|---|---|---|
| M3-CSS-01 | Unit | Stylesheets parse: rules, comments, `!important`, `@font-face`; unknown properties and at-rules warn without dropping the rest of the sheet. |
| M3-CSS-02 | Unit | Structural selectors match: `:first-child`, `:last-child`, `:only-child`, `:nth-child(2n+1)`, `:nth-child(odd of .x)`, `:nth-last-child`, `:first-of-type`, `:empty`, `:root`; combinators `>`, `+`, `~`, descendant. |
| M3-CSS-03 | Unit | Logical selectors: `:not(.a)`, `:is(.a, .b)`, `:where()` (zero specificity), `:has(> .child)`, `:has(+ .sibling)`. |
| M3-CSS-04 | Unit | Cascade: specificity, then source order; inline `style` beats rules; `!important` beats inline; inherited properties flow down. |
| M3-CSS-05 | Unit | Custom properties and `var(--x, fallback)`, including inheritance and fallbacks for undefined variables. |
| M3-CSS-06 | Unit + Golden | `::before`/`::after` with `content: ""` and `content: "text"` create first/last generated children with their own styles; removing the rule removes them; they're invisible to React. |
| M3-CSS-07 | Unit | Restyle after changes: adding a class, inserting a sibling (`:nth-child`, `+`), and a descendant change that affects a `:has()` ancestor all update the right nodes. |
| M3-CSS-08 | Unit | Simple selectors: type, `*`, `.class` (multiple classes), `#id`, `[data-x]`, `[data-x="y"]`, and compound forms like `box.card#main`, with correct specificity. |
| M3-CSS-09 | Unit | `:empty` matches no children and no text; `:blank` also matches whitespace-only text; neither matches a node with a child box. |
| M3-CSS-10 | Unit + Integration | `:hover`, `:active`, `:focus`, `:focus-visible`, `:focus-within`, `:enabled`, `:disabled`, `:checked` follow element state; toggling `disabled`/`checked` props restyles; disabled elements ignore pointer and focus. |
| M3-BEN-01 | Benchmark | Incremental restyle on the 2,246-node tree after one class change: **< 1 ms**; full style of the tree tracked. |

M3b restyle results (`cargo bench -p web_ui_dom --bench restyle`; 2,246 nodes, ~30-rule editor stylesheet
with combinators, `:nth-child`, `:has`, `:focus-within` and variables; times include the following
layout pass):

| Measurement | Naive (match every rule, invalidate generously) | Rule index + dependency-aware invalidation |
|---|---|---|
| Style the whole tree | 50 ms | **8.8 ms** |
| One class change mid-list (M3-BEN-01) | 2.2 ms | **0.20 ms** ✓ |
| Hover a card | 0.61 ms | **0.21 ms** |
| Focus an input (`:focus-within`) | 0.21 ms | **0.20 ms** |

### M3c — Interaction
| ID | Kind | Checks |
|---|---|---|
| M3-EVT-01 | Integration | A click hits the topmost node under the point, respecting rounded corners, `pointerEvents: none`, `display: none` and overflow clipping. |
| ~~M3-EVT-02~~ | Integration | Events capture down and bubble up the path; `stopPropagation()` stops them; `preventDefault()` is reported back. (removed 2026-10-28: QuickJS removed) |
| ~~M3-EVT-03~~ | Integration | `onClick` in a React component runs when its box is clicked; `:hover` / `:active` styles apply while the pointer is over / pressed. (removed 2026-10-28: QuickJS removed) |
| ~~M3-EVT-04~~ | Integration | Tab / Shift-Tab follow `tabIndex` and tree order; the focused node gets key events; `:focus`, `:focus-visible`, `:focus-within` apply. (removed 2026-10-28: QuickJS removed) |
| M3-EVT-06 | Integration + JS unit | Pointer capture needs a held button; while captured, moves and hover go to the capturer even over another element; the release goes to it and clicks it, then `lostpointercapture` and hover returns to what's under the pointer; capture ends on `releasePointerCapture`, removal and window blur; React sends the ops and `hasPointerCapture` follows the events. |
| M3-EVT-05 | Integration + Golden | `overflow: hidden` clips children (including rounded clips); `overflow: scroll` scrolls with the wheel and keeps hit testing consistent with what's drawn. |
| M3-EVT-07 | Integration | A span inside text is the target over its own characters (and in event paths, under its text); elsewhere, and over a `pointerEvents: none` span, the text element is hit. |
| M3-EVT-08 | Integration + JS unit | `scrollIntoView` puts an element at the start, centre or end of each scroll container, or (`nearest`) scrolls only as far as needed and not at all when it's visible, clamped to the scroll range; `ref.scrollIntoView()` takes the DOM's arguments and defaults (`block: "start"`, `inline: "nearest"`; `false` is the end). |
| M3-EVT-09 | Integration + JS unit | Smooth scrolling: `behavior: "smooth"` (or `auto` under `scroll-behavior: smooth`) on `scrollTo` / `scrollBy` / `scrollIntoView` eases in and out over 250 to 500 ms by distance, sends `scroll` events as it moves, stops when something scrolls it directly, and `scrollBy` steps add up from where it is heading; refs take the DOM's option objects (`{ left, top, behavior }`). |
| M3-EVT-10 | Integration | `scroll-padding` (1 to 4 lengths, the side longhands, `block` / `inline`) insets the view `scrollIntoView` aligns to, so an element lands clear of a header or fade at the container's edges. |
| M3-IME-01 | Integration | IME composition (pre-edit) text shows in a text field and the committed text is inserted; the candidate window is positioned at the caret. |
| M3-BG-01 | Unit + Golden | `conic-gradient` (start angle, centre, angle/percentage stops, two-position stops) and the `repeating-` gradients parse, and render as a pie, repeating stripes and a rotated off-centre conic. |
| M3-BG-02 | Unit + Golden | Background layer lists (images, clips, repeats, positions, `position-x/-y`) parse; `round`/`space` fit whole tiles; a content-box-clipped colour, two clipped layers, spaced tiles and gradient text (`background-clip: text`) render. |
| M3-BG-03 | Unit + Golden | Blend modes parse; `mixBlendMode` multiply, screen, difference and color give the W3C results over a backdrop; `backgroundBlendMode` multiplies a layer into the colour without touching what's beneath the element. |
| M3-BG-04 | Golden | Gradients follow `background-size`, `-position` and `-repeat`: a repeated 20px tile makes stripes; a single no-repeat tile sits at the right. |
| M3-BG-05 | Unit + Golden | `radial-gradient(circle at X Y, ...)` parses its centre (a colour before `at` is rejected); a radial centred on the top-right corner is sized to the far corner on both executors; a pixel-stop ring paints inside an overflow-hidden parent. |
| M3-BRD-01 | Unit + Golden | Border styles and per-side colours parse (shorthands, longhands, 1-4 values; `none` sides take no space); side colours join on the diagonals, and dashed, dotted, double, inset and a single dashed side render. |
| M3-BRD-02 | Unit + Integration + Golden | Elliptical radii parse (`a / b`, per-corner pairs) and resolve like CSS (`50%` of a wide box is an ellipse); hit testing follows the ellipse; ellipses and shallow corners with a border render. |
| M3-BRD-03 | Unit + Golden | Outline properties parse (shorthand defaults, `auto`, negative offsets; `hidden` rejected); a solid outline sits outside the box past its offset, and a dashed one follows rounded corners. |
| M3-BRD-04 | Unit + Golden | `border-image` shorthand and longhands parse; the nine pieces map slices to the border (fill, round tiles, scaled-down widths, outset); tiling along an axis; an image 9-slice, a gradient border and a filled one render. |
| M3-CLIP-01 | Unit + Integration + Golden | SVG path data flattens (relative commands, curves, arcs, flags without separators; malformed data rejected); `polygon()` and `path()` clips resolve against the reference box and hit testing follows them (evenodd holes); a triangle, a framed hole and a clipped opacity group render. |
| M3-CLIP-02 | Integration | Drawing: an absolute dropdown whose containing block is outside a static `overflow: hidden` bar is drawn without the bar's clip (its siblings before and after keep it); a clipping containing block, a positioned bar, a transformed bar and the root behave as CSS says; a `clip-path` between still clips it, an opacity group outside the bar still holds it, one inside the bar keeps the clip (a known limit). |
| M3-CLIP-03 | Integration | Hit testing matches: the dropdown is hit below the bar and over it, a positioned bar's dropdown is not hit outside the bar, and the bar's clipped-off siblings stay unhittable. |
| M3-CLIP-04 | Integration | Scrolling: a dropdown in a static scroller doesn't move when it scrolls, its `scroll_path` skips the scroller, and the wheel over it doesn't scroll the scroller (over the scroller's own content it does). |
| M3-FX-01 | Unit | `opacity` accepts numbers, numeric strings and percentages. |
| M3-FX-02 | Unit + Golden | Filter chains parse (amount defaults and limits, blurs combine, colour functions compose in order, bad functions rejected; no `drop-shadow` in `backdrop-filter`); grayscale, brightness, invert, hue-rotate, sepia, a shape-following drop shadow and a greying backdrop filter render. |
| M3-FX-03 | Unit + Golden | `image-rendering` parses and inherits; a scaled-up image blends between texels by default and keeps them pure when `pixelated`. |
| M3-FX-04 | Unit + Golden | Mask lists and modes parse; alpha and luminance gradient masks fade a box; a repeated dot mask cuts out the box and its child. |
| M3-FX-05 | Pixel | A spread box shadow and a negative-offset shadow as large as half their box paint inside an `overflow: hidden` parent. |
| M3-XF-01 | Unit | Every 2D and 3D transform function maps points like CSS (rotation direction, order, origin, `matrix`/`matrix3d` layouts, perspective scaling); homographies invert; back faces are detected; the individual properties and `transform-origin` parse; bad functions are rejected. |
| M3-XF-02 | Integration + Golden | Hit testing follows rotation, 3D turns and `backface-visibility`; `preserve-3d` combines turns before flattening; transform properties accept every form and keyword; a rotated bordered box with text, a skewed gradient, a card in perspective with a clipped child, and scaled text render. |
| M3-XF-03 | Unit + Golden | Text under a transform that only scales evenly and moves (`Homography::as_scale_translate`) is rasterized at its on-screen size in window space, so it stays sharp while it pops in or is scaled (golden `M3-XF-02`); rotated, skewed, unevenly scaled or perspective text keeps sampling its masks in the plane. |
| M3-VIS-01 | Integration | `visibility: hidden` keeps layout space, draws nothing and takes no hits (the element behind is hit); a `visible` child still draws and is hit; a visible span in hidden text draws only its own glyphs. |
| M3-LST-01 | Integration + Golden | List items get markers numbered by position (renumbered after removals); every marker type, strings and `none`; outside markers hang in the padding while inside ones take space; `::marker` rules style markers; discs, numbers, inside roman numerals, string markers on text items and image markers render. |
| M3-SEL-01 | Integration | Static text: dragging selects and Ctrl+C copies; a click collapses; double-click selects a word and triple-click a line; `user-select: none` blocks it (and shows the arrow cursor), `all` takes the whole block, a child can opt back in with `text`; presses map through transforms. |
| M3-SEL-02 | Integration | Selected text draws a highlight (default blue); an ancestor's `::selection` sets its background and recolours the selected glyphs; `::placeholder` sets a field placeholder's colour, opacity and font size. |
| M3-CUR-01 | Unit + Integration | `cursor` image lists parse (keyword last, two-number hotspots) and inherit; bad lists are rejected; disabled fields show `not-allowed`; PNG cursors take the CSS hotspot and scale with the window, `.cur` files bring their own hotspot, oversized or broken images are skipped; every CSS keyword maps to an OS cursor. |
| M3-DLG-01 | Integration + JS unit | Closed dialogs are hidden and keep their authored `display`; open ones centre; modal ones paint and hit above fixed `zIndex` elements over a window-sized `::backdrop` (default tint, `::backdrop` rules); a modal takes focus, traps Tab and gives focus back; Escape and (with `closedBy="any"`) backdrop clicks send `cancel` then `close`, a prevented `cancel` stops `close`, `none` ignores both; `[open]`/`[modal]` selectors; React sends the attributes and runs non-bubbling `onCancel`/`onClose`. |
| M3-DLG-02 | Golden | A modal dialog over a darkened, blurred `::backdrop` on a striped page. |
| M3-DLG-03 | Integration | A script that focuses a field inside a modal as it opens keeps that focus, and closing the dialog gives focus back to what had it before; `UiTree::to_window` maps a point through the dialog's centring translate to where it is hit (`to_local` undoes it) (`crates/web_ui_dom/tests/dialog.rs`). |
| M3-ANI-01 | Unit | Timing functions (`ease` mid-point, `cubic-bezier` limits, every `steps()` position); values interpolate token by token (lengths, numbers, colours premultiplied, mixed units through `calc()`, transforms with matching functions, `none` as the neutral form); non-matching values don't; the `transition` shorthand and property matching. |
| M3-ANI-02 | Integration + JS unit | Hover rules and inline styles (without stylesheets) transition over time with delays and easing; a reversal starts from the current value; unlisted properties snap and removing `transition` ends it; `transitionrun`/`start`/`end`/`cancel` events reach scripts with the property and elapsed seconds, and `onTransitionEnd` bubbles in React. |
| M3-ANI-03 | Unit + Integration + JS unit | `@keyframes` parse (offsets sorted, selector lists, bad blocks dropped, `!important` ignored); the `animation` shorthand; sampling by phase, direction, fill mode and pause; keyframes with implicit ends and discrete flips; stylesheet and inline animations run, iterate, fill, pause and resume, end and report events; infinite alternating animations keep running; `onAnimationEnd` gets the name. |
| M3-ANI-04 | Integration | New elements transition from `@starting-style`; elements already styled ignore it when a class adds one; opening a dialog runs its starting style. |
| M3-ANI-05 | Integration | With `:outro` rules a removed element stays, fading out, unhittable and unfocusable, until its transition ends; without `:outro` rules (or with nothing to animate) removal is immediate; an exit animation holds removal but an ongoing infinite one doesn't. |
| M3-SEL-03 | Golden | A focused field's selected text (default highlight), static text dragged across under an ancestor's `::selection` (yellow, recoloured glyphs), and a field's styled `::placeholder`. |
| M3-ANI-06 | Golden | One keyframe animation (colour, rotation, radius) sampled at 0/25/50/75/100% through negative delays, and a width + colour transition caught halfway. |
| M3-SCR-01 | Integration | `scrollbar-gutter: stable` narrows a clipping box's content by the scrollbar and its margins (less for `thin`, nothing for `visible`); `overscroll-behavior: contain` stops a wheel at an inner scroller's end from scrolling the outer one. |
| M3-GAME-01 | Golden | Image tints: an untinted grayscale tile, red and half-transparent blue tints, a green-tinted border image and a yellow-tinted `image` element. |
| M3-GAME-02 | Unit | UI scale modes (none, fixed, target height, fit a design) give the right factor, clamped and safe for a minimised window; `UiScale.none/fixed/height/fit` set the mode the host reads and a zero, negative or NaN size leaves it as it was (`platypus_engine_api::ui_scale`; re-added natively 2026-10-28 after the QuickJS `setUiScale` went). |
| M3-CSS-11 | Unit + Integration | Nested rules flatten against their parent (`&`, implicit descendant, `>`, compound `&.x`, ident-first `text:first-child`, selector lists via `:is()`), declarations after nested rules still belong to the parent, bad nested rules and nested at-rules warn; nested `:hover` rules restyle children. |
| M3-CSS-12 | Integration | `@import` loads files relative to the importing sheet (nested imports relative to theirs), applies them before the importer's rules, rebases their `url()`s, and warns about missing files, cycles and conditional imports. |
| M3-CSS-13 | Unit | Minified function lists get a space between calls (`translateY(0)scale(1)` → `translateY(0) scale(1)`, also before `#` colours); strings and custom properties are kept as written. |
| M3-CSS-15 | Integration | `content` strings decode CSS escapes: `\201C` (hex, with its closing space), escaped quotes, `\` + newline continuations (the css example's curly quote). |
| M3-CSS-14 | Integration | Setting text on an empty text node (and emptying it again) re-evaluates `:empty` on its parent and `:has(span:not(:empty))` above it. |
| M3-CSS-15 | Integration | `content` strings decode CSS escapes: `\201C` (hex, with its closing space), escaped quotes, `\` + newline continuations (the css example's curly quote). |
| M3-SVG-01 | Golden | An SVG file (gradient, dashed stroke, path, text) as a 32px background and a 96px `image` rasterized sharp at that size, and a tiled `data:image/svg+xml` background. |
| M3-SVG-02 | Unit + Integration | Attribute names (camelCase kept, kebab otherwise), natural sizes from width/height/viewBox, escaping, CSS transforms in SVG syntax; an `svg` sizes from its attributes, serializes its content with attributes, CSS presentation properties and `currentColor`, restyles on `:hover` and transitions `fill`; its content isn't laid out, it's hit as one box, its `text` isn't a Platypus text block, and attribute/CSS size changes resize it. |
| M3-SVG-03 | Golden | Inline SVG at 2x: a gradient from `defs` clipped by a `clipPath`, a dashed `currentColor` ring, a CSS-rotated polygon and SVG text, all styled from a stylesheet. |
| M3-SVG-04 | Integration | An `svg` whose opacity transitions on a parent's `:hover` fades in and out through its box's layer only (not also through an `opacity` in its markup), and isn't drawn at opacity 0. |
| M3-FNT-03 | Unit + Integration | WOFF and WOFF2 subsets of Inter register under a family name and decode to plain TrueType data; truncated files are rejected; `@font-face` `src` lists pick the first usable format, skipping `local()`, EOT and SVG fonts. |
| M3-MQ-01 | Unit | Media queries: min/max and range syntax, em units, media types with `not`/`only`, comma lists, nested `and`/`or`/`not`, aspect ratios, resolution in dppx/dpi, colour scheme, pointer/hover, reduced motion; unknown features don't match and malformed queries are rejected. |
| M3-MQ-02 | Integration | `@media` rules (top level and nested in a rule) apply and stop applying as the window resizes and the colour scheme changes; `print` never applies; `@import ... (min-width)` imports apply only when the query matches, and `supports()` imports are skipped. |
| M3-IMG-01 | Integration / JS unit | `image` elements: `Image` sends `src` as an attribute; the tree requests each `src` once, takes the natural size the app reports (one known axis keeps the ratio; failed loads stay empty), and draws the image in the content box per `objectFit` / `objectPosition`, clipped. |
| ~~M3-INP-01~~ | Integration | A controlled `<input>`: typing calls `onChange`, the value round-trips, selection with mouse and Shift+arrows works, copy/paste uses the clipboard. (removed 2026-10-28: QuickJS removed) |
| M3-INP-03 | Integration | `<input type="password">`: five `i`s and five `W`s draw alike (one bullet each) while `editor_value` and `input` events carry the typed text; edits (Backspace, Delete, typing at the caret, over a selection, paste, a typed bullet, multi-byte characters) land where the caret is; Ctrl+Arrow/Backspace/Delete go to the ends; copy and cut give nothing and cut deletes nothing; `value` and `type` changes keep the text. |
| M3-INP-04 | Integration | `type="number"` drops letters (except `e`) from typed and pasted text and ArrowUp/ArrowDown step by `step` from `min`, stopping at `min`/`max` without float noise and snapping to the grid; `type="search"` clears on Escape; `maxLength` cuts typing and paste (a selection makes room, a password counts characters); `email` is plain text. |
| M3-INP-05 | Integration | Constraint validation of text fields: `required` is missing only when empty; `email` and `url` need their shape; `pattern` must match the whole text (a bad pattern is ignored, a textarea has none) and `minLength` counts characters; `number` checks `min`, `max`, the `step` grid from `min` (`any` lifts it) and bad input; `:valid` / `:invalid` / `:required` / `:optional` restyle as the user types and when `pattern` changes, and a disabled field is never invalid. |
| M3-INP-02 | Integration | Text-field shortcuts (Ctrl/Cmd+A/C/X/V) follow the physical key `code` when the layout's `key` isn't a Latin letter (Russian layout: `key` "ф", `code` "KeyA" selects all); AltGr (Ctrl+Alt) still types. |
| M3-PAD-01 | Integration | Gamepad button events from `gilrs` reach scripts (using a simulated device where possible). |
| M3-PAD-02 | Unit | Each pad connection reaches scripts once: pads already plugged in are announced on the first poll, a repeated `Connected` for a known pad is dropped, and one that disconnects and comes back is announced again. |
| M3-MAN-01 | Manual | `platypus-app examples/form` on Windows: hover/active styles, typing with a Japanese or Chinese IME, scrolling a long list. |

### M3d — Floats and tables

| ID | Kind | What |
|---|---|---|
| M3-FLT-01 | Integration | A text block beside a left float starts its lines after the float while they touch it, and returns to the edge below it; its box keeps the full width. |
| M3-FLT-02 | Integration | A right float sits at the right edge and the lines end before it. |
| M3-FLT-03 | Integration | Floats on both sides leave the middle for the lines. |
| M3-FLT-04 | Integration | Floats of one side sit next to each other, then wrap below the first. |
| M3-FLT-05 | Integration | `clear` moves a block below the floats; a block without it keeps its place. |
| M3-FLT-06 | Integration | A float sticks out of a plain block; a `flow-root` (and a formatting context root) contains it. |
| M3-FLT-07 | Integration | A word wider than the room beside a float moves the line below the float. |
| M3-FLT-08 | Integration | `text-align` aligns each line within the room beside a float. |
| M3-FLT-09 | Integration | `float` is ignored on flex and grid items. |
| M3-FLT-10 | Integration | Removing, changing or resizing a float relayouts the text beside it (no stale cache). |
| M3-FLT-11 | Integration | A float's margins keep text away from it. |
| M3-FLT-12 | Integration | Text with padding and a background wraps around a float inside its padding box. |
| M3-FLT-13 | Integration | An `image` floated in a block of bare text: the paragraph wraps around it. |
| M3-FLT-14 | Integration | A later paragraph stays clear of an auto-height float's bottom margin. |
| M3-FLT-15 | Integration | The taller of two floats keeps narrowing lines after the shorter one ends (needs taffy 0.15.1: 0.15.0 lost the tail of the taller float). |
| M3-TBL-01 | Golden | The table of `examples/table`: content-sized columns, header, striping, a wrapping note, right-aligned numbers, a column-span footer. |
| M3-TBL-02 | Integration | In a grid of `display: contents` rows the columns line up across rows and are sized from every row's content; the `1fr` column takes the rest. |
| M3-TBL-03 | Integration | A long cell wraps and every cell of its row grows to the row's height. |
| M3-TBL-04 | Integration | `text-align: right` aligns numbers to the cell's edge. |
| M3-TBL-05 | Integration | `grid-column` / `grid-row` spans act as colspan and rowspan and following rows' cells skip the spanned column. |
| M3-TBL-06 | Integration | A `display: contents` row is hovered through its cells, and `.row:hover .cell` styles only that row's cells. |
| M3-FLT-20 | Golden | A page with a left picture, a right pull-quote, a paragraph flowing between and under them, and a `clear: both` bar. |

---

## M4 — Hot reload

| ID | Kind | Checks |
|---|---|---|
| M4-HOT-01 | Integration | Changing a component's `onUpdate` and reloading runs the new code on the next tick. |
| ~~M4-HOT-02~~ | Integration | `@property` values and script components' plain fields survive a reload (`speed` set to 42 is still 42, a counter keeps counting), references between objects are remapped, `onStart` doesn't run again and `onReload` does, and the clock carries on. (removed 2026-10-28: QuickJS removed) |
| ~~M4-HOT-03~~ | Integration | Objects, hierarchy and transforms come back where they were after a script reload (rebuilt from the saved scenes). (removed 2026-10-28: QuickJS removed) |
| M4-HOT-04 | Integration | A reload with a syntax error (or a throwing top level) keeps the old code running and reports the error. |
| ~~M4-HOT-05~~ | Integration | A renamed or removed property is dropped with a warning; new properties get their defaults; a saved class that isn't registered any more makes the reload start over. (removed 2026-10-28: QuickJS removed) |
| M4-HOT-06 | Integration | A dev build reloaded in place: a React counter keeps its count while rendering with the new code (Fast Refresh), a live component answers with its new class and keeps its fields, and the top level's style sheet is swapped, not added twice. |
| ~~M4-HOT-07~~ | Benchmark | Reload of a ~1 MB bundle with 1,000 components (each with a child object), keeping their state: **< 100 ms** (`crates/platypus_scene/benches/reload.rs`; ~146 ms on 2026-10-17, not met yet). (removed 2026-10-28: QuickJS removed) |
| M4-HOT-08 | Manual | Edit a TS file while the game runs; the change appears without restarting. |
| M4-HOT-09 | Integration | Clearing the UI tree for a reload removes every script node (attached or not, skipping `:outro`), and the new bundle's reconciler can reuse IDs from 1. |
| M4-HOT-10 | Integration | The bundle watcher reports a change to `game.js` once, after it settles; other files in the folder are ignored. |
| M4-HOT-11 | Unit | A texture made with `Texture.create` (point filtered, clamped, no mipmaps) is saved by `__platypus_save_state` and restored with its pixels and options, re-sent to the engine as before, and the restored material uses it. |

---

## M5 — Scene, assets and rendering

| ID | Kind | Checks |
|---|---|---|
| M5-OBJ-01 | Unit | GameObjects nest; walking is depth-first in order; parenting under itself is refused; destroying takes the subtree (parent first) and stale ids miss after their slot is reused. |
| M5-OBJ-02 | Unit | World transforms compose through the hierarchy (rotation and scale), cached or not; re-parenting can keep the world position; setting a world position under a rotated parent lands there. |
| M5-OBJ-03 | Unit | Moved objects (with their children) are reported once; unchanged sets aren't moves; a disabled parent deactivates its subtree. |
| M5-OBJ-04 | JS unit | GameObjects: creating and parenting send ops and mirror the hierarchy; re-parenting keeps the world position; world transforms compose through parents (`position`, `forward`, `lookAt`, `eulerAngles`, `inverseTransformPoint`); engine moves update the mirror without echoing ops. |
| M5-OBJ-05 | JS unit | Components: `addComponent` with props, `getComponent`, `findObjectOfType`, removal; engine-spawned components get the engine's object. |
| ~~M5-OBJ-06~~ | Integration | In QuickJS, a component creates a turret with a child and a `Spinner`; the spinner turns it 45° in 30 ticks (the child swings with it in Rust's scene), stops when the turret is disabled, and gets `onDestroy` when it's destroyed with its children. (removed 2026-10-28: QuickJS removed) |
| ~~M5-OBJ-07~~ | Integration | `scene.*` ops: bad ops fail; script changes aren't echoed back but engine moves are, as local transforms; destroying reports the subtree; ids past 2^24 stay exact. (removed 2026-10-28: QuickJS removed) |
| M5-MTH-01 | JS unit | `Vector3` / `Vector2` / `Vector2Int` / `Mathf` behave like Unity's (magnitude, normalized, angle, reflect, projectOnPlane, moveTowards, signedAngle, deltaAngle, ...) and are immutable. |
| M5-MTH-02 | JS unit | `Quaternion`: Euler degrees in Unity's order round-trip; `lookRotation`, `fromToRotation` (also opposite vectors), `slerp`, `rotateTowards`; Unity's statics `Quaternion.slerp` / `slerpUnclamped` / `lerp` / `lerpUnclamped` / `rotateTowards` / `dot` (clamped, the short way round). |
| M5-MTH-03 | JS unit | `Plane` / `Ray` / `Bounds` / `Rect` queries; `Pose` and `Vector4`; `Color.hex` (sRGB to linear and back) / `lerp`; `Mathf` extras (angles, `smoothDamp`, Perlin noise, powers of two, gamma). |
| M5-TIM-01 | JS unit | `TimeSpan`: parts, totals, C# constant and custom formats, parsing, millisecond rounding. |
| M5-TIM-02 | JS unit | `DateTime`: months 1-12, calendar arithmetic with month-end clamping, C# standard and custom formats, JS `Date` conversion, ISO parsing, UTC/local views. |
| M5-HLP-01 | JS unit | `Debug` / `Log` / `Console` reach the right console level and format vectors, objects, maps and cycles. |
| M5-HLP-02 | JS unit | `Time` advances per tick and `timeScale` scales the `dt` components get; `TimeSince`, `TimeUntil`, `Task.frame`. |
| M5-HLP-03 | JS unit | Seeded `Rng` / `Random` repeat and stay in range (ints, floats, unit circle/sphere, rotations, shuffle). |
| M5-HLP-04 | JS unit | Unity-style input: `getKeyDown`/`getKeyUp` last one tick (repeats aren't presses); `getAxis` combines keys and sticks; `getButton`. |
| M5-RND-05 | Golden | The world pass: a floor in perspective, a green plane seen through a half-transparent blue one, a textured plane, and a translucent screen-UI bar blended over the 3D view. |
| M5-RND-06 | Golden | Cameras draw into their viewports in depth order: a perspective view on the left, an orthographic top-down view on the right (a floor marker to the east appears right of centre), and a picture-in-picture inset over the left view. |
| M5-CAM-01 | JS unit | `Matrix4x4` (TRS, inverse, determinant); camera conversions for perspective and orthographic cameras with a `rect` (screen and viewport points round-trip, parallel orthographic rays); `Camera.main` / `allCameras` order; `CameraFollow` and `CameraOrbit`. |
| ~~M5-OBJ-08~~ | Integration | Lifecycle hooks run in Unity's order: start, fixed updates, update, late update, pre-render, destroy. (removed 2026-10-28: QuickJS removed) |
| M5-OBJ-09 | Unit | `Scene::tree_version` changes for renames, new children, children moved away and objects destroyed below an object, not for moves, flags, an unchanged name or edits in another tree. |
| M5-OBJ-11 | Unit | `Scene::moved_since`: after `update_world` nothing has moved; a move stamps the object and everything below it (not its parents or other trees) until the next update; a parent's move reaches children moved earlier in the epoch; stale objects, re-parented ones and destroyed ones count as moved. |
| M5-OBJ-10 | Unit | `mark_moved` stops at objects already stale and listed: over 3000 random moves, re-parentings, creations, destructions, `update_world`s and `take_moved`s in 40-deep hierarchies the moved list equals the old walk-the-whole-subtree one, in order, and every world matrix equals one composed from the locals. |
| M5-PNL-01 | JS unit | Each React root (screen or world panel) commits to its own engine tree with its target id; updates to a panel's nodes only go to the panel's tree; one root per target. |
| M5-PNL-02 | Unit | Rays meet panel quads at the right UV and distance (centre, corner); rays behind or parallel miss. |
| M5-PNL-03 | Manual | In `examples/world`, hovering the world panel's menu items highlights them, clicking counts, and the screen HUD still takes clicks over the panel. |
| ~~M5-RB-01~~ | Integration | In QuickJS with the app's physics sync: a crate with a `Rigidbody` and `BoxCollider` falls through a trigger and lands on a static floor (its GameObject at rest 0.5 m up; `onTriggerEnter` then `onCollisionEnter`); a kinematic body follows its script-moved GameObject. (removed 2026-10-28: QuickJS removed) |
| M5-RB-02 | JS unit | Colliders make a static body at the object (scaled sizes, materials); a `Rigidbody` makes it dynamic with mass and constraints; colliders share the body; `ForceMode` impulses; Unity's capsule height; contacts reach `onCollisionEnter/Exit` and `onTriggerEnter` on both objects. |
| M5-MSH-01 | Unit | Built-in primitives are valid, have unit normals, wind outwards (positive volume) and have Unity's sizes; missing normals are made from triangles. |
| M5-ANI-01 | JS unit | `AnimationCurve`: linear and eased segments, steps (infinite tangents), loop / ping-pong wrapping, smooth keys through their points, sorted keys, saving with steps kept. |
| M5-ANI-02 | JS unit | `AnimationClip`: curves by path pose descendants' transforms (one op each) and component properties, including a field of a value (`color.g`); missing paths are skipped; JSON round trip; events call methods on the object's components once per pass, looping too. |
| M5-ANI-03 | JS unit | `Animation`: play, crossFade weights (halfway is an even blend), clips played once hold their end and stop, inactive objects don't animate. |
| M5-ANI-04 | JS unit | `Animator`: a 1D blend tree mixes by its parameter; triggers start transitions and are used up; exit-time and Any State transitions (no re-entry), cross-fades; rotations blend the short way; `play` / `crossFade`; controllers save as JSON and in scenes. |
| M5-ANI-05 | Integration | `__platypus_animate(dt)` runs each frame after every `onUpdate`. |
| M5-ANI-07 | Unit | Native (`platypus_animation`): an `Animator` state keeps normalized time, so a blend tree whose clips differ in length keeps its phase when its parameter moves to the longer clip, then advances at the blend's pace (`animator.rs`). |
| M5-ANI-08 | Unit | Native (`platypus_animation`): bindings resolved once per (clip, root) give the same transforms, `active` flags, component writes and moved lists, bit for bit, as the string-keyed `Pose` they replaced (kept as a test oracle, `legacy_pose.rs`), over 300 random hierarchies, clips (duplicate, invalid and missing curves, curves sharing key times so they share the key search, other wrap modes), weights and times with renames, re-parenting, new children and destroyed objects between frames, one reused `Pose` throughout; a rename or a curve edit after binding is seen, and the kept clip length follows edits and curves added later (`pose_tests.rs`). |
| M5-ANI-09 | Unit | `Animator.cullingMode = cullUpdateTransforms`, unseen: no transform is written, but the clock advances and clip events fire; seen again, the pose is where it would have been (`culling.rs`). |
| M5-ANI-10 | Unit | `cullCompletely`, unseen: nothing runs, no events, the clock stops and the last pose holds; seen again it resumes from where it stopped. |
| M5-ANI-11 | Unit | A `cullUpdateTransforms` animator unseen for 25 frames of varying length gives, on every seen frame, exactly the transforms of an always-animated twin. |
| M5-ANI-12 | Unit | By default (`alwaysAnimate`) visibility is never asked and animation goes on unseen; `Animation.cullingType = basedOnRenderers` pauses its states' times while unseen; the components' `cullingMode` / `cullingType` properties set and read back, and `culledRoots` lists their objects. |
| M5-ANI-13 | Unit | The app's watch (`anim_cull.rs`): a culled object is seen through any renderer on it or below it, from the last frame's report; a renderer not watched before counts as seen; an object with no renderers is always seen; with no culled objects nothing is watched. |
| M5-ANI-17 | Unit | Animators posed in parallel (`parallel.rs`, 2, 3 and 8 threads) give every object's local transform bit for bit and the same events in the same order as the update in turn, over 90 frames of 64 figures with blend trees, triggered transitions, every culling mode, a legacy `Animation` among them and a moving parent; with one figure on another's arm (nested) too (that frame runs in turn); a clip animating `GameObject.active` keeps the update in turn. |
| M5-ANI-16 | Unit | Scene files (`platypus_scenes` kinds): an `Animator`'s controller, speed, `cullingMode` and `enabled` and an `Animation`'s clip, clips, `playAutomatically` and `cullingType` survive save then load (controller and clips made again from the file's shared `assets`), a prefab and `instantiate` (the live controller and clips kept); a file without these fields loads with the defaults and no warnings. |
| M5-ANI-15 | Unit | `Animator.update(dt)` poses a culled (`cullCompletely`, unseen) animator at once and keeps its events; with `enabled = false` the frame's update leaves it alone (pose held) while `update` still advances it; enabled and updated by hand it advances twice. |
| M5-ANI-14 | Unit | The culling safeguard (`anim_cull.rs`): an animated object with a physics component, `AudioSource`, `ParticleSystem`, `Light` or script component strictly below it is always seen and its renderers are not watched, with one warning (naming the object and component) per object; a component on the animated object itself does not block; the answer is worked out again only when a component is added or removed (`platypus_kinds::components_version`) or the culled objects' tree versions change; the blockers come from the physics, render, audio and script registries, and adding or removing counts. |
| M5-ANI-06 | JS unit | An `Animator` state keeps normalized time (Unity's): a blend tree whose clips differ in length keeps its phase when its weights change (a walk breaking into a run carries on mid-stride), then advances at the blend's pace. |
| M5-SKN-01 | Unit | A skinned glTF (`worm.glb`, made by `make_worm.py`) gives joints and normalized weights per vertex, its skin's joints and inverse bind matrices, and its animation's channels (rotation keys, a step channel). |
| M5-SKN-02 | JS unit | Skinned model parts get a `SkinnedMeshRenderer` with their bones (ids and bind poses in the op); the file's animations become clips with paths from the copy's root and play on their own; script meshes send bone weights (normalized) and bind poses. |
| M5-SKN-04 | Unit | Native (`platypus_animation::gltf`): a glTF animation's channels become a looping clip; rotation keys that switch sides are flipped to agree (cubic keys with their tangents), so sampling halfway between `q` and `-q` keeps the turn. `Model.animations` / a copy's `Animation` are built from them (`platypus_render_api` `models.rs`). |
| M5-SKN-03 | JS unit | glTF rotation keys that switch sides (q, then -q: the same turn) are flipped to agree when made into curves, so sampling between them keeps the turn (it summed to near zero and normalized to a wild pose: a leg flung up for a frame); cubic keys flip with their tangents. |
| M5-SKN-05 | Unit | Skinned renderers on one skeleton (the same bones and bind poses: a figure's body and joints) share one run of bone matrices in the views (`world_view.rs`); the same bones with other bind poses get their own. |
| M5-RND-21 | Golden | A tube skinned to two bones bends where the upper bone turns, ignoring its object's matrix, and casts a bent shadow. |
| M5-RND-22 | Golden | The editor's ground grid (`WorldView.grid`): 1 m and 10 m lines on y = 0 fading with distance, the X axis red and the Z axis blue, hidden behind a cube standing on the origin. |
| M5-RND-23 | Golden + JS unit | Gizmo lines (`WorldView.lines`, `render.gizmos`): a constant width on screen, faint behind a cube, cut at the near plane. `Gizmos` draws through `matrix` in `color` (lines, rays, a cube's 12 edges, a sphere's three circles, a frustum along -z) and `present` hands them to one camera, replacing what it had. |
| M5-RND-24 | Golden + unit + JS unit | Strokes (`WorldView.strokes`, `render.stroke`, `LineRenderer2D`): capsules a width in world units with round joins and caps, soft edges, a 2-pixel line still shows, hidden behind a cube; segments follow their object, a closed loop joins the last point to the first, a camera's layers hide them, `remove` drops them; the component sends its points flat once per frame and `render.remove` on destroy. |
| M5-RND-25 | Golden + unit + JS unit | Fills (`tessellate_fill`, `WorldView.fills`, `LineRenderer2D.fillColor`): a square fills with two triangles covering its area, a self-crossing star has a hollow middle with even-odd and a solid one with non-zero, broken points are skipped; rendered fills sit under their outline and a see-through fill blends; fills follow their object and a width of 0 draws no outline; `Shapes` (native since 2026-10-22, the JS unit moved to `M9-API-1600`..`1602`: rectangles, rounded rectangles, circles, ellipses, arcs, pies, regular polygons, Béziers, cardinal curves through every point, tension 0 straight, closed curves). Shapes draw by `sortingOrder`, then the order they were made, each fill with its outline (`WorldView.shapes`). |
| M5-RND-26 | Unit | A `Mirror` without a normal faces its local +z on a quad (the mirror wall of `examples/materials`) and +y on anything else; a written normal wins; a quad `MeshRenderer` added after the mirror turns it (`crates/platypus_app/src/render_sync.rs`). |
| M5-TXT-01 | Unit + JS unit | `TextMesh` (`render.text`): its panel fits the text (wider for longer text, two lines for a line break, wrapped and taller at `maxWidth`), takes the colour as its tint and `pixelsPerUnit` as its scale, and isn't interactive; the component sends its text and look once per frame and `render.remove` on destroy. `examples/shapes` shows it in the window. |
| M5-SPL-01 | JS unit | `Spline`: passes through its knots, auto tangents match GDI's cardinal curve, linear knots are straight and bezier knots use their handles, `evaluate` goes by distance across uneven segments, closed splines join back, `nearest` finds the closest point, knots can be inserted, moved, re-moded (an auto knot turned bezier keeps its shape) and removed. `SplineAnimate` moves its object along the path (loop, ping-pong, once), facing along it, and in a container's space. Since 2026-10-22 `Spline` is native: its cases are `M9-API-1603`..`1605` (QuickJS) and `M9-TS-3112` (compiled); the `SplineAnimate` case is `M9-TS-3116` (compiled, native component). |
| M5-PRE-01 | JS unit | `Prefab`: captured from a live tree, copies are independent and placed where asked, references inside point at the copy's objects and outside ones are dropped; later changes to the original don't leak in; JSON round trip; copies from a file save its path; other data is refused. |
| M5-AST-01 | Unit | The asset watcher reports changed files by game path once settled (not builds); a texture whose file changes loads again under its id with its new size, a model again with the same mesh ids, other files change nothing. |
| M5-AST-02 | JS unit | `Assets.onChanged` listeners hear changed paths until stopped; a throwing listener doesn't stop the others. |
| M5-AST-03 | Bun unit | `scripts/assets.ts`: assets (not code, builds or config) get a `.meta` with a GUID and their kind's importer; reruns change nothing; a moved file's meta follows it with its GUID; orphans are reported and pruned only on request. |
| M5-AST-04 | Unit | The engine reads `.meta` files into paths, GUIDs and importers (sorted; packages and broken metas skipped). |
| M5-AST-05 | JS unit | GUID lookups both ways; `"guid:..."` texture and sound sources; importer settings as `Texture.load` defaults (options win) and a streamed sound; scenes save GUIDs and load a renamed texture by its GUID. |
| M5-CLO-01 | Unit | The cloth solver: a grid pinned along an edge hangs about its length below the pins, keeps its edges' lengths, and its pins follow the object; a falling sheet drapes over a sphere without entering it; seams weld into one particle; capsules push points out sideways. |
| M5-CLO-02 | Unit | The app's cloth system: a plane pinned along an edge hangs (the far edge swings down), its mesh gets normals each step with one upload queued; cloths go with their objects; unknown ops fail. |
| M5-CLO-03 | JS unit | `Cloth` sends its settings, pin box and colliders, draws a simulated copy of its renderer's mesh, and gives the original back (freeing the copy) when removed. |
| M5-AST-06 | Unit | Cooking: BC1 / BC3 blocks round-trip closely (one-way gradients tightly), cut-outs keep their holes and smooth alpha its steps; cooked textures and models save and load back (models under fresh ids, descriptions renumbered); a linear request doesn't take an sRGB cook; a newer source makes the cook stale. |
| M5-MAT-01 | Unit | Tangents made from UVs point along increasing u with the bitangent (up the image) on the side `w` says; mirrored UVs flip it; every primitive's tangents are unit length and perpendicular to its normals. |
| M5-MAT-02 | JS unit | `Material` sends its maps, specular colour, rendering mode, cut-off and double-sidedness with `render.meshRenderer`; `with()` changes one setting and keeps the rest; scenes save the settings that differ from the defaults (and textures' `linear` / `anisoLevel`) and load them back. |
| ~~M5-RB-03~~ | Integration | A `CharacterController` walking with `simpleMove` along a floor covers the expected distance, stays grounded and keeps its capsule on the floor. (removed 2026-10-28: QuickJS removed) |
| M5-JNT-01 | Integration | Joints in `platypus_physics`: a rope keeps a ball within its length of a world anchor, a ball joint keeps a pendulum's length, a motorised hinge turns, a spring settles near its rest length; bad joints are refused; removing one works. |
| ~~M5-JNT-02~~ | Integration | Through the components: a `Rope` from a world point holds a weight about its length below; a `HingeJoint` with a motor turns its wheel. (removed 2026-10-28: QuickJS removed) |
| M5-AUD-01 | JS unit | `AudioSource`: a 2D source plays its clip on awake (path clips load once) and applies volume, pitch and mute to what's playing; pause / unPause / stop; a 3D source plays from an emitter that follows its object (only real moves are sent) and is removed with it; one-shots don't interrupt `play()`; `playClipAtPoint` removes its emitter when the sound ends. |
| M5-AUD-02 | JS unit | `AudioListener` sends its pose on awake and when it moves or turns; `AudioListener.volume` sets the master volume. |
| M5-TEX-01 | Unit + JS unit | Textures: `Texture.load` sends the file and settings and gets its size from `__platypus_textures` (`whenLoaded`, failures reject); `Texture.create` sends sRGB bytes top row first from linear colours bottom row first; materials carry a texture and tiling to `MeshRenderer`; the app decodes pixels into uploads, refusing mismatched sizes and paths outside the game; mip levels average in linear light, weighted by alpha. |
| M5-TEX-02 | Unit + JS unit | Sprites: `SpriteRenderer` sends its sprite (rect, pivot, pixels per unit, colour, flips) and clearing it removes the quad; `Sprite.grid` cuts sheets left to right, top to bottom; the app sizes the quad by pixels per unit, puts the pivot on the object's origin and flips by UV. |
| M5-RND-08 | Golden | Textured meshes and sprites: a tiled, mipmapped checker floor, a textured cube and an offset sphere, a see-through cube using a texture's alpha, a missing texture drawing the magenta checker, and point-filtered sprites upright and flipped. |
| M5-PRT-01 | Unit | Particle simulation (`crates/platypus_app/src/particles.rs`): rate and bursts emit on time, particles die after their lifetime, `stop` lets them finish; one-shot systems stop after their duration and keep to `maxParticles`; gravity pulls local particles down in the world whatever the object's rotation; cones spray within their angle from their disc; size and colour over lifetime and texture-sheet frames; additive quads; unknown settings are refused. |
| M5-PRT-02 | JS unit | `ParticleSystem` sends its settings on awake and again only when they change (nested edits too); `play` / `stop` / `pause` / `emit` become ops; `__platypus_particles` reports update `particleCount` / `isPlaying` and fire `onStopped` and `stopAction: "destroy"`. |
| M5-RND-09 | Golden | Glows: bloom (HDR world pass, threshold, intensity, scatter) on a hot emissive bar, an additive sphere, additive soft spots, and a spot light's beam (soft edges, fading along its length). |
| ~~M5-RB2D-01~~ | Integration | 2D physics through the app's sync (`tests/fixtures/src/physics2d.ts`): a box at z = 2 falls through a 2D trigger onto an edge at z = 0 (`onTriggerEnter2D`, `onCollisionEnter2D`), rests on it and keeps its z and its x/y rotation; a circle rolls down an edge slope; a capsule with `freezeRotation` stands upright; a polygon lands; `Physics2D.raycast` hits the trigger above the box. (removed 2026-10-28: QuickJS removed) |
| M5-RB2D-02 | JS unit | 2D colliders: boxes and circles become slabs `Physics2D.depth` deep (circles turned to face z), polygons convex slabs, edges static meshes; `Rigidbody2D` keeps to the plane (constraints), speaks Vector2 and degrees, reads spin from the engine's angular velocity; `Physics2D.gravity`. |
| M5-CUR-01 | JS unit | Game mouse input: `mousePosition`, mouse buttons (`getMouseButton*`, Fire1), raw `mouseDelta` adding up within a frame (y up), `getAxis("Mouse X" / "Mouse Y" / "Mouse ScrollWheel")`; `Cursor.visible` / `lockState` / `setCursor` become `cursor.*` ops. Showing, locking and images need the window: a manual check. |
| M5-APP-01 | Unit | The game's manifest comes from its `package.json` (`name`, `version`, `author`, and `platypus.title` / `company` / `identifier` / `icon`), with the folder name and defaults when missing; `__platypus_info` carries the app and system sections and later GPU / screen additions. |
| M5-APP-02 | JS unit | `Application` and `SystemInfo` read `__platypus_info` (defaults before it exists); `Application.quit()` is an op. |
| M5-APP-04 | Unit | The log goes to `Player.log` in the game's settings folder (or `PLATYPUS_LOG_FILE`); the last run's is kept as `Player-prev.log`. |
| M5-APP-11 | Unit + JS unit | Arguments after `--` on `platypus-app`'s command line go to scripts as `Application.args` (Platypus's own options before it still apply); a copy each read. |
| M5-APP-12 | JS unit + Manual | `Application.quit(code)` sends `app.quit` with a whole exit code (0 by default) and `platypus-app` exits with it (checked by hand: the md-reader's `--root` exits with 2). |
| M5-APP-03 | Unit | `platypus-app` without a game argument runs the current folder; the game folder may come before or after the options, and a second folder or an unknown option is an error. |
| M5-APP-05 | Unit | winit's `Focused` and `Occluded` window events become `InputEvent::Focused` / `Occluded`. |
| M5-APP-06 | Integration | Muting the mixer for the background silences everything and fades back to the scripts' master volume, including a master set while muted. |
| M5-APP-07 | Unit | Losing focus or minimising (0 × 0 or occluded) pauses unless `runInBackground` (or a screenshot run); pause and focus changes are reported once each, and again to reloaded scripts; the fixed clock caps catch-up at five ticks and restarts after a pause with a single tick. |
| M5-APP-08 | Unit | The `targetFrameRate` cap is a frame interval only with vsync off, a positive rate and no screenshot run. |
| M5-APP-09 | Unit | `platypus.runInBackground` comes from the manifest (off by default); `runInBackground` and `targetFrameRate` set by scripts go into `__platypus_info` for reloads. |
| M5-APP-10 | JS unit | `Application.runInBackground` reads the engine's value and sends `app.runInBackground`; `__platypus_focus` updates `isFocused` / `isPaused`, calls `onFocusChanged` listeners (until unsubscribed, a throwing one doesn't stop the rest) and `onApplicationPause` then `onApplicationFocus` on components of active objects only. |
| M5-GLB-01 | Unit | glTF loading (`tests/fixtures/assets/house.glb`, made by `make_house.py`): each primitive becomes an engine mesh (normals made up when missing), embedded images become textures with their sampler's filter, the description has nodes (names, TRS, children, primitives), roots and materials (colour, roughness to smoothness, unlit, texture); buffers and images outside the game folder are refused; mesh colliders resolve a render mesh (engine ids past f32 precision too), scaled. |
| M5-FBX-01 | Unit | FBX loading (`tests/fixtures/assets/cube.fbx`, ASCII, made by `make_fbx.py`): a Z-up centimetre file comes in as metres, Y-up: the cube one metre up and one metre across, its empty child half a metre above it in the world, one root (`crates/platypus_app/src/models_fbx.rs`). |
| M5-FBX-02 | Unit | FBX loading: faces split into one engine mesh per material (corners shared only where position, normal and UV are), material colours, a texture named by the file loaded once from beside it. |
| M5-FBX-03 | Unit | FBX loading: a file that isn't FBX is an error; paths outside the game folder are refused. |
| M5-FBX-04 | Unit | FBX skins (`rig.fbx`): one skin with the bones Root and Tip, inverse bind matrices from the clusters (Tip's undoes its metre), the mesh node pointing at it, four weights per vertex summing to 1, the upper half following Tip. |
| M5-FBX-05 | Unit | FBX takes: the take "Bend" is baked into keys, Tip's rotation ending at 90 degrees about X after one second. |
| M5-FBX-06 | Unit | An FBX's `.meta` importer `scaleFactor` scales it (2: the cube is 2 m across and its node twice as high); a missing or non-positive factor is 1 (`models_fbx.rs`). |
| M5-GLB-02 | JS unit | `Model.load` loads once per path; `instantiate` builds the node tree as GameObjects (one child per part when a node has several), with materials and textures from the model, mesh colliders on request (`renderMesh` shapes), even when placed before loading finished; failures reject `whenLoaded` and place nothing. |
| M5-RND-10 | Golden | Directional shadows: a soft-shadowing sun over a cube, sphere, capsule and a floating plank whose shadow lands on the sphere and the floor (shadow map fitted to the camera, normal offset and slope bias, 3 × 3 PCF). |
| M5-RB-04 | JS unit | Colliders resize when their object or a parent is scaled, and when reparented (the old shape is removed); setting the same scale rebuilds nothing. |
| M5-LIN-01 | JS unit | `LineRenderer`: a tube has rings at its radius around a Catmull-Rom smoothed path through every point; it redraws only when its points change. |
| M5-RND-11 | Golden | PBR under a procedural sky: spheres sweeping smoothness (columns) and metallic (rows) reflect the sky's cubemap at roughness-matched mips, a smooth dark floor reflects it, and the camera shows the sky behind everything (skybox clear). |
| M5-RND-12 | Golden | A planar mirror: a tinted metallic quad reflects the scene from the reflected camera (projection flipped to keep winding, a clip plane dropping what's behind the mirror: the yellow cube there never shows), sampled in screen space. |
| M5-RND-13 | Golden | A box-projected reflection probe in a room: a chrome ball reflects the red, green and blue walls, the ceiling, the open side and the gold ball (whose rougher reflection is blurrier), from the probe's six-face cubemap. |
| M5-RND-14 | Golden | Quality low and high side by side: half render scale, no MSAA and a small hard shadow map against 4x MSAA, full scale and a big soft (5 x 5) shadow map, on thin leaning bars and their shadows. |
| M5-RND-15 | Golden | Culling masks: a sun that lights layers 0 and 1 (casting their shadows), a red lamp that lights only layer 2, and a mirror that leaves layer 1 out of its reflection. |
| M5-RND-17 | Golden | Material maps and modes under a sky: a normal-mapped cube (round bumps lit from the side), an occlusion-striped sphere, a specular-workflow sphere (green reflectance), an emission-mapped cube with a detail checker, a double-sided cut-out leaf whose shadow has its holes, and a faded sphere next to a transparent one in front of a red bar. |
| M5-RND-18 | Golden + JS unit | Post-processing: the same bright scene clipped, with neutral tonemapping, with ACES a stop darker, and with ACES plus FXAA (which changes only pixels near edges, the thin bar's among them); `RenderSettings.tonemapping` / `postExposure` send `render.post`, `Camera.antialiasing` goes out with the camera op. |
| M5-RND-19 | Golden | A second camera draws the scene into a render texture (yellow background, from behind) before the screen, and a monitor quad in the main view shows it. |
| M5-RTX-01 | JS unit | `RenderTexture` sends its size (`render.renderTexture`) and `setSize` resends; `Camera.targetTexture` goes out with the camera op (null clears it); a texture camera isn't `Camera.main`. |
| M5-RND-20 | Golden + Integration | Shadows: posts receding into the distance under four sun cascades, a cube under a spot light, a ring of pillars round a point light (six faces); 4 + 1 + 6 maps drawn once a frame, and a reflection probe refreshed every frame adds none. |
| M5-SHD-01 | Unit | Shadow planning: cascades grow with distance and each covers its slice; spot (1 layer) and point (6) lights nearest the camera share a 24-layer budget; a point light's +x face sees what's to its +x. |
| M5-SHD-02 | JS unit | `QualitySettings.shadowCascades` rounds to 1, 2 or 4 and goes out with `render.shadowSettings`; levels set it (Low 1, Medium 2, High and Ultra 4). |
| M5-RND-16 | Golden | Screen UI with `backdrop-filter: blur()` over the 3D world blurs the world behind it (sharp stripes outside the frosted panel, smooth ones inside), drawn the app's way: the world into the backdrop texture, the UI over a copy of it. |
| M5-CUL-01 | Unit | Frustums from perspective and orthographic view-projections keep boxes in view (and big ones reaching in) and drop ones behind, beside or past the far plane; lights' spheres count while their range reaches in; rotated boxes grow to hold their corners. |
| M5-CUL-02 | Integration | A camera looking down skips a mesh far to the side, a quad behind it and a point light out of reach (`stats()` counts them), and doesn't draw a plank above it whose shadow still lands on the floor. |
| M5-CUL-03 | Unit + Integration | `WorldRenderer::seen` (animation culling): meshes with a `visibility_key` are reported when in a view or when a shadow caster in a shadow map whose shadow can fall in view (the plank above the camera; not when it casts nothing, nor a figure behind a camera looking along the ground under a high sun, but yes under a low sun behind the camera); untracked meshes are not reported; a skinned box holds every bone's image of the bind-pose box; shadow volumes reach back past their near plane. |
| M5-CUL-04 | Unit | `Renderer.isVisible` (`platypus_render_api` `visibility.rs`, the app's `anim_cull.rs`): a renderer is watched from the first time a script asks (false until a frame has been drawn watching it), with no animator culling at all; mesh and skinned renderers are numbered apart (skinned from `SKIN_BASE`); the host's report after each frame says which were seen. |
| M5-CUL-07 | Unit + Integration | Bone matrices: the app's cache (`bone_cache.rs`) keeps a skeleton's run while none of its bones moved (the list kept in place) and works out exactly the moved ones' again (also when asked before `update_world`), the parts of one figure sharing a run; a removed skin lays the list out again. The renderer sends only the drawn figures' runs (in a view or a shadow map; `WorldStats::bones`), once for the shadow pass and every view sharing the list, and two views over one list draw the same frame as one. |
| M5-CUL-06 | Integration | A camera drawing into a render texture (how a secondary window shows the world: windows draw no world of their own) counts for `seen()`: a figure the screen's camera looks away from is reported once a render-texture camera looks at it. |
| M5-CUL-05 | Integration | Skinned meshes are frustum-culled by their bind-pose box under every bone (one under the camera drawn, one beside it and one far away culled, counted in `stats()`), casters outside every shadow layer are left out of the shadow maps (`WorldStats::shadow_casters`: the far one), and `seen()` uses the same boxes. |
| ~~M5-LAY-01~~ | Integration | `scene.layer`: objects start on layer 0, keep the layer set, refuse ones outside 0 to 31; unknown objects read as 0. (removed 2026-10-28: QuickJS removed) |
| M5-LAY-02 | Unit | Cameras draw only the layers in their culling mask, and a layer's things only out to its cull distance, measured along the view or (spherical) straight from the camera. |
| M5-LAY-03 | JS unit | `LayerMask` names (built-ins and the manifest's, bad entries skipped), `getMask` / `contains`; `GameObject.layer` sends and range-checks; `Camera.cullingMask` (signed masks made unsigned), `layerCullDistances` (copies) and `layerCullSpherical` go out with `render.camera`; `fieldOfView` is clamped to 1 to 179 and converts to and from `horizontalFieldOfView`. |
| M5-LAY-04 | JS unit | `Light.cullingMask`, `ReflectionProbe.cullingMask` and `Mirror.cullingMask` go out with their ops, unsigned (all layers by default). |
| M5-LOD-01 | Unit | A LOD group shows its detailed renderer up close, the simple one farther off and nothing when tiny; `lodBias` 2 keeps detail twice as far; a forced level wins at any distance. |
| M5-LOD-02 | JS unit | `LODGroup` sends transition heights and renderer ids per level, hands out copies, forces levels and refuses heights that go up. |
| M5-DSP-01 | Unit | Window modes this platform lacks fall back to the nearest it has (no exclusive fullscreen on Wayland, only fullscreen on phones); mode names round-trip. |
| M5-DSP-02 | Unit | Saved display settings override the manifest's `platypus.display`; bad values are ignored; saving and reading back is exact. |
| M5-DSP-03 | JS unit | `Screen` reads the engine's display state (mode, window size, supported modes, monitor resolutions) and sends `screen.set`; `Application.targetFrameRate` and `QualitySettings.vSyncCount` go to the engine; quality choices are saved to prefs and come back on the next launch without being saved again. |
| M5-DSP-04 | Unit + JS unit | Exclusive fullscreen without a size takes the monitor's current resolution and rate (not the smallest mode), an explicit size is kept, and leaving fullscreen goes back to the remembered window size; `Screen.fullScreenMode` / `fullScreen` ask for a mode without a size. |
| M5-DSP-05 | Unit | `display::centered_position`: the main window opens centred on the primary monitor when windowed (outer size, so the frame counts), never above or left of the monitor's origin when it is bigger; fullscreen, maximized and screenshot runs are left alone (`platypus_platform`). |
| M5-PRF-01 | Unit | The prefs file saves and loads back (making its folders), skips unchanged writes, and a broken file starts afresh. |
| M5-PRF-02 | JS unit | `PlayerPrefs` reads what the engine loaded (fallbacks for missing keys and wrong types, ints truncated) and sends set / delete / deleteAll / save. |
| M5-QUA-02 | JS unit | Texture quality: `QualitySettings.anisotropicFiltering` and `globalTextureMipmapLimit` (clamped 0 to 3) go out with the quality op and come with each level. |
| M5-QUA-01 | JS unit | `QualitySettings`: levels (Low / Medium / High / Ultra) send the quality and shadow ops together; single settings change on their own (anti-aliasing rounded to 1 / 2 / 4 / 8, render scale clamped); unknown levels throw; `lodBias` goes out with the quality op; the manifest picks the default level and overrides levels. |
| M5-MSH-02 | JS unit | `MeshRenderer` sends its mesh (primitive or script `Mesh`) and material and resends on change; `Mesh.create` sends flat arrays and `destroy` removes it; `Light` and `RenderSettings.ambientLight` ops; `shadowCastingMode` reaches the op; `Mirror` (quads along +z) and `ReflectionProbe` (size, resolution, box projection, `renderProbe`) ops; `createPrimitive` adds the mesh and the matching collider. |
| M5-RND-07 | Golden | Lit primitives: a plane, cube, sphere and capsule under a directional and a point light, an unlit emissive cube and a see-through cylinder. |
| M5-SCN-01 | JS unit | Scene save → load round-trips objects, hierarchy, layers, enabled flags, engine and script components, `@property` values, references, shared meshes and textures and render settings exactly (saving again gives the same data); properties are set before `onAwake`; unknown components and values are skipped with warnings. |
| ~~M5-SCN-02~~ | Integration | In QuickJS, a component saves a scene (with `@property` fields and an engine `Light`), loads it back as the single scene and keeps itself with `dontDestroyOnLoad`; Rust's scene ends up with the loaded objects where they were saved. (removed 2026-10-28: QuickJS removed) |
| M5-SCN-03 | JS unit | `SceneManager`: the `Main` scene, additive loads, the active scene taking new objects, moving roots between scenes, unloading (not the last), events in order; single loads replacing all but `DontDestroyOnLoad`; `loadSceneAsync` from a file, failures rejecting; bad data refused. |
| M5-SCN-04 | JS unit | `GameObject.instantiate` copies objects with children and components; references inside the copy point at the copies, outside ones stay; world placement without a parent, local under one. |
| M5-SCN-05 | — | Asset references survive moving/renaming the file (GUID in `.meta`): covered by `M5-AST-05`. |
| M5-SCN-06 | JS unit | `HideFlags`: objects flagged `DontSave` (and their children) stay out of saved scenes; a camera on a `HideInHierarchy` object is never `Camera.main`. |
| M5-SCN-10 | Unit | Components of kinds the program doesn't register (a game's scripts, in the editor) are kept as data and saved back in their place among the object's components, with their other keys; references to and from them get the right file ids after renumbering; a second round trip gives the same data; copies carry them (`crates/platypus_scenes/src/data_tests.rs`). |
| M5-AST-07 | Integration (planned) | Cooking is deterministic: the same input gives byte-identical output. |
| M5-AST-08 | Integration (planned) | Cooking per platform: BC7 / BC5 for desktop, ASTC for mobile. |
| M5-RND-01 | Golden | A PBR sphere lit by a directional light. |
| M5-RND-02 | Golden | A cube casting a shadow onto a plane. |
| M5-RND-03 | Golden | IBL from a fixture HDRI on metallic and rough spheres. |
| M5-RND-04 | Benchmark | Sponza (or similar) scene frame time tracked over time. |

---

## M4 — Physics and audio

| ID | Kind | Checks |
|---|---|---|
| M4-PHY-01 | Integration | A dropped box lands on a plane and comes to rest at the right height. |
| ~~M4-PHY-02~~ | Integration | Collision enter/exit events reach TS components. (removed 2026-10-28: QuickJS removed) |
| M4-PHY-03 | Integration | Character controller walks up a step and stops at a wall. |
| M4-PHY-04 | Integration | Physics bodies survive a script hot reload. |
| M4-PHY-05 | Integration | Raycasts queued in a tick are answered after the step with the collider, distance and normal; misses report `-1` in the flat batch. |
| M4-PHY-06 | Integration | Bad physics ops (unknown kinds and shapes, missing bodies, zero directions, NaN) are rejected without changing the world; a triangle-mesh collider catches a ball. |
| M4-PHY-07 | Integration | A box kept in a plane (`constraints`: no z movement, turning only about z) stops under friction like a free one when shoved, stays in its plane when pushed along z, and is held where `position` puts it. |
| M4-PHY-07 | Integration | A shoved box stops from friction when its axes are locked to a plane (z movement and x/y turns locked); ignored until the rapier friction fix in TODO.md lands. |
| M4-PHY-08 | Integration | Contact points of a pair are read from either side after a step: a box resting on the floor touches it at the floor's top, normals point towards the side asking, normal impulses hold up its weight, friction pushes a shoved box back; a collision's start carries the relative velocity going in. |
| M4-AUD-01 | Integration | Playing a WAV (decoded and streamed) through `kira` with an offline backend produces non-silent output, and both are reported ended after they play out; undecodable files fail to load. |
| M4-AUD-02 | Integration | An emitter 1 m to the right pans right, fades at 10 m, is silent past its max distance, and pans left when moved to the left. |
| M4-AUD-03 | Integration | Volume and hard panning set the channel levels; a bus at ¼ volume scales what plays on it; muting the master silences everything; a stopped sound is reported ended once it has faded. |
| M4-AUD-04 | Unit | Gains convert to decibels (0 → silence); bad audio ops (unknown sounds, buses and emitters, NaN, unknown names) are rejected. |

---

## M5.e — Editor shell

| ID | Kind | Checks |
|---|---|---|
| M5-EDT-01 | Integration | Hierarchy panel shows the scene tree and updates when entities are added or removed. |
| M5-EDT-02 | Integration | Inspector shows `@property` fields; editing one changes the live component. |
| M5-EDT-03 | Integration | Undo/redo reverses and reapplies property edits and entity creation. |
| M5-EDT-04 | Benchmark | A 10,000-item virtualized list scrolls with layout + draw under **2 ms** per frame. |
| M5-EDT-05 | Manual | Dock, undock and resize panels; layout persists across restarts. |
| M5-EDT-06 | Integration | Virtual list: the mounted range is the rows in view plus overscan (a million rows mount a handful); scroll-to-index moves only as far as needed or aligns start / center / end, clamped. (`js/editor-ui/test/models.test.ts`) (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-07 | Integration | Virtual list keys: arrows, Home / End and Page Up / Down move the selection within bounds; other keys are ignored. (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-08 | Integration | Tree view: flattening walks only expanded nodes (20,000 levels deep without overflowing); click selects, Ctrl toggles, Shift ranges from the anchor, Ctrl+Shift adds a range. (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-09 | Integration | Tree view keys: Up / Down / Home / End / Page keys move, Right expands then enters, Left collapses then goes to the parent, Shift extends, Ctrl moves focus only, Ctrl+Space toggles; revealing a node expands its ancestors. (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-10 | Integration | Docking: a tab dropped on a group's middle joins it; on an edge it splits the group (merging into a parent split the same way); emptied groups and single-child splits collapse; tabs reorder, activate and close; splitting a group by its only tab does nothing. (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-11 | Integration | Docking geometry: splitters resize neighbours within the minimum; computed rects match the flex layout; drops find the group and zone (tab strip and middle: center; outer quarter: that side). (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-12 | Integration | Dock layouts save to JSON and load back equal; unknown tabs (and their empty groups) are dropped; malformed JSON gives null. (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-13 | Integration | Menubar accelerators parse (`Ctrl+Shift+S`, `F5`, `Ctrl++`, `Del`), show normalized and match key presses by code and modifiers, searching submenus. (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-14 | Integration | Menubar keys skip separators and disabled items, Right / Left open and close submenus or move between menus, Enter chooses, Escape closes a level at a time. The React `MenuBar` component test (`menubar.test.tsx`, through QuickJS) went with `js/`: the component is checked in the window (M5-EDT-21). (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| ~~M5-EDT-15~~ | Unit | The log capture layer keeps records (level, target, message plus fields) in order in a ring buffer read by sequence number (`logging.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M5-EDT-16~~ | Unit | `editor.openFile`: VS Code's `code -g file:line:col` by default when on PATH, else nothing to run; a `PLATYPUS_EDITOR_OPEN` template is split with quotes and fills `{file}` / `{line}` / `{col}`; relative paths resolve against the first root that has them (`editor.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M5-EDT-17~~ | Unit | `editor.*` ops are refused outside `platypus_editor`. (removed 2026-10-28: QuickJS removed) |
| M5-EDT-18 | Integration | Console code links: `path/file.ext:line[:col]`, Windows drives and backslashes, absolute Unix paths; URLs with ports and times aren't links. (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-19 | Integration | Console: identical entries collapse with counts at the first one's place; level counts (debug as info); filters by level and by search over message and target. (native: `editor/test/models.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-20 | Integration | The log feed adds `Editor.logSince` records once, in order (records seen already are skipped), tells its listeners, and clearing empties it. (native: `editor/test/log.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-DND-01 | Integration | A press on a `draggable` element moved past 4 px drags it: `dragstart`, `drag`, `dragenter` / `dragleave` as the element under the pointer changes, `dragover`; no pointer events meanwhile; a prevented `dragover` accepts, so the release sends `drop` then `dragend` (detail 1), and no click. (`crates/web_ui_dom/tests/dnd.rs`) |
| M5-DND-02 | Integration | A drop nobody accepted ends with `dragleave` and `dragend` (detail 0); Escape cancels the same way; preventing `dragstart` leaves a normal move; a short press is still a click; elements without `draggable` don't drag. |
| M5-DND-03 | Integration | The drag image is the dragged element moved by the pointer's travel (drawn last, translucent, in a transform), or another element grabbed at a point (`SetDragImage`); it goes when the drag ends. |
| M5-DND-04 | Integration | Files from the system: `FileDragEnter` sends `dragenter` / `dragover` with the paths, moves change the target, `FileDrop` sends `drop` with the paths as the event's strings, `FileDragCancel` a `dragleave`. |
| M5-DND-05 | Unit | The app gathers winit's per-file hover / drop / cancel events into one `FileDragEnter`, moves only when the cursor moved, one `FileDrop` or `FileDragCancel` (`file_drop.rs`). |
| M5-DND-06 | JS unit | `PlatypusDragEvent`: one `dataTransfer` from `dragstart` to `drop` and `dragend` (`setData` / `getData` / `types`, `dropEffect`, `dropped`); `setDragImage` is a `dragImage` op; `draggable` is an attribute; system files arrive as `dataTransfer.files` with names and `"Files"` in `types`. |
| ~~M5-DND-07~~ | Unit | Project file ops: paths can't leave the project (`..`, absolute, drives); listings put folders first and skip dot files; copies come in renamed instead of overwriting, folders whole; moves refuse overwriting, moving into itself or the root (`project.rs`). (removed 2026-10-28: QuickJS removed) |
| M5-DND-08 | Integration | Project paths (join, parent, breadcrumbs, `canMoveInto`, sizes); without a project the file calls fail and `Editor.lastError` says why. (native: `editor/test/project.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| ~~M5-DND-09~~ | Integration | End to end through QuickJS and React (`tests/fixtures/dnd.js`): dragging the item onto the folder drops its data and `dragend` says dropped; system files dropped on the folder reach its `onDrop`. (removed 2026-10-28: QuickJS removed) |
| ~~M5-DND-10~~ | JS component | The project browser lists the folder from `editor.listDir`; files dropped on a folder row call `editor.copyIn` into it; a file dragged onto a folder calls `editor.move`, and onto itself isn't accepted. (removed 2026-10-29 with `js/`: a QuickJS React test; the file calls are M5-EDT-27 and M5-DND-12, the drops M5-DND-11) |
| M5-DND-11 | Manual | In the window (`platypus_editor <project>`): drag a file from Explorer / Finder onto the Project panel and onto a folder in it (copied there); drag an item onto a folder or breadcrumb (moved); the drag image follows the pointer and the cursor shows whether it can drop. |
| M5-EDT-21 | Manual | `platypus_editor --screenshot out.png` (or `bun run screenshots editor`): menubar, Hierarchy tree with icons and chevrons, Scene / Inspector placeholders, Console with coloured levels, counts and underlined code links, status bar. Checked by eye, also with a menu and submenu open and with tabs docked elsewhere. |
| M5-EDT-22 | Integration | Scene view camera (`scene-camera.ts`): yaw / pitch agree with the rotation; look keeps the eye, orbit the pivot; pan moves a view height per view height dragged; zoom scales and clamps the distance; fly speeds up with distance and Shift; F frames a sphere; the axis gizmo snaps to an axis and lists the axes farthest first. (native: `editor/test/scene-camera.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-23 | Manual | `platypus_editor --screenshot`: the Scene panel shows the demo scene through the see-through panel with the ground grid, SceneIcons (sun, bulb, spot, camera, speaker, probe) at their objects and the orientation gizmo in the corner; the Hierarchy lists the live objects. In the window: right-drag looks (WASD / QE fly, Shift faster), middle-drag pans, the wheel zooms, Alt+left-drag orbits, F frames the selection, clicking an icon selects its object (and the Hierarchy row) and shows its wire gizmos (frustum, light range / cone / rays, probe box, audio distances), clicking a gizmo axis looks along it and the centre toggles Persp / Iso; Q / W / E / R (or the toolbar) pick view / move / rotate / scale handles on the last selected object, which light up on hover and drag it; the Game tab shows the main camera. |
| M5-EDT-24 | Integration | Transform handles (`handles.ts`): axes a constant length on screen; the pointer picks the centre or the nearest axis or ring (front arcs only); moving follows the pointer along local axes (Ctrl snaps to 1 m) or in the view plane; rings turn about their axis (Ctrl snaps to 15°); scale by the ratio along an axis, or uniformly from the centre, never below 0.01. (native: `editor/test/handles.test.ts`, run by `tools/platypus_ts_tests/tests/ui/editor_models.rs` in every GC mode) |
| M5-EDT-25 | Unit | The editor's log capture (`platypus_app` `logging.rs` `CaptureLayer` into `platypus_engine_api::editor`): records keep level, target and message plus fields in order (replaces M5-EDT-15). |
| M5-EDT-26 | Unit | `Editor.openFile`: VS Code's `code -g file:line:col` by default when on PATH, else nothing to run; a `PLATYPUS_EDITOR_OPEN` template is split with quotes and fills `{file}` / `{line}` / `{col}`; relative paths resolve against the first root that has them (`crates/platypus_engine_api/src/editor/open.rs`; replaces M5-EDT-16). |
| M5-EDT-27 | Unit | The `Editor` namespace works only with a project (`platypus-app editor --project <folder>`): without one the log keeps nothing and file calls fail with `lastError`; with one, `logSince` gives records after a sequence number, `listDir` lists, and `copyIn` refuses paths not dropped on the window (`editor.rs`; replaces M5-EDT-17). |
| M5-EDT-28 | Integration | The native editor builds (`platypus_build::build` of `editor/`) and a windowless `platypus-app editor --project examples/arcade --bench` run of 430 frames, clicking Hierarchy rows, the dock's tabs (Project and back to Console), Clear, Collapse and the Scene view, ends cleanly with no crash, panic or error in the log and every click finding its text (`tools/platypus_build/tests/editor_smoke.rs`; needs a built host, `PLATYPUS_HOST` or the newest `build/{release,debug}/platypus-app`, and a GPU; about 4 s release, 6 s debug). |
| M5-EDT-29 | Integration | The Scene view keeps its own camera after the Game tab and back: a `--bench-shots` run of the editor (Scene, Game, Scene) whose Scene view pixels match before and after (grid and icons, not the game camera's view) (`tools/platypus_build/tests/editor_smoke.rs`; needs a built host and a GPU). |
| M5-EDT-30 | Integration | Selecting Cube in the Hierarchy reaches the Scene view and the Inspector: a `--bench-shots` run of the editor finds the Inspector's "1 selected: Cube" and the Scene view's pixels change round the cube (its move handles; Cube has no icon or wire gizmo) (`tools/platypus_build/tests/editor_smoke.rs`; needs a built host and a GPU). |
| M5-DND-12 | Unit | Project file ops: paths can't leave the project (`..`, absolute, drives); listings put folders first and skip dot files; copies come in renamed instead of overwriting, folders whole; moves refuse overwriting, moving into itself or the root and take an asset's `.meta` along; listings leave `.meta` files out (`crates/platypus_engine_api/src/editor/project.rs`; replaces M5-DND-07). |
| M5-EFS-02 | Unit | `Editor.writeText` makes the folders, replaces the old file whole (no temporary file left), refuses folders and paths outside the project; `readText` reads it back and refuses folders and missing files (`crates/platypus_engine_api/src/editor/files.rs`). |
| M5-EFS-03 | Unit | `exists` / `stat` (folder, size, modification time) of project paths; `createDir` makes nested folders and refuses existing ones; `freeName` numbers a taken name (`files.rs`). |
| M5-EFS-04 | Unit | `rename` stays in the item's folder, takes its `.meta` along, refuses taken and unusable names (separators, `..`, spaces round it) and the root; a change of case only is allowed (`files.rs`). |
| M5-EFS-05 | Unit | `trash` moves an item and its `.meta` into `.platypus/trash/<id>/` beside a note of where it was; `restore` puts it back once, and refuses when something new is in its place; the trash itself, the root and outside paths are refused (`files.rs`). |
| M5-UNDO-01 | Unit | `EditorScene.snapshot` of an object with children and components, destroyed, then `restore`d at its old sibling place (or under a parent) builds it again the same; bad text makes nothing (`crates/platypus_scenes/src/editor_tests.rs`). |
| M5-UNDO-02 | Integration | Undo history (`editor/src/history.ts`): commands run, undo and redo in order with their labels; a new command drops what was undone (`editor/test/history.test.ts`). |
| M5-UNDO-03 | Integration | Undo history: commands with the same merge key fold into one entry (a drag); `record` keeps a command without running it. |
| M5-UNDO-04 | Integration | Undo history: the saved position (dirty flag) follows undo, redo and merging; the size cap drops the oldest entries and, past the saved one, leaves the history dirty for good; clearing. |
| M5-UNDO-05 | Integration | Undo history: a group runs in order and undoes in reverse. |
| M5-UNDO-06 | Integration | Editor smoke: Delete on Cube, Ctrl+Z, Ctrl+Y, Ctrl+Z, Ctrl+D (duplicate) and dragging Sphere onto Floor in the Hierarchy then Ctrl+Z, each reported in the status bar (`tools/platypus_build/tests/editor_smoke.rs`). |
| M5-UNDO-07 | Unit | Copies are named like Unity's (`duplicateName`): "Cube" -> "Cube (1)", then "Cube (2)", a copy of a copy takes the next free number, after the highest; other names and non-number brackets are left alone (`editor/test/models.test.ts`). |
| M5-UNDO-08 | Integration | Editor smoke: a CameraFollow's target picked in the object picker; deleting the target and undoing points the target at the rebuilt object ("Restored ..." in the log, and the saved scene keeps the reference); Ctrl+D on Sphere makes "Sphere (1)" (`editor_smoke.rs`). |
| M5-SCN-11 | Unit | `EditorScene.saveScene` writes the active scene to a project file (pretty JSON, the scene named after the file), `newScene` empties the world, `openScene` loads the file again with its path; a missing file or a path outside the project fails with `Editor.lastError` (`editor_tests.rs`). |
| M5-SCN-12 | Unit | `platypus_core::meta::ensure` gives an asset (a scene file) one `.meta` with a GUID and keeps it; a file that isn't an asset gets none; `.fbx` is a model (`crates/platypus_core/src/meta.rs`). |
| M5-SCN-13 | Integration | Editor smoke on a new project: File > New Scene, GameObject > 3D Object > Cube, Ctrl+S, Enter in the Save dialog writes `scenes/Untitled.scene.json` (with Cube, Main Camera, Directional Light, cameras saved with their own rect) and its `.meta`; a second run with the project's `startScene` opens it (`tools/platypus_build/tests/editor_smoke.rs`). |
| M5-PRJ-01 | Unit | `project_edit::set_key` replaces one top-level value in place: templates, unknown keys, key order and spacing stay (`crates/platypus_core/src/project_edit.rs`). |
| M5-PRJ-02 | Unit | A new key goes last with the file's indentation (or on the same line for a one-line object, or into an empty one); the result parses. |
| M5-PRJ-03 | Unit | `remove_key` takes the key with its comma and line (first, middle and last members); a missing key changes nothing; `get_key` gives the raw text; broken JSON and non-objects are refused. |
| M5-PRJ-04 | Unit | `Editor.projectFields` lists the settings as written (templates unexpanded, set or not, enum options); `setProjectField` writes only the changed keys (an empty choice removes the key) and refuses unknown settings, bad enum values, wrong types and an empty name; `removeProjectField` refuses `name` (`crates/platypus_engine_api/src/editor.rs`). |
| M5-PRJ-05 | Integration | Editor smoke: Edit > Project Settings, "7" typed into company: `project.json` gains `"company": "7"` last with everything else as written; the dialog's Undo puts the file back exactly (`tools/platypus_build/tests/editor_smoke.rs`). |
| M5-PRJ-06 | Unit | Dotted keys in `project_edit` (`display.width`, `layers.9`, `quality.default`): read, replaced in place, added inside the section in its own style or as a new section last, removed with an emptied section going too; refused inside a non-object. `Editor.setProjectField` takes numbers for `number` settings, refuses text there, and an empty layer name removes the key (`project_edit.rs`, `platypus_engine_api` `editor.rs`). |
| M5-PRJ-07 | Integration | Editor smoke: Project Settings scrolled to Display, Quality and Layers; width typed as 1600, Layer 8's name cleared and the default quality chosen in the picker: `project.json` changes in exactly those places (the emptied `layers` goes, `quality` is added last) with the rest as written (`editor_smoke.rs`). |
| M5-PRF-10 | Unit | `EditorScene.savePrefab` writes an object with its children and components as a `.prefab.json` (placed at the origin, assets in the file's table); `placePrefab` places copies at the root or under a parent, which remember their file (`prefab` in saved scenes); a prefab's `assets` table reaches its copies' loader; a missing file places nothing (`crates/platypus_scenes/src/editor_tests.rs`). |
| M5-PRF-11 | Integration | Editor smoke: Cube dragged from the Hierarchy onto the Project list is saved as `assets/Cube.prefab.json` (with its `.meta`); the file dragged onto the Hierarchy is placed; Ctrl+Z takes it away (`tools/platypus_build/tests/editor_smoke.rs`). |
| M5-DND-20 | Integration | Editor smoke: `assets/house.glb` dragged from the Project browser onto the Scene view is placed (status "Place house.glb") (`editor_smoke.rs`). |
| M5-DND-21 | Integration | Placement model: a drop meets the ground under the pointer, else lands a set distance along the ray; files are told apart by name (models incl. `.fbx`, prefabs, scenes); system models are imported into `assets/models` (`editor/test/placement.test.ts`). |
| M5-DND-22 | Integration | Editor smoke: `assets/house.glb` dragged from the Project browser over the Scene view shows a preview there (the view's pixels change); moved off the view and released over the Project list, the preview goes; dragged and dropped on the view, it is placed once (`editor_smoke.rs`). |
| M5-EDT-35 | Unit | `EditorScene.components` lists an object's registered and kept (opaque) components; `fields` gives typed fields with enum options and colours from the kind's schema (`platypus_kinds::register_fields`, written by `platypus_render_api/build.rs`); `setField` sets numbers, colours from a plain array, an old field's JSON (undo) and a kept component's data (saved so); `addComponent` / `removeComponent` / `componentKinds` (`crates/platypus_scenes/src/editor_tests.rs`). |
| M5-EDT-36 | Integration | Editor smoke: the Inspector of Point Light takes a typed intensity (status "Set Light.intensity", the field's pixels change), Ctrl+Z undoes it, Add Component > AudioSource on Cube (`tools/platypus_build/tests/editor_smoke.rs`). |
| M5-EDT-37 | Integration | Inspector model: numbers parsed and shown short, field names as labels, one vector part changed, a component found again by kind and place after a rebuild (`editor/test/inspector.test.ts`). |
| M5-EDT-41 | Integration | Editor smoke: the Console tab's close box closes it (the bottom panel's pixels change), View > Panels > Console opens it again, Edit > Preferences and Help > About open and close with Escape (`tools/platypus_build/tests/editor_smoke.rs`). |
| M5-EDT-42 | Integration | Dock model: `hasTab`; a closed panel opens again in the first group and is active; an open one is made active where it is (`editor/test/models.test.ts`). |
| M5-EDT-43 | Integration | Editor smoke: a dialog dims the Hierarchy behind it; Escape closes it and F2 on the Project list works again (focus given back); in New Folder, Tab three times wraps from the name past both buttons back to the name, where Enter makes the folder (`editor_smoke.rs`). |
| M5-EDT-44 | Unit | Menu keys in text fields (`menu.ts`): a field keeps typing, Delete, Backspace, Escape, Enter, arrows and Ctrl+A / C / V / X; Ctrl+Z / Y / S / N / O / D, Ctrl+Shift+S, F1 and Alt+F4 go to the menus (`editor/test/models.test.ts`). |
| M5-EDT-48 | Unit | Inspector references: an empty CameraFollow `target` is an editable `object` field (`refKind` GameObject) by its schema, a placeholder set through `setField` shows the target's name; Camera's `targetTexture` is an `asset` field (`Texture`) and `clearFlags` an enum from the hand-written schema; `assetFromFile` of an unknown kind is ""; AudioSource's `clip` schema is a `sound` (`crates/platypus_scenes/src/editor_tests.rs`); `Editor.layerNames()` puts the project's `layers` over the built-ins, 32 entries (`platypus_engine_api` `editor.rs`). |
| M5-EDT-49 | Unit | Inspector logic for several objects and references (`inspector.ts`): shared components by kind and place, mixed values and vector parts, the edited parts of a vector applied to each object's own; reference / sound JSON, asset files by kind, hex colours, HSV and the palette (`editor/test/inspector.test.ts`). |
| M5-EDT-50 | Integration | Editor smoke: two objects selected show their shared components; Position Y typed sets both ("Set Position of 2 objects"), a layer chosen in the picker sets both and Edit > Undo undoes it as one step; Point Light's swatch opens the colour picker and Enter sets the colour (`editor_smoke.rs`). |
| M5-EDT-46 | Unit | Tree drops (`tree.ts` `dropPlace`): before / after a row lands among its siblings at its index or the next, after an expanded parent goes first under it, on a row goes last under it, with the depth of the insertion line (`editor/test/models.test.ts`). |
| M5-EDT-47 | Integration | Editor smoke: Sphere dragged to the bottom quarter of Floor's row shows an insertion line under Floor; the drop makes it the root after Floor ("Move Sphere") and Ctrl+Z undoes it (`editor_smoke.rs`). |
| M5-EDT-45 | Integration | Editor smoke: with the Inspector's name field focused, Delete and Ctrl+A stay with the field (Cube isn't deleted), Ctrl+Z undoes the duplicate made before and Ctrl+S opens Save Scene As (`editor_smoke.rs`). |
| M5-EDT-31 | Integration | File dialog model: filters by extension (any case), folders always listed, only folders when picking one (`editor/test/file-dialog.test.ts`). |
| M5-EDT-32 | Integration | File dialog model: a saved name gets the filter's extension once (`level.json` → `level.scene.json`), trimmed; the stem for the name field. |
| M5-EDT-33 | Integration | File dialog model: typed paths, relative or from the root (`/`), with `.`, `..` and backslashes, become project paths; leaving the project or a drive gives none. |
| M5-EDT-34 | Integration | Editor smoke: File > Open Scene shows the file dialog (its filter), Escape closes it (`--bench-shots` pixels), the Project browser's New Folder asks for a name (`tools/platypus_build/tests/editor_smoke.rs`). |

## M9.b — AOT scripts (`tools/web_ui_ts` spike)

Run from `tools/`: `cargo test --workspace` (the build script compiles `sim.ts` with `web_ui_ts` and links the object into the test crate).

| ID | Kind | Description |
|---|---|---|
| M9-TS-01 | Integration | Port of `sim.test.ts` "a baked mask keeps the runs of walls and open floor" against the native code (`tools/platypus_ts_tests/tests/sim_port.rs`). |
| M9-TS-02 | Integration | Port: zombies spawn in single player. |
| M9-TS-03 | Integration | Port: a level's zombie budget grows by 3 to 5 each level (8..=10 at level 1). |
| M9-TS-04 | Integration | Port: a player moves 400 px a second with one key held. |
| M9-TS-05 | Integration | Port: a wall stops the player. |
| M9-TS-06 | Integration | Port: a press fires one pistol round and turns the aim toward the mouse. |
| M9-TS-07 | Integration | Port: holding fire with the pistol does not repeat. |
| M9-TS-08 | Integration | Port: an empty clip starts a reload that blocks shooting (and emits the `reload` event). |
| M9-TS-09 | Integration | Port: a hit takes health and scores 10; a kill scores 80 once the fade ends. |
| M9-TS-10 | Integration | Port: four zombie hits kill a player with 100 health, the score is floored at 0, game over clears the events. |
| M9-TS-11 | Integration | Port: the next level starts when the last zombie of this one dies. |
| M9-TS-12 | Integration | Port: touching an ammo powerup adds 320 rounds. |
| M9-TS-13 | Integration | Port: zombies speed up a little each level until level 3, then hold. |
| M9-TS-14 | Unit | `web_ui_ts` lowers a discriminated union of object types to one struct holding every variant's fields, with string-literal tags interned. |
| M9-TS-15 | Unit | Unsupported code is a compile error naming the line (`typeof`, `any`). |
| M9-TS-16 | Unit | Array-method callbacks are inlined (no closure call) and exported functions get `ts_` symbols in the object. |
| M9-TS-17 | Unit | `web_ui_rt`: arrays grow and keep values (17a), strings concat and compare (17b), `Uint8Array.fill` clamps (17c). |
| M9-TS-18 | Manual | `cargo run --release -p platypus_ts_tests --bin bench` (from `tools/`): QuickJS, bun and the native code run 10,000 steps of a busy state and print the same checksum; timings in `docs/research/platypus-ts-spike.md`. |
| M9-TS-19 | Manual | `cargo run --release -p platypus_ts_tests --bin sine_bench` (from `tools/`): the sine-circle curve (`bench/sine.ts`) in QuickJS with and without the `__platypus_op` crossing, bun and native code; one checksum for all.
| M9-TS-20 | Unit | `web_ui_rt` GC: unreachable objects are freed and their blocks handed out again. |
| M9-TS-21 | Unit | `web_ui_rt` GC: host roots and what they reach survive, also in stress mode; unrooted ones go. |
| M9-TS-22 | Unit | `web_ui_rt` GC: pointer arrays are traced and shadow-stack frame slots are roots; popping the frame frees what only it held. |
| M9-TS-23 | Integration | Collecting at every allocation (stress mode, freed memory poisoned) gives the same sim result as normal collection (`tools/platypus_ts_tests/tests/gc.rs`). |
| M9-TS-24 | Integration | A long run (8,000 steps) collects on its own, hands out far more than is alive, and live bytes and objects stay small afterwards. |
| M9-TS-25 | Integration | After 300 steps and a stress collection, the host-rooted state and what it reaches are still readable (no poison). |
| M9-TS-26 | Manual | `PLATYPUS_TS_GC=stress cargo test -p platypus_ts_tests --test engine sim_port`, and `PLATYPUS_TS_GC=stress cargo run --release -p platypus_ts_tests --bin bench 1500` / `--bin sine_bench 100`: the 13 ported tests pass and the checksums agree with QuickJS and bun with a collection at every allocation. |
| M9-TS-27 | Integration | A pointer read from a field and held as an argument while a later argument overwrites that field and allocates is not freed (`sum(h.item, replaceItem(h, 100))` = 105 as in JS; `bench/gap.ts`). Failed before the root analysis was tightened. |
| M9-TS-28 | Integration | A local reassigned while its old value waits as an argument is not freed (`sum(a, (a = fresh(100), fresh(1).v))` = 6 as in JS). Failed before the fix. |
| M9-TS-29 | Integration | A pointer waiting to be compared with `===` while the right side frees and reuses its object is not equal to the new object (`h.item === swapAndAlloc(h)` is false as in JS). Failed before comparisons rooted their operands. |
| M9-TS-30 | Integration | The zombie sim and `gap.ts` run interleaved in one heap with a collection at every allocation: their struct ids do not collide (`struct_base`). Crashed before. |
| M9-TS-31 | Unit | A collector does not mark objects of another thread's heap found through a shared global slot; the owner's next collection still traces them. Failed before headers carried a heap id. |
| M9-TS-32 | Unit | Array data counts toward the live size, so a heap with a large live array collects less often. |
| M9-TS-33 | Unit | Registering a different pointer map under a struct id already taken is refused; the same one again is fine. |
| M9-TS-34 | Integration | A closure and the function that made it share one variable (`counterDemo`, 33). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-35 | Integration | Closures from one function keep separate captured values (101011). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-36 | Integration | Two closures over one variable (`inc`, `get` fields of a counter object) see each other's writes; another counter is independent (7101). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-37 | Integration | A `for...of` variable is a new variable every iteration for closures (10409). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-38 | Integration | A `const` declared in a `while` body is new every iteration (211101). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-39 | Integration | Top-level functions work as values: passed, and as `xs.map(double)` next to an inlined arrow callback (42060608). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-40 | Integration | A closure can call itself through an annotated variable (720). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-41 | Integration | A closure over an object changes the object everyone holds (34). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-42 | Integration | A closure made in an inlined array callback captures the callback's parameter (332211). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-43 | Integration | An immediately called arrow works (42). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-44 | Integration | Closures held by the host keep their captured state, including an object, through collections and garbage churn, with a collection at every allocation. Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-45 | Integration | 20,000 short-lived arrays, cells and closures are collected. Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-46 | Integration | Closures are passed, returned and called through a call result while two fresh ones wait for each other (`compose(makeAdder(1), makeAdder(2))(10)` = 13). Runs normally, in stress GC mode and with collection off (`tests/closures.rs`). |
| M9-TS-47 | Integration | A module-level `let counter = 0` is changed by functions, an arrow function and a `forEach` callback, continues across calls and starts over after `ts_init` (19, 38, 0). Runs normally, in stress GC mode and with collection off (`tests/globals.rs`). |
| M9-TS-48 | Integration | A `let` global replaced by a call while its old value waits as an argument (`sum(current, swapCurrent(100))` = 101) is not freed. Fails if `let` globals are trusted like constants. Runs in all three GC modes (`tests/globals.rs`). |
| M9-TS-49 | Integration | A `let` global assigned inside a later argument, with an object literal and no call after it (`sum(current, (current = {v: 50}, {v: 7}.v))` = 8), keeps the old value alive. Runs in all three GC modes (`tests/globals.rs`). |
| M9-TS-50 | Integration | A global `Item | null` holds an object, then nothing (`remember`, `lastOr`, `forget`). Runs in all three GC modes (`tests/globals.rs`). |
| M9-TS-51 | Integration | A global `number | null` keeps the largest value seen (9). Runs in all three GC modes (`tests/globals.rs`). |
| M9-TS-52 | Integration | `let pending: Item | undefined;` and `let n: number | undefined;` without a value start as null and can be assigned. Runs in all three GC modes (`tests/globals.rs`). |
| M9-TS-53 | Integration | A closure stored in a global keeps what it captured (`handler = (n) => n * k + base`; 17), and the global's first value (`(n) => n`) is callable. Runs in all three GC modes (`tests/globals.rs`). |
| M9-TS-54 | Integration | A linked list reachable only from a module variable survives collections at every allocation (4950, then 4995). Runs in all three GC modes (`tests/globals.rs`). |
| M9-TS-55 | Unit | A `let` with a literal initialiser is a data slot written by the function, not folded away; a `const` literal is folded (`web_ui_ts/src/lib.rs`). |
| M9-TS-56 | Unit | Assigning a `const` (number or object) is an error naming the constant (`web_ui_ts/src/lib.rs`). |
| M9-TS-57 | Integration | `for (let i = 0; i < n; i++)` sums 0..n-1, and `continue` still runs the update clause (sum of odd numbers below 10 = 25). Runs normally, in stress GC mode and with collection off (`tests/loops.rs`). |
| M9-TS-58 | Integration | A `for` head with two declarators and a comma update, with no test, with no initialiser, or with no update (30, 8, 398). Runs in all three GC modes (`tests/loops.rs`). |
| M9-TS-59 | Integration | A `do...while` body runs once when the test starts false (111), and `continue` reaches its test (1206). Runs in all three GC modes (`tests/loops.rs`). |
| M9-TS-60 | Integration | `continue outer` and `break outer` reach the outer loop (15, 26). Runs in all three GC modes (`tests/loops.rs`). |
| M9-TS-61 | Integration | Labels on a `while`, a `for...of` and a plain block (600100, 100100, 2001, 2, 3). Runs in all three GC modes (`tests/loops.rs`). |
| M9-TS-62 | Integration | A captured `for` variable is a new binding every iteration (closures see 0, 1, 2) and a closure's change to it carries into the next iteration (12, 3). Runs in all three GC modes (`tests/loops.rs`). |
| M9-TS-63 | Integration | Objects allocated in the head, body and update of a `for` survive a collection at every allocation (5050). Runs in all three GC modes (`tests/loops.rs`). |
| M9-TS-64 | Integration | A switch on an enum value picks the case, the shared `case Left: case Right:` body and the default. Runs normally, in stress GC mode and with collection off (`tests/switches.rs`). |
| M9-TS-65 | Integration | Enum members are numbered (0, 1, 5, 6), refer to earlier members (`B = A << 1`) and fold to constants; functions take and return enum values. Runs in all three GC modes (`tests/switches.rs`). |
| M9-TS-66 | Integration | A switch falls through into the next clause and a `default` in the middle runs only when nothing matches, then falls into the following case. Runs in all three GC modes (`tests/switches.rs`). |
| M9-TS-67 | Integration | A switch on a string-literal union and on a string enum compares tags. Runs in all three GC modes (`tests/switches.rs`). |
| M9-TS-68 | Integration | `continue` in a switch reaches the loop, `break` leaves only the switch, no match and no default does nothing, `break label` leaves a labelled switch from a nested loop. Runs in all three GC modes (`tests/switches.rs`). |
| M9-TS-69 | Integration | A discriminant and case expressions that allocate are evaluated once and in order and survive a collection at every allocation; case blocks keep their own constants. Runs in all three GC modes (`tests/switches.rs`). |
| M9-TS-70 | Unit | Non-constant enum members, unknown members, a string member followed by a member without a value, `continue` outside a loop and `continue` to a non-loop label are errors with a reason (`web_ui_ts/src/lib.rs`). |
| M9-TS-71 | Integration | Object patterns rename (`{ x: ax }`), nest (`min: { x, y }`) and work as function parameters. Runs normally, in stress GC mode and with collection off (`tests/destructure.rs`). |
| M9-TS-72 | Integration | A pattern default replaces a missing (null) property and the value wins when present; the default may allocate (a call). Runs in all three GC modes (`tests/destructure.rs`). |
| M9-TS-73 | Integration | Array patterns skip holes and give a missing element its default (`[a, b, c = 5] = [1, 2]`). Runs in all three GC modes (`tests/destructure.rs`). |
| M9-TS-74 | Integration | `for (const { x, y } of items)`, `for (const [a, b] of rows)` and patterns as inlined callback parameters. Runs in all three GC modes (`tests/destructure.rs`). |
| M9-TS-75 | Integration | A closure takes a pattern parameter, and a closure captures a destructured variable (a cell: it sees later writes). Runs in all three GC modes (`tests/destructure.rs`). |
| M9-TS-76 | Integration | Patterns over freshly allocated objects and arrays inside a loop survive a collection at every allocation. Runs in all three GC modes (`tests/destructure.rs`). |
| M9-TS-77 | Integration | A missing argument takes the parameter's default, which may use earlier parameters (`hi = lo + 10`); several defaults can be left out together. Runs normally, in stress GC mode and with collection off (`tests/params.rs`). |
| M9-TS-78 | Integration | An optional parameter `y?: number` and `it?: Item` is null when left out. Runs in all three GC modes (`tests/params.rs`). |
| M9-TS-79 | Integration | Defaults that are object literals or calls that allocate (`n = make(3).v, m = n * 2`) are evaluated per call. Runs in all three GC modes (`tests/params.rs`). |
| M9-TS-80 | Integration | A rest parameter collects the remaining arguments, alone or after fixed and default parameters. Runs in all three GC modes (`tests/params.rs`). |
| M9-TS-81 | Integration | Spread in a call (into a rest parameter) and in array literals (`[...a, 3, ...a]`, arrays of objects). Runs in all three GC modes (`tests/params.rs`). |
| M9-TS-82 | Integration | `f(...a)` and `[...a]` give new arrays; writing to them leaves `a` alone. Runs in all three GC modes (`tests/params.rs`). |
| M9-TS-83 | Integration | Defaults, rest arrays and spreads that allocate, in a loop, survive a collection at every allocation. Runs in all three GC modes (`tests/params.rs`). |
| M9-TS-84 | Unit | Too few or too many arguments, and a spread into a fixed parameter, are errors (`web_ui_ts/src/lib.rs`). |
| M9-TS-85 | Integration | `Math.ceil` and `Math.trunc` agree with JS bit for bit, including `-0` (`ceil(-0.5)`, `trunc(-0.2)`). Runs normally, in stress GC mode and with collection off (`tests/mathlib.rs`). |
| M9-TS-86 | Integration | `Math.round` sends halves toward +Infinity, keeps `-0`, and `round(0.49999999999999994)` is 0. Runs in all three GC modes (`tests/mathlib.rs`). |
| M9-TS-87 | Integration | `Math.pow` and `**` follow JS for NaN, `1 ** Infinity`, `0 ** 0`, negative bases, right associativity. Runs in all three GC modes (`tests/mathlib.rs`). |
| M9-TS-88 | Integration | `Math.clz32`, `Math.fround` and unary `~` agree with JS. Runs in all three GC modes (`tests/mathlib.rs`). |
| M9-TS-89 | Integration | `tan asin acos atan exp log log2 log10 cbrt` and the `LN2`.. constants agree with JS to nine decimals. Runs in all three GC modes (`tests/mathlib.rs`). |
| M9-TS-90 | Integration | `??=`, `\|\|=`, `&&=`, `**=` assign only when needed, and an indexed target with a side effect is evaluated once. Runs in all three GC modes (`tests/mathlib.rs`). |
| M9-TS-91 | Integration | `Math.random()` stays in [0, 1) over 10,000 draws and repeats after the same seed. Runs in all three GC modes (`tests/mathlib.rs`). |
| M9-TS-92 | Integration | `pop` and `shift` return the element, or null on an empty array. Runs normally, in stress GC mode and with collection off (`tests/arraylib.rs`). |
| M9-TS-93 | Integration | `unshift` puts values in front and returns the new length. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-94 | Integration | `slice` with negative and out-of-range indexes, and `slice()` as a copy. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-95 | Integration | `splice` removes, inserts and replaces, with negative start and no count. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-96 | Integration | `concat` joins arrays and single values into a new array. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-97 | Integration | `reverse` is in place and returns the same array. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-98 | Integration | `sort` with a comparator orders numbers ascending and descending. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-99 | Integration | `sort` is stable for objects, and the comparator may allocate (found a merge buffer that was traced before it was initialised). Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-100 | Integration | 200 pseudo-random items sort into a stable order. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-101 | Integration | Sorting with a collection at every allocation, including a comparator that captures variables, loses nothing (`tests/arraylib.rs`). |
| M9-TS-102 | Integration | `fill` over a range and `new Array(5).fill(2)`. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-103 | Integration | `indexOf`, `lastIndexOf`, `findIndex` and `includes`, found and not found. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-104 | Integration | `at` counts negative indexes from the end. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-105 | Integration | `reduce` over numbers and objects, with index and initial value. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-106 | Integration | `xs.length = n` truncates. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-107 | Integration | `Array.from({ length }, fn)`, `Array.from(xs)` (a copy), `Array.of`, `Array.isArray`. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-108 | Integration | Array operations on arrays of objects in a loop (sort, splice, slice, concat, reverse, unshift, pop, reduce) survive a collection at every allocation. Runs in all three GC modes (`tests/arraylib.rs`). |
| M9-TS-109 | Unit | `sort` keeps its merge buffer alive while the comparator collects: with the roots removed the elements come back poisoned (`web_ui_rt/src/lib.rs`). |
| M9-TS-110 | Unit | `sort()` without a comparator and `pop` over booleans are errors with a reason; `reduce` without an initial value compiles (since M9-TS-3386) (`web_ui_ts/src/lib.rs`). |
| M9-TS-111 | Integration | A class with a constructor, methods, a getter and statics (`new Vec2(3, 4).add(Vec2.of(1, 2)).scale(2)`, `length`, a static counter, `static readonly zero`) computes like JS. Runs normally, in stress GC mode and with collection off (`tests/classes.rs`). |
| M9-TS-112 | Integration | Constructor parameters take their defaults (`new Vec2()`, `new Vec2(7)`). Runs in all three GC modes (`tests/classes.rs`). |
| M9-TS-113 | Integration | Private fields and methods (`#count`, `#bump`), a getter/setter pair, chaining by returning `this`, `this` captured by an arrow function and used in an inlined callback, an optional field starting undefined. Runs in all three GC modes (`tests/classes.rs`). |
| M9-TS-114 | Integration | Objects linked through class fields (a stack of cells) survive collections. Runs in all three GC modes (`tests/classes.rs`). |
| M9-TS-115 | Integration | Constructors run parameter properties, then field initialisers in order (`b = this.a + 1`), then the body; a bare `return` returns the object. Runs in all three GC modes (`tests/classes.rs`). |
| M9-TS-116 | Integration | A method receiver waiting for its arguments is not freed: `h.item.plus(swap(h))` gets the old `this` (5 + 1). Fails when field reads are trusted as stable. Runs in all three GC modes (`tests/classes.rs`). |
| M9-TS-117 | Integration | Two names for one object, and `===` as identity. Runs in all three GC modes (`tests/classes.rs`). |
| M9-TS-118 | Integration | Methods, constructors and a stack in a loop survive a collection at every allocation. Runs in all three GC modes (`tests/classes.rs`). |
| M9-TS-119 | Unit | `extends`, decorators, a method used as a value, a field without a type and `this` outside a method are errors that say what is missing (`web_ui_ts/src/lib.rs`). |
| M9-TS-120 | Integration | A call on a base-class variable runs the subclass's method (areas of a Rect, Square, Circle and Tri held as `Shape`). Runs normally, in stress GC mode and with collection off (`tests/inherit.rs`). |
| M9-TS-121 | Integration | `this` dispatches on the real class inside a base-class method (`describe()` calls `this.area()`). Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-122 | Integration | Getters can be overridden and are dispatched too. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-123 | Integration | `super.area()` calls the base implementation; a base-typed variable still gets the override. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-124 | Integration | `instanceof` is true for the class and its subclasses and false for null. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-125 | Integration | A member only a subclass has is usable after `instanceof` narrowing or `as`. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-126 | Integration | Subclass objects flow through variables, arrays and conditionals as their base class. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-127 | Integration | A subclass constructor assigns after `super(...)` returns. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-128 | Integration | A field a subclass declares again is the same slot and its initialiser wins. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-129 | Integration | A base constructor calling an overridden method reaches the subclass's version. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-130 | Integration | A derived class without a constructor forwards its parameters, then runs its initialisers. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-131 | Integration | An override calls `super` through a recursive structure. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-132 | Integration | A static counter in the base class counts every subclass construction. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-133 | Integration | Virtual calls, `instanceof` and subclass arrays under allocation pressure survive a collection at every allocation (`tests/inherit.rs`). |
| M9-TS-134 | Unit | Instantiating an abstract class, a subclass declared before its base, a field that changes type in a subclass, and `instanceof` on a non-class are errors that say so (`web_ui_ts/src/lib.rs`). |
| M9-TS-135 | Integration | A virtual call's receiver waiting for its arguments is not freed (`h.s.scaled(swapShape(h))` = 3). Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-136 | Integration | A call that returns nothing and needs a default filled in still runs (it was skipped when its value was void). Runs in all three GC modes (`tests/params.rs`). |
| M9-TS-137 | Integration | Methods as values (`g.twice`, `xs.map(this.twice)`) and an arrow-function field that captures `this`. Runs normally, in stress GC mode and with collection off (`tests/classes.rs`). |
| M9-TS-138 | Integration | A method taken as a value from a base-typed variable still dispatches on the real class. Runs in all three GC modes (`tests/inherit.rs`). |
| M9-TS-139 | Integration | The `@property` table lists only the marked fields, in declaration order with the base class's first, with byte offsets and type names (`PROPS_Player`, `PROPS_Hero`). Not GC-dependent (`tests/props.rs`). |
| M9-TS-140 | Integration | A host writes properties through the table (a number, a boolean, a subclass's own through its table and a base property through the base table) and the compiled code sees the new values; run with a collection at every allocation (`tests/props.rs`). |
| M9-TS-142 | Integration | A script calls engine functions through `#[ts_api]` shims with the engine's number types (f64, i32 clamp, u32 and bool); fractional numbers truncate toward zero at an i32 parameter. Runs normally, in stress GC mode and with collection off (`tests/engine.rs`). |
| M9-TS-143 | Integration | f32 arguments narrow and f32 results widen at the call (`Demo.half(0.1)` equals `(0.1f32 * 0.5f32) as f64`). Runs in all three GC modes (`tests/engine.rs`). |
| M9-TS-144 | Integration | Handles are plain ids: `===` compares them, a destroyed entity's id misses even when its slot is reused, and an engine function can answer for a stale id. Runs in all three GC modes (`tests/engine.rs`). |
| M9-TS-145 | Integration | A handle lives in objects, arrays, nullable variables and closure cells without being traced, between allocations that collect at every step (`handle !== 0` is the null test). Runs in all three GC modes (`tests/engine.rs`). |
| M9-TS-146 | Unit | Calls of the engine API are checked against the manifest: an unknown function, a wrong argument count and a malformed manifest are errors; each symbol is declared once (`web_ui_ts/src/lib.rs`). |
| M9-TS-147 | Integration | A value class is an object in the script, passed to the engine as scalars and coming back as a new object; its fields are f32-rounded. Runs normally, in stress GC mode and with collection off (`tests/engine.rs`). |
| M9-TS-148 | Integration | A value stored in the engine next to a handle reads back (`tests/engine.rs`). |
| M9-TS-149 | Integration | Dozens of short-lived vectors survive a collection at every allocation (`tests/engine.rs`). |
| M9-TS-150 | Unit | Value classes of the manifest are checked: flattened parameters, an out pointer for a result, wrong constructor arity, a number where a value is expected (`web_ui_ts/src/lib.rs`). |
| M9-TS-151 | Integration | Compiled code builds, parents, moves and destroys a hierarchy in the real `platypus_scene::Scene`. Runs normally, in stress GC mode and with collection off (`tests/scene.rs`). |
| M9-TS-152 | Integration | A per-frame update loop moves an object round a circle through the `Scene` API (`tests/scene.rs`). |
| M9-TS-153 | Integration | Stale object handles miss even when the slot is reused (`tests/scene.rs`). |
| M9-TS-154 | Integration | A `Quaternion` value goes in and comes back normalised (`tests/scene.rs`). |
| M9-TS-155 | Integration | Thirty objects and their handles in an array survive a collection at every allocation (`tests/scene.rs`). |
| M9-TS-156 | Integration | A hierarchy built and moved with handle classes (`child.transform.setParent(..)`, `transform.localPosition = new Vector3(..)`, `go.destroy()`) in the real scene (`tests/scene.rs`). |
| M9-TS-157 | Integration | Boolean, number and handle properties and a method of `GameObject`, and the `transform.gameObject` round trip (`tests/scene.rs`). |
| M9-TS-158 | Integration | An update loop moves an object through `transform.position` (`tests/scene.rs`). |
| M9-TS-159 | Integration | `Vector3` methods and getters (`add`, `scale`, `dot`, `cross`, `lerp`, `distance`, `magnitude`, `normalized`) compile to direct calls, chained on call results (`tests/scene.rs`). |
| M9-TS-160 | Integration | `Quaternion` `rotate`, `mul` and `inverse` (`tests/scene.rs`). |
| M9-TS-161 | Integration | A vector kept across the allocations of a loop of method calls survives collection at every allocation (`tests/scene.rs`). |
| M9-TS-162 | Unit | A string's `length` counts UTF-16 units (accents, CJK, a pair is 2, a lone surrogate 1) (`web_ui_rt/src/string.rs`). |
| M9-TS-163 | Unit | Both units of a surrogate pair are reachable by index; `codePointAt` on the low half is the lone surrogate (`string.rs`). |
| M9-TS-164 | Unit | Slicing through a pair leaves lone surrogates (WTF-8) that rejoin into the original on concatenation (`string.rs`). |
| M9-TS-165 | Unit | Ordering is by UTF-16 code unit, not code point (an emoji sorts before U+FF21) (`string.rs`). |
| M9-TS-166 | Unit | Search converts between byte offsets and unit indices; a lone low surrogate is found inside a pair (`string.rs`). |
| M9-TS-167 | Unit | A forward and a backward scan of a long non-ASCII string through the cursor are correct (`string.rs`). |
| M9-TS-168 | Differential | `length`, `charCodeAt`, `codePointAt`, `charAt` at every index of a 24-string corpus (ASCII, accents, CJK, astral, lone surrogates) equal what bun answers (`web_ui_rt/tests/strings.rs`, expected data from `tests/gen_strings.ts`). |
| M9-TS-169 | Differential | `slice` and `substring` for every index pair equal bun's (`tests/strings.rs`). |
| M9-TS-170 | Differential | Equality, `<` ordering and concatenation (rejoining split pairs) over every pair of strings equal bun's (`tests/strings.rs`). |
| M9-TS-171 | Differential | `indexOf`, `lastIndexOf`, `includes`, `startsWith`, `endsWith` with every position over every pair equal bun's (`tests/strings.rs`). |
| M9-TS-172 | Unit | A string is one collectable object: an unrooted one is freed, a rooted one survives (`string.rs`). |
| M9-TS-173 | Differential | `trim`, `trimStart`, `trimEnd`, `toUpperCase`, `toLowerCase` (special casing, final sigma) over a corpus with Unicode white space, accents and lone surrogates equal bun's (`web_ui_rt/tests/strings_more.rs`). |
| M9-TS-174 | Differential | `at`, `repeat`, `padStart`, `padEnd` (fillers cut inside a pair) equal bun's (`tests/strings_more.rs`). |
| M9-TS-175 | Differential | `split` (empty, one-unit, multi-unit and lone-surrogate separators) and `join` (rejoining split pairs) equal bun's (`tests/strings_more.rs`). |
| M9-TS-176 | Differential | `String(x)`, `toFixed(0..6)` (ties round up on the exact value) and `toString(radix)` equal bun's (`tests/strings_more.rs`). |
| M9-TS-177 | Differential | `Number(s)`, `parseFloat(s)` and `parseInt(s, radix)` equal bun's over hex/binary/octal, signs, exponents, white space, trailing text and bad radixes (`tests/strings_more.rs`). |
| M9-TS-178 | Integration | Compiled string code (`length`, comparison by code unit, search, `switch`, truthiness, parsing, a loop over `charCodeAt`) returns what bun returns, in the normal, stress and collection-off modes (`platypus_ts_tests/tests/strings.rs`, `bench/strings.ts`, expected data from `bench/gen_strings_expected.ts`). |
| M9-TS-179 | Integration | `+`, `+=` with numbers/booleans/null, template literals, method chains, padding, `fromCharCode`, `toFixed`, `toString(16)`, `split`/`join` and nested temporaries equal bun's (`tests/strings.rs`). |
| M9-TS-180 | Integration | Strings in arrays (`includes`, `indexOf`, `sort` with a comparator), object fields, `string \| null` and string-literal unions mixed with `string` equal bun's (`tests/strings.rs`). |
| M9-TS-181 | Integration | Three hundred strings made in a loop survive a collection at every allocation (`tests/strings.rs`). |
| M9-TS-182 | Unit | Unsupported string methods (`localeCompare`, `normalize`, `replace`) and a literal with a lone surrogate are compile errors with a reason (`web_ui_ts/src/lib.rs`). |
| M9-TS-183 | Integration | `GameObject.name` set and read, and `GameObject.find(name)`, through the real scene: strings in as script strings and out as new ones (`platypus_ts_tests/tests/scene.rs`). |
| M9-TS-184 | Integration | A name with accents and an emoji round-trips; a lone surrogate reaches the engine as U+FFFD (`tests/scene.rs`). |
| M9-TS-185 | Integration | Thirty names made, read back and found survive a collection at every allocation (`tests/scene.rs`). |
| M9-TS-186 | Integration | `ts_api::ScriptStr` has the layout of the runtime's `Str` (`tests/scene.rs`). |
| M9-TS-187 | Integration | `throw` in a callee and `catch` in the caller: message, name, `toString`, a subclass with its own field, `instanceof` to tell the classes apart, equal to bun (`platypus_ts_tests/tests/errors.rs`, `bench/errors.ts`, expected data from `bench/gen_errors_expected.ts`). |
| M9-TS-188 | Integration | An exception unwinds through six frames holding strings, is rethrown wrapped, and leaves an inlined array callback and a closure (`tests/errors.rs`). |
| M9-TS-189 | Integration | `continue` and `break` inside `catch`, and two hundred throws and catches with a collection at every allocation (`tests/errors.rs`). |
| M9-TS-190 | Integration | An uncaught exception leaves the exported function with the exception pending; the host takes the error object (`tests/errors.rs`). |
| M9-TS-191 | Unit | Only functions that can throw are checked after a call; callers of non-throwing code pay nothing (`web_ui_ts/src/lib.rs`). |
| M9-TS-192 | Unit | `throw` of a non-Error is a compile error with a reason (`web_ui_ts/src/lib.rs`). |
| M9-TS-193 | Integration | `finally` runs once on a normal exit, after `catch`, and on `return`, in nested order, and while an exception propagates (`tests/errors.rs`). |
| M9-TS-194 | Integration | `break`, `continue`, `continue label` and `return` pass through `finally`; a `return` in `finally` replaces the one in `try` (`tests/errors.rs`). |
| M9-TS-195 | Integration | Runtime errors (`repeat(-1)`, `toFixed(101)`, `toString(37)`, `fromCodePoint(-1)`) are catchable `RangeError`s with bun's messages (`tests/errors.rs`). |
| M9-TS-196 | Integration | An uncaught runtime error reaches the host as a pending `RangeError` (`tests/errors.rs`). |
| M9-TS-197 | Unit | A throwing function nothing reaches does not make closure calls checked (`web_ui_ts/src/lib.rs`). |
| M9-TS-198 | Integration | A `break` inside an array callback inlined into a `try ... finally` stays inside the callback (`platypus_ts_tests/tests/errors.rs`). |
| M9-TS-199 | Integration | `typeof` of numbers, strings, booleans, functions, objects, arrays, a missing value and an undeclared name, and `typeof x === "..."` comparisons equal bun's (`platypus_ts_tests/tests/stdlib.rs`, `bench/stdlib.ts`, expected data from `bench/gen_stdlib_expected.ts`). |
| M9-TS-200 | Integration | `Number.isInteger/isNaN/isFinite/isSafeInteger`, global `isNaN`/`isFinite`, the number constants, `Number.parseFloat/parseInt` equal bun's (`tests/stdlib.rs`). |
| M9-TS-201 | Integration | `Math.sinh cosh tanh asinh acosh atanh expm1 log1p` equal bun's to ten decimals (`tests/stdlib.rs`). |
| M9-TS-202 | Integration | `toSorted`, `toReversed`, `findLast`, `findLastIndex`, `reduceRight`, `flat`, `flatMap`, the default `sort()` of numbers and strings, and `entries()`/`keys()`/`values()` in `for...of` equal bun's (`tests/stdlib.rs`). |
| M9-TS-203 | Integration | `console.log/warn/error` join their arguments with spaces and reach the host's console hook with the level (`tests/stdlib.rs`). |
| M9-TS-204 | Integration | `Object.keys/values/entries/assign/freeze/hasOwn`, `in`, object spread (new fields, source order) and rest, and array rest equal bun's (`tests/stdlib.rs`). |
| M9-TS-205 | Unit | `Object.entries` outside `for...of`, `Object.values` over mixed types, `in` with a computed key and `console.log` of an object are compile errors with a reason (`web_ui_ts/src/lib.rs`). |
| M9-TS-206 | Unit | `Map` numbers follow SameValueZero (`NaN` is a key, `-0` is `0`) and keep insertion order (`web_ui_rt/src/map.rs`). |
| M9-TS-207 | Unit | A map iterator (sequence-number cursor) survives deletes, adds and table rebuilds (`map.rs`). |
| M9-TS-208 | Unit | A map is one collectable object that keeps its keys and values alive; string keys compare by content (`map.rs`). |
| M9-TS-209 | Integration | `Map<string, number>`: `set` chained, `get`, `has`, `delete`, `size`, `clear`, `keys()`/`values()`, `for...of`, `forEach`, equal to bun (`platypus_ts_tests/tests/collections.rs`, `bench/collections.ts`, expected data from `bench/gen_collections_expected.ts`). |
| M9-TS-210 | Integration | Number keys (SameValueZero) and object keys (identity) equal bun's (`tests/collections.rs`). |
| M9-TS-211 | Integration | `Set`: from an array, `add`, `has`, `delete`, spread, `Array.from`, `for...of`, `forEach`, element type inferred, equal to bun's (`tests/collections.rs`). |
| M9-TS-212 | Integration | Iteration is live: deleted-before-reached entries are skipped, added entries are visited, the table rebuilds under the loop (`tests/collections.rs`). |
| M9-TS-213 | Integration | Two thousand string keys with deletes and adds under a collection at every allocation, and `WeakMap`/`WeakSet` (`tests/collections.rs`). |
| M9-TS-214 | Unit | Collections without a type, `entries()` outside `for...of`, `Map.get` over booleans and a `Map` as a key are compile errors with a reason (`web_ui_ts/src/lib.rs`). |
| M9-TS-215 | Integration | `JSON.stringify` of objects, arrays, escapes, lone surrogates, `NaN`/`-0`/exponents, booleans, empty collections, with and without indentation equals bun's (`platypus_ts_tests/tests/json.rs`, `bench/json.ts`, expected data from `bench/gen_json_expected.ts`). |
| M9-TS-216 | Integration | Optional fields are left out when absent, `T \| null` prints `null`, a subclass instance prints its own fields (`tests/json.rs`). |
| M9-TS-217 | Integration | `JSON.parse` into typed objects (unknown fields skipped, surrogate pairs from escapes, optional fields), arrays, nullable numbers and literal unions, and back through `stringify`, equals bun's (`tests/json.rs`). |
| M9-TS-218 | Integration | Sixty-seven malformed texts raise a `SyntaxError` with bun's text (`tests/json.rs`). |
| M9-TS-219 | Integration | Three hundred objects stringified, parsed and stringified again under a collection at every allocation (`tests/json.rs`). |
| M9-TS-220 | Integration | A shape mismatch is a `TypeError`, and a syntax error anywhere wins over it (`tests/json.rs`). |
| M9-TS-221 | Unit | `JSON.parse` without a type, with a reviver, `stringify` with a replacer and of a function are compile errors with a reason (`web_ui_ts/src/lib.rs`). |
| M9-TS-222 | Integration | `exec` and `test`: the match object (`m[0]`, groups, `index`, `length`, `input`), a miss, `source`, `flags`, `global` equal bun's (`platypus_ts_tests/tests/regexp.rs`, `bench/regexp.ts`, expected data from `bench/gen_regexp_expected.ts`). |
| M9-TS-223 | Integration | A `g` regex steps through a string with `lastIndex`; a miss resets it (`tests/regexp.rs`). |
| M9-TS-224 | Integration | Named groups (`m.groups.year`), a missing name, no `groups` without named groups (`tests/regexp.rs`). |
| M9-TS-225 | Integration | The flags `m s i y` change matching as in JavaScript (`tests/regexp.rs`). |
| M9-TS-226 | Integration | Unicode: `u` matches a surrogate pair as one character, indices count UTF-16 units, property escapes and code point escapes (`tests/regexp.rs`). |
| M9-TS-227 | Integration | Lookbehind, backreferences, alternation, lazy quantifiers, negated classes, word boundaries (`tests/regexp.rs`). |
| M9-TS-228 | Integration | `new RegExp(str, flags)`, `RegExp(..)`, copying with new flags, canonical flag order, `source` escaping `/`, `String(re)` (`tests/regexp.rs`). |
| M9-TS-229 | Integration | Bad patterns and flags are `SyntaxError`s with JavaScriptCore's text (`tests/regexp.rs`). |
| M9-TS-230 | Integration | A match object works as an array: destructuring, spread, `for...of`, `map` (`tests/regexp.rs`). |
| M9-TS-231 | Unit | An invalid regex literal is a compile error with the engine's message (`web_ui_ts/src/lib.rs`). |
| M9-TS-232 | Integration | `search` with a regex or a string (UTF-16 index) equals bun's (`platypus_ts_tests/tests/regexp2.rs`, `bench/regexp2.ts`, expected data from `bench/gen_regexp2_expected.ts`). |
| M9-TS-233 | Integration | `match`: a plain regex gives the match object, a `g` regex the texts of all matches (`lastIndex` reset), a string pattern is a regex (`tests/regexp2.rs`). |
| M9-TS-234 | Integration | `matchAll` steps over every match, past empty matches; a non-global regex is bun's `TypeError` (`tests/regexp2.rs`). |
| M9-TS-235 | Integration | `replace` / `replaceAll` with string patterns (matched literally) and `$` patterns; `replaceAll` with a non-global regex is bun's `TypeError` (`tests/regexp2.rs`). |
| M9-TS-236 | Integration | `replace` with a regex: `$1 $01 $10 $& $` $' $<name> $$`, global and first-only, empty matches, non-ASCII text (`tests/regexp2.rs`). |
| M9-TS-237 | Integration | `replace` with a function: match, groups, offset, string and the named-groups object; a `const` regex keeps its `lastIndex` (`tests/regexp2.rs`). |
| M9-TS-238 | Integration | `split` with a regex (captures included, limit, empty input, empty pattern, `u` flag) and a string with a limit (`tests/regexp2.rs`). |
| M9-TS-239 | Unit | A replacement function with more than the match as a parameter needs a regex whose groups are known; too many parameters is a compile error (`web_ui_ts/src/lib.rs`). |
| M9-TS-240 | Integration | Local and UTC fields of nine instants (epoch, DST, year -1, the extremes) equal bun's in five time zones (`platypus_ts_tests/tests/date.rs`, `bench/date.ts`, expected data from `bench/gen_date_expected.ts`). |
| M9-TS-241 | Integration | `Date.UTC` with overflowing and fractional parts, two-digit years and the range edges (`tests/date.rs`). |
| M9-TS-242 | Integration | `new Date(y, m, ...)` in local time, including a clock set forward or back and years far from now (`tests/date.rs`). |
| M9-TS-243 | Integration | `new Date(ms)`, `(string)`, `(date)`, NaN and out-of-range values (`tests/date.rs`). |
| M9-TS-244 | Integration | Every `set*` method in local time and UTC: overflow, optional extra arguments, an invalid date, `setTime` (`tests/date.rs`). |
| M9-TS-245 | Integration | `toISOString` (expanded years) and `toJSON`, and the `RangeError` text for an invalid date (`tests/date.rs`). |
| M9-TS-246 | Integration | `toUTCString`, `toGMTString` and `toDateString` including negative and five-digit years and an invalid date (`tests/date.rs`). |
| M9-TS-247 | Integration | `Date.parse` of 34 ISO-form strings: date only is UTC, date-time is local, offsets, `24:00`, expanded years, bad input (`tests/date.rs`). |
| M9-TS-248 | Integration | `Date.parse` of 42 other forms: month names, slashes, AM/PM, zone names and offsets, two-digit years, bad input (`tests/date.rs`). |
| M9-TS-249 | Integration | A date in arithmetic and comparison: `b - a`, `+a`, `Number(a)`, `<`, `===` is identity (`tests/date.rs`). |
| M9-TS-250 | Integration | `getTimezoneOffset` over DST changes, old local mean time and the range edges (`tests/date.rs`). |
| M9-TS-251 | Integration | `Date.now()` and `new Date()` give the current time (`tests/date.rs`). |
| M9-TS-252 | Integration | Time clipping at +-8.64e15 and truncation of fractions (`tests/date.rs`). |
| M9-TS-253 | Unit | `toLocaleString`, `toLocaleDateString` and `toLocaleTimeString` are compile errors that point to Intl (`web_ui_ts/src/lib.rs`). |
| M9-TS-254 | Integration | `localeCompare` with default and explicit locales (de, sv, tr, a locale list) equals bun's (`platypus_ts_tests/tests/intl.rs`, `bench/intl.ts`, expected data from `bench/gen_intl_expected.ts`). |
| M9-TS-255 | Integration | `localeCompare` options: `sensitivity`, `numeric`, `caseFirst`, `ignorePunctuation`, `usage` (`tests/intl.rs`). |
| M9-TS-256 | Integration | An invalid language tag or option value is a `RangeError` with JavaScriptCore's text (`tests/intl.rs`). |
| M9-TS-257 | Integration | `normalize` NFC, NFD, NFKC, NFKD, Hangul, and a lone surrogate left alone (`tests/intl.rs`). |
| M9-TS-258 | Integration | A bad normalization form is bun's `RangeError` (`tests/intl.rs`). |
| M9-TS-259 | Integration | `toLocaleUpperCase` / `toLocaleLowerCase` with tr, az, lt, el, de, special casing (`tests/intl.rs`). |
| M9-TS-260 | Integration | `array.sort(collator.compare)` for en, de numeric, sv, ja, zh and case/sensitivity options gives bun's order (`tests/intl.rs`). |
| M9-TS-261 | Integration | `new Intl.Collator().compare(a, b)` results (`tests/intl.rs`). |
| M9-TS-262 | Integration | `new Intl.Collator` with a bad tag or option is bun's `RangeError` (`tests/intl.rs`). |
| M9-TS-263 | Unit | Intl options that are not literals, and `resolvedOptions`, are compile errors (`web_ui_ts/src/lib.rs`). |
| M9-TS-264 | Integration | Decimal locales: `toLocaleString` for 15 locales (grouping, decimal marks, Indian grouping, Arabic digits, minus signs, NaN and infinity) equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-265 | Integration | Fraction digits: `minimumFractionDigits`, `maximumFractionDigits`, `minimumIntegerDigits` and their defaults equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-266 | Integration | Significant digits: `minimumSignificantDigits` / `maximumSignificantDigits` equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-267 | Integration | Percent and sign display: `style: percent` in three locales, `signDisplay` (auto, never, always, exceptZero, negative), NaN and infinity equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-268 | Integration | Currency: `style: currency` in ten locale/currency pairs (symbols, placement, currency digits, rounding) equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-269 | Integration | Currency display: `currencyDisplay` code/name/narrowSymbol, `currencySign: accounting`, lower-case codes, fraction overrides equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-270 | Integration | Compact: `notation: compact` short and long in en, de, ja, fr, en-IN, with carry into the next unit equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-271 | Integration | Scientific: `notation: scientific` and `engineering` equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-272 | Integration | Grouping: `useGrouping` false/true/min2 and locales that group from five digits equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-273 | Integration | Rounding: all nine `roundingMode`s and `trailingZeroDisplay` equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-274 | Integration | Format object: `new Intl.NumberFormat(..).format`, also as a detached function and as a `map` callback equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-275 | Integration | Errors: bad `style`, `currency`, `currencyDisplay`, `notation`, `signDisplay`, digit ranges and tags are bun's `RangeError`/`TypeError` text equals bun's (`platypus_ts_tests/tests/numfmt.rs`, `bench/numfmt.ts`). |
| M9-TS-276 | Integration | `toString`, `toTimeString`, `String(date)`, template strings and `date + x` give bun's text, including the zone name ("Eastern Daylight Time"), old dates, the range edges and an invalid date, in five zones (`tests/date.rs`). |
| M9-TS-277 | Integration | Date and time with a locale and no options: `toLocaleString`, `toLocaleDateString`, `toLocaleTimeString` for 15 locales equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-278 | Integration | `dateStyle` full, long, medium, short: in 15 locales (all but the columns listed in `docs/differences.md`) equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-279 | Integration | `timeStyle`: full, long, medium, short in 15 locales with zone names equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-280 | Integration | `dateStyle` with `timeStyle`: five locales equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-281 | Integration | Component options: year, month, day, weekday, hour, minute, second, `hour12`, `hourCycle`, `fractionalSecondDigits` in en-US, en-GB, de, fr, ja equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-282 | Integration | `timeZoneName`: short, long, shortOffset, longOffset, shortGeneric, longGeneric equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-283 | Integration | `timeZone` option: UTC, Tokyo, Los Angeles, Lord Howe, Kiritimati equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-284 | Integration | `Intl.DateTimeFormat.format`: also detached and as a `map` callback equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-285 | Integration | An invalid date: `toLocale*String` gives `Invalid Date`, `format` throws bun's `RangeError` equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-286 | Integration | Errors: bad option values, `dateStyle` with other options, a style the entry point does not allow, bad zone, bad tag: bun's text equal bun's in five time zones (`platypus_ts_tests/tests/datefmt.rs`, `bench/datefmt.ts`, made by `bench/gen_datefmt_program.py`). |
| M9-TS-287 | Integration | `encodeURIComponent` / `encodeURI` of ASCII, reserved characters, accents, euro, emoji and control characters equal bun's (`platypus_ts_tests/tests/uri.rs`, `bench/uri.ts`). |
| M9-TS-288 | Integration | `decodeURIComponent` / `decodeURI` (which keeps escapes of reserved characters, upper and lower case hex) equal bun's (`tests/uri.rs`). |
| M9-TS-289 | Integration | Encode then decode returns the text, for several scripts and emoji (`tests/uri.rs`). |
| M9-TS-290 | Integration | A lone surrogate, a bad or truncated escape, an overlong, surrogate or too-large UTF-8 sequence are `URIError`s with JavaScriptCore's text and class (`tests/uri.rs`). |
| M9-TS-291 | Integration | `escape` / `unescape` equal bun's (`tests/uri.rs`). |
| M9-TS-292 | Integration | `new Error(message, { cause })` chains, `cause` absent is null (`platypus_ts_tests/tests/errors2.rs`, `bench/errors2.ts`). |
| M9-TS-293 | Integration | `cause` set in a `catch`, `instanceof` on it (`tests/errors2.rs`). |
| M9-TS-294 | Integration | `AggregateError`: `errors`, `name`, message, `cause`, `instanceof` equal bun's (`tests/errors2.rs`). |
| M9-TS-295 | Integration | A thrown `AggregateError` is caught and read (`tests/errors2.rs`). |
| M9-TS-296 | Integration | `Intl.PluralRules` cardinal categories for 28 numbers in 20 locales (zero, one, two, few, many, other) equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-297 | Integration | `Intl.PluralRules` ordinal categories in 8 locales equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-298 | Integration | `PluralRules` with `minimumFractionDigits` / `maximumFractionDigits`, NaN and infinity equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-299 | Integration | `PluralRules` bad type and bad tag give bun's `RangeError` text equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-300 | Integration | `Intl.ListFormat` conjunction, long/short/narrow, 17 locales, lists of 0 to 4 equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-301 | Integration | `Intl.ListFormat` disjunction equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-302 | Integration | `Intl.ListFormat` unit equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-303 | Integration | `ListFormat` default options equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-304 | Integration | `ListFormat` bad style, type and tag equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-305 | Integration | `Intl.RelativeTimeFormat` numeric always, long: 17 locales, 8 units, 9 values equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-306 | Integration | `RelativeTimeFormat` numeric always, narrow equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-307 | Integration | `RelativeTimeFormat` numeric always, short equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-308 | Integration | `RelativeTimeFormat` numeric auto (yesterday, tomorrow, this year), long equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-309 | Integration | `RelativeTimeFormat` numeric auto, narrow equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-310 | Integration | `RelativeTimeFormat` numeric auto, short equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-311 | Integration | `RelativeTimeFormat` accepts plural unit names and `-0` equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-312 | Integration | `RelativeTimeFormat` bad unit, NaN, infinity, bad options and tag equal bun's (`platypus_ts_tests/tests/intl4.rs`, `bench/intl4.ts`, made by `bench/gen_intl4_program.py`). |
| M9-TS-313..317 | Integration | `Int8Array`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-318..322 | Integration | `Uint8Array`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-323..327 | Integration | `Uint8ClampedArray`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-328..332 | Integration | `Int16Array`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-333..337 | Integration | `Uint16Array`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-338..342 | Integration | `Int32Array`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-343..347 | Integration | `Uint32Array`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-348..352 | Integration | `Float32Array`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-353..357 | Integration | `Float64Array`: stores of 28 awkward numbers (wrapping, clamping, rounding, NaN, infinities), constructors (length, `number[]`, other typed arrays, `from`), `forEach/map/filter/slice/reduce/indexOf/includes/find/some/every/at/join`, `fill/set/sort/reverse/toSorted/toReversed`, `for...of`, spread and out-of-range writes equal bun's (`platypus_ts_tests/tests/typed.rs`, `bench/typed.ts`, made by `bench/gen_typed_program.py`). |
| M9-TS-358 | Integration | `set` out of range or with a negative offset is bun's `RangeError` text (`tests/typed.rs`). |
| M9-TS-359 | Integration | Sorting floats with NaN and infinities, and `Float32Array` rounding (`tests/typed.rs`). |
| M9-TS-360 | Integration | Literals in decimal, hex, binary, octal and with separators, 30 digits and more equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-361 | Integration | Arithmetic: `+ - * / % **` with truncating division, negative operands and large powers equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-362 | Integration | Bitwise `& | ^ ~ << >>` over positive, negative and huge operands (all pairs) equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-363 | Integration | Comparison with bigints and numbers (`< <= > >= == != === !==`, NaN, infinities, fractions, 2^64) equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-364 | Integration | Conversions: `BigInt(number | string | boolean | bigint)`, `Number(big)`, `toString(radix)`, `String`, template, `typeof` equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-365 | Integration | Truthiness of `0n` equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-366 | Integration | `BigInt.asIntN` / `asUintN` at several widths equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-367 | Integration | `++`, `--`, `*=`, `for` and `while` loops, factorial of 30, Fibonacci of 150, arrays of bigints equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-368 | Integration | `RangeError` / `SyntaxError` texts: non-integer, bad string, division by zero, negative exponent, radix, too big, negative bits equal bun's (`platypus_ts_tests/tests/bigint.rs`, `bench/bigint.ts`). |
| M9-TS-369 | Unit | Mixing a bigint with a number in arithmetic, unary `+`, and `>>>` are compile errors; comparing with a number is allowed (`web_ui_ts/src/lib.rs`). |
| M9-TS-370 | Integration | `a || b` and `a && b` on numbers give the operand, not a boolean equal bun's (`platypus_ts_tests/tests/logic.rs`, `bench/logic.ts`). |
| M9-TS-371 | Integration | The same on strings equal bun's (`platypus_ts_tests/tests/logic.rs`, `bench/logic.ts`). |
| M9-TS-372 | Integration | The same with a nullable left side (`x || 5`) and with objects equal bun's (`platypus_ts_tests/tests/logic.rs`, `bench/logic.ts`). |
| M9-TS-373 | Integration | Chains (`a || b || c`, `a && b && c`, mixed) equal bun's (`platypus_ts_tests/tests/logic.rs`, `bench/logic.ts`). |
| M9-TS-374 | Integration | In conditions (`if`, `while`, `?:`, `!`) they stay tests, including `obj && obj.v > 3` and `!(n || false)` equal bun's (`platypus_ts_tests/tests/logic.rs`, `bench/logic.ts`). |
| M9-TS-375 | Integration | Defaults (`name || "anonymous"`, an empty string is falsy; a nullable string tests for empty) equal bun's (`platypus_ts_tests/tests/logic.rs`, `bench/logic.ts`). |
| M9-TS-376 | Integration | `for...of` over classes with `[Symbol.iterator]()`: a separate iterator class, an iterator that returns itself, string values, nested loops equal bun's (`platypus_ts_tests/tests/iterators.rs`, `bench/iterators.ts`). |
| M9-TS-377 | Integration | `break` and `continue`, nested iterables equal bun's (`platypus_ts_tests/tests/iterators.rs`, `bench/iterators.ts`). |
| M9-TS-378 | Integration | Spread `[...iterable]` and `Array.from(iterable)` equal bun's (`platypus_ts_tests/tests/iterators.rs`, `bench/iterators.ts`). |
| M9-TS-379 | Integration | Calling `obj[Symbol.iterator]()` and `next()` by hand equal bun's (`platypus_ts_tests/tests/iterators.rs`, `bench/iterators.ts`). |
| M9-TS-380 | Integration | An exhausted iterator that returns itself yields nothing the second time equal bun's (`platypus_ts_tests/tests/iterators.rs`, `bench/iterators.ts`). |
| M9-TS-381 | Unit | A coroutine yields, is resumed with a value, keeps its shadow-stack frames alive while suspended through collections, stays done, and can be finished early under stress GC (`web_ui_rt/src/coro.rs`). |
| M9-TS-382 | Integration | `function*` with `for`, `next()` results, `done`, exhaustion equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-383 | Integration | An infinite generator taken with `break` equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-384 | Integration | A generator of strings, spread and `Array.from` equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-385 | Integration | A generator of objects equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-386 | Integration | `yield*` of a generator, an array and a counter, in order equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-387 | Integration | A `*[Symbol.iterator]()` method (an in-order tree walk with recursive `yield*`) and a `*method()` using `this` equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-388 | Integration | Two generators advanced alternately equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-389 | Integration | `return()` finishes a generator early equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-390 | Integration | `try` / `finally` around a `yield`, a throw inside a generator reaching the caller, and a generator that has thrown stays finished equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-391 | Integration | `next(value)` sending values in (`Generator<number, void, number | undefined>`) equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-392 | Integration | 300 short generators and 20 kept suspended under collection equal bun's (`platypus_ts_tests/tests/generators.rs`, `bench/generators.ts`). |
| M9-TS-393 | Unit | A generator without a written return type, and `yield*` used as a value, are compile errors with a reason (`web_ui_ts/src/lib.rs`). |
| M9-TS-394 | Integration | `Symbol("x")` values compare by identity (`===`, `!==`) equal bun's (`platypus_ts_tests/tests/symbols.rs`, `bench/symbols.ts`). |
| M9-TS-395 | Integration | `symbol.description` and `toString()` equal bun's (`platypus_ts_tests/tests/symbols.rs`). |
| M9-TS-396 | Integration | `Symbol.for` returns one symbol per key, `Symbol.keyFor` finds registered ones (`platypus_ts_tests/tests/symbols.rs`). |
| M9-TS-397 | Integration | `typeof symbol` is `"symbol"` (`platypus_ts_tests/tests/symbols.rs`). |
| M9-TS-398 | Integration | The `symbol` type in annotations and arrays equals bun's (`platypus_ts_tests/tests/symbols.rs`). |
| M9-TS-399 | Integration | `generator.throw()` caught inside the generator equals bun's (`platypus_ts_tests/tests/generators2.rs`, `bench/generators2.ts`). |
| M9-TS-400 | Integration | `throw()` escaping the generator finishes it, with `finally` run (`platypus_ts_tests/tests/generators2.rs`). |
| M9-TS-401 | Integration | `throw()` before the first `next()` throws and finishes (`platypus_ts_tests/tests/generators2.rs`). |
| M9-TS-402 | Integration | `return(v)` runs `finally` blocks and reports `v` (`platypus_ts_tests/tests/generators2.rs`). |
| M9-TS-403 | Integration | `return(v)` before the first `next()` (`platypus_ts_tests/tests/generators2.rs`). |
| M9-TS-404 | Integration | A `catch` inside a generator does not swallow `return()` (`platypus_ts_tests/tests/generators2.rs`). |
| M9-TS-405 | Integration | `return()` while a `finally` yields, then `next()` finishing with the value (`platypus_ts_tests/tests/generators2.rs`). |
| M9-TS-406 | Integration | `return value` in a generator (`Generator<number, number>`) is the final `value` (`platypus_ts_tests/tests/generators2.rs`). |
| M9-TS-407 | Integration | An early `return` inside a loop (`platypus_ts_tests/tests/generators2.rs`). |
| M9-TS-408 | Integration | An async function returns a value and `await` waits for it equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-409 | Integration | `await` of a plain value takes one tick equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-410 | Integration | The order of async functions, promise reactions, `queueMicrotask` and a timer equals bun's equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-411 | Integration | An async function that throws rejects, and `try/catch` around `await` catches it equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-412 | Integration | `.catch(handler)` returning a replacement value equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-413 | Integration | `.then` chains that change the value's type (number to string) equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-414 | Integration | `.then` returning a promise flattens it, with bun's tick count equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-415 | Integration | `.finally` runs on both outcomes and passes the result through, with bun's ordering equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-416 | Integration | `Promise.all` of promises of different speeds, in order equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-417 | Integration | `Promise.all` rejects with the first rejection equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-418 | Integration | `Promise.race` settles with the fastest equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-419 | Integration | Timers run by due time and then insertion order (`setTimeout` 0, 5, 5, 10) equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-420 | Integration | `setInterval` repeats until `clearInterval` equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-421 | Integration | `clearTimeout` stops a timer equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-422 | Integration | Async methods, async arrows with a block and with an expression body equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-423 | Integration | The executor of `new Promise` settles once; a throwing executor rejects equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-424 | Integration | `return promise` in an async function takes more ticks than `return await promise`, as in bun equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-425 | Integration | 200 async calls under collection (coroutines kept suspended while the collector runs) equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-426 | Integration | `await` in a loop runs the steps in sequence equal bun's (`platypus_ts_tests/tests/promises.rs`, `bench/promises_src.ts`). |
| M9-TS-427 | Integration | `then(f, g)` with both callbacks, fulfilled and rejected, equal bun's (`platypus_ts_tests/tests/promises.rs`). |
| M9-TS-428 | Integration | `Promise.any` resolves with the first fulfilment and rejects with an `AggregateError` holding every reason (`platypus_ts_tests/tests/promises.rs`). |
| M9-TS-429 | Integration | `Promise.allSettled` results (`status`, `value`, `reason`) equal bun's (`platypus_ts_tests/tests/promises.rs`). |
| M9-TS-430 | Unit | Async limits (async generators, `await` outside async, `new Promise` without a type, untyped async return, `then(f, g)`) are compile errors with a reason (`web_ui_ts/src/lib.rs`). |
| M9-TS-431 | Integration | Typed arrays over one `ArrayBuffer` (`Uint8Array`, `Uint32Array`, `Float32Array` with an offset) share memory equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-432 | Integration | Views with offsets and lengths report `length`, `byteOffset` and `byteLength` equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-433 | Integration | Bad view offsets and lengths throw `RangeError` equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-434 | Integration | `subarray` shares memory with its source, including a subarray of a subarray and negative indexes equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-435 | Integration | `array.buffer` of an owning typed array shares its memory equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-436 | Integration | `subarray(..).buffer` is the whole buffer and `byteOffset` is the window's start equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-437 | Integration | `ArrayBuffer.slice` copies (positive and negative indexes) equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-438 | Integration | `DataView` integer getters and setters, both byte orders equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-439 | Integration | `DataView` float getters and setters, both byte orders equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-440 | Integration | `DataView` over a window, its `buffer` and its bounds errors equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-441 | Integration | Views stay alive while their buffer is only reachable through them, under collection equal bun's (`platypus_ts_tests/tests/buffers.rs`, `bench/buffers.ts`). |
| M9-TS-442 | Unit | The host copies bytes into script as a typed array and reads them back; a trailing partial element is dropped (`web_ui_rt/src/typed.rs`). |
| M9-TS-443 | Integration | `structuredClone` of nested objects and arrays equals bun's (`platypus_ts_tests/tests/clone.rs`, `bench/clone.ts`). |
| M9-TS-444 | Integration | The clone is independent of the original (objects, arrays, nested fields) (`platypus_ts_tests/tests/clone.rs`). |
| M9-TS-445 | Integration | `structuredClone` keeps null fields null (`platypus_ts_tests/tests/clone.rs`). |
| M9-TS-446 | Integration | `structuredClone` of a recursive tree type (`platypus_ts_tests/tests/clone.rs`). |
| M9-TS-447 | Integration | `structuredClone` of arrays of arrays and of strings (`platypus_ts_tests/tests/clone.rs`). |
| M9-TS-448 | Integration | `structuredClone` of a typed array copies its elements (`platypus_ts_tests/tests/clone.rs`). |
| M9-TS-449 | Integration | `structuredClone` of numbers, strings, booleans and bigints (`platypus_ts_tests/tests/clone.rs`). |
| M9-TS-450 | Integration | 300 clones under collection (`platypus_ts_tests/tests/clone.rs`). |
| M9-TS-451 | Integration | `WeakRef.deref()` finds its target (`platypus_ts_tests/tests/weak.rs`, `bench/weak.ts`). |
| M9-TS-452 | Integration | `WeakRef<Derived>` typed and used (`platypus_ts_tests/tests/weak.rs`). |
| M9-TS-453 | Integration | An array of `WeakRef`s (`platypus_ts_tests/tests/weak.rs`). |
| M9-TS-454 | Integration | `WeakMap` get/set/has/delete by object identity (`platypus_ts_tests/tests/weak.rs`). |
| M9-TS-455 | Integration | `WeakSet` add/has/delete by object identity (`platypus_ts_tests/tests/weak.rs`). |
| M9-TS-456 | Integration | Graphemes of ASCII text equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-457 | Integration | Graphemes with combining marks equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-458 | Integration | Graphemes of emoji sequences, flags and skin tones equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-459 | Integration | Words of English text with punctuation, numbers and apostrophes equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-460 | Integration | `isWordLike` of word segments equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-461 | Integration | Sentences, including an abbreviation equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-462 | Integration | `segments.containing(index)` equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-463 | Integration | The default granularity is grapheme equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-464 | Integration | A bad granularity is a `RangeError` equal bun's (`platypus_ts_tests/tests/segmenter.rs`, `bench/segmenter.ts`). |
| M9-TS-465 | Integration | `Intl.Locale` language, script, region, base name equal bun's (`platypus_ts_tests/tests/locale.rs`, `bench/locale.ts`). |
| M9-TS-466 | Integration | Canonical form of mixed-case tags equal bun's (`platypus_ts_tests/tests/locale.rs`, `bench/locale.ts`). |
| M9-TS-467 | Integration | Unicode extension keywords (calendar, hour cycle, numbering system, collation, case first, numeric) equal bun's (`platypus_ts_tests/tests/locale.rs`, `bench/locale.ts`). |
| M9-TS-468 | Integration | Options (region, calendar, hour cycle) applied to a tag equal bun's (`platypus_ts_tests/tests/locale.rs`, `bench/locale.ts`). |
| M9-TS-469 | Integration | `maximize()` equal bun's (`platypus_ts_tests/tests/locale.rs`, `bench/locale.ts`). |
| M9-TS-470 | Integration | `minimize()` equal bun's (`platypus_ts_tests/tests/locale.rs`, `bench/locale.ts`). |
| M9-TS-471 | Integration | `maximize`/`minimize` keep extensions equal bun's (`platypus_ts_tests/tests/locale.rs`, `bench/locale.ts`). |
| M9-TS-472 | Integration | Invalid tags are `RangeError`s equal bun's (`platypus_ts_tests/tests/locale.rs`, `bench/locale.ts`). |
| M9-TS-473 | Integration | `Intl.DisplayNames` of regions equal bun's (`platypus_ts_tests/tests/dispnames.rs`, `bench/dispnames.ts`). |
| M9-TS-474 | Integration | Regions in French, German and Japanese equal bun's (`platypus_ts_tests/tests/dispnames.rs`, `bench/dispnames.ts`). |
| M9-TS-475 | Integration | Scripts equal bun's (`platypus_ts_tests/tests/dispnames.rs`, `bench/dispnames.ts`). |
| M9-TS-476 | Integration | Languages with dialects (`en-US` is American English) equal bun's (`platypus_ts_tests/tests/dispnames.rs`, `bench/dispnames.ts`). |
| M9-TS-477 | Integration | `languageDisplay: "standard"` equal bun's (`platypus_ts_tests/tests/dispnames.rs`, `bench/dispnames.ts`). |
| M9-TS-478 | Integration | Languages in French equal bun's (`platypus_ts_tests/tests/dispnames.rs`, `bench/dispnames.ts`). |
| M9-TS-479 | Integration | A missing `type` is a `TypeError` equal bun's (`platypus_ts_tests/tests/dispnames.rs`, `bench/dispnames.ts`). |
| M9-TS-480 | Integration | Temporal: date fields equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-481 | Integration | Temporal: date week years equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-482 | Integration | Temporal: date constrain equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-483 | Integration | Temporal: date add equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-484 | Integration | Temporal: date with equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-485 | Integration | Temporal: date until equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-486 | Integration | Temporal: date compare equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-487 | Integration | Temporal: date errors equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-488 | Integration | Temporal: date formats equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-489 | Integration | Temporal: time basics equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-490 | Integration | Temporal: time math equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-491 | Integration | Temporal: time compare equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-492 | Integration | Temporal: time errors equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-493 | Integration | Temporal: date time basics equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-494 | Integration | Temporal: date time math equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-495 | Integration | Temporal: date time diff equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-496 | Integration | Temporal: date to date time equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-497 | Integration | Temporal: duration basics equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-498 | Integration | Temporal: duration strings equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-499 | Integration | Temporal: duration math equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-500 | Integration | Temporal: duration total equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-501 | Integration | Temporal: duration errors equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-502 | Integration | Temporal: instant basics equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-503 | Integration | Temporal: instant offsets equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-504 | Integration | Temporal: instant math equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-505 | Integration | Temporal: instant errors equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-506 | Integration | Temporal: now is sane equal bun's (`platypus_ts_tests/tests/temporal.rs`, `bench/temporal.ts`). |
| M9-TS-507 | Integration | Temporal.ZonedDateTime: parse and print equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-508 | Integration | Temporal.ZonedDateTime: fields equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-509 | Integration | Temporal.ZonedDateTime: spring forward equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-510 | Integration | Temporal.ZonedDateTime: fall back equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-511 | Integration | Temporal.ZonedDateTime: convert equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-512 | Integration | Temporal.ZonedDateTime: arithmetic equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-513 | Integration | Temporal.ZonedDateTime: from fields equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-514 | Integration | Temporal.ZonedDateTime: errors equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-515 | Integration | Temporal.ZonedDateTime: now zoned equal bun's (`platypus_ts_tests/tests/zoned.rs`, `bench/zoned.ts`). |
| M9-TS-516 | Integration | Temporal.PlainYearMonth / PlainMonthDay: year month basics equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-517 | Integration | Temporal.PlainYearMonth / PlainMonthDay: year month math equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-518 | Integration | Temporal.PlainYearMonth / PlainMonthDay: year month diff equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-519 | Integration | Temporal.PlainYearMonth / PlainMonthDay: year month compare equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-520 | Integration | Temporal.PlainYearMonth / PlainMonthDay: year month conversions equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-521 | Integration | Temporal.PlainYearMonth / PlainMonthDay: year month errors equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-522 | Integration | Temporal.PlainYearMonth / PlainMonthDay: month day basics equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-523 | Integration | Temporal.PlainYearMonth / PlainMonthDay: month day math equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-524 | Integration | Temporal.PlainYearMonth / PlainMonthDay: month day errors equal bun's (`platypus_ts_tests/tests/monthday.rs`, `bench/monthday.ts`). |
| M9-TS-525 | Integration | Temporal rounding: time units equal bun's (`platypus_ts_tests/tests/rounding.rs`, `bench/rounding.ts`). |
| M9-TS-526 | Integration | Temporal rounding: time modes equal bun's (`platypus_ts_tests/tests/rounding.rs`, `bench/rounding.ts`). |
| M9-TS-527 | Integration | Temporal rounding: time increments equal bun's (`platypus_ts_tests/tests/rounding.rs`, `bench/rounding.ts`). |
| M9-TS-528 | Integration | Temporal rounding: date time equal bun's (`platypus_ts_tests/tests/rounding.rs`, `bench/rounding.ts`). |
| M9-TS-529 | Integration | Temporal rounding: instant equal bun's (`platypus_ts_tests/tests/rounding.rs`, `bench/rounding.ts`). |
| M9-TS-530 | Integration | Temporal rounding: zoned equal bun's (`platypus_ts_tests/tests/rounding.rs`, `bench/rounding.ts`). |
| M9-TS-531 | Integration | Temporal rounding: duration equal bun's (`platypus_ts_tests/tests/rounding.rs`, `bench/rounding.ts`). |
| M9-TS-532 | Integration | Temporal rounding: errors equal bun's (`platypus_ts_tests/tests/rounding.rs`, `bench/rounding.ts`). |
| M9-TS-533 | Integration | Intl.getCanonicalLocales: single equal bun's (`platypus_ts_tests/tests/canon.rs`, `bench/canon.ts`). |
| M9-TS-534 | Integration | Intl.getCanonicalLocales: list equal bun's (`platypus_ts_tests/tests/canon.rs`, `bench/canon.ts`). |
| M9-TS-535 | Integration | Intl.getCanonicalLocales: empty equal bun's (`platypus_ts_tests/tests/canon.rs`, `bench/canon.ts`). |
| M9-TS-536 | Integration | Intl.getCanonicalLocales: invalid equal bun's (`platypus_ts_tests/tests/canon.rs`, `bench/canon.ts`). |
| M9-TS-537 | Integration | formatToParts: decimal equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-538 | Integration | formatToParts: currency equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-539 | Integration | formatToParts: percent equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-540 | Integration | formatToParts: other locales equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-541 | Integration | formatToParts: signs equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-542 | Integration | formatToParts: compact equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-543 | Integration | formatToParts: scientific equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-544 | Integration | formatToParts: special equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-545 | Integration | formatToParts: date english equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-546 | Integration | formatToParts: date long equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-547 | Integration | formatToParts: date fields equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-548 | Integration | formatToParts: date other locales equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-549 | Integration | formatToParts: date hour12 equal bun's (`platypus_ts_tests/tests/fparts.rs`, `bench/fparts.ts`). |
| M9-TS-550 | Integration | resolvedOptions: date time format equal bun's (`platypus_ts_tests/tests/resolved.rs`, `bench/resolved.ts`). |
| M9-TS-551 | Integration | resolvedOptions: date time zones equal bun's (`platypus_ts_tests/tests/resolved.rs`, `bench/resolved.ts`). |
| M9-TS-552 | Integration | resolvedOptions: default locale equal bun's (`platypus_ts_tests/tests/resolved.rs`, `bench/resolved.ts`). |
| M9-TS-553 | Integration | resolvedOptions: number format equal bun's (`platypus_ts_tests/tests/resolved.rs`, `bench/resolved.ts`). |
| M9-TS-554 | Integration | resolvedOptions: number currency equal bun's (`platypus_ts_tests/tests/resolved.rs`, `bench/resolved.ts`). |
| M9-TS-555 | Integration | resolvedOptions: number options equal bun's (`platypus_ts_tests/tests/resolved.rs`, `bench/resolved.ts`). |
| M9-API-01 | Unit | The `#[ts_api]` manifest lists every `pub` function with its script name, Rust name, symbol, docs, parameters and result; private functions are left out (`crates/ts_api/tests/api.rs`). |
| M9-API-02 | Unit | The TypeScript declaration is generated from the manifest: `declare namespace`, parameter and result types, handle types, doc comments as JSDoc (`crates/ts_api/tests/api.rs`). |
| M9-API-03 | Unit | The JSON manifest names each symbol and type (`handle:GameObject`, `f64`) for the compiler (`crates/ts_api/tests/api.rs`). |
| M9-API-04 | Unit | The generated `extern "C"` shims call the functions with the C ABI, including `f32`, `i32`, `bool` and handles as raw ids (`crates/ts_api/tests/api.rs`). |
| ~~M9-API-05~~ | Unit | The generated QuickJS bindings install the namespace as a global with typed arguments and results (`crates/ts_api/tests/api.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-06~~ | Benchmark (ignored) | 1,000,000 typed QuickJS calls against the same through an `__platypus_op`-style queue; prints the time per call (`crates/ts_api/tests/api.rs`). (removed 2026-10-28: QuickJS removed) |
| M9-API-07 | Unit | Shims reach the world the host made current; with none they answer their defaults (`crates/ts_api/tests/context.rs`). |
| M9-API-08 | Unit | A nested call inside `context::with` finds no world instead of a second `&mut` (`crates/ts_api/tests/context.rs`). |
| M9-API-09 | Unit | Context scopes nest and restore the previous world (`crates/ts_api/tests/context.rs`). |
| M9-API-10 | Unit | The manifest, `.d.ts` class and JSON describe a value class (`crates/ts_api/tests/values.rs`). |
| M9-API-11 | Unit | The shims pass a value as scalars and return one through a pointer (`crates/ts_api/tests/values.rs`). |
| M9-API-12 | Unit | An `ObjectId` handle is never 0 (even index 0, generation 0) and converts back exactly (`crates/platypus_scene/tests/api.rs`). |
| M9-API-13 | Unit | The `Scene` shims create, parent, move and destroy objects in the current scene, stale ids miss, and with no scene they answer defaults (`crates/platypus_scene/tests/api.rs`). |
| M9-API-14 | Unit | A rotation is a `Quaternion` value; enabled/active flags and layers work through the shims (`crates/platypus_scene/tests/api.rs`). |
| M9-API-15 | Unit | The `Scene` manifest declares the `GameObject` handle and the `Vector3` and `Quaternion` values, and the generated `.d.ts` has the expected signatures (`crates/platypus_scene/tests/api.rs`). |
| M9-API-16 | Unit | Handle class functions are listed per class with the receiver first (`this`); a `set_x` next to `#[property] x` is the property's setter (`crates/ts_api/tests/classes.rs`). |
| M9-API-17 | Unit | The `.d.ts` has `declare class` with static, method and `get`/`set` members; a getter alone is read-only; a class replaces the plain handle alias (`crates/ts_api/tests/classes.rs`). |
| M9-API-18 | Unit | The class shims take the receiver first and call the methods, properties and statics (`crates/ts_api/tests/classes.rs`). |
| M9-API-19 | Unit | `GameObject` and `Transform` of `platypus_scene` work through their shims, a transform shares its object's handle, and the declarations have accessors (`crates/platypus_scene/tests/api.rs`). |
| M9-API-20 | Unit | Value class methods and getters (`#[method(Vector3)]`, `#[getter(Vector3)]` on free functions whose first parameter is the value) are declared inside the value class, with the receiver as `this` and passed as its fields (`crates/ts_api/tests/values.rs`). |
| M9-API-21 | Unit | Strings cross as script string objects (`&str` parameters, borrowed) and a returned buffer (`String` results); the manifest and `.d.ts` say `string` (`crates/ts_api/tests/strings.rs`). |
| M9-API-22 | Unit | A lone surrogate in a script string becomes one U+FFFD for the engine; a well formed string is borrowed, not copied; null is empty (`crates/ts_api/tests/strings.rs`). |

---

## M7 — Lighting

| ID | Kind | What |
|---|---|---|
| M7-FOG-01 | Golden | Distance fog (`WorldView.fog`): red cubes from 3 m to 60 m in four views, clear air, linear (none before the start, all fog past the end), exponential and exponential squared (clearer nearby than exponential, both thick far away); the background stays clear. |
| M7-FOG-02 | Integration | Orthographic cameras fog by depth along the view (two quads at one depth match wherever they are on screen, a deeper one is foggier), and fog covers quads, filled shapes and strokes. |
| M7-FOG-03 | Unit + JS unit | `render.fog` reaches the views of cameras that show it and `null` clears it; an unknown mode is an error; `Camera.fog` (on by default) opts out. `RenderSettings.fog` / `fogMode` / `fogColor` / `fogDensity` / `fogStartDistance` / `fogEndDistance` send the fog only while it's on; scenes save it and bring it back. |
| M7-FOG-04 | Golden + unit + JS unit | Height fog (`Fog.height`, exact exponential integral): a camera 4 m up over a floor sees pillars hazy at the foot and clear at the top, the floor fading to fog far off, and the background thick with fog just above the horizon but nearly clear 30° up. `render.fog`'s seventh argument and `RenderSettings.heightFog` carry it; scenes save it. |
| M7-FOG-05 | Golden + unit + JS unit | Light shafts (`Fog.volumetric`): a low sun through a slatted roof in exponential fog; without shafts the air in front of the back wall is evenly dim, with them it's brighter and banded by the slats' shadows. `render.fog`'s eighth argument (clamped, with an optional albedo) and `RenderSettings.volumetricFog` (defaults for missing fields) carry it; `Light.volumetricIntensity` (default 1, not below 0) goes out with `render.light`. |
| M7-FOG-06 | Golden + unit + JS unit | Fog volumes (`WorldView.fog_volumes`): a box of mist over a pond hides the lower part of a post behind it, the part above shows, a post beside it is clear, and its top edge is soft. `render.fogVolume` follows its object's position and scale, cameras with `fog` off hide volumes, `render.remove` drops them; the `FogVolume` component sends its size, density, colour, blend distance (not below 0) and shape (box or ellipsoid). |
| M7-FOG-07 | Integration + unit + JS unit | Fog culling (`Fog::cull`, `cull_distance`): a view with linear fog ending at 40 m draws the cubes at 10 and 30 m and the near quad and skips (and counts in `WorldStats::fog_culled`) the cubes at 50 and 200 m and the far quad; exponential squared at 0.02 is opaque at about 117.7 m; with culling off, no fog or no density nothing extra is culled. `FogReach` measures perspective views by distance and orthographic ones by depth. `RenderSettings.fogCulling` rides on `render.fog`; `__platypus_render_stats` fills `RenderStats`. |
| M7-FOG-08 | Golden | Spot and point lights in fog (`local_shafts`, worked out per pixel): in dark fog before a black wall, a spot light's cone is lit inside and dark just outside with a sharp edge right up to its tip, nothing above it; a point light glows all round, falling off from its middle. |
| M7-FOG-09 | Conformance + JS unit | Typed arrays cross the op boundary as number lists, read straight from their bytes (`Float32Array`, `Uint32Array`, views from `subarray`, `Uint8Array`); `Mesh.createFlat` / `setFlat` send flat lists (typed arrays as they are, UVs flipped into a `Float32Array`) and make plain arrays only when the mesh is saved. |
| M7-FOG-10 | Golden + JS unit | God rays (`VolumetricFog.sharp_distance`, `sharp_sun`): under a grate of thin bars, with the sun's shafts worked out per pixel near the camera the air ripples with thin beams the volume alone smooths away (the row's total variation at least 10% more); `volumetricFog.sharpDistance` / `sharpSamples` ride on `render.fog`. |
| M7-FOG-11 | Integration + JS unit | Temporal light volume (`cs_volume_light`, `volume::SIZES`): each cell takes one jittered sample a frame blended with its reprojected history, so under a grate of thin bars a row through the averaged volume ripples less than a single frame's (no stair steps); `volumetricFog.resolution` (low, medium, high, ultra) rides on `render.fog` (index 9), `denoising` (HDRP's mode, index 10: none / reprojection / gaussian / both; each cell's samples shifted by a hash of the cell, 64 a cycle, and the volume read through a four-tap filter, so blinds leave no moire or grain). |
| M7-FOG-12 | Integration | The light volume follows a turning sun (0.25 degrees a frame, a fast day) without trailing its old beams: after 60 frames of turning it differs from the same sun held still by less than half as much as a sun 2.5 degrees (10 frames) back would. |
| M7-FOG-13 | Integration + JS unit | The light volume's noise settings (`VolumetricFog.history_weight`, Unreal's history weight, and `samples` per cell per frame; `volumetricFog.historyWeight` / `samples`, op indices 11 and 12): with a still camera under a grate's beams, frame-to-frame change is nil without denoising (it steps instead), small with the defaults (1 sample, 0.9) and less than a third of that with 4 samples and 0.96. |
| M7-FOG-14 | Integration | A turning camera's beams stay close to a still camera's at the same view: each light-volume cell trusts its history less the further it moved (in cells), so a long history (0.96, 4 samples) smears no more than the default while turning (it smeared 40% more before); the history is read through a five-tap Catmull-Rom filter (sharp, so reprojecting doesn't blur); cells whose view moved take `motionSamples` (2) and smear less than with 1. |
| M7-FOG-15 | Integration | A spot light shining at a solid wall's back puts no haze on its front: the volume is read a slice and a half short of a surface (Unreal's depth bias), since the cells at its depth reach behind it (the shed's scanner showed its round cross-section through the planks). Shadowed spot and point lights shaft through the volume; a moving one's history fades by how far its beam swept. |
| M7-SPOT-01 | Integration | A pyramid spot light (HDRP's Pyramid shape: `outer_degrees` high, `aspect` times as wide in tangent) lights a wide, thin rectangle on a wall, dark above, below and past its sides; turned a quarter about its direction (`right` along y) it lights a tall, thin one. |
| M7-SPOT-02 | Unit | `Light.spotShape` (`cone` / `pyramid`) and `aspectRatio` (kept above 0) go out with the light's op. |
| M7-CLOUD-01 | Integration | A cloud layer (HDRP's) covers the background: looking up at a blue sky through dense distance fog (which the background ignores), an overcast deck shows grey. |
| M7-CLOUD-02 | Unit | `render.clouds` reaches every camera's view (`None` by default, `null` clears it); `advance_clouds` carries it on the wind (speed, direction from +x towards +z). |
| M7-CLOUD-03 | JS unit | `RenderSettings.cloudLayer` sends `render.clouds` with its defaults filled in; `null` clears it. |
| M7-CLOUD-04 | Unit | Native: a compiled game's `RenderSettings.cloudLayer` (`#[ts_api]` record `CloudLayerSettings` in `platypus_render_api`) reaches the views with the defaults for the keys left out and drifts on its wind; `null` clears it (`render_sync.rs`). |
| M7-EXP-01 | Integration + JS unit | Auto exposure (`AutoExposure`, `cs_meter` in bloom.wgsl): a dim view and one twenty times brighter settle to the same grey; switching from dim to bright dazzles on the first frame, then eases off over the next; `RenderSettings.autoExposure` rides on `render.post` (index 3). |
| M7-EXP-02 | Integration | Each camera adapts on its own: on one split screen a dim view and one twenty times brighter both settle to the same grey (state keyed by `WorldView.exposure_key`, each viewport metered and graded with its camera's). |
| M7-EXP-03 | Integration + JS unit | Histogram metering (`AutoExposure.low_percent` / `high_percent`, `metering`): averaging everything, a small blazing patch darkens a dim view; between 10% and 80% it's left out and the view shows as it would alone. |
| M7-EXP-04 | Integration + JS unit | Auto exposure blends with the manual exposure (`Grading.fixed_ev`, `transition`): turned off, it's halfway between adapted and manual half a transition later and manual after it; `fixedExposure` / `exposureTransition` / `freezeExposure()` ride on `render.post`. |
| M7-EXP-05 | Integration + JS unit | Scripts read each camera's exposure back (`WorldRenderer::exposure`, `__platypus_exposure` → `Camera.exposure` / `sceneBrightness`): a flat 0.05 view meters and is shown at EV100 log2(0.4); a reset (`reset_exposure`, `Camera.resetExposure()`) makes a cut to a bright view show settled at once where without it it dazzles. |
| M7-EXP-06 | Integration + JS unit | A soft floor (`dark_adaptation` / `dark_below`) and a compensation curve keep a very dark view darker than full adaptation; `QualitySettings.autoExposure` (the player's switch) turns auto exposure off and on. |
| M7-EXP-07 | Integration | A change of the manual exposure (`Grading.fixed_ev`) eases over `transition` (per camera, smoothstep): two stops darker over a second is partway at half a second and there after the second. |
| M7-RP-01 | Unit + JS unit | One render pipeline, as Unity 7 (URP; HDRP and Built-in go away): the manifest's `platypus.lightUnits` (`physical` or `legacy`, the default) reaches the renderer and scripts (`GraphicsSettings.lightUnits`); the old `platypus.renderPipeline` choice no longer picks anything, it only warns. |
| M7-RP-02 | Integration | A quality level's pipeline asset (`PipelineAsset`, Unity's URP asset per quality level): without light shafts a fogged view draws as if none were asked for, and a cap of 32 additional lights draws the nearest 32 of 40 point lights where the default draws all 40. |
| M7-RP-03 | Integration + JS unit | The asset's other settings: with HDR off a bright view clips at white instead of being tonemapped; without additional light shadows a shadowed spot light gets no shadow map. Each quality level carries them (`QualitySettings.volumetricFog`, `hdr`, `additionalLights`, `additionalLightShadows`; Low, the phones' tier, has no light shafts and 8 additional lights without shadows), sent with `render.quality`; the manifest's default level can go by platform (`platypus.quality.default: { Android: "Low", default: "High" }`). |
| M7-HD-01 | Integration + Unit | Real-world light units (`platypus.lightUnits: "physical"`, `WorldRenderer::physical_units`): directional lights' intensity is read as lux (renderer units lux / pi), spot and point lights' as lumens (/ 4 pi²); everything drawn is pre-exposed by the camera's last exposure and the post pass divides it out, so a view at 200000 nits (past fp16's 65504) settles to the same grey as one at 2 nits (the histogram reaching +32 stops). |
| M7-HD-02 | JS unit | A physical camera (`Camera.aperture`, `shutterSpeed`, `iso`, HDRP's defaults f/16, 1/200 s, ISO 200) gives `physicalExposure` = log2(N²/t) - log2(ISO/100); `Light.illuminanceAt(point, normal)` sums lux from every light (lumens to candela over 4 pi, inverse square, cosine); `Light.measureIlluminance` also counts shadows by physics rays (checked by eye in `examples/light-units`: 8 lux in the hall, 81438 in the sun). |
| M7-FX-01 | Unit | `PostProcess`'s built-in table (`EFFECTS`): the script's `PostEffect` names equal the table's, every effect has an `effect` function, at most eight parameters and valid WGSL names. |
| M7-FX-02 | Unit | Passes come out enabled only, ordered by `order` then creation, with their camera; a parameter change bumps `post_version`. |
| M7-FX-03 | Unit | `PostProcess.createCustom` parameters (`"name"`, `"name:vec3"`, `"name:color"`): bad, repeated and extra ones are left out; values land in their slots; a source using `scene_depth` / `scene_normal` is marked as reading depth. |
| M7-FX-04 | Golden + GPU | Every built-in effect on a scene of receding balls: vignette darkens the corners and not the middle; chromatic aberration leaves the middle and fringes the edges; blur softens; pixelate makes constant blocks; film grain moves pixels; grading with saturation 0 is grey and a warm balance is warmer; outline inks silhouettes; CRT cuts the corners; posterize bands; tilt-shift softens off its band; depth of field softens the near ball and leaves the focused one. Contact sheet golden. |
| M7-FX-05 | Golden + GPU | `scene_depth` (metres along the view, background far) and `scene_normal` (world space, rebuilt from depth, zero on the background) reach a custom shader. |
| M7-FX-06 | GPU | A custom WGSL pass runs (inverted light); a source with no `effect`, a type error or a syntax error is logged and skipped, the picture unchanged and later passes still running. |
| M7-FX-07 | GPU | Passes run in order (pixelate then blur differs from blur then pixelate); with two cameras side by side an effect on the left camera stays in its half and the right half is untouched. |
| M7-FX-09 | Integration | A script's `PostProcess.create` / `createCustom` passes reach each camera's `WorldView.post`: disabled ones left out, ordered by `order`, one aimed at a camera only on that camera's view, parameters in their slots, and the renderer's shader is the same one next frame (its pipeline stays cached). |
| M7-FX-08 | GPU | Depth-reading passes work with 4x MSAA (multisampled depth) and behind exposure / tonemapping (the HDR path). |

---

## Later milestones (outline)

- **Networking:** `@sync` fields replicate; `@rpc.server` runs only on the server; prediction and reconciliation under simulated latency and packet loss.
- **Android / Quest:** app launches, renders, survives pause/resume and rotation.
- **VR:** stereo render at the headset's refresh rate; world panel interaction with controllers.
- **iOS:** QuickJS backend passes the full M1-SCR suite; app passes TestFlight.
- **Packaging:** packed build runs from a clean machine with no dev tools installed.
| M9-TS-556 | Integration | The generated `classes()` table lists the registered classes and only the hooks each has (`platypus_ts_tests/tests/components.rs`, `bench/components.ts`). |
| M9-TS-557 | Integration | A compiled component moves its GameObject every frame through the real scene; fields persist; all GC modes. |
| M9-TS-558 | Integration | Host order: `onStart` once, every `onUpdate` before any `onLateUpdate`. |
| M9-TS-559 | Integration | Subclasses inherit hooks and override one; inactive objects are skipped; `destroy` runs `onDestroy`. |
| M9-TS-560 | Integration | A throwing hook disables its component and reports it; the others run; an unknown class is an error. |
| M9-TS-561 | Integration | `fixed_update` reaches only classes with `onFixedUpdate`. |
| M9-TS-562 | Integration | Generics `identityTypes` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-563 | Integration | Generics `firstOf` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-564 | Integration | Generics `mapping` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-565 | Integration | Generics `pairs` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-566 | Integration | Generics `fills` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-567 | Integration | Generics `reducing` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-568 | Integration | Generics `boxes` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-569 | Integration | Generics `stacks` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-570 | Integration | Generics `lists` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-571 | Integration | Generics `subclass` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-572 | Integration | Generics `aliases` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-573 | Integration | Generics `nested` equals bun (`platypus_ts_tests/tests/generics.rs`, `bench/generics.ts`). |
| M9-TS-574 | Unit | Generic mistakes are errors with a reason: cannot infer, exported generic, conflicting inference, generic static or accessor. |
| M2-NAT-01 | Integration | Native reconciler: a description becomes nodes in a real `UiTree` (`crates/web_ui_react/tests/reconcile.rs`). |
| M2-NAT-02 | Integration | State setter re-renders only its component, text updated in place; same value renders nothing. |
| M2-NAT-03 | Integration | Keyed children keep their nodes when reordered, inserted and removed. |
| M2-NAT-04 | Integration | Style diff sends what changed; a removed property resets. |
| M2-NAT-05 | Integration | Components and fragments place their nodes in the container; a component switching output replaces in place. |
| M2-NAT-06 | Integration | Effects and cleanups run in real React's order (log from `js/react/test/native_oracle.test.tsx`). |
| M2-NAT-07 | Integration | `useRef`/`useMemo`/effect without deps keep values and run as React does. |
| M2-NAT-08 | Integration | Portals mount under another node; events bubble through the component tree. |
| M2-NAT-09 | Integration | Capture then bubble, `stopPropagation`, `preventDefault`, state set in a handler is committed before dispatch returns. |
| M2-NAT-10 | Integration | Refs set and cleared, context follows the provider, unmount cleans up and frees. |
| M2-NAT-11 | Integration | An effect that sets state every render is reported after 100 chained updates. |
| M2-NAT-12 | JS unit | Real React's effect/cleanup order for mount, update, removal, replacement and unmount (`js/react/test/native_oracle.test.tsx`). |
| M9-TS-575 | Integration | A TSX counter (useState, style, onClick, text and number children) renders into a real UiTree and handles clicks (`platypus_ts_tests/tests/ui.rs`, `bench/ui.ts`). |
| M9-TS-576 | Integration | A keyed list from `items.map(..)` keeps its nodes when reversed. |
| M9-TS-577 | Integration | `useEffect` with dependencies and cleanup: +1, no rerun, -1 +2, cleanup on unmount. |
| M9-TS-578 | Integration | `useMemo` recomputes only when its dependencies change. |
| M9-TS-579 | Integration | Boolean and string state and `useRef` persist across renders; all GC modes. |
| M9-TS-580 | Integration | Context: a consumer reads the nearest provider, the default without one, and follows the provider (`platypus_ts_tests/tests/ui.rs`). |
| M9-TS-581 | Integration | `ref={r}` writes the node into the ref on attach (an effect sees it). |
| M9-TS-582 | Integration | `useImperativeHandle` fills the parent's ref before the parent's effect runs. |
| M9-TS-583 | Integration | `useSyncExternalStore` follows a store, subscribes once, unsubscribes on unmount. |
| M9-TS-584 | Integration | `memo` skips a child with equal props and renders it when a prop changes. |
| M9-TS-585 | Integration | `forwardRef` passes the ref as the second parameter. |
| M9-TS-586 | Integration | `useReducer` applies actions to the latest state (two dispatches in one handler both count). |
| M2-NAT-13 | Integration | A controlled text field: an accepted (even transformed) edit shows the new value prop, a rejected one sends the old value back (`crates/web_ui_react/tests/reconcile.rs`). |
| M9-TS-587 | Integration | The compiler exports each component's stable id and ordered hook list (`__uiSignatures`). |
| M9-TS-588 | Integration | Hot reload with `Runtime::refresh`: state is kept, except for components in the remount list. |
| M9-TS-589 | Integration | The updater form `setN(n => n + 1)`: two updaters in one handler apply to the latest state (`platypus_ts_tests/tests/ui.rs`). |
| M9-TS-590 | Integration | `style={obj}` with a typed object sets its fields; an optional field left out is not set. |
| M9-TS-591 | Integration | `{...obj}` on a host element sets attributes; on a component it spreads into the props. |
| ~~M9-API-23~~ | Integration | QuickJS bindings: a value class is a plain object (arguments read from it, results new objects) (`crates/ts_api/tests/quickjs.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-24~~ | Integration | QuickJS bindings: `&str`/`String` are JS strings (non-BMP and accents). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-25~~ | Integration | A value object missing a field is a catchable JS exception. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-26~~ | Integration | The `Scene` namespace of `platypus_scene` works from QuickJS on the current scene (`crates/platypus_scene/tests/api.rs`). (removed 2026-10-28: QuickJS removed) |
| M9-TS-592 | Unit | A class field's type is inferred from constants, arithmetic, `C.x` of a constant object and object literals; a call is still an error asking for an annotation. |
| M9-API-27 | Integration | Namespace properties: a `#[property]` function without a receiver is a getter, `set_x(v)` its setter; manifest kinds, `.d.ts` (`let` / `const`) and JSON (`crates/ts_api/tests/ns_props.rs`). |
| M9-API-28 | Integration | The shims of namespace properties take no receiver. |
| ~~M9-API-29~~ | Integration | QuickJS gets accessors for namespace properties (`Cfg.level = 9`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-30~~ | Integration | `Color` (`crates/platypus_engine_api/tests/engine.rs`): constants, `hex` (sRGB to linear), `lerp`, `toHex`, `withAlpha`. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-31~~ | Integration | `Time` follows the host's frames and the time scale (a bad scale reads as 1). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-32~~ | Integration | `input` reports this frame's presses and releases; key repeats are not presses. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-33~~ | Integration | `Screen` and `QualitySettings` are read and set. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-34~~ | Integration | `Gizmos` collect lines and present them to a camera; the host takes the frame once. (removed 2026-10-28: QuickJS removed) |
| M9-API-35 | Integration | `Camera` defaults, field of view clamp, orthographic settings, removal with its object. |
| ~~M9-API-36~~ | Integration | Value classes are JS classes in QuickJS (constructor, methods, getters, instances as results) (`crates/platypus_scene/tests/api.rs`). (removed 2026-10-28: QuickJS removed) |
| M9-TS-593 | Integration | `Color` statics, `new Color`, `lerp`/`withAlpha`/`toHex` work in compiled code (`platypus_ts_tests/tests/engine2.rs`, `bench/engine2.ts`). |
| M9-TS-594 | Integration | Namespace properties (`Time.timeScale = ..`, `Time.deltaTime`, `Screen.fullScreen`, `QualitySettings.vSyncCount`) and `input` work in compiled code. |
| M9-TS-595 | Integration | `Camera.attach(go)` with property assignments, and `Gizmos` (static colour, `drawLine`, `present`) work in compiled code. |
| M9-TS-596 | Integration | The whole `SineDemo` component (copied from `9dc2f51`) compiles and wakes up: camera made with `new GameObject(..)` / `addComponent(Camera, {..})`, music loaded with options, the curve drawn as 1,083 gizmo lines (`platypus_ts_tests/tests/sine_demo_full.rs`). |
| M9-TS-597 | Integration | Keys drive the compiled demo: Space plays the music looped and the tweens move the curve, a digit picks a formula, F5 adds the 24-line centre ring. |
| M9-TS-598 | Integration | `boolean \| undefined` fields: `??`, truthiness, assignment (`bench/nullbool.ts`, bun-compared). |
| M9-TS-599 | Integration | Optional boolean parameters and `=== undefined` (bun-compared). |
| M9-TS-600 | Integration | Optional boolean members of a class (bun-compared). |
| M9-TS-601 | Integration | `NativeHost::properties`: each class of the generated table carries its `@property` list (name, type); a class without any has none (`tests/engine/components2.rs`). |
| M9-TS-602 | Integration | `get_property` / `set_property` through the table: numbers, nullable numbers, booleans, strings (collected between write and read); a wrong kind is an error naming the property and changes nothing. |
| M9-TS-603 | Integration | `save_properties` / `load_properties`: values saved from one component load into another, component references travel as ids; bad entries are reported and the rest apply. |
| M9-TS-604 | Integration | `this.getComponent(X)`, `go.addComponent(X, { .. })` between compiled components: the sibling is found, the added component has its options and runs `onAwake`, a missing class is `undefined`, destroying the object destroys it. |
| M9-TS-605..610 | Integration | `Rng` (`new Rng(seed?)`, `nextDouble`, `next`, `float`, `int`, `bool`) and the shared `Random`: mulberry32 sequences equal the original `random.ts` run in bun (`bench/random.ts`, `tests/engine_api/random.rs`). |
| M9-TS-611..614 | Integration | `LayerMask`: built-in and game layer names (renaming, out-of-range and empty names ignored), `getMask`, `contains`, `everything` / `nothing`, compared with `layers.ts` in bun. |
| M9-TS-615..616 | Integration | `PlayerPrefs`: typed reads with and without fallbacks, truncating `setInt`, `hasKey`, `deleteKey`, `deleteAll`, compared with `prefs.ts` in bun. |
| M9-TS-617..620 | Integration | `Assets`: GUID and path lookups (backslashes and `./` cleaned), `guid:` sources, importer JSON, enumeration, compared with `assets.ts` in bun through a prelude that adapts the shape. |
| M9-TS-621..623 | Integration | `Application` and `SystemInfo` read in compiled code against the manifest and machine the host installs, settable `targetFrameRate` / `runInBackground`, compared with `app.ts` in bun. |
| M9-TS-624..626 | Integration | `TimeSince`, `TimeUntil` and `Time` after three engine ticks, compared with `clock.ts` in bun. |
| ~~M9-API-37~~ | Integration | `Random` in QuickJS gives the TypeScript generator numbers for a seed; an omitted `bool` chance defaults to 0.5. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-38~~ | Integration | `LayerMask` in QuickJS: names, masks, `contains`. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-39~~ | Integration | Log lines reach the host sink with their level; `Debug.assert` logs only when false. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-40~~ | Integration | `PlayerPrefs` in QuickJS, and the ordered changes the host writes to the file. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-41~~ | Integration | `Assets` in QuickJS: lookups, `guid:` sources, importer JSON, polled changes. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-42~~ | Integration | `Application` / `SystemInfo` in QuickJS, script writes returned as requests, focus and pause changes. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-43~~ | Integration | `TimeSince` / `TimeUntil` value classes in QuickJS follow the game clock. (removed 2026-10-28: QuickJS removed) |
| M9-TS-627..636 | Integration | `input` after the engine delivered events and ticked: keys held / pressed / released, mouse position, delta and wheel, mouse buttons as keys, `getAxis` (keys, stick, mouse), `getButton` / `getButtonDown`, gamepads by id; a second program checks the frame after (a key released, a pad gone, a blur), compared with `input.ts` in bun (`inputs`, `inputs2`). |
| M9-TS-637..642 | Integration | `TimeSpan`: `new TimeSpan(ms)` rounding, `of`, `fromX`, parts and totals (negative too), arithmetic, comparison, `toString` constant and custom formats, `parse`, compared with `time.ts` in bun. |
| M9-TS-643..650 | Integration | `DateTime` in New York time: fields, epoch and kind, every standard and custom format, month / year / day arithmetic with clamping, daylight-saving gaps and overlaps, field carrying (`new DateTime(2024, 13, 1)`), ISO parsing, statics, compared with `time.ts` in bun. |
| ~~M9-API-44~~ | Integration | `input` in QuickJS follows the tick (`time::advance`): events become this frame, the stick beats the keys when pushed further, pads are read by id, a blur clears held keys. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-45~~ | Integration | `TimeSpan` and `DateTime` as JavaScript classes in QuickJS: `new X(..)` goes through `create`, members and statics work. (removed 2026-10-28: QuickJS removed) |
| M9-TS-651 | Integration | `Task.delay`, `Task.delaySeconds` and `Task.frame` in an async function wait on the runtime timers advanced with engine time (`tests/engine/task.rs`). |
| M9-TS-652..660 | Integration | `Mathf` (constants, interpolation, stepping, angles, comparisons, powers of two, Perlin noise, colour space, `smoothDamp`): every number printed in full equals the original `math.ts` in bun (`bench/mathf.ts`). |
| M9-TS-661..664 | Integration | `Vector2`, `Vector3`, `Vector4` (all members and statics, four values each paired with every other) and `Vector2Int` / `Vector3Int`, bit-for-bit equal to `math.ts` (`bench/vectors.ts`; `Math.hypot` of three or more numbers uses the native algorithm in both, see `docs/platypus-api.md`). |
| M9-TS-665..670 | Integration | `Quaternion` (construction, `euler` both forms, products, `slerp`, `rotateTowards`, `eulerAngles` over a grid, `lookRotation`, `fromToRotation`) and `Matrix4x4` (`trs`, `mul`, `inverse`, `determinant`, projections, point and vector transforms, `fromColumns`), bit-for-bit equal to `math.ts`. |
| M9-TS-671..674 | Integration | `Rect` / `RectInt`, `Ray`, `Plane` (NaN where the original answers null), `Bounds` and `Pose`, bit-for-bit equal to `math.ts`. |
| ~~M9-API-46~~ | Integration | The math types as JavaScript classes in QuickJS: `new X(..)` through `create`, instances from methods, statics and getters (`crates/platypus_math/tests/quickjs.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-47~~ | Integration | Overloads by argument count in QuickJS (`Quaternion.euler` two forms, `lookRotation`, `Matrix4x4.trs`), and the shapes (`Ray`, `Plane`, `Bounds`, `Pose`). (removed 2026-10-28: QuickJS removed) |
| M9-TS-675 | Integration | The vector-valued members of `Rng` and `Random` (`insideUnitCircle`, `onUnitSphere`, `insideUnitSphere`, `rotation`) equal the original in bun, bit for bit. |
| M9-TS-676..683 | Integration | Animation curves, clips and sampling (`Keyframe`, `AnimationCurve`, `AnimationClip.sampleAnimation`): Hermite evaluation over a custom curve and steps, wrap modes before and after, key editing and indexes, clip length / curve replacement / removal / events, sampling a three-level rig (position, scale, quaternion and Euler channels, `GameObject.active`, a missing path), clip wrap modes, JSON round trip; bit-for-bit equal to `animation.ts` run in bun over `scene.ts` (`bench/anim_clips.ts`). |
| M9-TS-684..691 | Integration | The `Animation` component: named states and `play` / `stop` / `isPlaying`, state properties (`time`, `speed`, `weight`, `normalizedTime`), `crossFade` weights over time, wrap modes of a state (once holds the last frame then stops), clip events (loop passes, once), the default clip and `playAutomatically`, animating `Camera` properties (`fieldOfView`, `backgroundColor.r`, `orthographic`) through the property registry, blending two clips with sign-aligned quaternions, objects inactive in the hierarchy; bit-for-bit equal to `animation.ts` in bun (`bench/anim_player.ts`). |
| M9-TS-692..697 | Integration | The `Animator` and its controller made with the chained builder and with the explicit classes (identical output): parameters, states, transitions on conditions and exit time, triggers used up, `Any` transitions, blend trees with time scales, `play` / `crossFade`, default state, offsets, JSON round trip; bit-for-bit equal to `animation.ts` in bun (`bench/anim_animator.ts`). |
| M9-TS-698..702 | Integration | GameObject and Transform members against `scene.ts` in bun (`bench/scene_objects.ts`): hierarchy (children, `findChild`, `root`, `destroyed`, roots, `hideFlags`, `byId`), world position / rotation / scale / Euler angles composed down the parent chain, `forward/right/up`, `translate` / `rotate` / `lookAt`, `transformPoint` and its inverse, `setParent` keeping the world pose; numbers to three decimals (the scene stores floats). |
| M9-TS-703..706 | Integration | Component lookups against `scene.ts` in bun (`bench/comp_query.ts`): `getComponent` finds subclasses, `getComponents`, `getComponentInChildren`, `GameObject.findObjectOfType`, `removeComponent` / `destroy()`, `transform` / `gameObject` on components; three GC modes. |
| M9-TS-707..710 | Integration | Component lifecycle (`bench/comp_life.ts`): `onDestroy` after removal runs when the host settles, add+remove in one hook never wakes, `onApplicationFocus/Pause` flags (inactive skipped), destroyed objects take their components (and children's) with them. |
| M9-TS-711 | Integration | `go.getComponent(Camera)` / `Animation` / `Animator` finds the engine component on that object only (static `findOn`). |
| M9-TS-712..718 | Integration | Scenes for compiled programs (`bench/scenes.ts`, `platypus_scenes`): a scene file loads (objects, transforms, layer, enabled, `Camera`, the program's components with number, `Vector3` and reference properties; unknown kinds warn; components wake at `settle`); save and load round trip; `GameObject.instantiate` with child remapping and `{ position, rotation, parent }`; references outside the copy stay; additive / single loads, unload, `dontDestroyOnLoad`; prefabs; the `examples/scenes` level file. |
| M9-TS-719 | Integration | An engine object (handle) stringifies as `{}`; script fields beside it are kept. |
| M9-TS-720..722 | Integration | Stack values (`web_ui_ts::sroa`): a vector built, combined and read in one function is not allocated; one that is returned or stored stays an object; aliases that change stay one object. |
| M9-TS-723..725 | Integration | Value classes (`Vector3`, `Color`, `Quaternion`, ...) stringify compactly as their numbers (`[1,2,3]`) and parse back from that or from the object form (`{"x":1,..}`, left-out fields 0, alpha 1) (a difference to the TypeScript classes, by decision). |
| M9-TS-726 | Integration | A `Vector2[]` is passed to the engine in place (`AnimationCurve.fromPoints(points, mode?)`; modes linear / step; default linear). |
| M9-API-48 | Unit | `script_f64s` reads a script `number[]` in place; null and empty give an empty slice. |
| M9-TS-727..730 | Integration | Native physics from compiled code (`bench/physics.ts`): a ball falls and rests on a floor; a ray answers at once (distance, point, normal; a miss); an attached body moves its GameObject; gravity, impulse with mass, body kind. |
| M9-TS-1000..1003 | Integration | Contact hooks on compiled components (`bench/contacts.ts`, `tests/engine/contacts.rs`): a collision tells both objects (other object, impact speed); Stay while touching and Exit (no contact points) when it ends, inherited hooks included; sensors call the trigger hooks; hand-made events skip inactive objects, a hook that destroys its component or throws is safe and reported. |
| M9-TS-1004..1005 | Integration | Unity's `Collision` in compiled hooks (`bench/contacts.ts`): the object hit, its `Collider` (`isTrigger`, `gameObject`), `rigidbody` (null for a static floor), `transform`, `relativeVelocity`, contact points at the floor with normals towards the object told, `impulse`, `contacts` and `getContact(i)` agreeing; a trigger hook gets the other `Collider` (`attachedRigidbody`, `bounds`). The same for `Collision2D` (`otherCollider`, `otherRigidbody`, `normalImpulse`, `enabled`) and `Collider2D`. |
| M9-TS-1006 | Integration | The concrete 2D colliders (`BoxCollider2D`, `CircleCollider2D`, ...) name their body `attachedRigidbody`, as the `Collider2D` base and Unity do (it was `attachedRigidbody2d`): a ball's own Rigidbody2D, none for a floor's BoxCollider2D (`bench/contacts.ts`). |
| M9-TS-1010..1013 | Integration | The component / GameObject surface of scene.ts in compiled code (`bench/comp_api.ts`): `children`, `root`, `parent`, null tests of handles; `components`; `GameObject.roots`, `find`, `byId`; `go.scene`. |
| M9-API-200..209 | Unit | Physics components (`crates/platypus_physics_api/src/tests.rs`): Rigidbody + BoxCollider rest on a floor; a kinematic object moved by script carries its body; colliders remade at the object's scale; centers, triggers and `collider_info` for contacts; fixed and rope joints; a Rope hangs; a CharacterController lands and is grounded; 2D bodies keep z and a 2D ray names its object; kinds save/load/copy/destroy; destroyed objects lose their physics. |
| M9-API-210 | Unit | Raycasts (`crates/platypus_physics_api/src/tests.rs`): a layer mask keeps only the colliders on its layers, a hit names its GameObject, a collider the origin starts inside is skipped (Unity's rule), and an infinite or default distance reaches far targets. |
| M9-API-211 | Unit | Tags: objects are "Untagged" until tagged, `find_with_tag` lists the active objects with a tag in scene order (`crates/platypus_scene/tests/api.rs`); `M9-API-212` (`crates/platypus_scenes/src/kinds_tests.rs`): tags save to and load from scene files and survive copies. |
| M9-API-214 | Unit | `Physics.raycastAll` lists every collider on the ray nearest first (layer mask, start-inside rule) and `overlapSphere` lists the colliders a sphere touches, by layer (`crates/platypus_physics_api/src/tests.rs`). |
| M9-API-213 | Unit | Transform hierarchy like Unity (`crates/platypus_scene/tests/api.rs`): `getChild`, `find("a/b")`, `root`, `isChildOf`, sibling order, vectors through a scaled parent, `rotateAround`, `setParent` keeping the world scale, `detachChildren`. |
| M9-TS-707 | Integration | `getComponentsInChildren`, `getComponentInParent` and `getComponentsInParent` against `scene.ts` in bun (`bench/comp_query.ts` `hierarchy`); three GC modes. |
| M9-TS-1100..1105 | Integration | Physics components from compiled code (`bench/physics_comp.ts`): the component world matches the same ops through `platypus_physics::Physics`; options round-trip through `getComponent`; `ForceMode.Impulse`; FixedJoint and Rope; CharacterController; 2D components, `Physics2D.raycast` and gravity. |
| M9-TS-1200..1207 | Integration | Closures given to the engine (`bench/callbacks.ts`): `input.onKeyDown/onKeyUp/onGamepad*` get codes, flags, ids and values and are removed by `input.off(token)`; a listener keeps captured variables; a Rust listener may unsubscribe inside an event; `Assets.onChanged`, `Application.onFocusChanged` (only on a change) and `SceneManager.onSceneLoaded/Unloaded/ActiveSceneChanged` (scene handles, mode text); all in the normal, stress and no-GC modes. |
| M9-API-300..302 | Unit | `Callback<(..)>` parameters: manifest `fn(f64,string):void` and the declaration `(a: number, b: string) => void`; listener tokens unique and ordered; a script callback without a runtime bridge is ignored. |
| M9-TS-1210..1219 | Integration | Native audio from compiled code (`bench/audio_native.ts`): `audio.load/play/createBus/createEmitter/setListener/setMasterVolume`, `Sound.play({..})` options by name, playback changes, `onEnded`, emitters; `AudioSource` (flat and 3D with an emitter that follows the object, one shots, pause/stop, `playClipAtPoint`) and `AudioListener`; destroying an object stops its sources; scene files load and save both components and play on awake. The commands the host runs on the mixer are checked as `audio.*` ops. |
| M9-TS-1220..1227 | Integration | Rendering data API from compiled code (`bench/render_native.ts`, `platypus_render_api`): `Light`, `MeshRenderer` (primitive meshes by name, script `Mesh`, `Material`), `Texture` (file, pixels, render texture) with camera extras, `Sprite` / `SpriteRenderer`, `LODGroup`, `Mirror`, `ReflectionProbe`, `PlaneRenderer`, `WorldPanel`, `LineRenderer2D`; the example `level1.scene.json` loads its Light, MeshRenderer, inline Material and camera settings and saves them back; the `assets` table (shared textures, meshes, render textures, inline sprites) loads once per entry and round-trips. |
| M9-TS-1228..1230 | Integration | `RenderSettings` (ambient light, procedural sky, bloom, tonemapping; unknown names ignored) and `Cursor` / `CursorLockMode` from compiled code; `CameraFollow` and `CameraOrbit` move their objects when the host runs `late_update_camera_follows` / `late_update_camera_orbits`; a scene file applies its `renderSettings` when it becomes active and saves the current ones. |
| M9-TS-1231..1235 | Integration | `ParticleSystem` (ranges, shape, bursts, curves, flipbook; play/stop/emit commands for the host; `report_particles` runs `onStopped` and `stopAction: "destroy"`), `LineRenderer` (a tube through a mesh renderer of its own, rebuilt on change), `Cloth` (swaps the renderer mesh for a simulated copy and back) and `Primitives.create` (collider hook); all three load from scene files and save the same data again. |
| M9-TS-1236..1238 | Integration | `Gizmos` shapes (ray, wire cube / disc / sphere, frustum, `matrix`, `lineCount`) from compiled code; `Screen` (modes, `setResolution` requests for the host, resolutions, `onChange` listeners run when the host updates the size) and `QualitySettings` (levels from the manifest, validation of names and numbers, level switches tell listeners, the choice is saved as `platypus.quality` and comes back). |
| M9-TS-1239 | Integration | `Model.load` / `instantiate` (options `position`, `rotation`, `scale`, `parent`, `colliders`; an object literal with those keys is the options, not a value): the file is read once, copies asked for before the host reports the description get their nodes when it does (renderers per primitive, glTF materials, colliders through a hook); a scene saves a copy as `"model"` without its nodes and builds them again on load. |
| M9-TS-1500..1510 | Integration | JSX and hooks need no manifest from the caller (`tools/web_ui_ts/tests/jsx_builtin.rs`): a component with no manifests or with another one, a hook-only program (single line), fragments, `{cond && <x/>}`, `.map` children, spread props, text children and conditionals, a JSX syntax error reports its line and the TSX parser's message, TypeScript that is not TSX is not blamed on the TSX parse, an explicit `Ui` manifest is still accepted. |
| M9-TS-1511 | Integration | The `Ui` manifest embedded in `web_ui_ts` equals `web_ui_bridge::manifest_json()` (`tools/web_ui_bridge/tests/embedded_manifest.rs`). |
| ~~M9-API-100~~ | Integration | A handle in QuickJS is an opaque object: the id is in an internal slot, `JSON.stringify` gives `{}`, no own or enumerable properties, `[object Thing]`, `instanceof`, script-added properties are own (`crates/ts_api/tests/handles_quickjs.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-101~~ | Integration | Handle class statics, methods, properties, setters and `new Class(args)` (through `create`) in QuickJS; a getter alone is read only. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-102~~ | Integration | The id 0 is `null` going out and coming in; optional handle parameters default to `null`. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-103~~ | Integration | A handle of the wrong class, a number or a plain object, a detached method and `new` on a class without `create` are `TypeError`s. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-104~~ | Integration | Modules share one class per handle name; a class named like its namespace is the namespace object. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-105~~ | Integration | Methods with more than four parameters, value results, `&[f64]` and `&[V]` parameters from JS arrays (objects, arrays or instances as elements). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-106~~ | Integration | The same calls from compiled shims (raw ids) and QuickJS agree. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-107~~ | Integration | QuickJS value classes: `toJSON` is the compact array, `X.fromJSON` reads the array (exact length) or the object form (left-out fields 0, alpha 1), and functions accept arrays. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-108~~ | Integration | `GameObject`, `Transform` and `Camera` as handle classes over the real scene (`crates/platypus_engine_api/tests/handles_quickjs.rs`); classes with one id are not interchangeable. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-109~~ | Integration | Handles made by a script and read by the shims (and the reverse), and a handle made by the host with `handle_new`. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-110~~ | Integration | `AnimationCurve` factories, `fromPoints` from an array of vectors, methods on the returned handle. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-111~~ | Integration | `SceneManager`, `Scene` and `Prefab` as handle classes (`crates/platypus_scenes/tests/handles_quickjs.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-112~~ | Integration | A namespace and a class with the same name (`Scene`) merge, whichever is registered first. (removed 2026-10-28: QuickJS removed) |
| M9-API-113 | Integration | `QuickJsRuntime::add_native` installs a native API and reinstalls it after `reset()` (`crates/platypus_script/tests/native.rs`). |
| M9-API-120 | Integration | Every function of every manifest (namespaces, class statics, methods, properties, value classes) exists in QuickJS after `platypus_native::register_quickjs` (`crates/platypus_native/tests/surface.rs`). |
| ~~M9-API-121~~ | Integration | Methods of one class with the same script name dispatch by argument count (`this` is passed on); no version for the count is a `TypeError`. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-122~~ | Integration | A JS function passed as a `Callback` is kept (survives scripts ending and a collection), called with the event's arguments, and `off(token)` removes it (`crates/ts_api/tests/callbacks_quickjs.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-123~~ | Integration | Handle and boolean callback arguments reach JS as handle objects / `null` and booleans; a callback with no arguments. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-124~~ | Integration | An exception in a JS callback is recorded (`take_errors`), the other callbacks still run, and a callback may subscribe from inside a call. (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-125~~ | Integration | A callback survives repeated collections while only the engine holds it; a non-function is a `TypeError`. (removed 2026-10-28: QuickJS removed) |
| M9-API-126 | Integration | Physics from QuickJS: a Rigidbody with a BoxCollider falls and rests on a floor at y = 1 (as the compiled test), `ForceMode` / `RigidbodyConstraints`, a wrong handle is a `TypeError` (`crates/platypus_native/tests/physics.rs`). |
| M9-API-127 | Integration | Raycast from QuickJS: distance, point, normal of the hit, a miss and a too-short ray are `null`, `maxDistance` optional. |
| M9-API-128 | Integration | Bodies, colliders and a rope joint made from script; gravity getter/setter; bad arguments are `TypeError`s. |
| M9-API-129 | Integration | `CharacterController` grounded and moving, `Rope` points, as the compiled tests. |
| M9-API-130 | Integration | 2D: `Rigidbody2D` rests on a 2D floor with z kept, `Physics2D.raycast` names the object. |
| M9-API-131 | Integration | `FixedJoint` holds a body, `HingeJoint` properties, `MeshCollider.setMesh` from arrays. |
| M9-API-132 | Integration | Audio from QuickJS: the host receives Load / Bus / Emitter / Play / Set / Stop / Listener commands with the script's values; `onEnded` runs when the host reports (`crates/platypus_native/tests/audio.rs`). |
| M9-API-133 | Integration | `AudioSource` / `AudioListener` components, one-shots and static `playClipAtPoint`. |
| M9-API-134 | Integration | `input.on...` listeners get the events' arguments, tokens and `off` (`crates/platypus_native/tests/services.rs`). |
| M9-API-135 | Integration | `Assets.onChanged`, `Application.onFocusChanged`, `Screen.onChange` callbacks and `Time`. |
| M9-API-136 | Integration | `Rng` handles and `Random` agree with the compiled shims for one seed. |
| M9-API-137 | Integration | Scenes from script: serialize, load additively, instantiate a prefab, `onSceneLoaded` / `onSceneUnloaded` / `onActiveSceneChanged`. |
| M9-API-138 | Integration | `Cursor`, `Screen` and `QualitySettings` property pairs. |
| M9-API-139 | Integration | `Light`, `MeshRenderer`, `Material` (enums, colors, numbers, handles), and the host's view of the data (`crates/platypus_native/tests/render.rs`). |
| M9-API-140 | Integration | `Texture`, `Mesh.create` from arrays, `Sprite`, `SpriteRenderer`. |
| M9-API-141 | Integration | `RenderSettings`, `Primitives`, `LineRenderer.setPositions`, `Cloth`, `ParticleSystem`. |
| M9-API-142 | Integration | `ParticleSystem.onStopped` runs when the host reports the system stopped. |
| M9-API-143 | Integration | `Model.load` / `instantiate` with optional values left out. |
| M9-API-144 | Integration | Every component class with `attach` (and assets with `create()`): each get/set property round-trips its own value (265 properties, `surface.rs`). |
| M9-API-145 | Integration | GC stress: tiny collection threshold, thousands of handle objects and 200 engine-held callbacks stay correct. |
| M9-TS-1600 | Integration | An effect whose cleanup is an arrow around a void call (`return () => clearTimeout(id)`) and a setter inside the timer compile (`tools/web_ui_ts/tests/react_front.rs`; was a Cranelift verifier error). |
| M9-TS-1601 | Integration | Parameter types come from their default values (`phase = 0`, `name = "x"`, `on = false`). |
| M9-TS-1602 | Integration | A program class named like an engine handle (`class Scene`) shadows it (was a panic). |
| M9-TS-1603 | Integration | `Text`, `Span`, `Input`, `TextArea`, `Image` are the intrinsic elements. |
| M9-TS-1604 | Integration | A program component named `Text` keeps its meaning. |
| M9-TS-1605 | Integration | Text, numbers, lists and `cond && <x/>` are children of a user component (`children?: ReactNode`). |
| M9-TS-1606 | Integration | `key={4}` and boolean children. |
| M9-TS-1607 | Integration | `onClick={f}` with a 0-parameter function, `undefined` handlers, an optional `onPress` prop, `<Btn />` for all-optional props. |
| M9-TS-1608 | Integration | `style` values that are `number \| string`, or `undefined` (`cursor: on ? "pointer" : undefined`). |
| M9-TS-1609 | Integration | `useState(() => 5)`, `useState(read)`, `useState<number[]>([])` and setters inside an effect. |
| M9-TS-1610 | Integration | `PlatypusStyle` constants and spreads, `React.ReactNode`, a function returning `PlatypusStyle`. |
| M9-TS-1611 | Integration | Event fields `shiftKey`, `ctrlKey`, `code`, `deltaY`, `scrollTop`, `target.value`. |
| M9-TS-1612 | Integration | `useRef<PlatypusElement>(null)` with `.focus()`, `.blur()`, `.scrollTo(x, y)`. |
| M9-TS-1613 | Integration | `static get twice()` reads like a field. |
| M9-TS-1614 | Integration | A component with `return null` and `return <box/>`. |
| M9-TS-1615 | Integration | `createRoot().render(<App />);` as a top-level statement lowers to `UiRoot.mount(0, el)` in the init function (imports `ts_ui_mount`); top-level expression statements run in source order. |
| M9-TS-1616 | Integration | `[number, number]` and `[number, number][]` are arrays (`react_front.rs`). |
| M9-TS-1617 | Integration | `Record<NumericEnum, V>` with computed enum keys is an array indexed by the enum. |
| M9-TS-1618 | Integration | A `const` arrow that calls itself (`setTimeout(loop, 16)`) and a top-level `let` arrow that is reassigned. |
| M9-TS-1700 | Integration | `RenderSettings.skybox = { .. }` and `.bloom = { .. }` (and `= null`) compile to the per-field setters: off, on with the defaults, then each key (`sunDirection: null` is `skyHasSun = false`); `bench/facade.ts`. |
| M9-TS-1701 | Integration | `new GameObject(name, { parent, position, rotation, scale, layer, active })`: `parent` first (`null` is no parent), `position` / `rotation` / `scale` are local to the parent. |
| M9-TS-1702 | Integration | `Camera` `rect`, `layerCullDistances` (read, element write, assign), `horizontalFieldOfView`, `Camera.main` (a static function with no arguments read as a property), `Screen.windowSize`, `QualitySettings.names`: facade members written in TypeScript (`lower/facade.ts`). |
| M9-TS-1703 | Integration | `new Color(r, g, b)`, `new Material(color, emission, smoothness, unlit)`, `material.withTexture(t)`, `GameObject.createPrimitive(kind, { position, scale, name })`. |
| M9-TS-1704 | Integration | `lod.setLODs([..])`, Cloth `pins` / `sphereColliders` / `pinBox` options, `scene.getRootGameObjects()`. |
| M9-TS-1705 | Integration | The `AnimatorController` builder (`addParameter`, `addState` with a blend-tree object and options, `addTransition` with conditions) over the native controller; `animator.getCurrentAnimatorStateInfo()`. |
| M9-TS-1706 | Integration | `Sprite.create(texture, rect, pivot, ppu)` and `Sprite.grid(texture, columns, rows, pivot?, ppu?)`. |
| M9-TS-1707 | Integration | `addStyleSheet` / `removeStyleSheet` / `loadFont({ family, src, weight })` queue requests for the host (`UiResources`); `input.gamepads()` and `input.on("gamepadconnected" / "gamepaddisconnected", fn)` return an unsubscribe function. |
| M9-TS-1708 | Integration | ParticleSystem options (`[min, max]` ranges, `bursts`, `shape`, `colorOverLifetime`, `sizeOverLifetime`), `points` / `positions` lists (`setPoints` / `setPositions` as setters), `sprite: null`. |
| M9-TS-2000 | Integration | `AnimationCurve.fromPoints([[t, v], ..])` pairs (facade) next to the native `Vector2[]` form. |
| M9-TS-2001 | Integration | `QualitySettings.onLevelChanged` (and the other subscribe calls) return an unsubscribe function; a `Record<FullScreenMode, string>` is indexed by `Screen.supportedFullScreenModes`. |
| M9-TS-2002 | Integration | A callback that returns nothing on one path and a function on another (an effect with an early `return;`) is nullable. |
| M9-TS-2003 | Integration | Array literals of mixed element types are tuples (objects with fields `0`, `1`, ..): `as const` rows, destructuring in `for-of`, `t[1]`. |
| M9-TS-2004 | Integration | Closure parameters with defaults and no annotation (`scale = 0.75`) may be left out. |
| M9-TS-2005 | Integration | `void expr` evaluates and yields nothing. |
| M9-TS-2006 | Integration | A `Record<union, V>` is indexed by a `string` from the engine. |
| M9-TS-2007 | Integration | `{ x, y }[]` goes where `Vector2[]` is expected (setter, `cond ? list : []`). |
| M9-TS-2008 | Integration | `for (const t of [a, b])` over unrelated types runs the body per element. |
| M9-TS-2009 | Integration | A component class no `register(..)` names is in the class table with `registered == false` (so `addComponent(Class)` works); the registered ones start as before. |
| M9-TS-2010 | Integration | `const set = <K extends keyof T>(key: K, value: T[K]) => ..` is written once per literal key it is called with. |
| M9-TS-2011 | Integration | `cond ? "50%" : 4` as a style value with no type around it. |
| M9-API-500 | Unit | `UiResources` requests come out in order with stylesheet ids; the queue is emptied by `take_requests`. |
| M9-TS-1800 | Unit | A relative import pulls the file in before the importer; `export` is dropped from non-entry files. |
| M9-TS-1801 | Unit | `@platypus/*` and `react` imports are erased; an aliased name is renamed to the real one. |
| M9-TS-1802 | Unit | Colliding top-level names are renamed (`name_2`) in the dependency, references included. |
| M9-TS-1803 | Unit | Imports through re-exports (`export { x as y } from`, `export *`) and aliases reach the declaration. |
| M9-TS-1804 | Unit | A renamed shorthand property keeps its key (`{ count: n_2 }`). |
| M9-TS-1805 | Unit | `import "./x.css"` is listed, not read; a missing module or export is an error naming it. |
| M9-TS-1806 | Unit | `export default <expression>` gets a generated name that importers use. |
| M9-TS-1810 | Unit | Manifests become `declare module "@platypus/engine"` declarations (classes, value classes, statics, properties, namespaces, callbacks) and the ambient text is appended. |
| M9-TS-1830 | Unit | `.meta` files are made for new assets, kept (GUID unchanged) for old ones, not for `package.json`. |
| M9-TS-1831 | Unit | A moved asset keeps its GUID; a gone one is an orphan, deleted with `--prune`. |
| M9-TS-1840 | Unit | CSS bundling: `@import` joined, small `url()` inlined as `data:`, large copied with a hash, missing ones warned. |
| M9-TS-1850 | Unit | `tsconfig.json` comments and trailing commas are stripped. |
| M9-TS-1851 | Unit | Game config from `package.json` `platypus` and `tsconfig.json`. |
| M9-TS-1852 | Integration | A build writes `game.pbundle`, `game.css`, the named assets and `.meta`; a compile error fails the build. |
| M9-TS-1860 | Unit | `build/.platypus-reload` holds `{seq, ok}` or `{seq, ok:false, error}`. |
| M9-TS-1861 | Unit | The watch fingerprint changes with sources, not with `build/`. |
| M9-TS-2300 | Unit | `types/platypus.d.ts` is deterministic; it declares the engine and UI names (classes, namespaces, hooks, components, types) as globals, and `@platypus/engine`, `@platypus/react`, `react` as re-export modules; nothing is `export`ed outside them. |
| M9-TS-2301 | Unit | Calls have an options-object form (`audio.load(path, { stream })`), creatable classes have constructors and `Class.create({..})`. |
| M9-TS-2302 | Integration | A program with no imports at all compiles (names are globals). |
| M9-TS-2303 | Integration | The old `import { .. } from "@platypus/engine"` forms still compile (the imports are erased). |
| M9-TS-2304 | Unit | The generated `build/.tsc/tsconfig.json` extends the game's, keeps its `include`, adds `platypus.d.ts`, forces module detection and `jsx: preserve`. |
| M9-TS-2305 | Integration | With `tsc`: a program with no imports type-checks, a wrong call (`new Vector3(1, 2)`) fails with TS2554, the old import forms (engine, `react`, `@platypus/react`, JSX) type-check. Skipped with a message when `tsc` is not found. |
| M9-TS-2306 | Integration | The generated declarations alone have no `tsc` errors with `skipLibCheck` off. Skipped without `tsc`. |
| M9-TS-2307 | Integration | All the examples type-check against the generated declarations (`tsc` ok, no `node_modules` in the games). Skipped without `tsc`. |
| M9-TS-2308 | Unit | No example imports `@platypus/engine`, `@platypus/react` or `react`. |
| M9-TS-2310 | Unit | Options-object overloads: leading required parameters stay positional, a required parameter after an optional one becomes optional, `function in(..)` is left out, creatable handle classes get constructors and `create(options)`, component handles get `transform` and `getComponent`. |
| M9-TS-2311 | Unit | A compiler built-in (`tools/web_ui_ts/src/builtins.rs`) replaces the native member of the same name in the generated class (a static of a namespace is a `function`), its constructors replace the generated ones, and its top-level names and the `<Value>Like` types they name are exported by `@platypus/engine` (`types_dts.rs`). |
| M9-TS-2312 | Unit | A program with no library imports bundles the same way (`library_imports` empty). |
| M9-TS-1900 | Integration | The object writer's output (size and hash of three programs) is unchanged by the generic code generation. |
| M9-TS-1901 | Integration | A program compiled in process (`jit::compile_and_load`) runs: exported functions called through their addresses. |
| M9-TS-1902 | Integration | JIT code uses the runtime (strings, arrays, `Math`) through the generated `ts_rt_*` symbol table. |
| M9-TS-1903 | Integration | An engine function the host does not provide is an error naming its symbol, before any code runs. |
| M9-TS-1904 | Integration | A JIT-compiled component program: the class table is read at run time and `NativeHost` drives it over the real scene. |
| M9-API-600 | Integration | Reading the manifests registers the shim addresses: every manifest function symbol resolves with no hand-kept list. |
| M9-API-601 | Unit | The runtime's generated symbol table lists its `extern "C"` exports. |
| M9-API-610 | Unit | A native bundle is JSON with a format number and a source; other formats and missing sources are errors. |
| M9-API-611 | Integration | A compiled component gets a GameObject and `onUpdate` every frame (`platypus_app::native`). |
| M9-API-612 | Integration | Timers of a compiled game run from the frame's clock. |
| M9-API-613 | Integration | A mounted root is drawn into the screen's `UiTree` and a click reaches its handler. |
| M9-API-614 | Unit | A game that does not compile is an error naming the compile step, not a panic. |
| M9-API-615 | Integration | A hot reload of a compiled game keeps the objects, the `@property` fields of components and the UI's hook state, with the new code running. |
| M9-API-616 | Integration | A reload that does not compile keeps the old game running. |
| M9-API-617 | Unit | The `build/.platypus-reload` note of a failed build gives its error. |
| M9-API-800 | Integration | Keys sent as `__platypus_input` arguments reach `input` state and the `onKeyDown` closures of a compiled game. |
| M9-API-801 | Integration | `preventDefault()` in a compiled handler is the answer the interaction layer gets (`Native::ui_event`). |
| M9-API-802 | Integration | `Native::fixed_tick` steps the physics world and the compiled Rigidbody's object falls in the scene. |
| M9-API-803 | Integration | Audio commands of a compiled game come out of `take_commands`, and `Native::audio_ended` runs its `onEnded` closures. |
| M9-API-804 | Integration | `Screen.onChange`, `QualitySettings.onLevelChanged` and `Application.onFocusChanged` closures of a compiled game run from the host calls; a `spawn: false` class gets no starting object. |
| M9-API-805 | Unit | `{ spawn: false }` is read from the `register(..)` calls of the source. |
| M9-API-806 | Integration | `loadSceneAsync` of a compiled game reads the file from the game folder and `onSceneLoaded` runs; `Assets.onChanged` closures run on the host's changes. |
| M9-API-807 | Manual | `platypus-build dev` writes `build/.platypus-reload`; a screenshot run with `PLATYPUS_RELOAD_AT_FRAME` after a source edit and rebuild reloads the native game and draws the new code (checked with a boxes copy: colour change shows after the reload). |
| M9-API-808 | Integration | Only `register()`ed classes get a starting object (`NativeClass.registered`), `addComponent` works for the others, and `e.target.type` is the element kind of the event's target. |
| M9-API-620 | Integration | A compiled game's camera, light and cube reach the renderer's views through `render_sync` (no GPU). |
| M9-API-700 | Integration | `render_sync` turns planes, `RenderSettings` (skybox, bloom, tonemapping) and quality into the renderer's views, and a second sync leaves them as they were (no GPU). |
| M9-API-1000 | Unit | A payload written after a host reads back entry by entry (deflate for compressible data, stored for tiny) with the right start offset (`platypus_core/src/payload.rs`). |
| M9-API-1001 | Unit | A file or buffer without the `IRONPAYL` trailer has no payload (`Ok(None)`), also an empty one. |
| M9-API-1002 | Unit | Payload paths with `..`, absolute, drive, backslash, empty or `.` parts are refused on write, and a table with one is refused on read. |
| M9-API-1003 | Unit | Duplicate paths, a damaged entry (CRC) and a trailer of another format version are errors. |
| M9-API-1004 | Unit | `extract` writes every file; the payload id changes with the content and not with a re-read. |
| M9-API-1005 | Unit | A payload followed by padding and an Authenticode certificate table (synthetic PE32+ header) is found before the certificate. |
| M9-API-1010 | Unit | `platypus_app::payload::prepare` of an exe without a payload gives `None` (the plain host starts normally). |
| M9-API-1011 | Unit | The payload is extracted once into a cache folder (marker file, second start reuses it), a new build gets a new folder and the old one is removed, another game's folder stays. |
| M9-API-1200 | Unit | The packaged game's data folder is `--data`, else `$PLATYPUS_DATA`, else `<exe dir>/<dataDir>` (`platypus_app/src/asset_root.rs`); an empty variable is ignored. |
| M9-API-1201 | Unit | A `--data` or `$PLATYPUS_DATA` that names a missing folder is an error naming the flag or variable, not a fallback. |
| M9-API-1202 | Unit | No data folder for a game that needs one is an error naming the folder looked for (`platypus.dataDir`), `--data` and `PLATYPUS_DATA`. |
| M9-API-1203 | Unit | A game with `platypus.dataRequired: false` runs from the extracted folder without a data folder; a payload without the field needs one. |
| M9-API-1204 | Unit | CSS `url()`s rebase onto the code folder (`css_base`); `AssetRoot::resolve` refuses paths outside the root. |
| M9-TS-2400 | Unit | Package metadata defaults: title, exe, company, version, copyright, description, identifier, icon from `package.json`; the folder name when nothing is given (`platypus_build/src/package.rs`). |
| M9-TS-2401 | Unit | `platypus.*` fields override the package.json ones, flags override both; the exe name is made file-name safe and loses `.exe`. |
| M9-TS-2402 | Unit | The payload's `package.json` carries the resolved version, title and icon entry. |
| M9-TS-2410 | Unit | Data collection takes the other `build/` files (CSS references; not the bundle, `game.o`, `*.exe`, dot files, asset copies, the output's own data folder), the named assets with `.meta`, `platypus.include` or the default `assets` folder; an unsafe asset path is an error. |
| M9-TS-2411 | Unit | `1.2.3`, `2.0.0-beta.1+x`, `1.2.3.4`, `junk` become four-part Windows versions. |
| M9-TS-2412 | Integration | `package` with a stand-in host writes one file whose payload holds the bundle, assets, rewritten package.json and icon, whose PE resources carry the version and a four-size icon, and on which `--sign-cmd` ran; packaging a packaged file replaces its payload. |
| M9-TS-2420 | Unit | A PNG becomes an `.ico` with 16, 24, 32, 48, 64, 128 and 256 px images that decodes (`platypus_build/src/icon.rs`). |
| M9-TS-2421 | Unit | A non-square image is centred on a square; an `.ico` passes through unchanged; garbage is an error. |
| M9-TS-2430 | Unit | Version info (ProductName, CompanyName, FileDescription, LegalCopyright, FileVersion, ProductVersion, OriginalFilename, 1.2.3.0 numbers) and the four icon sizes written into a PE file read back, and a payload appended after the edit is found (Windows; `platypus_build/src/pe.rs`). |
| M9-TS-2440 | Manual | `cargo test -p platypus_build -- --ignored standalone` (needs a built `platypus_app`, GPU): packages `examples/hello`, deletes the game folder and runs the exe from another folder with `--screenshot out.png --frames 5`. Checked by eye for hello (a log-only game, background colour) and boxes (the stripes image from `assets/` shows). |
| M9-TS-2550 | Unit | Globs (`platypus_core::glob`): `*` stays inside one path part, `**` crosses parts, `?`, case-sensitive; a plain path in `include` means the folder below it. |
| M9-TS-2551 | Unit | Dynamic path inference (`web_ui_ts::assetpaths`): templates and `+` chains become globs, one `*` per hole (nested holes too); prefix-less paths, paths without a file name, URLs and text with spaces are refused; whole literals and lines are reported. |
| M9-TS-2552 | Unit | `include` / `exclude` selection (`platypus_build::include`) and `collect_files`: globs work, a pattern's matches are copied under a narrowed `include`, `exclude` removes files, an unmatched `include` is a warning not a silent drop. |
| M9-TS-2553 | Manual | `platypus-build package examples/emoji --host target/debug/platypus-app.exe --out <dir>/emoji.exe`, then `emoji.exe --screenshot out.png --frames 150` from that folder with `PLATYPUS_LOG_FILE`: no `not found`, the sounds decode and the SVG emoji are drawn (checked 2026-10-22). |
| M9-TS-2554 | Unit | Pack placement (`platypus_build::packs`): an asset only one scene names goes to that scene's pack, one two scenes name, or the code, a dynamic path or `include` takes goes to `assets.dat`; prefabs are followed; the scene file is the first entry of its own pack; scene slugs stay distinct. |
| M9-TS-2556 | Unit | Two builds of the same files write identical bytes and remove an old pack; two keys that differ only in case are an error. |
| M9-TS-2558 | Unit | A pack written by the build reads back byte-identical through `platypus_pack::Pack` and `Vfs` (GUIDs and importer text from `.meta`, LZ4 for text, mp3 stored, `verify`), and `pack-info` lists it. |
| M9-TS-2559 | Unit | `find_host` picks the most recently built `platypus_app` among the candidates (a stale release build does not beat a fresh debug one); `package` prints which host it used (`platypus_build::package::newest_host`). |
| M9-TS-2557 | Integration | A compiled program calls `Assets.list("assets/sfx/*.wav")` and gets the sorted keys as a `string[]`, with `exists` and `packOf`, in every GC mode (`tools/platypus_ts_tests/tests/engine_api/assets_native.rs`). |
| M9-TS-2560 | Unit | `--codec zstd`: `parse_codec`, a pack written with zstd reads back (whole and across a chunk boundary), `index.json` names the codec, `pack-info` lists `zstd`. |
| M9-TS-2600 | Unit | The payload holds `build/game.pbundle`, `build/game.css`, `package.json`, the icon and the bundle-named assets only while those total at most 256 KiB (exactly at the limit: embedded, one byte over: none); `.meta`, unnamed assets and CSS files are never embedded. |
| M9-TS-2601 | Unit | `platypus.dataRequired` is false for a game without assets and for one whose only assets are embedded essentials (a `.meta` does not count); an unnamed file in `assets/` makes it true. |
| M9-TS-2602 | Unit | `--data-dir` accepts one plain folder name and refuses empty, `.`, `..`, separators, drive letters and dot names. |
| M9-TS-2603 | Integration | `package` copies the assets to `<out dir>/data/` with the same relative paths (a 300 KiB file byte for byte, `.meta`, scenes, CSS files), the exe's table of contents has none of the large or unnamed ones, `--data-dir` renames the folder and tells the exe, `--no-data` writes no folder, a bad name is an error. |
| M9-TS-2610 | Manual | `cargo test -p platypus_build -- --ignored standalone_data` (needs a built `platypus_app`, GPU): `examples/boxes` plus a 300 KiB padding asset packaged, the game folder deleted, run from another folder with `--screenshot` (stripes load from `data/`), then with `data/` deleted: exit code 1 and "data folder was not found". |
| M9-API-1300 | Unit | Every template variable resolves (`username`, `firstName`, `lastName`, `fullName`, `year`, `date`, `name`, `title`, `company`, `version`, `env:`). |
| M9-API-1301 | Unit | `$${` is a literal `${`; a lone `$` stays. |
| M9-API-1302 | Unit | An unknown variable is an error with the file, line and column of the `${..}` and the field name. |
| M9-API-1303 | Unit | `${company}` inside `copyright` (and other fields) resolves through the field; a reference cycle is an error. |
| M9-API-1304 | Unit | `${env:NAME:-fallback}`; an unset `${env:NAME}` is an error. |
| M9-API-1305 | Unit | An unterminated `${`, an empty `${}` and broken JSON (with its line) are errors. |
| M9-API-1306 | Unit | Name splitting: empty, one, two and three words, extra spaces, apostrophes, hyphens, non-Latin names. |
| M9-API-1307 | Unit | An empty full name falls back to the account name; one word gives an empty last name; GECOS parsing. |
| M9-API-1308 | Unit | The identifier is sanitised (spaces, punctuation, leading digit, empty parts). |
| M9-API-1309 | Unit | The platform section overrides for that platform; all platform sections leave the result. |
| M9-API-1310 | Unit | Deprecated fallback: with no `project.json` the `platypus` section and `package.json` fields are used without templates; no files: defaults named after the folder. |
| M9-API-1311 | Unit | `package.json` is not read at all when a `project.json` exists (not its fields, not even a broken file). |
| M9-API-1312 | Unit | Overrides (flags) are literal and other fields see them. |
| M9-API-1313 | Unit | A resolved project round-trips with no templates; unknown keys are kept and expanded once. |
| M9-API-1314 | Unit | Omitted fields take their defaults silently (title/company from the first name, version `0.1.0`, license `private`, exe without apostrophes, copyright, identifier, description); the omitted ones are listed for the package warnings. |
| M9-API-1315 | Unit | A leading `//` comment line is allowed and keeps line numbers. |
| M9-API-1316 | Unit | Civil dates from days; the system date is plausible. |
| M9-API-1317 | Unit | The system user has an account name. |
| M9-API-1318 | Unit | `license` defaults to `private`, is validated case-insensitively against the list, `custom` takes `licenseFile`, the npm mapping. |
| M9-API-1321 | Unit | `name` is the only required field: missing or empty is an error naming `project.json`; a command-line override counts. |
| M9-API-1322 | Unit | An explicitly empty string field stays empty (no default), is listed in `empty_fields` with a warning text, and round-trips through the resolved project. |
| M9-API-1330 | Unit | `renderer` in `project.json`: left out it is `auto` (not written back by `to_json`); `vulkan`, `gles` and `cpu` are kept and round-trip, a known key (not in `extra`); `gles` and `cpu` are UI-only; an unknown value warns naming `renderer` and counts as `auto`. |
| M9-API-1331 | Unit | The old `type` (`app`, `game`) gets the one-line warning (`type` is replaced by `renderer`) and is ignored: not mapped to a renderer, not kept in `extra`, not written back. |
| M9-API-1332 | Unit | `Renderer::first_set` picks the first renderer that is set (command line, then build argument, then project), `auto` when none is; every value parses back from its name. |
| M9-API-1320 | Unit | `platypus_app` `Manifest::load` of a folder uses `project.json` (title, identifier, startScene, layers, platform fps); a broken template leaves the defaults. |
| M9-TS-2700 | Unit | `init` writes the default templates, a self-contained `tsconfig.json` and `types/platypus.d.ts` (M9-TSC-02), `src/main.ts`, and the resolved preview for a given user; the result builds. |
| M9-TS-2701 | Unit | `init` flags replace only their own fields. |
| M9-TS-2702 | Unit | `init` refuses to overwrite `project.json` without `--force`. |
| M9-TS-2703 | Unit | `init --icon` copies the icon; `.gitignore` gets `build/`, `types/` and `*.meta` once and only if it exists; a bad icon writes nothing. |
| M9-TS-2704 | Unit | A user without a full name gets the account name for first name and company. |
| M9-TS-2705 | Unit | `init --license` sets and validates the license, shown in the preview. |
| M9-TS-2710 | Integration | `package` resolves templates and the platform section into the payload's `project.json` (and the PE version info), the payload has a `package.json` with `UNLICENSED`; the app side reads it back without the user's information. |
| M9-TS-2711 | Integration | A non-private license copies its file next to the exe (and into `data/` when named), adds `Comments` to the PE and the npm license to the payload; `custom` without a file and a missing `licenseFile` are errors. |
| M9-TS-2720 | Unit | `GameConfig` reads `project.json` (title, identifier, `main`) before the `platypus` section; a template typo names file and line. |
| M9-TS-2721 | Unit | All 20 examples are found by their `project.json` (`dev.platypus.<name>`, company from the user), none has a `package.json`. |
| M9-TS-2722 | Unit | `build` warns once per empty metadata field naming the file; an omitted field is not a warning. |
| M9-TS-2723 | Integration | `package` warns about empty fields and about `company`/`copyright`/`license` still at their defaults, still packages, and is silent when all are given. |
| M9-TS-2724 | Integration | The `platypus-build` program: `build` and `package` print the empty-field warnings, `check` does not (`tools/platypus_build/tests/cli.rs`). |
| M9-TS-2725 | Unit | Games are found by `project.json` (or a `package.json` with a `platypus` section, the deprecated fallback), not by any `package.json`. |
| M9-LIC-01 | Integration | Every tracked source file (`.rs`, `.ts`, `.tsx`, `.js`, `.mts`, `.wgsl`, `.css`, `.py`, `.toml`, `.sh`) has `Copyright 2026 The Platypus Authors` and, on the next line, `SPDX-License-Identifier: Apache-2.0` within its first 4 lines (a check of that rule too), except the commented list of generated files, built fixtures and third-party ports; skipped without git (`tools/platypus_build/tests/spdx_headers.rs`). |
| M9-LIC-02 | Integration | `notices_are_current`: the committed `THIRD-PARTY-NOTICES.md` is what `platypus_notices` writes now from Cargo.lock and `hand-written.md`; skipped when `cargo metadata --offline` cannot run or a registry source is missing (`tools/platypus_notices/tests/current.rs`). |
| M9-LIC-03 | Unit | SPDX expressions: `OR` (and the old `/`) give alternatives, `AND` joins licences into each, parentheses and `WITH` are kept, malformed expressions fail. |
| M9-LIC-04 | Unit | Licence files are classified by name (`LICENSE-MIT`, `UNLICENSE`) or else by text (Apache with the LLVM exception, BSD-3 by its endorsement clause); a pointer file holds no licence; NOTICE / THIRD-PARTY files are notices. |
| M9-LIC-05 | Unit | Copyright lines are found (`Copyright (c) 2014 ...`, `(c) 2015 ...`, `\u{a9} ...`) but not the licence's own wording (`copyright notice`, `COPYRIGHT HOLDERS`, `[yyyy]`, `(c) You must retain`); each once. |
| M9-LIC-06 | Unit | `--check`'s comparison ignores CRLF and names the first differing line and the lines gone and added. |
| M9-LIC-07 | Unit | The tree from `cargo metadata` follows normal and build dependencies (not dev), lists registry crates only (not path ones), authors without e-mail, a crates.io link without a repository; a missing source is its own error. |
| M9-LIC-08 | Unit | A dual-licensed crate is listed once, under the preferred licence it ships a file for (MIT before Apache-2.0). |
| M9-LIC-09 | Unit | An `AND` expression lists the crate under each of its licences, with that licence's copyright lines. |
| M9-LIC-10 | Unit | MPL-2.0 crates say they are used unmodified from crates.io and where the source is. |
| M9-LIC-11 | Unit | An unknown licence, a GPL with no other choice and a crate with no licence file fail with the list; allow-listed ones pass (no file: the standard text and the Cargo.toml authors; unknown: its own section). |
| M9-LIC-12 | Unit | Identical licence texts (copyright lines, titles and layout aside) are printed once; each crate keeps its own copyright lines; other wordings of MIT/BSD get their own text, a fixed-text licence (Apache-2.0) never does. |
| M9-LIC-13 | Unit | NOTICE files and reviewed bundled licence files are included (skipped ones are not); an unreviewed licence file inside a crate fails. |
| M9-LIC-14 | Unit | `platypus-app --licenses` asks for the notices whatever else is on the line; the embedded text is the committed file (`crates/platypus_app`). |
| M9-LIC-15 | Integration | `platypus-app --licenses` prints the embedded notices and exits 0 without opening a window (`crates/platypus_app/tests/licenses.rs`). |
| M9-LIC-16 | Unit | `platypus-build package` writes the embedded `THIRD-PARTY-NOTICES.md` beside the exe; `--no-notices` leaves it out. |
| M9-REN-01 | Unit | A game built before the rename to Platypus still runs: `build/game.irbundle` is a native bundle and reads like `game.pbundle` (the host logs a deprecation note when it falls back to it). |
| M9-REN-02 | Unit | The deprecated `package.json` fallback reads an `iron` section as well as `platypus` (`platypus` wins when both are there). |
| M9-REN-03 | Unit | Scene and prefab files saved with `"format": "iron.scene"` / `"iron.prefab"` still load. |
| M9-REN-04 | Unit | `import { .. } from "@iron/engine"` is erased like `@platypus/engine`. |
| M9-ICO-01 | Unit | The Platypus icon (`resources/icon/icon.png`, built into `platypus_core::icon::DEFAULT_PNG`) is a 1254 x 1254 PNG. |
| M9-ICO-02 | Unit | `write_ico` lists its sizes; `windows_res` writes a `.res` file of aligned entries: an empty first entry, 7 icons, the `MAINICON` group and a version block with `ProductName`. |
| M9-ICO-03 | Unit | The host's default window icon is 256 x 256 RGBA with transparent corners and an opaque middle. |
| M9-ICO-04 | Integration | `platypus-app.exe` (built by cargo, Windows MSVC) carries the `MAINICON` group, PNG icon images and `ProductName` = `Platypus`. |
| M9-ICO-05 | Unit | The default icon decodes and converts to an `.ico` with all seven sizes. |
| M9-ICO-06 | Unit | `package` of a game without an icon and with empty title and description gives the exe the Platypus icon (all seven sizes, the same 16 px image) and `ProductName` / `FileDescription` = `Platypus`; a game with its own icon keeps it (`M9-TS-2412`). |
| M9-API-900 | Unit | `format_fps` gives the integer rate as `N FPS` (36 frames in 0.25 s is `144 FPS`, zero time is `0 FPS`). |
| M9-API-901 | Unit | The rate rounds to the nearest integer. |
| M9-API-902 | Unit | `FpsCounter` closes a window once 0.25 s have passed and reports it; 100 FPS over 0.6 s gives two windows of `100 FPS`. |
| M9-API-903 | Unit | The rate is not smoothed: a fast window then a slow window shows the slow rate. |
| M9-API-904 | Unit | Before a first window closes (short screenshot runs) `text` shows the partial window. |
| M9-API-905 | Unit | The overlay is on by default and off in screenshot runs. |
| M9-API-906 | Unit | `PLATYPUS_FPS=0` and `platypus.fps: false` turn it off. |
| M9-API-907 | Unit | `--fps` / `--no-fps` override the defaults, the env and the manifest. |
| M9-API-908 | Unit | `FpsOverlay` draws commands for its text (and fewer for no text). |
| M9-API-909 | Unit | `--fps` and `--no-fps` parse. |
| M9-API-910 | Unit | `platypus.fps` is read from the manifest. |
| M9-API-911 | Integration | `platypus-app <fixture> --screenshot --frames 3 --fps` saves the file and the log has no errors (needs a GPU). |
| M9-API-912 | Integration | The same with `--no-fps`. |
| M9-API-913 | Unit | `Quality::default()` has vsync on; the `render.quality` op turns it off with a vSyncCount of 0 and leaving the argument out means the default (on). |
| M9-API-914 | Integration | Quality levels start with `vSyncCount` 1; a saved `platypus.quality` without `playerVSync` (older saves) is applied with the level's vsync, one with `playerVSync: true` keeps its `vSyncCount: 0`. |
| M9-API-915 | JS | `QualitySettings.vSyncCount` is 1 by default; an older save's vsync is ignored and a player's (`playerVSync`) is kept. |
| M9-API-916 | Unit | `FpsCounter::value` is the last closed window's unrounded rate (0 before one closed); the host publishes it as `Application.fps` whether or not the overlay is drawn. |
| M9-API-917 | Unit | `--crash-dialog <folder>` parses: the folder is the game, and the run is the crash dialog's (not the editor's or a packaged game's). |
| M9-API-919 | Integration | A fault in a local (not exported) compiled function is reported: the log names it (`near crashHere+..`) and a crash report is saved naming it. Only exported functions were known before, so a fault in one laid out before them was taken for a driver's: no report and no dialog (`tests/crash_report.rs`, Windows, needs a GPU). |
| M9-API-920 | Integration | A panic inside a host function the compiled game called (which aborts the process: it cannot unwind) saves a crash report with the panic's message and backtrace, as a fault does (`tests/crash_report.rs`, Windows). |
| M9-API-921 | Integration | A stack overflow in compiled code (endless recursion) saves a report: the reason, the function and line, and the call stack with the recursion as one counted run (`(x20000)`), written from a new thread because the overflowed one has no stack left (`tests/crash_report.rs`, Windows). |
| M9-API-922 | Unit | The DevTools server's WebSocket handshake answers RFC 6455's worked example, and its base64 pads like the standard (`cdp.rs`). A whole session (break on start, conditional breakpoint, locals, evaluate, step) was checked by hand against the release host with a raw WebSocket client. |
| M9-API-918 | Unit | The crash dialog reads the game and the reason from a crash report, with defaults when they are missing. |
| M9-VSP-01 | Unit | The startup video-settings prompt: the command line (`--video-settings-prompt` / `--no-video-settings-prompt`) wins over the project's `videoSettingsPrompt` (a packaged game's build argument is already in it); off by default (`video_prompt::should_prompt`). |
| M9-VSP-02 | Unit | It is shown once (`platypus.videoSettings.chosen` in prefs, as a bool or the engine's number) and never in screenshot or bench runs, unless forced by the command line. |
| M9-VSP-03 | Unit | The dialog's answer (its PlayerPrefs) becomes the engine's `platypus.display` and `platypus.quality` (the level with `vSyncCount` and `playerVSync: true`; per-setting changes kept only on the same level) and marks the prompt answered; missing answers are no choice. |
| M9-VSP-04 | Unit | A fixed-size game (`display.resizable: false`) gets no mode or resolution rows (`RESIZABLE` false) and its answer leaves `platypus.display` alone; the manifest's default level and vsync on start the dialog. |
| M9-VSP-05 | Unit | The dialog starts from the saved level and the player's saved vsync; an older save's vsync does not count. |
| M9-VSP-06 | Unit | `platypus.videoSettingsPrompt` is read from the manifest (`Manifest`), unset by default; `project.json` keeps it through `to_json`. |
| M9-VSP-07 | Unit | `--video-settings-prompt`, `--no-video-settings-prompt` and `--video-settings-dialog <folder>` parse. |
| M9-VSP-08 | Unit | `platypus-build package --video-settings-prompt` / `--no-video-settings-prompt` set `videoSettingsPrompt` in the exe's embedded `project.json` over the project's; without the argument the project's value (or none) is kept. |
| M9-VSP-09 | Integration | `platypus-app <game> --screenshot out.png --video-settings-prompt` saves the dialog's picture (its own UI-only window, 600 x 500 logical pixels), not the game's (needs a GPU). |
| M9-VSP-10 | Unit | The d-pad (`Input::Navigate`) reaches the focused element as an arrow key's `keydown` first; preventing it keeps focus (a list box moves its selection), else focus moves as before (`web_ui_dom` tests/navigation.rs). |
| M9-VSP-11 | Bun | The prompt's resolution list: the monitor's modes de-duplicated and sorted largest first (rates only for exclusive fullscreen, windows only below the monitor's size); the saved size is selected, else the nearest (`video_prompt_sizes.test.ts`). |
| M9-VSP-12 | Unit | Windows show the game's title and icon: `Manifest::window_title` is the title, else the name, else "Platypus"; `window_icon` is the project's icon, else Platypus's (the main window and the prompt share them). The prompt's folder names its window after the game, carries its icon (`icon.png`: the title bar and taskbar only, no image in its UI) and is a fixed 600 x 500 (`resizable: false`: no resize edges or maximize button). |
| M9-VSP-13 | Unit + Bun | `subtitle` in `project.json`: optional, takes templates, an empty one is none and gets no warning; `Manifest::subtitle` (empty when unset) reaches `__platypus_info.app.subtitle` and `Application.subtitle`; the prompt's header shows it under the title only when set (`headerLines`). |
| M9-VSP-14 | Unit | A host dialog (`Lifecycle::set_modal`) pauses the game even with `runInBackground`, reports it, and resumes it with the paused time; screenshot runs never pause. |
| M9-VSP-15 | Unit | `Application.showVideoSettings()` queues a `ShowVideoSettings` request; `video_settings_changed` runs the `onVideoSettingsChanged` listeners until `offVideoSettingsChanged`. |
| M9-VSP-16 | Unit | A choice applied in a running game maps to the window's settings (`Choice::display`: exclusive size and rate, borderless, the window size kept; none for a fixed-size game); the dialog a running game opens says Apply / Cancel (`STARTUP` false), the startup one Play / Quit. |
| M9-APP-01 | Unit | `--force` is no longer an option (a parse error) (`platypus_app`). |
| M9-APP-02 | Unit | A QuickJS bundle (`build/game.js`, a `.js` file) is refused with a pointer to `platypus-build build`; native bundles run. |
| ~~M9-APP-03~~ | Integration | `platypus_app` on a QuickJS bundle without `--force` (`--screenshot`) exits non-zero, writes no screenshot and logs the deprecation message naming `--force` and `platypus-build build` (`crates/platypus_app/tests/quickjs_deprecated.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-APP-04~~ | Integration | The same with `--force` runs, saves the screenshot and logs the deprecation note exactly once. (removed 2026-10-28: QuickJS removed) |
| ~~M9-APP-05~~ | Integration | A native game (a bare `.ts` source) runs without `--force` and logs no deprecation note. (removed 2026-10-28: QuickJS removed) |
| M9-APP-06 | Unit | A game folder with only an old `dist/` build is refused with an error naming `dist/`, `build/` and `platypus-build build`; with `build/game.pbundle` it resolves to that bundle. |
| M9-BEN-01 | Unit | `platypus-app --bench`: a bench file becomes segments of timed platform events (keys held, clicks, `clickText`, `count`/`every` repeats, unknown kinds refused), and frame stats give fps, median, p99, worst and the script share (`crates/platypus_app/src/bench.rs`). |
| M9-BEN-02 | Unit | `platypus-build bench` writes the summary table with speed-ups, marks games the base branch lacks as n/a, and keeps hand-written text around the generated part of `docs/benchmarks.md`. |
| M9-BEN-03 | Integration | `platypus_app game.ts --bench --bench-input spec.json` runs a native game with no window (offscreen target, fake platform), the scripted keys reach it, and the JSON report has the segments, frame counts, fps and start-up time (needs a GPU; `crates/platypus_app/tests/bench_smoke.rs`). |
| M9-BEN-04 | Unit | A bench file's presses and releases of Ctrl / Shift / Alt / the Windows key are each followed by the modifier state, as a window sends it, so the UI sees Ctrl+Z (`crates/platypus_app/src/bench.rs`). |
| M9-TS-3388 | Integration | Compiled `UiScale.height` / `fit` / `fixed` reach the host's mode (`ui_scale::current`), a zero factor is ignored (`tools/platypus_ts_tests/tests/engine_api/facade.rs`, `bench/facade.ts`). |
| M9-TS-3500 | Integration | Type guards: `(x): x is T` returns a boolean, `filter` with one keeps `T`s, `asserts v` returns nothing (`tools/platypus_ts_tests/tests/lang/editor_gaps.rs`; the IDs 3400..3499 stay with the dynamic-values plan). |
| M9-TS-3501 | Integration | A union of object literal types: `"file" in s` tells the members apart (a string or object field some variant lacks is there while set), an object literal takes the member its keys name. |
| M9-TS-3502 | Integration | Type arguments from the context: a generic call's expected result types a closure's body (`useMemo(() => ({ .. }))`), an object argument's plain properties fix `T` before its closures are typed (`TreeView<T>`), a bare generic interface takes its defaults. |
| M9-TS-3503 | Integration | `Record<K, V>` keyed by `Exclude<U, X>` and by an interface property's type (`Thing["kind"]`). |
| M9-TS-3504 | Integration | `{ ...f, [key]: !f[key] }` with `key` a union of field names; `obj[k]` with `k` a `const` of literals (a conditional of a literal and an interface field). |
| M9-TS-3505 | Integration | `new Map(pairs)` from an array of pairs that is not a literal; template parts that are conditionals of different types (`${n > 999 ? "999+" : n}`). |
| M9-TS-3506 | Integration | A union of interfaces with a tag: a field all members have, tag narrowing after an early `return`, an object literal or spread taking its member, `JSON.stringify` writing the member. |
| M9-TS-3507 | Integration | A defaulted literal-union closure parameter left out through an interface's optional one, `const` arrows of a block calling each other, `Error.stack` (the first line), `Promise.resolve` / `reject` typed by the context. |
| M9-TS-3508 | Unit | `any` / `unknown` / `never` are a clear error with the line (not built yet), also through a bare generic interface's `T = unknown` default; with a type argument it compiles. |
| M9-TS-3520 | Integration | `currentTarget` is the element whose handler runs, pointer capture through it (`setPointerCapture(e.pointerId)`) becomes the host's node commands, and `defaultPrevented` reads the event as it is now, also in outer handlers (`tests/ui/ui.rs`, `bench/ui.ts` `CaptureBox`). |
| M9-TS-3521 | Integration | A function component's `ref` prop (`Ref<T>`, React 19) is filled by `useImperativeHandle` (one without a ref renders too); a `"a" \| "b" \| null` state starts null and keeps each tag (`bench/ui.ts` `Picker`). |
| M9-API-1647 | Unit | `Camera.worldToScreenPoint` inverts `screenToWorldPoint` (perspective and orthographic, a camera in half the window), `screenPointToRay` starts on the near plane through the point, `pixelRect` is `rect` in UI pixels (`camera.rs`). |
| M9-TS-3510 | Integration | A `string \| undefined` compared with string literals, also after `JSON.parse` into an interface with optional keys (`tests/lang/editor_gaps.rs`). |
| M9-TS-3511 | Integration | `return bad()` where `bad` returns `null` (typed `null`) and sets a flag: the call still runs (it was replaced by the null it gives). |
| M9-TS-3512 | Integration | A closure inside a callback passed at the top level shares the callback's variables; a generic call's object literal argument takes the type an earlier argument fixed (`eq(range, { .. })`); `filter` with a guard picks members out of a union of interfaces. |
| M9-TS-3513 | Integration | Generic classes and functions and destructuring assignment to existing variables compile and run (settles `docs/platypus-ts.md`, which listed them as unsupported). |
| M9-TS-3514 | Integration | `f?.(x)` on a callback that is left out does nothing, also when a closure captured `f` (a destructured optional prop, as the editor's `VirtualList` row handler) and when its result is used (`undefined`). The call of a plain identifier skipped the guard and the editor crashed on a click in the Hierarchy. |
| M9-TS-3515 | Integration | Reversing an empty array (no storage yet) leaves it empty; `ts_rt_arr_reverse` made a slice over a null pointer (a debug build's check aborted the editor). |
| M9-TS-3522 | Integration | A state setter, an updater and a reducer's `dispatch` kept past their component's unmount do nothing (React's behaviour) instead of aborting on "a hook slot that is gone"; while mounted they still re-render (`bench/ui.ts` `Late`). |
| M9-TS-3523 | Unit | A program with forty types and one generic promise function lowers only the `then` copies it calls (the per-type promise helpers are lowered on demand, and the program's own types are not paired with each other), so the editor's object stays small (`tools/web_ui_ts/src/lib.rs`). |
| M9-TS-3524 | Integration | `cond && f()` / `cond \|\| f()` with a void `f`, as an arrow's body or a statement, runs `f` when it should (it was dropped: the editor's dialog buttons did nothing) (`tests/lang/editor_gaps.rs`). |
| M9-TS-3525 | Integration | `PlatypusStyle["position"]` (and `"left"`, `"width"`, `"flexDirection"`, `"cursor"`, `"flexGrow"`) types values that reach the style: an absolute and a relative box lay out as CSS says; `PlatypusStyle["nope"]` is an error with the line (`tests/ui/ui.rs`, `bench/ui.ts` `Placed`). |
| M9-TS-3526 | Integration | `Iface["k"]`, `Alias["k"]` over a type literal (a literal-union property too), nested `A["b"]["c"]`, `typeof x["k"]`, `typeof a.b.c` and `Record<Shape["kind"], V>` (`tests/lang/indexed.rs`). |
| M9-TS-3527 | Integration | Tuples: `T[0]` is that element's type, `T[number]` the union of the elements' types, `T["length"]` a number; `(typeof arr)[number]` the element type. |
| M9-TS-3528 | Integration | `T["a" \| "b"]` and `T[keyof T]` over fields of different types are the union of their types (gap T8); `o[k]` with `k` such a union reads into that union. |
| M9-TS-3529 | Integration | `T[K]` in generic code: `get(o, "x")` is a copy per key, so it returns `T["x"]` (also with written type arguments and a literal-union key); a generic alias `Get<T, K>`, `T["id"]` of a type parameter, a generic method `read<K extends keyof T>(k: K): T[K]`. |
| M9-TS-3530 | Unit | Indexed access errors name the line and the cause: a missing property, a tuple position past the end, a key parameter whose argument is a `string`, `typeof` of a missing property, writing through a key that names fields of different types. |
| M9-TS-3531 | Integration | `new Set(cond ? set : [])`, `new Set(cond ? [] : set)`, `new Map(cond ? [] : map)` and a typed `new Set<T>(cond ? set : [..])`: the arms may be a collection and an array (any iterable); the test runs once and only the arm taken is read (`tests/lang/editor_gaps.rs`). |
| M9-TS-3533 | Integration | `useState(name)` is a lazy initialiser only when `name` there is a function: a local value named like a top-level function is the first value itself (it was called, a compile error), a local arrow and a top-level function are called (`tests/ui/ui.rs`, `bench/ui.ts` `shadowApp`). |
| M9-TS-3534 | Unit | `compile_for_target`: a position-independent object differs from the default one and still exports `ts_<prefix><name>`; on x86-64 Linux the `x86_64-unknown-linux-musl` object is the host's byte for byte; an unknown triple is an error. |
| M9-TS-3535 | Unit | Only a line-leading `function setTimeout` (or `export function setTimeout`) of the program replaces the timer prelude; the words inside a string (a bundled doc) don't (web-ui `web_ui_ts/src/lower/prelude.rs`). |
| M9-TS-3536 | Unit | A compiler extension's `dts_members` are added to the engine classes it names in `platypus.d.ts` (a test extension adds `tag` to a class with a `gameObject`); with no extension nothing is added (web-ui `web_ui_ts/src/types_dts.rs`). |
| M9-TS-3532 | Integration | `{...(cond ? handlers(name) : {})}` in JSX sets the taken arm's props and handlers; the `{}` arm sets none (`tests/ui/ui.rs`, `bench/ui.ts` `condSpreadApp`). |
| M9-TS-3509 | Unit | A function nothing calls is skipped by a build; `check_unused` (`platypus-build build --check-unused`) lowers it too, so its error shows. |
| M9-SHOT-01 | Unit | `progress-shots` waits longer for effects that settle slowly: 120 frames by default, 900 for `dark-room`, 360 for `godrays-lift` (before the godrays rule), 600 for every `fog*` and other `godrays-*` example (`tests/tools/shots/src/bin/progress_shots.rs`). |
| M9-SHOT-02 | Unit | `progress-shots` shrinks a render to the given width keeping its aspect, as JPEG (quality 85) or PNG by the file name, with the `image` crate (`tests/tools/shots/src/bin/progress_shots.rs`). |
| M9-API-1100 | Unit | Asset pack hashes are fixed functions of the bytes (SipHash-1-3 128 content hash, FNV-1a path hash, CRC-32 vectors) and game paths normalize like `resolve_asset` (`crates/platypus_pack/src/hash.rs`). |
| M9-API-1101 | Unit | The LZ4 block codec round-trips (runs, text, noise, matches across the 64 KiB window), reads a reference block, and damaged input is an error, never a panic (`lz4.rs`). |
| M9-API-1102 | Unit | Pack header (64 bytes), entry (64) and chunk (24) records round-trip, the header checksum catches a flipped bit, GUIDs parse from `.meta` hex (`format.rs`). |
| M9-API-1103 | Unit | Entries round-trip by GUID and by path (source and cooked forms of one path) in memory, memory-mapped and positional-read modes; mapped uncompressed reads are views, not copies (`reader.rs`). |
| M9-API-1104 | Unit | Range reads return exactly the requested bytes across chunk edges, for compressed and stored entries. |
| M9-API-1105 | Unit | Identical bytes share chunks; building the same entries twice gives byte-identical packs; the pack id follows the content. |
| M9-API-1106 | Unit | Truncated packs, flipped TOC bytes and flipped data bytes are errors (format or checksum), never panics, over a sweep of damage. |
| M9-API-1144 | Unit | Zstd chunks round-trip whole, in ranges across chunk boundaries and under `verify`; incompressible data stays stored (`platypus_pack`). |
| M9-API-1145 | Unit | Lookup order: a key in a scene pack and in `assets.dat` comes from the scene pack, then `assets.dat`, then the folder; a patch above wins and a tombstone hides both; `pack_of`; the not-found message names `scene data, assets.dat or folder` (`platypus_pack::vfs`). |
| M9-API-1146 | Unit | A loose path that differs only in case from the file on disk reports the real spelling (`DirSource::real_case`: dev warning once, hint in the not-found message). |
| M9-API-1147 | Unit | `Vfs::open_range` reads a compressed pack entry in windows across chunk boundaries, to the end and past it. |
| M9-API-1148 | Unit | `load_sound` loads a WAV whole and streamed from a pack-only `Vfs` (offline mixer, no GPU) and a miss says `not found in assets.dat` (`platypus_app/src/main.rs`). |
| M9-API-1149 | Unit | A UI SVG image loads from the installed asset file system, not the folder (`platypus_render::ui::images`). |
| M9-API-1205 | Unit | `asset_root::mount`: loose folder, packs from `index.json` in any manifest order (scene packs above `assets.dat`), packs found by name without a manifest, a manifest path outside the folder or a missing pack is an error. |
| M9-API-1150 | Manual | `platypus-build package examples/emoji --packs --host target/debug/platypus-app.exe` (and `examples/scenes`), the exe run from its own folder with `--screenshot --frames 120`: assets load from `assets.dat` / scene packs, no `not found` in the log, the picture matches the loose build. |
| ~~M9-API-1151~~ | Unit | `Assets.exists` / `list` / `packOf` in QuickJS answer from the host's index: sorted keys, the `project.json` glob rules, `guid:` and `data:` sources, the pack of each path, nothing without an index (`platypus_engine_api/tests/batch1.rs`). (removed 2026-10-28: QuickJS removed) |
| M9-API-1107 | Unit | Stored, LZ4 and deflate packs read back the same bytes; compressed text shrinks over 4x. |
| M9-API-1108 | Unit | A pack inside a larger file (an APK asset) opens at its offset, mapped and read. |
| M9-API-1109 | Unit | The writer refuses a repeated GUID, a repeated path of the same form, and paths outside the game. |
| M9-API-1110 | Unit | The VFS answers from the last mounted source, a patch tombstone hides an asset, `guid:` and `data:` sources work, unmounting restores the old answer (`vfs.rs`). |
| M9-API-1111 | Unit | A game folder (`DirSource`, `.meta` GUIDs) and a pack answer the same calls with the same bytes, info and GUIDs; paths outside the game are refused. |
| M9-API-1112 | Unit | A path's cooked form is found beside its source form. |
| M9-API-1143 | Unit | A loaded file outside `build/assets.plan` gets a warning that names it and `include`; `.meta`, `build/` and listed files get none (`platypus_core::plan_warning`). |

## M9.b — Windows API (`docs/research/platypus-windows-plan.md`)

| ID | Kind | What it checks |
|---|---|---|
| M9-API-1400 | Unit | Window kinds parse and name round-trip; "Tool" and "" do not parse (`platypus_platform::windows`). |
| M9-API-1401 | Unit | `plan(tool, Windows)`: owner MAIN, small caption, no taskbar button, close button only. |
| M9-API-1402 | Unit | `plan(tool, MacOs)`: owner MAIN, no tool caption or taskbar rules. |
| M9-API-1403 | Unit | An embedded `child` keeps `embed_in` on Windows and X11, becomes a tool window with a warning on macOS, Wayland and others. |
| M9-API-1404 | Unit | Popups have no decorations and never take focus. |
| M9-API-1405 | Unit | `place` centres on the parent and cascades 24 px per axis, wrapping after 8. |
| M9-API-1406 | Unit | `WindowTree::close_order` lists children deepest first. |
| M9-API-1407 | Unit | A parent that would make a cycle is dropped. |
| M9-API-1440 | Unit | `WindowEvent::Moved` converts to `InputEvent::Moved` (physical outer position); `ScaleFactorChanged` maps to `ScaleChanged` (not constructible outside winit, so only `Moved` is asserted). |
| M9-API-1441 | Unit | A one-window `PlatformApp` gets the main window's events through `input` (the default `window_input`), including `CloseRequested`; other windows' are dropped (`crates/platypus_platform/tests/windows.rs`). |
| M9-API-1442 | Unit | `Open` commands over `FakePlatform` open windows and report them (`window_opened` with the physical size); a tool window's plan has the main window as owner; a key opened twice fails. |
| M9-API-1443 | Unit | A window stays hidden until the app lists it in `take_windows_to_show`. |
| M9-API-1444 | Unit | A refused close request keeps the window; closing the main window exits (children closed first) and a script `Close(MAIN)` does nothing. |
| M9-API-1445 | Unit | Closing a parent calls `window_closed` for its children first (`[3, 2]`); a `Close` for a window that went with its parent is harmless. |
| M9-API-1446 | Unit | Title, size, position, kind, flags and minimise reach the window; the plan is recomputed and the OS's `Resized` / `Moved` answers are sent. |
| M9-API-1447 | Unit | `attributes(spec, plan, ..)`: title, inner size, decorations, window level, buttons, hidden start, `active` follow the plan (`platypus_platform::windows`). |
| M9-API-1448 | Unit | Focus moves between windows with `Focused(false)` then `Focused(true)`; `events_done` runs once per step. |
| M9-API-1450 | Unit | iOS, Android and the web fail `Open` with `window_open_failed(key, "Cannot open Window due to platform restrictions (platform: android\|ios\|web)")`; the window never opens and the main window is unaffected (`FakePlatform::os`). |
| M9-API-1451 | Unit | Windows, macOS, X11, Wayland and other desktops open windows without errors (`plan(..).error` is `None`). |
| M9-API-1452 | Unit | A tool window on Wayland still opens, as a plain window, with the warning `Cannot open Window due to platform restrictions: tool windows are not supported on Wayland`. |
| M9-API-1453 | Unit | `window::open_failed` stores `openError`, runs `onOpenFailed` once with the text, leaves `isOpen` false and the main window alone; `Window.isSupported` follows `set_supported` (`crates/platypus_engine_api/tests/window.rs`). |
| M9-API-1454 | Unit | `onOpened` runs from `window::opened` with the client size and scale. |
| M9-API-1455 | Unit | QuickJS: `openError` is `null` until a failure then the text, `Window.isSupported`, `onOpenFailed` (`crates/platypus_native/tests/window.rs`). |
| M9-API-1456 | Integration | Compiled code: `w.openError` (`string \| null`, facade over `openErrorText`), `onOpenFailed`, `Window.isSupported`, with every GC mode (`tools/platypus_ts_tests/tests/ui/windows.rs`). |
| M9-API-1460 | Unit | Script window requests become platform commands: `Create` a tool window is an `Open` with kind Tool and parent MAIN, `Title(MAIN)` is the main title (not a command), `Close(MAIN)` is dropped (`crates/platypus_app/src/windows.rs`). |
| M9-API-1461 | Unit | A window without a position is centred on its parent (outer position and size), each further child 24 px lower and to the right; one with a position keeps it. |
| M9-API-1462 | Unit | Screenshot names are slugs of the titles (`Counter 1` is `counter-1`, `!!` is `window3`, a repeat is `counter-1-4`). |
| M9-API-1463 | Unit | The game counts as focused while any of its windows is (focus moving to a tool window never unfocuses it). |
| M9-API-1464 | Unit | Window targets (`WINDOW_TARGET \| handle`) do not collide with panel ids; physical sizes become UI pixels. |
| M9-API-1470 | Integration | A virtual window (offscreen target, `AppWindow`) draws its UI: a 40 x 40 red box reads back red at (10, 10) and not at (60, 60) (`crates/platypus_app/src/windows.rs`, shared test GPU). Acceptance run: a probe game with a tool window and `--screenshot out.png` writes `out.png` and `out-probe-tool.png` (200 x 120). |
| M9-API-1480 | Unit | `reload_begin` drops every window listener (also on the main window and on windows that stay), queued messages and pending requests, and returns the open windows without a key (never the main one) for the host to close (`crates/platypus_engine_api/tests/window_reload.rs`). |
| M9-API-1481 | Unit | `Window.create({ key })` with the key of a kept window returns that window (same handle, no `Create`), applies the new title, keeps the player's size and position, and lets the new code add listeners; a second `create` with the key is a new window. |
| M9-API-1482 | Unit | `reload_end` returns the keyed windows nobody claimed (once). Acceptance: the probe with `PLATYPUS_RELOAD_AT_FRAME=5 --frames 10` keeps the keyed tool window and replaces the keyless isolated one. |
| M9-API-1483 | Unit | The main window survives a reload with its title; the new code sets it again. |
| M9-API-1484 | Unit | A failed reload (`reload_abort`) gives the old code its window listeners back and forgets the windows the failed code made; the windows the host was to close stay open. |
| M9-API-1490 | Unit | A static property of a handle class is a getter without `this` in the manifest and the `.d.ts` (`static readonly`) (the QuickJS accessor half was removed 2026-10-28) (`crates/ts_api/tests/static_props.rs`). |
| ~~M9-API-1491~~ | Unit | A static setter writes through; assigning to a read-only static throws `TypeError` in strict mode. (removed 2026-10-28: QuickJS removed) |
| M9-API-1410 | Unit | The main window exists from the start (handle 1) and takes the title `set_main` gives (`crates/platypus_engine_api/tests/window.rs`). |
| M9-API-1411 | Unit | Setting a title queues `Title(w, ..)` and reads back at once. |
| M9-API-1412 | Unit | `create` returns a handle at once (2, 3, never reused), queues one `Create`, and the window is not `created` yet. |
| M9-API-1413 | Unit | `create` fills defaults per style: a tool's parent is the main window, a normal window has none, NaN size is 400 x 300, popups do not take focus, sizes clamp to 1..16384, a pending window reads a centred cascade position. |
| M9-API-1414 | Unit | Unknown styles fall back to normal; `child` cannot be set or unset after creation. |
| M9-API-1415 | Unit | Setters of one field coalesce (last value wins) while `Create` and `Close` keep their place; a second `close()` queues nothing. |
| M9-API-1416 | Unit | `opened` fills the native handle, platform, position, scale and the size the OS gave; setting a size keeps the old value until `resized`. |
| M9-API-1417 | Unit | Resize, move, focus and scale events reach their listeners once per change. |
| M9-API-1418 | Unit | An `onCloseRequested` listener can cancel the close; the cancel does not stick. |
| M9-API-1419 | Unit | `cancelClose` outside (or for another window's) close request does nothing. |
| M9-API-1420 | Unit | `closed` runs `onClosed` once and drops the listeners; a closed window takes no new ones. |
| M9-API-1421 | Unit | A closed window reads its last values and ignores setters, `focus` and `close`. |
| M9-API-1422 | Unit | `count` and `at` list open windows main first; `find(key)` and `byHandle`; a taken key still creates the window. |
| M9-API-1423 | Unit | `parent`, `isChild`, `childCount`, `childAt`; a closed parent is ignored. |
| M9-API-1424 | Unit | `Window.current` is the window whose event is being dispatched, else the main window; guards nest. |
| M9-API-1425 | Unit | `off` removes any listener by token, once. |
| M9-API-1426 | Unit | `Window.main.close()` asks to quit (`Application.quit`) and queues no `Close`. |
| ~~M9-API-1430~~ | Unit | A QuickJS call with one options object fills the `#[options]` function's optional parameters by camelCase name (`crates/ts_api/tests/options.rs`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-1431~~ | Unit | Left-out, `undefined` and `null` keys take the declared defaults (NaN, empty, false, the `= value`). (removed 2026-10-28: QuickJS removed) |
| ~~M9-API-1432~~ | Unit | Positional calls still work; plain objects are options objects, arrays, functions, dates, class and value-class instances are not. (removed 2026-10-28: QuickJS removed) |
| M9-API-1433 | Unit | `Window.create({ title, style: "tool", width })` in QuickJS: `Window.main` and `Window.count` are properties, `isChild`, `parent.handle`, flags through the object (`crates/platypus_native/tests/window.rs`). |
| M9-API-1434 | Unit | Window listeners run from QuickJS; `w.onCloseRequested(() => w.cancelClose())` makes `close_requested(w)` false; a closed handle keeps its last values. |
| M9-TS-2800 | Integration | Compiled `Window.create({ title, style: "tool", width, height, key })` maps the keys by name; the tool's parent is the main window (`tools/platypus_ts_tests/tests/ui/windows.rs`, `bench/windows.ts`). |
| M9-TS-2801 | Integration | `Window.main` reads as a property in compiled code and its title goes to the host as `WindowRequest::Title(MAIN, ..)`. |
| M9-TS-2802 | Integration | The static `Window.focused` (undefined when none) and the instance `w.focused` both resolve; `focus_changed` flips both. |
| M9-TS-2803 | Integration | Window listeners from compiled closures run (resize, closed) and `cancelClose` from `onCloseRequested` cancels. |
| M9-TS-2804 | Integration | `Window.count` and `Window.at(i)` list the windows in order and drop a closed one. |
| M9-TS-2805 | Integration | `createRoot(win).render(..)` queues a window mount (`web_ui_bridge::take_window_mounts()`), not a screen mount; `createRoot(Window.main)` is the screen (target 0). |
| M9-TS-2806 | Unit | `platypus-build types` declares `Window` (static readonly `main`, `current`, `count`), `WindowOptions`, `WindowStyle`, and `createRoot(target?: WorldPanel \| Window \| number)` (`tools/platypus_build`). |
| M9-TS-2807 | Unit | `tsc` accepts a program that opens windows, listens, mounts roots, and rejects an unknown style. |
| M9-TS-2820 | Unit | `createRoot(win)` of a window object commits to the target `0x4000_0000 \| handle` (`js/react/test/window_root.test.tsx`); a second root for it names the window in the error. |
| M9-TS-2821 | Unit | `createRoot` of the main window (handle 1) is the screen root (no target); panels still take `{ id }` or a number. |
| M9-TS-2822 | Unit | `@platypus/engine` exports the global `Window` (`js/engine/test/window.test.ts`, with `globalThis.Window` stubbed). Acceptance: a bun-built game with a tool window and an isolated one, run with `--screenshot`, writes both window PNGs and logs `run in domain 3`, `timer in domain 3` and `onOpened`. |
| M9-TS-2831 | Integration | A compiled timer fires in the domain that set it (`ts_rt_timers_advance_domain`): another domain's advance leaves it alone, and `Window.current.domain` inside it is its own; a promise job runs with the domain that queued it (`ts_rt_set_domain_hooks`). |
| M9-TS-2832 | Integration | A paused domain's timers wait (nothing is advanced), then the first advance fires an interval once, not once per missed period. |
| M9-TS-2833 | Integration | QuickJS timers keep their domain: `tick_domain(now, d)` fires only that domain's timers with `__platypus_enter_domain(d)` around each (`crates/platypus_script/tests/domains.rs`). |
| M9-TS-2834 | Unit | Focus moving to a tool window (or its child) keeps the main domain focused: `domain_focused` over a `WindowTree` and a focused set (`crates/platypus_app/src/windows.rs`). |
| M9-TS-2835 | Unit | Focusing an isolated window unfocuses the main domain, which then pauses without `runInBackground` (and not with it); an isolated domain pauses by its own lifecycle. |
| M9-TS-2836 | Unit | The app pauses only when every domain is paused; with the main game paused and another domain live, frames are capped at 30 Hz (`lifecycle::app_paused`, `frame_cap_with`). Acceptance: the screenshot probe with an isolated window writes three PNGs and the isolated `run` / timer log domain 3. |
| M9-TS-2837 | Unit | Events of an isolated window queue for its domain in arrival order (window state events apply at once); a closed root's queue goes with its domain. |
| M9-API-1492 | Unit | Domain focus is per domain: `Window.domainFocused` follows the current domain, `onDomainFocusChanged` runs only for its own domain with that domain current, `Window.off` removes it (`crates/platypus_engine_api/tests/window_families.rs`). |
| M9-API-1493 | Unit | `runInBackground` is kept on a root (`window::run_in_background`), and `Window.current` is the current domain's root unless a window event is entered. |
| M9-API-1495 | Unit | A window without a parent (normal / borderless) is the root of an isolated family with its own domain (`isIsolated`, `root`, `domain`); tools, dialogs and popups default to the creating domain's root; a normal window with a parent joins its family; a parent from another family is ignored. |
| M9-API-1496 | Unit | Another family's windows can be read but not changed (warning, nothing queued) except `run` / `on*` by the creator during setup (until `end_frame`); `run` and listener callbacks run with the root's domain current. |
| M9-API-1497 | Unit | `input` is per event domain: keys, mouse and gamepads of an isolated domain do not reach the main game, `tick_domain` / `blur_domain` act on one domain, and reads follow `window::current_domain()` (`crates/platypus_engine_api/tests/input_domains.rs`). |
| M9-API-1498 | Unit | Messages queue per domain and arrive at the next `deliver`, in posting order with the sender as `from`, the target's listeners before the domain's; ones posted during delivery wait; over 1 MiB is refused; a closed target refuses and drops queued messages (`crates/platypus_engine_api/tests/window_messages.rs`). |
| M9-API-1499 | Unit | A request gets one reply (a second `replyJson` is false), or fails with `no request listener`, `timed out` (once) or `window closed`; a late reply from a domain listener works; `broadcast` reaches the root of every other family and never the sender's own. |
| ~~M9-API-1500~~ | Integration | QuickJS `input` follows the current event domain: main reads its own keys, an isolated window's `run` callback reads its own (keys, mouse, per-domain frame `tick`), a promise job reads the main domain's, `blur` clears only that domain's keys, and listeners belong to the registering domain (`game_input.rs`, `tests/fixtures/src/input_domains.ts`). (removed 2026-10-28: QuickJS removed) |
| M9-API-1501 | Integration | `render_sync` turns native fog (mode, range, colour, height fog, light shafts), auto / fixed exposure and its blend time, a pyramid spot light with its volumetric intensity, a fog volume and `Camera.fog = false` into the renderer's views, and the player's quality switch for auto exposure takes the auto part out again (no GPU; `render_sync.rs`). |
| M9-API-1502 | Unit | `RenderSettings` fog starts off; an unknown mode is ignored; setting a height fog or light shaft property turns it on with the defaults, and its `...Enabled` switch turns it off (`crates/platypus_render_api/tests/fog_settings.rs`). |
| M9-API-1503 | Unit | Auto exposure properties and curve points; `freezeExposure()` sets the manual exposure to the main camera's read-back exposure and turns auto exposure off (nothing to freeze to before the first read-back). |
| M9-API-1504 | Unit | A physical camera's `physicalExposure` is log2(N^2 / t) - log2(ISO / 100); `Camera.fog`, `exposure`, `sceneBrightness` and `resetExposure()` counts. |
| M9-API-1505 | Unit | `RenderStats` is what the host reported; `GraphicsSettings.lightUnits` follows `set_physical_light_units`. |
| M9-API-1506 | Unit | `Light.volumetricIntensity` / `spotShape` / `aspectRatio` (unknown shape ignored), `Light.count()` / `at(i)`, `FogVolume` properties, and both save with scene files. |
| M9-API-1507 | Unit | `LineRenderer2D` starts unfilled (`fillRule` `evenodd`, `sortingOrder` 0); writing `fillColor` turns the fill on, `fillColor = null` takes it away; an unknown fill rule is ignored (`crates/platypus_render_api/tests/line_text.rs`). |
| M9-API-1508 | Unit | A fill saves as `fillColor` (a colour, or `null` for none) next to `fillRule` and `sortingOrder`; loading restores them, `null` clears. |
| M9-API-1509 | Unit | `TextMesh` defaults (32 px, white, centred, 100 pixels per unit, resolution 2), `attach` / `findOn`, an unknown alignment ignored, and it goes with its object. |
| M9-API-1510 | Unit | A `TextMesh` saves and loads with scene files; an unknown property and a wrongly shaped one are each warned about. |
| M9-API-1700 | Integration | The incremental render sync matches a full rebuild: a compiled game makes random changes for 150 frames (renderers made, destroyed, disabled, given other materials; materials recoloured; lights made, destroyed and dimmed; the camera's field of view; 2D shapes' points and fills; planes' sizes; particle rates; line renderer tubes made, bent and destroyed) and after every frame the views' components equal those a fresh sync builds from the registries (`render_sync.rs`). |
| M9-API-1701 | Integration | A frame in which no registry changed walks no renderer (the versions and the pass stay) and still matches a full rebuild (`render_sync.rs`). |
| M9-API-1511 | Integration | A compiled game's filled `LineRenderer2D` (rule, order, `Shapes.rectangle`), an unfilled one, a `TextMesh` and a `SplineAnimate` rider reach the renderer: the text spec in the panels' id space, strokes and fill triangles in the view (no GPU; `render_sync.rs`). |
| M9-TS-2838 | Integration | `win.postMessage(obj)` in compiled code queues the JSON of the argument's static type (`{"kind":"select","p":[1,2,3],"ids":[1,2],"name":"Player"}`, a `Vector3` as an array), to the isolated window's domain (`tools/platypus_ts_tests/tests/ui/windows.rs`). |
| M9-TS-2839 | Integration | `onMessage<T>(f)` (and `onMessage((from, msg: T) => ..)`) parses the message into the declared type; a message with the wrong shape throws inside the listener and is logged through `callback::take_errors`. |
| M9-TS-2840 | Unit | A handle (or a function, Map or Set) anywhere in a message, request or reply type is a compile error naming the path (`message.go`, `message.list[].target`, `reply.who`); `request` without a reply type, and `onMessage` without a type, are compile errors. |
| M9-TS-2841 | Integration | `await win.request<{ n: number }>(..)` resolves with the typed reply once both domains were delivered and the microtasks ran. |
| M9-TS-2842 | Integration | `request` rejects with `timed out`, `window closed` and `no request listener`. |
| M9-TS-2843 | Integration | `Window.broadcast(data)` reaches the other family's `Window.onMessage<T>` listener; the sender's own domain gets nothing. |
| ~~M9-TS-2844~~ | Integration | In QuickJS `win.postMessage(obj)` / `win.onMessage(f)` / `Window.onMessage(f)` / `win.run(f)` round-trip through `deliver` in two domains; a value class travels as `[x, y, z]`; `Window.broadcast` reaches the other family (`crates/platypus_native/tests/window.rs`, `QUICKJS_PRELUDE`). (removed 2026-10-28: QuickJS removed) |
| M9-TS-2845 | Integration | `postMessage` in QuickJS throws `TypeError` (naming the key) for engine handles, functions, `undefined` and cycles and queues nothing; over 1 MiB returns false. |
| M9-TS-2846 | Integration | In QuickJS `win.request(data)` returns a promise that resolves with the parsed reply after both domains delivered and the job queue ran; it rejects with `timed out`, `no request listener`, and `request refused` for a closed window. |
| M9-TS-2847 | Integration | A function returning a number, string, class object, object literal or element is a valid `onClick` (top-level function or arrow); clicking runs it and drops the result, also with a collection at every allocation (`tests/ui/ui.rs`). |
| M9-TS-2848 | Integration | The same functions, passed as a `() => void` callback argument, run and their results are dropped (`tests/ui/ui.rs`). |
| M9-TS-2849 | Integration | Top-level `if`/`else`, `for`, `while`, `do-while`, `switch`, `try/catch/finally`, labelled loops and bare blocks (with block-scoped `let`/`const`) run in source order with the surrounding top-level `const`/`let` and calls, and match bun (`bench/toplevel.ts`: `topLog`, `topNumbers`, `topSquares`; `tests/lang/toplevel.rs`). |
| M9-TS-2850 | Integration | A top-level `if` calls a function declared later in the file (hoisted) and the result matches bun (`topLog`, `later`). |
| M9-TS-2851 | Integration | Control-flow statements at the top of an imported module run when the module is bundled and match bun (`bench/toplevel_dep.ts`, `topImported`). |
| M9-TS-2852 | Integration | A `for` loop at the top level fills an array that functions use later (`topSquares`), also with a collection at every allocation. |
| M9-TS-2853 | Integration | A top-level `return` and a statement kind the compiler cannot lower (a namespace) are compile errors naming the statement and its line; nothing is dropped silently. |
| M9-TS-2854 | Integration | `win.request<R>` and `onRequest<T, R>` in compiled code decode a program-declared interface, a type alias, an array of aliases, `number[]` and a nested anonymous type (with an array and a nullable field), also with a collection at every allocation (`bench/windows.ts`, `tests/ui/windows.rs`). |
| M9-TS-2855 | Integration | In compiled code `win.emit(name, payload?)` reaches `win.on(name, (payload: T, from) => ..)` decoded as the handler's declared type (an interface, an array, no payload); other names and raw messages are ignored without errors (a raw `onMessage` never sees an event); the payload text reaches `onEventJson` (the wire form is `{"$e":name,"d":payload}`); `win.off(token)` removes the listener; also with a collection at every allocation (`bench/windows.ts`). |
| M9-TS-2856 | Integration | `await win.ask<R>(name, payload?)` resolves with the return value of `win.answer(name, f)` for a declared interface, an anonymous object type, a number from an array payload and a string from a handler without parameters. |
| M9-TS-2857 | Integration | In compiled code `ask` of a name no `answer` handles rejects at once with `no answer for "<name>"` (before the timeout), also for a window with no listener or only a raw `onRequest`. |
| M9-TS-2858 | Integration | An empty event name throws `Window event names must not be empty`; an event over 1 MiB as JSON is refused (`emit` returns false, `ask` rejects with `request refused`). |
| M9-TS-2859 | Integration | `Window.emitAll(name, payload?)` reaches the other family's raw listener as the named-event envelope. |
| M9-TS-2860 | Unit | A handle in an `emit` payload, `ask` without a reply type and an `on` handler whose payload type cannot be inferred are compile errors. |
| M9-TS-2861 | Integration | In QuickJS `win.emit(name, payload?)` reaches `win.on(name, (payload, from) => ..)` (also from an isolated domain to the main window); other names and raw messages are ignored; raw listeners do not see events; `off(token)` removes the listener; `Window.emitAll` reaches the other family (`crates/platypus_native/tests/window.rs`). |
| M9-TS-2862 | Integration | In QuickJS `win.ask(name, payload?, timeoutMs?)` resolves with what `win.answer(name, f)` returned (`null` for no return value), rejects at once with `no answer for "<name>"` for an unanswered name, also for a window without request listeners. |
| M9-TS-2863 | Integration | In QuickJS an empty event name throws `TypeError: Window event names must not be empty` from `emit`, `on`, `answer`, `ask` and `Window.emitAll`; handles in a payload throw; over 1 MiB makes `emit` return false and `ask` reject with `request refused`. |
| M9-TS-2864 | Integration | In compiled code raw `onMessage` / `onRequest` listeners never see named-event envelopes (`emit`, `emitAll`, `ask`); `on` ignores raw messages. |
| M9-TS-2865 | Integration | In QuickJS events and raw messages are separate layers: raw `onMessage` / `onRequest` miss `emit` / `ask`, `on` / `answer` miss `postMessage` / `request`; a hand-posted `{"$e":..}` object counts as an event. |
| M9-TS-2866 | Integration | `on_event_json(name)` / `on_answer_json(name)` get only their name's envelopes (payload text, `""` without one); an unanswered named request fails at once with `no answer for "<name>"`; `off` removes an answer; malformed envelopes are raw messages. |
| M9-TS-2867 | Unit | `window::parse_event` reads `{"$e":name}` / `{"$e":name,"d":payload}` (escaped names, nested payloads) and nothing else. |
| M9-TS-2870 | Unit | A bare import is a file, a folder index or a subpath of `src/modules/` (`.ts` / `.tsx`), loaded before the importer. |
| M9-TS-2871 | Unit | A bare import that is not found says where it looked (`src/modules/foo.ts`, ...) and to copy a package's `.ts` source in. |
| M9-TS-2872 | Unit | `@platypus/*` and `react` stay erased and win over a module of the same name. |
| M9-TS-2873 | Unit | Modules import each other relatively and by name; a module imported twice is in the bundle once. |
| M9-TS-2874 | Integration | A program importing a `src/modules` file by bare name compiles and runs. |
| M9-TS-2875 | Unit | The `tsc` config maps bare imports to the entry's `modules/` folder (`paths`). |
| M9-TS-2876 | Integration | A top-level function nothing refers to is not compiled (an unsupported construct in it does not fail the build); one called directly, through another function or as a value still fails with its line. |
| M9-TS-2877 | Unit | A dependency that declares an engine global (`class Random`) is renamed (`Random_2`) in its own file and what imports it; the engine's `Random` stays. |
| M9-TS-2878 | Unit | `global_names` reads the declared names of `types/platypus.d.ts` (classes, namespaces, functions, constants, enums, interfaces, aliases). |
| M9-TS-2879 | Integration | A build of a game whose `src/modules` file declares `class Random` keeps the engine's `Random.value` and renames the module's class. |
| M9-TS-2880 | Integration | `RenderSettings.heightFog` / `volumetricFog` / `autoExposure` / `fixedExposure` assignments of objects (also `cond ? {..} : null`) are facade writes over the field setters; `fixedExposure` reads back as a number or null (`bench/facade.ts`). |
| M9-TS-2881 | Integration | `Mesh.createFlat(..)` / `mesh.setFlat(..)` take typed arrays or `number[]` (copied at the engine boundary), and `Random.pick(list)` is a random item or undefined. |
| M9-TS-2882 | Integration | `Light.illuminanceAt` sums each light's lux on a point (directional head-on, point and spot in lumens over 4 pi); `Light.count()`. |
| M9-TS-2883 | Integration | An optional `rotation?: Quaternion` passed into `new GameObject(name, { position, rotation })` is left alone when missing, and `props.on === false` on an optional boolean is false for a missing one. |
| M9-TS-2884 | Integration | `RenderSettings.bloom = b` with `b: BloomSettings | null` (not a literal) goes through the facade write; the examples of the fog and god rays family build and render (checked by eye, `tests/output`). |
| M9-TS-2885 | Integration | `Shapes` makes GDI-style outlines (rectangle, rounded rectangle, ellipse, arc, pie, polygon, open and closed curves) with the right point counts and positions (`bench/facade.ts`). |
| M9-TS-2886 | Integration | `Spline` (a TypeScript class in the facade): length, `evaluate` by distance, `nearest`, closed splines, `insertKnot` / `removeKnot` / `setPosition` / `setMode`, `tangent`, `sample`, `version`. |
| M9-TS-2887 | Integration | `SplineAnimate` places its object by distance along the spline (through a compiled game in `render_sync.rs`; see M9-API-1511). |
| M9-TS-2888 | Integration | `LineRenderer2D.fillColor` reads and writes a colour or `null` (also in an `addComponent` options object) and `sortingOrder`; `TextMesh` takes its options. |
| M9-TS-2889 | Integration | `Camera.screenToWorldPoint` for an orthographic camera (middle and corner) and a perspective one (60 degrees, 10 in front). |
| M9-TS-2890 | Integration | Interface method signatures (also optional ones), named tuple elements and a method shorthand in an object literal compile and run (`tests/lang/jit.rs`). |
| M9-TS-2891 | Integration | A closure inside a closure in a top-level initialiser shares the outer closure's locals. |
| M9-TS-2892 | Unit | An error after a multi-line JSX element is reported at its own line. |
| M9-TS-2893 | Unit | A comment naming a facade trigger does not pull in a chunk written over a class the program's engine API lacks. |
| M9-API-1512 | Integration | `await SceneManager.loadSceneAsync(path, mode)` in a compiled game resolves with the `Scene` (its `path` is set) in the tick that read the file; code after the await runs in that tick (`native.rs`). |
| M9-API-1513 | Integration | `loadSceneAsync` rejects with an `Error` for a missing file, a file that is not a scene and a path outside the game folder; an `async onStart()` can await it (`native.rs`). |
| M9-API-1514 | Integration | A component awaiting a delay in an object that is destroyed meanwhile continues after the await; writing to the dead object does nothing (GC stress, `native.rs`). |
| M9-API-1515 | Integration | An `async () => {..}` JSX handler (`onClick`) awaits a microtask and a `Task.delay` and the state it sets re-renders (`native.rs`). |
| M9-API-1516 | Integration | A hot reload drops the old code's timers and awaits (`ts_rt_events_clear`): only the new code's own wait fires (`native.rs`). |
| M9-API-1517 | Integration | `Prefab.load(path)` resolves with the prefab (instantiable) and rejects for a missing file (`native.rs`). |
| M9-API-1518 | Integration | Components that a UI event's handler re-renders, and their effects, run with the scene current (`Native::settle`; also the world panels' and windows' settle): a `useState` initializer's `new GameObject` is made and `GameObject.roots` is seen (`native.rs`). |
| M9-TS-2894 | Integration | A compiled `answer` handler may be `async` (`Promise<R>`): the reply goes out when the promise resolves, a rejection fails the asker's `ask` with the error's message (`tests/windows.rs`). |
| M9-TS-2895 | Integration | A compiled raw `onRequest` handler may be `async`: it replies later, and a rejection before the reply fails the `request`. |
| M9-TS-2896 | Integration | In QuickJS async `answer` and `onRequest` handlers reply when resolved and reject to the asker (`Window.replyError`). |
| M9-TS-2897 | Integration | `async function load<T>(x: T): Promise<T>` and `load<number>(3)` / `load(true)` (bun-compared, `bench/promises2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-2898 | Integration | `async function wrap<T>(f: () => T): Promise<T>`: `T` inferred from a closure's result (bun-compared). |
| M9-TS-2899 | Integration | `Promise<T[]>` from a generic function (bun-compared). |
| M9-TS-2900 | Integration | `Promise<Map<string, T>>` from a generic function when `Promise<Map<string, number>>` is written once (bun-compared). |
| M9-TS-2901 | Integration | `function delay<T>(ms, v: T): Promise<T>` with `new Promise<T>(..)` and `setTimeout` (bun-compared). |
| M9-TS-2902 | Integration | Generic `retry<T>(f: () => Promise<T>)`: `T` inferred from a promise-returning closure; `try`/`catch` around `await` in a loop (bun-compared). |
| M9-TS-2903 | Integration | `function twice<T>(p: Promise<T>): Promise<T[]>` with `.then` on a generic promise (bun-compared). |
| M9-TS-2904 | Integration | Generic `each<T, U>(xs, f): Promise<U[]>` over `Promise.all(xs.map(f))` (bun-compared). |
| M9-TS-2905 | Integration | `class Box<T>` holding a `Promise<T>` and an async method returning `Promise<T>` (bun-compared). |
| M9-TS-2906 | Integration | A generic function promising a program class (inferred and written `<Foo>`): the promise class exists for every declared class once a generic promise is written (bun-compared). |
| M9-TS-2907 | Integration | A recursive generic async function (bun-compared). |
| M9-TS-2908 | Integration | Async methods keep `this` across awaits (bun-compared). |
| M9-TS-2909 | Integration | Promises of interfaces (object literal result) and of `T \| null` (bun-compared). |
| M9-TS-2910 | Integration | `await` in `try`, `catch` and `finally` blocks (bun-compared). |
| M9-TS-2911 | Integration | `for await (const x of ..)` over an array of promises and over plain values, with `continue` / `break` (bun-compared). |
| M9-TS-2912 | Integration | `Promise.withResolvers<T>()` (value, `void`, rejection; bun-compared). |
| M9-TS-2913 | Integration | `await` of a plain value, of an async IIFE, of `Promise.resolve()` and `Promise.resolve("literal")` (bun-compared). |
| M9-TS-2914 | Integration | Timers, `Promise.all` and `Promise.race` inside async code (bun-compared). |
| M9-TS-2915 | Integration | `catch`, `finally` and `allSettled` over rejections (bun-compared). |
| M9-TS-2916 | Integration | An unhandled rejection is reported once, after the microtask queue drained, as a level-4 console line with the error's `name: message`, and the program goes on. |
| M9-TS-2917 | Integration | A rejection handled in the same drain is not reported; a `then` chain reports only its last promise. |
| M9-TS-2918 | Integration | An exception escaping a `setTimeout` or `queueMicrotask` callback is reported and the other timers still run. |
| M9-TS-2919 | Integration | Clear compile errors: `Promise.all` of promises of different types, top-level `await`, `Promise<Promise<T>>`, async generators, a generic promise of a type no `Promise<..>` names (alias). |
| M9-TS-2600 | Unit | There is one host, `platypus-app` (`package::HOST`); a missing explicit `--host` is an error. |
| M9-TS-3000 | Integration | `let mode: M = "a"; if (o.mode !== undefined) mode = o.mode;` (a union local assigned from an optional field; formerly a Cranelift panic) equals bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3001 | Integration | The same with `as M`, a `const` copy and `o.mode as M` equals bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3002 | Integration | `??` over optional and nullable union members equals bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3003 | Integration | Union members as arguments, optional parameters, return values and `Dir | undefined` results equal bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3004 | Integration | Union members in class fields, arrays, `Map` keys and values (`Map.get` over members), object literals equal bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3005 | Integration | `switch`, `===` and `!==` over a union and over `union | undefined` equal bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3006 | Integration | Comparing members, `indexOf` / `lastIndexOf` / `includes` over union arrays equal bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3007 | Integration | A member read as a `string` (`toUpperCase`, `length`, `+`, templates, `join`) equals bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3008 | Integration | `keyof typeof TABLE` is the union of the object's keys; `TABLE[k]` equals bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3009 | Integration | A loop carrying a union local assigned from optional fields equals bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3010 | Integration | `JSON.stringify` / `JSON.parse` of an optional member field and 200 objects under GC churn equal bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3011 | Integration | `switch` over an optional member field of a class, with `case undefined` equals bun's (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3012 | Integration | A string literal outside the declared union (`const m: M = "c"`) is a compile error naming the literal and the members, with a line number (`platypus_ts_tests/tests/lang/unions.rs`, `bench/unions_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3013 | Integration | A conditional of two object shapes that can't unify is a compile error naming both shapes by their fields (`cannot unify { bottom: f64 } and { top: f64 }`), not by internal ids (`platypus_ts_tests/tests/lang/unions.rs`). |
| M9-TS-3020 | Integration | `Array.from({ length: n }, f)` over numbers, strings, objects and booleans equals bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3021 | Integration | `Array.from(array | Set | map.keys() | map.values(), f)` equals bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3022 | Integration | `new Array<T>(n)`, `new Array(n).fill(x)` (element type from `x`), `.fill(..).map(..)` equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3023 | Integration | `fill(x, start, end)` with negative and out-of-range bounds, and the returned array, equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3024 | Integration | `flat()`, `flat(2)`, `flatMap` over numbers, strings and objects equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3025 | Integration | `indexOf` / `lastIndexOf` / `includes` (with `fromIndex`) over numbers, strings, union members, booleans and objects equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3026 | Integration | `Math.min(5, ...xs)`, `Math.max(...xs)`, `f(a, ...xs)` into rest parameters equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3027 | Integration | `add3(...tuple)` and `greet(...pair)` into fixed parameters equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3028 | Integration | Array and object spread `[...a, ...b]`, `{ ...o, y: 5 }` still equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3029 | Integration | `arr[arr.length] = x` grows the array (numbers, strings, objects, union members; 40 appends) equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3030 | Integration | An out-of-bounds read is `undefined` (also negative and with `??`), never garbage, equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3031 | Integration | Spread edge cases: defaults and rest after a spread, method calls, typed arrays, `flat(Infinity)`, `flat(0)` equal bun's (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3032 | Integration | A write past the length, negative or fractional throws a `RangeError` and leaves the array as it was (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3033 | Integration | The `RangeError` message names the index (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3034 | Integration | Out-of-range writes to a typed array are still ignored (`platypus_ts_tests/tests/lang/arrays2.rs`, `bench/arrays2_cases.ts`, normal, GC stress, GC off). |
| M9-TS-3100 | Integration | No hand-written declaration file under `tools/` (no `.d.ts`, no `include_str!` of one; strict since `engine_ts.d.ts` went), the TypeScript compiled into games is the listed set (`ui.ts`, `temporal.ts`) and no Python step is under `tools/web_ui_ts` (`tools/platypus_build/tests/generated_dts.rs`). |
| M9-TS-3101 | Integration | The compiler built-ins declare no top-level name a manifest declares, and every class or namespace a built-in member, static or constructor belongs to exists in the manifests (`tools/platypus_build/tests/generated_dts.rs`). |
| M9-TS-3102 | Integration | The generated `types/platypus.d.ts`, parsed with oxc and normalised (one line per declaration and member, sorted), equals the blessed snapshot `tools/platypus_build/tests/golden/api_surface.txt`; `PLATYPUS_BLESS=1` rewrites it (`tools/platypus_build/tests/generated_dts.rs`). |
| M9-API-1600 | Integration | `Shapes` from QuickJS return `Vector2` instances with the old defaults (64 points, rounded corners, rectangle corners) (`crates/platypus_native/tests/outlines.rs`). |
| M9-API-1601 | Integration | `Shapes.arc`, `pie`, `regularPolygon`, `triangle`, `bezier`, `curve`, `closedCurve` from QuickJS (the cases of the old JS test `M5-RND-25`) (`crates/platypus_native/tests/outlines.rs`). |
| M9-API-1602 | Integration | `Shapes.polygon(...points)` takes arguments, a spread and none; a bad argument is a `TypeError` (`crates/platypus_native/tests/outlines.rs`). |
| M9-API-1610 | Integration | The manifest lists enums, records, list results, a rest parameter, `options_after`, `like` and the doc comments, in `Namespace` and in the JSON (`crates/ts_api/tests/records_enums.rs`). |
| M9-API-1611 | Integration | The enum helpers (`name`, `parse`, `index`, `from_index`, the names list) convert both ways (`crates/ts_api/tests/records_enums.rs`). |
| M9-API-1612 | Integration | List and record results come back flat through `ts_api_result`; enums travel as indexes (unknown ones are the first value); `platypus_<Enum>_name` gives a name (`crates/ts_api/tests/records_enums.rs`). |
| ~~M9-API-1613~~ | Integration | QuickJS gets arrays, objects, strings, rest arguments and an options object after a parameter (`crates/ts_api/tests/records_enums.rs`). (removed 2026-10-28: QuickJS removed) |
| M9-API-1614 | Integration | A create with `options(after = ..)` reads the parameters after the named one through the shim (`crates/ts_api/tests/records_enums.rs`). |
| M9-API-1615 | Integration | A program with a pending top-level `await` shows the UI it mounted before it, keeps the registered class unspawned for frames, and spawns it (and runs its first update) once the timer lets the module finish (`crates/platypus_app/src/native.rs`). |
| M9-API-1616 | Integration | A rejected top-level `await` stops the startup: `startup_error()` names the module and the error and nothing spawns (`native.rs`). |
| M9-API-1617 | Integration | A scene that names a component class no `register` call named gets a warning naming the class and saying it never registered (`native.rs`). |
| M9-API-1618 | Integration | A hot reload into a program that awaits restores the saved world and spawns the new class once its startup has finished (`native.rs`). |
| M9-API-1619 | Unit | `@platypus/engine`'s `Assets.exists` / `list` / `packOf` hand over to the engine's `Assets` namespace (QuickJS parity) and answer as an empty game without it (`js/engine/test/assets.test.ts`). |
| M9-API-1620 | Integration | A `Vec<String>` result of `#[ts_api]`: the shim returns the count, `ts_api_result_str(i)` gives the strings (empty past the end), the manifest says `list:string` (`crates/ts_api/tests/records_enums.rs`). |
| M9-TS-3103 | Unit | Enums, records, lists, rest, docs, `like` and `options_after` become declarations (`tools/web_ui_ts/src/types_dts.rs`). |
| M9-TS-3110 | Integration | Every point of every `Shapes` function on edge cases equals the old compiled TypeScript (`RefShapes`) bit for bit, in normal, stress GC and collection-off modes (`tools/platypus_ts_tests/tests/engine_api/outlines.rs`, `bench/outlines.ts`). |
| M9-TS-3111 | Integration | Shape points are `Vector2` objects; object literals pass where points go (`tools/platypus_ts_tests/tests/engine_api/outlines.rs`). |
| M9-API-1603 | Integration | `Spline` from QuickJS: through its knots, the same points as `Shapes.curve`, linear and bezier knots, evaluate by distance, closing, `sample` counts, `instanceof` and `[object Spline]` (`crates/platypus_native/tests/splines.rs`; the cases of the old JS test `M5-SPL-01`). |
| M9-API-1604 | Integration | `Spline.nearest`, `insertKnot` / `setPosition` / `setMode` / `setTangents` / `removeKnot`, version, and `knots` as a snapshot of copies (`crates/platypus_native/tests/splines.rs`). |
| M9-API-1605 | Integration | `TangentMode` strings, `name` / `parse`, and a `TypeError` for an unknown name (`crates/platypus_native/tests/splines.rs`). |
| M9-TS-3112 | Integration | Native splines equal the old compiled TypeScript (`RefSpline`) on every number, for 0 to 6 knots, open / closed, three tensions, and after every mutator, in every GC mode (`tools/platypus_ts_tests/tests/engine_api/outlines.rs`). |
| M9-TS-3114 | Integration | `Spline.knots`, `locate` and `nearest` give plain objects, `TangentMode` is a string with `name` / `parse` (`tools/platypus_ts_tests/tests/engine_api/outlines.rs`). |
| M9-TS-3113 | Integration | `SplineAnimate.pose(..)` equals the old component's `t` and direction for every loop mode, duration (0 and negative included) and elapsed time (`tools/platypus_ts_tests/tests/engine_api/outlines.rs`). |
| M9-TS-3115 | Integration | A `SplineAnimate` added with options and found again; the host `spline::update` moves its object along a ping-pong path (`tools/platypus_ts_tests/tests/engine_api/outlines.rs`). |
| M9-TS-3116 | Integration | A `SplineAnimate` places its object on the path facing along it, in every loop mode and in a container's space (the old `js/engine` `SplineAnimate` test) (`tools/platypus_ts_tests/tests/engine_api/outlines.rs`). |
| M9-API-1606 | Integration | `SplineAnimate.pose`, `progress` and `facing` from QuickJS (`crates/platypus_native/tests/splines.rs`). |
| M9-API-1607 | Integration | A `SplineAnimate` and its spline survive a scene file: the inline `Spline` asset, loop mode and alignment names, the container reference (`crates/platypus_native/tests/splines.rs`). |
| M9-TS-3120 | Integration | `function f(o: Partial<Opts> = {})` with the parameter left out, partly and fully given equals bun's (`tools/platypus_ts_tests/tests/lang/utility.rs`, `bench/utility_cases.ts`). |
| M9-TS-3121 | Integration | A full value (and `{}`) where a `Partial<T>` goes; absent fields read as `undefined` (`utility.rs`). |
| M9-TS-3122 | Integration | `Pick<T, K>` / `Omit<T, K>` over an interface with function and data fields; a full value goes where the picked type is expected (`utility.rs`). |
| M9-TS-3123 | Integration | `Required<T>` and `Readonly<T>` (`utility.rs`). |
| M9-TS-3124 | Integration | `Partial<Class>` for a class declared after its use; methods of the class still work (`utility.rs`). |
| M9-TS-3125 | Integration | `{ ...base, ...look }` with a `Partial` spread: a field that is absent leaves the earlier value (`utility.rs`). |
| M9-TS-3126 | Integration | `Partial<Record<Union, V>>` tables and arrays of `Partial` objects (`utility.rs`). |
| M9-TS-3127 | Integration | `Partial<LineRenderer2D>` / `Partial<TextMesh>` option bags spread into `addComponent` options, or passed as a variable (`tools/platypus_ts_tests/tests/engine_api/facade.rs`). |
| M9-TS-3128 | Integration | `const f = Util.twice`, `Util.label`, `Math.abs`, `Math.max`, `Math.pow` as values; static methods and `Math.sqrt` passed as arguments equal bun's (`tools/platypus_ts_tests/tests/lang/misc.rs`, `bench/misc_cases.ts`). |
| M9-TS-3129 | Integration | `xs.map(Math.abs)`, `map(Math.floor)`, `map(Math.round)`, `filter(Math.abs)` equal bun's (`misc.rs`). |
| M9-TS-3130 | Integration | `null as T \| null`, `[] as T[]`, `{} as T` in object literals, consts and locals (`misc.rs`). |
| M9-TS-3131 | Integration | Class fields with no annotation initialised by `new Float32Array(n)`, `new Map<K, V>()`, `new Array<T>(n)`, `new Set<T>()` (`misc.rs`). |
| M9-TS-3132 | Integration | `x[i]! += d`, `x[i]!++`, `m.get(k)![0]! *= 3`, `o.p!.n = 7`, `(x[i] as number) -= 1` (`misc.rs`). |
| M9-TS-3133 | Integration | Closures over `const [a, b] = ..` / `{ a, b } = ..` and `for (const [i, s] of xs.entries())` (`misc.rs`). |
| M9-TS-3134 | Integration | `[..."héllo"]`, `Array.from("a😀b")` (by code point), `Array.from(s, f)`, `for (const c of s)`, `Math.hypot(...xs)` (`misc.rs`). |
| M9-TS-3135 | Integration | `const hex = Color.hex`, `["#00ff00"].map(Color.hex)`, `Mathf.lerp` / `Mathf.clamp` as values (`tools/platypus_ts_tests/tests/engine_api/facade.rs`). |
| M9-TS-3136 | Integration | `ok ? [a, b] : []` and `[]` first: the empty array takes the other branch's type (`misc.rs`). |
| M9-TS-3137 | Integration | Field types found from `NaN`, `-Infinity`, `Math.PI`, `Number.MAX_SAFE_INTEGER`, `Math.sqrt(..)`, `new Int8Array(n).fill(x)`, `new Map<K, V>()`, `[] as T[]` in an object literal (`misc.rs`). |
| M9-TS-3138 | Integration | `new GameObject(name, cond ? { position, parent } : { position })` (`facade.rs`). |
| M9-TS-3140 | Integration | `interface Shape { name; area(); scaled(k) }` implemented by two classes: arrays, variables and parameters of the interface type call and read through it, equal bun's (`tools/platypus_ts_tests/tests/lang/ifaces.rs`, `bench/ifaces_cases.ts`). |
| M9-TS-3141 | Integration | A class, an object literal with methods and an arrow function property all go where one interface is expected (`ifaces.rs`). |
| M9-TS-3142 | Integration | `interface Animal extends Named`; a subclass instance in an `Animal[]`, overriding, a value of the extended interface passed where its parent is expected (`ifaces.rs`). |
| M9-TS-3143 | Integration | Optional interface members (`show?()`) read and called with `?.()`, and `face instanceof Fancy` narrowing an interface value to the class (`ifaces.rs`). |
| M9-TS-3144 | Integration | Writes through an interface (`g.level += 3`) to a field, an accessor pair and an object literal, with optional data members (`ifaces.rs`). |
| M9-TS-3145 | Integration | Interface values in a `Map`, in closures and filters, 40 objects built and collected in every GC mode (`ifaces.rs`). |
| M9-TS-3146 | Integration | An abstract class implementing two interfaces, `this` and `super.m()`, a method with a default parameter called through an interface with the argument left out, arrays of the interface inside objects (`ifaces.rs`). |
| M9-TS-3147 | Integration | Spread (and destructuring writes, JSON, `Object.keys`) of an interface implemented by classes is a compile error naming the interface (`ifaces.rs`). |
| M9-TS-3148 | Integration | A computed key in a style object, `style={{ [side]: 8 }}` with `side` a string-literal union (`tools/platypus_ts_tests/tests/ui/ui.rs`). |
| M9-TS-3149 | Integration | Tuple `.length`, `t[0] = x`, `t[1] += "z"`, `list[1]![0] = 9` on tuples of mixed types and arrays of tuples equal bun's (`tools/platypus_ts_tests/tests/lang/tuples.rs`, `bench/tuples_cases.ts`). |
| M9-TS-3150 | Integration | `const [first, ...rest] = tuple` (the rest is a tuple), `[...tuple]`, `[...tuple, 7]`, `f(...tuple)` into fixed parameters of different types (`tuples.rs`). |
| M9-TS-3151 | Integration | Tuple parameters (`([n, s], i) =>`, `function f([a, b]: [number, string])`), `for (const [n, s] of pairs)`, `.map(([k, v]) => ..)` (`tuples.rs`). |
| M9-TS-3152 | Integration | Destructuring assignment: swaps, `[p[0], p[3]] = [p[3], p[0]]`, `[n, s] = pair()`, `({ x, y } = o)`, defaults, `[first, ...rest] = xs`, element targets in nested arrays (`tuples.rs`). |
| M9-TS-3153 | Integration | A bubble sort and Fibonacci written with `[a, b] = [b, a + b]` swaps (`tuples.rs`). |
| M9-TS-3154 | Integration | Closure parameters of type `boolean` with defaults (`(flag = false, n = 1) =>`) left out at a call (`tuples.rs`). |
| M9-TS-3155 | Integration | `await Promise.all([num(3), str("x")])` over promises of different value types (and a plain value) gives a tuple, in order, with timers (`tools/platypus_ts_tests/tests/lang/promises2.rs`). |
| M9-TS-3156 | Integration | The tuple form of `Promise.all` rejects with the first failure (`promises2.rs`). |
| M9-TS-3157 | Integration | Top-level `await`: module code runs in order across awaits, a function declared after sees the module variables (`tools/platypus_ts_tests/tests/lang/tla.rs`, `bench/tla_cases.ts`). |
| M9-TS-3158 | Integration | Top-level awaits inside `for`, `if`, `for await` and `try/catch` (`tla.rs`). |
| M9-TS-3159 | Integration | Top-level `await` with classes, closures and a later function (`tla.rs`). |
| M9-TS-3160 | Integration | `const [a, b] = await Promise.all([p, q])` at the top level (`tla.rs`). |
| M9-TS-3161 | Integration | Only the code before the first top-level `await` has run when `ts_init` returns (`tla.rs`). |
| M9-TS-3162 | Integration | Top-level destructuring declarations: `const [a, b] = ..`, `const { p, q } = ..`, `let [m, s] = pair()`, `const [h, ...t] = ..` become module variables (`misc.rs`). |
| M9-TS-3163 | Integration | `f(...xs)` / `f(a, ...xs)` for a function value (the elements feed the parameters, null past the end), `new C(...xs)` and `rnd(...tuple)` equal bun's (`tools/platypus_ts_tests/tests/lang/misc.rs`). |
| M9-TS-3164 | Integration | The generated declarations are the ones games are written against: handle properties and options are `T \| null`, a property that is one of a few names reads as the union (`get type(): "directional" \| ..`, `set type(v: string)`), a namespace one is the union (`tools/platypus_build/tests/generated_dts.rs`). |
| M9-TS-3165 | Integration | `state[key] = !state[key]`, `counts[k] += n`, `counts[k]++` with `key: keyof typeof state` or a union of names equal bun's (`tools/platypus_ts_tests/tests/lang/tables.rs`, `bench/tables_cases.ts`). |
| M9-TS-3166 | Integration | Tables indexed by a tag: `NAMES[k]`, `TINTS[k]`, `Record<Tint, V>` over `keyof typeof TINTS`, `NAMES.warm = ..` (`tables.rs`). |
| M9-TS-3167 | Integration | `Record<string, V>` as a dictionary: a literal, `dict[k]`, `dict[k] = v`, `dict[k] += n`, `dict.name`, a missing key is `undefined` (`tables.rs`). |
| M9-TS-3168 | Integration | A `Record<"a" \| "b", V>` as a `Record<string, V>`: `(POSES as Record<string, number>)[name]`, assignment, then writes to the copy (`tables.rs`). |
| M9-TS-3169 | Integration | A key that is a parameter written as a union of some names indexes only the fields it can name, so the object may have fields of other types (`tables.rs`). |
| M9-TS-3170 | Integration | `(typeof NAMES)[number]` over `as const`, `(typeof TINTS)[Tint]`, optional tuple elements (`[number, number, "c"?]`), `s[i]` on a string, `Array.from({ length }, () => null)` as `(T \| null)[]` (`tables.rs`). |
| M9-TS-3171 | Integration | A prop typed `object` is a bag of style properties spread into `style` (`tools/platypus_ts_tests/tests/ui/ui.rs`). |
| M9-TS-3172 | Integration | `Pick<Ctx, ..>` of an interface that reaches the pick through its own members (`Ctx.scene: Effects`, `Effects.host: Host`) (`tools/platypus_ts_tests/tests/lang/utility.rs`). |
| M9-TS-3173 | Integration | A generic function with `T = void` (`under<void>(() => { n++; })`, `return f()` of a void call) (`tools/platypus_ts_tests/tests/lang/misc.rs`). |
| M9-TS-3174 | Integration | A generic function whose type argument appears only as the result of a closure that returns nothing (`under(root, () => this.build())`) equals bun's (`tools/platypus_ts_tests/tests/lang/originals.rs`, `bench/originals_cases.ts`). |
| M9-TS-3175 | Integration | Unions of different types: `number \| [number, number]`, `number \| string \| string[]`, narrowing by `typeof`, `Array.isArray`, `?:`, early `return`, and `typeof u` on a union equal bun's (`originals.rs`). |
| M9-TS-3176 | Integration | A written return type on an array callback (`rows.map(([a, b]): Star => ({ .. }))`) with spreads of the result into a typed array literal (`originals.rs`). |
| M9-TS-3177 | Integration | `ReturnType<typeof store<string>>`, `ReturnType<typeof f>`, `Parameters<typeof f>`, `Exclude`, `Extract`, `NonNullable<typeof local>`, `Awaited` equal bun's (`originals.rs`). |
| M9-TS-3178 | Integration | `new (a.constructor as new (n: number) => T)(len)` for typed arrays and classes inside a generic arrow declared in a function equals bun's (`originals.rs`). |
| M9-TS-3179 | Integration | A constant object whose methods call the constant (`sfx.play` calls `sfx.preload`) equals bun's (`originals.rs`). |
| M9-TS-3180 | Integration | A getter in an object literal is evaluated on each read, also when the object it reads is set after the literal (`originals.rs`). |
| M9-TS-3181 | Integration | `(name && lookup(name)) || fallback` with a string and an object equals bun's (`originals.rs`). |
| M9-TS-3182 | Integration | `Partial<Record<Name, V>>` over a union of 53 names is a dictionary: reads, missing keys (`originals.rs`). |
| M9-TS-3183 | Integration | A union-typed value keeps its union in an object literal (`indexOf(state.mode)`), `Map<Sfx, V>.keys()` feeds a function taking `Sfx`, a cycle through `MODES` equals bun's (`originals.rs`). |
| M9-TS-3184 | Integration | `pop` / `shift` over an array of handles give a handle or `undefined` (`tools/platypus_ts_tests/tests/engine_api/originals_engine.rs`, `bench/originals_engine.ts`). |
| M9-TS-3185 | Integration | `LineRenderer2D.points` reads and writes as a property (`originals_engine.rs`). |
| M9-TS-3186 | Integration | `new Color(...rgba)` and `new Color(...rgb)` from arrays: a missing alpha is 1 (`originals_engine.rs`). |
| M9-TS-3187 | Integration | `Mesh.createFlat` and `setFlat` with only some of the lists (`originals_engine.rs`). |
| M9-TS-3188 | Integration | An engine enum string meets a program's literal union: `RenderSettings.fogMode` read into a `FogMode` variable, a table over `FogMode \| "off"`, `MODES.indexOf(state.mode)` (`originals_engine.rs`). |
| M9-TS-3189 | Integration | Handles as `Map` values, `Map` keys and `Set` members (`originals_engine.rs`). |
| M9-TS-3190 | Integration | `Object.keys` / `values` / `entries` as values, `for (const k in o)`, `Object.fromEntries` and `"b" in R` over a `Record<Union, V>` equal bun's (`tools/platypus_ts_tests/tests/lang/leftovers.rs`, `bench/leftovers_cases.ts`). |
| M9-TS-3191 | Integration | A `Partial<Record<Tag, V>>`: assignment of a missing property, `in`, `delete o.k`, `keys`, `values`, `entries` and `o.k === undefined` equal bun's (`leftovers.rs`). |
| M9-TS-3192 | Integration | A `Record<string, V>` dictionary: `delete`, `key in d` with a run-time key, `keys`, `values`, `entries`, `Object.fromEntries`, `for ... in` equal bun's (`leftovers.rs`). |
| M9-TS-3193 | Integration | `["a", "b"] as const` arrays are literal unions: `for ... of` into a union parameter, `map(f)`, `indexOf`, `includes`, `NAMES[0]`, mixed `as const` tuples equal bun's (`leftovers.rs`). |
| M9-TS-3194 | Integration | `xs.flat(depth)` with a depth that is a variable or a constant equals bun's (`leftovers.rs`). |
| M9-TS-3195 | Integration | `function f(...args: [string, number])` and `[number, string?]`: calls with arguments and with a spread tuple equal bun's (`leftovers.rs`). |
| M9-TS-3196 | Integration | A generic function given `["a", "b"] as const` and a `string` (`next(MODES, current)`): `T` is `string`, the array is converted, a union-typed result is a member (`leftovers.rs`). |
| M9-TS-3197 | Unit | The window helpers of the facade (`.on(`, `.emit(`, `broadcast`) are added only for a program that names `Window` (`tools/web_ui_ts/src/lower/facade.rs`). |
| M9-TS-3198 | Integration | `input.gamepads()` gives pads with `buttons` and `axes` by name; `input.on("gamepadconnected", e => ..)` gets `{ gamepad }` (`tools/platypus_ts_tests/tests/engine_api/originals_engine.rs`). |
| M9-TS-3199 | Integration | `Light.measureIlluminance` stays pending until a physics step, then equals `Light.illuminanceAt` without colliders (`originals_engine.rs`). |
| M9-TS-3200 | Integration | A closure given to `Physics.afterNextStep` runs once, after the next step (`originals_engine.rs`). |
| M9-TS-3201 | Integration | The fog keys of a scene file's `renderSettings` apply (with defaults for missing keys) and are saved, as `scenes.ts` has them (`tools/platypus_ts_tests/tests/engine_api/render_native.rs`). |
| M9-TS-3202 | Integration | An importer of a module with a top-level `await` runs after it has finished: `b1,b2,main sees 7`, equal to bun running the files as ES modules (`tools/platypus_ts_tests/tests/lang/tla_modules.rs`, cases in `bench/tla_modules_cases.txt`). |
| M9-TS-3203 | Integration | A sibling that does not import the pending module runs while it waits (`b1,c,b2,main 3`), equal to bun (`tla_modules.rs`). |
| M9-TS-3204 | Integration | Two awaiting siblings overlap (`slow1,fast1,fast2,slow2,main 11`), equal to bun (`tla_modules.rs`). |
| M9-TS-3205 | Integration | A chain leaf, mid, main waits all the way up while an unrelated module runs (`leaf1,other,leaf2,mid 3,main 106`), equal to bun (`tla_modules.rs`). |
| M9-TS-3206 | Integration | Modules without a top-level `await` keep import order and the bundle has no module markers (`tla_modules.rs`). |
| M9-TS-3207 | Integration | The entry awaits after awaiting imports, equal to bun (`tla_modules.rs`). |
| M9-TS-3208 | Unit | Module markers are written only when some module has a top-level `await` (`tla_modules.rs`). |
| M9-TS-3209 | Integration | A rejected top-level `await` fails the startup with the module path and the error text (`tla_modules.rs`). |
| M9-TS-3210 | Integration | While a module waits the startup is pending and names it (first in start order); it is done after the event loop (`tla_modules.rs`). |
| M9-TS-3211 | Integration | `register` before and after an `await` records classes in call order; the startup is pending in between (`tools/platypus_ts_tests/tests/lang/startup.rs`). |
| M9-TS-3212 | Integration | `register` in `if` / `else`, a function called from a loop, and twice for one class: only the calls that run register, a class keeps its first place (`startup.rs`). |
| M9-TS-3213 | Integration | `register({ A, B }, { spawn: false })` and `{ spawn }` with a run-time value are recorded per class (`startup.rs`). |
| M9-TS-3214 | Integration | `register` of an unknown name, of a variable and of an expression are compile errors that say so; a program's own `register` function is left alone (`startup.rs`). |
| M9-TS-3215 | Integration | Each run of the top level starts with no registrations (`startup.rs`). |
| M9-TS-3216 | Integration | `for (const i in xs)` over an array gives the indices as strings (`"0"`, `"1"`), with `continue`, an empty array and `i + 1` string concatenation, equal to bun (`tools/platypus_ts_tests/tests/lang/lang2.rs`, cases in `bench/lang2_cases.ts`). |
| M9-TS-3217 | Integration | `Object.assign` into a `Record<string, V>` and a `{ [k: string]: V }` from objects and dictionaries, and into an object from several sources, equals bun (`lang2.rs`). |
| M9-TS-3218 | Integration | An object literal with `get v()` and `set v(n)`: writes call the setter, `+=` reads the getter and calls the setter, plain properties stay plain, equal to bun (`lang2.rs`). |
| M9-TS-3219 | Integration | A narrowing (`typeof u === "number"`) holds inside closures over a `const` or a never-assigned parameter, including non-inlined callbacks (`reduce`) and function expressions, equal to bun (`lang2.rs`). |
| M9-TS-3220 | Integration | A `let` that is reassigned does not keep a narrowing inside a closure (the closure reads the tag), equal to bun (`lang2.rs`). |
| M9-TS-3221 | Integration | `"radius" in s` narrows a union of object interfaces, object literals pick their member by their keys, equal to bun (`lang2.rs`). |
| M9-TS-3222 | Integration | `p instanceof Cat` narrows and tests a union of classes, also in an arrow function, equal to bun (`lang2.rs`). |
| M9-TS-3223 | Integration | `!v` / `if (v)` on unions (`number \| string \| number[] \| null`, `string \| Pt`, `number \| number[]`) test the member held (0, "" and null are false) and the falsy branch narrows to the members that can be falsy, equal to bun (`lang2.rs`). |
| M9-TS-3224 | Integration | `flat(depth)` with a depth variable that reaches the full flattening (2, 10, `Infinity`) equals bun (`lang2.rs`). |
| M9-TS-3225 | Integration | (retired: `flat(depth)` no longer throws, see `M9-TS-3313..3315`.) |
| M9-TS-3301 | Integration | Every style property of the `web_ui_dom::tables` table is applied by `apply_style` (each keyword of its type, a number where the type takes one, a free string where it takes one) and resetting it is accepted (`crates/web_ui_dom/tests/ui_tables.rs`). |

| M9-TS-3302 | Integration | Every `"camelCase" =>` name `apply_style` matches in `style.rs`, `text_style.rs` and `paint.rs` is in the style table (`ui_tables.rs`). |

| M9-TS-3303 | Integration | Style names are unique, named unions exist, and every cursor keyword applies (`ui_tables.rs`). |

| M9-TS-3304 | Integration | Lengths take every unit of `LENGTH_UNITS` (resolved by the tree) and each shorthand of `SHORTHANDS` applies together with its longhands (`ui_tables.rs`). |

| M9-TS-3305 | Integration | Attributes are handled as their scope says: `Html` by `Attrs::set`, `Svg` by SVG elements only, `Core` and `Compat` rejected by `Attrs::set`; every name `Attrs::set` matches is declared (`ui_tables.rs`). |

| M9-TS-3306 | Integration | Every element of `ELEMENTS` and `SVG_KINDS` can be created; components map to their tags (`ui_tables.rs`). |

| M9-TS-3307 | Integration | Every event field reads the slot (or flag, or string) `UiEvent::script_args` fills (`ui_tables.rs`). |

| M9-TS-3308 | Integration | Every `on*` prop names a real `EventKind`, capture variants resolve, `onGotPointerCapture` is not a capture prop, and every event type without a prop is a transition, animation or drag one (`ui_tables.rs`). |

| M9-TS-3309 | Unit | The generated UI declarations parse as a `.d.ts` and list every style property, attribute, event prop, element, component and hook of the tables, with docs and the formatter's union layout (`tools/web_ui_ts/src/ui_dts.rs`). |

| M9-TS-3310 | Unit | The compiled prelude has a `UiEvent` field per event field with the slot expressions, an alias per event interface, and the module export lists come from the tables (`ui_dts.rs`). |

| M9-TS-3311 | Unit | `onFocus` / `onBlur` / `onBlurCapture` / `onChange` / `onGotPointerCapture` lower to `focusin` / `focusout` / `focusout:capture` / `input` / `gotpointercapture` handlers (`tools/web_ui_ts/src/lower/jsx.rs`). |

| M9-TS-3312 | Unit | Capitalised components stand for the tags of the element table (`jsx.rs`). |
| M9-TS-3313 | Integration | `xs.flat(d)` of a `number[][]` with a run-time depth (0, 1, 5, -3, NaN, 0.9, 1.7, Infinity) gives `(number \| number[])[]` holding exactly that many levels, equal to bun (`tools/platypus_ts_tests/tests/lang/lang3.rs`, cases in `bench/lang3_cases.ts`). |
| M9-TS-3314 | Integration | The same for `number[][][]` and `string[][][]`, narrowing the union elements with `typeof` / `Array.isArray` and reading `.length` of a union of arrays, equal to bun (`lang3.rs`). |
| M9-TS-3315 | Integration | A constant depth keeps its exact result type (`flat()` is `number[][]`, `flat(2)` and `flat(Infinity)` are `number[]`, `flat(0)` the same nesting), equal to bun (`lang3.rs`). |
| M9-TS-3316 | Integration | `xs.flat(depth) as number[]` with a depth that does not reach the numbers throws a `TypeError` naming the flat depth (`lang3.rs`; no bun comparison, bun returns the wrong type). |
| M9-TS-3317 | Integration | Methods, `get` / `set` accessors and compound writes in an object literal use `this` as the literal's object (`this.n++`, `this.n += k`, `this.double = 20`, a getter reading another field), equal to bun (`lang3.rs`). |
| M9-TS-3318 | Integration | `this` in literal methods when the type is declared (an interface returned from a factory), in two objects made by the same code, in a nested literal, in an arrow inside a method and in a loop over `this.list`, equal to bun (`lang3.rs`). |
| M9-TS-3319 | Integration | `Object.assign(obj, dictionary)` copies the entries whose keys are fields of `obj` by name at run time, with several sources and an unknown key skipped (a once-per-site dev warning), equal to bun on the known fields (`lang3.rs`). |
| M9-TS-3320 | Integration | `{ x: 0, y: 0, ...dict }` into an object type copies by key; `{ ...dict, z: 1 }` and `{ ...copy, ...{ q: 2 } }` into a `Record<string, number>` make dictionaries, equal to bun (`lang3.rs`). |
| M9-TS-3321 | Integration | `Object.keys` / `values` / `entries` of an object and `b[k as keyof Box]` over the keys equal bun (`lang3.rs`). |
| M9-TS-3322 | Integration | A dictionary value assigned into a field of another type throws a `TypeError` naming the key and the struct (`lang3.rs`; no bun comparison). |
| M9-API-1621 | Integration | `&[String]` and `Vec<String>` parameters: the shim reads a script array of strings (null is empty), the manifest type is `strings`, the declaration `string[]` (`crates/ts_api/tests/records_enums.rs`). |
| M9-TS-3323 | Integration | A compiled program passes `string[]` (also empty, with non-ASCII text, a mapped array) to `&[String]` / `Vec<String>` parameters of a host function, equal to bun with a JS stand-in (`lang3.rs`, `bench/lang3_prelude.js`). |
| M9-TS-3324 | Integration | `onFocus` / `onBlur` / `onFocusCapture` / `onBlurCapture` in a compiled game driven by a real `Interaction` (`Input::Focus`, `Blur`, Tab): `focusin` / `focusout` bubble from the input to the form, capture runs first, `relatedTarget` names the other element (`ui.rs`, `bench/ui.ts`). |
| M9-TS-3325 | Integration | `onTransitionRun` / `Start` / `End` / `Cancel` in a compiled game: `propertyName` (kebab-case), `elapsedTime` in seconds and an empty `pseudoElement`, from a transition driven by `Input::Refresh` (`ui.rs`). |
| M9-TS-3326 | Integration | `onAnimationStart` / `Iteration` / `End` in a compiled game with `animationName` and the spec's `elapsedTime` (`ui.rs`). |
| M9-TS-3327 | Integration | Drag and drop in a compiled game: one `dataTransfer` from `dragstart` to `dragend` (`setData` / `getData` / `types` / `dropEffect`), `dropped`, an unaccepted drag, and files dragged in from the system as `dataTransfer.files` with their names (`ui.rs`). |
| M9-TS-3328 | Integration | `onMouseEnter` / `onMouseLeave` / `onMouseMove` in a compiled game (non-bubbling events still run the target's handlers); a non-bubbling `scroll` does not reach the parent's `onScroll` (`ui.rs`). |
| M9-TS-3329 | Unit | `EventKind::ALL` lists every event type (an exhaustive `match` stops compiling when one is added) (`crates/web_ui_dom/tests/ui_tables.rs`). |
| ~~M9-TS-3330~~ | Unit | The `on*` props of the tables are exactly those `HANDLERS` of `js/react/src/events.ts` runs per event type, and the non-bubbling types agree (`ui_tables.rs`). (removed 2026-10-28: QuickJS removed) |
| M9-TS-3331 | Unit | The slots of `propertyName` / `animationName` / `elapsedTime` / `relatedTarget` / `dropped` / `dataTransfer` read what `UiEvent::script_args` fills (`ui_tables.rs`). |
| M9-TS-3332 | Integration | Focus events from `Interaction` reach native reconciler handlers (capture, bubble, `relatedTarget` in `data[9]`), and a non-bubbling `scroll` stays on its element (`crates/web_ui_react/tests/reconcile.rs`). |
| M9-TS-3333 | Integration | `cols` sizes a textarea (8 px per character, CSS wins); `title` and `type` are kept (type lower-cased) and match `[title=..]` / `input[type=password]` selectors, also after a change (`crates/web_ui_dom/tests/editor.rs`). |
| M9-TS-3334 | Unit | The compiled `UiDataTransfer` has every member of the declared `PlatypusDataTransfer`, and the alias and the declaration exist (`tools/web_ui_ts/src/ui_dts.rs`). |
| M9-API-1622 | Unit | An `enums(extern X = ..)` enum adds no second set of symbols or manifest entry (the declaring namespace lists it once, the other names `enum:X`); an explicit variant name wins; `aliases(..)` and the helpers exist (`crates/ts_api/tests/enums_nullables.rs`). |
| M9-API-1623 | Integration | `Option<f64>` / `Option<V>` / `Option<String>` cross the C ABI with NaN, NaN fields and the `ts_api_result_none()` flag, both ways; the manifest says `opt:f64`, `opt:value:V`, `opt:string` (`enums_nullables.rs`). |
| M9-API-1624 | Unit | An `Option<Enum>` parameter is none for -1 or an unknown index (a setter changes nothing); a `Vec<Enum>` result is the count and the indexes in the result buffer; the explicit-name converters (`enums_nullables.rs`). |
| ~~M9-API-1625~~ | Integration | QuickJS: an enum takes and gives names, an unknown name is a `TypeError`, `Option` results are `null`, an object or `null` sets an optional value, `Enum.name(i)` / `.parse(s)` exist, an alias is the same global (`enums_nullables.rs`). (removed 2026-10-28: QuickJS removed) |
| M9-API-1626 | Unit | `#[nullable(undefined)]` is `"nullable":true,"undefined":true` in the manifest JSON, on a function and on a class property (`enums_nullables.rs`). |
| M9-API-1627 | Integration | The engine enums from QuickJS (`FogMode`, `VolumeDetail`, `FilterMode`, `TextureWrapMode`, `PrimitiveType`, `PrimitiveMesh`, `FullScreenMode`, `ShadowQualityMode`, `WindowStyle`, `LoadSceneMode`, `AnimatorController`): names in and out, `Enum.name` / `parse`, unknown names throw a `TypeError` (`crates/platypus_native/tests/enums.rs`). |
| M9-API-1628 | Integration | The engine nullables from QuickJS are `null` when none and take `null`: `fixedExposure`, `Camera.main`, `Window.focused` / `find` / `at` / `byHandle` / `parent`, `openError`, `LineRenderer2D.fillColor` (`enums.rs`). |
| M9-TS-3340 | Integration | Compiled code with every GC mode: enum constants and dynamic strings, an unknown name changes nothing, `Texture.create(.., { filterMode, wrapMode })` defaults, `Record<FullScreenMode, number>` indexed by `Screen.supportedFullScreenModes`, `PrimitiveType.Cube`, `PrimitiveMesh`, `Camera.main === undefined`, `fixedExposure` and `fillColor` set and `null`, `AnimatorController` (`tools/platypus_ts_tests/tests/engine_api/facade.rs`, `bench/facade.ts`). |
| M9-API-1629 | Integration | QuickJS gets `Screen.windowSize`, `currentResolution`, `resolutions` and `Animator.getCurrentAnimatorStateInfo()` (a record with a string field) as plain objects and arrays of them (`crates/platypus_native/tests/enums.rs`). |
| M9-API-1630 | Integration | A record with a `string` field: the numbers go to the result buffer and the strings to the string list, in field order, element after element for a list; manifest field type `string` (`crates/ts_api/tests/enums_nullables.rs`). |
| M9-API-1631 | Integration | `#[options(name = "PokeOptions", only)]`: the manifest has `optionsName`, `optionsOnly` and the parameter docs from the `/// key: text` lines (they leave the function's doc) (`enums_nullables.rs`). |
| M9-TS-3341 | Integration | Compiled code with every GC mode: `Screen.windowSize`, `currentResolution`, `resolutions` and `Animator.getCurrentAnimatorStateInfo()` come back as objects (records with number and string fields, lists of records) (`tools/platypus_ts_tests/tests/engine_api/facade.rs`). |
| M9-TS-3342 | Unit | A named options object is an `interface` with the parameter docs (a handle key also takes `null`), `options?: Name` is the signature, `only` drops the positional form, array parameters are `readonly`, the module re-exports the name (`tools/web_ui_ts/src/types_dts.rs`). |
| M9-API-1632 | Integration | An `Option<Record>` parameter and result travel as JSON text: optional keys, `null` for a nested option, a value class as an array or an object, an enum by name, `pairs`, a script key (`as "label"`); a missing required key, a wrong type or an unknown enum name is reported and is none; the manifest says `opt:record:R` and `opt:opt:like:V` / `opt:list:pair` / `opt:string` for the fields (`crates/ts_api/tests/enums_nullables.rs`). |
| M9-API-1633 | Integration | `RenderSettings.heightFog` / `skybox` / `bloom` / `autoExposure` / `volumetricFog` from QuickJS take an object or `null` and read back as objects (colour instances, `null` for no sun, the curve as pairs, the defaults of keys left out); a bad object is ignored (`crates/platypus_native/tests/enums.rs`). |
| M9-TS-3343 | Integration | Compiled code with every GC mode: the same properties from literals, a variable of the game's own interface and `cond ? { .. } : null`, a key written as `null` differs from one left out (`sunDirection`), the values read back (`tools/platypus_ts_tests/tests/engine_api/facade.rs`). |
| M9-TS-3344 | Integration | `JSON.parse(f())` of a text a call returns survives collections: the operand is held in a local while the parser allocates (every GC mode) (`tools/platypus_ts_tests/tests/stdlib/json.rs`, `bench/json.ts`). |
| M9-API-1634 | Integration | Lists of handles, numbers and values from QuickJS: `SkinnedMeshRenderer.bones`, `Cloth.sphereColliders` / `capsuleColliders` / `pins`, `Camera.layerCullDistances` are properties whose setter replaces the list and whose getter is a copy; `Cloth.pinBox` takes a record or `null`; a wrong class or a non-array is a `TypeError` (`crates/platypus_native/tests/lists.rs`). |
| M9-API-1635 | Integration | `Vec<Handle>` results from QuickJS: `Scene.getRootGameObjects()`, `Sprite.grid(.., pivot?, pixelsPerUnit?)` (a left-out pivot or pixels-per-unit keep the sprite's own) (`crates/platypus_native/tests/lists.rs`). |
| M9-TS-3350 | Integration | Compiled code with every GC mode: the same list properties (`bones`, cloth colliders / pins / `pinBox`, `layerCullDistances`, `getRootGameObjects()`, `QualitySettings.names`, a `Vector2Like[]` setter fed a `Vector3`); a setter replaces (`tools/platypus_ts_tests/tests/engine_api/facade.rs`, `bench/facade.ts`). |
| M9-API-1636 | Integration | `#[subscription(off = ..)]` functions from QuickJS (`Assets.onChanged`, `Application.onFocusChanged`, `Screen.onChange`, `QualitySettings.onLevelChanged`, `SceneManager.onSceneLoaded` / `onSceneUnloaded` / `onActiveSceneChanged`) return a function; calling it (twice) stops the listener (`crates/platypus_native/tests/subscriptions.rs`). |
| M9-TS-3351 | Integration | Compiled code with every GC mode: the same listeners return an unsubscribe function written by the compiler over the token-returning native (`tools/platypus_ts_tests/tests/engine_api/facade.rs`, `bench/facade.ts`). |
| M9-API-1637 | Integration | `#[promise]` functions from QuickJS: `SceneManager.loadSceneAsync` and `Prefab.load` return a real `Promise` that the engine settles in the tick the host loads the file (`finish_load` + `dispatch`), and rejects with an `Error` for a file that cannot be read or parsed (`crates/platypus_native/tests/subscriptions.rs`; the compiled side is `M9-API-1512`, `1513`, `1517` in `crates/platypus_app/src/native.rs`). |
| M9-API-1638 | Integration | `Texture.whenLoaded()` from QuickJS resolves when the host reports the load (`texture_loaded`), at once for a loaded texture, and rejects with an `Error` when loading failed (`crates/platypus_native/tests/subscriptions.rs`). |
| M9-API-1639 | Integration | Lists of records and records that hold handles from QuickJS: `LODGroup.setLODs(levels)` (`renderers: MeshRenderer[]`), `ParticleSystem.bursts` (optional `cycles` / `interval`), `colorOverLifetime`, `sizeOverLifetime`, `shape` (keys left out keep their values); a non-array or a wrong handle is a `TypeError` (`crates/platypus_native/tests/lists.rs`). |
| M9-API-1640 | Integration | Records hold lists of numbers (`Vec<f64>`, `list:f64`) and of records (`Vec<R>`, `records:R`, optional too) (`crates/ts_api/tests/record_lists.rs`). |
| M9-API-1641 | Integration | `#[options(.., object)]`: the manifest says `optionsObject` (and `#[global]`: `global`). |
| M9-API-1642 | Unit | A class function declared twice under one script name is in the manifest twice, in declaration order (the compiler picks by the argument types). |
| M9-API-1643 | Integration | `#[setter(prop)]` is another version of a property setter (the manifest has both) and `#[constructor]` marks the positional constructor (`crates/ts_api/tests/record_lists.rs`). |
| M9-API-1644 | Integration | Gamepads from QuickJS: `input.gamepads()` is a list of records whose `buttons` / `axes` are dictionaries in order (standard names, 0 when not reported); `input.on(type, f)` (`#[subscription(.., json = ..)]`) passes the event as JSON text and returns a stop function (`crates/platypus_native/tests/services.rs`). |
| M9-TS-3352 | Integration | Compiled code with every GC mode: the same lists of records through JSON text with handles as ids (`bench/facade.ts` `recordLists`, `tools/platypus_ts_tests/tests/engine_api/facade.rs`). |
| M9-TS-3353 | Integration | Compiled code with every GC mode: `Camera.screenToWorldPoint`, `horizontalFieldOfView` (read and write) and `rect` are native (an 800x600 window, a 90 degree camera) (`tools/platypus_ts_tests/tests/engine_api/facade.rs`, `bench/facade.ts` `cameraMaths`). |
| M9-TS-3354 | Integration | Compiled code with every GC mode: `Mesh.create` takes one object (any value with the keys; `#[options(.., object)]`): bone influences become four indices and weights per vertex, bind poses 16 numbers each (`tools/platypus_ts_tests/tests/engine_api/facade.rs`, `bench/facade.ts` `skinnedMesh`). |
| M9-TS-3355 | Integration | Compiled code with every GC mode: `AnimatorController` builds with descriptions (`addParameter(name, type)`, `addState(name, clip | blendSpec, opts)`, `addTransition(spec)`; records with nested record lists) next to the handle forms of the same names (a class function declared twice: the version is picked by the argument types) (`bench/facade.ts` `animatorController`, `tools/platypus_ts_tests/tests/engine_api/facade.rs`). |
| M9-TS-3356 | Integration | Compiled code with every GC mode: `addStyleSheet`, `removeStyleSheet` and `loadFont` are globals of the namespace `UiResources` (`#[global]`); `loadFont` takes one `FontFace` object, a literal or a variable, with the weight and style left out (`bench/facade.ts` `uiResources`, `tools/platypus_ts_tests/tests/engine_api/facade.rs`). |
| M9-TS-3357 | Integration | Compiled code with every GC mode: `Scene.serialize()` is a record with a list of records (JSON text) and `toJson()` the whole scene as text (`bench/facade.ts` `sceneSummary`). |
| M9-TS-3358 | Integration | Compiled code with every GC mode: `new Material(color, emission, ..)` is a `#[constructor]`, `Sprite.create(texture, rect | null, pivot, ppu)` a second version of the static, `startSize = [2, 5]` / `startColor = [a, b]` / the options form a second version of the setter (`bench/facade.ts` `constructors`, `sprites`, `particlesAndPoints`). |
| M9-TS-3359 | Integration | Compiled code with every GC mode: `Random.pick(items)` is a compiler built-in (declared in `tools/web_ui_ts/src/builtins.rs`, lowered to `items[Random.rangeInt(0, length)]`) over lists of strings, booleans and objects; an empty list gives `undefined` (`bench/facade.ts` `picksOfEveryKind`). |
| M9-API-1645 | Integration | Compiled code with every GC mode: the light meter is native (`crates/platypus_render_api/src/light_meter.rs`): `Light.illuminanceAt(point, normal?)` faces the normal (nothing facing away) and `Light.measureIlluminance` (`#[promise]`, resolved after the next physics step) counts a collider between the point and the light as a shadow (`bench/originals_engine.ts` `startShadowed`, `tools/platypus_ts_tests/tests/engine_api/originals_engine.rs`). |
| M9-API-1646 | Unit | The engine writes the named-event envelope (`window::event_text`, `emitJson`, `Window.emitAllJson`, empty names refused) and settles the promises of `requestJson` / `askJson` itself (reply, `no answer for`, refusal at once, timeout; `onReplyJson` listeners never see them); `Window.eventNameError` names what the typed calls throw (`crates/platypus_engine_api/tests/window_messages.rs`). |
| M9-TS-3360 | Unit | The compiler built-ins table (`tools/web_ui_ts/src/builtins.rs`): the `Component` declaration has every hook the host calls, with the parameter its thunk passes (`components::hook_param`), and names the contact hooks; every top-level entry declares the name it is listed under. |
| M9-TS-3361 | Integration | Compiled code with every GC mode: `Application.args` is the host's arguments after `--` (a copy each read) and `Application.quit(code)` asks to quit with a whole exit code (0 by default, `2.7` is 2); declared `args: string[]` and `quit(exitCode?: number)` (`tests/engine_api/app.rs`). |
| M9-TS-3362 | Integration | `scrollIntoView`, `scrollTo` and `scrollBy` on a ref in compiled code, positional and with the DOM's option objects, give the host's node commands with the DOM's defaults (`block: "start"`, `inline: "nearest"`, `behavior: "auto"`; `scrollTo` keeps a left-out axis) (`tests/ui/ui.rs`, `bench/ui.ts` `ScrollCalls`). |
| M9-TS-3363 | Unit | The spans of a program that are not code (string literals, template text, JSX text, comments) are found by parsing; text-level passes (window message glue, `Temporal.` names, `WeakRef` classes, native aliases) leave them alone (`tools/web_ui_ts/src/lower/literals.rs`). |
| M9-TS-3364 | Integration | `input.on` in compiled code hears every kind `js/engine`'s `InputEvents` has: `keydown` / `keyup` with `code`, `key` and `repeat`, `gamepadbuttondown` with `button` and `gamepad`, `gamepadaxis` with `axis` and `value`; the returned function stops a listener (`tests/engine_api/app.rs`). |
| M9-TS-3365..3379 | Integration | The TypeScript of the md-reader and launcher examples, compiled with every GC mode and compared with bun (`bench/readers_cases.ts`, `tests/lang/readers.rs`): an intersection with a recursive union alias, a tag test narrowing a field whose type differs per variant (`if`, `switch` with grouped cases), `filter(Boolean)` / `Boolean(x)`, function declarations hoisted in a body, `x \|\| null` and `s ?? 1` as a `string \| number` (`Number`, templates), closures inferring `null`-then-value results and default-parameter types, `-1 \| 1`, `new Set(set)` / `new Map(map)`, `find` over a tuple of literals, `re.exec(s)?.[0]` and `RegExpExecArray`, strings mentioning `Temporal.` / `class` / `.request<..>(` keeping their text, literal unions from and to strings that may be missing, `id<F>(f: F)` inferring a closure type, a copy where `T \| null` goes keeping null, `clearTimeout(undefined)`. |
| M9-TS-3380 | Integration | A callback ref (`ref={(el) => ..}`) in compiled code is called with the element, with null and the element when a re-render brings a new function, and with null when the element goes (`tests/ui/ui.rs`, `bench/ui.ts` `CallbackRef`). |
| M9-TS-3381 | Integration | `<Fragment key>` siblings in a list, components returning text, nothing or a list (`ReactNode`, `JSX.ElementType`), a component that renders itself with no written return type, and `memo(function Name(..) {..})` (`tests/ui/ui.rs`, `bench/ui.ts` `nodesApp`). |
| M9-TS-3382 | Integration | `useSyncExternalStore` re-renders when its store says it changed, and `useState` holds a string-literal union (`tests/ui/ui.rs`, `bench/ui.ts` `Themed`). |
| M9-TS-3383 | Integration | `project.json`'s `prebuild` command runs in the game folder before compiling (a generated module the entry imports), `--no-prebuild` skips it, and a failing command stops the build with its text (`tools/platypus_build/tests/cli.rs`). |
| M9-TS-3384 | Integration | A value-class key of an options object takes a plain object (`Material.create({ mainTextureScale: { x: 4, y: 2 } })`: declared `Vector2Like`), and a controller's `addParameter(name, "bool", true)` takes a flag default (`bench/facade.ts` `whiteRoomShapes`). |
| M9-TS-3385 | Integration | `find` and `reduce` over arrays of engine handles (a miss is `undefined`), `getComponent(Animation)?.destroy()` (then `undefined`), and `Model.materials` / `Model.animations` lists, empty before the model loads (`bench/facade.ts` `whiteRoomShapes`). |
| M9-TS-3386 | Integration | `reduce` / `reduceRight` with no initial value start from the first (last) element, over numbers, strings and objects; an empty array throws a `TypeError` (`bench/leftovers_cases.ts`, compared with bun). |
| M9-TS-3387 | Integration | Compiled code with every GC mode: `MeshRenderer.isVisible` and `SkinnedMeshRenderer.isVisible` are false until the host reports a frame, asking starts the watch (`watched_renderers`), the report (`set_seen_renderers`) answers each apart; `RenderStats.shadowCasters` reads the host's count (`bench/render_native.ts`). |
| M9-TS-3389 | Integration | The JIT line table (`Game::line_table`) is ascending and covers the source lines of a compiled function, from `Stmt::Line` markers and Cranelift source locations (`lang/debug_lines.rs`). |
| M9-TS-3413 | Unit | `platypus-build build` writes `build/game.pdebug` (format, hash of the bundled source, where each file starts) and the host reads it: bundle lines map to `file:line` (`crash.rs`). |
| M9-TS-3390 | Integration | On Windows the JIT registers x64 unwind tables for compiled functions (`web_ui_ts::unwind`), so the system can walk a stack through them: `is_registered` (`lang/debug_lines.rs`). The call stack a crash report prints from it (`crash.rs` `game_stack`) is not tested automatically. |
| M9-TS-3391 | Integration | A scan of the stack from inside an engine call (`unwind::stack_words`) finds the return addresses of the compiled game frames in call order (`lang/game_stack.rs`, Windows x64). Checked by hand end to end too: a null dereference in a game gives `src/main.ts` lines 7, 11, 15 and 20 in the crash report. |
| M9-TS-3392 | Integration | A compiled function with a frame of tens of kilobytes (a 6000-row top-level table) probes the stack pages it passes (`enable_probestack`, inline), so it runs on a fresh thread stack; without probes it jumped Windows' guard page and crashed Zombie Horders at start-up about one run in five (`lang/big_frame.rs`). |
| M9-TS-3393 | Integration | A debug build stops at a breakpoint's line, reports it, and runs to the end after `continue` (`lang/debugger.rs`). |
| M9-TS-3394 | Integration | Step over runs a call through, step in enters it, step out returns to the caller's next statement (stack-pointer comparison). |
| M9-TS-3395 | Integration | `pause` stops a running loop at one of its lines, and `detach` lets it finish. |
| M9-TS-3396 | Integration | A debug build nobody is debugging runs to its result without stopping. |
| M9-TS-3397 | Unit | The host's debugger answers malformed or out-of-state commands with an `error` line; `DebugInfo::bundle_line` maps a file and line to the bundle (`debugger.rs`, `crash.rs`). Checked live too: break, over, in, out, continue, detach over a socket, and breakpoints after a hot reload. |
| M9-TS-3398 | Unit | A DAP session over in-memory streams: `initialize` (and the `initialized` event), `attach`, `setBreakpoints` for an unknown file (answered, not verified), `configurationDone`, `threads`, `stackTrace` with nothing stopped, an unsupported request (`success: false`), `disconnect` ends the session (`dap.rs`). |
| M9-TS-3399 | Integration | A DAP client against the real host (windowless bench run): the game waits for `configurationDone`, stops at the breakpoint, `stackTrace` gives its line, `next` and `stepIn` step, `disconnect` lets the game finish and exit cleanly (`tests/dap_session.rs`, Windows). |
| M9-TS-3400 | Integration | A stop in a debug build gives the function's named locals with type and value (number, string, boolean, a captured variable read through its cell) (`lang/debugger.rs`). |
| M9-TS-3401 | Integration | A stop lists the callers found on the stack: the stop's function and line, then its caller's (Windows). |
| M9-TS-3402 | Integration | Watch expressions at a stop read locals, fields (`p.x`), elements (`arr[1]`), `.length` and do arithmetic, comparison and string joins; errors name the problem (`lang/debugger.rs`). |
| M9-TS-3403 | Integration | Objects and arrays open into their fields and elements (`debug::variables`); a number has nothing inside. |
| M9-TS-3404 | Integration | A conditional breakpoint stops only when its condition is true (`i == 3` in a loop). |
| M9-TS-3405 | Integration | Hit conditions: a bare number is that hit only, `% 2` every second; the game runs to the end after the last stop. |
| M9-TS-3406 | Integration | A log point writes its message with `{expression}` values and does not stop the game. |
| M9-TS-3407 | Integration | A condition or hit condition that does not parse is reported and that breakpoint is not set. |
| M9-TS-3408 | Unit | Expression precedence, shadowing (the later declaration of a name wins), string and null rules (`debug_eval.rs`). |
| M9-TS-3409 | Unit | Bad expressions say what is wrong (ends too soon, unbalanced, unknown name, no field, no calls). |
| M9-TS-3410 | Unit | Hit conditions (`3`, `= 3`, `>= 3`, `> 3`, `< 3`, `% 2`) and log message interpolation. |
| M9-TS-3411 | Unit | Objects and arrays are read from memory by their layout: fields, `.length`, elements, out of range, children listings. |
| M9-TS-3412 | Integration | Break on throw: with it off a caught `throw` does not stop; on, the game stops at the `throw` statement (reason exception) with its locals readable, and runs on after `continue` (`lang/debugger.rs`). |

## M9 — One host: the renderer chosen from what the game uses

| ID | Kind | What |
|---|---|---|
| M9-HOST-01 | Unit | The tier of a set of engine names is the highest among them (`ui` < `world2d` < `world3d`); only names above `ui` are listed in `uses`, sorted and once (`platypus_core::renderer`). |
| M9-HOST-02 | Unit | A scene or prefab file names its component and asset types (`"type"` and `"$type"` at any depth). |
| M9-HOST-03 | Unit | Each tier parses back from its name (`ui`, `world2d`, `world3d`); anything else is none. |
| M9-HOST-04 | Unit | Every name of the tier table is a class or namespace of the real engine manifests (no stale tags). |
| M9-HOST-05 | Unit | Every class and namespace of `platypus_render_api` has a tier above `ui`, and so do `Camera` and `Gizmos`. |
| M9-HOST-06 | Unit | A compiled program that is only UI (React state, `Debug.log`, `Time`, `Vector3`) needs `ui` with nothing listed. |
| M9-HOST-07 | Unit | A program that adds a `Camera` needs `world2d` (`uses: ["Camera", "GameObject"]`); with a `MeshRenderer` too it needs `world3d`. |
| M9-HOST-08 | Unit | A program that adds a `TextMesh` needs `world2d`. |
| M9-HOST-09 | Unit | A `Camera` used only in a top-level function nothing calls does not count (the unused function is not compiled): `ui`. |
| M9-HOST-10 | Unit | With no renderer set, the host starts a `ui` program on GLES and a `world2d` / `world3d` one on wgpu with the world (`host::resolve`). |
| M9-HOST-11 | Unit | The command line's `--renderer` beats the project's `renderer`, which beats the tier (`vulkan`, `gles`, `cpu` each way). |
| M9-HOST-12 | Unit | A UI-only renderer (`gles`, `cpu`) for a program that uses the world warns naming the renderer, the names it uses and "3D Not Supported"; `vulkan`, or a `ui` program, does not. |
| M9-HOST-13 | Unit | Engine calls named only inside strings (the md-reader's bundled docs: `Model.load(..).whenLoaded()`, `Light.measureIlluminance`) pull no facade chunk in and do not count: `ui`. |
| M9-HOST-14 | Unit | `platypus-build package --renderer` writes `renderer` into the exe's embedded `project.json` over the project's (`auto` leaves it out, no argument keeps the project's), warns once for a UI-only renderer of a world bundle, and writes `THIRD-PARTY-NOTICES.md` beside the exe. |
| M9-HOST-15 | Unit | A scene or prefab file brings `GameObject` with its component types; a script component type (`Spinner`) has no tier of its own, `GameObject` makes it `world2d`. |
| M9-HOST-16 | Unit | A program with only a registered component (it logs in `onUpdate`, touches no object) needs `world2d` with `Component` in `uses`; so does a class that extends `Component` with no `register`. |
| M9-HOST-17 | Unit | A program that only creates a `GameObject` (and names it) needs `world2d` (`uses: ["GameObject"]`). |
| M9-HOST-18 | Unit | `extends Component`, `register({ .. })` and `new GameObject(..)` inside strings and comments do not count, nor does a plain object's `.children` (the scene hierarchy helper is not lowered): `ui`. |
| M9-TSC-01 | Unit | The default `tsconfig.json` is self-contained (no `extends`): ES2022, strict, `noUncheckedIndexedAccess`, `noImplicitOverride`, `isolatedModules`, `verbatimModuleSyntax`, `moduleResolution: bundler`, `jsx: react-jsx`, `paths` to `src/modules`, `include: ["src"]` and `files: ["types/platypus.d.ts"]`. |
| M9-TSC-02 | Unit | `init` writes that `tsconfig.json` (keeping an existing one) and `types/platypus.d.ts`, the text `platypus-build types` writes (it declares `Vector3` and `GameObject`), so an editor completes engine names at once. |
| M9-TSC-03 | Unit | `init --editor-files` rewrites only `tsconfig.json` and `types/platypus.d.ts` (a stale declarations file is replaced) of one game or of every game in a folder; the other files stay byte-identical; a folder without games is an error. |
| M3-INS-01 | Unit | `UiTree::inspect` reports a node's kind, classes, parent and border box; an unknown node is `None`. |
| M3-INS-02 | Unit | `inspect` lists the matching rules with selector text and marks which declarations win the cascade (specificity, order, inline, `!important`). |
| M3-INS-03 | Unit | `inspect_set_style` changes a node live and an empty value removes the inline declaration again. |
| M3-INS-04 | Unit | `Bundle::parse` reads `uiInspector` (false when absent): the host's fast path without `--add-ui-inspector`. |
| M3-INS-05 | Unit | `UiInspector` getters read the published snapshot (boxes, classes, children, inline styles, rules and declarations); an unknown or negative id answers 0 / `""` / `false`. |
| M3-INS-06 | Unit | The host learns what to publish (hovered, selected, expanded) and takes the queued style edits; with the inspector off the edits are dropped. |
| M3-INS-07 | Integration | Compiled TypeScript calls `UiInspector`: off, it answers nothing and drops the edit; on, it reads the published snapshot (kind, child, rule, declaration, inline) and queues `select` / `expand` / `setStyle`, in every GC mode. |
| M3-INS-08 | Integration | `platypus-build build --add-ui-inspector` appends the overlay to the program (it compiles) and writes `uiInspector` into the bundle; without the flag the bundle has neither. |
| M3-INS-09 | Unit | Pick mode: a press on the screen UI is swallowed with its release and its node comes back from `takeClick` once; not picking, or the inspector off, lets presses through. |
| M3-INS-10 | Integration | The editor built with `--add-ui-inspector` shows a "UI" tab with the editor's own UI tree (its `root` row) and takes F9 without a fault (windowless `--bench` run). |
| M3-INS-11 | Unit | The inspector value logic (`tools/platypus_build/inspector/values.ts`): numbers with units found (not inside colours or names) and stepped at the caret, colours read and written as hex / rgb / hsl and round-tripped through hsv, easing keywords and `cubic-bezier()` found in a transition and written back (as a keyword when it is one), gradients split into head and stops and formatted, autocomplete ordering. Run natively in every GC mode (`editor/test/ui-values.test.ts`). |
| M3-INS-12 | Unit | `inspect` gives the computed form of values with `calc()` (percentages against the parent's box), `var()` and relative units, and none for plain values. |
| M3-INS-13 | Unit | `inspect` reports the layout a flexbox preview draws: display, direction, wrap, justify, align, gaps. |
| M3-INS-14 | Unit | `inspect_set_class` adds and removes a class (once), and `inspect_class_names` lists the classes the stylesheets mention. |
| M3-INS-15 | Unit | `inspect_pseudo` lists the rules matching `::before`, `::after` and the other pseudo-elements, apart from the element's own rules. |
| M3-INS-16 | Unit | Forced states (`hover`, `active`, `focus`, `focus-visible`), class edits and the pseudo view reach the host; off, nothing is queued; `probe` keeps up to 256 nodes. |
| M3-INS-17 | Unit | The style table (properties, keywords, kinds, docs, units) and the stylesheets' class names answer autocomplete. |
| M3-INS-18 | Unit | `inspect_brief` has the boxes and names but none of the styles. |
| M3-INS-19 | Unit | `inspect` layout has margins and paddings in px (percentages of the parent's content width) for a box-model overlay. |
| M3-INS-20 | Unit | `inspect_rule_matches` names the elements a rule styles (a pseudo-element rule: its owners); unknown sheet or rule: none. |
| M3-INS-21 | Unit | Highlights (node or rule) and the box focus reach the host and the highlighted boxes come back. |
| M3-INS-22 | Unit | Text copied from the inspector (`UiInspector.copy`) waits for the host; dropped when the inspector is off. |

## Readiness port (text effects, 2D physics, colour space)

| ID | Kind | Checks |
|---|---|---|
| RDY-EVT-01 | Unit | `EventList` (no type arguments): listeners run in order; `addListener` returns a token and `removeListener(token)` removes that one; a throwing listener doesn't stop the rest; a listener may remove itself. |
| RDY-PHY-01 | Unit + integration | `Physics2D.ignoreCollision`: two ignored colliders pass through each other (rapier pair filter in `platypus_physics`; `set_ignored` in `platypus_physics_api`), also after one collider is rebuilt; `false` lifts it and the contact returns. |
| RDY-PHY-02 | Unit | `HingeJoint2D` (kind `hinge2d`) hinges about +z and connects `Rigidbody2D`s; a hung body swings when pushed. |
| RDY-PHY-04 | Unit | `Rigidbody2D.addForce(..., Impulse)` changes `velocity` at once (native reads rapier live), not after the next step. |
| RDY-PHY-05 | Unit + integration | A body without colliders gets rotational inertia from the box its object's scale stands for (`set_mass(id, mass, size)`), so a hinged chain pushed before the first step swings (also with limits of -45..225 degrees). |
| RDY-RND-01 | Unit | `colorSpace: "srgb"` in `project.json`: world colours (sprites, materials, lights, text meshes, lines, particles, the camera background) are converted from sRGB to linear on the way to the renderer; alpha is never converted. |
| RDY-RND-02 | Unit | `bleed_transparent`: fully transparent pixels take the average colour of their nearest visible neighbours (within the radius), alpha never changes, bad sizes are ignored; textures are bled on load and when cooked. |
| RDY-RND-03 | Headless render | `WorldQuad.flat` quads (2D sprites) write no depth, so sprites at one depth aren't cut by the rounding error in a tilt. |
| RDY-TEX-01 | Unit | `SpriteRenderer.drawMode`: `tiled` repeats the tile across `size` and cuts it off at the edge; `sliced` keeps the corners (from `Sprite.border`) and stretches the rest (`sprite_quads`). |
| RDY-TEX-02 | Integration | `Texture.load` returns the same texture for the same file and settings (a scene loaded again shares it). |
| RDY-INP-01 | Integration | `input.getAxis("Horizontal" / "Vertical")` eases keys like Unity's default axes (3 per second); `getAxisRaw` snaps; sticks aren't eased. |
| RDY-TXT-01 | Unit | `textEffect` names combine (`wavy rainbow`), `none` clears, an unknown name is refused, and only the always-on ones are ambient. |
| RDY-TXT-02 | Unit | Glyphs not yet reached by `textReveal` are invisible, a fresh one is part way through its entrance, and an old one has no effect left (a blink may be out for a moment). |
| RDY-TXT-03 | Unit | Always-on effects follow the clock and the glyph's index (wavy offsets differ over time and along the line; rainbow overrides the hue). |
| RDY-TXT-04 | Headless render | `textReveal: 4` draws only the first four glyphs, at the positions they have in the full text; `0` draws nothing. |
| RDY-TXT-05 | Unit | `UiTree::is_animating` is true only while text with an always-on effect is drawn (not for entrances like `drop`, nor plain text). |
| RDY-TXT-06 | Unit | Entrances keep their shapes: fade only fades; zoom-in grows from nothing, zoom-out shrinks from double; drop overshoots; pixar bounce arrives from the right and hops; blink is fully on or off, random per glyph while arriving, then lit with rare dips. |
| RDY-TXT-07 | Headless render | Glyphs age by real time (`UiRenderer::set_effect_time`) from when they were first drawn shown: settled glyphs match plain text when a fourth appears (still arriving), and after a pause with the reveal unchanged it has finished arriving. |

## web-ui facade (`web_ui`, `web_ui_c`, in the web-ui repo)

| ID | Kind | Checks |
|---|---|---|
| WUI-APP-01 | Unit | `web_ui::Program`: `createRoot().render(..)` mounts the screen's root, the draw list is not empty, and pointer clicks over a box run its `onClick` (twice: `clicks 2`); a click outside runs nothing. |
| WUI-APP-02 | Unit | Timers run from the frame clock: a `setTimeout(.., 100)` set in an effect has not fired at 50 ms and has at 150 ms. |
| WUI-APP-03 | Unit | winit pointer positions become logical pixels (divided by the scale factor), a wheel line is 40 pixels with the DOM's sign, and buttons get the DOM's numbers (right = 2). |
| WUI-APP-04 | Unit | Keys get DOM `key` / `code` names (`a` / `KeyA`, `Enter`, `" "` / `Space`); control characters type no text; repeats and releases carry over. |
| WUI-APP-05 | Unit | A plugin given as manifest JSON plus a registered (name, address) symbol is callable from the program (`Calc.twice(21)` shows 42). |
| WUI-APP-06 | Unit | A program that does not compile is an `Err` naming the unknown name, not a panic. |
| WUI-APP-07 | Unit | An entry file follows its relative imports and loads the stylesheet it imports (`.big` sizes the box to 123 x 45). |
| WUI-C-01 | Unit | `web_ui_register` refuses NULL, non-JSON, a manifest without a namespace and a count without symbols (-1); a good plugin returns 0, its symbols are found by name and its manifest is kept for `web_ui_start`. |
| WUI-C-02 | Unit | `web_ui_start` with NULL options, or options with neither an entry nor a source, returns exit code 1 without opening a window. |
| WUI-EX-01 | Manual | `cargo run -p hello-rust -- --screenshot out.png` and `cargo run -p hello-c -- --screenshot out.png` open a window, draw the app (the Rust plugin's greeting; the C plugin's counter) and write the picture. |
| WUI-APP-08 | Unit | `Source::Bundle`: a bundle built by `web_ui_build::build` starts with its sources deleted; its embedded CSS sizes the box (77 x 11) and assets resolve from the folder above `build/`. |
| WUI-BLD-01 | Unit | `AppBundle::parse` reads `source`, `cssText`, `assets` and `title`, ignores keys it doesn't know (`renderer`), and refuses format 2, a missing `source` and non-JSON. |
| WUI-BLD-02 | Unit | `AppBundle::stylesheet`: `cssText` wins over the loose file (and counts as `<bundle>.css`); without it the loose `css` file, else `<bundle>.css`, is read; neither: none. |
| WUI-BLD-03 | Unit | `web_ui_build::build` follows the entry's imports, embeds its imported CSS as `cssText` (`@import`s first), and writes no `cssText` for a program without stylesheets. |
| WUI-BLD-04 | Unit | `platypus-build build` writes the bundled CSS into `game.pbundle` as `cssText` as well as `build/game.css`; `platypus-app` applies `cssText` and falls back to `game.css` for older bundles. |
