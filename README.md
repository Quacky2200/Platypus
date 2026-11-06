<p align="center"><img src="resources/icon/icon.png" width="128" alt="Platypus"></p>

Platypus
========

**Indie Game Engine for Unity and S&Box enthusiasts featuring Rust, TypeScript and React.**

Platypus is a game engine with a Rust core in which games, and their UI, are written in TypeScript and React. Its
scripting API follows Unity's names and units, and its UI and styling take their cue from S&box's web-style UI.
The aim is that Unity, S&box and React developers feel at home, the API names and units are Unity's, the UI
approach is S&box's, and the UI itself is plain React.

<p align="center">
<img src="docs/progress/godrays-forest.jpg" width="400" alt="God rays through a forest canopy">
<img src="docs/progress/fog-park.jpg" width="400" alt="A Victorian park in the fog">
<img src="docs/progress/crowd.jpg" width="400" alt="Animated mannequins on two sidewalks">
<img src="docs/progress/css.png" width="400" alt="The CSS example">
</p>

A web-style UI, with React
--------------------------

Platypus takes its inspiration from S&box's web-style UI (HTML-like elements styled with CSS) and goes one step
further: UI is written as React components and hooks. React is the most widely used UI library on the web, and its
component model is where much of today's UI practice comes from, so web designers and developers already know how to
build the menus, HUDs and tools of a game this way.

A GameObject component and the React component that draws its UI can live in the same file: a `.tsx` module holds
the `Component` class that runs on a GameObject next to the JSX that shows its HUD or panel, and JSX makes the two
sit together naturally (see [examples/arcade](examples/arcade/src/main.tsx)).

Game and UI development use the same language, TypeScript, and the same tooling: no scripting language for
gameplay next to a separate UI toolkit or markup language, so one person can work on both without context switching.

Why TypeScript, and why a compiler
----------------------------------

Platypus started on QuickJS, a small JavaScript interpreter (removed since). It was easy to embed, but without a JIT its
performance was poor, and shipping a JIT runtime (V8, or Bun's JavaScriptCore) inside every game is heavy. The goal was
performance in the range of ahead-of-time compiled C#, so instead of embedding a runtime we wrote a compiler:
`web_ui_ts` turns a typed subset of TypeScript into native code ([docs/platypus-ts-native.md](docs/platypus-ts-native.md)).
Perhaps it is the first game engine to do this: TypeScript compiled natively as its scripting language, an
alternative to the .NET route taken by Unity, Godot's C# support and Stride.

How close is it? On the one micro-benchmark we compared across engines (a game-rules simulation, 10,000 steps) it
takes 9.1 ms, against 36.1 ms for Bun's JIT and 391 ms for QuickJS. We have not benchmarked it against C#, so "close
to C#" is the aim, not a measured result. Details: [docs/benchmarks.md](docs/benchmarks.md).

The idea
--------

- **Game code and UI in one place.** A game is a folder of TypeScript: components on GameObjects with a Unity-style API
  (`Transform`, `Rigidbody`, `Camera`, `SceneManager`, ...) and its menus and HUD as React components styled with CSS, in
  the same program.
- **TypeScript compiled to native code.** `web_ui_ts` compiles the game's TypeScript ahead of time (oxc parser, a typed
  IR, Cranelift code generation, a precise garbage collector) and the host runs it in process, JIT-compiled at start.
  No JavaScript engine runs a game.
- **React on a native reconciler.** UI is written with React components and hooks; the reconciler that diffs them is
  native, and the result is laid out by Taffy, set by Parley and drawn by Platypus's own wgpu renderer, not a browser.
- **Rust owns the world, scripts own behaviour.** The scene, physics, audio and rendering live in Rust. Scripts hold
  numeric IDs, never engine references. The engine API is Rust annotated with `#[ts_api]`, which also generates
  the game's TypeScript declarations, so the types always match the engine.

Technologies
------------

- **Rust 2024** for the engine and the tools.
- **wgpu** for rendering (Vulkan by default on Windows, DX12 optional): PBR materials, cascaded and spot / point shadows,
  bloom, tonemapping and auto exposure, distance and height fog, volumetric light shafts.
- **Taffy** (flexbox and grid layout), **Parley** (text layout), **swash** (glyphs), **resvg** (SVG), **cssparser** and
  **selectors** (stylesheets).
- **rapier** (3D and 2D physics), **kira** (audio), **glTF** models.
- **oxc** (TypeScript parser) and **Cranelift** (code generation) in `web_ui_ts`, with its own runtime and precise GC.
- **React** components and hooks on Platypus's native reconciler (`web_ui_react`).
- **winit** (windows and input) and **gilrs** (gamepads).
- **ICU4X** for `Intl` (see below).

What it can do
--------------

What exists today; the dated overview with a picture of every example is [docs/PROGRESS.md](docs/PROGRESS.md).

**Scripting and language**
- TypeScript compiled natively: classes, closures, generics, async / await and promises, Map / Set, JSON, RegExp,
  template literals, modules (`src/modules/` for copied-in packages), checked by `tsc` against generated declarations.
- Not yet: a dynamic `any` / `unknown` (designed, not built). QuickJS, the first script backend, was removed: compiled TypeScript is the only backend.

**UI and CSS**
- `box`, `text`, `input`, `textarea`, `image`, `list`, `dialog` and inline `svg` elements; flex and grid layout.
- Stylesheets with the cascade, specificity and `!important`, custom properties, `@media`, `@font-face`, transitions,
  `@keyframes`, pseudo-classes and pseudo-elements, gradients, shadows, filters, masks, blend modes, backdrop blur.
- Pointer, keyboard and focus, gamepad navigation, scrolling, text fields with clipboard and IME, drag and drop.
- UI on planes in the 3D world (`WorldPanel`). Known differences from browsers: [docs/differences.md](docs/differences.md).

**2D and 3D rendering**
- PBR (metallic and specular), material maps, cutout / fade / transparent modes, a procedural sky, reflection probes,
  planar mirrors, render textures.
- Shadows, bloom and glows, tonemapping, exposure, auto exposure and real-world light units, MSAA, FXAA, render scale,
  quality levels.
- Layers, culling masks, frustum culling, LOD groups, particles.
- 2D: sprites, thick lines and filled shapes, `TextMesh`, sorting order.

**Physics**
- Rigidbodies, colliders (mesh colliders from models), a character controller, joints and rope in 3D and 2D; cloth.

**Audio**
- Decoded and streamed sounds, buses, spatial emitters, `AudioSource` / `AudioListener`.

**Animation and skinned meshes**
- Curves and clips, `Animation`, `Animator` with blend trees and transitions, glTF skinned meshes with GPU skinning,
  animation culling, splines and `SplineAnimate`.

**Scenes and prefabs**
- JSON scene files, `SceneManager` (single, additive, async), prefabs, `instantiate`, `dontDestroyOnLoad`, hot reload
  that keeps the scene.

**Windows**
- More OS windows from TypeScript (tools, dialogs, isolated windows talking by messages), each with its own UI. Checked
  with a fake platform and screenshot runs only: the multi-window runner has not been run on a real display.

**Assets and packs**
- GUID `.meta` files, asset hot reload, cooked textures (BC1/BC3) and models, `.ppk` packs (LZ4 or zstd) under a
  virtual file system.

**Build and packaging**
- `platypus-build`: `build`, `types`, `assets`, `dev` (rebuild on save, the running game reloads), `check`, `package`
  (one `.exe` holding the host and the game, with its icon and version info; the Platypus icon when the game has none),
  `bench`.

**Tooling and benchmarks**
- Screenshot runs and windowless bench runs, golden image tests, a test plan with IDs ([TESTS.md](TESTS.md)).
- The editor binary is gone until the editor is re-made natively in TypeScript (at its previous feature state: menubar,
  docking, console, project browser and a Scene view; no Inspector or undo).

**Platforms**
- Developed and tested on Windows. Linux and macOS are untested.

Performance
-----------

Each example was measured natively and, before QuickJS was removed, as its QuickJS original in the same release host
(windowless, 1280x720, fixed step; the QuickJS numbers below are from that time).
Script-heavy examples run up to 7.2x faster natively (raindrop-demo 7.22x, fishtank 5.37x, godrays-room 4.88x, world
4.45x), with 20-75x less script time; nothing is slower beyond the noise between runs. On a pure script micro-benchmark
(a game-rules simulation, 10,000 steps) QuickJS takes 391 ms, bun (JavaScriptCore JIT) 36.1 ms and `web_ui_ts` 9.1 ms.
Tables, start-up times and the method: [docs/benchmarks.md](docs/benchmarks.md).

Quick start
-----------

Windows, with Rust (1.99 or later) and its MSVC toolchain. Bun is only used to run `bun run typecheck` and
the screenshot script.

```bash
# The host (platypus-app.exe, every game) and the build tool. On Windows the host links ANGLE,
# which needs libclang to build: point LIBCLANG_PATH at an LLVM 20+ bin folder (e.g. C:\llvm-23.1.4\bin)
cargo build --release -p platypus_app
cd tools && cargo build --release -p platypus_build && cd ..

# Build an example (into examples/fog-park/build/) and run it
tools/build/release/platypus-build build examples/fog-park
build/release/platypus-app examples/fog-park

# Render one frame to a PNG and exit
build/release/platypus-app examples/css --screenshot out.png --frames 30

# Package a game as one executable (examples/boxes/build/<name>.exe)
tools/build/release/platypus-build package examples/boxes
```

A game is a folder with `project.json` (only `name` is required), `tsconfig.json` and `src/main.ts(x)`; start from
[examples/hello](examples/hello). Engine and UI names are globals declared in the generated `types/platypus.d.ts`
(`platypus-build types`). Building, the manifest and packaging: [docs/platypus-build.md](docs/platypus-build.md).

Documentation
-------------

- [docs/README.md](docs/README.md): how the code works, page by page.
- [docs/PROGRESS.md](docs/PROGRESS.md): where the project stands, with a picture of every example.
- [DESIGN.md](DESIGN.md): the design and the decisions. [TODO.md](TODO.md): the plan by milestone.
- [examples/](examples): 45 example games.

Languages, dates and numbers (Intl)
-----------------------------------

TypeScript games get the usual JavaScript `Intl` features natively (string comparison, number and date formatting,
time zones) through ICU4X, the Unicode Consortium's Rust library, rather than ICU4C, the C++ library browsers use:

- ICU4X is plain Rust, so it links into the game with the rest of Platypus: no C++ toolchain and no separate native
  library to ship.
- Its data is split by locale and feature, so a game can carry only the languages it lists. Today the build still
  links every locale's data; trimming it to the game's list is planned.
- ICU4X has no `availableFormats` table: common date options give the same text as the browsers (compared against bun
  in the tests), but unusual combinations can differ or be refused ([docs/differences.md](docs/differences.md)). An
  optional ICU4C build for exact browser behaviour is planned
  ([docs/research/platypus-ts-intl-datetime.md](docs/research/platypus-ts-intl-datetime.md)).

Licence
-------

Platypus is licensed under the Apache License, Version 2.0 (Apache-2.0). The full text is in [LICENSE](LICENSE), and
[NOTICE](NOTICE) is the notice that goes with copies.

Every source file we write starts with two lines in that file's comment syntax (`//` for Rust, TypeScript, JavaScript
and WGSL, `/* */` for CSS, `#` for Python, shell and TOML), after a `#!` line if there is one:

    Copyright 2026 The Platypus Authors
    SPDX-License-Identifier: Apache-2.0

then a blank line. New files need them too: the `M9-LIC-01` test (`tools/platypus_build/tests/spdx_headers.rs`) fails
for a tracked source file without both lines in its first four. Generated files, built fixtures and third-party code
are left out; the test's exclusion list says which.

Credits and third-party notices
-------------------------------

[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) has the notices of everything the engine host is built from: ANGLE
and the code it bundles, the Inter font, and every Rust crate in `Cargo.lock`, one section per licence. It ships with
every exe: the host prints it with `platypus-app --licenses`, and `platypus-build package` writes it beside each
packaged game. It is generated (`cd tools && cargo run -p platypus_notices`, from `Cargo.lock` and
`tools/platypus_notices/hand-written.md`); the `M9-LIC-02` test fails when it is out of date.

The Inter font is under the SIL Open Font License (`crates/web_ui_dom/fonts/Inter-LICENSE.txt`). The
`text` and `css` examples ship the Charm font by Cadson Demak, from Google Fonts, under the same licence
(`examples/*/assets/fonts/OFL.txt`). Examples
that use outside art or music list it in their `assets/CREDITS.md` (crowd, emoji, fog-white-room, sine-circle-demo);
ports of other projects (the MiniGameEngine demos, the clock's lava lamp) say so in their source headers.
