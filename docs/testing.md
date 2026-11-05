# Testing

The plan (kinds, IDs, per-milestone checks) is [TESTS.md](../TESTS.md); the commands are in
[CLAUDE.md](../CLAUDE.md). This page explains how the tests are physically organised so you can find
or add one. Every test that implements a plan item carries its ID in a comment (for example
`// M3-EVT-05`) and tests are named after the behaviour they check.

## Running

```bash
cargo test --workspace          # Rust unit + integration + headless render + golden
bun run typecheck               # every example via platypus-build check --tsc (runs cargo)
cargo bench -p platypus_ui --bench restyle
```

There is no `bun test`, `bun run build` or `bun run build:fixtures` any more (the JS packages and their tests were
deleted with QuickJS).

**One test executable per crate.** `platypus_ui`, `web_ui_render`, `platypus_render`, `platypus_render_gles`,
`platypus_render_cpu` and `platypus_app` each build their integration tests into a single executable
(`autotests = false`, `[[test]] name = "integration"`): `tests/main.rs` declares every test file as a module
(`mod plan;`), so there is one link instead of dozens (many test executables linking at once used up the
machine's memory). A new test file must be added to its `tests/main.rs` or it does not run. Shared helpers are
`tests/common/mod.rs`, also a module of `main.rs`.

- Run one file: `cargo test -p web_ui_render plan::` (the module path is the filter: the file name and `::`).
- Run one test: `cargo test -p platypus_ui dialog::escape`.
- Limit the jobs on a small machine: `cargo test --workspace -j 4`.
- GLES: ANGLE (D3D11) crashes when contexts are made and used on parallel test threads, so every
  `platypus_render_gles` test takes one process-wide lock from
  [common/mod.rs](../crates/platypus_render_gles/tests/common/mod.rs): `one()` (the lock) or `headless()` (a
  headless context that holds it until dropped; `None`, with a note, where there is no EGL driver). New GLES tests
  use `headless()` and do not make their own contexts.

The workspace `Cargo.toml` sets `[profile.dev.package."*"] opt-level = 3`: dependencies (Parley, swash, wgpu)
are optimized even in debug builds, Platypus's own crates are not.

CI ([ci.yml](../.github/workflows/ci.yml)) runs fmt, clippy `-D warnings`, build and test on Windows
and Ubuntu (software Vulkan via Mesa, WARP on Windows). Its old JS job (bun test, fixture check) no longer applies.

## Where tests live

| Kind | Location | Examples |
|---|---|---|
| Rust unit | `#[cfg(test)]` inside the source file | [css.rs](../crates/platypus_ui/src/css.rs) (parser, `rebase_urls`, `var()`), [color_fn.rs](../crates/platypus_ui/src/color_fn.rs) (colour functions, `color-mix()`), [input.rs](../crates/platypus_platform/src/input.rs), [assets.rs](../crates/platypus_core/src/assets.rs), [attrs.rs](../crates/platypus_ui/src/attrs.rs), [paint.rs](../crates/platypus_ui/src/paint.rs) |
| Rust integration | `crates/*/tests/*.rs` | see below |
| Compiled-game tests | `tools/platypus_ts_tests` (groups `lang`, `stdlib`, `engine`, `engine_api`, `ui`) | `cargo test -p platypus_ts_tests --test <group> <filter>` from `tools/` |
| Render (headless) | [web_ui_render/tests](../crates/web_ui_render/tests) (UI), [platypus_render/tests](../crates/platypus_render/tests) (3D) | `plan.rs`, `headless.rs`, `ui.rs`, `ui_flow.rs`, `blending_over.rs`; `world.rs` (world pass, M5-RND-05 to 15, M5-CUL-02), `post.rs` |
| Golden images | `tests/golden/<id>.png` | compared inside `web_ui_render/tests/ui.rs` and `platypus_render/tests/world.rs` |
| Layout fixtures | `tests/layout/*.json` | run by `layout_fixtures.rs` |
| Fixture assets | `tests/fixtures/{assets,fonts,images}` | glTF and FBX models (`assets/`, some made by its `make_*.py` scripts), fonts (WOFF/WOFF2, TTF) and images used by native tests. The JS bundles and sources that used to live here (`*.js`, `src/`, `package.json`) were deleted with QuickJS |
| Benchmarks | `crates/*/benches` (Criterion) | `restyle.rs`, `layout.rs`, `commits.rs` |

### `platypus_ui` integration tests ([crates/platypus_ui/tests](../crates/platypus_ui/tests))

| File | Covers |
|---|---|
| `tree.rs` | Commit semantics: ops, moves, removal, errors, text nodes |
| `layout_fixtures.rs` | JSON layout cases through the full commit pipeline, generic over `LayoutEngine` |
| `text.rs` | Fonts (including WOFF/WOFF2 from `tests/fixtures/fonts/`, M3-FNT-03), wrapping, inheritance, shaping counts |
| `text_props.rs` | M3d group 4 text properties through the tree: transform, ellipsis, white-space/word-break, font shorthand and features, decorations, stroke, smoothing |
| `selection.rs` | Static text selection (M3-SEL-01/02): drag, click, double and triple click, Ctrl+C, `user-select`, transforms, highlight and `::selection` draw commands, `::placeholder` |
| `dialog.rs` | Dialogs (M3-DLG-01): hidden, open and modal layout, top-layer paint and hit order, `::backdrop`, focus trap and restore, Escape and backdrop `cancel`/`close`, `closedBy`, `[open]`/`[modal]` selectors |
| `motion.rs` | Transitions, `@keyframes` animations, `@starting-style` and `:outro` removal (M3-ANI-02..05); timing/interpolation/parsing units (M3-ANI-01) are in `motion.rs` and `css.rs` source |
| `css.rs` | Selectors, cascade, variables, `::before/::after`, invalidation |
| `hit.rs` | Hit testing rules |
| `pointer.rs` | `Pointer`: hover/active/click/dblclick/wheel |
| `focus.rs` | Tab order, focus events, `:focus-visible` |
| `clip.rs` | `overflow` and `clip-path` (including `polygon()`/`path()`, M3-CLIP-01) in draw list and hit testing |
| `scroll.rs` | Scrolling (M3c step 4b): offsets and hit testing, which axes scroll, wheel chaining, Shift+wheel, key scrolling, Tab into view, script `scrollTo/By/IntoView`, scrollbar geometry, thumb drag and track paging, inherited `scrollbarColor` |
| `editor.rs` | Text fields (M3c step 5): sizing from the font, typing and editing keys with `input` events, textarea Enter and line moves, prevented `keydown`, controlled `value` / `defaultValue`, read-only and disabled, tab order, pointer caret / word / drag selection, cut / copy / paste requests, draw list (selection, blinking caret, dimmed placeholder, caret kept in view), text cursor, IME pre-edit and commit, focus loss dropping pre-edit |
| `navigation.rs` | Gamepad UI navigation (M3-PAD-01): `Input::Navigate` picks the nearest focusable element in a direction (in-line beats closer), nothing without focus, `Input::Activate` clicks with detail 0 |
| `units.rs` | Relative units and math (M3d step 1): `em` from own font size, `rem`, viewport units, `calc`/`min`/`max`/`clamp` (including `%` math in sizes, margins, padding, gaps and shorthands), `boxSizing`, restyle when font size or window size changes, invalid values rejected |
| `layout_props.rs` | Layout properties (M3d steps 2a-2c; `order` re-sorting layout, `zIndex` paint and hit order, Tab order unchanged, `display: contents` flattening and `position: fixed` placement, paint and hit): `gridTemplateAreas` and `gridArea` by name or lines, `gridTemplate` forms, line longhands, `gridAutoRows`, `placeItems`/`placeSelf` setting both axes, bad grid/place/logical values rejected, logical properties mapped left-to-right |
| `layout_props.rs` (more) | Transforms (M3-XF-02): hit testing follows rotation, 3D turns and `backface-visibility`, every property form and keyword is accepted and resets, `preserve-3d` combines turns before flattening; `visibility` (M3-VIS-01): hidden boxes keep space but draw and hit nothing, visible children and spans still do |
| `wide_keywords.rs` | CSS-wide keywords (M3d step 1c): `inherit` on non-inherited properties, `initial` stopping inheritance, `unset`/`revert`, stylesheets without warnings, `inherit` at the root, `::before` inheriting |
| `common/mod.rs` | `Dom` builder helper: builds ops with a CSS string and lays out a 400x400 tree |

Most tests build a tree by emitting ops directly (helpers such as `Dom` or the `Scene` struct in
`web_ui_render/tests/ui.rs`). Tests that need a real reconciler use `platypus_ui_native`
(`tests/reconcile.rs`) or a compiled game in `tools/platypus_ts_tests` (group `ui`).

## Fixtures

`tests/fixtures` holds only assets for native tests: `assets/` (glTF models such as `house.glb` and `worm.glb`, FBX models `cube.fbx` and `rig.fbx` from `make_fbx.py`),
`fonts/` and `images/`. The QuickJS fixture bundles, the `platypus_script` conformance suite
(`runtime_conformance_tests!`) and the tests that drove React in QuickJS are gone.

## Golden images

[testing.rs](../crates/web_ui_render/src/testing.rs) (`web_ui_render::testing`, also reachable as `platypus_render::testing`):

- `assert_golden(id, w, h, &pixels, tolerance)` compares RGBA8 pixels with `tests/golden/<id>.png`,
  each channel within `tolerance` (render tests use 3, to allow driver differences). It panics with
  the count of differing pixels and the first mismatch.
- The `Scene` harness in `web_ui_render/tests/ui.rs` builds a tree from ops and CSS. Its optional
  `prepare` hook (`Scene::prepare`) runs on the laid-out tree just before drawing, for state the
  ops can't set up (hover, focus, text selection, an animation clock), as in the `M3-ANI-06` and
  `M3-SEL-03` goldens.
- `PLATYPUS_UPDATE_GOLDEN=1 cargo test -p web_ui_render` (or `-p platypus_render` for the world goldens) rewrites references instead of comparing.
- Every `assert_golden` call also writes its render to `tests/output/<id>.png`, passing or
  not, for inspection. The folder is git-ignored.
- `cd tools && cargo run -p platypus_shots --bin screenshots [-- names...]` (`tests/tools/shots`, replacing the old `bun run screenshots` /
  `scripts/screenshots.ts`) builds the release `platypus-app` and each example (`platypus_build` as a library), then
  renders it with `--screenshot` to `tests/output/examples-<name>.png`; a window opens for a moment per example.
  `--no-host-build` and `--no-build` reuse what is built. `tests/output/history/` keeps older screenshots for comparison.
- `cd tools && cargo run -p platypus_shots --bin progress-shots [-- names...]` (replacing `scripts/progress-shots.ts`) makes the pictures of
  [PROGRESS.md](PROGRESS.md) (see there; `M9-SHOT-01`, `-02`).
  Updating goldens is always explicit; review the PNG by eye before committing.
- `shared_gpu()` is **required** in GPU tests: one device per test binary behind a mutex
  (creating devices from parallel threads can deadlock some Vulkan layers).
- Render tests mostly assert pixels analytically ("this pixel is the border colour") and use a
  golden only as a regression net. They render a scene built through the real `UiTree`, so draw-list
  and renderer bugs both show up. `RenderStats` is asserted for batching and layer counts.

## Benchmarks

Criterion benches track, not gate: [restyle.rs](../crates/platypus_ui/benches/restyle.rs) (incremental
restyle on a ~2,200 node tree, M3-BEN-01), [layout.rs](../crates/platypus_ui/benches/layout.rs) (M2-BEN-*),
and the other benches in `crates/*/benches`. (The QuickJS-era `commits.rs` and `reload.rs` benches, including
`M4-HOT-07`, were removed with `platypus_script`.)

## The `tools/` workspace

The experimental TypeScript compiler (`tools/platypus_ts`, `platypus_ts_rt`, `platypus_ts_tests`) is a separate
cargo workspace, so `cargo test --workspace` at the repo root does not run it. Run
`cargo test --workspace` from `tools/` instead; `PLATYPUS_TS_GC=stress cargo test -p platypus_ts_tests` repeats
the tests with a garbage collection at every allocation. Its tests are `M9-TS-*`. See
[platypus-ts.md](platypus-ts.md) for how they are organised and how to add one.

## Mapping to TESTS.md

| ID prefix | Area | Main files |
|---|---|---|
| `M1-PLT` | Platform input conversion | `platypus_platform/src/{input,platform}.rs` |
| `M1-RND` | Headless GPU, clear, 0x0 resize | `web_ui_render/tests/headless.rs`, `tests/golden/M1-RND-02.png` |
| `M1-SCR`, `M1-CMP` | Script runtime, components (QuickJS `platypus_script` conformance and tests removed 2026-10-28; compiled games are covered by `tools/platypus_ts_tests`) | `tools/platypus_ts_tests` |
| `M2-REC` | React reconciler ops (the JS reconciler was removed; see `M2-NAT`) | `platypus_ui_native/tests/reconcile.rs` |
| `M2-UI`, `M2-STY`, `M2-VIS`, `M2-LAY` | Tree, styles, visuals, layout | `platypus_ui/tests/{tree,layout_fixtures,layout_cache}.rs` (`M2-LAY-07`: size-query cache, text measure counts), `positioning.rs` (`M2-LAY-08..10`: absolute boxes in static parents sit in their containing block; `M2-LAY-11..12`: transforms, filters and `contain` make one), `scroll.rs` (`M2-DRW-09`: draw culling), unit tests in `style.rs`/`paint.rs`/`calc.rs` |
| `M2-DRW` | Renderer | `web_ui_render/tests/ui.rs` + `tests/golden/M2-DRW-*` (`M2-DRW-10`: backdrop blur stays inside the element's box; `M2-DRW-11`: a radial gradient fades from transparent without darkening; the same cases run through GLES as `M2-GLES-16`, `-17` and the CPU renderer as `M2-CPU-01`; these goldens are pinned to `Blending::Linear`, `vs_wgpu.rs` compares both executors in the `srgb` default) |
| `M2-BLEND` | sRGB blending by default, linear as a choice (`M2-BLEND-01..11`): plan colours and clear (`plan.rs`), `project.json` `blending` (`project.rs`), wgpu and GLES pixels against the sum computed by hand, over the world (`web_ui_render/tests/blending_over.rs`), `platypus_render_gles/tests/blending.rs`, `platypus_render_cpu/tests/blending.rs`, the app's `ui_frame.rs` |
| `M2-INT`, `M2-SCR` | React in QuickJS driving the tree (`M2-INT-01`, `-02` removed 2026-10-28) | - |
| `M3-TXT`, `M3-FNT` | Text and fonts; right-to-left `direction`/`dir` (`M3-TXT-09`) | `platypus_ui/tests/text.rs`, `platypus_ui/tests/direction.rs` (plus rtl cases in `editor.rs`), `text_props.rs` (`M3-TXT-10..15`), `ui.rs` goldens `M3-TXT-*` |
| `M3-CSS` | Stylesheets and cascade; native nesting (`M3-CSS-11`); `@import` expansion, relative paths, cycles (`M3-CSS-12`); spaces between minified function calls (`M3-CSS-13`, `css.rs` unit); `@font-face` source choice (`M3-FNT-03`, `css.rs` unit) | `platypus_ui/src/css.rs` tests, `platypus_ui/tests/css.rs` |
| `M3-MQ` | `@media`: query parsing and matching (`M3-MQ-01`, unit in `media.rs`); rules applying and un-applying (`M3-MQ-02`) | `platypus_ui/src/media.rs`, `platypus_ui/tests/css.rs` |
| `M3-GAME` | Game extensions: `background-image-tint` / `border-image-tint` (`M3-GAME-01`, golden in `web_ui_render/tests/ui.rs` using `tests/fixtures/images/gray.png`) and UI scale modes (`M3-GAME-02`, `platypus_engine_api::ui_scale`) | `web_ui_render/tests/ui.rs` |
| `M3-EVT` | Hit testing, events, focus, clipping, scrolling | `platypus_ui/tests/{hit,pointer,focus,clip,scroll}.rs` (`M3-EVT-02..04`, the React-in-QuickJS event tests, were removed 2026-10-28), goldens `M3-EVT-05-clip` and `M3-EVT-05-scroll` (scrolled content clipped to a rounded scroller with overlay scrollbars); `scrollbar-gutter` and `overscroll-behavior` (`M3-SCR-01`, `scroll.rs`); inline spans as hit targets (`M3-EVT-07`, `hit.rs`); smooth scrolling, `scroll-behavior` (`M3-EVT-09`); `scroll-padding` in `scrollIntoView` (`M3-EVT-10`, `scroll.rs`) |
| `M3-INP` | Text fields: editing, selection, controlled values, clipboard, shortcuts on non-Latin layouts (`M3-INP-02`) | `platypus_ui/tests/editor.rs` (`M3-INP-01`'s React-field tests were removed 2026-10-28), golden `M3-INP-01-fields` in `web_ui_render/tests/ui.rs` (filled input, placeholder, focused input with selection) |
| `M3-IME` | IME composition | `platypus_platform/src/input.rs` (`converts_ime_events`), `platypus_ui/tests/editor.rs` |
| `M3-PAD` | Gamepads and game input | `platypus_platform/src/gamepad.rs` (names, connect/disconnect conversion, `announce_once` dropping repeat connects: `M3-PAD-02`), `platypus_app/src/game_input.rs` (nav mapping, stick hysteresis, `script_args`), `platypus_ui/tests/navigation.rs` (the QuickJS input fixture test and `input.test.ts` were removed) |
| `M3-IMG` | `image` element: natural-size measuring, object-fit/position, rounded clip | `platypus_ui/tests/image.rs` |
| `M3-BG`, `M3-BRD`, `M3-CLIP` | Backgrounds (conic and repeating gradients, layers, `background-clip`, round/space, blend modes: `M3-BG-01..03`; gradient size/position/repeat: `M3-BG-04`), borders (styles, per-side colours, elliptical radii, outline, `border-image`: `M3-BRD-01..04`), polygon and path clips (`M3-CLIP-01`), clips and scrolling of escaping absolute boxes (`M3-CLIP-02..04`, `escape_clip.rs`; GLES match `M2-GLES-18`) | `web_ui_render/tests/ui.rs` (pixel asserts and goldens), `platypus_ui/tests/{clip,hit}.rs`, unit tests in `paint.rs`, `style.rs`, `svg_path.rs` and `web_ui_render/src/ui/images.rs` |
| `M3-FX` | Effects: opacity percentages (`M3-FX-01`, unit), filter chains, drop shadows and backdrop colour filters (`M3-FX-02`, golden plus `paint.rs`/`style.rs` units), `image-rendering` (`M3-FX-03`, golden plus `text_style.rs` unit), masks (`M3-FX-04`, golden plus `style.rs` unit) | `web_ui_render/tests/ui.rs`, unit tests in `paint.rs`, `style.rs`, `text_style.rs` |
| `M3-XF`, `M3-VIS` | Transforms (`M3-XF-01` unit tests in `platypus_ui/src/transform.rs`: function maths, inversion, backfaces, parsing; `M3-XF-03` `as_scale_translate` recognises scale-and-move maps and rejects rotation, uneven scale, perspective, mirroring; `M3-XF-02` integration in `layout_props.rs` plus a golden in `web_ui_render/tests/ui.rs`: rotated bordered box with text, skewed gradient, perspective card with a clipped child, scaled text re-rasterized at its size); `visibility` (`M3-VIS-01`) | `platypus_ui/src/transform.rs`, `platypus_ui/tests/layout_props.rs`, `web_ui_render/tests/ui.rs`, golden `tests/golden/M3-XF-02.png` |
| `M3-LST` | Lists: markers, numbering, `::marker`, inside/outside | `platypus_ui/tests/text_props.rs`, golden `M3-LST-01` in `web_ui_render/tests/ui.rs` |
| `M3-SEL` | Static text selection, `::selection`, `::placeholder` | `platypus_ui/tests/selection.rs` |
| `M3-CUR` | `cursor` image lists, field cursors, cursor decoding | `platypus_ui/tests/{pointer,editor}.rs`, `platypus_app/src/cursor.rs` units |
| `M3-DLG` | `dialog`, top layer, `::backdrop`, focus and close events | `platypus_ui/tests/dialog.rs`, golden `M3-DLG-02` in `web_ui_render/tests/ui.rs` |
| `M3-SVG` | SVG files as images, inline `<svg>` (see [svg.md](svg.md#tests)) | `platypus_ui/tests/svg.rs`, unit tests in `platypus_ui/src/svg.rs`, goldens `M3-SVG-01` and `M3-SVG-03` in `web_ui_render/tests/ui.rs` |
| `M3-BEN`, `M2-BEN` | Benchmarks (`M4-HOT-07`, the QuickJS reload benchmark, was removed 2026-10-28) | `benches/` |
| `M4-PHY` | Physics: box falls and rests (`01`), character controller climbs a step and stops at a wall (`03`), `M4-PHY-04` (ID kept across hot reload) not done, raycasts answered after the step (`05`), bad ops rejected and meshes collide (`06`); `M4-PHY-02` (collision events through the QuickJS fixture) was removed 2026-10-28 | `platypus_physics/tests/physics.rs` |
| `M4-AUD` | Audio, mixed offline: output then ended event (`01`), emitters fall off with distance and pan (`02`), volume, pan, buses, stop (`03`), dB conversion and bad ops (`04`) | `platypus_audio/tests/audio.rs` (`OfflineBackend::render`) |
| `M4-HOT` | Hot reload: new code runs after a reload (`01`); a syntax error or failing build keeps the old code (`04`); clearing the UI tree removes attached and detached script nodes and frees IDs (`09`); the bundle watcher reports a settled change once and ignores other files (`10`). A texture made with `Texture.create` is saved and restored with its pixels and options (`11`). `02`, `03`, `05` and `07` (the QuickJS state save, rebuilt objects, fallback and the benchmark) were removed 2026-10-28; `06` (Fast Refresh in place) no longer applies; `08` was never done | `platypus_ui/tests/tree.rs`, unit test in `platypus_app/src/reload.rs`, `tools/platypus_ts_tests` |
| `M5-OBJ` | GameObjects and components. Rust: hierarchy, nesting and stale ids (`01`), world transforms and re-parenting (`02`), moved sets and inactive subtrees (`03`) in `platypus_scene/tests/scene.rs`. `M5-OBJ-04`, `-05` (JS mirror, `addComponent`) were JS tests; `-06`..`-08` (the QuickJS scene fixture) were removed 2026-10-28 | `platypus_scene/tests/scene.rs` |
| `M5-SCN` | Scenes: save and load round trips, load order and warnings (`01`), `SceneManager` (`03`), `GameObject.instantiate` (`04`); `M5-SCN-02` (QuickJS save/load fixture) was removed 2026-10-28; `examples/scenes` is the manual check | `platypus_scenes`, `tools/platypus_ts_tests` |
| `M5-MTH` | Math types: `Vector*`, `Mathf` (`01`), `Quaternion` (`02`), `Plane`/`Ray`/`Bounds`/`Rect`/`Pose`/`Color` (`03`) | `platypus_math` tests (the JS tests were deleted) |
| `M5-TIM` | `TimeSpan` (`01`) and `DateTime` (`02`), millisecond resolution | `platypus_engine_api` tests (the JS tests were deleted) |
| `M5-HLP` | Helpers: logging (`01`), `Time`, `TimeSince`, `TimeUntil`, `Task` (`02`), seeded `Rng`/`Random` (`03`), Unity-style input (`04`) | `platypus_engine_api` tests (the JS tests were deleted) |
| `M5-CAM` | `Matrix4x4` and camera conversions with viewport rects, `Camera.main`, `CameraFollow`, `CameraOrbit` (`01`) | `platypus_render_api` / `platypus_math` tests (the JS tests were deleted) |
| `M5-RND` | World pass goldens, `05` to `21` (`21`: a tube skinned to two bones, bent, with a bent shadow): perspective quads with UI over them (`05`), cameras by depth and viewport (`06`), lit primitives (`07`), textures and sprites (`08`), glows and beams (`09`), shadows (`10`), PBR under a sky (`11`), a planar mirror (`12`), a reflection probe (`13`), quality low and high (`14`), culling masks (`15`), the UI over the world with backdrop effects (`16`), material maps and modes (`17`), tonemapping, exposure and FXAA (`18`), a render texture (`19`), cascades and spot and point shadows (`20`); `01`-`04` are still to come | `platypus_render/tests/world.rs`, goldens `tests/golden/M5-RND-*.png` |
| `M5-MAT` / `M5-RTX` / `M5-SHD` / `M5-QUA` | Materials: tangents from UVs (`MAT-01`, unit in `mesh.rs`) and `Material` ops with scene saves (`MAT-02`); render textures and `Camera.targetTexture` (`RTX-01`); shadow planning (`SHD-01`, unit in `shadows.rs`) and `shadowCascades` (`SHD-02`); texture quality settings (`QUA-02`); the JS tests of `MAT-02`, `RTX-01`, `SHD-02` and `QUA-02` were deleted with `js/engine`. The goldens are `M5-RND-17` to `-20` | as listed |
| `M5-APP` | The app binary: `platypus-app` without a game argument runs the current folder (`APP-03`, unit in `platypus_app/src/main.rs`), the log file (`APP-04`, unit in `platypus_app/src/logging.rs`), focus and background pausing: window events (`APP-05`, `platypus_platform/src/input.rs`), muting the mixer (`APP-06`, `platypus_audio/tests/audio.rs`), `Lifecycle` and `FixedClock` (`APP-07`) and the frame cap (`APP-08`) in `platypus_app/src/lifecycle.rs`, and the manifest's `runInBackground` (`APP-09`, `platypus_app`); `APP-10` (the JS side) was removed with QuickJS | as listed |
| `M5-ANI` / `M5-SKN` | Animation of objects: curves, clips, `Animation`, `Animator` (`ANI-01..05`, tested natively in `crates/platypus_animation`; the JS tests and `platypus_script/tests/components.rs` are gone); glTF skins and animations (`SKN-01`, unit in `platypus_app/src/models.rs` with `tests/fixtures/assets/worm.glb`), skinned renderers and clips from models (`SKN-02`, its JS test is gone); the skinning golden is `M5-RND-21`. `examples/animation` is the manual check. See [animation-3d.md](animation-3d.md) | as listed |
| `M5-PRE` | Prefabs: capture, independent copies, references, JSON and file round trips (`PRE-01`) | `platypus_scenes`, `tools/platypus_ts_tests` |
| `M5-AST` (assets) / `M5-CLO` | Asset watcher and reload under the same ids (`AST-01`, units in `platypus_app/src/reload.rs` and `world_view.rs`), `Assets.onChanged` (`AST-02`), `.meta` files (`AST-03`, the Bun test `scripts/assets.test.ts` is gone), the app's asset database (`AST-04`, `platypus_app/src/asset_db.rs`), GUIDs and importers (`AST-05`), cooking (`AST-06`, `platypus_app/src/cook.rs`); cloth: solver (`CLO-01`, `platypus_physics/src/cloth.rs`), the app's system (`CLO-02`, `platypus_app/src/cloth_sync.rs`) and `Cloth` ops (`CLO-03`, its JS test is gone). `examples/cloth` is the manual check. TESTS.md also has older planned `M5-AST` rows for a later packaging step | as listed |
| `M5-LAY` / `M5-LOD` / `M5-CUL` | Layers and culling: `scene.layer` (`LAY-01`, removed with the QuickJS scene fixture), camera masks and cull distances (`LAY-02`, unit test in `world_view.rs`), `LayerMask`, `Camera.cullingMask` and field-of-view conversions (`LAY-03`) and light, probe and mirror masks (`LAY-04`) (JS tests deleted), LOD groups (`LOD-01`, `world_view.rs`), frustums and culling stats (`CUL-01`, `CUL-02`, `platypus_render`); the culling-mask golden is `M5-RND-15`. `examples/layers` is the manual check | as listed |
| `M5-EDT` | Editor (native, `editor/`): the models run natively from `editor/test/*.test.ts` (`06`-`14`, `18`-`20`, `22`, `24`), the editor screenshot by eye (`21`); Rust units for the log capture, `Editor.openFile` and the `Editor` namespace (`25`-`27`, which replaced the removed `15`-`17`); `01`-`05` are still to come | `editor/test/*.test.ts` via `tools/platypus_ts_tests/tests/ui/editor_models.rs`, `logging.rs`, `platypus_engine_api/src/editor.rs` |
| `M5-DND` | Drag and drop: the event order, accepting, cancelling, the drag image and system files in `Interaction` (`01`-`04`, `platypus_ui/tests/dnd.rs`), the app's file-drag state (`05`, `file_drop.rs`), project file ops (`08`, `project.test.ts`), the project browser (`10`, `project-browser.test.tsx`); `06` (`dataTransfer` in React), `07` (`project.rs`) and `09` (QuickJS end to end) were removed 2026-10-28 | as listed |
| `M5-PNL` | World panels: `M5-PNL-01` (one commit target per React root) was a JS test, removed; ray to panel UV (`02`, unit test in `platypus_app/src/panels.rs`), hover and clicks on the `examples/world` panel (`03`, manual) | as listed |
| `M5-DSP` / `M5-PRF` | Display and prefs: mode fallbacks per platform (`DSP-01`, `platypus_platform/src/display.rs`), saved display settings over the manifest's (`DSP-02`, `platypus_app/src/display.rs`), `Screen`, `targetFrameRate`, `vSyncCount` and saved quality (`DSP-03`, its JS test is gone), exclusive fullscreen taking the monitor's current resolution and leaving it restoring the window size (`DSP-04`, `platypus_platform/src/display.rs`), the prefs file (`PRF-01`, `platypus_app/src/prefs.rs`), `PlayerPrefs` (`PRF-02`). `examples/arcade` (Esc) is the manual check | as listed |
| `M9-VSP` | Startup video-settings prompt (`platypus_app/src/video_prompt.rs`, `tests/video_prompt_smoke.rs`, `platypus_ui/tests/navigation.rs`, package args in `platypus_build`): when it shows, the flags, the resolution list, what Play writes, d-pad as arrow keys (`10`); see [platform.md](platform.md#startup-video-settings-prompt) | as listed |
| `*-MAN` | Manual checks (resize, IME, gamepad) | no automated test; listed in TODO.md. Text input and gamepad checks (`M3-MAN-01`) use `examples/form` |

When you add a test for a plan item, put the ID in a comment, tick the checkbox in TODO.md and add
the ID to TESTS.md if it is new (working agreement in CLAUDE.md).
