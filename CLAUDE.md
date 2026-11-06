# Platypus

An s&box-like game engine. The core is Rust. Gameplay is TypeScript compiled to native code by our own
compiler (`tools/web_ui_ts`: oxc, Platypus IR, Cranelift, JIT-loaded by `platypus-app`; no JavaScript
engine, no Bun). UI is React-style TSX rendered by a native reconciler (`web_ui_react`), drawn by our
own renderer. The editor (`editor/`) is a game written in TSX like the others. Full design: `DESIGN.md`.
Plan and progress: `TODO.md`. Test IDs (e.g. `M3-EVT-01`): `TESTS.md`.

**Core rule: Rust owns the world, scripts own behaviour.**
- Scripts never hold engine references, only handles (64-bit ids) and value types.
- Compiled code calls the engine through the `extern "C"` shims `#[ts_api]` generates (`ts_api`);
  the same declarations give the compiler its manifests and TypeScript its `types/platypus.d.ts`.
- QuickJS (rquickjs) was removed on 2026-10-28; `platypus-app` refuses a QuickJS `game.js`.

## Layout

The TypeScript compiler (`web_ui_ts`), its runtime (`web_ui_rt`), the UI bridge (`web_ui_bridge`), `ts_api`, the UI crates and the UI renderers live in the separate **web-ui** repository (`..\web-ui`, its own git repo and cargo workspace; rows below under `../web-ui/`). Platypus names them once, in `[workspace.dependencies]` of `Cargo.toml` and `tools/Cargo.toml` (the only place web-ui's folder is written). Change and test them there (`cd ../web-ui && cargo test -j 4 -p <crate>`); a Platypus build compiles them as path dependencies. Design: `docs/research/web-ui-split.md`.

| Path | What |
|---|---|
| `crates/platypus_core` | `project.rs`: `project.json` (the game's metadata, template variables like `${firstName}`, platform sections, `license`, `renderer` (`vulkan`/`gles`/`cpu`, left out: chosen from the build-time tier; the old `type` warns and is ignored), `blending` (`srgb`, the default, or `linear`: how translucent UI colours combine, web-ui `docs/rendering.md` "Blending"), only `name` is required, other fields default; empty strings warn in build/package; deprecated fallback to `package.json`'s `platypus` section; games need no `package.json`; `platypus-build init` writes one); `renderer.rs`: the renderer tier table (which games need the world renderer). Re-exports `web_ui_core`'s shared types under its own paths |
| `../web-ui/crates/web_ui`, `../web-ui/crates/web_ui_c` | web-ui's facade, not used by Platypus: `web_ui::App` runs a TS / React app in its own winit window (GLES), `web_ui::Program` is the headless core; `web_ui_c` is the C ABI (`include/web_ui.h`, `web_ui_register` / `web_ui_start`; a shared library on Linux, static on Windows / macOS). Examples `examples/hello-rust`, `examples/hello-c` there; docs web-ui `docs/app.md` |
| `../web-ui/crates/web_ui_cli` | web-ui's `web-ui` command (build / run / package an app folder as one exe), not used by Platypus; docs web-ui `docs/app.md` |
| `../web-ui/crates/web_api`, `web_api_storage`, `web_api_clipboard`, `web_api_fullscreen`, `web_api_worker`, `web_api_net`, `web_api_vibration`, `web_api_geolocation`, `web_api_audio`, `web_api_indexeddb`, `web_api_service_worker`, `web_api_webrtc`, `ts_os` | The web APIs, one `ts_api` plugin crate each behind a feature of the `web_api` umbrella (`init(&AppInfo)`, `plugins()`; `web-ui` registers them for every app): `localStorage` / `sessionStorage`, `navigator.clipboard`, `navigator.vibrate` (gamepad rumble), `navigator.geolocation` (the OS's location service), Web Audio (`web_api_audio`: `AudioContext` / `OfflineAudioContext` over its own node graph, cpal output, symphonia decoding), IndexedDB (`web_api_indexeddb`: its own in-memory store, a JSON file per database; requests are facade objects whose events fire from microtasks), the Cache API and a service worker (`web_api_service_worker`: caches on disk; a thread whose `fetch` listeners answer the app's fetches first, through `web_api_net`'s interceptor), WebRTC (`web_api_webrtc`: peer connections and data channels over webrtc-rs; feature `webrtc`, off by default), the Fullscreen API, Web Workers (a thread with its own compiled program; typed JSON messages through the plugin's TypeScript `facade`), `fetch` / `WebSocket` (`web_api_net`: tokio, reqwest, rustls; results settle through `ts_api::task`, which every host pumps each frame and worker turn), and `ts_os` (feature `os`, not sandboxed: `fs`, `path`, `process`, failures thrown by its facade over `#[hidden]` natives); docs web-ui `docs/web-api.md`, writing a plugin `docs/plugins.md`, next ones in TODO.md Part 3 |
| `../web-ui/crates/web_ui_core` | The types every UI crate shares: `Value` (crosses into scripts), ids (`NodeId`, ...), `Commit` / ops, asset reading and path safety (`read_asset`, `AssetFs`, `resolve_asset`, `decode_data_url`), `Blending`, and the packaging code (`payload`: the appended archive, `unpack`: `extract_cached` / `cache_dir`, `icon`: `.ico` / `.res` writers; re-exported by `platypus_core`) |
| `crates/platypus_platform` | winit window + event loop (`Platform`, `PlatformApp`), `InputEvent` conversion, gilrs gamepads |
| `../web-ui/crates/web_ui_dom` | UI tree built from React commits (opcodes in `opcodes.rs`), styles (`style.rs`, `text_style.rs`, `paint.rs`), Taffy layout (`layout.rs`), Parley text (`text.rs`), CSS (`css.rs` parser, `cascade.rs` matching/cascade/invalidation), draw list (`draw.rs`), hit testing (`tree.rs`), pointer events + `:hover`/`:active` (`events.rs`), focus/keys/default actions (`interaction.rs`), drag and drop (`interaction/dnd.rs`) |
| `../web-ui/crates/web_ui_dom/src/tables` | Plain-data tables of what a script can say to the UI: style properties (`PlatypusStyle`: type, doc, group), attributes, elements, event fields / interfaces / `on*` props, React hook signatures. `tools/web_ui_ts/src/ui_dts.rs` writes the UI half of `types/platypus.d.ts` and the compiled prelude from them; `web_ui_dom` tests check them against `apply_style`, `Attrs::set` and the event layout |
| `../web-ui/crates/web_ui_react` | Native reconciler (re-render semantics, hooks, keyed diff, portals, events) over `UiTree`; see web-ui `docs/ui-native.md` |
| `crates/platypus_math` | `#[ts_api]` math types (f64, bit-for-bit as the old TypeScript `math.ts`): `Mathf`, `Vector2/3/4`, `Quaternion`, `Matrix4x4`, `Rect`, `Ray`, `Plane`, `Bounds`, `Pose`, `Shapes` (GDI outlines, `outlines.rs`), `Spline` (`spline.rs`); see `docs/platypus-api.md` |
| `crates/platypus_animation` | Animation without a script engine: curves, clips, pose blending (design: `docs/research/platypus-native-animation.md`); the script API is in `platypus_engine_api::animation` |
| `crates/platypus_kinds` | Registry of component kinds (`register`, `get`, `all`): how the scene loader, `instantiate` and the editor make, save and load components without knowing their classes (placeholders for references) |
| `crates/platypus_scenes` | Scenes in Rust: `SceneManager`, `Scene`, `Prefab`, `instantiate`, the `platypus.scene` JSON loader/saver (same format as `scenes.ts`), kinds for `Camera` / `Animation` / `Animator`; see `docs/platypus-api.md` |
| `crates/platypus_engine_api` | `#[ts_api]` declarations of `Color`, `Time`, `TimeSince`, `TimeSpan` / `DateTime`, `input`, `Random`, `LayerMask`, `Debug` / `Log`, `PlayerPrefs`, `Assets`, `Application` / `SystemInfo`, `Gizmos`, `Camera`, `Screen`, `Window` (windows, families, messages), `QualitySettings`, `UiScale` (screen UI scaling, `ui_scale.rs`), `Spline` / `SplineAnimate` (`spline.rs`), audio (state per thread, host functions); see `docs/platypus-api.md` |
| `../web-ui/crates/web_ui_render` | wgpu device, surfaces, offscreen targets and the UI renderer, in two halves: a backend-neutral `ui/plan.rs` (draw list to `FramePlan`, `PlanResources`, `glyphs.rs` `GlyphCache`, `image_cache.rs` `ImageCache`; tested without a GPU) and the wgpu executor `UiRenderer` (a `UiBackend`; GLES and CPU are the other executors): quads, SDF rounded boxes, shadows, blur layers, gradients, images, glyph atlas (swash); `render_overlay` blends UI over the 3D world. `testing.rs` has golden helpers and a shared test GPU |
| `crates/platypus_render` | The 3D pass `world/` (`WorldRenderer`, `WorldCamera`, `WorldQuad`) over `web_ui_render`, which it re-exports whole (`platypus_render::UiRenderer` still works) |
| `crates/platypus_scene` | GameObjects: generational `ObjectId`s, names, enabled/active, parent/children, local `Transform` (glam) and cached world matrices, `take_moved()`; `ScriptScene` holds the app's scene (script ids, engine ids from 2^30) |
| `crates/platypus_physics` | rapier world driven by `physics.*` ops (script-chosen body/collider IDs); `step()` per fixed tick. Plane locks (a locked axis plus both turns across it, e.g. every Rigidbody2D) are pinned by Platypus (`set_constraints`, `hold_pins`), not rapier: see Gotchas |
| `crates/platypus_physics_api` | Native physics API for compiled scripts (`Physics`, `Body`, `Collider`, `RaycastHit`) over `platypus_physics`; `step()` moves attached GameObjects and queues contacts |
| `crates/platypus_components` | Components over the compiler's generic host classes: `NativeHost` (Unity-order lifecycle, contacts, `@property` values) and the `ts_rt_host_*` functions (`host.rs`); Platypus's compiler extension (`compile.rs`: hook thunks, hierarchy searches, `GameObject` conveniences, the `Component` / `GameObject` d.ts). `manifests()` (the `hostClasses` manifest) registers both: every host that compiles games adds it to its manifests |
| `crates/platypus_audio` | kira mixer driven by `audio.*` ops: sounds (decoded or streamed), playbacks, buses, spatial emitters; `Mixer` trait over a device or `OfflineBackend`; finished sounds are polled by the host |
| `crates/platypus_app` (panels) | `panels.rs`: world panels (a UI tree + interaction + texture each), camera rays and panel hits; `world_view.rs`: cameras and planes from `render.*` ops; `main.rs` routes the pointer and keys between the screen and panels (`Surface`) |
| `crates/platypus_app` (native.rs) | The native backend: a game compiled in process by `tools/web_ui_ts` (JIT, `build/game.pbundle`) driven per frame and event (`Native`); see `docs/platypus-api.md` |
| `crates/platypus_app` (render_sync.rs, native_render.rs) | The render side of the native backend: registries to `WorldViews` ops each frame, world panels with their own reconciler, cloth, `UiResources`, cursor, screen and quality; see `docs/platypus-api.md` |
| `crates/platypus_app` (windows.rs) | Secondary windows (`Window.create`): a UI tree, interaction and surface (or offscreen target in screenshot runs) per window, event routing, per-domain dispatch of isolated families, `--screenshot` writing `out-<slug>.png`; the platform side is `platypus_platform/src/windows.rs` + the runner, the API `platypus_engine_api::window`; guide `docs/windows.md` |
| `crates/platypus_app` (video_prompt.rs) | The optional startup video-settings prompt (`videoSettingsPrompt`, `--video-settings-prompt`): `video_prompt.tsx` run once in a UI-only child process of the same exe (`--video-settings-dialog`, `host::ui_only()`), its answer written to `platypus.display` / `platypus.quality`; see `docs/platform.md`. Vsync is on by default |
| `../web-ui/crates/web_ui_render_gles` | GLES 3 executor of `platypus_render`'s `FramePlan` (`glow`; context through EGL): the light renderer of `platypus-app` for games that are only UI. ANGLE is linked in on Windows (feature `static-angle`, on by default: Mozilla's `mozangle`, EGL + GLES over D3D11, +7 MB per exe; building it needs libclang, see Commands); without the feature EGL is loaded at run time (`PLATYPUS_ANGLE_DIR`, Mesa elsewhere). Quad pass, glyph atlas, images and layers (blur, opacity groups, filters, masks, blend modes, backdrop filters) done and pixel-identical to wgpu in the tests (`M2-GLES-01..13`, which run without any DLL when ANGLE is linked in); `EglContext::window` + `render_overlay`; `platypus-app` draws with it (`gles_gfx.rs`), wgpu fallback (`docs/rendering.md`) |
| `../web-ui/crates/web_ui_render_cpu` | CPU renderers for the UI. `plan::PlanRenderer`: a third executor of `platypus_render`'s `FramePlan` (no GPU, driver or shader compiler): seven AOT kernel families (fill, shadow, glyph, image, transform, composite, blur) drawn in row bands by one pool per process (at most 4 threads), AVX2+FMA picked at run time on x86-64, NEON on arm; within 2/255 of GLES (box-pass blurs past radius 8 within 4/255, `M2-KRN-01..12`); no `render_over`; `platypus-app` doesn't choose it (Fluid's `fluid-app` does, `FLUID_APP_RENDERER=cpu`). The vello renderer (`vello_cpu` + `softbuffer`, feature `vello`, default) is `platypus-app`'s opt-in `renderer: "cpu"` (`cpu_gfx.rs`, never chosen automatically). Gaps in web-ui `docs/rendering.md` |
| `crates/platypus_pack` | Asset packs (`.ppk`): `PackWriter`, `Pack`, `Vfs` over loose folders and packs; design `docs/research/platypus-asset-packs.md` |
| `crates/platypus_app` | The engine host `platypus-app` (the default of `cargo run -p platypus_app`), the one program for every game. It picks its renderer per run (`host.rs`): `--renderer vulkan\|gles\|cpu`, else `project.json` `renderer`, else the bundle's tier found at build time (`platypus_core::renderer`: a program that is only UI starts on GLES through the linked-in ANGLE, `gles_gfx.rs`, with no world renderer; one that uses the 2D or 3D world on wgpu with the world renderer; `cpu` is opt-in, `cpu_gfx.rs`). A UI-only start asked for the world shows "3D Not Supported" (`notice.rs`). `platypus-app [game folder] [--screenshot out.png] [--frames N] [--fps\|--no-fps] [--renderer r]`, `platypus-app --licenses` (prints the built-in third-party notices and exits) (no folder: the current one; an FPS counter shows top right, off for `--screenshot` unless `--fps`, off with `PLATYPUS_FPS=0` or `"fps": false` in `project.json`). Loads `build/game.pbundle` (or compiles a `.ts` given directly) + `build/game.css` and wires scripts → UI → renderer; a QuickJS `build/game.js` is refused. |
| `editor/` | The editor, a game compiled like the examples: `src/` (menubar, docking, virtual list, tree view, console, project browser, Scene view and their models as plain modules), `test/` (the models' tests, run natively by `tools/platypus_ts_tests/tests/ui/editor_models.rs`). `platypus-build build editor`, then `platypus-app editor --project <folder>`: the `Editor` namespace (`platypus_engine_api::editor`: log feed, `openFile`, project file calls) works only with `--project`. See `docs/editor.md` |
| `examples/*` | `hello`, `boxes`, `text`, `css`, `form`, `physics`, `world` (3D), `platformer` (2D), `classics` (test models), `cornell`, `materials`, `quality`, `layers`, `scenes`, `arcade` (a small game with a React HUD), `animation`, `cloth`, `sine-circle-demo` and `raindrop-demo` (MiniGameEngine ports), `shapes`, `splines`, `clock` (twelve faces), `fishtank`, `fog` (distance, height fog and light shafts in a misty clearing), `fog-teapots` (the classic distance-fog picture: teapots on a mirrored checkerboard), `fog-raider` (a PS1 tomb with fog a few metres away, its distance on keys), `fog-blocks` (a Minecraft-like hill in sky-blue fog under a cloud deck), `fog-park` (a Victorian London park: pond mist by morning, gas lanterns glowing in the fog by night), `fog-world` (an endless streamed block world: the fog is the render distance, fog culling skips the spare ring), `godrays-forest` (god rays through a forest canopy), `godrays-cell` (god rays through a barred prison window, with dust), `godrays-room` (god rays through a broken roof, a window and shutters into a dusty room, a day passing), `godrays-sunset` (a 25th-floor flat over a city as the sun sets: beams through mullions and blinds, the city's windows lighting up), `godrays-shed` (a craft's green or red scanning beam sweeping a dusty garden shed through its window and plank gaps), `godrays-lift` (a dark hotel corridor: the lift's doors open and its light floods out), `godrays-clouds` (crepuscular rays: the sun through gaps in a high cloud layer over patchwork fields), `light-units` (real-world light units: a 100000 lux courtyard and a lamplit hall, light meter and physical camera), `tunnel` (auto exposure: a road tunnel, dark inside, the exit a white blaze, dazzling on the way out), `dark-room` (eye adaptation: louvred shutters open on a dim bedroom, blinding, then settling, and close again), `emoji` (a spinner wheel picks one of 50 Noto SVG emoji, each with its own show: entrance, signature pose with sprite particles and props, scene, crowd reaction and exit; sounds synthesised by `tools/make_sfx.py`) `postfx` (post-processing: a colonnade with every built-in `PostProcess` effect switchable, depth of field, ink outlines, a custom WGSL pass; `docs/postprocessing.md`), `postfx-look` (stacked looks: noir, dream, comic, old TV, arcade), `postfx-aberration`, `postfx-focus` (depth of field, tilt-shift, blur), `postfx-outline`, `postfx-film` (vignette, grain, grading: one effect or combination each) `flow` (floats: an article with a floated picture and pull-quote, text wrapping around them), `table` (a table from a grid of `display: contents` rows), `article` (floats and a sortable table in one article) `windows` (extra OS windows: owned tools, an isolated Inspector over messages) games (built with `platypus-build build`; each is just a `project.json`, `tsconfig.json` and `src/`, no `package.json`; the `main` ones await porting) |
| `docs/` | Implementation docs: how the code works today (pipeline, crates, events, rendering, testing). Start at `docs/README.md`. `docs/upstream/` holds reports for upstream projects (the rapier friction issue and PR) |
| `tools/platypus_build` | `platypus-build` (build/types/assets/dev/check/package/init): module graph (`web_ui_ts::modules`) -> `build/game.pbundle` (with the renderer tier, `platypus_core::renderer`) + `game.css` + assets for a game compiled by web_ui_ts, no bun; `init` writes `project.json`, a self-contained `tsconfig.json` and `types/platypus.d.ts`, `init --editor-files <game or folder>` refreshes the last two; see `docs/platypus-build.md` |
| `tools/platypus_notices` | Writes `THIRD-PARTY-NOTICES.md` (built into `platypus-app --licenses` and `platypus-build package`): `cargo metadata` of the host, licence files from the cargo registry, one section per licence; `hand-written.md` (ANGLE, Inter) verbatim; reviewed allow tables in `tables.rs` |
| `tests/fixtures` | Data for Rust tests (glTF models in `assets/`, `fonts/`, `images/`); `tests/layout` JSON layout fixtures; `tests/golden` PNG goldens |
| `tests/tools/shots` | `platypus_shots` (a member of the `tools/` workspace): `screenshots` renders every example to `tests/output/`, `progress-shots` makes `docs/progress/` for `docs/PROGRESS.md` |

### UI model
- **Elements:** `box`, `text`, `span`, `input`, `textarea`, `image`, `list`, `dialog`, `svg` (with SVG
  elements inside) and `#text` nodes. `ROOT` is node 0. Generated `::before`/`::after`/`::marker`/
  `::backdrop` ids start at `0x8000_0000`.
- **Styles:** inline `style` props (camelCase) go through `apply_style`. Stylesheets go through `cssparser` + `selectors`, with cascade (specificity, inline, then `!important`), custom properties and `var()`, and a rule index plus dependency-aware restyle.
- **Units:** layout is in logical pixels. The app divides by the window scale factor.

## Commands

On Windows the shell needs `export PATH="$HOME/.cargo/bin:$PATH"`, and building `platypus_app` needs
libclang for the linked-in ANGLE (mozangle's bindgen): set `LIBCLANG_PATH` to an LLVM 20 or newer `bin` folder per command
(this machine: `export LIBCLANG_PATH='C:\llvm-23.1.4\bin'`; never in a committed file). Without libclang,
`cargo build -p platypus_app --no-default-features` leaves ANGLE out (GLES then needs `PLATYPUS_ANGLE_DIR`, else wgpu).
Cargo writes to `build/` (the root workspace: `build/release/platypus-app.exe`) and `tools/build/` (the `tools/` workspace,
`tools/build/release/platypus-build.exe`), set by `.cargo/config.toml` there; there is no `target/`.

```bash
cargo fmt --all
```
```bash
cargo clippy --workspace --all-targets -- -D warnings
```
```bash
cargo test --workspace
```
```bash
cd tools && cargo run -p platypus_build -- check ../examples --tsc
```
```bash
cd tools && cargo run -p platypus_build -- build ../examples/hello
```
```bash
cd tools && cargo run -p platypus_build -- build ../editor && cd .. && cargo run --release -p platypus_app -- editor --project examples/arcade
```
```bash
cargo run -p platypus_app -- examples/css --screenshot out.png --frames 30
```
```bash
cargo run -p platypus_app -- examples/world --cook
```
```bash
cd tools && cargo run -p platypus_build -- package ../examples/boxes --icon icon.png
```
`platypus-build package <game>` writes `<game>/build/<name>.exe`: one file holding the host (`platypus-app`, from `build/{release,debug}` or `--host`),
the game's bundle, CSS and assets, and the icon and version info from `project.json`, with `THIRD-PARTY-NOTICES.md` beside it (built in; `--no-notices` skips it)
(`docs/platypus-build.md`). `--renderer vulkan|gles|cpu` writes the exe's `renderer`.
```bash
cd tools && cargo run -p platypus_notices
```
regenerates `THIRD-PARTY-NOTICES.md` (the host's crates from `Cargo.lock` plus `tools/platypus_notices/hand-written.md`; `-- --check` only compares).
Run it whenever `Cargo.lock` changes: `notices_are_current` (`M9-LIC-02`) fails until the file matches. A new crate without a licence file, with an
unknown or copyleft licence, or with a licence file inside it, fails until reviewed in `tools/platypus_notices/src/tables.rs`.

- Examples are games compiled by `platypus-build` (no bun, no `node_modules`; engine and UI names are the globals of the generated,
  git-ignored `types/platypus.d.ts`), so plain `tsc` in an example folder fails with "Cannot find name". The `check ../examples --tsc`
  line builds every example and type-checks it with the generated types (`tsc` from the root `node_modules`: the root
  `package.json` holds only the `typescript` dev dependency; `npm install` / `bun install` once). Without `--tsc` it only builds.
- Examples: `platypus-build dev <game>` rebuilds on every edit and signals the running `platypus-app` window; reloads keep state
  through scene serialization.
- The `cargo run` line renders a screenshot and exits; check it by eye. `--cook` prepares a
  game's textures and models into `.platypus/cooked/` (git-ignored) and exits.
- `platypus-build assets <game folder>` writes `.meta` files (GUIDs, import settings) for new
  assets; `platypus-build dev` runs it too.
- `cd tools && cargo run -p platypus_shots --bin screenshots [-- names]` (the old `bun run screenshots`) builds the release host and each example,
  then renders it to `tests/output/examples-<name>.png` (git-ignored; a window opens for a moment per example).
  `cd tools && cargo run -p platypus_shots --bin progress-shots [-- names]` makes the `docs/PROGRESS.md` pictures in `docs/progress/` (windowless
  `--bench` runs, shrunk with the `image` crate). Both take `--no-host-build` / `--no-build` (`tests/tools/shots`).
- Benchmarks: `cargo bench -p web_ui_dom --bench restyle` (and the other benches in `crates/web_ui_dom/benches`).

## Working agreements
- **Default mode:** follow the `iterate-to-target` skill (`.claude/skills/`): keep going until the conversation's target is met unless told to stop, and compact between unrelated tasks. For a plan of two or more tasks, run `multi-agent-lead` once to decide between parallel agents and working in order. When the target is met, run `merge-on-target`, then `clean-branches`, before archiving or closing a chat.
- **Commits:** work milestone by milestone, one commit per step, without asking.
  - Run every check above before each commit.
  - End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
  - Update `TODO.md` checkboxes, and `TESTS.md` when adding test IDs.
  - After each finished step, update `docs/` to match the code. Delegate this to a subagent on the
    Sonnet model, and commit the docs with the step or right after it. Give the subagent only
    what changed since the last docs commit (the commits and files from `git diff`), and have it
    read just that code and the affected doc pages, not the whole codebase.
- **Charm font:** it is committed with the `text` and `css` examples (`examples/*/assets/fonts/`, SIL OFL 1.1 with `OFL.txt`, from Google Fonts, credited in their README, source and the root README). The base checkout's `.git/info/exclude` still lists the Charm paths, so a new copy needs `git add -f`. `build/` is ignored.
- **Code style:** match the surrounding code. Comments are short and say why. Tests are named after behaviour and tagged with their `TESTS.md` ID in a comment.
- **Notes for agents (speed):** a scan found these make work slow, so avoid them.
  - **Two levels of checks.** Per step run `cargo fmt`, the new tests, the existing tests of the area you touched (`-p <crate>`, or `cargo test -p platypus_ts_tests --test <lang|stdlib|engine|engine_api|ui> <filter>`), clippy on the crates you edited, and the `generated_dts` guard when declarations change. Run the FULL sweep (root and `tools/` fmt, clippy, `cargo test --workspace`, `PLATYPUS_TS_GC=stress cargo test -p platypus_ts_tests` (all five groups), `platypus_build check ../examples --tsc`) once at the end of a planned group of work or before a merge, then look at regressions together.
  - **Few link jobs at once.** Each crate has one integration-test executable (`tests/main.rs` declares the files as modules; run one with `cargo test -p <crate> <file>::`); don't add `[[test]]` targets per file. Test runs use `-j 4`, one crate at a time per step: dozens of test executables linking at once (MSVC `link.exe`, 1-2 GB each over wgpu / ANGLE) once used up the machine's 32 GB. GL tests share one context lock (`web_ui_render_gles/tests/common`): ANGLE crashes with contexts on parallel threads.
  - **One build command shape.** Use the same command line and environment every time (same flags, `--workspace` or `-p`, `CARGO_INCREMENTAL` unchanged within a session): a different shape changes the build hash and rebuilds everything, leaving stale copies of every crate in `build/`. Never run two cargo builds at once in the shared checkout.
  - **Read big files in parts.** `tools/web_ui_ts/src/lower/expr.rs` (about 5,800 lines), `crates/web_ui_dom/src/tree.rs`, `ts_api_macros/src/lib.rs`, `platypus_app/src/main.rs`, `TESTS.md` (326 KB) and `tools/web_ui_ts/TODO.md` (long lines are cut off by Read) are huge: use Grep and read ranges, never the whole file. New code goes in a new module rather than growing these files.
  - **Notes stay short.** Keep the "Resume point" of `tools/web_ui_ts/TODO.md` to about 30 lines and replace it, don't append; put history in git, not in TODO/NOTES.
  - **Line endings.** Many files are CRLF on disk (`.gitattributes` says LF): use the Edit tool, not shell tools, and do not mass-convert files in a feature commit.
  - **No QuickJS:** it was removed (2026-10-28) with `js/engine`, `js/react` and the JS fixtures, and `js/` went with the native
    editor (2026-10-29); don't bring back JavaScript-engine paths.
  - **Disk is tight.** No extra target dirs or worktrees. Prune stale build output with a script that keeps the newest build per crate (old hashes pile up); never run a bare `cargo clean` while another agent builds.
- **Write clippy-clean code the first time:** write Rust the way `cargo clippy --workspace --all-targets -- -D warnings` wants it (no needless clones, borrows or returns, `is_empty()` over `len() == 0`, `&str`/`&[T]` parameters, `Default` where `new()` takes nothing, no `too_many_arguments`: group them in a struct, and so on) instead of fixing lints after the fact. Declare things once and in the right place (one interface, type or table that others reuse) rather than repeating them. Never silence a lint with `#[allow]` unless a short comment says why.
- **CSS:** prefer standard CSS over s&box's custom extensions. The CSS parity backlog is TODO.md "M3d".
- **Known differences from browsers** (filter order, mask modes, linear-light fades, defaults, gaps)
  are listed in web-ui `docs/differences.md`. Check it before touching those areas, and keep it current.

## Gotchas
- **No engine imports in games:** engine and UI names (`Vector3`, `GameObject`, `Component`, `useState`, `createRoot`, ...) are TypeScript globals declared in the generated `types/platypus.d.ts` (`platypus-build types`; `platypus-build build` passes it to `tsc` by itself). Examples import only relative files and `.css`. Old `import { .. } from "@platypus/engine"` forms still compile. There is no hand-written `.d.ts`: declarations come from `#[ts_api]`, the UI tables and `tools/web_ui_ts/src/builtins.rs` (see `docs/platypus-build.md`).
- **Modules:** a bare import (`from "foo"`) is a file of `src/modules/` (next to the entry); there is no `node_modules` resolution, so a TypeScript package is used by copying its `.ts` source in (`docs/platypus-build.md`, "Modules and packages"). A module may not capture engine global names (it is renamed internally); unreferenced top-level functions are not compiled.
- **Shell edits:** never create or change files with bash heredocs, `echo`/`cat` redirects or `python -c` strings, even though the tools are installed: they mangle apostrophes and backslashes on this machine. Always use the Edit/Write tools, or a Python script saved as a file (written with Write, then run).
- **Interrupted builds:** if cargo was killed mid-build, delete the corrupt `.pdb` files under `build/` (or `tools/build/`) before rebuilding.
- **Stale app binary:** `cargo clippy`/`cargo test` don't rebuild `build/debug/platypus-app.exe`.
  Run `cargo build -p platypus_app` before any manual check in the window. A running window locks the
  exe, so close it before rebuilding.
- **Logs:** `platypus-app` is a GUI-subsystem program. Its log goes to `Player.log` (last run:
  `Player-prev.log`) next to the game's `prefs.json`, with ANSI colours, and to stderr when
  started from a terminal or piped. Screenshot runs write no file unless `PLATYPUS_LOG_FILE` is set.
- **Memory:** hold UI apps to the private working set (Task Manager's Memory column), not private bytes (the
  D3D11 driver commits ~60 MB it never touches). `PLATYPUS_MEM_TRACE=1` prints memory per start-up stage;
  procedure and numbers in `docs/rendering.md` "Memory".
- **Graphics backend:** Vulkan by default on Windows; `WGPU_BACKEND=dx12` switches.
  `PLATYPUS_FRAME_LATENCY` (1 or 2) sets the queued frames. The default is 1, or 2 on DX12, where 1 made
  resizing jitter.
- **GPU tests:** use `web_ui_render::testing::shared_gpu()` (also `platypus_render::testing`). Creating devices in parallel deadlocks. GLES tests take `web_ui_render_gles/tests/common`'s `headless()` (one context at a time).
- **Library versions:** cssparser 0.38, selectors 0.41, parley 0.12, taffy 0.15, edition 2024, Rust 1.99. APIs differ from older docs.
- **Parley fonts:** after registering a font, Parley needs a fresh `LayoutContext`, or its resolver cache keeps the old fonts.
- **Screenshot runs open a real window** for a moment, and it takes the user's clicks (a probe in
  `examples/physics` once got 60 extra bodies). Tell the user when a batch of runs is going.
- **rapier plane locks:** rapier 0.36.1 drops all friction for a body that can't move along an
  axis nor turn across it (its 3D friction solve inverts a singular 2×2 matrix). Platypus pins such
  axes itself instead (`platypus_physics` `set_constraints` / `hold_pins`, `M4-PHY-07`). Reported as
  dimforge/rapier#1047, fixed by #1048; drop the pins once a rapier release has it.
- **Fixed-size windows:** a game's manifest can set `platypus.display.resizable: false`; its window
  then opens at the manifest's size, ignores saved sizes, and can't be resized or maximized.
- **Hot reload and module state:** restored components get `onReload`, not `onAwake`, and the new
  code's module-level variables start empty: rebuild caches (textures, singletons) in `onReload`.
  A reload restores objects rather than rebuilding them, so changes to what `onStart` builds
  (geometry, materials) only show after a restart.

## Status (2026-10-20)
- **Done:**
  - M1 scaffold
  - M2 React → Taffy → boxes
  - M3a text and fonts
  - M3b CSS stylesheets
  - M3c step 1: hit testing (`UiTree::hit_test`, `event_path`)
  - M3c step 2: pointer events. `web_ui_dom::Pointer` turns input into DOM-style events and sets
    `:hover`/`:active`; the app sends them to `__platypus_ui_event(type, path, data)`, which
    `@platypus/react` (`events.ts`) runs through capture/bubble over `on*` props. `cursor` → OS cursor
  - M3c step 3: keyboard and focus. `web_ui_dom::Interaction` wraps `Pointer`, owns focus and runs
    default actions (Tab, focus on press) using the "prevented" flag `dispatch` returns;
    `ref.focus()`/`blur()` arrive as `focus`/`blur` ops
  - M3c step 4: `overflow`/`clipPath` clipping (clip table in the renderer) and scrolling
    (offsets, wheel/keys/scroll-into-view, draggable overlay scrollbars, `onScroll`)
  - M3c step 5: text fields (`editor.rs`: Parley `PlainEditor` per `input`/`textarea`), clipboard
    (arboard in the app), IME (pre-edit/commit, `set_ime_cursor_area`)
  - M3c step 6: gamepads (`platypus_platform::gamepad`) and keys to scripts through `@platypus/engine`'s
    `input`; d-pad UI navigation (`Input::Navigate`/`Activate`) while UI has focus
  - M3c step 7: `examples/form`. **M3c is done.**
  - M3d (CSS parity) is done: groups 1–10 and step 5.1 (inline SVG and SVG images via resvg),
    (`sound-in`/`sound-out` skipped: s&box-specific, CSS stays about display)
  - M4 (2026-10-17): physics (`platypus_physics`, rapier), audio (`platypus_audio`, kira) and hot
    reload plumbing (watch `build/`, new runtime then swap, old code kept on error, world reset).
    State-keeping reload and Fast Refresh moved after M5.a/b ("Hot reload, part 2" in TODO.md).
  - M5.a done (2026-10-17): `platypus_scene`, GameObjects/components with Unity/s&box-style TS APIs
    (math, DateTime/TimeSpan, Time, Debug/Log, Random, input), lifecycle hooks, 3D world pass,
    camera system, `WorldPanel` (React UI on planes).
  - M5.a2 done (2026-10-17): Rigidbody/colliders (3D and 2D), CharacterController, joints and
    rope (`LineRenderer` tube), colliders following scale; MeshRenderer/Light/primitives;
    AudioSource/AudioListener; textures, sprites (`sortingOrder`), the missing-texture
    checker; particles; glows (halos, bloom, spot beams); Cursor and game mouse input;
    Application/SystemInfo (from the game's `project.json`); glTF models (with mesh
    colliders); directional shadows (`shadowCastingMode`); examples `platformer`, `classics`
    (Stanford Bunny, Newell teaset), `cornell` (bounce lights). Docs refreshed.
  - M5.c materials, first part (2026-10-17): PBR (Cook-Torrance metallic/roughness), a
    procedural sky with reflections and skybox, planar mirrors (`Mirror`), reflection probes,
    `examples/materials`. Docs refreshed (`8bac39e`).
  - Quality settings (2026-10-17, `4f796b8`): MSAA, render scale, shadow quality,
    `QualitySettings` levels with manifest defaults, `examples/quality`.
  - Camera render groups and culling (2026-10-17): `GameObject.layer`, `LayerMask`
    (manifest names), `Camera.cullingMask`, `layerCullDistances` / `layerCullSpherical`,
    `fieldOfView` / `horizontalFieldOfView`; `Light` / `ReflectionProbe` / `Mirror`
    culling masks (per instance in the shader); frustum culling of meshes, quads and
    lights (`world/cull.rs`, `WorldRenderer::stats()`); `LODGroup` with
    `QualitySettings.lodBias`; `examples/layers`.
  - M5.b scenes (2026-10-17): JSON scene files (`scenes.ts`, `@property` in
    `property.ts`), `SceneManager` (single / additive / async from files, unload,
    active scene, events), `dontDestroyOnLoad`, `GameObject.instantiate`,
    `platypus.startScene`, `register(..., { spawn: false })`, `examples/scenes`.
  - Hot reload part 2, state (2026-10-17): the old code saves every scene
    (`__platypus_save_state`), the new code restores it (`__platypus_restore_state`); restored
    script components get `onReload`; unregistered classes make it start over.
    `PLATYPUS_RELOAD_AT_FRAME=N` reloads during screenshot runs. Benchmark `M4-HOT-07` at
    ~146 ms against 100 ms (follow-ups in TODO.md).
  - React Fast Refresh (2026-10-17): dev builds reload game code in place (`hot.ts`,
    `js/react/src/vendor.ts` / `refresh.ts`, `scripts/dev.ts`, `react-refresh` 0.19).
  - Display settings and PlayerPrefs (2026-10-17): window modes / resolution
    (`platypus_platform::display`, `screen.set`), vsync, `targetFrameRate`, `PlayerPrefs`
    (`prefs.rs`, per-OS folder), saved display and quality choices; the arcade has a
    settings menu (Esc).
  - Fixes (2026-10-17): UI backdrop blur and blend modes over the 3D world
    (`render_over`, `M5-RND-16`); a layout panic when UI with absolute boxes was removed
    (`hoisted_by`, `M2-LAY-06`).
  - `examples/arcade` (2026-10-17): a coin collector with a React HUD, FPS counter and
    settings menu; Classic and Deluxe arenas as separate scenes (Style button), the round
    and positions kept across the switch.
  - App (2026-10-17): `platypus-app` with no folder runs the current one; a GUI-subsystem program
    logging to `Player.log`; exclusive fullscreen at the monitor's resolution;
    `Application.runInBackground`, focus events and pause hooks (`lifecycle.rs`).
  - M5.c done (2026-10-17): material maps (normal, metallic, occlusion, emission,
    detail), the specular workflow, rendering modes (cutout / fade / transparent),
    double-sided, texture quality (anisotropy, mip limit); tonemapping (neutral, ACES),
    exposure and FXAA; render textures (`Camera.targetTexture`); shadows drawn once a
    frame with sun cascades and spot / point light shadows. Goldens `M5-RND-17`..`20`.
  - M5.d done (2026-10-17): animation (`animation.ts`: curves, clips, `Animation`,
    `Animator` with blend trees and transitions), skinned meshes and glTF animations
    (GPU skinning), prefabs, asset hot reload, GUID `.meta` files (`scripts/assets.ts`,
    `asset_db.rs`), cooking (`platypus-app --cook`, BC1/BC3), cloth (`platypus_physics::cloth`);
    `examples/animation`, `examples/cloth`.
  - M5.e started (2026-10-17, built by a background agent, cherry-picked here: work from
    agents' worktrees comes in as linear commits, no merges): `platypus_editor` (a second
    binary over main.rs, `editor.rs`), `@platypus/editor-ui` (menubar, docking, virtual
    list, tree view, console with code links, project browser) and the `editor/` app;
    UI drag and drop (`dnd.rs`) with files dropped from the OS. See `docs/editor.md`.
  - The editor's Scene view (2026-10-17/18, another session): an editor camera in the panel,
    the renderer's ground grid (`M5-RND-22`), the x/y/z orientation gizmo, SceneIcons, wire
    gizmos on the selection (`Gizmos`, `M5-RND-23`), move / rotate / scale handles
    (`M5-EDT-24`). The controls still need a manual check in the window (`M5-EDT-23`).
  - 2D drawing (2026-10-18): thick 2D lines as instanced SDF capsules (`LineRenderer2D`),
    filled shapes (lyon), GDI-style `Shapes` helpers, per-shape draw order (`sortingOrder`),
    `TextMesh` (world text without a UI panel, redrawn only when its text changes), `Spline`
    (auto / bezier / linear knots, by distance) and `SplineAnimate`.
  - Examples (2026-10-18): `sine-circle-demo` and `raindrop-demo` (MiniGameEngine ports, CC0
    music credited in `assets/CREDITS.md`), `shapes`, `splines`, `clock` (twelve faces: analog,
    dials, meter, a 3D sundial, sand timer, lava lamp after a CodePen by Corevision, underwater,
    chandelier, rainbow, morphing, flip, alarm clock), `fishtank` (an aquarium-channel look,
    depth haze, now linear fog).
  - Script overhead (2026-10-18): transforms batched into one `scene.locals` op per script
    turn (`__platypus_flush` after every call into JS), lifecycle hooks run in one `__platypus_run`
    call per hook, identity-parent fast paths, scalar `Spline` maths. 123 fish went from ~40 ms
    to ~16 ms a frame (release).
  - Fixes (2026-10-18/19): fixed-size windows (`platypus.display.resizable`, scripts told the real
    size); plane-locked bodies keep their friction (`M4-PHY-07`, rapier PR above);
    `examples/physics` keeps its bodies in the plane; an inline `svg`'s own opacity and
    transform apply once, not also in its markup (`M3-SVG-04`).
  - Fog (2026-10-20, TODO.md M7): distance fog (linear / exponential / squared, `RenderSettings.fog*`,
    `Camera.fog`), exponential height fog (`heightFog`, also over the background) and volumetric
    light shafts (`volumetricFog`, `Light.volumetricIntensity`: a jittered march per fogged pixel
    through hard shadow maps). `examples/fishtank` uses linear fog. Tests `M7-FOG-01`..`05`.
  - One host (2026-10-28): `platypus-app` runs every game, its renderer chosen from the build-time tier
    (`ui` -> GLES through statically linked ANGLE, `world2d`/`world3d` -> wgpu with the world) or `renderer` /
    `--renderer` (`vulkan`, `gles`, `cpu`); `type` is gone; `init` writes tsconfig.json + types (TODO.md).
  - QuickJS removed (2026-10-28): no rquickjs, `platypus_script`, `platypus_native`, `js/engine`, `js/react`, JS fixtures,
    `--force` or `platypus_editor` binary; compiled TypeScript is the only backend (TODO.md "QuickJS removed").
  - Native editor (2026-10-29): `editor/` compiled like a game and run by `platypus-app editor --project <folder>` (the
    `Editor` namespace), at the QuickJS editor's feature state; its model tests run natively; `js/` and
    `tsconfig.base.json` are gone. The compiler gaps it found are fixed or listed in tools/web_ui_ts/TODO.md "TypeScript gaps".
  - web_ui_ts (another session, branch `claude/recursing-wright-856ffa`, ~106 commits ahead):
    an AOT compiler from TypeScript to native code (oxc → Platypus IR → Cranelift) with a precise
    GC and closures; the zombie sim and the whole SineDemo compile and run natively (M9.b2).
    Not merged yet.
- **Next (in this order, see TODO.md):**
  1. Merge web_ui_ts back (from `claude/recursing-wright-856ffa`; history has been rewritten, so
     check its base first; bring it in as linear commits, no merge commits, as with other
     agents' work).
  2. The editor (M5.e), native since 2026-10-29 (`editor/`): the manual check of the Scene view controls, an Inspector bound to
     the scene (`@property` fields), undo/redo, accessibility; then M6 (Script IO).
  - Follow-ups left inside M5.c / M5.d are unticked items there (TAA, refraction, LOD
    cross-fade, morph targets, linked prefabs, cloth self-collision, mobile texture
    formats, ...): later.
  - "Optimisation fixes" in TODO.md (backdrop blur cost, the reload benchmark,
    batching hundreds of 2D shapes, the rest of the frame) wait until
    they're picked.
  - Also in TODO.md: a canvas UI element embedding the 3D engine in React UI (an Electron
    replacement).
  - Also open in TODO.md: the rest of M6 file IO, more classic test scenes (downloads
    need the user's OK first).
  - API names and units follow Unity/s&box (degrees for Euler angles). Decided: own wgpu
    renderer; `platypus_scene` without an ECS. Later items in TODO.md: GPU-tessellated SVG with
    SMIL and path morphing; UI and SVG editors (M5.e).
  Milestones: M5.a–e objects, scene, assets, rendering, editor; M6 Script IO; M7 lighting; M8
  multiplayer; M9.a–c backends, AOT, release packaging; M10 optional (OpenXR, Steam Audio, ray
  tracing). Commit per step; refresh docs once per group (Sonnet subagent, changed code only).
- **Also planned:** "Script IO" (files, fetch, WebSocket) is M6, designed in
  `docs/research/networking-and-files.md`.
- **GPU crashes:** the user's PC has hard-reset with the window open (an existing NVIDIA
  instability, also seen before Platypus). Warn before launching the interactive window; prefer
  `--screenshot` runs for checks.
- **Needs the user:** the CI run, deferred until close to the end (the user's call).
