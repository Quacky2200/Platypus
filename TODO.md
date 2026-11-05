# Platypus — TODO

Work plan by milestone. Each task lists the tests from [TESTS.md](TESTS.md) that prove it's done. Architecture lives in [DESIGN.md](DESIGN.md).

Legend: `[ ]` to do · `[~]` in progress · `[x]` done

---

## Decisions needed before / during M1

- [x] Window/input: `winit` + `gilrs`
- [x] Audio: `kira`
- [x] Layout: Taffy first, Yoga fallback behind `LayoutEngine`
- [x] **First script backend:** QuickJS (`rquickjs` 0.14). `deno_core` follows as a second backend. (QuickJS was removed 2026-10-28; compiled TypeScript, `platypus_ts`, is the only backend and `deno_core` was never added.)
- [x] Rust edition 2024, latest stable (1.99 at time of writing)
- [x] Licence: Apache-2.0 (see `LICENSE` and the README; it lets people modify anywhere and keep their changes with less friction). It was private until a working demo. Done with it: `license = "Apache-2.0"` in the Cargo manifests and package.json files, an `SPDX-License-Identifier: Apache-2.0` header on our source files, checked by `M9-LIC-01`. The holder line came with the rename: `Copyright 2026 The Platypus Authors` above it, and `NOTICE`
- [x] Third-party notices (2026-10-28): `tools/platypus_notices` writes `THIRD-PARTY-NOTICES.md` from Cargo.lock (the host's crates, one section per licence, texts once, NOTICE files and bundled code) and `hand-written.md` (ANGLE and what it bundles, Inter); unknown licences, copyleft and crates without a licence file fail unless in its reviewed table. Built into `platypus-app` (`--licenses`) and `platypus-build` (`package` writes it beside every exe, `--no-notices` to skip); `notices_are_current` fails when Cargo.lock changes without it (`M9-LIC-02..16`)
- [x] Name: **Platypus** (2026-10-27, was Iron): crates `platypus_*`, binaries `platypus-app` (the one host since 2026-10-28; between main's two-host work and then the hosts were `platypus-game` and `platypus-app`, picked by `project.json` `type`, and before that one `platypus`) / `platypus_editor` / `platypus-build`, the renderer crates `platypus_render_cpu` / `platypus_render_gles`, `PLATYPUS_*` variables, `@platypus/*` packages, `game.pbundle`, `.ppk`; files of the old names are still read (`M9-REN-01..04`). The icon `resources/icon/icon.png` is the default for every game without one (`M9-ICO-01..06`)

---

## Proposed repository layout

```
Platypus/
├─ Cargo.toml              # workspace
├─ package.json            # Bun workspace root
├─ DESIGN.md  TESTS.md  TODO.md
├─ crates/
│  ├─ platypus_core/           # IDs, math re-exports, op buffer types, logging
│  ├─ platypus_platform/       # Platform layer: winit + gilrs
│  ├─ platypus_render/         # wgpu device, surfaces, offscreen targets
│  ├─ platypus_script/         # ScriptRuntime trait + QuickJS backend + shared test suite
│  ├─ platypus_ui/             # (M2) UI tree, LayoutEngine trait, Taffy backend, UI renderer
│  └─ platypus_app/            # the `platypus-app` host (and `platypus_editor`): main loop wiring everything together
├─ js/
│  ├─ engine/              # @platypus/engine: Component base class, engine API, .d.ts
│  └─ react/               # (M2) @platypus/react: custom reconciler
├─ examples/
│  └─ hello/               # TS game: a Hello component
└─ tests/
   ├─ fixtures/            # tiny TS bundles, glTF files
   └─ golden/              # reference PNGs
```

---

## M1 — Hello Platypus (scaffold)

**Goal:** `cargo run -p platypus_app -- examples/hello` opens a window with a clear colour, loads a Bun-built TS bundle, and a `Hello` component logs `hello` once and `tick` every frame.

### 1. Repository and tooling
- [x] `git init`, `.gitignore` (target/, node_modules/, build/), `.gitattributes` (LF line endings, PNGs as binary)
- [x] Cargo workspace with crates from the layout above — `M1-BLD-01`
- [x] `rustfmt.toml`; `cargo fmt --check` / `cargo clippy -D warnings` clean — `M1-BLD-02`
- [x] Bun workspace: root `package.json`, `js/engine`, `examples/hello`, `tests/fixtures`, shared `tsconfig.base.json` (strict) — `M1-BLD-03`, `M1-BLD-04` (Bun 1.4.3)
- [x] Logging via `tracing` + `tracing-subscriber` (`RUST_LOG` to override)

### 2. `platypus_core`
- [x] Typed IDs (`EntityId`, `ComponentId`, `NodeId`) as `u32` newtypes
- [x] `Op` and `OpBuffer` (name + `Value` args; binary encoding comes later)
- [x] `Value` enum for values crossing the script boundary
- [x] Re-export `glam` for math

### 3. `platypus_platform`
- [x] `Platform` wrapping the winit event loop (`ApplicationHandler`)
- [x] Headless mode for tests — `M1-PLT-01`
- [x] Convert winit events to Platypus `InputEvent`s — `M1-PLT-02`
- [x] Clean exit on window close — `M1-PLT-03` (manual, passed 2026-10-16 on Windows)
- [x] `gilrs` polling stub (wired up properly in M3)

### 4. `platypus_render`
- [x] wgpu instance/adapter/device; software fallback when no hardware adapter — `M1-RND-01`
- [x] Window surface (sRGB, handles outdated/lost/minimized) + offscreen target
- [x] Clear to colour, read back pixels (with row padding) — `M1-RND-02`
- [x] Resize and 0×0 (minimized) are safe — `M1-RND-03`
- [x] Golden image helper (`testing::assert_golden`, `PLATYPUS_UPDATE_GOLDEN=1`)

### 5. `platypus_script`
- [x] `ScriptRuntime` trait: `load_bundle`, `eval`, `call_global`, `drain_ops`, `reset`, `set_log_sink`
- [x] `ScriptError` with message, file, line, stack
- [x] QuickJS backend (`rquickjs`) — `M1-SCR-01`..`M1-SCR-03`
- [x] `console.*` → log sink (default: `tracing`, target `script`) — `M1-SCR-04`
- [x] `reset()` creates a fresh context, keeps natives and log sink — `M1-SCR-05`
- [x] Native `__platypus_op(name, ...args)` queues ops — `M1-SCR-06`
- [x] UTF-8 ↔ UTF-16 round-trip test — `M1-SCR-07`
- [x] Backend-generic suite (`runtime_conformance_tests!` in `platypus_script::conformance`) — `M1-SCR-08`

### 6. `@platypus/engine` (TS) and components
- [x] `Component` base class with `onStart()` / `onUpdate(dt)`
- [x] `register({ Hello })` registry (object form keeps names stable under minification) — `M1-CMP-01`
- [x] `ComponentHost` calls methods by `(component id, method name)` via `__platypus_dispatch` — `M1-CMP-02`, `M1-CMP-03`
- [x] Exceptions disable only the failing component — `M1-CMP-04`
- [x] Hand-written globals `.d.ts` (`js/engine/src/globals.ts`; generate from Rust later)
- [x] `bun test` unit tests for the JS host (`js/engine/test/`, 4 tests)

### 7. `platypus_app` and the example
- [x] `platypus` binary: game folder (`build/game.js`) or `.js` path; opens window; runs loop
- [x] Fixed 60 Hz tick with catch-up limit; clear colour each frame
- [x] `examples/hello`: `Hello` logs `hello` on start and `tick` on update
- [x] `tests/fixtures/{hello,throws}.js` built by Bun from `tests/fixtures/src/*.ts` (`bun run build:fixtures`; CI checks they're up to date)
- [x] `platypus examples/hello` runs the Bun-built game end to end

### 8. CI
- [x] GitHub Actions workflow (`.github/workflows/ci.yml`): Windows + Linux Rust jobs, JS job with Bun
- [x] Software rendering on CI runners (WARP on Windows, Mesa llvmpipe on Linux)
- [ ] Push to GitHub and see it go green (not yet run)

**M1 done when:** all `M1-*` tests pass in CI and `M1-PLT-03` passes manually.

**Status:** all 24 Rust tests and 4 JS tests pass locally (Windows, RTX 5080); build and typecheck are clean. `platypus examples/hello` runs on Vulkan: `hello` once, ~60 ticks/s. The manual close test passed. Remaining: a CI run.

---

## M2 — Boxes (React → Taffy → screen)

**Goal:** `platypus examples/boxes` shows a React-rendered layout of coloured, rounded, bordered boxes that relayouts when the window resizes and when React state changes (a timer-driven counter that adds/removes boxes).

**Versions checked 2026-10-16:** React 19.3.1, react-reconciler 0.34.0, Taffy 0.15.0, criterion 0.8.2.

### M2 decisions (proposed, confirm before starting)
- **D1 — Op transport:** the reconciler collects ops in a JS array during a commit and calls a native `__platypus_commit(ops)` **once** in `resetAfterCommit`. Ops are flat arrays: `[opcode, ...args]`. Start with the existing `Value` conversion; switch to a binary `ArrayBuffer` encoding only if `M2-BEN-04` says so.
- **D2 — Style values:** numbers mean logical pixels; strings are CSS values parsed in Rust (Taffy's `parse` feature for layout properties, a small parser for colours). Styles travel as flat `[prop, value, prop, value, ...]` lists, so `Value` needs no object type. M3's CSS files reuse the same parsers.
- **D3 — Elements in M2:** `<box>` only. Text arrives in M3; `createTextInstance` throws a clear "text isn't supported until M3" error.
- **D4 — Scheduling:** React 19 + react-reconciler 0.34. The script runtime gains `setTimeout`/`clearTimeout`/`queueMicrotask` and runs the QuickJS job queue every frame. Root renders use `updateContainerSync` + `flushSyncWork` so a frame's state changes commit in the same frame.
- **D5 — Crate split:** a new `platypus_ui` crate owns the node tree, op decoding, style parsing, the `LayoutEngine` trait with the Taffy backend, and produces a GPU-free **draw list** of quads. `platypus_render` gains a `QuadRenderer` that draws any draw list. UI stays testable without a GPU, and the quad renderer is reusable for world panels later.
- **D6 — Units:** layout in logical pixels; the window's scale factor is applied only in the renderer.

### 1. Script runtime additions (`platypus_script`)
- [x] `setTimeout` / `clearTimeout` in a shared JS prelude (`platypus_script/src/prelude.js`; Rust holds no JS function refs); `queueMicrotask` is built into QuickJS-ng. Timers fire from `ScriptRuntime::tick(now)`, zero-delay chains in the same tick, capped at `MAX_TIMERS_PER_TICK` — `M2-SCR-01`
- [x] Pending QuickJS jobs (promises, microtasks) run after every call into JS, including between timers — `M2-SCR-02`
- [x] `__platypus_commit(ops)` native → `Commit` (in `platypus_core`), collected with `drain_commits()` — `M2-SCR-03`
- [x] Uncaught timer/job errors collected via `__platypus_report_error` and returned by `tick()`
- [x] Bundle defines: `process.env.NODE_ENV` via `bun build --define` (development for `examples/hello`, production for test fixtures)
- [x] New tests are in the backend conformance suite (7 added; 15 total)
- [x] `platypus_app` calls `tick` every frame (timers work in games now); commits drained until step 6 consumes them

### 2. `platypus_ui` crate: tree, styles, layout (no GPU)
- [x] Opcodes (`platypus_ui::opcodes`): `CREATE`, `SET_STYLE`, `APPEND_CHILD`, `INSERT_BEFORE`, `REMOVE_CHILD`. Simplified from the plan: the root container is node `0` (no separate root ops), and `REMOVE_CHILD` destroys the subtree (no `DESTROY`), because React never reinserts removed instances
- [x] `UiTree`: applies a commit, syncs with layout once per commit; bad ops (unknown IDs, duplicates, cycles, bad args) are reported and skipped — `M2-UI-01`, `M2-UI-02`
- [x] Style parsing: Taffy `parse` for layout props; colours as `#rgb(a)`, `#rrggbb(aa)`, `rgb()`/`rgba()` (comma, space, `/ alpha`), `transparent`, a few names. `null` resets a prop — `M2-STY-01`, `M2-STY-02`
- [x] M2 style subset: `display` (flex/grid/block/none), flex props + `flex` shorthand, `width/height/min/max`, `aspectRatio`, `padding`/`margin`/`borderWidth`/`inset` (1–4 value shorthands and per side), `gap`/`rowGap`/`columnGap`, `position` + insets, alignment, `gridTemplateColumns/Rows`, `gridAutoFlow`, `gridColumn/Row`, `backgroundColor`, `borderColor`, `borderRadius`, `opacity`
- [x] `LayoutEngine` trait + `TaffyLayout` backend; `layout_if_needed` only runs when something changed — `M2-LAY-01`..`M2-LAY-04`
- [x] 24 layout fixtures as data (`tests/layout/*.json`: wrapping, shrink weighting, min/max, auto margins, justify distribution, reverse, alignSelf, aspect ratio, nested percentages, grid repeat/spans/alignment, absolute insets), run through the full op pipeline by a backend-generic runner — `M2-LAY-05`
- [x] Root resize relayouts the tree — `M2-UI-03`
- [x] Draw list: one `Quad` per visible box (absolute rect, colours with inherited opacity, resolved border widths, radius) in paint order — `M2-UI-04`

### 3. `@platypus/react` package (`js/react`)
- [x] Host config for react-reconciler 0.34 (mutation mode) in `js/react/src/host.ts`; React 19's required host functions (priorities, transitions, suspend-commit hooks, `bindToConsole`) stubbed in one place. Ops flushed with one `__platypus_commit` in `resetAfterCommit`
- [x] Node IDs: numbers from a counter, never reused in a session — `M2-REC-05`
- [x] `commitUpdate` diffs `style` and emits only changed properties (`null` for removed ones); unchanged styles emit nothing — `M2-REC-02`
- [x] Suspense support via `hideInstance`/`unhideInstance` (toggles `display`)
- [x] `createRoot().render(<App/>)` (one root per game, sync commits via `updateContainerSync` + `flushSyncWork`); `<box>` typed through `JSX.IntrinsicElements` with a typed `PlatypusStyle`
- [x] Clear errors for text children (until M3) and unknown elements, reported through React's error callbacks
- [x] `bun test` suite (9 tests) with a fake `__platypus_commit` feeding a model of the Rust tree, so tests check the resulting tree rather than React's ID order — `M2-REC-01`..`M2-REC-05`
- [ ] Note: renders abandoned by concurrent React could leave created-but-never-attached nodes in the Rust tree. Not an issue with sync rendering; revisit if we enable transitions

### 4. Integration in QuickJS
- [x] React fixture (`tests/fixtures/src/boxes.tsx` → `boxes.js`, production + minified, ~148 KB) runs in `QuickJsRuntime` with no extra shims; its first render is one commit, and applied to `UiTree` it gives the expected tree, layout and draw commands — `M2-INT-01`
- [x] A `setTimeout`-driven state change commits once within `tick`, creating only the two new nodes and keeping existing IDs; the grid relayouts to two rows — `M2-INT-02`
- [x] Built fixtures marked as generated in `.gitattributes` (no noisy diffs; CI still checks they match their sources)

### 2b. Box visuals in `platypus_ui` (styles + draw commands, no GPU) — see DESIGN §5.2
- [x] Colours: `hsl()/hsla()` (deg/turn/rad/grad hues) and all 148 CSS named colours — `M2-VIS-01`
- [x] `border` shorthand (`"2px solid #fff"`, any order, `thin/medium/thick`; only `solid`/`none`/`hidden` styles; like CSS, no style means no border) — `M2-VIS-02`
- [x] `borderRadius` with 1–4 values, per-corner props, percentages; resolved at draw time and scaled down like CSS when corners would overlap. Circular corners only (no `a / b` elliptical radii yet) — `M2-VIS-03`
- [x] `boxShadow` list parser: offsets, blur, spread, colour, `inset`, `none` — `M2-VIS-04`
- [x] `filter: blur()` and `backdropFilter: blur()` (`none` resets; other filter functions rejected for now) — `M2-VIS-05`
- [x] `backgroundImage` / `background`: `linear-gradient(...)` (angles, `to` sides and corners), `radial-gradient(...)` (circle/ellipse, centred), `url(...)`; `backgroundSize`, `backgroundPosition`, `backgroundRepeat` — `M2-VIS-06`
- [x] Draw list is now `Vec<DrawCmd>`: `BackdropBlur`, `PushLayer`, `BoxShadow`, `Quad` (with optional image fill and per-corner radii), `InsetShadow`, `PopLayer`, in CSS paint order — `M2-VIS-07`
- [x] Group opacity through layers; simple single-quad leaves just scale alpha — `M2-VIS-08`
- [ ] Later: per-side border colours, elliptical radii, `radial-gradient` position/size keywords, more filter functions (`brightness`, `grayscale`, `drop-shadow`, ...), multiple backgrounds

### 5. Quad renderer (`platypus_render`)
- [x] `UiRenderer` (`platypus_render::ui`): instanced quads with an SDF shader for per-corner radii and borders (background extends under the border, like CSS), premultiplied alpha, one physical pixel of anti-aliasing
- [x] Linear and radial gradient fills (up to 8 stops, CSS stop fix-up, magic-corner directions, premultiplied-sRGB interpolation)
- [x] Image fills: `url(...)` loaded from the game folder (relative paths only; `..`, absolute paths and URL schemes rejected), cached textures, `backgroundSize`/`Position`/`Repeat`, clipped to the rounded box, border drawn on top
- [x] Analytic box shadows (outer clipped outside the box, inset clipped to the padding box), one quad each
- [x] Offscreen layers for `filter: blur` and group opacity; backdrop copy + separable Gaussian blur, clipped to the rounded box, for `backdropFilter`. Textures pooled across frames
- [x] Frame drawn into an internal sRGB canvas, then blitted to any target format
- [x] Golden + pixel-assertion tests, all checked by eye — `M2-DRW-01`, `M2-DRW-02`, `M2-DRW-05` (+ images), `M2-DRW-06`..`M2-DRW-08`
- [x] 2,000 boxes in one draw call — `M2-DRW-03`
- [x] Scale factor applied at draw time — `M2-DRW-04`
- [x] Blending in sRGB by default, as browsers (2026-10-30): `blending: "srgb" | "linear"` in `project.json` and `set_blending` on the renderers, one setting per frame in `FramePlan::blending`; `examples/alpha-probe`, `M2-BLEND-01..11` (docs/rendering.md "Blending"). A runtime switch for scripts is not done
- [x] System theme (2026-10-30, asked by the Fluid desktop): `prefers-color-scheme` follows the system live; on Linux from the XDG Settings portal (`ReadOne`/`Read`, `SettingChanged` on a thread waking the loop; `gsettings` once as fallback; `zbus`, Linux only), elsewhere winit; `project.json` `theme: "system" | "light" | "dark"` forces it; `M2-THEME-01..03` (docs/platform.md "System theme"). Still to do: the manual check on a Linux desktop (`M2-THEME-04`); the Linux code is type-checked only on Windows (no Linux target here)
- [ ] Later: large blurs should downsample (dual Kawase) instead of up to 257 taps; per-op uniform buffers and bind groups could be pooled; gradients ignore `backgroundSize`/`Position` for now

### 6. App wiring and example
- [x] `platypus_app`: each frame calls `tick(now)`, applies commits to `UiTree` (bad ops logged), relayouts if dirty, and draws with `UiRenderer` via `WindowSurface::present_with`
- [x] Window size and scale factor → root size in logical pixels → relayout
- [x] Bundles can register components, render UI, or both; `url()` images load from the game folder
- [x] `platypus <game> --screenshot out.png [--frames N]` saves the UI after N frames and exits (used to verify the examples)
- [x] `examples/boxes`: showcase of every M2 feature (glass header over blobs, gradients, images, shadows, radii, blur, group opacity) plus a timer-driven counter that adds and removes boxes; checked by screenshot
- [x] Found and fixed while doing this: unitless numbers inside CSS strings (`padding: "0 20px"`) were rejected by Taffy's parser
- [x] Test GPU shared and serialised (`testing::shared_gpu`): creating devices on parallel test threads deadlocked on this machine's Vulkan setup
- [ ] Manual check: resize the window and watch it relayout — `M2-MAN-01` (**needs you**: `cargo run -p platypus_app -- examples/boxes`)

### 7. Benchmarks and the Taffy/Yoga decision
- [x] `criterion` benches in `platypus_ui` on a 2,246-node editor-like tree — `M2-BEN-01`..`M2-BEN-03`
- [x] JS→Rust commit transfer bench (QuickJS) — `M2-BEN-04` (0.40 ms per 1,000 ops; binary encoding not needed yet)
- [x] Profiled and fixed our overhead: boxed UI nodes and FxHash maps (commit apply 5.1 → 3.1 ms)
- [x] Results, analysis and revised targets recorded in TESTS.md
- [x] **Decision: keep Taffy.** Costs fit the frame budget (~0.8 ms worst-case incremental update for 2,246 nodes); Yoga would do the same ancestor relayout. Revisit at M5.e with the real editor UI

**M2 status:** steps 1–7 done. Remaining: the manual resize check (`M2-MAN-01`) and a CI run (needs a push).

**Suggested order:** 1 → 2 → 3 → 4 → 5 → 6 → 7. Steps 2 and 3 are independent once the opcode list (step 2's first item) is agreed, so they can run in parallel.

## M3 — Real UI: text, CSS, interaction

**Goal:** game UIs with real text and fonts, CSS stylesheets with selectors and pseudo-elements, and
interaction (pointer, keyboard, focus, text input with IME, scrolling). Planned in three parts that land
in order: **M3a** text and fonts → **M3b** CSS stylesheets → **M3c** interaction. Each part ends with
its tests passing and a commit per step. **M3d** (CSS parity with s&box) is a backlog of CSS breadth
to pick from after M3c; its items can also be pulled forward when an example needs them.

**Versions checked 2026-10-16:** parley 0.12 (+ fontique 0.12, harfrust shaping), swash 0.2.10,
selectors 0.41, cssparser 0.38, cosmic-text 0.19 (alternative), arboard for the clipboard.

### M3 decisions (proposed, confirm before starting)
- **D1 — Text engine: Parley.** Same Linebender family as Taffy (Blitz renders CSS with taffy + parley +
  selectors). `TreeBuilder` maps nested styled spans directly, `InlineBox` supports inline elements
  (icons, `::before` boxes), `PlainEditor` handles editing with IME composition, and `fontique` gives
  font registration plus system fallback. cosmic-text is the fallback if Parley's still-young API hurts.
- **D2 — Glyph rasterization: swash**, at physical pixel size with 4 horizontal subpixel bins, into a
  glyph atlas (R8 coverage pages + an RGBA page for colour emoji).
- **D3 — Glyphs batch with boxes.** The atlas is bound in the quad pipeline and glyphs are a new
  instance kind, so text interleaves with boxes in paint order without breaking batches.
- **D4 — Text shadows via layers:** each shadow draws the text's glyphs tinted and offset into a layer
  that is blurred with the existing blur pass (exact CSS Gaussian). Multiple shadows stack in CSS order.
- **D5 — CSS engine:** `cssparser` for stylesheets, the `selectors` crate (Servo's) for matching against
  our tree, and the existing `apply_style` for declarations (kebab-case → camelCase). Stylesheets come
  from Bun-bundled CSS imports (Bun writes `build/game.css` next to `game.js`) and from an
  `addStyleSheet(cssText)` API for CSS-in-JS.
- **D6 — Events:** Rust hit-tests and sends the target's ancestor path to JS; `@platypus/react` runs capture
  and bubble phases over React props (`onClick`, `onPointerDown`, ...) with `stopPropagation` and
  `preventDefault`, like React DOM.
- **D7 — Text inputs are Rust-owned editors** (Parley `PlainEditor`: caret, selection, IME, clipboard),
  exposed as controlled React elements (`value` + `onChange`).
- **D8 — Fonts:** a bundled default family (Inter, SIL OFL) so games look the same everywhere; system
  fonts through fontique for fallback (CJK, emoji); game fonts via `@font-face` and `loadFont()` from
  the game folder (same path rules as images). TTF/OTF first; WOFF2 later.

### M3a — Text and fonts
1. **Text in the tree**
   - [x] Elements `text` and `span` (exposed as `Text`/`Span` components, since React's JSX types already
     claim `<text>` and `<span>`); React strings become text nodes (`CREATE_TEXT`, `SET_TEXT`); strings
     outside text containers are anonymous text blocks — `M3-TXT-07`
   - [x] Opcode additions (`CREATE_TEXT`, `SET_TEXT`) mirrored in `js/react/src/opcodes.ts`
2. **Text styles and inheritance**
   - [x] `color`, `fontFamily` (fallback lists), `fontSize` (px/em/%), `fontWeight` (1–1000, `bold`,
     `normal`), `fontStyle`, `lineHeight` (factor or length), `letterSpacing`, `wordSpacing`, `textAlign`,
     `textDecoration`, `whiteSpace` (normal / nowrap / pre / pre-wrap), `textShadow` (parsed) — `M3-TXT-04`
   - [x] Computed-style inheritance (`UiTree::computed_text`); `currentColor` for backgrounds, borders and
     shadows, and as the CSS default when `border`/`boxShadow` omit a colour — `M3-TXT-06`
   - [ ] `currentColor` inside gradient stops (later)
3. **Fonts**
   - [x] Bundled Inter (variable, upright + italic, SIL OFL) for `sans-serif`/`system-ui`; fontique
     system fallback in the app (`UiTree::with_system_fonts`), bundled-only in tests — `M3-FNT-02`
   - [x] `loadFont({ family, src, weight, style })` → op → loaded from the game folder (shared
     `platypus_core::resolve_asset` path rules) → registered; text relayouts — `M3-FNT-01`
   - [x] A family with only an italic face uses it for normal text (CSS font matching), instead of
     falling back to another family: works with Parley 0.12 / fontique; guarded by a test (2026-10-17)
     — `M3-FNT-02`
4. **Layout integration**
   - [x] Text blocks are Taffy measured leaves (Taffy 0.15's `compute_leaf_layout` + Parley line
     breaking for min-content / max-content / definite widths) — `M3-TXT-02`
   - [x] Shaping happens only for dirty blocks; re-measuring just re-breaks lines — `M3-TXT-03`
5. **Rendering**
   - [x] Glyph atlas (2048² R8 coverage page + 1024² RGBA colour page, cleared when full), swash
     rasterization at physical size with 4 horizontal subpixel bins, variable-font coordinates,
     synthetic bold/italic — `M3-TXT-01`
   - [x] `DrawCmd::Text` (glyph runs + decorations) → glyph instances in the quad pipeline (texel-exact
     `textureLoad`), so text batches with boxes
   - [x] Text contrast: coverage boost for dark text to offset linear-light blending
   - [x] Underline / line-through from font metrics, drawn as quads
   - [x] `textShadow` lists: tinted, offset copies under the text; blurred ones in layers — `M3-TXT-05`
   - [x] Golden text sampler at 1× and 2× (weights, italic, sizes, decorations, shadows, spans), checked by eye
   - [x] Colour emoji and CJK through system fonts, verified in `examples/text` (goldens use the bundled font only)
6. **Example + checks**
   - [x] `examples/text`: weights 300–800, italic face, decorations, wrapping and alignment, a font loaded
     at runtime (Charm, regular + bold, Latin and Thai), hard/soft/glow text shadows, CJK, Greek,
     Cyrillic and colour emoji via system fallback, and a live clock; verified by `--screenshot`.
     The Charm files are only present locally (git-excluded); without them the card uses fallback fonts

**M3a status:** done.

### M3b — CSS stylesheets and selectors
1. **Stylesheets**
   - [x] Parse stylesheets with `cssparser`: style rules, `@font-face` (feeds `loadFont`), comments,
     `!important`; unsupported at-rules warn — `M3-CSS-01`
   - [x] Sources: `build/game.css` loaded with the bundle; `addStyleSheet(cssText)` / `removeStyleSheet`
   - [x] Declarations: kebab-case → our camelCase props; values as CSS strings into `apply_style`
   - [x] Custom properties (`--x`) and `var(--x, fallback)` — `M3-CSS-05`
2. **Selectors** (`selectors` crate, `Element` implemented for UI nodes)
   - [x] Simple selectors: type (`box`, `text`, `span`, `input`, `textarea`, `checkbox`), universal
     `*`, `.class` (`className` prop), `#id` (`id` prop), attribute selectors on `data-*` props — `M3-CSS-08`
   - [x] Combinators: descendant (space), child `>`, next sibling `+`, subsequent siblings `~` — `M3-CSS-02`
   - [x] Structural: `:first-child`, `:last-child`, `:only-child`, `:nth-child(an+b [of S])`,
     `:nth-last-child()`, `:first-of-type`, `:last-of-type`, `:nth-of-type()`, `:root` — `M3-CSS-02`
   - [x] Emptiness: `:empty` (no children and no text) and `:blank` (no children, or only whitespace
     text; Selectors Level 4, so implemented as our own pseudo-class) — `M3-CSS-09`
   - [x] Logical: `:not()`, `:is()`, `:where()`, `:has()` (relative selectors) — `M3-CSS-03`
   - [x] User action (driven by M3c, testable earlier by setting states directly): `:hover`, `:active`,
     `:focus`, `:focus-visible`, `:focus-within` — `M3-CSS-10`
   - [x] Form state: `:enabled` / `:disabled` (the `disabled` prop on inputs, buttons and boxes with
     `tabIndex`; disabled elements also ignore pointer and focus), `:checked` (the `checked` prop on
     a new `<checkbox>` element and on `<input type="checkbox" | "radio">`) — `M3-CSS-10`
3. **Cascade**
   - [x] Specificity + source order; inline `style` prop beats stylesheet rules (unless `!important`);
     inheritance from M3a — `M3-CSS-04`
4. **Pseudo-elements**
   - [x] `::before` / `::after` generate anonymous first/last children (invisible to React), styled by
     their rules; `content: ""` boxes for decoration and `content: "text"` for text — `M3-CSS-06`
5. **Restyle**
   - [x] Rule index: selectors bucketed by the id / class / type of their rightmost compound
   - [x] Dependency-aware invalidation: at load, record which classes, ids, attributes and states appear
     in ancestor positions, before `+`/`~`, inside `:has()`; a change restyles the node plus only the
     subtree / later siblings / ancestors it can affect. Structural changes restyle siblings only when
     structural selectors exist — `M3-CSS-07`
   - [x] Benchmark: one class change on the 2,246-node tree 0.20 ms (target < 1 ms); whole tree 8.8 ms — `M3-BEN-01`
6. **Example + checks**
   - [x] `examples/css`: a `.css` file imported through Bun (`build/game.css`, `url()`s rebased, small
     assets inlined as `data:` URLs), classes, combinators, structural selectors, `:has`/`:not`/`:empty`,
     `:checked`/`:disabled`, `::before`/`::after`, `@font-face`, and a runtime `addStyleSheet` that
     overrides `--accent` every second

**M3b status:** done. The `<checkbox>` / `<input>` element types arrive with M3c; `:checked` and
`:disabled` already work on any element through the `checked` / `disabled` props.


### M3c — Interaction
1. **Hit testing and pointer events**
   - [x] Hit test in reverse paint order: rounded corners, `display: none`, `pointerEvents: none`
     (inherited, children can opt back in); text hits its text element, `::before`/`::after` their
     owner — `M3-EVT-01` (`crates/platypus_ui/tests/hit.rs`)
   - [x] Clipping in hit tests: `overflow` and `clipPath` cut away what isn't drawn — `M3-EVT-05`
     (`crates/platypus_ui/tests/clip.rs`)
   - [ ] Spans inside text as targets (needs node ids in the text brush); `visibility`
   - [x] Pointer events to JS with the target's ancestor path; capture + bubble phases,
     `stopPropagation`, `preventDefault`; `onClick`, `onPointerDown/Up/Move/Enter/Leave`, `onWheel`,
     double click, `onContextMenu`, `onMouse*` aliases — `M3-EVT-02`, `M3-EVT-03`
     (`platypus_ui::Pointer` in `events.rs`, `js/react/src/events.ts`, `crates/platypus_ui/tests/pointer.rs`,
     `events_quickjs.rs`)
   - [x] `:hover` / `:active` states (hover re-tested each frame when the tree moves under the
     pointer); `cursor` CSS property (inherited) → OS cursor
2. **Keyboard and focus**
   - [x] `tabIndex`, Tab / Shift-Tab order (wrapping), `onFocus`/`onBlur` (bubbling, with
     `relatedTarget`), `:focus`, `:focus-visible` (keyboard only), `:focus-within`; `onKeyDown` /
     `onKeyUp` to the focused element with DOM `key`/`code`/`repeat`; pressing focuses the nearest
     focusable ancestor or blurs (unless `pointerdown` is prevented) — `M3-EVT-04`
     (`platypus_ui::Interaction` in `interaction.rs`, `crates/platypus_ui/tests/focus.rs`, `events_quickjs.rs`)
   - [x] Imperative `ref.current.focus()` / `blur()` via ops
3. **Text input**
   - [x] `<input>` and `<textarea>` (`Input` / `TextArea` components) backed by Parley
     `PlainEditor`: typing, Backspace/Delete (Ctrl for words), arrows/Home/End with Shift and Ctrl,
     Ctrl+A, Enter in textareas; caret placement, word (double-click) and line (triple-click)
     selection and drag selection; blinking caret, selection highlight, dimmed `placeholder`, the
     caret kept in view; `value` (controlled, restored when `onChange` rejects an edit like React
     DOM), `defaultValue`, `readOnly`, `disabled`, `rows`; focusable by default; `onChange` /
     `onInput` with `e.target.value`; Ctrl+C/X/V produce clipboard requests — `M3-INP-01`
     (`crates/platypus_ui/tests/editor.rs`, golden `M3-INP-01-fields`)
   - [x] Ctrl/Cmd shortcuts fall back to the physical key `code` on non-Latin layouts (Ctrl+ф is
     Ctrl+A on a Russian layout); AltGr still types — `M3-INP-02`. Hand-checked 2026-10-16:
     Ctrl+A/C/X/V work on English, French, Russian, Arabic, Hebrew, the Chinese Pinyin layout and
     with the Japanese IME on
   - [x] Pasting into an `<input>` flattens line breaks to spaces and drops a trailing one
     (`M3-INP-01`)
   - [x] Placeholders re-wrap when the field resizes (they kept the wrap width of their first
     layout) — `M3-INP-01`
   - [x] System clipboard (arboard, opened on first use) for the requests above, in `platypus_app`
   - [ ] Later: undo/redo, `type="password"`, `maxLength`, wheel scrolling inside a textarea,
     `::placeholder` / `::selection` / `caret-color` styling
   - [x] Selecting static text and `user-select`: done in M3d step 5h
   - [x] IME: winit `Ime` events → pre-edit shown underlined at the caret (not part of the value,
     no `input` events), commit inserted with an `input` event, pre-edit dropped when focus
     leaves; IME is allowed only while an editable field has focus and the candidate window
     follows `set_ime_cursor_area` — `M3-IME-01`. Manual check (`M3-MAN-01`, 2026-10-16, Windows
     Microsoft IME, driven by computer use): Japanese pre-edit, conversion, commit, Backspace /
     Escape in the pre-edit, textarea lines, the candidate window under the field, IME only while
     a field has focus, Russian and French typing, and the clipboard all work. Re-checked after the
     shortcut, segmentation and RTL changes with no regressions. Chinese Pinyin composition not
     checked: Windows reports its dictionaries aren't ready, so it stays in English (英) mode
   - [x] Complex-script segmentation: Parley's `complex-scripts` feature compiles in ICU4X
     dictionaries, so CJK word moves/selection and Thai, Lao, Khmer and Burmese line breaks
     follow word boundaries (no more "No segmentation model" log) — `M3-TXT-08`. Costs about
     3.8 MB in the release binary
   - [x] Right-to-left text: inherited CSS `direction` and the `dir` attribute (CSS wins;
     `[dir=rtl]` matches) set Parley's paragraph direction; `text-align: start/end` follow it;
     rtl inputs/textareas align right (placeholder too) with visual arrow keys — `M3-TXT-09`.
     Shown in `examples/css` (Arabic line and input). Hand-checked 2026-10-16 with the Arabic (101)
     and Hebrew keyboards: typing, shaping, bidi order, Home/End, visual arrows, Backspace and
     Ctrl+A/C/X/V all work
   - [ ] RTL follow-ups: `dir="auto"`, flex/grid `row` order and `justify-content: start` in rtl
     (Taffy has no direction), logical properties (`margin-inline-start`, `inset-inline-*`)
4. **Overflow and scrolling**
   - [x] `overflow` / `overflowX` / `overflowY` (`visible | hidden | clip | scroll | auto`, CSS
     computed-value rules) clip children to the padding box with inner radii; `clipPath`
     (`inset() round`, `circle()`, `ellipse()`, box keywords) and legacy `clip: rect()` clip the
     element itself. `DrawCmd::PushClip`/`PopClip`; the renderer keeps a clip table (elliptical
     corners, chained) and tags each instance, so clips don't split batches; layers are clipped
     when composited; fully clipped commands are culled — `M3-EVT-05` (golden `M3-EVT-05-clip`)
   - [x] Scrolling: scroll offsets shift content (rects, hit testing and drawing agree; offsets
     clamp when content shrinks); the wheel scrolls the innermost container that can move and
     chains outward (Shift for horizontal); arrow keys, Page Up/Down, Space, Home/End; Tab and
     `focus()` scroll the element into view; overlay scrollbars (`scroll` always, `auto` when
     needed) with thumb dragging and track paging; `scrollbarWidth`, `scrollbarColor`; `onScroll`
     and `ref.scrollTo/scrollBy/scrollIntoView()` — `M3-EVT-05` (`crates/platypus_ui/tests/scroll.rs`,
     golden `M3-EVT-05-scroll`)
   - [ ] Later: smooth scrolling, scroll snapping, culling off-screen children in the draw list
     (the renderer already culls their GPU work), touchpad momentum
5. **Gamepads**
   - [x] `gilrs` events with stable button/axis names (`South`, `DPadUp`, `LeftStickX`, ...) to
     scripts through `@platypus/engine`'s `input` (`isKeyDown`, `gamepads()`, `isButtonDown`,
     `on("gamepadbuttondown" | "keydown" | ...)`; keys typed into a text field stay the
     field's); spatial focus navigation with the d-pad / left stick and South to click, only
     while UI has focus — `M3-PAD-01` (`crates/platypus_platform/src/gamepad.rs`,
     `crates/platypus_app/src/game_input.rs`, `crates/platypus_ui/tests/navigation.rs`,
     `js/engine/test/input.test.ts`)
   - [ ] Later: rumble, button repeat while held, mouse state in `input`
6. **Example + checks**
   - [x] Manual IME check on Windows (`M3-MAN-01`, IME part): done in `examples/css` with the
     Microsoft Japanese IME (see Text input above)
   - [x] `examples/form`: a profile form with controlled fields (validation, a character
     counter), keyboard- and gamepad-operable toggles and buttons (`:hover`, `:active`,
     `:focus-visible`, `:disabled`), Enter to submit, and a scrolling list of saved profiles;
     focus starts in the first field so the d-pad works at once (checked by screenshot)
   - [x] Hand check: `examples/form` with a real gamepad (d-pad / A navigation), 2026-10-16
   - [x] Hand check: resizing `examples/css` relayouts smoothly (`M2-MAN-01`), 2026-10-16
   - [ ] Hand check still open: Chinese Pinyin composition once Windows has its dictionaries

**M3c status:** done (2026-10-16). Left for hand checks: Chinese Pinyin.

### M3d — CSS parity with s&box

Gaps found by comparing our CSS with s&box's (survey of 2026-10-16, from the official
`Facepunch/sbox-docs` style-properties page and the `Facepunch/sbox-public` engine source). Items
already planned for M3c (`overflow`, `cursor`, `pointer-events`, scrolling) aren't repeated here.
Where s&box has a custom extension, we prefer the standard CSS form if there is one.

**Order agreed for the rest of M3d (2026-10-17):** 5h, 5i, 5j, then groups 7, 8, 9, 10, then
5.1 (SVG); then stop and check in with the user before M4. All done (2026-10-17) except
`sound-in`/`sound-out`, skipped as s&box-specific (CSS stays about display). Added afterwards: groups 11 (pointer capture),
12 (WOFF/WOFF2) and 13 (`@media`). Known deviations from browsers go in
[docs/differences.md](docs/differences.md).

We're already **ahead** of s&box on: custom properties and `var()` (s&box has none), `!important`
(accepted but ignored there), attribute selectors, `:checked`/`:disabled`/`:enabled`/`:focus-within`/
`:focus-visible`/`:blank`, `:nth-child(An+B of S)` and the `*-of-type` family, `:is()`/`:where()`,
and `@font-face`.

1. **Units and values**
   - [x] `em`, `rem`, `vw`, `vh`, `vmin`, `vmax` (and the `d`/`s`/`l` viewport variants) in every
     property, inline or from stylesheets: resolved to px at restyle like `var()`
     (`crates/platypus_ui/src/units.rs`); `em` is the element's own font size (the parent's for
     `fontSize`); nodes using them restyle when the window, their font size or the root font
     size changes (`crates/platypus_ui/tests/units.rs`)
   - [x] `calc()`, `min()`, `max()`, `clamp()` with pixels, relative units and numbers, nested;
     invalid math is ignored (stylesheets warn, inline values are rejected and keep the old value)
   - [x] `calc()` / `min()` / `max()` / `clamp()` with percentages (`calc(100% - 20px)`) in layout
     lengths (sizes, insets, margins, padding, borders, gaps, `flex-basis`): parsed once and
     interned (`crates/platypus_ui/src/calc.rs`), evaluated by Taffy through our own Taffy tree
     (`TaffyLayout` now implements Taffy's tree traits instead of using `TaffyTree`, whose
     `resolve_calc_value` returns 0). Paint lengths (`border-radius`, shadows) don't take them yet
   - [x] `boxSizing: border-box | content-box`; Platypus keeps Taffy's (and s&box's) `border-box`
     default, where CSS defaults to `content-box`
   - [x] CSS-wide keywords `inherit`, `initial`, `unset`, `revert` (and `revert-layer`, both like
     `unset`: no user-agent stylesheet) on every property, inline, in stylesheets and on
     `::before`/`::after`; `inherit` copies the parent's value for non-inherited properties
     (`apply_wide_keyword`, `crates/platypus_ui/tests/wide_keywords.rs`)
   - [x] Colours: `hwb()`, `lab()`, `lch()`, `oklab()`, `oklch()`, `color(srgb | srgb-linear |
     display-p3 ...)`, `none` channels, and `color-mix(in srgb | srgb-linear | lab | oklab | lch |
     oklch, ...)` (premultiplied, shorter hue), the standard form of s&box's `mix()` /
     `darken()` / `lighten()`, which are supported too (SCSS semantics: HSL lightness ± amount;
     `mix(a, b, weight of a)` in sRGB) (`crates/platypus_ui/src/color_fn.rs`); out-of-gamut colours clip
   - [x] Angles in `deg`, `rad`, `grad`, `turn` (hues and gradients); times (`s`/`ms`) come with
     transitions (group 7), the first properties that take them
2. **Layout**
   - [x] `order` (layout children are stably sorted by it; Tab order stays in document order),
     `z-index` (paint order and hit testing among siblings, then `order`, then document order;
     each z-indexed element stacks on its own, so it never lifts above an ancestor's siblings),
     `isolation` (accepted, no effect in that model) (`crates/platypus_ui/tests/layout_props.rs`)
   - [x] `display: contents` (children join the layout parent, flattened before Taffy sees them;
     the element paints nothing and isn't hit, but stays in event paths and passes on inherited
     styles); `position: fixed` (Taffy lays it out as an absolute child of the root; it ignores
     ancestor scrolling and clips and paints above the whole tree, ordered by `zIndex`; ancestor
     opacity and filters don't apply to it)
   - [x] `flex-flow` (Bun's minifier emits it)
   - [x] Shorthands `place-items`, `place-self`, `place-content`
   - [x] Grid: `grid-auto-rows`/`grid-auto-columns`, `grid-template` (`rows / columns` and the area
     strings form with row sizes), `grid-template-areas` (rectangles checked), `grid-area` (a name
     or four lines), `grid-row-start`/`-end`, `grid-column-start`/`-end`
   - [x] Logical properties: `margin-block`/`-inline`, `padding-block`/`-inline`,
     `inset-block`/`-inline` (with `-start`/`-end`), `block-size`/`inline-size` (+ `min-`/`max-`),
     mapped left-to-right (`crates/platypus_ui/tests/layout_props.rs`); RTL-aware mapping later
   - [x] An `image` element (`Image` in `@platypus/react`) with `src` (a path inside the game folder
     or a `data:` URL) and `object-fit` (`fill`, `contain`, `cover`, `none`, `scale-down`) and
     `object-position`: the tree asks the app for natural sizes (`take_image_requests` /
     `set_image_size`, loaded by the renderer's image cache) and measures images like replaced
     elements (one known axis keeps the ratio); drawn in the content box, clipped to the inner
     rounded corners — `M3-IMG-01`
3. **Backgrounds and borders**
   - [x] `conic-gradient()` (`from <angle>`, `at <position>`, stops in angles or percentages) and
     `repeating-linear-gradient()` / `repeating-radial-gradient()` / `repeating-conic-gradient()`;
     two-position colour stops (`red 0 25%`) in every gradient — `M3-BG-01`
   - [x] `background-clip` (`border-box`, `padding-box`, `content-box`, `text` for gradient text:
     the topmost gradient layer fills the element's own glyphs, under the text colour);
     `background-position-x`/`-y`; `background-repeat: round | space` (and per-axis pairs);
     multiple background layers (comma lists, cycled like CSS) — `M3-BG-02`. Images still
     position against the border box (no `background-origin` yet)
   - [x] `background-blend-mode` and `mix-blend-mode`: all 16 W3C modes, blended in sRGB. A
     blended element draws into a layer that the composite pass blends with a copy of its
     backdrop; blended background layers do the same inside an isolated group — `M3-BG-03`
   - [x] `border-style`: `dashed`, `dotted`, `double`, `groove`, `ridge`, `inset`, `outset`; per-side
     colours and styles (`borderColor`/`borderStyle` with 1-4 values, `borderTopColor`,
     `borderTopStyle`, ... and the `borderTop`/... shorthands). Uniform solid borders stay on the
     quad; others are a `DrawCmd::Border` styled per side in the shader (dashes and dots fit a
     whole number per side). Platypus's default style stays `solid` — `M3-BRD-01`
   - [x] Elliptical corner radii (`border-radius: 10px / 20px`, `borderTopLeftRadius: "10px 20px"`);
     percentages now resolve like CSS (horizontal of the width, vertical of the height, so `50%`
     on a wide box is an ellipse). `Radii { x, y }` runs through the draw list, hit testing and
     clips; instances carry `radii_y`. Blurred shadows approximate them with circles — `M3-BRD-02`
   - [x] `outline`, `outline-width`, `outline-color`, `outline-style` (`auto` is solid),
     `outline-offset`: a `DrawCmd::Border` around the border box, following the corner radii,
     drawn after the element's children; no layout or hit-testing effect — `M3-BRD-03`
   - [x] `border-image` and its longhands (source, slice with `fill`, width, outset, repeat:
     `stretch`/`repeat`/`round`/`space`), from images and gradients. `BorderImage::pieces`
     computes the nine pieces; image pieces draw a part of the texture, gradient pieces stretch
     (no tiling for gradients yet) — `M3-BRD-04`
   - [x] Shaped boxes: s&box's custom `border-shape: polygon() | circle()` (SDF, hit testing follows
     the shape), done the standard way with `clip-path` — `inset()`, `circle()`, `ellipse()` (M3c),
     now `polygon()` and `path()` (SVG path data, curves and arcs flattened; `nonzero`/`evenodd`).
     Polygon points ride in the clip table; both shaders compute a signed distance, and hit
     testing uses the same fill rule — `M3-CLIP-01`. `url()` needs SVG `<clipPath>` elements,
     which Platypus doesn't have
4. **Text**
   - [x] `text-transform` (inherited; applied to run text before shaping, `capitalize` across
     inline runs), `text-overflow: ellipsis` (not inherited; lines wider than the content box of
     a block whose x overflow clips drop whole glyphs and end in a separately shaped `…`, on the
     left for rtl) — `M3-TXT-10`
   - [x] `word-break` (`break-word` as `overflow-wrap: anywhere`), `overflow-wrap` / `word-wrap`;
     `white-space: pre-line | break-spaces` — all Parley options — `M3-TXT-11`
   - [x] `font` shorthand; `font-size` keywords (`xx-small` ... `xxx-large`, `smaller`, `larger`);
     `font-weight` keywords (`thin` ... `black`, `bolder`/`lighter` from the parent);
     `font-variant-numeric` and `font-feature-settings` as Parley font features — `M3-TXT-12`
   - [x] Decoration: `overline`, `text-decoration-style` (`double`, `dotted`, `dashed`, `wavy`),
     `-color`, `-thickness`, `text-underline-offset`, and the full `text-decoration` shorthand.
     Runs carry their decoration in Parley's brush; Platypus draws the lines from font metrics (dots
     and dashes through the styled-border shader, waves through their own kind) — `M3-TXT-13`.
     `text-decoration-skip-ink` is parsed but underlines don't skip descenders yet
   - [x] Outlined text: `-webkit-text-stroke` / `text-stroke` and their width/colour longhands
     (inherited). Glyph runs carry the stroke; the atlas rasterizes stroked outlines with swash
     (keyed by stroke width) and draws them over the fill. Colour glyphs aren't stroked — `M3-TXT-14`
   - [x] Gradient text: `background-clip: text` (M3d step 3b). s&box's gradient `color` isn't
     added: the standard form covers it
   - [x] `caret-color` (inherited; `auto` is the text colour); `font-smooth` (s&box's `auto` /
     `never` / `always`) and `-webkit-font-smoothing`: `never` / `none` thresholds glyph coverage
     in the shader for crisp pixel fonts — `M3-TXT-15`
5. **Effects**
   - [x] `filter` / `backdrop-filter` functions beyond `blur()`: `brightness()`, `contrast()`,
     `saturate()`, `grayscale()`, `sepia()`, `hue-rotate()`, `invert()`, `opacity()`, and
     `drop-shadow()` (follows the shape); chains. Colour functions fold into one 4×5 matrix
     applied in sRGB when the layer (or backdrop copy) is composited; drop shadows are blurred,
     tinted, shifted copies of the layer drawn beneath it. Platypus applies blur, then colour, then
     shadows, whatever the written order — `M3-FX-02`
   - [x] `image-rendering` (`pixelated`, `crisp-edges`, `auto`/`smooth`/`high-quality`), inherited,
     for background images, `Image` elements and border images: each image has a second bind
     group with a nearest-neighbour sampler — `M3-FX-03`
   - [x] Masks: `mask-image` (images and gradients; `mask` takes just the images), `mask-mode`
     (`alpha`/`match-source`, `luminance`; the top layer's mode applies to all), `mask-size`,
     `mask-position`, `mask-repeat`, with `-webkit-` aliases. A masked element draws in a layer;
     `BeginMask` switches drawing to a mask texture (ordinary background quads), which the
     composite multiplies by — `M3-FX-04`. Along the way, gradients now follow
     `background-size`/`-position`/`-repeat` (one tile, repeated in the shader) — `M3-BG-04`
   - [x] `opacity` as a percentage (and as a numeric string) — `M3-FX-01`
   - [x] **5e** Transforms (moved up from group 6): `transform` with every function — `matrix`,
     `matrix3d`, `perspective`, `rotate`/`rotateX`/`rotateY`/`rotateZ`/`rotate3d`, `translate`/
     `translateX`/`Y`/`Z`/`translate3d`, `scale`/`scaleX`/`Y`/`Z`/`scale3d`, `skew`/`skewX`/`skewY`, `none`,
     several functions, and the CSS-wide keywords; the `translate` / `rotate` / `scale` properties;
     `transform-origin` (with z); `perspective` / `perspective-origin`; `transform-style: flat |
     preserve-3d`; `backface-visibility`. Drawing and hit testing through them. Absolute length units
     (`in`, `cm`, `mm`, `Q`, `pt`, `pc`) everywhere. Each element's 4×4 transform flattens to a 3×3
     homography (`transform.rs`); `PushTransform`/`PopTransform` in the draw list, transforms stored
     in the renderer's clip table, instances carry a transform index (the vertex shader maps corners,
     fragments map back to the plane), clips remember their plane; hit testing inverts — `M3-XF-01`,
     `M3-XF-02`
   - [x] **5f** `visibility: visible | hidden | collapse` (keeps its layout space, isn't drawn or hit;
     children can opt back in with `visible`; inherited; text runs carry it, so visible spans in
     hidden text still draw; hidden elements can't take focus) — `M3-VIS-01`
   - [x] **5g** Lists: a `list` element (React `<List>`; UA style: column, 24px left padding) whose
     element children are list items (also `display: list-item`); `list-style-type` (`disc`,
     `circle`, `square`, `decimal`, `decimal-leading-zero`, `lower/upper-alpha`, `lower/upper-roman`,
     `lower-greek`, `none`, a string), `list-style-position`, `list-style-image`, `list-style`
     (all inherited); markers are generated `::marker` children (outside: absolutely placed left of
     the item; inside: first child), styled by `::marker` rules, renumbered when items change.
     A list without markers (`list-style: none`, no image) drops its default indent unless its start
     padding is set. Paragraphs and headings stay `text` with classes (no `p`/`h1`). Counters aren't done —
     `M3-LST-01`
   - [x] **5h** `user-select: none | auto | text | all` (and `-webkit-`; `auto` takes the parent's
     value, so children can opt back in) and selecting static text: drag, double-click word,
     triple-click line, Shift+click extends, Ctrl+C copies through the fields' clipboard request.
     The tree keeps a Parley `Selection` on one text block (presses map through transforms, which
     fields now do too); `::selection` sets the highlight and recolours the selected glyphs (also in
     fields); `::placeholder` styles placeholders (text properties, colour, opacity); the I-beam
     shows only over selectable text — `M3-SEL-01`, `M3-SEL-02`. Selections stay in one block (see
     docs/differences.md)
   - [x] **5i** Custom cursor images: `cursor: url(a.cur), url(b.png) 4 4, pointer` (any number of
     images, then the required keyword; hotspot in image pixels, else the `.cur` file's own, else the
     top-left). The app decodes the first image that loads (PNG, JPEG, `.cur`; over 128px is skipped,
     as in browsers), scales it for the window's DPI and asks the platform, which creates and caches
     winit `CustomCursor`s (`PlatformApp::take_custom_cursor`). Fields show `text`, or `not-allowed`
     when disabled; every CSS keyword is checked to map to a winit cursor (winit picks the nearest
     per platform) — `M3-CUR-01`. SVG's extra `pointer-events` values (`visiblePainted`, `fill`,
     `stroke`, `bounding-box`, ...) come with 5.1
   - [x] **5j** A `dialog` element (React `<Dialog open modal closedBy onCancel onClose>`): closed
     dialogs are `display: none` (the authored value comes back on open); open ones are centred
     (`left/top: 50%`, `translate: -50% -50%`). Modal ones are fixed and go in a top layer painted
     and hit above everything, `zIndex` and fixed elements included, each over a generated
     `::backdrop` box covering the window (default `rgba(0,0,0,0.1)`; `dialog::backdrop` rules,
     `backdrop-filter` too). Escape (and backdrop clicks with `closedBy="any"`) send `cancel`
     (cancelable) then `close`; scripts close by clearing `open`. A modal takes focus (its first
     focusable element, else itself), keeps Tab and the d-pad inside, and gives focus back when it
     closes; `[open]` / `[modal]` match — `M3-DLG-01`, `M3-DLG-02`. No `popover` yet
   - [x] **5.1** SVG: an `svg` element (React `<Svg>` and children) with the core shapes (`rect`,
     `circle`, `ellipse`, `line`, `polyline`, `polygon`, `path`, `g`, `text` basics), `viewBox` /
     `preserveAspectRatio`, `use`/`defs`, gradients (`linearGradient`, `radialGradient`), `clipPath`
     and `mask` (which also unlocks `clip-path: url(#id)`), and loading `.svg` files as images
     (`url(icon.svg)`, `Image src`). CSS styling: presentation attributes and properties (`fill`,
     `stroke`, `stroke-width`, `stroke-linecap`/`-linejoin`/`-dasharray`/`-dashoffset`,
     `fill-rule`, `opacity`/`fill-opacity`/`stroke-opacity`, `transform` on shapes,
     `currentColor`), selectors matching SVG elements, inheritance, transitions later. Rendering:
     tessellate fills and strokes (e.g. lyon) into triangles, or rasterize with a vector rasterizer
     into a texture (e.g. resvg/tiny-skia for files) — decided: resvg, rasterized at the drawn size
     - [x] **5.1a** `.svg` files and `data:image/svg+xml` URLs as images (`url()`, `Image src`): usvg
       parses (with the bundled font for `<text>`), resvg rasterizes at the tile's physical size
       (rounded up to 8px, cached per size), layout uses the SVG's natural size — `M3-SVG-01`
     - [x] **5.1b** Inline `<svg>`: `svg` is a measured box (width/height/viewBox, like an image);
       the SVG elements inside are tree nodes React creates (`g`, shapes, `text`/`tspan`, `defs`,
       `use`, gradients/`stop`, `clipPath`, `mask`, `pattern`, `symbol`) with any attributes. CSS
       styles them: SVG presentation properties (`fill`, `stroke*`, `stop-color`, geometry like `r`)
       are stored as written; `opacity`, `transform` (converted to SVG syntax), `clip-path:
       url(#id)`, `mask: url(#id)` and font properties are kept too. At draw time the tree
       serializes the subtree with those values (`currentColor` from the `svg`'s colour) into a
       `data:` SVG, which the renderer rasterizes at the drawn size (stale rasters are evicted). So
       selectors, `:hover`, transitions and animations work on SVG content — `M3-SVG-02`, `M3-SVG-03`.
       Hit testing is per `svg` box; GPU paths, SMIL and path morphing are under "Later"
6. **Transforms**
   - [x] Done as step 5e (2D and 3D); see group 5
7. **Transitions and animations**
   - [x] **7a** `transition` (+ `-property`, `-duration`, `-delay`, `-timing-function`, `-behavior`)
     for numbers, lengths, colours, shadows, filters and transforms; discrete properties snap.
     `motion.rs` interpolates computed values as CSS text, token by token (mixed units through
     `calc()`, colours premultiplied, `none` as the neutral list); restyle compares each property's
     winning value, starts transitions from the current value (so reversals don't jump) and applies
     running ones on top; `set_time` restyles moving elements each frame; inline-only games switch to
     restyle once a `transition` is set. `transitionrun`/`start`/`end`/`cancel` events reach React
     (`onTransitionEnd`, ...) — `M3-ANI-01`, `M3-ANI-02`
   - [x] **7b** `@keyframes` (`from`/`to`/percentages, selector lists, per-frame
     `animation-timing-function`, `!important` ignored) and `animation` (+ name, duration, delay,
     iteration count, direction, fill mode, play state, timing function). Keyframe values resolve
     `var()` and units per element; missing 0%/100% frames use the element's own value; discrete
     values flip halfway; animations apply under transitions. `animationstart`/`iteration`/`end`/
     `cancel` events reach React (`onAnimationEnd`, ...) — `M3-ANI-03`
   - [x] Timing functions: `linear`, `ease*`, `cubic-bezier()`, `steps()` (all jump positions),
     `step-start`/`-end` (done with 7a; `linear()` with stops isn't)
   - [x] **7c** Entry styling (s&box's `:intro`): `@starting-style` rules (their own rule index) are
     the before-change style of new elements and of elements whose `display` goes from `none` to
     shown (opening a dialog restyles it), so transitions run from them; elements already styled
     ignore them — `M3-ANI-04`
   - [x] **7d** Exit styling, s&box's `:outro`: when any rule uses `:outro`, an element React removes
     stays in the tree with `:outro` set (no hits, focus or Tab stops) until the transitions and
     animations its exit started finish, then goes; ongoing animations (a spinner) don't hold it.
     Done in the tree, so the reconciler needs no changes — `M3-ANI-05`
8. **Scrolling** (extends M3c overflow)
   - [x] `scrollbar-width`, `scrollbar-color` and `overflow: clip` (already done in M3c);
     `scrollbar-gutter: stable` (Taffy keeps the bar's width and margins at the end of a box that
     clips or scrolls vertically; Platypus's bars otherwise overlay), `overscroll-behavior` (+ `-x`/`-y`/
     `-inline`/`-block`; `contain` and `none` stop wheel and key scrolling from chaining outward;
     there's no bounce to turn off) — `M3-SCR-01`
9. **Game-specific extensions** (s&box custom properties worth copying)
   - [~] **Skipped:** s&box's `sound-in` / `sound-out` (play a named sound when a rule starts or
     stops applying). CSS is for UI display, not sound, so this stays an s&box-specific feature;
     Platypus games play UI sounds from scripts (`onPointerEnter`, `onClick`) with M4 audio
   - [x] **9a** Tint for images: s&box's `background-image-tint` (background images and an `image`
     element's picture) and `border-image-tint`, a colour (alpha too) multiplied into the image's
     pixels in the image shader; gradients aren't tinted — `M3-GAME-01`
   - [x] **9b** UI scale: `setUiScale({ height: 1080 })` (s&box's ScreenPanel), `{ width, height }`
     (fit a design), a fixed number, or `null`, from `@platypus/engine` (`uiScale` op). The app lays
     the tree out in UI pixels and multiplies the factor (clamped to 0.25–8) into rendering, pointer
     and wheel input and the IME area, so text stays sharp — `M3-GAME-02`
10. **Stylesheet authoring**
    - [x] **10a** Native CSS nesting (`&`, nested rules, implicit descendant, `> + ~` relative
      rules, selector lists as `:is()`): nested rules are flattened at parse time after their
      parent; `text:hover { ... }` is told apart from a declaration by its block; nested at-rules
      are skipped with a warning — `M3-CSS-11`
    - [x] **10b** `@import url(x.css)` / `@import "x.css"` for runtime stylesheets (Bun already inlines
      them when bundling): `UiTree::add_stylesheet_with_imports` loads them through the app (game
      folder only), relative to the importing sheet, inlined before its rules (fonts, keyframes and
      `@starting-style` too) with their URLs rebased; cycles, deep chains, missing files and
      conditional imports (media, `supports()`, `layer`) are skipped with warnings — `M3-CSS-12`
    - [ ] SCSS (`$variables`, `@mixin`/`@include`) is **not planned** in the engine: custom properties
      and nesting cover most uses, and a Bun plugin can compile `.scss` for those who want it

11. **Pointer capture**
    - [x] `ref.setPointerCapture(pointerId)` / `releasePointerCapture` / `hasPointerCapture`: while
      captured (only while a button is held, as in browsers), the capturing element is the hit
      target, so moves, `:hover` and the release (and its click) go to it wherever the pointer is;
      `gotpointercapture` / `lostpointercapture` events (`onGotPointerCapture`, ...); capture ends
      when the buttons are released, on `releasePointerCapture`, when the element is removed or
      hidden, or when the window loses focus; `hasPointerCapture` follows the events. One pointer,
      so `pointerId` is ignored — `M3-EVT-06`
    - [ ] `touch-action` isn't needed until touch input exists; drag and drop is in M5.e
12. **Web fonts: WOFF and WOFF2**
    - [x] `.woff` and `.woff2` fonts from `@font-face` and `loadFont`: `register_font` recognizes
      their signatures and decompresses them to TTF/OTF with the pure-Rust `wuff` crate before
      Parley sees them; `@font-face` `src` lists take the first entry Platypus can load (by
      `format()`, else extension), skipping `local()`, EOT and SVG fonts — `M3-FNT-03`
13. **`@media` queries**
    - [x] `media.rs`: `width`/`height` (with `min-`/`max-` and range syntax like
      `400px <= width < 800px`; px, em/rem as 16px), `orientation`, `aspect-ratio`, `resolution`
      (dppx/dpi/x: the window's DPI times the UI scale), `prefers-color-scheme` (the window's
      theme), `prefers-reduced-motion`, `pointer`/`any-pointer`, `hover`/`any-hover`; `and`, `or`,
      `not`, `only`, media types (`screen`/`all` match, `print` doesn't), comma lists; unknown
      features never match. `@media` blocks at the top level, nested in rules and on `@import`
      give rules conditions the cascade checks; the tree keeps a `MediaEnv` (size from
      `set_size`, the rest from `set_media_env`) and restyles when any condition's result flips
      — `M3-MQ-01`, `M3-MQ-02`

Accessibility and drag and drop moved to M5.e, as UI requirements before the editor starts.

#### Floats and tables (2026-10-27)

- [x] `float: left | right` and `clear` on blocks (Taffy's `float_layout`): floated boxes sit beside
  the line flow, several stack and wrap down, `clear` moves a block below them, margins count. Text
  blocks lay out as block items of their container, and their lines break band by band around the
  floats (Parley `BreakLines`: a line's room is what every float its height touches leaves; a word
  wider than the room moves below the float). An image floated inside a block of bare text is the
  "image in a paragraph" case: no richtext element is needed — `M3-FLT-01`..`13`, golden `M3-FLT-20`
- [ ] A float inside a `text` element (a floated child between runs of one paragraph): needs in-flow
  inline boxes in text (Parley `InlineBox`); only block-level floats before the text exist
- [x] Table: `examples/table` builds one from a grid of `display: contents` rows (content-sized columns,
  header, striping, hover, wrapping, right-aligned numbers, `grid-column` / `grid-row` spans); it works
  well, so no table elements — `M3-TBL-01`..`05`. Not covered (revisit if a game needs them):
  `display: table*` on arbitrary elements, `border-collapse`, `<col>` widths, `position: sticky` headers,
  a rect for the row itself

#### Unity compatibility: raycasts and transforms (2026-10-27)

- [x] Raycasts like Unity (Angry Bots needed them): layer masks, the hit's GameObject, a ray that
  starts inside a collider skips it, unlimited distance by default, `Physics.raycastAll`,
  `Physics.overlapSphere`; `GameObject` tags (`M9-API-214`)
- [x] Transform hierarchy like Unity: `children`, `root`, `find` (`..`, `.`, leading `/`),
  sibling order, `rotateAround`, direction/vector transforms, `localToWorldMatrix` /
  `worldToLocalMatrix`, `setParent` keeping world scale, nullable `parent`;
  `getComponent(s)InChildren/InParent` (`M9-API-213`, `M9-TS-707`)
- [ ] `transform.parent = x` assignment; `overlapBox` / `overlapCapsule` / `sphereCast`; the dirty-collider
  fallback of `raycastAll` / `overlapSphere` is untested
- [ ] Angry Bots (readiness): rebuild and check it after the tags and nullable `parent` changes

## M4 — Hot reload, physics and audio

Physics and audio come ahead of the scene work (M5), so its object model is designed with them in
mind. Hot reload is split (2026-10-17): the plumbing is here; keeping state across a reload waits
for GameObjects and scene serialization (M5.a/b), which it reuses — see "Hot reload, part 2" in M5.

- [x] Bun watch → rebuild → engine notified: `bun run dev` in an example (`bun build --watch`);
  the app watches `build/game.js` and `game.css` with `notify` and reloads once they settle —
  `M4-HOT-10`
- [x] Reload plumbing: the new bundle loads into a fresh runtime and is swapped in only if it
  loads and its components spawn; otherwise the old code keeps running with the error logged.
  The UI tree, script stylesheets and `game.css`, physics and audio are reset, and components
  start over (`onStart` runs again) — `M4-HOT-01`, `M4-HOT-04`, `M4-HOT-09`
- [ ] Edit a TS file while the game runs; the change appears without restarting (manual, needs
  the window) — `M4-HOT-08`
- [ ] `rapier` integration + TS components — `M4-PHY-*`
  - [x] `platypus_physics` crate: rapier world driven by `physics.*` ops with script-chosen IDs;
    bodies (dynamic, fixed, kinematic), colliders (box, ball, capsule, cylinder, convex, mesh;
    sensors), forces, character controller, raycasts; one flat batch back per tick —
    `M4-PHY-01`..`M4-PHY-03`, `M4-PHY-05`, `M4-PHY-06`
  - [x] TS API in `@platypus/engine` (`physics`: `RigidBody`, `Collider`, collision handlers,
    `raycast` promises, `makeCharacter`/`move`), stepped in the app's fixed tick with results
    sent back through `__platypus_physics`; `examples/physics` — `M4-PHY-02`
  - [ ] Bodies survive script hot reload (moved to hot reload part 2, M5) — `M4-PHY-04`
  - [x] `Rigidbody` / `Collider` components on GameObjects (M5.a2)
- [ ] `kira` integration (sounds, music, buses) + TS API — `M4-AUD-*`
  - [x] `platypus_audio` crate: kira mixer driven by `audio.*` ops with script-chosen IDs; decoded
    and streamed sounds, instances (volume, rate, pan, loop, fades, seek, pause/stop), buses,
    spatial emitters around one listener, ended instances polled; an offline backend for tests
    and deviceless runs — `M4-AUD-01`..`M4-AUD-04`
  - [x] TS API in `@platypus/engine` (`audio`: `Sound`, `Playback` with `onEnded`, `Bus`, `Emitter`,
    `setListener`, `audio.play(src)`); the app opens the mixer on the first `audio.*` op (offline
    for `--screenshot` runs or without a device), loads files from the game folder and reports
    finished sounds through `__platypus_audio`; `examples/physics` knocks on impacts
    (`CollisionEvent.speed`)

## M5 — Scene, assets, rendering, editor

- [ ] **M5.a** GameObject + Component core (the s&box way), from TS components, for 2D and 3D
  objects. The first plane use is UI in 3D space (a menu drawn on a plane in the world).
  **Decided (2026-10-17):** our own wgpu renderer (DESIGN §7.4); a `platypus_scene` crate with
  GameObjects in a generational arena, no ECS underneath (§7.5).
  API style (2026-10-17): Unity / s&box naming and units: `Vector2` (2D), `Vector3`
  (`forward`, `up`, `right`, ...), `Quaternion` with Euler angles in degrees,
  `transform.forward` / `eulerAngles` / `lookAt`, components on GameObjects.
  - [x] `platypus_scene`: GameObjects (generational ids, names, enabled, parent/children), local and
    world transforms (glam `Vec3`/`Quat`/scale), moved-object tracking — `M5-OBJ-01`..`M5-OBJ-03`
  - [x] TS API: `Vector2`, `Vector3`, `Quaternion`, `Transform`, `GameObject`; script components
    attached to GameObjects (`addComponent`, `getComponent`, `this.gameObject`,
    `this.transform`); engine-side transform changes synced back in one batch per tick
    (`scene.*` ops, `ScriptScene`, `__platypus_scene`); the starting components each get a
    GameObject; inactive objects' components don't update — `M5-OBJ-04`..`M5-OBJ-07`
  - [x] Unity / C# value types: `Vector2Int`, `Vector3Int`, `Vector4`, `Rect`, `RectInt`,
    `Bounds`, `Ray`, `Plane`, `Color`, `Mathf` — `M5-MTH-01`..`M5-MTH-03`
  - [x] C# `DateTime` / `TimeSpan` on JS `Date` (millisecond resolution, C# formats) —
    `M5-TIM-01`, `M5-TIM-02`
  - [x] Everyday helpers: `Debug` / `Log` / `Console` logging, `Time` (fed by `__platypus_tick`,
    with `timeScale`), `TimeSince` / `TimeUntil`, `Task.delay` / `frame`, `Random` and a seedable
    `Rng`, Unity-style input (`getKey`, `getKeyDown`, `getAxis`, `getButton`) —
    `M5-HLP-01`..`M5-HLP-04`
  - [x] 3D view basics: `Camera` component, a perspective pass with depth in `platypus_render`
    (`WorldRenderer`: instanced quads, back-to-front, textured or coloured), `PlaneRenderer`
    (a plane in the object's XZ plane), screen UI blended over the world
    (`UiRenderer::render_overlay`); `examples/world` — `M5-RND-05`
  - [x] Camera system: draw order by `depth`, perspective and orthographic (2D) projections,
    viewport rects (split screen, minimaps), `Camera.main` / `allCameras`, `Matrix4x4`,
    `Screen`, screen/viewport/world conversions (`screenPointToRay`, `worldToScreenPoint`,
    `viewportToWorldPoint`, ...), `CameraFollow` and `CameraOrbit` — `M5-CAM-01`, `M5-RND-06`
  - [x] Cameras rendering to a texture (`targetTexture`): done in M5.c (render textures)
  - [x] Lifecycle hooks the Unity / s&box way: `onStart`, `onUpdate` and `onLateUpdate` per
    frame, `onFixedUpdate` on the 60 Hz tick, `onPreRender`, `onDestroy`; the engine only
    calls the hooks a component has — `M5-OBJ-08`
  - [x] Planes: a `WorldPanel` component (s&box's name) draws React UI on a plane in the world:
    `createRoot(panel)` commits to the panel's own UI tree (`__platypus_commit(ops, target)`), drawn
    into a texture each frame; the pointer is raycast from the camera under it into the
    nearest panel (screen UI wins where it has elements), drags stay with the panel, keys go
    to the surface clicked last; stylesheets and fonts reach every tree — `M5-PNL-01`..`M5-PNL-03`
- [x] **M5.a2** Unity / s&box-style components (its own step, before the rest of M5); done
  2026-10-17.
  - [x] `Rigidbody` component (mass, drag, gravity, kinematic, velocity, angular velocity,
    `addForce` with `ForceMode` Force / Acceleration / Impulse / VelocityChange, at a point,
    explosions, constraints, continuous detection) and collider components (`BoxCollider`,
    `SphereCollider`, `CapsuleCollider`, `MeshCollider`, triggers, `PhysicsMaterial`) driving
    GameObject transforms both ways (`physics.attach`); `onCollisionEnter/Exit` and
    `onTriggerEnter/Exit` on the object's components; `onAwake` for components; the low-level
    handles are now `PhysicsBody` / `PhysicsCollider` — `M5-RB-01`, `M5-RB-02`
  - [x] Collider sizes follow scale changes: setting `localScale` (or reparenting) tells
    `scaleListeners`, and colliders on the object and its children whose world scale changed
    rebuild — `M5-RB-04`
  - [x] `CharacterController` component (Unity's: a `Collider` with `move` returning
    `CollisionFlags`, `simpleMove` with gravity, `isGrounded`, `velocity`, slope limit, step
    offset) on rapier's character controller; character moves wait a step after colliders
    are added (refreshing queries mid-step tripped rapier's islands) — `M5-RB-03`
  - [x] Joints as components: `FixedJoint`, `HingeJoint` (axis, limits, motor),
    `SpringJoint`, `CharacterJoint` (ball), `RopeJoint` (max distance), with `connectedBody`
    (or the world), anchors and auto-configured connected anchors; `platypus_physics` `joint` /
    `removeJoint` ops; a `Rope` component builds a visible, colliding chain of capsule
    segments on ball joints, holding an optional end body — `M5-JNT-01`, `M5-JNT-02`
  - [x] Rope rendering as one smooth tube: `LineRenderer` (a round tube through world-space
    points, Catmull-Rom smoothed, parallel-transported rings, rebuilt when the points change;
    unlike Unity's flat ribbon) draws `Rope`s by default (`renderMode: "segments"` keeps the
    capsules) — `M5-LIN-01`
  - [x] Thick 2D lines without a mesh: `LineRenderer2D` (points on its object's XY plane, a
    width in world units, a colour, `loop`) sends one `render.stroke` op per change; Rust
    draws one instanced SDF capsule per segment (`WorldView.strokes`, `vs_stroke` /
    `fs_stroke`): smooth edges without MSAA, round joins and caps — `M5-RND-24`
    - [x] Fills (`FillPolygon` / `FillClosedCurve`): `LineRenderer2D.fillColor` and `fillRule`
      (`evenodd`, GDI's default `Alternate`, or `nonzero`), tessellated with lyon when the
      points change (`tessellate_fill`, `WorldView.fills`), drawn under the outline; a
      `width` of 0 leaves just the fill — `M5-RND-25`
    - [x] `Shapes` (GDI's outlines): rectangle, rounded rectangle, ellipse, circle, arc, pie,
      regular polygon, polygon, triangle, Bézier, cardinal `curve` / `closedCurve` — `M5-RND-25`
    - [x] `TextMesh` (GDI's `DrawString`, Unity's `TextMesh`): text on its object's XY plane, laid
      out by a one-element UI tree that `render.text` builds in Rust (any `loadFont` font, line
      breaks, `maxWidth` wrapping, alignment), sized to fit and tinted like a sprite;
      `examples/shapes` shows every shape, both fill rules and labels — `M5-TXT-01`
      - [x] A text's texture is redrawn only when its text or look changes; a new colour is just
        the quad's tint (other panels still draw every frame) — `M5-TXT-01`
    - [x] Draw order: each shape's fill, then its outline, by `sortingOrder` and then the order
      they were made (`WorldView.shapes`); 2D shapes draw before the see-through things, so
      sprites and text show over them — `M5-RND-25`
    - [x] `Spline` (Unity's Splines, the core): knots with auto (cardinal; 0.5 is
      Catmull-Rom), Bézier or linear tangents, open or closed, positions by distance
      (`evaluate`), tangents, length, `sample`, `nearest`, editing; `SplineAnimate` moves an
      object along one (loop, ping-pong, once; facing along it; in a container's space) —
      `M5-SPL-01`. `examples/splines`: splines that move and one to edit with the mouse
    - [x] `examples/clock`: twelve faces built on these (analog rings and spline hands, turning
      dials, a glowing rolling meter, a 3D sundial whose shadow tells the time under a sun
      placed for the date, a sand timer, a reactive lava lamp, an underwater porthole clock, a
      chandelier, a rainbow, Material-style morphing digits, a flip clock in Platypus's UI, a
      seven-segment alarm clock with bloom), switched with side arrows and dots
    - [x] `examples/fishtank`: an aquarium-channel view into deep water: a slowly drifting
      perspective camera, a hazy back wall, rocks and kelp at several depths, a sand floor
      fading into the blue, light shafts, caustics, marine snow, a vignette; four species
      school in 3D, swim level, keep inside and flee the pointer; click to feed (the nearest fish
      race for the food and grow), right-click adds a fish, Space taps the glass; the readout
      shows only while the mouse moves
    - [ ] Later: pen options (flat / square caps, miter / bevel joins, dashes), gradient and
      texture brushes, smooth fill edges without MSAA, clipping
    - [ ] Later: see-through strokes without darker overlaps at the joins (a layer)
    - [ ] Later: points as one `Float32Array` if the op's number conversion shows up
  - [x] The Cornell box (`examples/cornell`): built from primitives with the original's
    reflectances; lit by an emissive panel, a point light, a soft-shadowed light from above
    and bounce lights (soft point lights tinted by the walls and floor, faking GI's colour
    bleeding); the room's shell doesn't cast (`MeshRenderer.shadowCastingMode`, new:
    `on` / `off`, per-mesh in the shadow pass)
  - [x] Meshes, lights, textures on meshes, glTF models, the missing-texture checker,
    classic test models and shadows:
    - [x] Renderer: `platypus_render::world::mesh` builds Unity's primitives (cube, sphere, capsule,
      cylinder, plane, quad; Unity sizes) and fills in missing normals; `WorldRenderer` has a
      lit mesh pass (instanced by mesh, `set_mesh` / `remove_mesh` for script meshes,
      primitives at `primitive_id`), `MeshInstance` (colour, emission, smoothness, unlit,
      texture), `Light` / `LightKind` (directional, point with range, spot with soft cone),
      per-view `ambient`; opaque meshes first, then see-through meshes and quads back to
      front — `M5-MSH-01`, `M5-RND-07`
    - [x] Script side: `render.mesh` / `render.meshRenderer` / `render.light` /
      `render.ambient` ops (`crates/platypus_app/src/world_view.rs`; script meshes uploaded before
      drawing); TS `MeshRenderer` (primitive name or `Mesh`), `Material` (color, emission,
      smoothness, unlit), `Mesh.create`, `Light` (directional / point / spot, color,
      intensity, range, spot angles), `RenderSettings.ambientLight`,
      `GameObject.createPrimitive` / `PrimitiveType` (mesh + matching collider);
      `examples/world` drops lit primitives with Rigidbodies — `M5-MSH-02`
    - [x] Docs refresh for all of M5.a2 (2026-10-17): `docs/world-rendering.md`,
      `physics-and-audio.md`, `scene-and-objects.md`, `input-and-events.md`, new
      `assets-and-models.md` and `particles.md`
    - [x] Textures on meshes: `Material.mainTexture` / `mainTextureScale` /
      `mainTextureOffset` (`withTexture`), texture alpha on see-through meshes; `Mesh.create`
      takes Unity's bottom-up UVs — `M5-TEX-01`, `M5-RND-08`
    - [x] glTF models (`.glb` / `.gltf`, `platypus_app/src/models.rs`): meshes (one engine mesh per
      primitive, ids from 2^30), base colour textures with their samplers, materials (colour,
      emission and strength, roughness → smoothness, unlit, blend), node trees; buffers and
      images only from inside the game folder; TS `Model.load` / `whenLoaded` /
      `instantiate({ colliders: "mesh" | "convex" })`, `MeshCollider.sharedMesh` (resolved
      engine-side from the render mesh's data); `examples/world` places a house — `M5-GLB-01`,
      `M5-GLB-02`
    - [x] Missing textures (never set, failed to load, a model image that can't be read) draw
      a magenta / black checker (`MISSING_TEXTURE`), sprites too — `M5-RND-08`
    - [x] Classic test models: `examples/classics` shows the Stanford Bunny and the Newell
      teaset (teapot, teacup, teaspoon) as .glb (converted by `tools/make_models.py` from the
      downloaded originals, which aren't in the repo) on turntables, plus the material sweep
      (spheres from matte to glossy)
    - [x] Shadows: the first directional light with `Light.shadows` (`hard` / `soft`,
      `shadowStrength`) casts them from opaque meshes onto meshes: a depth map
      (`QualitySettings.shadowResolution`, default 2048) fitted to the main camera out to
      `QualitySettings.shadowDistance` (default 25 m), snapped to texels, with slope bias and
      a normal offset; soft is 3 × 3 PCF — `M5-RND-10`
  - [x] `AudioSource` (clip as a `Sound` or path, `playOnAwake`, volume, pitch, pan, mute,
    loop, `spatialBlend` 0 = flat / above 0 = from an emitter following its GameObject,
    min / max distance, `outputBus`, `play` / `stop` / `pause` / `unPause` / `isPlaying`,
    `playOneShot`, `playClipAtPoint`) and `AudioListener` (its object's pose, static
    `volume`) components, in TS over the existing `audio.*` ops; `examples/world`'s
    primitives thud in 3D — `M5-AUD-01`, `M5-AUD-02`
  - [x] Particles: a `ParticleSystem` component simulated in Rust (`platypus_app/src/particles.rs`):
    duration / loop / delay, ranges for lifetime, speed, size, rotation and spin, two-colour
    start, gravity, drag, local or world space, max particles, rate and bursts, point /
    sphere / hemisphere / cone / box shapes (along the object's up), size and colour over
    lifetime, texture sheets, alpha or additive blending, `play` / `stop` / `pause` /
    `clear` / `emit`, `particleCount` / `isPlaying` / `onStopped` / `stopAction` through
    `__platypus_particles`; drawn as camera-facing quads (a built-in soft spot texture,
    additive quads without depth writes, straight-alpha textures premultiplied) —
    `M5-PRT-01`, `M5-PRT-02`
  - [x] Glows and spotlights (2026-10-17, asked for with particles):
    - [x] Light halos: `Light.haloSize` draws an additive glow in the light's hue, pulled
      towards the camera so nearby surfaces don't slice it
    - [x] Bloom: with `RenderSettings.bloom` on, the world draws into an `Rgba16Float`
      texture, a soft-knee threshold and dual-filter down / tent up chain (scatter mixes the
      levels) blurs the bright parts, and a composite adds them back — `M5-RND-09`
    - [x] Visible spotlight beams: `Light.beamIntensity` draws an open cone (`BEAM_MESH`)
      as an additive `MeshGlow::Beam` mesh, soft at its edges and fading along the range;
      additive meshes (`MeshGlow::Additive`) render without depth writes from both sides
  - [x] Textures and sprites: `platypus_render` keeps textures (`set_texture`: sRGB, point /
    bilinear / trilinear, repeat / clamp / mirror, CPU mipmaps averaged in linear light);
    `WorldTexture::Frame` (panels) or `Asset`; `render.texture` / `textureRemove` ops
    (PNG/JPEG from the game folder or pixels; sizes back through `__platypus_textures`); TS
    `Texture.load` / `Texture.create` / `whenLoaded`, `Sprite.create` / `Sprite.grid`
    (Unity's bottom-left rects and pivots, pixels per unit) and `SpriteRenderer` (colour,
    flips; unlit quads); `examples/world` has animated coin sprites — `M5-TEX-01`,
    `M5-TEX-02`, `M5-RND-08`
  - [x] 2D: `Rigidbody2D` (body type, mass, drag, gravity scale, freeze rotation, Vector2
    velocity, degrees, `addForce` / `addTorque` with `ForceMode2D`, `movePosition`) and
    `BoxCollider2D` / `CircleCollider2D` / `CapsuleCollider2D` / `PolygonCollider2D` /
    `EdgeCollider2D` (offset), `onCollisionEnter2D` / `onTriggerEnter2D` and friends,
    `Physics2D.raycast` / `gravity`, in the same rapier world as 3D (planar bodies, colliders
    extruded `Physics2D.depth` deep so z doesn't matter); the physics batch now carries
    angular velocity (`Rigidbody.angularVelocity` reads it too); sprites get
    `sortingOrder`, see-through things sort by order then depth (along the view for
    orthographic cameras), quads depth-test `LessEqual`; `examples/platformer` — `M5-RB2D-01`,
    `M5-RB2D-02`
  - [x] `Cursor` API: `visible`, `lockState` (`CursorLockMode`: locked falls back to
    confined + hidden where the OS can't pin it, as on Windows; re-applied on focus),
    `setCursor(texture or path, hotspot)` through the CSS cursor code (UI cursors still win
    where set); game mouse input: `input.mousePosition` (UI pixels, top-left),
    `getMouseButton` / `Down` / `Up`, raw `mouseDelta` from device motion (keeps coming
    while locked), `mouseScrollDelta`, `getAxis("Mouse X" / "Mouse Y" / "Mouse
    ScrollWheel")`; mouse buttons are `MouseLeft` / `MouseRight` / `MouseMiddle` keys —
    `M5-CUR-01`
  - [x] `SystemInfo`: OS name, family and version, device type and name, CPU model and
    count, memory, architecture, language, touch; GPU name, vendor, API, kind and driver and
    the monitor's size, refresh rate and DPI once the window opens (`sysinfo`, `sys-locale`,
    wgpu's adapter info, winit's monitor); gamepads are `input.gamepads()` — `M5-APP-02`
  - [x] `Application`: name, `productName`, version, company, identifier and engine
    version from the game's `package.json` (`platypus` section; the window title and icon use
    them), `platform`, `systemLanguage`, `isMobilePlatform`, `quit()`; installed as
    `globalThis.__platypus_info` before the bundle runs (and on reload) — `M5-APP-01`, `M5-APP-02`
- [ ] **M5.a2 follow-ups** (after M5.a2 or with the milestone that needs them):
  - [ ] More classic test scenes, with downloads the user approves first: Suzanne, Sponza,
    the Damaged Helmet (PBR), the Stanford Dragon / Armadillo / Lucy
  - [ ] glTF follow-ups: normal / metallic-roughness / emissive / occlusion maps (with PBR,
    M5.d), skins and animations (M5.d), cameras and lights from the file, KTX2 / Draco /
    meshopt compression, texture transforms
  - [ ] Shadow follow-ups: cascades (1 / 2 / 4, `QualitySettings.shadowCascades`), spot
    and point light shadows (a cube of six maps) and alpha-cut casters done in M5.c
    (`M5-SHD-01`, `M5-RND-20`); left: shadows on quads / sprites / particles, transparent
    casters, contact shadows, per-light shadow resolution, skipping casters outside each
    map, cascade blending
  - [ ] Audio follow-ups: partial `spatialBlend` (kira's spatialization strength), Doppler,
    a source's playback `time`, `onDisable` pausing sources
  - [ ] Particle follow-ups: GPU simulation for big systems, collisions, velocity / force
    over lifetime, stretched billboards, sub-emitters, trails, soft particles (fade near
    surfaces), `AnimationCurve` / `Gradient` types for the over-lifetime keys
  - [ ] Glow follow-ups: tonemapping (`RenderSettings.tonemapping` none / neutral / aces)
    and `postExposure` done in M5.c (`M5-RND-18`); left: per-camera post-processing
    settings (volumes), `Material` blend modes for additive meshes in scripts,
    lens flares
  - [ ] Texture follow-ups: SVG textures, async decoding off the frame, sorting layers
    for sprites, sprite atlases, `Texture.getPixel` / `setPixels` after
    creation; render textures (a camera into a texture) moved to M5.c
  - [ ] 2D follow-ups: a real 2D physics world (rapier2d) if the slabs fall short,
    concave polygons (decomposition), 2D joints, `Physics2D.overlapCircle` and friends,
    tilemaps, sorting layers, pixel-perfect camera
  - [ ] App follow-ups:
    - [x] `Application.targetFrameRate` (2026-10-17): the `app.targetFrameRate` op caps
      frames with `ControlFlow::WaitUntil` while vsync is off (`lifecycle::frame_cap`);
      the value survives a full reload through `__platypus_info` — `M5-APP-08`, `M5-DSP-03`
    - [x] `Application.runInBackground` (2026-10-17, off by default, `platypus.runInBackground`
      in `package.json`): unfocused or minimised, the game pauses — no fixed ticks,
      updates or timers, the mixer fades out, the event loop sleeps (`PlatformApp::paused`
      / `idle`, which still polls hot reload) — and resumes without catch-up ticks
      (`FixedClock::restart`, script time skips the pause). Screenshot runs never pause —
      `M5-APP-06`, `M5-APP-07`, `M5-APP-09`
    - [x] Focus events (2026-10-17): `InputEvent::Focused` / `Occluded` (and Windows'
      0 × 0 minimise) → `__platypus_focus(focused, paused)`; `Application.isFocused`,
      `isPaused`, `onFocusChanged(fn)`; component hooks `onApplicationFocus(hasFocus)` and
      `onApplicationPause(paused)` on active objects — `M5-APP-05`, `M5-APP-07`, `M5-APP-10`
    - [ ] `persistentDataPath` (with M6 file IO), live monitor changes, battery / power state
- [x] **M5.b** Scene representation and serialization (objects, components, hierarchy,
  transforms) with skybox support (2026-10-17, `examples/scenes`)
  - [x] Scene files (`SceneData`, JSON `platypus.scene` v1): objects (name, enabled, layer,
    local transform, children), components by registered or engine name with their
    `@property` values (standard decorators; every engine component's settings are
    marked), references to objects and components inside the file, shared meshes
    (`Mesh.create` data) and textures (by file), materials, sprites, sounds, plain
    objects; model copies saved as their model (nodes rebuilt on load); the active
    scene's render settings (ambient, sky, bloom). Loading builds everything, sets the
    properties, then wakes components (`onAwake`) — `M5-SCN-01`, `M5-SCN-02`
  - [x] Scene API (`SceneManager`): `createScene`, `loadScene` (single or additive),
    `loadSceneAsync` (a file in the game folder via `asset.readText`), `unloadScene`,
    `setActiveScene`, `getSceneAt` / `getSceneByName` / `sceneCount`,
    `moveGameObjectToScene`, `mergeScenes`, loaded / unloaded / active-changed events;
    `Scene.getRootGameObjects` / `serialize`; `GameObject.scene` / `root`;
    `GameObject.dontDestroyOnLoad`; `GameObject.instantiate` (copies with references
    inside remapped) — `M5-SCN-03`, `M5-SCN-04`
  - [x] Start scene: `platypus.startScene` in `package.json`; `register(..., { spawn: false })`
    for components that only live in scenes
  - [ ] Later: writing scene files from the game (M6 file IO, then the editor, M5.e);
    stable asset GUIDs (`M5-SCN-02` in the old plan, now `M5-SCN-05`); prefabs with
    overrides; saving user children under model copies
- [ ] **Hot reload, part 2** (moved from M4, after M5.a/b): keep state across a reload by reusing
  scene serialization
  - [x] Wipe and rebuild through scene serialization (2026-10-17): the old code saves every
    scene (`__platypus_save_state`: objects, transforms, components, `@property` values and
    script components' plain fields, references remapped, plus quality level and clock);
    the new code rebuilds them (`__platypus_restore_state`) instead of starting components.
    Restored script components get `onReload` instead of `onAwake` / `onStart`; engine
    components are rebuilt like a scene load. Gone properties are dropped with a warning,
    new ones get their defaults; if a saved class isn't registered any more, the game
    starts over (with the start scene) and says what to register.
    `PLATYPUS_RELOAD_AT_FRAME` reloads during a screenshot run — `M4-HOT-02`, `M4-HOT-03`,
    `M4-HOT-05`
  - [ ] Still lost on reload: physics velocities and joints' live state, playing sounds
    (sources with `playOnAwake` start again), particles in flight, module-level variables,
    React state (Fast Refresh below); `M4-PHY-04`
  - [x] React Fast Refresh (2026-10-17): dev builds (`bun run dev` → `scripts/dev.ts`) make
    `vendor.js` (React, the reconciler, `react-refresh`, `@platypus/engine`, `@platypus/react`)
    and a CommonJS `game.js` marked `// @platypus-dev`; when only game.js changes the app
    reloads it in place: React keeps state (Fast Refresh), live components move onto
    their new classes (fields kept, new fields' defaults added), the top level's style
    sheets are swapped, and the world, physics and sounds stay. A changed vendor or a
    production build reloads the old way (state kept through scenes) — `M4-HOT-06`
  - [ ] In-place reload follow-ups: undo more of the top level (fonts, input and scene
    listeners, timers); statics on component classes start fresh; objects a top level
    creates are made again
  - [ ] Reload benchmark — `M4-HOT-07`: the bench exists (`cargo bench -p platypus_scene
    --bench reload`) and measures ~146 ms against the 100 ms target (1 MB bundle parse
    ~54 ms, save ~27 ms, restore ~58 ms). To get under it: parse the new bundle on a
    background thread while the old code keeps running (hides the parse), make
    `GameObject` construction cheaper in QuickJS (~10 µs each now), and a leaner state
    format than JSON
- [x] **Bug (found and fixed 2026-10-17): UI backdrop blur didn't blur the 3D world.**
  `render_overlay` drew UI into a transparent canvas over the swapchain image, which it can't
  read. Now, when the UI has a backdrop filter or blend mode (`needs_backdrop`), the world is
  drawn into a sampleable texture (`backdrop_target`) and the UI canvas starts as a copy of it
  (`render_over`); other frames are unchanged — `M5-RND-16`; the arcade's frosted HUD
- [x] **Ported examples (2026-10-18)**: `examples/sine-circle-demo` and `examples/raindrop-demo`
  (MiniGameEngine's SineDemo and RaindropDemo: the original's transitions, all four sine
  formulas drawn every degree with `LineRenderer2D`, the weather's ripples, the music, an FPS
  readout with a vsync toggle).
- [ ] **Optimisation fixes** (found 2026-10-17 with `examples/arcade`; not scheduled yet):
  - [x] Reflection probes redrew the shadow map six times: shadow maps are now drawn once
    a frame from the main view and shared by probe faces, mirrors and render textures
    (`WorldStats::shadow_maps` counts them, `M5-RND-20`). Not yet re-measured in the
    arcade.
  - [x] Mirrors that keep their texture: the reflection has its own binding now (with
    the material maps), so a mirrored surface keeps its texture and maps. The arcade
    still has its see-through mirror sheet (parked).
  - [ ] Backdrop blur costs a full-screen world copy plus blur passes each frame
    (10–25% of the arcade's uncapped frame rate): blur only the panels' areas at a lower
    resolution, and skip the copy when no panel overlaps the world.
  - [ ] Reload benchmark under 100 ms (`M4-HOT-07`, ~146 ms): see "Hot reload, part 2".
  - [x] Script overhead with many objects (found 2026-10-18 with `examples/fishtank`: 123
    fish ran at ~40 ms a frame in release, ~32 ms of it QuickJS; now ~16 ms):
    - [x] Transforms go to the engine in one `scene.locals` op per script turn
      (`flushTransforms`, `__platypus_flush` after every call into JS), not one op per setter
    - [x] Lifecycle hooks run in one `__platypus_run(method, ids)` call per hook, not one call
      per component (every `LineRenderer2D` has `onPreRender`)
    - [x] World setters and getters under an identity parent skip the parent maths, and
      the parent's world transform is one walk up, not three per level
    - [x] `Spline` evaluates from cached control points in plain numbers
    - [x] The fish tank flocks over a spatial grid in plain numbers
  - [x] QuickJS allocation: every `Vector3` / `Quaternion` op allocates. (Moot: QuickJS was removed 2026-10-28; compiled code has no such cost.)
  - [ ] 2D shapes: check draw calls and re-tessellation with hundreds of shapes, and batch
    them where it helps
  - [ ] Profile the rest of the frame (~6 ms engine and render with 123 fish, ~800 shapes)
  - [x] Skeletal animation in script (found 2026-10-21 with `examples/fog-white-room`: one
    65-bone mannequin cost ~20 ms a frame): `Pose` resolved every curve's path by name and
    searched the clip again for each rotation, every frame, and `clip.length` rescanned all
    curves. Clips' curves are now grouped per bone once, paths cached per root, lengths
    cached: 24.6 → 5.7 ms a frame for the room (~3.3 ms of it the figure)
  - [x] Skeletal animation natively: sample glTF clips and blend poses in Rust (the
    `Animator`'s state machine stays in script), so a crowd of animated characters costs
    little script time. Compiled games (platypus_ts) do this (2026-10-23: `Model.animations`
    built by `platypus_animation::gltf`, the `Animator` in `platypus_animation`). Clips' curves are now
    bound once per (clip, root) to bone slots and channels (`bind.rs`, Unity's `Rebind`), the
    pose is a dense per-bone array reused between frames, a bone's curves share their key
    search, and `mark_moved` stops at listed objects: fog-white-room's animation 0.375 → 0.027 ms
    idle, 0.635 → 0.039 ms moving (docs/benchmarks.md, "Animator"). QuickJS is left as it is
  - [x] Animation culling (2026-10-23): `Animator.cullingMode` (`alwaysAnimate`,
    `cullUpdateTransforms`, `cullCompletely`) and `Animation.cullingType` (`basedOnRenderers`),
    seen from the renderer's last frame (views and shadow maps); 50 mannequins looking away
    0.81 → 0.10 / 0.085 ms
  - [x] Animation culling follow-ups, part 1 (2026-10-23, found with `examples/crowd`): skinned
    meshes are frustum-culled in every view by their bone box (worked out once a frame) and left
    out of shadow maps they miss (`RenderStats.shadowCasters`); `Renderer.isVisible` (watched
    once a script asks); the parts of one skeleton share their bone matrices (render 14.6 → 8.6
    ms for 500 figures); animators are grouped per object (no scan of every animator per object)
  - [x] Animation culling follow-ups, part 2 (2026-10-23): the safeguard (an animated object
    with a physics component, AudioSource, ParticleSystem, Light or script below it is never
    culled, one warning each, `M5-ANI-14`); `Animator.update(dt)` and `Animator.enabled`
    (`M5-ANI-15`); render-texture cameras (how a secondary window shows the world) count
    (`M5-CUL-06`; windows draw no world of their own); `Animator` / `Animation` settings,
    controllers and clips saved in scene files (`M5-ANI-16`); bone matrices kept between frames
    and only drawn ones uploaded, once (`M5-CUL-07`); animators posed in parallel, bit for bit
    (`M5-ANI-17`). 1000 a block, view 4: 9.5 → 25.9 fps (`docs/benchmarks.md`)
  - [ ] Animation culling follow-ups, left: a cheaper bone box (bounds per bone at import; a
    moving skeleton's matrices are still built every frame for its frustum test, seen or not);
    writing the animated transforms stays in turn (about 4 ms at 2000 figures, mostly
    `set_local`); `update_world` walks every object every frame; animator parameter values and
    current states are not saved in scene files; `Camera.targetWindow` (D3) would need its own
    views counted; a script switch for frustum culling (none today); a thread pool instead of
    scoped threads per frame
  - [x] `examples/crowd` (2026-10-23): two city blocks of walking mannequins (50 to 1000 a
    block, each its own Animator), four views, the culling modes on a key, a HUD counting what
    is seen, animated and culled per block; `tools/bench/inputs/crowd.json`
  - [x] Native render sync (found 2026-10-22 by `platypus-build bench`: `sync_native_render` cost
    0.6-2 ms a frame, making fog-world and six clock faces slower native than QuickJS): it
    rebuilt every component every frame and copied every script mesh's vertices per renderer
    (`mesh_info`). Now incremental (registry versions, compare with what was sent, typed
    `set_renderer` / `set_stroke`; `M9-API-1700..1701`): 1.9 → 0.03 ms in fog-world,
    godrays-clouds, godrays-forest and fog-park, 0.6 → 0.07 ms in clock (docs/benchmarks.md)- [ ] **A profiler** (asked for 2026-10-21): today a slow frame is found by switching
  features off and timing runs. Wanted, in the window (an overlay or the editor) and as a
  capture to compare:
  - [ ] Frame time split: script (per hook: `onUpdate`, animation, physics callbacks, UI
    commits), engine (scene, physics step, audio), render (CPU encode, GPU passes via
    timestamp queries: shadows, world, light volume, post, UI), present / vsync wait
  - [ ] Scripts: time per component class and per hot function (a sampling profiler or
    instrumented hooks of the native backend), allocations and GC pauses
  - [ ] Animation: animators, bones and curves sampled per frame, time per character
  - [ ] Rendering counters beside `RenderStats`: draw calls, triangles, pipeline switches,
    texture and buffer memory, shadow maps drawn
  - [ ] UI: restyle, layout, text shaping and draw-list time per commit, nodes touched
  - [ ] Physics: bodies, contacts and step time; audio: voices and mixer time
  - [ ] Frame-time graph with spikes kept, and a capture written to a file (Chrome trace
    format, so it opens in Perfetto)
- [x] **Physics: locked axes switch friction off** (found 2026-10-18 with `examples/physics`):
  a body that can't move along one axis and can't turn about the other two (`setAllowedAxes`
  with z movement and x/y turns locked, the usual way to keep bodies in a plane) slid as if
  on ice. rapier 0.36.1's 3D friction solve (`contact_constraint_element.rs`) inverts the
  2×2 tangent mass matrix; the locked direction has zero mass, the determinant is 0,
  `simd_inv(0)` gives 0, and every friction impulse becomes 0. Worked around in Platypus
  (2026-10-19): that combination isn't given to rapier as a lock; `platypus_physics` pins the axis
  itself each step (`set_constraints`, `hold_pins`) — `M4-PHY-07`
  - [ ] Upstream: reported as dimforge/rapier#1047, fixed by dimforge/rapier#1048 (branch
    `fix/plane-locked-friction` on Quacky2200/rapier; the material is in
    `docs/upstream/rapier-plane-locked-friction/`). Once a rapier release has it, drop the pins
    and lock the axis in rapier again
- [x] **M5.c** Rendering depth: a basic cube scene end to end, better shadows, materials
  (done 2026-10-17; the unticked items below are follow-ups for later)
  - [x] Materials (asked for 2026-10-17), Unity Standard / URP Lit style: PBR, the sky and
    its reflections, planar mirrors, reflection probes, the showroom example (`8bac39e`);
    the specular workflow, maps, rendering modes and texture quality (2026-10-17).
    - [x] PBR shading: Cook-Torrance (GGX, Smith, Schlick) metallic / roughness replacing
      Blinn-Phong; `smoothness` stays as 1 - roughness (Unity's name); `Material.metallic`,
      `Material.create({...})` / `with({...})`; glTF `metallicFactor`; ambient reflections
      with the analytic split-sum BRDF — `M5-RND-11`
    - [x] A procedural sky (`RenderSettings.skybox`: zenith, horizon, ground, sun) drawn
      into an HDR environment cubemap with blurred mips: smooth surfaces reflect it sharply,
      rough ones see blurrier mips; sky light for ambient (`ambientMode` `skybox` / `color`),
      `reflectionIntensity`; cameras show it with `clearFlags: "skybox"` — `M5-RND-11`
    - [x] The specular workflow: `Material.specularColor` (reflectance head-on; metallic
      ignored), with `metallicGlossMap` as the specular map — `M5-RND-17`
    - [x] Planar mirrors: `Mirror` on a MeshRenderer (plane or quad) reflects the scene
      exactly: the renderer draws the main view from the camera reflected in the plane
      (projection flipped left to right to keep winding, a clip plane dropping what's behind
      it) into an HDR texture, and the mirror samples it at its screen position as its
      reflection, tinted by the material; other cameras see the sky — `M5-RND-12`
    - [x] Reflection probes: `ReflectionProbe` (size, resolution, box projection, refresh on
      awake / every frame / `renderProbe()`): six 90° views of the main view's scene from the
      probe into an HDR cubemap (copied in mirrored, mips blurred); meshes whose centre is in
      its box (the smallest box wins) reflect it instead of the sky — `M5-RND-13`
    - [ ] Reflection follow-ups: baked probes, probe blending at box edges, screen-space
      reflections, skyboxes from images (cubemap faces, equirectangular HDR), proper GGX
      prefiltering of cube mips, mirror resolution scale
    - [x] `examples/materials`: the roughness × metallic sphere grid, a mirror wall, a gold
      bunny and a chrome teapot under a procedural sky
    - [x] Maps: `normalMap` / `normalScale` (tangents made from UVs, or glTF's),
      `metallicGlossMap` (Unity packing; glTF's roughness / metallic packing for models),
      `occlusionMap` / `occlusionStrength`, `emissionMap`, `detailAlbedoMap` with its own
      tiling; data maps read through a linear view, `Texture` `{ linear: true }` for data
      textures; glTF models bring theirs — `M5-MAT-01`, `M5-MAT-02`, `M5-RND-17`
    - [ ] Map follow-ups: a detail normal map and detail mask, parallax (height) maps,
      tiling per map beyond the detail map's
    - [x] Rendering modes: `renderingMode` `opaque` / `cutout` (`alphaCutoff`, holes in
      shadows too) / `fade` / `transparent` (highlights and reflections kept), and
      `doubleSided`; glTF alpha modes and double-sidedness map onto them — `M5-RND-17`
    - [ ] Glass with refraction (screen-space), order-independent transparency
    - [x] A material test scene (spheres sweeping roughness × metallic, the classic grid):
      `examples/materials`, `M5-RND-11`
  - [x] Camera into a texture (asked for 2026-10-17; from M5.a's `targetTexture` and the
    M5.a2 texture follow-ups): `RenderTexture` (`new RenderTexture(w, h)`, `setSize`,
    saved in scenes), `Camera.targetTexture` drawing that camera into it before the
    screen (post-processing included), usable as any material's or sprite's texture;
    a camera can't show its own texture (plain colour there); texture cameras stay out
    of `Camera.main` and pointer rays — `M5-RTX-01`, `M5-RND-19`
  - [ ] Render texture follow-ups: formats (HDR, depth), mipmaps, `antiAliasing` per
    texture, `Camera.Render()` on demand, reading pixels back
  - [x] Quality settings (asked for 2026-10-17), Unity's `QualitySettings` levels. MSAA,
    FXAA, render scale, shadows, texture quality, levels, manifest defaults, saved choices,
    the example (`M5-RND-14`, `M5-QUA-01`, `M5-QUA-02`). Left: TAA, bloom and particle
    limits per level.
    - [x] Anti-aliasing: MSAA (2 / 4 / 8x, capped by what the GPU supports for the format;
      the adapter-specific feature is requested so 2x and 8x work where offered) for the main
      world pass; mirrors and probes stay 1x
    - [x] FXAA as a cheap post pass after tonemapping (`Camera.antialiasing: "fxaa"`, the
      main camera's choice for the screen) — `M5-RND-18`
    - [ ] TAA (needs motion vectors and jitter), SMAA
    - [x] Shadow quality: off / hard only / all, soft filter 3 x 3 or 5 x 5, resolution,
      distance; cascades with the cascade work
    - [x] Render scale (0.25 to 2): the world drawn into a texture at the scaled size and
      stretched onto the target (bilinear), UI kept at native resolution; FSR-style
      sharpening later
    - [x] Render textures: see "Camera into a texture" above
    - [x] Texture quality: `QualitySettings.globalTextureMipmapLimit` (top mips skipped),
      `anisotropicFiltering` (disable / enable / forceEnable) with `Texture.anisoLevel`;
      per level (Low: half size, no anisotropy; Ultra: forced) — `M5-QUA-02`
    - [ ] Bloom and particle limits per level
    - [x] Named levels (Low / Medium / High / Ultra) setting all of the above;
      `QualitySettings.setQualityLevel` / `getQualityLevel` / `names` / `levels` /
      `onLevelChanged`; the default level applied when the engine loads
    - [x] Project / game settings: the default level and per-level overrides in the game's
      manifest (`package.json` `platypus.quality`); a settings UI menu in games (React, in the
      example); the editor's (M5.e) later
    - [x] Players' quality choices saved per user (`platypus.quality` in `PlayerPrefs`)
  - [x] Display settings (asked for 2026-10-17): `Screen.fullScreenMode` (exclusive,
    borderless, maximized, windowed; platforms without a mode fall back: no exclusive on
    Wayland, fullscreen only on phones), `Screen.setResolution` / `resolutions` /
    `currentResolution` / `supportedFullScreenModes`, `QualitySettings.vSyncCount`,
    `Application.targetFrameRate` (with vsync off); defaults in `platypus.display`, the
    player's choice saved and applied before the window shows; the arcade's settings
    menu — `M5-DSP-01`..`03`
  - [x] `PlayerPrefs` (part of M6 brought forward): a JSON file per game in the OS's
    settings folder (`%APPDATA%`, `~/Library/Application Support`, `$XDG_CONFIG_HOME`),
    `PLATYPUS_PREFS_DIR` to override — `M5-PRF-01`, `M5-PRF-02`
  - [x] Fixes (asked for 2026-10-17): exclusive fullscreen at the monitor's current
    resolution unless one is chosen, the window size kept through fullscreen
    (`M5-DSP-04`); `platypus` is a windowed (GUI-subsystem) program logging to `Player.log`
    (with colours) in the game's settings folder, and to the terminal it was started
    from (`M5-APP-04`)
  - [ ] Display follow-ups: picking the monitor (`Display.displays`), HDR output, a
    window-position memory, mobile safe areas and orientation (`Screen.safeArea`,
    `Screen.orientation`), console/handheld presets; exclusive fullscreen needs testing
    on each platform
    - [x] `examples/quality`: a fence, a turning fan of thin blades and cables (aliasing,
      shadows, scaling) with a settings menu for levels and each setting; it cycles the
      levels until the menu is used
  - [x] Camera render groups and distance (asked for 2026-10-17; done, `examples/layers`,
    `M5-LAY-01`..`03`), Unity's layers:
    - [x] `GameObject.layer` (0–31, default 0) sent as `scene.layer object layer` (kept on
      the scene's objects, so physics can use it later); named layers (Unity's built-ins: 0
      Default, 1 TransparentFX, 2 Ignore Raycast, 4 Water, 5 UI, plus the game's from
      `package.json` `platypus.layers`); `LayerMask.nameToLayer` / `layerToName` /
      `getMask(...names)` / `contains` / `everything`
    - [x] `Camera.cullingMask` (bit per layer, default all) turning render groups on and off
      per camera (a minimap that hides the HUD layer, a first-person camera that hides the
      player's body); sent with `render.camera`
    - [x] Render distance: `Camera.farClipPlane` (exists) plus `Camera.layerCullDistances`
      (32 distances, 0 = the far plane) hiding things on a layer farther than its distance,
      measured from the object's origin (bounds come with frustum culling)
    - [x] Field of view: `Camera.fieldOfView` (vertical, clamped 1 to 179),
      `horizontalFieldOfView`, `verticalToHorizontalFieldOfView` / back (asked for
      2026-10-17); the example's menu cycles FOV and render distance
    - [x] Engine side: `views()` tags meshes, quads, sprites, world panels, particles,
      beams and halos with their object's layer and position and filters them per camera
      by mask and distance (objects a camera skips don't cast shadows in its view either,
      like Unity's)
    - [ ] Later: layers for physics (collision matrix, raycast masks) and light culling masks
  - [x] Culling (asked for 2026-10-17), with the render groups above:
    - [x] Culling masks: `Camera.cullingMask` (above); `Light.cullingMask` (which layers a
      light lights, in the shader per instance; the shadowing light's mask also picks
      the casters); `ReflectionProbe.cullingMask` and `Mirror.cullingMask` (what they
      capture); `M5-RND-15`, `M5-LAY-04`
    - [ ] Culling distances: `Camera.layerCullDistances` and `layerCullSpherical` done
      (above); per-light range culling done (with frustum culling); per-light
      range culling (skip lights whose range doesn't reach the view); shadow distance per
      light; particle systems and sprites culled by distance too
    - [x] Frustum culling (`world/cull.rs`): meshes by their local bounds (kept per
      uploaded mesh) moved by their model, quads (sprites, particles, panels) by the unit
      quad, point and spot lights by the sphere they reach; every view, mirror and probe
      face. Casters out of view still go into the shadow map. `WorldRenderer::stats()`
      counts drawn and culled meshes, quads and lights (`M5-CUL-01`, `M5-CUL-02`)
    - [ ] Later: skip casters outside the shadow map's box too; cull whole particle
      systems by their bounds before building quads; expose the stats to scripts
    - [x] LOD groups (Unity's `LODGroup`): `setLODs` / `getLODs` / `forceLOD` / `size`;
      each camera shows one level's MeshRenderers by the group's screen-relative height
      (perspective or orthographic), nothing below the last level;
      `QualitySettings.lodBias` per level (Low 0.4, Medium 0.7, High 1, Ultra 2); trees in
      `examples/layers` (`M5-LOD-01`, `M5-LOD-02`)
    - [ ] LOD cross-fade (dithered) and `QualitySettings.maximumLODLevel`; bounds-based
      group size and reference point (now the object's origin and a given size)
    - [ ] Occlusion culling later (GPU depth pyramid / Hi-Z), for big scenes
- [x] **M5.d** The rest of scene, assets and rendering (done 2026-10-17; the unticked items
  below are follow-ups for later):
  - [x] Animation (Unity's), with `examples/animation` (a robot on an Animator, a skinned
    glTF worm, a skinned tentacle made in code)
    - [x] `AnimationCurve` / `Keyframe` (Hermite tangents, steps, wrap modes),
      `AnimationClip` (curves by path for transform channels and component properties,
      events, `sampleAnimation`, JSON), the legacy `Animation` component (play,
      crossFade), `Animator` + `AnimatorController` (parameters, states, 1D blend trees,
      conditional / exit-time / Any State transitions with cross-fades); evaluated each
      frame between `onUpdate` and `onLateUpdate` (`__platypus_animate`); saved in scenes —
      `M5-ANI-01`..`05`
    - [x] glTF animations as clips (linear, step, cubic spline; copies get an `Animation`
      playing the first), skins and GPU skinning: `SkinnedMeshRenderer` (`bones`,
      `rootBone`), `Mesh` `boneWeights` / `bindposes`, bone matrices computed per frame
      in the app, skinned pipelines and shadows — `M5-SKN-01`, `M5-SKN-02`, `M5-RND-21`
    - [ ] Skinning follow-ups: bounds for culling skinned meshes (they're always drawn),
      morph targets (blend shapes), more than four weights per vertex, GPU-side bone
      matrices
    - [ ] Later: animator layers and avatar masks, 2D blend trees, root motion,
      `AnimationCurve` in particles' over-lifetime keys, `Gradient`
  - [x] Prefabs (Unity's): `Prefab.fromObject` / `fromData` / `load` (a `.prefab.json` in
    the game folder) / `toJSON`, `prefab.instantiate` and `GameObject.instantiate(prefab)`;
    copies keep references inside the prefab, drop those outside, and remember their file
    when saved in scenes — `M5-PRE-01`
  - [ ] Prefab follow-ups: linked instances with overrides (save only what differs, update
    copies when the prefab changes), nested prefabs and variants (with the editor, M5.e)
  - [x] Cloth (Unity's `Cloth`; rapier has no soft bodies): position-based dynamics in
    `platypus_physics::cloth` (welded vertices, stretch and bend constraints, damping,
    gravity, wind and gusts, friction), pins (points or a box) held where the object is,
    sphere and capsule colliders; a copy of the renderer's mesh updated each fixed tick;
    `examples/cloth` — `M5-CLO-01`..`03`
  - [ ] Cloth follow-ups: self-collision, collision with the physics world (floors,
    boxes), tearing, saving a cloth's object in scenes keeps the source mesh, volumetric
    soft bodies
  - [x] Scene serialization (M5.b) and GUID `.meta` files: `scripts/assets.ts` (run by
    `bun run dev`) gives every asset a `.meta` (GUID, importer settings), keeps GUIDs
    through moves, reports orphans (`--prune` deletes them); the engine reads them into
    `__platypus_info.assets`; `Assets.guidToPath` / `pathToGuid` / `importer`, `"guid:..."`
    sources for textures, models and sounds, importer defaults for `Texture.load` and
    streaming; scenes save GUIDs and find renamed files by them — `M5-AST-03`..`05`
  - [x] Asset hot reload: the game folder is watched (not `build/`, `node_modules/`,
    hidden folders); textures, models (same mesh and texture ids) and sounds from a
    changed file load again in place, then `Assets.onChanged` listeners hear the paths —
    `M5-AST-01`, `M5-AST-02`. Placed model copies keep their materials and nodes.
  - [x] Cooking: `platypus --cook [game]` prepares textures (every mip, BC1 for opaque or
    cut-out images, BC3 for smooth alpha, RGBA8 when not whole blocks; `.meta` import
    settings) and glTF models (meshes ready to upload, textures cooked) into
    `.platypus/cooked/`; loads use a cooked file newer than its source (decoded on GPUs
    without BC); the device asks for BC when the GPU has it — `M5-AST-06`
  - [ ] Cooking follow-ups: mobile formats (ASTC / ETC2), BC7 and BC5 (normal maps),
    better BC endpoint fitting, compression of cooked files, cooking scenes and sounds,
    packing a release (with M9)
  - [x] PBR, shadows, IBL, post-processing — done in M5.c (`M5-RND-11`..`21`)
- [ ] **M5.e** Editor shell. UI requirements first (before the editor work starts):
  - [ ] Accessibility: an AccessKit tree from the UI tree (roles, names, states, focus, actions),
    so screen readers and platform accessibility tools work
  - [x] Drag and drop in the UI: `draggable`, `dragstart` / `drag` / `dragenter` / `dragover` /
    `dragleave` / `drop` / `dragend` with a data payload, drop targets, a drag image, and files
    dropped onto the window (winit's `DroppedFile`); builds on pointer capture (M3d group 11)
    — `M5-DND-01..06`, `-09`
    - [x] `Interaction` (`interaction/dnd.rs`): threshold, events, accepting by preventing
      `dragover`, Escape / blur cancel, a live drag image (`UiTree::set_drag_image`,
      `setDragImage`), system files (`FileDragEnter` / `FileDrop` / `FileDragCancel`)
    - [x] `@platypus/react`: `onDrag*` props, `PlatypusDragEvent`, `PlatypusDataTransfer` (`setData`,
      `getData`, `types`, `files`, `dropEffect`, `setDragImage`)
    - [x] App: winit's hovered / dropped files gathered per frame (`file_drop.rs`), the cursor
      position from the system during a drag on Windows
    - [x] Editor example: a project browser (`platypus_editor [project]`) listing the project
      folder; files dropped from the system are copied into the folder they land on, items
      dragged inside move into folders; editor-only ops `editor.listDir` / `copyIn` / `move`,
      confined to the project — `M5-DND-07`, `-08`, `-10`
    - [ ] Later: a snapshot drag image; `dragover` repeating while still; cursors per
      `dropEffect`; text and URLs dragged in from other programs; drag positions from the
      system on macOS and Linux (winit 0.31's drag events); dragging out of the window;
      deleting, renaming and creating files in the project browser; the manual check in the
      window (`M5-DND-11`)
  - [ ] Then the shell: docking, tree view, virtualized list, inspector, undo/redo — `M5-EDT-*`
    - [x] `platypus_editor` (`platypus_editor.exe` on Windows): a second binary of `platypus_app`
      (`src/bin/platypus_editor.rs` includes main.rs as a module; `editor::ENABLED` from
      `CARGO_BIN_NAME`) that loads `editor/build` (or `PLATYPUS_EDITOR_UI`); the editor UI is a
      Bun workspace (`editor/`) in the root `bun run build`; `Application.isEditor` — `M5-EDT-17`
    - [x] `@platypus/editor-ui` (`js/editor-ui`): reusable components drawn by Platypus's UI, with
      the logic in plain modules — `M5-EDT-06..14`, `-18..20`
      - [x] Menubar (non-native): submenus, accelerators shown and working from anywhere,
        Alt+letter / F10, arrows / Home / End / Enter / Escape, hover switches open menus,
        a press outside closes
      - [x] Docking: splits with draggable splitters (pointer capture), tab groups, tabs
        dragged onto a group's middle or edges (drop preview and a ghost), layout to and
        from JSON (the editor saves it in PlayerPrefs)
      - [x] Virtualized list (fixed row height, overscan, scroll-to-index, keys) and a tree
        view on it (expand/collapse, Ctrl/Shift selection, keyboard, icon and rename slots)
      - [x] Console: level colours, filter buttons with counts, collapse, search, auto-scroll
        that pauses when scrolled up, code links opened with `editor.openFile` (`code -g`
        or `PLATYPUS_EDITOR_OPEN`); fed from the log through a capture layer and `__platypus_log`
        — `M5-EDT-15`, `M5-EDT-16`
      - [x] Demo layout: Hierarchy (a ~20,000-node tree), Project, Scene, Game and Inspector
        placeholders, Console, status bar — `M5-EDT-21`
    - [ ] **Next: the Scene view** (asked for 2026-10-17), replacing the placeholder panel in
      `editor/src/main.tsx`:
      - [x] An editor camera drawing into the Scene panel: a `Camera` whose `rect` follows the
        panel's layout box (the UI over it left see-through: `DockPanel.seeThrough`), kept
        out of the Hierarchy and scene saves (`HideAndDontSave`). Unity's controls:
        right-drag to look (WASD / QE to fly, Shift faster), middle-drag to pan, wheel to
        zoom, Alt+left-drag to orbit, F to frame the selection (`scene-camera.ts`,
        `SceneView.tsx`, `M5-EDT-22`). A demo scene (`editor/src/demo-scene.ts`) until
        scenes open from the project; the Hierarchy is live; the Game tab shows
        `Camera.main` in its box (`M5-EDT-23`).
        - [ ] Check the controls by hand in the window (`M5-EDT-23`).
      - [x] A grid on the ground plane, drawn by the renderer (`WorldView.grid`,
        `Camera.sceneGrid`): a pass after the opaque meshes that intersects each pixel's
        ray with y = 0, draws anti-aliased 1 m and 10 m lines that fade with distance, the
        X axis red and the Z axis blue, depth-tested at the plane's depth so objects hide
        it (`M5-RND-22`). `HideFlags` on GameObjects (`DontSave` objects aren't saved,
        hidden cameras aren't `Camera.main`; `M5-SCN-06`).
      - [x] The x/y/z orientation gizmo in the panel's corner (Unity's scene gizmo): axes from
        the camera's rotation, drawn as SVG in the UI; clicking an axis looks along it,
        the centre toggles perspective / orthographic.
      - [x] **SceneIcons** (the user's name for Unity's gizmo icons): icons at cameras, lights
        (directional, point, spot), audio sources and probes, at a constant screen size,
        drawn as UI (small SVGs) over the view from projected positions; clicking one
        selects its object.
      - [x] Wire gizmos on selected objects (a camera's frustum, a light's range, cone or
        rays, a probe's box, an audio source's distances): thin lines drawn by the
        renderer (`WorldView.lines`, `render.gizmos`), faint where hidden, from Unity's
        `Gizmos` API (`gizmos.ts`; `M5-RND-23`).
      - [ ] Later: components' own `onDrawGizmos` / `onDrawGizmosSelected`; picking
        meshes by clicking them in the view (a GPU id pass or physics raycasts);
        outlines on selected meshes; the grid on the view's plane in side views (Unity
        flips it to XY / YZ when orthographic along an axis); gizmo icons for user
        components.
      - [x] Move / rotate / scale handles on the active (last selected) object (Q / W / E / R
        and a toolbar; Ctrl snaps; local axes; `handles.ts`, `M5-EDT-24`), drawn as SVG over
        the view; `SceneView.onTransformed` reports each drag for undo.
      - [ ] Later handles: Global / Local and Pivot / Center toggles, plane handles (move
        on two axes), the view-aligned rotation ring, multi-object drags, snap settings.
      - [ ] Then: an Inspector bound to the same scene (the Hierarchy is live already).
    - [ ] Inspector (`@property` fields), live Hierarchy from the scene, undo/redo
      (`M5-EDT-01..03`); the 10,000-row scroll benchmark (`M5-EDT-04`); the manual docking
      check in the window (`M5-EDT-05`). Inspector, live Hierarchy and undo/redo are done (E3, E5 below; script
      `@property` fields wait for E11); the benchmark and the manual checks are open
    - [ ] **A usable editor** (2026-10-29, milestones E1..E10 of the editor roadmap):
      - [x] E1: the editor's world loads assets from `--project` (its own UI still from `editor/`);
        components of kinds the editor doesn't register (the game's scripts) are kept as data and
        saved back (`Opaque` in `platypus_scenes`, `M5-SCN-10`)
      - [x] E2: `Editor.readText` / `writeText` (atomic) / `exists` / `stat` / `freeName` / `createDir` / `rename` /
        `trash` / `restore` (the project's own `.platypus/trash`; `.meta` files go along and are hidden in listings);
        in-editor `Modal`, `confirmDialog`, `textDialog` and `FileDialog` (open / save / folder, filters, typed paths,
        New Folder, overwrite check); New Folder, Rename (F2) and Delete (Del) in the Project browser
        (`M5-EFS-02..05`, `M5-EDT-31..34`)
        - [x] Dialogs are modal `dialog`s (2026-10-30): a dimmed `::backdrop` (black at 0.6 while blending was in linear
          light, 0.35 since sRGB blending, the same dimming), Tab and the pointer kept inside, Escape cancels, focus given back on
          close (also when a field focuses itself as it opens: `Interaction` keeps the focus from before, `M3-DLG-03`);
          dialog buttons take Tab, Enter and Space; the bench finds text through transforms (`UiTree::to_window`)
          (`M5-EDT-43`)
        - [ ] No context menu in the Project browser yet
      - [x] E3: undo / redo (`history.ts`: merge keys, saved position, cap; `scene-edit.ts`: commands keep where an
        object is, not the object, so undoing a delete builds it again from an `EditorScene.snapshot`); Edit > Undo /
        Redo / Cut / Copy / Paste / Duplicate / Delete / Select All; handle drags, renames, Hierarchy drags (onto a row:
        under it; below the rows: to the root) and Project deletes are undoable (`M5-UNDO-01..06`)
        - [x] Dropping between rows in the Hierarchy (2026-10-30): a row's top or bottom quarter puts the dragged
          object before or after it (after an open parent: first under it), shown by an insertion line; undoable
          (`tree.ts` `dropPlace`, `TreeView` `onMoveNode`, `M5-EDT-46`, `-47`)
        - [x] References from other objects to a deleted one come back on undo (`refs.ts`: noted by place before the
          delete, set to the rebuilt objects after); copies are named "Cube (1)", "Cube (2)" like Unity's
          (`duplicateName`) (`M5-UNDO-07`, `-08`)
      - [x] E4: scenes (`scenes.ts`): File > New / Open / Open Recent (per project, Clear Recent) / Save / Save As,
        "Save changes?" before New, Open and Exit, the window title with `*`; the last scene (or the project's
        `startScene`) opens at start, the demo scene only without one; GameObject > Create Empty / 3D Object / Light
        / Camera (undoable); game cameras draw only in the Game panel and are saved with their own rect; saved
        scenes and copied-in assets get `.meta` files (`platypus_core::meta`, `Editor.ensureMeta`) (`M5-SCN-11..13`).
        A compiler fix came with it: a generic promise function no longer makes a `then` for every pair of types
        (`M9-TS-3523`)
      - [x] E5: the Inspector (`Inspector.tsx`, `inspector.ts`; `EditorScene.components` / `fields` / `setField` /
        `addComponent` / `removeComponent` / `componentKinds` over the kinds, `platypus_scenes/src/inspect.rs`): name,
        enabled, Transform (local position, Euler angles, scale), a foldout per component with number, checkbox,
        text, choice, vector and colour fields, live, every edit undoable; Add / Remove Component; kept (opaque)
        components edited as data. Field types and enum options from `platypus_kinds::register_fields` (the
        generated render components; others from their saved values) (`M5-EDT-35..37`)
        - [x] E5b (2026-10-30): references to objects, components and assets (textures, sounds) picked in a picker
          dialog or dropped from the Hierarchy / Project (`InspectorInputs.tsx`, `Pickers.tsx`,
          `EditorScene.assetFromFile`, `FieldInfo.refKind`); a colour picker (palette and hex); tag and layer editable
          (layer names from the project, `Editor.layerNames`); several objects at once (shared components, "—" for
          mixed values, one undo step for all); schemas for the hand-written kinds (physics in `platypus_physics_api`
          `kind_fields.rs`; Camera, Animation, Animator, AudioSource, AudioListener, SplineAnimate in
          `platypus_scenes` `kind_fields.rs`). Choices open in a picker dialog: a dropped-down list paints under the
          rows after it (z-index orders siblings only) (`M5-EDT-48..50`). A compiler fix came with it: `useState`
          of a local named like a top-level function (`M9-TS-3533`)
        - [ ] Later: schemas for script `@property` fields (needs the project's code in the editor, E11); assets other
          than textures and sounds from files (meshes, materials, clips come from models and code); scrubbing numbers
          by dragging the label; field docs as tooltips (no tooltips yet); the startup compile (~2 s)
      - [x] E6: Edit > Project Settings (`ProjectSettings.tsx`): `project.json`'s settings grouped (Identity, Game,
        Build) from `platypus_core::project_edit::FIELDS`; each change rewrites only that key's text
        (`Editor.projectFields` / `setProjectField` / `removeProjectField`), keeping unknown keys, key order and
        `${templates}`; values are checked; the dialog has its own Undo / Redo (`M5-PRJ-01..05`). A compiler fix came
        with it: `cond && f()` with a void `f` dropped the call (`M9-TS-3524`)
        - [x] `layers`, `quality` and `display` in the form (2026-10-30): dotted keys edit inside those objects
          (`display.width`, `layers.8`, `quality.default`) with the same in-place text edits; an emptied layer name or
          size removes its key and an emptied section goes (`M5-PRJ-06`, `-07`)
        - [ ] Later: the platform sections and per-level quality overrides (`quality.levels.*`) in the form; a file
          picker for path settings; showing a template's resolved value as a hint
      - [x] E7: prefab files: a Hierarchy object dragged onto a Project folder is saved as `<name>.prefab.json` (with
        its `.meta`; `EditorScene.savePrefab`, `manager::prefab_file_json`: assets in the file's table); a prefab file
        dragged onto the Hierarchy (a row: under it) or the Scene view, or double-clicked, is placed
        (`EditorScene.placePrefab`), undoably; prefab files keep their `assets` table (`M5-PRF-10`, `-11`)
        - [ ] Later (decided 2026-10-29: unlinked copies first): linked prefab instances (a scene saves `prefab: path`
          plus overrides; apply / revert), nested prefabs and variants, a prefab editing mode
      - [x] E8: models placed by drag and drop: a `.glb` / `.gltf` / `.fbx` from the Project browser or from the system
        (copied into `assets/models` with its `.meta` first) onto the Scene view lands on the ground under the
        pointer, onto the Hierarchy under the row (or at the root); double-clicking places it too; the new object is
        selected; undoable (`placement.ts`, `M5-DND-20`, `-21`)
        - [x] A preview while dragging a Project model or prefab over the Scene view (2026-10-30): the real object,
          hidden from the Hierarchy and never saved, at the ground point under the pointer; gone when the drag leaves
          the view or ends (`SceneView` `makePreview`, `M5-DND-22`). Dropping between Hierarchy rows: done
          (`M5-EDT-46`, `-47`)
        - [ ] Later: a preview for files dragged in from the system (they are copied in on drop, not before);
          dropping onto meshes (a physics ray)
      - [x] E9: FBX models (`.fbx`, binary or ASCII) through the `ufbx` crate (MIT OR Unlicense, C built with `cc`;
        notices regenerated, reviewed in `tools/platypus_notices/src/tables.rs`): `models_fbx.rs` makes the same
        `Model` as glTF: units and axes to metres and Y-up, meshes split by material, materials (base colour,
        emission, roughness, metalness, base colour / normal / emission textures, embedded or beside the file),
        the node tree, skins (bones, ≤4 normalised weights, inverse bind matrices) and the takes baked to keys;
        `.fbx` counts as a model for `.meta` files, packs and `--cook`. Test files are made by
        `tests/fixtures/assets/make_fbx.py` (no downloads) (`M5-FBX-01..05`)
        - [x] The `.meta` importer's `scaleFactor` scales an FBX (2026-10-30, `M5-FBX-06`); a cooked model or texture
          goes stale when its `.meta` is newer
        - [ ] Later (FBX): the other importer options (import animation / materials, colliders); cameras and lights in
          the file (glTF doesn't bring them in either: no model description of them yet); vertex colours and UV1 (the
          engine's meshes have neither: `MeshInfo` and the shaders need them first); blend shapes (no morph targets in
          the engine); a golden of a posed FBX character; real exporter files (Maya, Blender, 3ds Max) as fixtures once
          their licences are checked
      - [x] E10: the remaining menus: View > Panels (checked when open; closes the active tab or opens it again,
        tabs have a close box), Show Grid / Gizmos kept as preferences, Edit > Preferences (`Preferences.tsx`:
        grid, gizmos, reopening the last scene, in PlayerPrefs), Help > Documentation (opens `docs/editor.md` in the
        code editor) and About; the window's close box asks to save unsaved changes like File > Exit
        (`Window.main.onCloseRequested`) (`M5-EDT-41`, `-42`)
        - [x] Accelerators while a text field has focus (2026-10-30): the field keeps typing, Delete and Ctrl+A / C /
          V / X; other Ctrl / Alt keys and the function keys run the menus (`fieldAccelerator` on the editor's root,
          `fieldKeepsKey`); bench files send the modifier state (`M5-EDT-44`, `-45`, `M9-BEN-04`)
        - [ ] Later: a code-editor command setting (it is
          `PLATYPUS_EDITOR_OPEN` today); a manual check of the close-box prompt in the window
      - [ ] Editor startup compile (~2.3-3.3 s in release; measured 2026-10-30: lowering ~2.3 s, Cranelift ~0.9 s,
        GPU device and renderer setup ~0.7 s after it): most of the lowered text is the UI prelude (73,874 lines
        against the editor's 6,935). No cheap cache: JIT code holds absolute addresses and the IR has no
        serialisation. Options: serialise the lowered IR keyed by bundle hash and compiler version (medium), or set
        the GPU up while compiling (medium)
      - [ ] E11 (not started, decided 2026-10-29: later): the project's own code in the editor (its component kinds
        registered, so script `@property` fields are edited like the built-ins instead of as data) and play mode
        (Play / Pause / Stop in the Game view, the scene restored on Stop). Needs a design doc first: a second
        bundle JIT-loaded into the editor process with its hooks held back in edit mode
    - [ ] Later: variable row heights in the virtual list; dropping tabs at a position in
      the tab strip; floating panels in
      their own windows; a golden image for the editor (needs a fixed font, the
      editor uses system fonts)
    - [ ] Later: native menus as a fallback (Win32 menus on Windows, NSMenu on macOS, GTK on
      Linux): `MenuBar` stops drawing itself and builds the platform menu from the same
      `Menu` data, running the same actions
  - [ ] Later editor tasks:
    - [ ] UI editor: pick elements in a running game's UI, inspect their computed styles and box
      model, edit styles live (like browser devtools), and edit/arrange React UI visually
    - [ ] SVG editor: draw and edit shapes and paths (nodes, curves), gradients, strokes and
      groups; preview CSS styling and animations on them; save `.svg` files

## M6 — Networking and files for scripts (Script IO)

Research and design: [docs/research/networking-and-files.md](docs/research/networking-and-files.md).

- [ ] Bridge: `Value::Bytes`, JS-side promise table (`asyncOp`), one `__platypus_io(events)` batch per
  frame with a budget, new `platypus_io` crate (`IoHost`, generations for hot reload) — `IO-BRG-*`
- [ ] Files: game manifest; read-only `game` root and per-game writable `data` root (cap-std,
  stricter names, quotas, atomic writes); `Bun.file`-shaped async API — `IO-FS-*`
- [ ] `fetch`: lazy tokio runtime, reqwest over rustls; policy (manifest hosts, HTTPS-only when
  shipped, resolved-address guard with pinning, checked redirects, forbidden headers, limits);
  `fetch`/`Headers`/`Request`/`Response`/`AbortController`/`URL` — `IO-NET-*`
- [ ] WebSocket: tokio-tungstenite on the same policy; text/binary, close codes, backpressure,
  limits — `IO-WS-*`
- No raw TCP/UDP for scripts: multiplayer stays engine-side (M8)

## M7 — Lighting

- [ ] Baked lighting (lightmaps), light and reflection probes
- [ ] Global illumination: bounced light and colour bleeding computed, not faked with bounce
  lights (`Light.bounceIntensity`, Unity's): baked with the lightmaps first, then a realtime
  approximation (virtual point lights from shadow-map hits, or probe volumes); the Cornell
  box is the test scene (asked for 2026-10-17)
- [ ] Fog (Unity's `RenderSettings.fog`, asked for 2026-10-18)
  - [x] Distance fog (2026-10-20): linear, exponential and exponential squared in a colour
    (`RenderSettings.fog` / `fogMode` / `fogColor` / `fogDensity` / `fogStartDistance` /
    `fogEndDistance`, saved with scenes), in the world shader for meshes, quads (sprites,
    particles, panels, text) and 2D shapes alike; glows fade out; the background stays clear
    (Unity's); per camera on or off (`Camera.fog`); orthographic cameras fog by depth.
    `examples/fishtank` uses it instead of mixing its colours by depth (`M7-FOG-01`..`03`)
  - [x] Height fog (2026-10-20): exponential in height, integrated exactly along each view
    ray (`RenderSettings.heightFog`: density, base height, falloff); it covers the background
    out to the far plane, so the horizon sinks into it (`M7-FOG-04`)
  - [x] Volumetric light shafts (2026-10-20): each pixel marches its view ray (jittered,
    `RenderSettings.volumetricFog`: intensity, anisotropy, steps, reach), each step's share
    of the fog lit by every light that opts in (`Light.volumetricIntensity`) through its hard
    shadow map, Henyey-Greenstein phase (`M7-FOG-05`)
  - [x] Follow-ups (2026-10-21): a froxel volume (lit once per cell, then added up along each
    view ray) and temporal accumulation of its jitter's grain (see the light volume below)
  - [ ] Shafts in the sky without height fog
  - [x] Fog volumes (2026-10-21): `FogVolume`, a world-aligned box of fog in its own colour
    with soft edges (mist over a pond, a cloud layer), integrated along each view ray at fixed
    points, over the background too, lit by light shafts; 8 per camera (`M7-FOG-06`)
  - [x] `examples/fog` (2026-10-21): every kind of fog in one misty clearing, switched from a
    menu
  - [x] Light shafts' albedo (`volumetricFog.albedo`, separate from `fogColor`, so dark night
    fog still glows under a lamp) and ellipsoid fog volumes (`FogVolume.shape`) (2026-10-21)
  - [x] Fog culling (2026-10-21): meshes, quads and local lights wholly past where the
    distance fog is opaque aren't drawn (`RenderSettings.fogCulling`, on by default), counted
    in `WorldStats::fog_culled`; scripts read the renderer's stats as `RenderStats`
    (`M7-FOG-07`)
  - [x] Spot and point lights' shafts worked out per pixel (2026-10-21): each view ray's stretch
    inside the light's range and cone found exactly, so cones have sharp edges and lanterns a
    round glow; the volume keeps the sun; lit fog goes on after fog volumes, so mist glows
    under a lamp (`M7-FOG-08`)
  - [x] God rays, engine (2026-10-21): within `volumetricFog.sharpDistance` of the camera
    the sun's shafts are worked out per pixel from its shadow map (fixed samples, closer
    nearer the eye), the volume covering the rest, so beams through bars and gaps stay sharp
    (`M7-FOG-10`)
  - [x] God-ray examples (2026-10-21): `godrays-forest`, `godrays-cell` (dust motes),
    `godrays-room`, `godrays-sunset`, `godrays-shed` (a scanning beam: partial or full scans,
    dust motes, haze and exposure pages), `godrays-lift`, `godrays-clouds`; reviewed in the
    window
  - [x] The light volume's quality (2026-10-21): no moire or grain through blinds (each
    cell's jitter shifted by a hash, 64 Halton points, a spatial filter); HDRP's denoising
    modes (`volumetricFog.denoising`), Unreal's `historyWeight` and `samples` per cell;
    history read through a 5-tap Catmull-Rom filter and trusted less the further it moved
    (no smear while turning); `motionSamples` for cells whose view moved (`M7-FOG-11`..`14`)
  - [x] Shadowed spot and point lights shaft through the volume (2026-10-21): their beams cut
    by planks and bars made arcs and moire per pixel; a moving one's history fades by how far
    its beam swept (capped at a quarter a frame). Unshadowed ones keep exact per-pixel cones.
    The volume is read a slice and a half short of surfaces (Unreal's depth bias), so a beam
    behind a wall shows no disc of haze on it (`M7-FOG-15`)
  - [x] Pyramid spot lights (2026-10-21, HDRP's spot shape): `Light.spotShape = "pyramid"`
    and `aspectRatio`; a thin, wide one is a sheet of light (the shed's full scan)
    (`M7-SPOT-01`, `02`)
  - [ ] Blades finer than the volume's cells (a beam through 1 cm plank gaps, seen towards
    the light) average into a soft disc of haze: work shadowed spot lights near the camera
    out per pixel too, edges found exactly as the sun's sharp shafts are (without bringing
    back the per-pixel arcs), the volume covering the rest
  - [ ] A moving light's sweep is measured at the end of its range, overstating it for nearer
    haze (more noise than needed); measure it where its beam meets the view, or per cell.
    Only four moving lights are followed (more count as moving a long way)
  - [ ] HDRP's third spot shape, Box (parallel light from a rectangle), and the beam glow
    mesh (`beamIntensity`) for pyramids
  - [x] Hot reload of a release build keeps the old scene (2026-10-21): with a game's
    window open, rebuilding `build/` reloaded it in place, but scenes built in `onStart` came
    back from the saved state, so changed geometry (fog-raider's corridor fix) didn't show,
    and the low-res look (`QualitySettings.renderScale`) looked blurrier than before. Check
    what a reload should rebuild versus restore, and re-apply quality settings once.
    Done: the blur was textures made with `Texture.create` (fog-raider's point-filtered
    speckle) being dropped by the save, so materials came back untextured; they are now
    saved with their pixels and options (`M4-HOT-11`). Quality was already restored once.
    Restoring rather than rebuilding objects is by design (docs/scripting.md "Limits"):
    changes to what `onStart` builds show after a restart
  - [x] God rays, first notes (2026-10-21, done as the mix: the volume for broad haze, the
    sun per pixel near the camera; the screen-space radial blur wasn't needed): crisp, high-contrast sun beams cutting through fog and
    cloud where light comes through gaps (skylights, a broken roof, a tree canopy), as in
    Unreal's volumetric fog and painted-style games. The froxel volume makes the sun's shafts
    soft; options: a screen-space radial blur from the sun's position over an occlusion mask
    (Crytek / GPU Gems 3 light shafts, cheap, works for the sky too), or a per-pixel sun march
    with the shadow map (like the spot cones now, sharper, costlier), maybe mixed with the
    volume (volume for broad haze, per pixel near the camera). An example: a ruined hall or a
    forest clearing with beams through the roof or canopy, with dust motes catching the light
  - [x] `examples/fog-world` (2026-10-21): an endless, streamed block world whose render
    distance is the fog: seeded noise terrain built per 16×16 chunk as one textured mesh of
    visible faces (`Mesh.createFlat` / `setFlat`), chunks recycled around the camera, a spare
    ring past the fog for fog culling to skip; the HUD reads `RenderStats`
  - [x] An "endless white room" fog level, like Assassin's Creed's loading screen (asked for
    2026-10-21; `examples/fog-white-room`, 2026-10-21: Quaternius's CC0 mannequin with an
    Animator (idle / walk / jog / sprint blend tree, jumps, a roll), linear fog past a clear
    bubble, a thin height fog, tiles lit underfoot and on their own, drifting fragments, a
    floating origin): a third-person character on a bright white floor of glowing grid tiles under
    a grey cloudy sky, everything fading into white fog a few tiles away. The floor moves with
    the player like a treadmill (tiles recycled around them), so the area never ends: no walls
    to reach, no edge to fall off. The player can run and jump with gravity (a
    CharacterController on a floor collider that follows them). Lighting: soft, bright, the
    tiles' grid lines glowing; fog close and white, height fog where the floor meets the sky
- [ ] Auto exposure (eye adaptation)
  - [x] `RenderSettings.autoExposure` (2026-10-21): metered HDR image, eased exposure,
    min/max EV, compensation, two speeds; bloom thresholds on exposed light (`M7-EXP-01`);
    examples tunnel, dark-room and the god-ray scenes
  - [x] Per camera, HDRP metering, blending, scripts (2026-10-21): each camera adapts on its
    own, split screens included (`M7-EXP-02`); a histogram with low/high percentiles and
    average / centre-weighted / spot metering (`M7-EXP-03`); a manual exposure
    (`fixedExposure`, EV100) blended with auto over `exposureTransition`, `hold`, and
    `freezeExposure()` to turn it off unseen (`M7-EXP-04`); `Camera.exposure` /
    `sceneBrightness` read back and `Camera.resetExposure()` (`M7-EXP-05`); a soft floor
    (`darkAdaptation` / `darkBelow`) and a compensation curve instead of `minEV`'s wall
    (`M7-EXP-06`); `QualitySettings.autoExposure`, the player's switch
  - [x] A change of manual exposure eases over `exposureTransition` too (2026-10-21);
    `QualitySettings.softShadowQuality` low / medium / high (3x3, 5x5, 7x7) softens beams'
    patches on floors; dark-room and shed have tabbed exposure pages, FPS, quality and vsync
- [ ] One render pipeline, as Unity 7 (asked for 2026-10-21, needed for iOS / Android). Unity
  is keeping only URP (HDRP deprecated in 7.0, removed after the first 7 LTS; Built-in removed
  in 7.0) and moving HDRP's features into it, scaled per quality level and platform
  - [x] Built-in / Universal / HD pipelines first built after Unity 6's split (b123848..d59281c),
    then folded into one (2026-10-21): each quality level carries a `PipelineAsset` (light
    shafts, HDR, additional lights and their shadows; auto exposure via
    `QualitySettings.autoExposure`); Low is the phones' tier; the manifest's default level can
    go by platform; `platypus.renderPipeline` only warns now (`M7-RP-01`..`03`)
  - [ ] More per-level switches as features land: shadow cascades and resolution caps per
    platform, MSAA and texture defaults, the light volume's detail, post effects
  - [ ] Mobile GPU limits: fewer storage buffers, no compute where it's missing (auto exposure
    and the light volume fall back or switch off), checked on an iOS / Android device
- [x] Real-world light units (2026-10-21, required): `platypus.lightUnits: "physical"` (a
  project-wide switch, as Unity 7 brings them into URP): the sun in lux, spot and point lights
  in lumens, emissive surfaces and the sky in nits; pre-exposure by each camera's last exposure;
  a physical camera (`Camera.aperture` / `shutterSpeed` / `iso` → `physicalExposure`); a light
  meter (`Light.illuminanceAt`, `measureIlluminance`); `examples/light-units` (`M7-HD-01`, `02`).
  Old projects keep Platypus's own units (`legacy`, the default)
- [ ] Post-processing (asked for 2026-10-21; today: bloom, exposure and tonemapping, FXAA,
  render scale, fog and light shafts, and CSS filter / backdrop-filter / blend modes for
  screen UI only)
  - [ ] Crisp pixel upscale: a point-filtered choice for `QualitySettings.renderScale`'s
    upscale (it's bilinear, so a low render scale looks soft, not like real low-res pixels);
    `examples/fog-raider`'s PS1 look uses it
  - [x] Custom post-process passes (2026-10-27, branch `claude/postfx-9b4c75`): `PostProcess`
    (`platypus_render_api/src/postfx.rs`): `create(effect)`, `createCustom(wgsl, params)`, `enabled`,
    `order`, `camera`, `setFloat` / `setColor` / `setVector`; a WGSL `effect(uv, color)` with
    helpers (`scene`, `scene_depth`, `scene_normal`, `view_uv`, `post_time`, ..); passes run per
    camera view after bloom, tonemapping and FXAA, compiled once per shader, errors logged and
    the pass skipped (`docs/postprocessing.md`, `M7-FX-01`..`09`)
  - [x] Built-in effects on that API: pixelate, vignette, colour grading (lift / gamma / gain,
    saturation, contrast, hue, balance; a LUT later), CRT scanlines and curvature, film grain,
    chromatic aberration, plus gaussian blur, tilt-shift and posterize
  - [x] Depth of field: thin-lens circle of confusion from a focal distance, f-stop and focal
    length, round or polygon bokeh with bright highlights weighted, a blurred background can't
    smear over a sharp foreground; tilt-shift as a variant. Depth is resolved from the world
    pass (MSAA included). Auto-focus is the game's: `examples/postfx` eases `focus_distance` to
    a chosen object
    - [ ] Near and far blur as separate layers (today one gather with a behind-the-pixel rule);
      an autofocus helper on the API
  - [x] Ink outlines: lines along silhouettes (second difference of depth) and creases (normals
    rebuilt from depth), thickness easing off with distance, optional brightness edges
    - [ ] Per-object opt-out (layers) and a cel / toon material option (a `posterize` pass does
      the banding for now)
  - [x] `examples/postfx`: a colonnade with every effect switchable (menu and keys), a custom
    WGSL effect, a tour mode, `-- <effect names>` to start with some on

## M8 — Multiplayer

- [ ] QUIC transport, Steam multiplayer (`steamworks`), `@sync` / `@rpc`

## M9 — Script backends, AOT and release packaging

- [ ] **M9.a** `deno_core` and Bun script backends; packaging and distribution (pack files, Steam
  Workshop?)
- [ ] **M9.b** Ahead-of-time scripts for shipped games: compile the Bun-bundled TypeScript down when the game
  is built, instead of interpreting it in QuickJS. Native code compiled into the executable is
  the preferred output; bytecode is the fallback. Candidates to evaluate:
  - QuickJS bytecode (the cheapest step: skips parsing at startup)
  - TypeScript → Rust native via Perry or Vercel's scriptc
  - AssemblyScript → wasm → wasm IR → Rust via `wasm2rs`
  - Questions: keeping the `__platypus_op` / `__platypus_commit` boundary unchanged, React's dynamic
    code under each option, and keeping dev builds on QuickJS for hot reload (M4)
- [x] **Windows API** (2026-10-21): `Window` (tool / dialog / popup windows, families and event domains, messages, hot reload by key,
  platform support table), `examples/windows`, guide `docs/windows.md`; steps in `tools/platypus_ts/TODO.md` "Windows API". Open
  optional leftovers: embedded `child` style, modal dialogs, `Camera.targetWindow`, transparent windows, `Application.wantsToQuit`,
  domains on their own thread / process, and a manual check of the winit runner on a real display (never run there yet).
- [x] **Startup video settings** (2026-10-28): an optional prompt shown once before the game's window (`videoSettingsPrompt` in
  `project.json`, `platypus-build package --video-settings-prompt`, `platypus-app --video-settings-prompt` / `--no-...`):
  window mode, a resolution list box, vsync, quality level, Play / Quit; a UI-only child process (`video_prompt.rs`,
  `video_prompt.tsx`); the choice goes into `platypus.display` / `platypus.quality`. Vsync is now on by default (a saved
  choice keeps it only with `playerVSync`). The main window opens centred on the primary monitor when windowed. The d-pad
  reaches the focused element as arrow keys first. Tests `M9-VSP-01`..`11`, `M5-DSP-05`. ANGLE is linked into the host now
  (one host, below), so the dialog draws with GLES in packaged exes too.
- [x] **One host, the renderer chosen from what the game uses** (2026-10-28): `platypus-app` and the UI-only `platypus-app`
  are one `platypus-app` (`platypus_editor` stays); `project.json` `type` is replaced by `renderer` (`vulkan` / `gles` /
  `cpu`, left out: automatic; the old `type` warns and is ignored); the compiler finds the program's tier at build time
  (`ui` / `world2d` / `world3d`, `platypus_ts::renderer`, written into `game.pbundle`) and a `ui` program starts on GLES through
  ANGLE linked in statically (`static-angle`, default on Windows; needs libclang to build), the rest on wgpu with the world;
  "3D Not Supported" stays as the safety net; `cpu` is an opt-in third path (`platypus_render_cpu`); `--renderer` on
  `platypus-app` and `platypus-build package`; `THIRD-PARTY-NOTICES.md` beside packaged exes; `init` writes a self-contained
  `tsconfig.json` and `types/platypus.d.ts`, `init --editor-files` refreshes them (all examples regenerated, no more
  `extends` of `tsconfig.base.json` there). Tests `M9-HOST-01..14`, `M9-TSC-01..03`, `M9-API-1330..1332`. Follow-ups:
  - [ ] 2D-world content on the GLES path: shapes, sprites, text meshes and the camera's clear drawn through the UI draw list
    (or a small GLES 2D pass), so `world2d` programs start on GLES too (today they go to wgpu like `world3d`).
  - [ ] wgpu failing at start (no Vulkan / DX12 device) falls back to the GLES path for `ui` games, and says so.
  - [x] Remove the root `tsconfig.base.json` (2026-10-29, with `js/`; `editor/` has its own tsconfig) together with the packages that still extend it (`js/editor-ui`, `editor/`;
    `js/engine`, `js/react` and the `tests/fixtures` JS are already gone); the examples and `tools/bench/crowd` no longer use it.
  - ANGLE no longer needs scoping to one binary (the old `ui_app_only` idea): there is one host.
- [x] **A CPU executor of the frame plan** (2026-11-05, asked for by Fluid: apps on a software rasteriser without Mesa):
  `platypus_render_cpu::plan::PlanRenderer` runs the same `FramePlan`, glyph cache and image cache as the GLES executor with
  seven kernel families compiled ahead of time (fill, shadow, glyph, image, transform, composite, blur; `plan::kernels`),
  plain Rust vectorised per target (x86-64 SSE2 and AVX2 + FMA chosen at run time, NEON on arm64 and armv7 with `+neon`),
  drawn in row bands by one pool per process (at most 4 threads, fewer on fewer cores). Within 2/255 of GLES on its scenes;
  blurs past radius 8 take three box passes (within 4/255). The vello renderer moved behind the default feature `vello`.
  Tests `M2-KRN-01..12`. Follow-ups:
  - [ ] `render_over` (a canvas seeded from the target), for a host that draws over a compositor's windows.
  - [ ] `platypus-app --renderer cpu` choosing the plan executor (today it's the vello one).
  - [x] The scene counts (2026-10-28): a `GameObject`, `Transform`, scene or prefab (file or API), or a component class
    (`extends Component`, a lowered `register(..)`, found in the parsed code) makes a program `world2d`; the hierarchy
    helpers are lowered only when called. `hello` now logs from a timer (stays `ui`); `physics` became `world2d` (rapier
    bodies live on GameObjects). Tests `M9-HOST-15..18`.
  - [x] UI memory on GLES (2026-10-28): no D3D debug layer (ANGLE enabled it: mozangle builds without `NDEBUG`), a
    config without depth/stencil/samples, programs and the colour glyph page made on first use, one driver trim
    a second after the first frame, a native game's source dropped once compiled, `PLATYPUS_MEM_TRACE`. Private
    working set (Task Manager's Memory) css 41.6 -> 29.5 MB, launcher 45.2 -> 34.8, md-reader 60.4 -> 45.5; private
    bytes css 134 -> ~100 (the D3D11 device alone is ~60). `docs/rendering.md` "Memory". Tests `M2-GLES-14..15`.
    - [ ] A glyph mask page that starts at 512 px and grows (~3-5 MB private on GLES; the atlas is shared with wgpu).
    - [ ] Program binaries cached on disk (`glGetProgramBinary`): ~0.7 s less start-up, no memory change measured.
    - [ ] DirectComposition surface (`EGL_DIRECT_COMPOSITION_ANGLE`; not important now): ANGLE would build a swap
      chain for composition and attach it to the window through a DirectComposition visual, so the compositor blends
      it directly instead of keeping the extra window-sized surface of an HWND-attached swap chain. Measured ~4 MB
      less private bytes (of ~100), which is why it is parked: the Memory column is already 23-46 MB. Worth doing
      later for transparent or shaped windows and possibly smoother resizing. Before adopting: check resizing and
      the first-frame flash by hand, alpha, screenshots, and that mozangle's ANGLE snapshot exposes the extension.
- [ ] **M9.b2** `platypus_ts`: ahead-of-time TypeScript compiler (oxc → typed Platypus IR → Cranelift → object
  linked into the game exe). Spike done (`tools/`, `docs/research/platypus-ts-spike.md`): the zombie
  game's `sim.ts` compiles, 13/13 ported tests pass, ~50x QuickJS, a precise GC and closures work.
  **Progress and the next task are tracked in `tools/platypus_ts/TODO.md`** (update it every step).
  Rule for this list: a box is ticked only when the whole item is done. Finished parts of an open item
  are ticked sub-items. **STOP** marks an item big or risky enough that we write down the design and
  review it with the user before starting, not after.

  What real gameplay code needs, counted in `js/engine/src` (11,500 lines), the examples and the zombie
  game (`docs/research/platypus-ts-spike.md` section 11): classes extending `Component` with `@property`
  decorators, `this` (1,400 uses), getters/setters (210), `??` and `?.`, spread (96), `Map`/`Set` (64),
  template literals (84 in the engine, 6 to 17 per example), string methods (51), `try`/`throw` (22 / 25),
  `instanceof`/`typeof` (94), `switch` (7), a few `async`. React/JSX appears only in each game's UI file.

  - [ ] **Type sidecar**: types from the real TypeScript checker instead of our own inference
    - [ ] Bun script using the `typescript` API: per source span, the resolved type of every expression and
          symbol, as JSON; fail the build on `tsc` errors
    - [ ] Schema and the mapping from TS types to IR types (unions, literal types, generics instantiations,
          `satisfies`, `as const`, `readonly`)
    - [ ] Replace annotation reading and bottom-up inference in `lower/` with the JSON; keep a test that the
          zombie `sim.ts` lowers to the same IR both ways
    - [ ] Scope-resolved capture analysis (today: by name, over-boxes)
    - [ ] Incremental: cache by file hash
  - [ ] **GC** (the collector works and is tested; not done)
    - [x] Precise non-moving mark-sweep, size-class free lists, per-thread heaps with heap ids
    - [x] Roots: shadow-stack frames with spill temporaries, globals, host handles; closure cells and
          environments traced
    - [x] Stress mode (collect at every allocation, poison freed memory) and tests that broke the root
          analysis (`M9-TS-27..33`)
    - [ ] Check the root assumption instead of trusting it: an IR verifier (debug builds) that no pointer
          SSA value is live across an allocating operation without a frame slot
    - [ ] Cheaper frames: no runtime call per push/pop (single script thread: a global top pointer, or a
          register); skip frames for functions that cannot allocate
    - [ ] Escape analysis: frame-scoped temporaries (`filter` results, `events` arrays, `{...gun}`) skip
          the heap
    - [ ] Pauses: measure a collection on a 50-100 MB script heap; if long, generations or incremental
          marking. **STOP** if the measurement says so
    - [ ] Return memory to the OS (chunks are only reused today); a frame-arena reset API for hosts
    - [ ] Enforce or document thread affinity of script objects
  - [ ] **Closures** (core done and tested; not complete)
    - [x] Arrow/function expressions as values, shared mutable cells, per-iteration bindings, nesting,
          recursion via annotated variable, calls through fields/call results/elements, top-level functions
          as values, host-held closures, GC-traced
    - [x] Mutable **module-level variables** (`let demo: SineDemo` assigned from functions and closures):
          data slots with GC roots, `let` never folded, `let x: T | undefined;`, closures in globals,
          the spill rule extended (`M9-TS-47..56`). Still open: a global read by an initialiser that
          runs before a later declaration, and sharing a module's variables between threads
    - [ ] Named function expressions that refer to their own name
    - [ ] Closures that use `this` (done for arrows and bound methods, `M9-TS-137`) / `arguments`
    - [ ] Destructured, default and rest parameters of closures (after those features)
    - [ ] Arrow functions as class fields (done, `M9-TS-137`) / object-literal methods
    - [ ] Capture by value when a variable is never reassigned after the closure is made (no cell)
    - [ ] Functions in generic positions (after Generics)
  - [ ] **Classes**: **STOP, design first.** Gameplay code is classes: `extends Component`, `@property`,
        `this`, `override`, getters/setters, statics, `instanceof`
    - [x] Design note written: `docs/research/platypus-ts-classes.md` (layout, calls, constructors, members,
          engine boundary, `@property`, sub-tasks)
    - [x] Answers recorded (section 12 of the note): closed world, per-method tables, structural-typing
          note first, scope cut as recommended; the engine becomes native with `.d.ts` declarations,
          so `Component`/`Vector3` are extern classes, not compiled TypeScript. Plain classes may start;
          inheritance from an extern class waits for the engine-boundary note
    - [x] Plain classes: fields, constructor, methods, `this` (`M9-TS-111..119`)
    - [x] Inheritance, `super(...)`/`super.m()`, virtual calls, `override`, abstract classes (`M9-TS-120..136`; extending an engine class waits for the manifest)
    - [x] Getters/setters, statics, private fields (`#x`, `private`); static accessors are open
    - [ ] `instanceof` (done: `M9-TS-124`), `typeof` (done, `M9-TS-199..205`), class expressions
    - [x] Decorators: `@property` as compile-time metadata, a generated `PROPS_<Class>` table (`M9-TS-139..140`); options and other decorators are open
    - [ ] `Component` and the `register()` / spawn / lifecycle path calling compiled classes
          (joins Direct engine calls)
  - [ ] **Generics**: **STOP, needs the type sidecar first** (inference of type arguments is the
        checker's job)
    - [ ] Generic functions by monomorphisation (explicit, then inferred type arguments)
    - [ ] Generic interfaces/type aliases/classes, constraints, defaults
    - [ ] Generic helpers over `Array<T>`/`Map<K, V>` signatures; variance is not checked (tsc did)
    - [ ] Code-size budget and a limit on instantiation depth
  - [ ] **Structural typing**: **STOP, design first.** Structs are nominal by shape today; TS lets any
        object with the right fields be passed (`Vector3Like`, interface subsets, object literals of
        another interface)
    - [ ] Choose: copy-coercion at the boundary, field accessors by name, or interface vtables; document
          the cost
    - [ ] Unions of different interfaces/classes (discriminated by a field), not only inline object types
    - [ ] Optional properties and index signatures (`Record<string, V>`)
  - [ ] **Strings**: **STOP, UTF-16 vs UTF-8 decision first** (JS `length`/indexing are UTF-16)
    - [x] Design note: representation, `length`/indexing/`charCodeAt` semantics, interop with Rust `String` (`docs/research/platypus-ts-strings.md`: WTF-8 storage, exact JavaScript behaviour)
    - [x] `string` values: literals, `+`, `===`/`<`, `.length`, template literals (`M9-TS-162..186`, including strings across the engine boundary)
    - [ ] Number to string exactly as JS (shortest round-trip, `toFixed`, `toString(radix)`), `parseFloat`,
          `Number()` (done except `toString(radix)` of fractions, `toPrecision`, `toExponential`; `M9-TS-173..177`)
    - [x] Methods: `slice substring indexOf includes startsWith endsWith split join trim replace
          padStart padEnd toUpperCase toLowerCase charAt at` (`replace` and the other regex methods with `RegExp`, `M9-TS-232..239`; `localeCompare`/`normalize` with Intl, `M9-TS-254..263`)
    - [x] `Map` and `Set` proper (hash by number, string, object identity; insertion order; iteration) (`M9-TS-206..214`; not done: `new Map(map)`, map spread, `groupBy`)
    - [x] `JSON.stringify/parse` for plain data, typed from the program's types (`M9-TS-215..221`; a dynamic `JsonValue` is open)
  - [ ] **Language features** (each small to medium; tick each)
    - [x] `for(;;)` with update clause, `do...while`, labelled `break`/`continue` (`M9-TS-57..63`)
    - [x] `switch` (numbers, string-literal unions), enums (`enum`, `const enum`) (`M9-TS-64..70`)
    - [ ] Destructuring
      - [x] Objects and arrays in declarations, parameters, `for...of` and callback parameters; nested; defaults (`M9-TS-71..76`)
      - [ ] Rest elements (`{ a, ...r }`, `[a, ...r]`: done, `M9-TS-199..205`), assignment to existing variables (`[a, b] = [b, a]`), computed keys
    - [x] Default, optional and rest parameters (`M9-TS-77..84`; a function with them used as a value needs all its arguments)
    - [ ] Spread
      - [x] Array literals, call arguments into a rest parameter, object literals with overrides
      - [ ] Into fixed parameters (`f(...xs)`), of maps and strings
    - [ ] Operators
      - [x] `??=`, `||=`, `&&=`, `**`, `**=`, unary `~` (`M9-TS-85..91`)
      - [ ] Optional call `f?.()`, `in` (done, `M9-TS-199..205`), `void`, `delete` (limited)
    - [x] `try`/`catch`/`finally`/`throw`: design in `docs/research/platypus-ts-stdlib.md` (approved: checked returns in
          may-throw functions only), built with the `Error` classes from a prelude (`M9-TS-187..198`); not done:
          `error.stack`/`cause`, throwing non-Error values, `AggregateError`
    - [x] `async`/`await`/Promises and timers (design in `docs/research/platypus-ts-generators-and-proxy.md`, approved): built on stackful
          coroutines with a microtask queue and timers in the runtime (`M9-TS-408..430`). The host must call `ts_rt_microtasks_run()` after
          every call into script and `ts_rt_timers_advance(now_ms)` once per frame (needs `platypus_app`, not done); also done: `Symbol`
          values, iterators and generators (`M9-TS-376..407`), `ArrayBuffer`/`DataView`/views, `structuredClone`, `WeakRef`,
          `Intl.Segmenter/Locale/DisplayNames`, `Temporal` (ISO calendar) (`M9-TS-431..515`); detail in `tools/platypus_ts/TODO.md`
  - [ ] **Standard library, native** (`docs/research/platypus-ts-stdlib.md`; rule: JavaScript's exact results, compared with
        bun; what is not built is a compile error). Detail and open gaps are in `tools/platypus_ts/TODO.md`
    - [x] Tier 1: errors and `try`/`catch`/`finally`, `typeof`, `Object.*`, spread/rest, `Array` gaps, `Number`, `Math`, `console` (`M9-TS-187..205`)
    - [x] Tier 2: `Map`/`Set`/`WeakMap`/`WeakSet`, typed `JSON` (`M9-TS-206..221`)
    - [ ] Tier 3
      - [x] `RegExp` (the `regress` crate): literals, `new RegExp`, `test`/`exec`, flags, named groups, and `String.match/matchAll/search/replace/replaceAll/split` (`M9-TS-222..239`)
      - [x] `Date` (`jiff`): constructors, `Date.now/UTC/parse`, getters and setters (local and UTC), ISO and legacy parsing, `toISOString/toJSON/toUTCString/toDateString`, `toString/toTimeString` with zone names, arithmetic (`M9-TS-240..253`, `M9-TS-276`)
      - [ ] `Intl` (ICU4X, compiled data for every locale)
        - [x] I1 `normalize`, `localeCompare`, `Intl.Collator`, `toLocaleUpperCase/LowerCase` (`M9-TS-254..263`)
        - [x] I2 `Intl.NumberFormat`, `Number.toLocaleString` (`M9-TS-264..275`; not: `style: unit`, compact currency, `formatToParts`)
        - [ ] I3 `Intl.DateTimeFormat`, `toLocale*String`, component options: decided 2026-10-19 (`docs/research/platypus-ts-intl-datetime.md`): ICU4X now with lightweight main-market data, `rust_icu` as an optional `icu-full` build (spike first)
          - [~] I3a on ICU4X field sets: `dateStyle`/`timeStyle`, `toLocale*String`, `Intl.DateTimeFormat.format`, component options (`M9-TS-277..286`); open: `formatToParts`, `formatRange`, `resolvedOptions`, `era`, `dayPeriod`, other calendars
          - [ ] Lightweight baked data (`icu4x-datagen`) for the main-market locale list
          - [ ] (optional, later) `icu-full` feature (`rust_icu`/ICU4C): spike on Windows, then the backend
        - [~] I4 `PluralRules`, `ListFormat`, `RelativeTimeFormat` done (`M9-TS-296..312`); open: `Locale`, `DisplayNames`, `Segmenter`
        - [ ] The game's locale list from `package.json` into `ts_rt_intl_set_locales` (needs `platypus_app`)
    - [ ] Tier 4 (asked for): `encodeURI`/`decodeURI` family and `AggregateError`/`cause` are done (`M9-TS-287..295`); iterators and generators (done, `M9-TS-376..393`; design in `docs/research/platypus-ts-generators-and-proxy.md`), `Symbol` values, typed arrays / `ArrayBuffer` / `DataView`,
          `Error.stack`/`captureStackTrace`; `BigInt` is done (`M9-TS-360..369`), `Temporal`, real `WeakRef`/`FinalizationRegistry`,
          `Promise`/`async`/timers (**STOP**), `Proxy`/`Reflect` (**STOP**), `structuredClone`, `globalThis`, bytes across the engine boundary
          (`btoa`/`atob`, `TextEncoder`/`TextDecoder`)
  - [ ] **Library surface** the compiler knows
    - [x] `Math`: `ceil round trunc pow exp log log2 log10 tan asin acos atan cbrt fround clz32 random` (`M9-TS-85..91`); `sinh cosh tanh log1p expm1` and the rest in `M9-TS-199..205`
    - [ ] Arrays
      - [x] `pop shift unshift slice splice concat reverse sort indexOf lastIndexOf findIndex reduce fill at`, `length = n`, `Array.from/of/isArray`, `new Array(n)` (`M9-TS-92..110`)
      - [x] `join` (needs strings), `flat`, `flatMap`, `entries`, `keys`, `findLast`, `reduceRight`, `sort()` without a comparator (string order) (`M9-TS-199..205`; not done: `toSpliced`, `with`, `copyWithin`)
      - [ ] `for...of` over `Map`/`Set` (done, `M9-TS-206..214`)/strings
    - [~] Typed arrays beyond `Uint8Array`: all nine element kinds done (`M9-TS-313..359`); `ArrayBuffer`, `DataView` and shared views are open
    - [ ] `Object.keys/entries/values/assign` and the `Number` statics (done, `M9-TS-199..205`), `Date` (done, `M9-TS-240..253`, `M9-TS-276`), `performance` (open)
  - [ ] **JS semantics conformance**: differential tests against bun on generated and real code
    - [ ] A harness that runs every tested module (zombie `sim.ts` first) in bun and native and compares
    - [ ] `-0` from `%` and rounding, bit operations beyond 2^63, `null === null` of two nullable numbers,
          `==`, NaN payloads vs the null NaN, integer-valued fast paths
    - [ ] Random program generator for the supported subset
  - [ ] **`JsValue` fallback**: **STOP, decide whether it is needed at all** (the hybrid plan keeps
        untyped code on QuickJS)
    - [ ] Decision note: what must fall back (`any`, `unknown`, computed property access, `eval`-like) and
          what simply stays in QuickJS
    - [ ] If needed: NaN-boxed value, dynamic object model (shapes or hash maps), property get/set by name,
          calls between native and QuickJS, GC handles across the two heaps
  - [ ] **Direct engine calls**: **STOP, design first.** Biggest payoff, biggest surface. Direction set by
        the user: the engine is native and TypeScript sees it only as declaration files (`.d.ts`); scripts
        call native functions directly (maths, `Date`, scene, physics, audio); QuickJS stays as an option
    - [x] Design note written: `docs/research/platypus-ts-engine-boundary.md` (declared handle / value / singleton /
          extern-base classes, manifest generated from Rust, class table and hooks, f32, what `js/engine`
          becomes module by module, QuickJS bindings from the same manifest). Answers recorded in its
          section 13 (all as recommended; the whole engine is ported to Rust; generics-as-intrinsics
          still open)
    - [ ] `#[ts_api]` + manifest: generate the `.d.ts`, `extern "C"` shims and QuickJS bindings from
          annotated Rust (replaces `__platypus_op`; speeds up QuickJS too)
      - [x] First slice: `crates/ts_api` + `ts_api_macros`: scalars, bools and handles, manifest, `.d.ts`,
            JSON, `extern "C"` shims, typed QuickJS bindings (`M9-API-01..06`); additive
      - [x] Value classes (`Vector3`) scalarised across the boundary (`M9-API-10..11`, `M9-TS-147..150`) and the engine context: a thread-local current world (`M9-API-07..09`)
      - [ ] Strings (done, `M9-API-21..22`), methods on handles (done, `M9-API-16..19`), value-class methods as intrinsics (done, `M9-API-20`), QuickJS bindings for values and strings (done, `M9-API-23..26`; handle classes in QuickJS open)
      - [x] `platypus_scene` annotated: the `Scene` namespace (handles, `Vector3`/`Quaternion`, hierarchy, flags), driven by compiled TypeScript (`M9-API-12..15`, `M9-TS-151..155`); `GameObject`/`Transform` handle classes (`go.transform.position = v`, `M9-API-16..19`, `M9-TS-156..158`); not yet wired into `platypus_app`
      - [ ] Annotate `platypus_physics`, `platypus_audio`, `platypus_script`; wire `enter` into the app; retire the ops they replace
    - [x] Compiler: extern table from the JSON manifest, `Handle` type, namespace calls with scalar arguments and f32/i32/u32/bool conversions (`M9-TS-142..146`)
    - [~] Compiled `Component` with hooks driven by a Rust host (`NativeHost`, `register(X)`, class table; `M9-TS-556..561`); `Input` axes and `platypus_app` wiring open
    - [ ] First slice: `Time`, `Mathf`, `Vector3`, `Transform`, `Input` axes and a compiled `Component`
          with `onUpdate`, driven by `ComponentHost` with no JS engine in the loop
    - [ ] Inventory the `__platypus_op` names and shapes (`js/engine/src`) and what each passes
    - [ ] Value types with shared layouts: `Vector2/3/4`, `Quaternion`, `Color`, `Matrix` are TS classes in
          `math.ts` today; decide: compile `math.ts` natively, or intrinsics over glam types (f32 fields:
          the IR has only f64 numbers, so a layout/annotation for f32 is needed)
    - [ ] Generate `extern "C"` bindings from one op table, used by the engine and the compiler
    - [ ] Handles and ids (`GameObject`, `Transform`, ...), strings and arrays across the boundary
    - [ ] Callbacks from Rust into native script (`__platypus_*` hooks) through a function table
    - [ ] `Mathf`/vector arithmetic as inline intrinsics
    - [ ] Component lifecycle without QuickJS (joins Classes)
  - [ ] **UI without React**: **STOP, design first.** A small component/effect library (Preact or our own
        remake) over a native UI tree, written in the compiled TypeScript subset; no React reconciler, no
        OneJS-style embedded engine for the common case
    - [x] Generics in `platypus_ts` (functions, classes, methods, interfaces, aliases; `M9-TS-562..574`)
    - [x] Step 3 of the migration: compiler side for TSX counters, lists, effects, memo and refs (`jsx.rs`, `ui.ts`, `platypus_ts_ui` bridge; `M9-TS-575..579`); also `ref=`, context, `useReducer`, `useImperativeHandle`, `useSyncExternalStore`, `memo`, `forwardRef` (`M9-TS-580..586`); controlled text fields (`M2-NAT-13`), hot-reload data and `Runtime::refresh` (`M9-TS-587..588`); updater setters, dynamic style objects and spread (`M9-TS-589..591`); next: `platypus_app` wiring (another session), the dev loop
    - [x] Step 2 of the migration: `platypus_ui_native` reconciler core with Rust closures (hooks, keyed diff, effects in React's order, portals, events; `M2-NAT-01..12`); next: compiler side (JSX, hook intrinsics), then events/refs wiring, then `memo`/`forwardRef`/`useReducer`
    - [x] Step 1 of the migration: typed `Op` enum and one applier (`M2-UI-OP-01`)
    - [x] Design note written and approved (answers: portal events follow React, non-boolean `&&` is a compile error, `memo`/`forwardRef`/`useReducer` last, generics added to `platypus_ts`): `docs/research/platypus-ui-native.md` (user decision: hooks kept, native reconciler with re-render semantics and element descriptions, portals, one `Op` applier; React stays until it exists)
    - [ ] Design note: what of React is used (components, state, effects, events, refs), signals-style
          effects versus a vdom diff, how the library compiles under `platypus_ts` (it must stay inside the
          typed subset), the native UI tree API (`platypus_ui` opcodes today), styles and CSS
  - [ ] **Nothing left in TypeScript but generated types** (user request 2026-10-19): animation, scenes, math, physics and audio components, app/time/input ported to Rust behind `#[ts_api]`; games without `node_modules`; plan awaiting review in `docs/research/platypus-native-engine.md` (**STOP**: order and the type-checking question)
  - [ ] **Dev loop without Bun**: **STOP, design first.** Hot reload with the compiler as the watcher
    - [ ] Design note: Cranelift JIT in-process for development and AOT for release from the same IR,
          function-level swap, state kept through the `@property` reflection table, type checking via
          a code server or the language service, the type files
  - [ ] **Hybrid release builds**: **STOP, design first**
    - [ ] Module partition: which files compile natively (manifest list or a directive), fallback to QuickJS
          per module when the compiler rejects a file, with the reason printed
    - [ ] Calls both ways between native modules and QuickJS (components in either)
    - [ ] Scene save/restore, hot reload and `@property` reflection in release builds (or declared absent)
    - [ ] `platypus build --aot` (or similar) driving oxc + Cranelift + linking into the game exe; React/UI
          stays on QuickJS
  - [ ] **Performance** (after correctness)
    - [ ] Integer loop counters / range analysis instead of f64 indexes; bounds-check elision
    - [ ] Inline allocation fast path; scalar replacement of small non-escaping objects
    - [ ] Avoid the frame call per function; inline small functions; devirtualise
    - [ ] Re-run `bench`/`sine_bench` after each and record in the spike doc
  - [ ] **Cross targets and CI**: one sub-item per platform, each tested on the platform
    - [ ] Linux x86_64 (ELF), Linux aarch64
    - [ ] macOS x86_64 / aarch64 (Mach-O)
    - [ ] Android aarch64 (NDK link)
    - [ ] iOS aarch64 (static link into the Xcode wrapper; AOT is the reason it can work at all)
    - [ ] Windows aarch64
    - [ ] Debug info / source maps for crashes; unwind tables where the platform needs them
    - [ ] CI job building and running `tools/` tests on each host
- [ ] **M9.c** Release packaging for macOS, Linux, Android and iOS (iOS: no JIT, so the game must be AOT-compiled and linked in
  (the QuickJS backend this once relied on was removed); Xcode wrapper)

## M10 — Optional extras

- [ ] OpenXR: Quest VR, PC VR
- [ ] Steam Audio spatial sound
- [ ] Ray tracing tier
- [ ] Video: a `VideoClip` asset and `VideoPlayer` component (to a texture, UI or a plane)

## Platform plan: app shell and web APIs (from docs/difference-to-electron.md)

Planned, not started. Order may change.

- [ ] **App shell:** a window, menus, dialogs and the UI tree without a game world; React app
  entry point and the `@platypus/api` TypeScript surface (no IPC bridge)
- [ ] **Measure:** disk and RAM of a small app shell against a comparable Electron app, before
  making any public claim about size or speed
- [ ] **Files and fetch** (M6 Script IO), **WebSocket** (M6, also for M8)
- [ ] **Canvas API:** a 2D drawing API close to the browser's canvas, on `LineRenderer2D`,
  `Shapes` and `TextMesh`
- [ ] **Web Audio-like graph** on kira: gain, filters, analysers
- [ ] **WebAssembly** modules loaded from scripts (see M9.b)
- [ ] **Storage:** key-value stores for app data
- [ ] **Security design doc** for the app shell (isolation without Chromium's sandbox)
- [ ] **Licence:** choose and write `LICENSE` before the first release (see difference-to-unity.md)
- [ ] **API stability policy:** exported names, script ops and the scene format are public;
  deprecation period and migration notes before any breaking change

## Watch list: pitfalls from mature engines

Not tasks yet. Known areas where mature Rust/wgpu engines (Bevy in particular) have had trouble,
so we can check Platypus for them early. Each item says what to verify. Confirm each against Platypus's
code and the upstream issue before acting on it. See [docs/difference-to-bevy.md](docs/difference-to-bevy.md).

- [ ] **Renderer:** shadow acne, peter-panning and cascade seams in sun shadows (M5-RND-*); check
  bias on slopes and at cascade edges.
- [ ] **Renderer:** transparency sorting and see-through overdraw; check that layers and sorting
  stay stable when objects move.
- [ ] **Renderer:** sRGB vs linear mistakes in textures and UI blending; check colours in a
  golden for each texture format.
- [ ] **Renderer:** HDR, bloom and tonemapping shifts; check exposure does not change when the
  window is resized.
- [ ] **Renderer:** shader compile stutter on first use of a material; measure the first frame of a
  new material and consider a warm-up step.
- [ ] **GPU / drivers:** resize and frame-pacing jitter on DX12 and Vulkan (`PLATYPUS_FRAME_LATENCY`),
  and crashes that depend on the adapter. Keep the NVIDIA hard-reset note in `CLAUDE.md` in mind.
- [ ] **Assets:** async load races and stale handles after asset hot reload; check that a reload
  replaces every user of an asset.
- [ ] **Assets:** scene and prefab file format changes between versions; add a version field and a
  migration path before the format is public.
- [ ] **Text:** glyph atlas eviction under many fonts or sizes; fallback fonts, bidi and IME
  composition on each platform.
- [ ] **Input:** gamepad mapping differs per controller (gilrs); check button and axis names on at
  least one Xbox-style and one generic pad.
- [ ] **Input:** key repeat and focus loss leaving keys or buttons stuck down; check on
  focus/background.
- [ ] **Physics:** the fixed-step "spiral of death" when a frame takes too long; cap the catch-up
  steps.
- [ ] **Physics:** fast bodies tunnelling through thin colliders; check CCD on bullets and falling
  objects.
- [ ] **Physics:** results differing between platforms or runs; don't rely on bit-exact physics
  until M8 needs it.
- [ ] **Audio:** device switches, sample-rate changes and latency; check the mixer after the
  default output device changes.
- [ ] **Audio:** many simultaneous sounds causing clicks or dropped voices; test with a stress
  scene.
- [ ] **Memory:** growth over long sessions from script allocations, texture caches and the SVG
  cache; check with the arcade running for an hour.
- [ ] **Scripts:** React state or effects leaking across hot reloads; test Fast Refresh and the
  state-keeping reload together.
- [ ] **Build times:** Rust compile time growing as crates grow; track `cargo build` times in CI
  once it exists.
- [ ] **API churn:** our own public API changing often; keep a changelog once the Unity-style API
  has users outside this repo.

## Later

- [x] `examples/launcher`: a game launcher mock-up, to see whether a store / launcher front end
  works in Platypus's UI. A cover-flow carousel of the examples' screenshots (`gen-games.ts` copies
  the `screenshots` tool's renders at build time) with CSS 3D transforms and transitions, browsed
  with the mouse (arrows, clicks, wheel), keys or a gamepad (d-pad, bumpers, left stick); a
  details panel (title, folder, `package.json` description) with Play, a loading spinner, then
  an alert that launching isn't implemented.
  - [x] A grid view (G / pad Y, remembered in `PlayerPrefs`): tiles rise in row by row, the
    selected one lifts; arrows / d-pad / stick move in 2D and keep it scrolled into view
  - [x] Button prompts follow the last device used, as PC games do (keys and mouse show
    keycaps and the cursor; a pad hides the cursor and shows Xbox, PlayStation or Nintendo
    icons from its name; Nintendo pads confirm with A, the right-hand button)
  - [x] Engine fix found on the way: minified function lists (`translateY(0)scale(1)`) didn't
    parse; stylesheet values now get a space after a `)` that runs into a word (`M3-CSS-13`)
  - [x] Pads count as the input device as soon as they're plugged in (and the keyboard again
    when the last one goes); pads plugged in before Platypus started are announced on the first
    poll (`M3-PAD-02`)
  - [x] A sidebar menu (Menu / Esc / the ☰ button: library, view, settings, Exit) and a
    context menu on a game (Y / C / right-click: Play, Details, favourites, Show in folder);
    the view toggle moved to R3 / G. Favourites are kept in `PlayerPrefs`
  - [x] Engine: smooth scrolling (`behavior: "smooth"`, `scroll-behavior: smooth`), eased over
    250 to 500 ms (`M3-EVT-09`); the grid keeps its selection in view with it, and fades out
    at its top and bottom edges under the header
  - [x] A Favourites category for both views (tabs, LT / RT, Tab, the sidebar), with an
    empty state; the grid has room under its fades, and its scrollbar stays unfaded
  - [x] A background in each game's colours (palette.ts reads them from its screenshot at
    build time): a gradient with two drifting glows that eases between games; after 3 s on
    a game its picture fades in, and fades out over 1 s when the selection moves on
  - [x] Engine fix: with any `:outro` rule in use, an element removed mid-transition (a
    caption replaced while browsing fast) stayed, in layout, until that transition ended;
    only motion the exit starts holds it now (`M3-ANI-05`)
  - [x] Backgrounds stay below the text's brightness: grey or white pictures borrow their
    accent's hue, tones are capped lower, bright pictures fade in fainter, titles get a shadow
  - [x] Engine: text that is only scaled and moved is rasterized at its on-screen size, so
    pop-ins and lifted tiles stay sharp, with no pop at the end (`M3-XF-03`)
  - [ ] Engine: gradients draw only 8 stops and warn every frame about the rest; warn once
  - [ ] Actually launch a game (start `platypus <folder>` as a child process, or switch games in
    place) and return to the launcher when it quits
- [~] `examples/md-reader`: a Markdown reader for Platypus's own docs, drawn by Platypus's UI. Its own
  plan and progress: [examples/md-reader/TODO.md](examples/md-reader/TODO.md). Engine work it
  needs:
  - [x] Spans hit-tested inside text (`M3-EVT-07`); an exact layout size cache (`M2-LAY-07`);
    drawing culled to scroll clips (`M2-DRW-09`); unchanged line breaking skipped
  - [x] A jump made in the first frame landed off: `platypus_app` applied scripts' output (layout,
    `scrollIntoView`) before sizing the UI to the window, so the first frame laid out at 1x1.
    It now sizes the UI at the start of each frame too
  - [ ] Scrolling lays out again on every wheel tick: move the scrolled subtree's rects instead
  - [x] Extra command-line arguments passed to scripts: `platypus <game> -- ...` is `Application.args`
    (`M5-APP-11`)
  - [ ] Reading a folder outside the game while running (the reader's `--root`, which exits with
    an error for now): M6 plans only the `game` and `data` roots, so this needs a decision (a
    read-only folder the user names on the command line?)
  - [x] `scrollIntoView` takes the DOM's `{ block, inline }` (`start` / `center` / `end` /
    `nearest`, default `start` / `nearest`; `M3-EVT-08`); the reader no longer scrolls to the
    bottom first to put a heading at the top
  - [x] `Application.quit(exitCode)`: `platypus` exits with it (`M5-APP-12`)
  - [ ] Review: a `setTimeout(fn, 0)` set while timers run fires in the same frame (the
    prelude's loop takes any timer due by `now`, and new ones are due at once), so "wait a
    frame" needs a 1 ms delay. Browsers run it in a later task. Decide whether timers created
    during the timer pass should wait for the next tick (and whether a `requestAnimationFrame`
    belongs in the prelude)
  - [ ] Review: symbols missing from Inter (☰ ▸ ▾ ☑ ☐) drew as empty boxes instead of falling
    back to a system symbol font (Segoe UI Symbol on Windows); the reader draws its icons with
    CSS and SVG instead. Check fontique's fallback for symbol characters in `TextEngine`
- [ ] GIF images in `platypus_ui`: `image` elements and `background-image` only decode PNG, JPEG and
  SVG today (`The image format Gif is not supported`). Decode GIFs (the `image` crate's `gif`
  feature in `platypus_render`), then play animated ones with their frame timings (frames in the
  image atlas, a redraw only when the frame changes, paused when off screen); WebP likewise.
- [ ] A canvas UI element that embeds the 3D engine (a `<canvas>` / 3D view element for React UI):
  a box that shows a camera's view of a world (or its own world) and passes the pointer and keys
  through, so window apps written mostly in React can put live 3D anywhere in the layout — Platypus as
  an Electron replacement for desktop apps (one native binary, no browser, the same React UI).
  - [ ] The element: a camera (by id) rendered into a texture sized to the box at the device
    scale, drawn by the UI renderer in place, resized with layout, hidden views not rendered
  - [ ] Input: pointer and wheel in the box go to that view (camera rays, panel hits, the editor's
    Scene view uses the same idea), focus and keys while it's focused
  - [ ] Several views at once, each its own scene or a shared one; a 2D canvas mode for
    `LineRenderer2D` / `Shapes` / `TextMesh` drawing without a scene of your own
  - [ ] App shell for UI-first apps: no scene needed, window chrome and menus, file dialogs and
    tray (with M6 Script IO), packaging (M9)
- [ ] SVG on the GPU ("native" SVG): tessellate fills and strokes (e.g. lyon) into triangles drawn
  by the UI renderer, instead of rasterizing every change on the CPU with resvg (M3d 5.1), so
  animated and large SVGs stay cheap; per-shape hit testing (`pointer-events` on shapes). Then:
  - [ ] SMIL animation (`<animate>`, `<animateTransform>`, `<animateMotion>`, `<set>`), which resvg
    ignores
  - [ ] `d` path morphing between paths with different commands (normalize both to cubic curves
    with matching segment counts before interpolating)

## Native backend, render side (round 2, agent R)

- [x] Registries to the renderer: sprites, 2D lines, planes, mirrors, probes, LOD groups, skins, line renderers, particles, models, render textures, gizmos, `RenderSettings`, `QualitySettings`.
- [x] World panels (`createRoot(panel)`), `UiResources`, cursor, `Screen` requests, cloth, camera follow/orbit, animation update.
- [ ] glTF animation clips (`Model.animations`) as `platypus_animation` clips; animation events to components.
- [ ] Model colliders (`colliders: "mesh"`) need the physics hook; cloth over a model mesh needs the mesh data under the host's id.
- [ ] Panel events are untested without a window (a unit test of the routing would need an `App`).
- Differences seen in the screenshots: the timer of the world example is ahead of the QuickJS one (real time, not a render difference); the rope of `world` waits for the physics wiring.

## UI follow-ups (events and attributes, 2026-10-22)

Left over after the UI events and attributes work (`b116ac6`, `docs/ui-native.md`, `docs/input-and-events.md`):
- [ ] **`title` tooltip:** the attribute is stored and selectable (`[title]`), but nothing shows a tooltip: there is no tooltip UI. Add a hover tooltip (delay, placement near the pointer, clipping to the window, styling hook such as `::tooltip` or a `.tooltip` class, no tooltip while dragging or when `title` is empty).
- [ ] **`<input type>` behaviour:** `type` is stored lower-cased and matches `input[type=password]` selectors, but changes no behaviour and every input is a text field. Implement per type: `password` (masked text, no copy), `number` (digit filtering, step/min/max, spin keys), `checkbox` / `radio` (look, checked state, `onChange`), `range`, `color`, `date`/`time` if wanted, `search`, `email`/`tel`/`url` keyboard hints, `hidden`, `button`/`submit`. Keep React's controlled and uncontrolled semantics.
- [ ] **`pseudoElement` on transition and animation events** is always `""`: generated `::before` / `::after` boxes are not event targets, so events for them are reported on the owning element. Make generated boxes report their pseudo-element name (`"::before"`, `"::after"`) as in the DOM.

## UI inspector (2026-10-31)

Design: `docs/research/platypus-ui-inspector.md`. Opt-in at build time (`platypus-build build <game> --add-ui-inspector`); the release build keeps the fast path. Embedded in games and in the editor.
- [x] **Tree queries:** `UiTree::inspect_pick` / `inspect` / `inspect_set_style` (matched rules with winning declarations, computed on demand; `M3-INS-01..03`).
- [x] **Build flag (flag and bundle field):** `--add-ui-inspector` in `platypus-build` (`Options.ui_inspector`), `"uiInspector": true` in `game.pbundle`, read by `platypus-app`, which enables the API.
- [ ] **Build flag (the rest):** compile the TSX inspector in only with the flag; warn when `package` is used with it.
- [x] **`UiInspector` API (screen UI):** `#[ts_api]` namespace with scalar getters over a per-frame snapshot, `select` / `expand` / `setStyle`; always declared, inert unless the host enables it (`M3-INS-05..06`). Still to do: `setClasses`, a compiled-code test through the facade, and the TypeScript `d.ts` check (`generated_dts`).
- [ ] **Tree ids:** every call names a tree (screen UI, world panels, editor-hosted game UI).
- [x] **Overlay (first version):** `tools/platypus_build/inspector/overlay.tsx`, appended by `--add-ui-inspector` and mounted in a second screen root (`createRoot(9000001)`, `INSPECTOR_TARGET`); F12 toggles (off by default), F9 pins the hovered node; outline (padding box green, border box blue), label, and a card of matching rules with overridden declarations struck through. `M3-INS-08`. **Not seen in a window yet** (compiles; needs a manual run).
- [x] **Edit mode (first version):** F8 pins the hovered node and makes the card editable: a field per declaration and per `element.style` line (Enter commits, Escape reverts, empty removes), a `×` to drop an override, and an add-property row. Edits are inline overrides of the pinned node (a stylesheet rule's own text is not changed). Compiles; not seen in a window yet.
- [x] **Pick mode (click to pin):** F7; `UiInspector.pick(on)` makes the host swallow a mouse press (and its release) on the screen UI and report the node under it (`takeClick`), so no capture layer is needed and the hover pick stays clean (`M3-INS-09`). Not seen in a window yet.
- [ ] **Overlay, next:** margin box; write an edited declaration back to its CSS rule/file; undo; keys typed into a field also reach the game; the pick ignores the inspector's own card (it lands on the card when the pointer is over it).
- [ ] **Panel:** elements tree, Styles list (cascade order, overridden declarations struck through, in-place value edit, per-declaration toggle), box-model diagram; `useInspector()` for custom layouts.
- [x] **Editor tab (first version):** a dock tab "UI" (`editor/src/UiInspectorPanel.tsx`): the editor's own UI as a tree, the picked node's styles, an Edit toggle; shared parts in `tools/platypus_build/inspector/shared.tsx` (`M3-INS-10`). Needs the editor built with `--add-ui-inspector`. Not seen in a window yet.
- [x] **Checked with `--bench` screenshots** (a run with clicks, keys and mouse moves writes a picture per segment): the overlay's outline, label and card, the UI tab's tree opening down to a pinned node, and edit mode's fields (`platypus-app editor --project examples/arcade --bench --bench-input <spec> --bench-shots <dir>`).
- [ ] **Editor, next:** scroll the UI tab's tree to the pinned row (the shared `TreeView` only scrolls on user actions; the row can sit below the visible part); edits as undoable commands (`history.ts`); inspect the project's UI (the Game panel) rather than the editor's own (needs tree ids); an "open in a window" menu item (`Window.create`; the window API has not run on a real display yet); highlight a row's node under the tree's pointer; menu entry to build the editor with the flag.
- [ ] **Source locations:** rule file and line (`StyleRule.source`, only with the flag), `@media` condition, pseudo-element rules, computed values; write an edited declaration back to the CSS file.
- [ ] **Component tree (live component editing):** `Runtime` introspection (component names, props, hook state) behind the flag; JSX site of a host element; edits through hot reload.
- [ ] **Docs:** `docs/ui-native.md` / `docs/platypus-build.md` once the flag exists.
- [ ] **Game UI in the editor (design: `docs/research/platypus-react-pane.md`, 2026-11-06):** the UI tab inspects the editor's own UI; the user wants the game's (the Game view). In order:
  - [ ] Tree ids (`UiInspector.target` / `targets`, one tree per call; screen = 0, a `WorldPanel` id for a panel), a target menu in the UI tab, the editor's own roots marked as tool UI so picking in the Game view never lands on them, a camera-ray pick into the world panel under the pointer.
  - [ ] `File > New React Component`: `src/modules/<Name>.tsx` (+ optional `.css`, `.meta`), offers the import.
  - [ ] `GameObject > React Pane`: a `WorldPanel` with a `component` property (a dropdown of `src/modules` components), a placeholder until E11.
  - [ ] E11 (above) makes a pane render its component; the canvas UI element gives the Game view a screen UI to inspect.

## UI style editor (2026-11-05; direction revised by the user the same day)

Direction: **no JSX / TSX source editing** (too likely to go wrong). What is wanted is a tree viewer with collapsible nodes and a good style editor, in the inspector (overlay, the editor's "UI" tab, later a UI-only editor app), in light and dark. `docs/research/platypus-ui-editor.md` has the earlier source-editing study; it is parked. The VS Code extension is still wanted, but for **TS code execution and debugging** (the `ts-debugger` session's DAP work), not for UI editing.
- [x] **Value logic** (`tools/platypus_build/inspector/values.ts`, tests `M3-INS-11`): stepping a number, colours (hex / rgb / hsl, hsv for a picker), cubic-bezier, gradients, autocomplete filter.
- [x] **Smart value field:** Up / Down add one unit (Shift ×10, Alt ×0.1; first number, not yet at the caret); `calc()` / `var()` / relative units show `= 752px` beside the value (always, not on hover). Checked in screenshots.
- [x] **Colour picker:** saturation-value square, hue and alpha sliders, hex / rgb / hsl output (opens from the swatch; also inside gradient stops).
- [x] **Cubic-bezier editor** for `transition` / `animation`: draggable handles, presets, the `cubic-bezier()` text. Not yet: a moving dot preview.
- [x] **Gradient helper:** angle field and arrows, stops with colour swatch (opens the picker) and position, add / remove, live preview bar.
- [x] **Autocomplete limited to what is supported:** property names, keywords and class names complete from the engine's own tables (`STYLE_PROPS`, `UiInspector.styleProps` / `styleKeywords` / `classNames`); hover shows the property's doc as a tooltip. Not yet: units and `var(--…)` names.
- [x] **Layout info and flexbox editor:** `UiInspector.layout(id, name)`; a flexbox editor with a live preview and buttons. Not yet: the overlay drawing the flex axis / gaps over the page.
- [x] **State toggles:** force `:hover`, `:active`, `:focus`, `:focus-visible` (text links; ancestors too for hover and active). Not yet: `:disabled`, `:checked`.
- [x] **Classes:** links with a hand cursor (a click takes one off, a struck-through one comes back), `+` opens a pill input with completion.
- [x] **Rules you write:** a "new rule" field (`.card:hover { color: red }`) adds a real rule to the program's styles, pseudo-classes and pseudo-elements included; the rules typed are listed to edit or remove. Pseudo-element rules of the node show inline under "Pseudo ::before element" headings.
- [x] **Hover feedback:** the box model (margin, border, padding, content) over a hovered element; marching ants (1px dashes, animated by CSS) for a pinned element; while editing nothing over the node (it hid colours), only the box model of a hovered margin / padding / size line and ants round the elements a hovered rule matches (`UiInspector.highlightRule` / `highlightNode` / `showBox`), and the editor tab's rows highlight their element.
- [x] **Light and dark:** a stylesheet with colour variables (`theme.ts`: dark, light, system via `prefers-color-scheme`) and a theme pill (system / light / dark).
- [ ] **Light and dark, next:** preview the UI under edit in either scheme with `UiTree::set_media_env`.
- [x] **Simpler flow (2026-11-06):** F12 shows a toolbar (Inspect, `?`, theme, close). Inspect (F7) shows each element's box model and name under the pointer; a click pins it and opens the editing card at once (no F8 / F9). The card has a close ×; `?` lists the shortcuts. The host skips the inspector's own UI (anything under a `uii` root) when picking and hovering. Like Visual Studio's "select element".
- [x] **Styles in a CSS file:** `tools/platypus_build/inspector/inspector.css` is added to `game.css` by `--add-ui-inspector` (not part of the program, no `addStyleSheet`).
- [x] **Chrome-style new property:** `element.style { }` stays empty; clicking in it starts a line (name with completion, Enter or Tab to the value, Enter commits and starts the next line, Escape cancels).
- [x] **Toolbar polish (2026-11-06):** a dark bar at the top centre with SVG icons (grip, inspect = a box with a cursor over it, `?`, theme, a thin white ×); each icon names itself on hover with its shortcut (`Inspect (Ctrl+Shift+C)` as in Chrome, F7 too; `Close (F12)`); the grip drags the bar (move cursor, fist while held) and a card's title bar drags the card; controls are not text-selectable. Checked in screenshots.
- [x] **Card and property editing polish (2026-11-06):** the card has its own header (move grip, title to drag by, Copy styles, a hamburger menu, close ×) over a scrolling body and a fixed strip that shows the doc of the hovered property (a floating tooltip covered the next rows, and could not leave the scrolling card); the menu has Copy styles, Colour scheme ▸ Match toolbar / Light / Dark (the card's own, independent of the toolbar) and Detach to window (greyed: "soon"). `:` (and Enter / Tab) in a new property's name checks it against the engine's properties and moves to the value, or shows an error balloon (`bgx` is not a style property here. Did you mean …?); a pasted `color: red` is split; a property name is editable and Backspace through the value, then the name, removes the line (no floating ×). Copy styles puts clean CSS (kebab-case names, element.style then the rules, overridden ones commented) on the system clipboard (`UiInspector.copy`). Calculated values sit right beside their values and follow the layout (read again every frame). Swatches have a light edge inside a dark ring. Floating things (menu, balloons) are raised over later rows (z-index only orders within a group: the header is raised too). Checked in screenshots; the clipboard read back.
- [x] **Toolbar theme and help tables (2026-11-06):** the toolbar follows the theme (system, light, dark) like the cards; its icons take their colours from CSS classes (`.uii-ico-*`), which the engine applies inside inline SVG. The `?` help is two tables with equal columns (CSS grid) and a header row: KEYBOARD (what to press) and THE INSPECTOR (what each part does). Checked in screenshots.
- [x] **Field padding (2026-11-06):** the text box keeps its spare room under the glyphs, so a field's padding is above them (2px top, none below, 2px sides) and the new-class pill has 3px on top; completions list shorter words first (`color` before `columnGap`).
- [x] **Long values:** a value wider than its field is edited in a wrapping `TextArea` (Enter commits), so a two-part `transition` shows in full.
- [ ] **Still open:** the doc strip is not in the editor's UI tab yet beyond the strip itself; the card is not clamped to the window when dragged; typing a value that is not valid for its property gives no error yet; a moving-dot preview in the curve editor; units and `var(--…)` completion; Up / Down at the caret (not the first number); the flex axis / gaps drawn over the page; `:disabled` / `:checked` toggles; undo for the editor tab's edits; editing a rule's declaration writes an inline override, not the rule.
- [ ] **Standalone UI editor app** (`ui-editor/`, UI-only, GLES tier): the tree and the style editor over a game built with the inspector (a local socket, or later the canvas UI element).
- [ ] **VS Code extension for TS execution / debugging:** with the `ts-debugger` session (DAP server and attach exist); not part of this work.

## web-ui: TypeScript and UI as a library (2026-11-05, branch `web-ui-split`)

Design and decisions: `docs/research/web-ui-split.md`. The library lives in `..\web-ui` (its own repo); Platypus becomes a web-ui app.

What would have been a Platypus app becomes a web-ui app: a TS/TSX/CSS program plus the Rust plugins it links. Platypus is then one web-ui app (the engine is its plugins). Move this list to `..\web-ui\TODO.md` with the crates (step "Move").

### Part 1: the split

- [x] **Rename:** `platypus_api` -> `ts_api` (crates `ts_api`, `ts_api_macros`; `#[ts_api(..)]`; `ts_api_result*`).
- [ ] **Host extension:**
  - [x] The component lifecycle leaves the runtime (`host.rs` -> `crates/platypus_components`); the runtime carries a generic `HostClass`; Platypus declares components in a manifest (`{"hostClasses": ..}`), whose reading registers `ts_rt_host_*`.
  - [x] The compiler takes the base class, object class and field, method names, prelude and hook list from `hostClasses`; runtime calls `ts_rt_host_*`, exports `host_new_` / `host_hook_`. No `hostClasses` manifest: no host classes.
  - [ ] Still Platypus-specific in the compiler: the `GameObject` sugar in `expr.rs` / `facade.rs` / `engine_objs.rs` (`createPrimitive`, `instantiate`, `dontDestroyOnLoad`, `roots`, `children`, `components`, `scene`, the `parent` option, `component.transform`) and the d.ts text in `builtins.rs` / `types_dts.rs`: these go to Platypus's `Extension` (lowering hooks), with the rewrites below.
  - [ ] Source rewrites (`register()` thunks, contact thunks, hierarchy and tree helpers) move to Platypus as a `web_ui_ts::Extension` installed once per process; the renderer tier table moves to Platypus.
- [ ] **Core and render split:** `web_ui_core` from `platypus_core`; `platypus_render` keeps `world/` over `web_ui_render` (`gpu.rs`, `surface.rs`, `offscreen.rs`, `ui/`).
- [ ] **Move:** the TS and UI crates to `..\web-ui`; Platypus names them once in `[workspace.dependencies]`.
- [ ] **Facade:** `web_ui::App` (Rust), `web_ui_register` / `web_ui_start` (C, `web_ui.h`), cdylib on Linux (shared by every app, lower PSS) and static on Windows / macOS; `examples/hello-rust`, `examples/hello-c`.
- [ ] **Docs, CLAUDE.md, full sweep.**

### Part 2: app parity with Platypus apps

- [ ] **CSS embedded:** every `import "./x.css"` the module graph sees (with `@import` and `url()` assets) goes into the bundle and applies at start; a loose `game.css` is still read when the bundle has none.
- [ ] **Packaging:** `web-ui build` / `web-ui package` as `platypus-build` has them: one executable holding the launcher, the bundle, CSS and assets, icon and version info, licence notices. Windows `.exe`, macOS app; Linux an executable that links the shared `libweb_ui.so` (or a static build on request).
- [ ] **App layout:** `project.json`, `src/` (entry, `src/modules/` for TS modules), `src/plugins/` (Rust crates: `#[ts_api]` namespaces built into the app's launcher), `assets/`.
- [ ] **Example Rust plugin:** a small app with one `src/plugins/` crate whose namespace the TS calls (values, a handle class, a callback, a promise).

### Part 3: web APIs, each an optional crate

Every API is a crate of its own (a `ts_api` plugin with its manifest and symbols), gathered behind cargo features of a `web_api` umbrella crate. An app leaves out what it doesn't use; code naming an API that isn't linked fails at compile time (the name isn't in the generated `platypus.d.ts`), never at run time. Promises and events behave as on the web.

- [ ] **Fullscreen API** (`web_api_fullscreen`): `element.requestFullscreen()`, `document.exitFullscreen()`, `fullscreenchange`.
- [ ] **Workers** (`web_api_worker`): `new Worker(url)`, `postMessage` / `onmessage` with structured clone. Each worker is a thread with its own GC heap (the runtime's heap is already per thread).
- [ ] **fetch and WebSocket** (`web_api_net`): `fetch` (Request / Response / Headers / streams), `WebSocket`; the M6 design in `docs/research/networking-and-files.md`.
- [ ] **Service worker** (`web_api_service_worker`): fetch interception and a cache over `web_api_net`.
- [ ] **Geolocation** (`web_api_geolocation`): the OS location services (Windows.Devices.Geolocation, CoreLocation, GeoClue).
- [ ] **Clipboard** (`web_api_clipboard`): `navigator.clipboard` read / write text and images (arboard, as the UI already uses).
- [ ] **Vibration** (`web_api_vibration`): `navigator.vibrate`, gamepad rumble where there is no phone motor.
- [ ] **Web Storage** (`web_api_storage`): `localStorage` / `sessionStorage`, per app.
- [ ] **IndexedDB** (`web_api_indexeddb`): object stores, indexes, transactions over an embedded store. Lower priority (see Web Storage); added because the set is meant to be the web's.
- [ ] **Web Audio** (`web_api_audio`): `AudioContext`, nodes, `AudioWorklet` later (kira, or its own graph).
- [ ] **File system** (`ts_os`, not a web API): files, folders, paths, process and environment, for apps that are programs rather than pages; removable like the rest.

### Part 4: larger features

- [ ] **WebAssembly:** `WebAssembly.instantiate` / `compile` with imports and exports from TS.
- [ ] **WASM to native:** translate `.wasm` to Cranelift IR (as `wasmtime` does), compile it ahead of time and pack the native code into the executable; the JIT path for development.
- [ ] **Audio / video elements** (`web_api_media`): `<audio>` / `<video>` decoded by FFmpeg (LGPL build, dynamically linked; its licence goes in the notices), frames into the UI renderer as textures.
- [ ] **DRM** (`web_api_eme`): Encrypted Media Extensions as Chromium and Firefox build them (a CDM process behind `navigator.requestMediaKeySystemAccess`). ClearKey first; Widevine needs a licence from Google and their CDM binary, so the user decides that.
- [ ] **Canvas** (`web_api_canvas`): the Canvas 2D API, and a canvas that Platypus's world renderer can draw into (the "canvas UI element" in Later), so Platypus games keep working.
- [ ] **WebKit iframe** (`web_ui_webview`, optional): an `<iframe>` element backed by WebKitGTK / WKWebView / WebView2 for real web content.
- [ ] **Example: v86** (github.com/copy/v86, the x86 emulator): its JavaScript converted faithfully to TypeScript, its Rust core (built to WASM upstream) linked natively as an accelerator plugin; none of its files are committed: sources, BIOS and disk images are fetched from their upstream locations with `fetch` / `WebSocket` at build or run time (downloading needs the user's OK first). Needs `web_api_net`, the Canvas and WASM items.

## QuickJS removed (2026-10-28; was "deprecated; removal plan", 2026-10-22)

QuickJS is gone from Platypus: `platypus_app` runs only compiled games and refuses a QuickJS bundle with a pointer to
`platypus-build build` (`--force` no longer exists, `M9-APP-01`, `-02`). Done:
- [x] Delete `crates/platypus_script` (the QuickJS backend, `ComponentHost`) and the QuickJS paths of `platypus_app` (`load_bundles`, `dev_vendor`, bundle reload, `__platypus_*` pumps; `setUiScale` came back natively as `UiScale`, 2026-10-28).
- [x] Delete `crates/platypus_native` (`register_quickjs`, `install`) and the `quickjs` flag and `qjs.rs` of `ts_api`.
- [x] Delete `js/engine`, `js/react`, the JS fixtures of `tests/fixtures` and the `bun run build*` / `scripts/dev.ts` / `scripts/assets.ts` bun paths (root `package.json` keeps only `typecheck` and `screenshots`).
- [x] Rust integration tests that used ScriptRuntime/QuickJS as a library: deleted with their crates (the removed test IDs are marked in TESTS.md).
- [x] The `platypus_editor` binary and its host side (`editor.rs`, `project.rs`, the editor log capture and `editor.*` ops) are removed; `editor/` and `js/editor-ui` sources stay.

Left:
- [x] **Re-make the editor natively in TypeScript at its current feature state** (2026-10-29); `js/` is deleted.
  End state (decided 2026-10-28): no `js/` folder; the editor is re-made natively at its current feature state.
  - [x] `editor/project.json` (title, company, identifier, `renderer: "vulkan"`, 1600 × 900 window), replacing
    `package.json`'s `platypus` section (2026-10-28).
  - [x] Components into `editor/src` as plain modules with relative imports; `init --editor-files` wrote its
    `tsconfig.json` / `types/` (git-ignored); `editor/package.json` and `editor/node_modules` are gone.
  - [x] A native `Editor` namespace (`platypus_engine_api::editor`): `logSince(seq)` records, `openFile`, `listDir` /
    `copyIn` / `move` answering at once with `lastError`, `project`, `isAvailable`. The host runs the editor like any game:
    `platypus-app editor --project <folder>` (no second binary); without `--project` the calls fail.
  - [x] Gaps of the trial build: type guards and the other compiler gaps (tools/platypus_ts/TODO.md, "TypeScript gaps");
    `currentTarget`, `defaultPrevented`, pointer capture, `textAnchor` / `fontSize` / `fontWeight`, `Ref` / `RefObject` /
    `useImperativeHandle`, `Camera.worldToScreenPoint` / `screenPointToRay` / `pixelRect`. `createPrimitive` is
    `GameObject.createPrimitive` (the native name; no free function), `op` is replaced by `Editor`; `HideFlags`,
    `Camera.sceneGrid` and `Gizmos.present` were already native.
  - [x] Built with `platypus-build build editor` and run natively (release screenshot checked by eye); the model tests run
    natively (`editor/test`, `tools/platypus_ts_tests/tests/ui/editor_models.rs`). The React component tests that ran
    through QuickJS (`menubar.test.tsx`, `project-browser.test.tsx`) were not ported (M5-DND-10 struck).
  - [x] The Scene view lost its grid and icons after the Game tab and back, showing the game camera: renders and effects
    that a UI event caused ran without the scene current (`Native::settle`, the panels' and windows' settle), so the
    remounted view's `new GameObject` camera was never made (`M9-API-1518`, `M5-EDT-29`).
  - [x] Selecting Cube in the Hierarchy showed "1 selected" but no handles: the same cause, the App's render after the
    click read `GameObject.roots` as empty, so the selection matched no objects (`M5-EDT-30`).
  - [ ] Editor follow-ups: a manual check of the Scene view controls, menus and drag and drop in the window (M5-EDT-21,
    M5-EDT-23, M5-DND-11); the editor source still works around the compiler gaps listed as "Found by the editor, still open"
    in tools/platypus_ts/TODO.md (mixed-type class lists, an intersection array, conditional spreads, written return types
    on self-referencing closures; `parseLayout` reads a typed `SavedNode` and flags broken parts because `unknown` and a
    separate `undefined` are not built).
- [ ] **TypeScript conformance after the editor** (audit 2026-10-29, list in tools/platypus_ts/TODO.md "TypeScript gaps"), in
  this order: cheap diagnostics and library holes (G4, G5, L2, `Object.is`, L5, L7, L8, M3, S10); a null property access
  as a catchable `TypeError` instead of a process crash (R2); dynamic `any` / `unknown` P0-P1
  (`docs/research/platypus-dynamic-values.md`: D1-D3, R1, untyped `JSON.parse`, `console.log(obj)`); generics (T3, T10, T11,
  T12) and modules (M1, M2, M6); a checker of our own or `tsgo` and a conformance harness against bun (G1, G6), before
  wider TypeScript (T2, S1-S9, A1-A2). Also T14 (an engine subclass handle to its base).
- [x] **Replace `scripts/screenshots.ts` and `scripts/progress-shots.ts` with Rust tools under `tests/tools/`** (2026-10-28: `tests/tools/shots`, `screenshots` and `progress-shots`; `scripts/` and the root `package.json` bun script are gone).

## Code review (after the platypus_ts branch is merged into `main`)

Most of this branch (about 300 commits) was written by agents and checked by tests, ratchets and snapshots, which catch regressions but not design drift. Once `claude/recursing-wright-856ffa` is merged into `main` (the user decides squash or merge as-is), do a human-readable review pass:
- [ ] **Run `/code-review`** on the merged range (start at `high` effort, then `xhigh` on the compiler and engine crates) and triage the findings into TODO items.
- [ ] **Read-only review agents per area**, one report each (no edits): `tools/platypus_ts` (the compiler: `lower/expr.rs` is about 5,800 lines, split candidates), `tools/platypus_ts_rt` (GC, maps, JSON: unsafe code and rooting), `crates/ts_api_macros` (3,900 lines), `crates/platypus_app` (`main.rs` 3,300 lines, `native.rs`, `render_sync.rs`, `windows.rs`), `crates/platypus_ui` / `platypus_ui_native`, `crates/platypus_render_api` (generated registries), packs and the virtual file system, `platypus-build`.
- [ ] **Look for design drift and duplication:** the same logic in the QuickJS and native paths, copies of the same table, helpers that grew into modules, `#[allow]` without a reason, `unsafe` blocks without a safety comment, `TODO`/`FIXME` left in code, dead code left behind by the QuickJS removal.
- [ ] **Security-minded pass** (`/security-review`): asset path handling (`resolve_asset`, the unsafe-paths design), pack reading (`.ppk`), JSON parsing, the `prebuild` command of `project.json`, anything that runs external commands.
- [ ] **Split the largest files** the review flags (new modules rather than growing them), and keep the notes in `CLAUDE.md` ("Notes for agents") current.
- [ ] **Prune the notes:** `TESTS.md` (split by milestone), `TODO.md` (done items to history), the `CLAUDE.md` Status section (stale), the `tools/platypus_ts/TODO.md` log.
