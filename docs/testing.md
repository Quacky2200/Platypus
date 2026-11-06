# Testing

The plan (kinds, IDs, per-milestone checks) is [TESTS.md](../TESTS.md); the commands are in
[CLAUDE.md](../CLAUDE.md). This page explains how the tests are physically organised so you can find
or add one. Every test that implements a plan item carries its ID in a comment (for example
`// M3-EVT-05`) and tests are named after the behaviour they check.

## Running

```bash
cargo test --workspace          # Rust unit + integration + headless render + golden
bun run typecheck               # every example via platypus-build check --tsc (runs cargo)
cargo bench -p web_ui_dom --bench restyle
```

There is no `bun test`, `bun run build` or `bun run build:fixtures` any more (the JS packages and their tests were
deleted with QuickJS).

**One test executable per crate.** `web_ui_dom`, `web_ui_render`, `platypus_render`, `web_ui_render_gles`,
`web_ui_render_cpu` and `platypus_app` each build their integration tests into a single executable
(`autotests = false`, `[[test]] name = "integration"`): `tests/main.rs` declares every test file as a module
(`mod plan;`), so there is one link instead of dozens (many test executables linking at once used up the
machine's memory). A new test file must be added to its `tests/main.rs` or it does not run. Shared helpers are
`tests/common/mod.rs`, also a module of `main.rs`.

- Run one file: `cargo test -p web_ui_render plan::` (the module path is the filter: the file name and `::`).
- Run one test: `cargo test -p web_ui_dom dialog::escape`.
- Limit the jobs on a small machine: `cargo test --workspace -j 4`.
- GLES: ANGLE (D3D11) crashes when contexts are made and used on parallel test threads, so every
  `web_ui_render_gles` test takes one process-wide lock from
  web-ui's `crates/web_ui_render_gles/tests/common/mod.rs`: `one()` (the lock) or `headless()` (a
  headless context that holds it until dropped; `None`, with a note, where there is no EGL driver). New GLES tests
  use `headless()` and do not make their own contexts.

The workspace `Cargo.toml` sets `[profile.dev.package."*"] opt-level = 3`: dependencies (Parley, swash, wgpu)
are optimized even in debug builds, Platypus's own crates are not.

CI ([ci.yml](../.github/workflows/ci.yml)) runs fmt, clippy `-D warnings`, build and test on Windows
and Ubuntu (software Vulkan via Mesa, WARP on Windows). Its old JS job (bun test, fixture check) no longer applies.

## Where tests live

| Kind | Location | Examples |
|---|---|---|
| Rust unit | `#[cfg(test)]` inside the source file | web-ui's `crates/web_ui_dom/src/css.rs` (parser, `rebase_urls`, `var()`), web-ui's `crates/web_ui_dom/src/color_fn.rs` (colour functions, `color-mix()`), [input.rs](../crates/platypus_platform/src/input.rs), web-ui's `crates/web_ui_core/src/assets.rs`, web-ui's `crates/web_ui_dom/src/attrs.rs`, web-ui's `crates/web_ui_dom/src/paint.rs` |
| Rust integration | `crates/*/tests/*.rs` | see below |
| Compiled-game tests | `tools/platypus_ts_tests` (groups `lang`, `stdlib`, `engine`, `engine_api`, `ui`) | `cargo test -p platypus_ts_tests --test <group> <filter>` from `tools/` |
| Render (headless) | web-ui's `crates/web_ui_render/tests` (UI), [platypus_render/tests](../crates/platypus_render/tests) (3D) | `plan.rs`, `headless.rs`, `ui.rs`, `ui_flow.rs`, `blending_over.rs`; `world.rs` (world pass, M5-RND-05 to 15, M5-CUL-02), `post.rs` |
| Golden images | `tests/golden/<id>.png` | compared inside `web_ui_render/tests/ui.rs` and `platypus_render/tests/world.rs` |
| Layout fixtures | `tests/layout/*.json` | run by `layout_fixtures.rs` |
| Fixture assets | `tests/fixtures/{assets,fonts,images}` | glTF and FBX models (`assets/`, some made by its `make_*.py` scripts), fonts (WOFF/WOFF2, TTF) and images used by native tests. The JS bundles and sources that used to live here (`*.js`, `src/`, `package.json`) were deleted with QuickJS |
| Benchmarks | `crates/*/benches` (Criterion) | `restyle.rs`, `layout.rs`, `commits.rs` |

The UI crates' tests (`web_ui_dom`, `web_ui_render`, `web_ui_react`, the GLES and CPU executors, the compiler's own
crates) and their golden images, layout fixtures and benchmarks moved to web-ui: web-ui `docs/testing.md`.

## Fixtures

`tests/fixtures` holds only assets for native tests: `assets/` (glTF models such as `house.glb` and `worm.glb`, FBX models `cube.fbx` and `rig.fbx` from `make_fbx.py`),
`fonts/` and `images/`. The QuickJS fixture bundles, the `platypus_script` conformance suite
(`runtime_conformance_tests!`) and the tests that drove React in QuickJS are gone.

## Golden images

The helpers (`assert_golden`, `shared_gpu()`, `PLATYPUS_UPDATE_GOLDEN=1`) are web-ui's `web_ui_render::testing` (also reachable
as `platypus_render::testing`); how they work is in web-ui `docs/testing.md`. The goldens of the 3D pass (`M5-*`, `M7-*`)
are in `tests/golden/` here and compared by `platypus_render/tests/world.rs`. `cd tools && cargo run -p platypus_shots
--bin screenshots [-- names...]` (`tests/tools/shots`) builds the release `platypus-app` and each example, then renders it
with `--screenshot` to `tests/output/examples-<name>.png`; a window opens for a moment per example. `--no-host-build` and
`--no-build` reuse what is built. `tests/output/history/` keeps older screenshots. `cd tools && cargo run -p platypus_shots
--bin progress-shots [-- names...]` makes the pictures of [PROGRESS.md](PROGRESS.md) (`M9-SHOT-01`, `-02`). Updating goldens
is always explicit; review the PNG by eye before committing.

## The `tools/` workspace

The experimental TypeScript compiler (`tools/web_ui_ts`, `web_ui_rt`, `platypus_ts_tests`) is a separate
cargo workspace, so `cargo test --workspace` at the repo root does not run it. Run
`cargo test --workspace` from `tools/` instead; `PLATYPUS_TS_GC=stress cargo test -p platypus_ts_tests` repeats
the tests with a garbage collection at every allocation. Its tests are `M9-TS-*`. See
web-ui `docs/platypus-ts.md` for how they are organised and how to add one.

## Mapping to TESTS.md

| ID prefix | Area | Main files |
|---|---|---|
| `M1-PLT` | Platform input conversion | `platypus_platform/src/{input,platform}.rs` |
| `M1-SCR`, `M1-CMP` | Script runtime, components (QuickJS `platypus_script` conformance and tests removed 2026-10-28; compiled games are covered by `tools/platypus_ts_tests`) | `tools/platypus_ts_tests` |
| `M2-INT`, `M2-SCR` | React in QuickJS driving the tree (`M2-INT-01`, `-02` removed 2026-10-28) | - |
| `M3-IME` | IME composition | `platypus_platform/src/input.rs` (`converts_ime_events`), `web_ui_dom/tests/editor.rs` |
| `M3-PAD` | Gamepads and game input | `platypus_platform/src/gamepad.rs` (names, connect/disconnect conversion, `announce_once` dropping repeat connects: `M3-PAD-02`), `platypus_app/src/game_input.rs` (nav mapping, stick hysteresis, `script_args`), `web_ui_dom/tests/navigation.rs` (the QuickJS input fixture test and `input.test.ts` were removed) |
| `M3-CUR` | `cursor` image lists, field cursors, cursor decoding | `web_ui_dom/tests/{pointer,editor}.rs`, `platypus_app/src/cursor.rs` units |
| `M4-PHY` | Physics: box falls and rests (`01`), character controller climbs a step and stops at a wall (`03`), `M4-PHY-04` (ID kept across hot reload) not done, raycasts answered after the step (`05`), bad ops rejected and meshes collide (`06`); `M4-PHY-02` (collision events through the QuickJS fixture) was removed 2026-10-28 | `platypus_physics/tests/physics.rs` |
| `M4-AUD` | Audio, mixed offline: output then ended event (`01`), emitters fall off with distance and pan (`02`), volume, pan, buses, stop (`03`), dB conversion and bad ops (`04`) | `platypus_audio/tests/audio.rs` (`OfflineBackend::render`) |
| `M4-HOT` | Hot reload: new code runs after a reload (`01`); a syntax error or failing build keeps the old code (`04`); clearing the UI tree removes attached and detached script nodes and frees IDs (`09`); the bundle watcher reports a settled change once and ignores other files (`10`). A texture made with `Texture.create` is saved and restored with its pixels and options (`11`). `02`, `03`, `05` and `07` (the QuickJS state save, rebuilt objects, fallback and the benchmark) were removed 2026-10-28; `06` (Fast Refresh in place) no longer applies; `08` was never done | `web_ui_dom/tests/tree.rs`, unit test in `platypus_app/src/reload.rs`, `tools/platypus_ts_tests` |
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
| `M5-DND` | Drag and drop: the event order, accepting, cancelling, the drag image and system files in `Interaction` (`01`-`04`, `web_ui_dom/tests/dnd.rs`), the app's file-drag state (`05`, `file_drop.rs`), project file ops (`08`, `project.test.ts`), the project browser (`10`, `project-browser.test.tsx`); `06` (`dataTransfer` in React), `07` (`project.rs`) and `09` (QuickJS end to end) were removed 2026-10-28 | as listed |
| `M5-PNL` | World panels: `M5-PNL-01` (one commit target per React root) was a JS test, removed; ray to panel UV (`02`, unit test in `platypus_app/src/panels.rs`), hover and clicks on the `examples/world` panel (`03`, manual) | as listed |
| `M5-DSP` / `M5-PRF` | Display and prefs: mode fallbacks per platform (`DSP-01`, `platypus_platform/src/display.rs`), saved display settings over the manifest's (`DSP-02`, `platypus_app/src/display.rs`), `Screen`, `targetFrameRate`, `vSyncCount` and saved quality (`DSP-03`, its JS test is gone), exclusive fullscreen taking the monitor's current resolution and leaving it restoring the window size (`DSP-04`, `platypus_platform/src/display.rs`), the prefs file (`PRF-01`, `platypus_app/src/prefs.rs`), `PlayerPrefs` (`PRF-02`). `examples/arcade` (Esc) is the manual check | as listed |
| `M9-VSP` | Startup video-settings prompt (`platypus_app/src/video_prompt.rs`, `tests/video_prompt_smoke.rs`, `web_ui_dom/tests/navigation.rs`, package args in `platypus_build`): when it shows, the flags, the resolution list, what Play writes, d-pad as arrow keys (`10`); see [platform.md](platform.md#startup-video-settings-prompt) | as listed |
| `*-MAN` | Manual checks (resize, IME, gamepad) | no automated test; listed in TODO.md. Text input and gamepad checks (`M3-MAN-01`) use `examples/form` |

When you add a test for a plan item, put the ID in a comment, tick the checkbox in TODO.md and add
the ID to TESTS.md if it is new (working agreement in CLAUDE.md).
