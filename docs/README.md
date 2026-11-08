# Platypus developer docs

These pages explain **how Platypus is implemented today**: data flow, key types, algorithms and where
the code lives. They complement, and do not repeat:

| File | Role |
|---|---|
| [DESIGN.md](../DESIGN.md) | The intended design and decisions (may be ahead of the code) |
| [TODO.md](../TODO.md) | Plan and progress by milestone |
| [TESTS.md](../TESTS.md) | Test plan with IDs such as `M3-EVT-01` |
| [CLAUDE.md](../CLAUDE.md) | Short overview, commands, conventions, gotchas |

Where the code differs from DESIGN.md the pages say so ("Difference from DESIGN.md").

## Index

| Page | Covers |
|---|---|
| [scripting.md](scripting.md) | How games run: compiled bundles and how `platypus_app` loads them, the native backend, the dev loop, hot reload (state kept); QuickJS is removed |
| [ui-and-typescript.md](ui-and-typescript.md) | Overview of a game's TypeScript and UI: how it is compiled and loaded, what Platypus adds to web-ui (components as host classes, the engine API, the renderer tier, `platypus-build`), and where the details live in web-ui |
| [spatial-audio.md](spatial-audio.md) | Effect chains on buses and emitters, `AudioZone` gradients (cave echo, sea, nature beds), `AudioOccluder` (walls, doors, floors), `SoundSensor` and `Acoustics.emit`, Doppler, the `Microphone`, the `audio-*` examples |
| [physics-and-audio.md](physics-and-audio.md) | `platypus_physics` (rapier) and `platypus_audio` (kira): ops and script IDs, per-tick order, batch formats, TS handle APIs, GameObject components (`Rigidbody`, colliders, joints, `Rope`, `CharacterController`, 2D physics, `AudioSource`/`AudioListener`), body-object sync, `Cloth` (position-based solver, `cloth.*` ops), offline mixer and muting, limits |
| [input-and-events.md](input-and-events.md) | Platform input (incl. raw mouse motion), the app glue, script `Cursor` (hide, lock, image), gamepads, game `input` API and mouse, gamepad UI navigation, UI scale. The UI side (`Pointer`, `Interaction`, hit testing, scrolling, text fields, dialogs, drag and drop) is in web-ui `docs/input-and-events.md` |
| [animation-3d.md](animation-3d.md) | Animation of GameObjects: `AnimationCurve`/`AnimationClip`, blending (`Pose`), `Animation`, `Animator` with `AnimatorController` (blend trees, transitions), skinned meshes (bones, bind poses, GPU skinning), glTF skins and animations |
| [scene-and-objects.md](scene-and-objects.md) | `platypus_scene` GameObjects, script and engine ids, `scene.*` ops, the move batch, component lifecycle order (incl. `onAwake` and the application focus and pause hooks), `HideFlags`, scenes (file format, `@property`, `SceneManager`, `instantiate`, prefabs, start scene), the TS API (math, `Transform`, `Time`, dates, logging, `Random`, input, `Application`/`SystemInfo`), limits |
| [world-rendering.md](world-rendering.md) | The 3D world pass (cameras in depth order, viewports, background fill, opaque and see-through sorting, glow), lit meshes (PBR) and lights, skinned meshes, cooked (BC) texture uploads, halos and beams, the procedural sky and environment map, the cloud layer drawn with the background (HDRP's, `RenderSettings.cloudLayer`), planar mirrors, reflection probes, cascaded sun, spot and point shadows, layers and culling masks, frustum culling, LOD groups, material maps and rendering modes, bloom, exposure, tonemapping and FXAA, render textures, the editor ground grid and gizmo lines, thick 2D lines with fills and `Shapes`, `render.*` ops (internal), `render_overlay` for screen UI, `WorldPanel` and `TextMesh` (commit targets, panel trees and textures, pointer and key routing), coordinates |
| [postprocessing.md](postprocessing.md) | `PostProcess`: passes over a camera's finished picture (script API, the built-in effects and their parameters, your own WGSL and its helpers, depth and normals for depth of field and outlines, per-camera passes, how the chain ping-pongs and what it can't do), `examples/postfx` |
| [assets-and-models.md](assets-and-models.md) | `Texture` (load, create, filters, CPU mipmaps), the missing-texture checker, `Material` (metallic, maps, rendering modes, `create`/`with`)/`RenderTexture`/`Mesh`, mesh colliders from render meshes, `Sprite`/`SpriteRenderer`, glTF and FBX `Model.load`/`instantiate` (skins, animations), engine ids, the asset database (`.meta` GUIDs and importers), asset hot reload, cooking (`platypus-app --cook`, BC1/BC3) |
| [particles.md](particles.md) | `ParticleSystem`: settings as key/value pairs, the CPU simulation in `particles.rs`, reports to scripts, drawing as camera-facing quads |
| [platform.md](platform.md) | Window modes (`FullScreenMode`, platform fallbacks), how display settings flow from manifest and saved prefs, `screen.set`, the log file, vsync (on by default) and the frame cap, the startup video-settings prompt, the centred main window, `PlayerPrefs` (file, keys, when written), focus and background pausing (`runInBackground`, `Lifecycle`, `FixedClock`), limits |
| [windows.md](windows.md) | User guide to `Window`: opening tool windows, dialogs and isolated windows from TypeScript, options and styles, parent/child, events, keys and hot reload, messages (`postMessage`, `request`, `broadcast`) and their limits, families and event domains, how `Window` relates to `Screen` and `Application` (and Unity and Godot), platform support table, screenshots |
| [crashes.md](crashes.md) | User guide to crashes: what a player sees (Platypus's crash dialog, the plain fallback), where crash reports go and what is in them, `Application.crash(message?, plain?)`, trying the dialogs, when no dialog shows, reading a report and chasing a crash that only happens sometimes, flags and platforms |
| [debugger.md](debugger.md) | Debugging a game: `--debug-port`, debug builds (a check before every statement), breakpoints, pause and stepping over a text protocol and the Debug Adapter Protocol (VS Code), how files and lines map and survive hot reload, what is not there yet |
| [rendering.md](rendering.md) | How `platypus-app` chooses a renderer per run (flags, `project.json`, the renderer tier, the light wgpu path) and its memory numbers; the frame plan and executors are in web-ui `docs/rendering.md` |
| [editor.md](editor.md) | The native editor (`editor/`, run as `platypus-app editor --project <folder>`): the `Editor` namespace (log feed, code links, project files), its components in `editor/src` (menubar, docking, virtual list, tree view, console, project browser), the Scene view (editor camera, orientation gizmo, SceneIcons, wire gizmos, transform handles, `onEditorFrame`) and the editor UI in `editor/` (demo scene, live Hierarchy, Game tab) |
| [platypus-build.md](platypus-build.md) | `tools/platypus_build`, the build tool for games compiled by web_ui_ts: config, module graph, `game.pbundle`, CSS bundling, assets and `.meta` files, `types/platypus.d.ts`, `src/modules` imports, the dev loop protocol |
| [PROGRESS.md](PROGRESS.md) | Where Platypus stands (dated): what exists, a picture of every example (`docs/progress/`, made by `progress-shots` in `tests/tools/shots`), the benchmark headline, what is next |
| [benchmarks.md](benchmarks.md) | Frame times of every example, native against the QuickJS originals from `main` (old results, from before the QuickJS removal; `platypus-build bench` now times only the native backend; raw data in `benchmarks/results.json`), the spread of re-runs, the stages, the Animator and crowd numbers |
| [animation-native.md](animation-native.md) | `platypus_animation`: curves, clips, pose blending, the `Animation` and `Animator` components and their script API |
| [platypus-api.md](platypus-api.md) | The engine's script API catalogue: what the engine crates declare with `#[ts_api]`, the engine context, native games in `platypus_app`; how the macro works is in web-ui `docs/ts-api.md` |
| [multiplayer.md](multiplayer.md) | `platypus_net` (transport trait, loopback, `Conditioned` fault injection, session handshake and reconnect, schema, replication with acknowledged baselines, `NetHost` and RPC checks), `platypus_net_quic`, the script `Network` namespace (text formats, event lines), how `platypus_app` serves it (addresses, pinning), running `examples/net-arena`, what is not done, test IDs `M8-*` |
| [testing.md](testing.md) | Test layout of the engine, fixtures, the engine's goldens, mapping to TESTS.md; the UI and compiler tests are in web-ui `docs/testing.md` |
| [difference-to-bevy.md](difference-to-bevy.md) | How Platypus relates to Bevy: what each project is for, where the overlap comes from (shared libraries, references), where Platypus differs, and what Bevy has that Platypus doesn't yet |
| [difference-to-unity.md](difference-to-unity.md) | Platypus vs Unity: TypeScript instead of C# with the same component model, portability and popularity, licensing (per-install fees and why we avoid them), API stability |
| [difference-to-sbox.md](difference-to-sbox.md) | Platypus vs s&box: what we took (Unity-style APIs, Razor-like panels with CSS, editor-first), the stack and language changes, publishing approval, stability |
| [difference-to-electron.md](difference-to-electron.md) | Platypus vs Electron: bundled Chromium and V8 cost, the Rust and React app-shell plan, and the web platform APIs we plan (files, fetch, WebSocket, canvas, audio, wasm, storage) |

## Crate map

```
web-ui crates (the web-ui repository; its docs: web-ui `docs/README.md`): web_ui_core, web_ui_render, web_ui_dom, web_ui_react, web_ui_render_gles, web_ui_render_cpu, ts_api, ts_api_macros, web_ui_ts, web_ui_rt, web_ui_bridge
platypus_core      project.json, the renderer tier table (renderer.rs), meta, icon, payload; re-exports web_ui_core (Value, ids, Commit/Op, assets, Blending)
platypus_platform  winit window + event loop, InputEvent (keys, pointer, IME, gamepads), gilrs,
                   window modes and resolutions (display.rs, see platform.md)
platypus_scene     GameObject hierarchy (Scene), ScriptScene (the scene as the game sees it)
platypus_physics   rapier3d world (bodies, colliders, characters, joints), per-tick batches (no deps on other Platypus crates but platypus_core)
platypus_audio     kira mixer, offline backend
platypus_render    WorldRenderer (lit meshes, quads, lights, shadows, bloom, post-processing); re-exports all of web_ui_render (Gpu, surfaces, UiRenderer)
platypus_app       the host `platypus-app` (every game; renderer chosen per game: GLES for UI-only programs, wgpu with the world otherwise) that wires everything together (world_view, particles, models, physics_sync, cloth_sync, lifecycle, asset_db, cook, info)
platypus_engine_api / platypus_render_api / platypus_physics_api / platypus_math  the script API (#[ts_api], from the crates ts_api / ts_api_macros)
platypus_components  NativeHost (component lifecycle, contacts, @property values), the ts_rt_host_* functions and Platypus's compiler extension; its manifest (platypus_components::manifests()) tells the compiler about host classes
editor/            the native editor (a game built by platypus-build: MenuBar, Dock, Scene view, ...; run as `platypus-app editor --project <folder>`, see editor.md)
```

## The frame pipeline

The binary [main.rs](../crates/platypus_app/src/main.rs) is driven by winit's redraw callback
(`PlatformApp::frame`). Input arrives separately, between frames, through `PlatformApp::input`.
The window is created hidden and shown after its first frame; a resize also draws a frame right away.

```
 winit WindowEvent
      |  platypus_platform::convert_window_event
      v
 InputEvent --(scale to logical px)--> web_ui_dom::Input --> Interaction::handle
                                                              |  UiEvent (hover, click, key, ...)
                                                              v
                                       native handlers (web_ui_react::event_of)
                                                              |  compiled handlers run, state changes
 frame():                                                     v
   0. sync_screen: UiTree::set_size(window / scale) (first, so the first frame's script output
      lays out and scrolls at the window's size, not 1x1)
   1. per fixed 60 Hz tick (App::tick): Native::fixed_tick (onFixedUpdate) -> native_after (apply what
      the game asked for) -> step_physics (bodies follow moved objects, rapier, objects follow bodies)
      -> cloth step (while paused in the background none of this runs, see platform.md)
   2. Native::frame: component updates (onUpdate, animation, onLateUpdate), timers and microtasks;
      native_after, then update_particles, update_audio
   3. Interaction::handle(Input::Refresh)         -> re-hit-test under a still pointer
   3b. Native::pre_render (onPreRender), native_audio_commands, sync_native_render (registries ->
      WorldViews ops, world panels, UI resources)
   4. sync_screen again; load_images: take_image_requests -> image_size -> set_image_size (before each draw_list)
   5. UiTree::draw_list()                         -> layout_if_needed: restyle -> text shaping
                                                     -> Taffy -> absolute rects -> Vec<DrawCmd>
   5b. Panels::render: each world panel's tree is drawn into its own texture
   5c. mesh, texture, bloom and shadow settings are handed to WorldRenderer
   6. draw_frame: with a camera, WorldRenderer::render_views (cameras in depth order) then
      UiRenderer::render_overlay; without one, UiRenderer::render -> instances, layers, clips
      -> wgpu passes
   7. present (or --screenshot: offscreen target, PNG, exit)
```

Key ideas to keep in mind:

- **Rust owns the world, scripts own behaviour.** Scripts only hold numeric ids. The native reconciler applies
  one batch of typed ops per UI update.
- **Lazy, dirty-driven work.** `apply_ops` only records what changed. The expensive work
  (restyle, text shaping, layout) happens once in `UiTree::layout_if_needed`, called by
  `draw_list()` and `hit_test()`.
- **Logical pixels everywhere above the renderer.** The app divides window sizes and cursor
  positions by the scale factor; `RenderTarget::scale` converts back.
- **The draw list is GPU-free.** `web_ui_dom` never touches wgpu; tests can inspect `Vec<DrawCmd>`.

## Where to start reading

| I want to... | Read |
|---|---|
| Add a CSS property | `apply_style` in web-ui's `crates/web_ui_dom/src/style.rs`, then web-ui `docs/ui-tree.md` |
| Add a selector / pseudo-class | web-ui's `crates/web_ui_dom/src/css.rs`, web-ui's `crates/web_ui_dom/src/cascade.rs`, web-ui `docs/css.md` |
| Change text field editing | web-ui's `crates/web_ui_dom/src/editor.rs`, web-ui `docs/input-and-events.md` |
| Add a UI event | web-ui's `crates/web_ui_dom/src/events.rs`, `tables/events.rs`, [input-and-events.md](input-and-events.md) |
| Change gamepad / game input | [gamepad.rs](../crates/platypus_platform/src/gamepad.rs), [game_input.rs](../crates/platypus_app/src/game_input.rs), [input-and-events.md](input-and-events.md#gamepads-and-game-input-m3c-step-6) |
| Add a physics or audio op | [physics-and-audio.md](physics-and-audio.md), [platypus_physics](../crates/platypus_physics/src/lib.rs), [platypus_audio](../crates/platypus_audio/src/lib.rs) |
| Add to the script API | `#[ts_api]` in `platypus_engine_api` / `platypus_render_api` / `platypus_physics_api`, [platypus-api.md](platypus-api.md) |
| Change script hot reload | [reload.rs](../crates/platypus_app/src/reload.rs), `App::reload_scripts` in [main.rs](../crates/platypus_app/src/main.rs), [scripting.md](scripting.md#hot-reload) |
| Add a scene feature or component hook | [script.rs](../crates/platypus_scene/src/script.rs), [native.rs](../crates/platypus_app/src/native.rs), [scene-and-objects.md](scene-and-objects.md) |
| Change meshes, lights, shadows, bloom, sky, mirrors or probes | [world/mod.rs](../crates/platypus_render/src/world/mod.rs), [world.wgsl](../crates/platypus_render/src/world/world.wgsl), [world_view.rs](../crates/platypus_app/src/world_view.rs), [world-rendering.md](world-rendering.md) |
| Change textures, sprites or glTF loading | [texture.rs](../crates/platypus_render/src/world/texture.rs), [models.rs](../crates/platypus_app/src/models.rs), [assets-and-models.md](assets-and-models.md) |
| Change animation, `Animator` or skinning | [animation-native.md](animation-native.md), [world/mod.rs](../crates/platypus_render/src/world/mod.rs), [world_view.rs](../crates/platypus_app/src/world_view.rs), [animation-3d.md](animation-3d.md) |
| Change asset reload, `.meta` files or cooking | [reload.rs](../crates/platypus_app/src/reload.rs), [asset_db.rs](../crates/platypus_app/src/asset_db.rs), [cook.rs](../crates/platypus_app/src/cook.rs), `platypus-build assets`, [assets-and-models.md](assets-and-models.md) |
| Change focus, pausing or the frame cap | [lifecycle.rs](../crates/platypus_app/src/lifecycle.rs), [platform.md](platform.md#focus-and-pausing) |
| Change cloth | [cloth.rs](../crates/platypus_physics/src/cloth.rs), [cloth_sync.rs](../crates/platypus_app/src/cloth_sync.rs), [physics-and-audio.md](physics-and-audio.md#cloth) |
| Change particles | [particles.rs](../crates/platypus_app/src/particles.rs), [particles.md](particles.md) |
| Change cameras, planes, the editor grid, gizmo lines or world panels | [world/mod.rs](../crates/platypus_render/src/world/mod.rs), [world_view.rs](../crates/platypus_app/src/world_view.rs), [panels.rs](../crates/platypus_app/src/panels.rs), [world-rendering.md](world-rendering.md) |
| Add a draw command | web-ui's `crates/web_ui_dom/src/draw.rs`, web-ui's `crates/web_ui_render/src/ui/mod.rs`, [rendering.md](rendering.md) |
