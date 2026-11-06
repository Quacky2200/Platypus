# UI and TypeScript

An overview for someone working on Platypus or writing a game. The compiler and the UI stack are not part of this
repository any more: they live in the web-ui library, and its `docs/` hold the reference. This page says how a game's
TypeScript runs, what Platypus adds on top of web-ui, and where to look for details.

## A game's TypeScript

A game is `project.json`, `tsconfig.json` and `src/` (TS and TSX, plus `.css`). There is no JavaScript engine, no Bun and
no `node_modules`:

1. `platypus-build build <game>` (`tools/platypus_build`) walks the module graph, type-checks with `tsc`, and calls web-ui's
   compiler (`platypus_ts`: oxc parses, a typed Platypus IR, Cranelift generates code). The result is
   `build/game.pbundle` plus `build/game.css` and the assets.
2. `platypus-app` (the host) loads the bundle and JIT-links it in process (`crates/platypus_app/src/native.rs`, `Native`).
   Compiled code runs with a precise garbage collector from `platypus_ts_rt`.
3. The engine's names (`Vector3`, `GameObject`, `Component`, `useState`, `createRoot`, ...) are TypeScript **globals**
   declared in the generated, git-ignored `types/platypus.d.ts` (`platypus-build types`). Games import only their own
   relative files and `.css`.

Rust owns the world, scripts own behaviour: scripts hold handles (64-bit ids) and value types, never engine references.
`#[ts_api]` (web-ui's `ts_api` crates) is how the engine exposes Rust to scripts: one annotated module produces the
`extern "C"` shims the compiled code calls, the compiler's manifest and the `.d.ts`, so the three cannot drift apart. The
engine's own declarations are catalogued in [platypus-api.md](platypus-api.md).

The language follows TypeScript and ESM semantics; what is supported and what is not is in web-ui `docs/platypus-ts.md`.

## What Platypus adds to web-ui

web-ui knows nothing about GameObjects. Platypus supplies:

- **Components as host classes.** `crates/platypus_components` gives the compiler the `hostClasses` manifest
  (`platypus_components::manifests()`), a compiler extension in `compile.rs` (so `class Spinner extends Component` and
  `register(..)` compile to the component lifecycle) and `NativeHost`, which runs `onAwake` / `onUpdate` / `onFixedUpdate`
  and the other hooks, delivers contacts and holds `@property` values. See [scripting.md](scripting.md) and
  [scene-and-objects.md](scene-and-objects.md).
- **The engine API crates.** `platypus_scene`, `platypus_scenes`, `platypus_engine_api`, `platypus_render_api`,
  `platypus_physics_api`, `platypus_math`, `platypus_animation` and the rest declare their namespaces with `#[ts_api]`.
- **The renderer tier.** `platypus_core::renderer` is one table of API class names; the compiler reports which engine
  symbols a program calls and `platypus-build` writes the highest tier (`ui`, `world2d`, `world3d`) into the bundle. The
  host uses it to start UI-only games on GLES and the rest on wgpu with the world. See [rendering.md](rendering.md).
- **`platypus-build`.** The build tool around the compiler: config, module graph, CSS bundling, assets and `.meta` files,
  `types`, `dev` (rebuild and hot reload), `check`, `package`. See [platypus-build.md](platypus-build.md).

## The UI in one section

UI is React-style TSX: function components, hooks (`useState`, `useEffect`, ...), keyed lists, portals and DOM-like
events (`onClick`, `onKeyDown`, drag and drop). A native reconciler (`platypus_ui_native`) re-renders a component and sends
one batch of typed ops to the UI tree (`platypus_ui`). The tree resolves CSS stylesheets (cascade, variables, `@media`,
`@keyframes`) and inline styles, lays out with Taffy (flex, grid, floats, tables), shapes text with Parley and produces a
GPU-free draw list. web-ui's renderers draw it: GLES (ANGLE on Windows) for UI-only games, wgpu when the game uses the
world, CPU on request. Where the browser differs, web-ui `docs/differences.md` says so.

Platypus's own UI pieces sit on top:

- **World panels** (`WorldPanel`): a UI tree drawn into a texture on a plane in the 3D world, with its own reconciler and
  pointer routing ([world-rendering.md](world-rendering.md), `crates/platypus_app/src/panels.rs`).
- **Windows** (`Window.create`): extra OS windows with their own tree and event domain ([windows.md](windows.md)).
- **The editor** (`editor/`): a game written in TSX like the others, run by `platypus-app editor --project <folder>`
  ([editor.md](editor.md)).
- **Screen UI over the world**: `render_overlay` / `render_over` draw the screen UI after the 3D pass, and `UiScale`
  scales it to the window ([input-and-events.md](input-and-events.md), [world-rendering.md](world-rendering.md)).

## Details in web-ui

These pages are in the web-ui repository (`docs/`), not here:

- web-ui `docs/platypus-ts.md`: the compiler: how to run it, the pipeline, source files, the supported language, the
  runtime (`platypus_ts_rt`), tests, adding a test module.
- web-ui `docs/platypus-ts-native.md`: notes on the move from QuickJS to the native compiler: what changed, benchmarks,
  limits.
- web-ui `docs/ts-api.md`: how `#[ts_api]` works: handle and value classes, the type table, strings, generated files,
  the engine context, optional parameters.
- web-ui `docs/ui-tree.md`: the UI tree: opcodes, styles, Taffy layout, Parley text, text fields, the draw list.
- web-ui `docs/css.md`: stylesheet parsing, selectors, cascade, invalidation, variables, pseudo-elements, `@font-face`,
  `@media`, URL rebasing.
- web-ui `docs/animation.md`: CSS transitions and `@keyframes` animations.
- web-ui `docs/svg.md`: SVG files as images, inline `<svg>`, caching and limits.
- web-ui `docs/ui-native.md`: the native reconciler: elements, hooks, diff, effects order, portals, events.
- web-ui `docs/react.md`: history of the removed QuickJS React reconciler (the UI element and event model).
- web-ui `docs/rendering.md`: the frame plan, the wgpu, GLES and CPU executors, blending, glyphs, images, shaders, goldens.
- web-ui `docs/input-and-events.md`: hit testing, `Pointer`, `Interaction`, focus, scrolling, dialogs, text fields, drag
  and drop.
- web-ui `docs/testing.md`: how the library's tests, fixtures and goldens are organised.
- web-ui `docs/differences.md`: where the UI knowingly differs from browsers. Read it before changing those areas.
- web-ui `docs/README.md`: the index and the crate map.
