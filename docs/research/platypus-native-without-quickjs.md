# Platypus without QuickJS: what still uses it

Status: research, 2026-10-20. Inventory of every remaining QuickJS use, the native replacement
(built or missing) and the order to remove it. Base: commit `a0dcd3a`. Companion notes:
[platypus-js-dynamic-tier.md](platypus-js-dynamic-tier.md) (can untyped JS run without QuickJS, measured),
[platypus-ts-compiler-improvements.md](platypus-ts-compiler-improvements.md) (compiler work for both tiers),
[platypus-native-without-quickjs-plan.md](platypus-native-without-quickjs-plan.md) (the step plan).

Background decisions: [platypus-native-engine.md](platypus-native-engine.md) section 5 (QuickJS stays "for
mod support and for anything that cannot use compiled TypeScript", a legacy path to remove when
possible; a TS-native runtime may absorb it).

## 1. Summary

- **One production path.** Everything that runs QuickJS in a shipped program goes through
  `crates/platypus_app` (both binaries, `iron` and `platypus_editor`): `platypus_script::QuickJsRuntime` plus
  `platypus_script::ComponentHost`. Every game, the editor, hot reload, scene-state saving and every
  `__platypus_*` callback depend on it.
- **The typed QuickJS bindings are test-only.** `#[platypus_api(quickjs)]` generates `register_quickjs`
  for each namespace, but `platypus_app` never calls it; only crate tests do.
- **No mod or user-script loader exists.** "Mods" appear only in design docs (DESIGN.md 4.3,
  platypus-native-engine.md 5). So the "QuickJS for mods" requirement is a future requirement, not code.
- **The native side is far along.** Engine state is Rust (`platypus_scene`, `platypus_scenes`,
  `platypus_physics_api`, `platypus_engine_api`, `platypus_math`, `platypus_animation`), the UI reconciler is native
  (`platypus_ui_native`), compiled components have a host (`platypus_ts_rt::host`, `NativeHost`), and the
  compiler (`tools/platypus_ts`) covers the TypeScript the examples use. What is missing is the **wiring**
  (an `platypus_app` path that runs compiled code) and a **dev loop** (reload without restart).

## 2. Inventory

Line numbers are at `a0dcd3a`.

### 2.1 `crates/platypus_script` (the QuickJS crate)

| Item | What it does | Native replacement | Status |
|---|---|---|---|
| `QuickJsRuntime` (quickjs.rs, 339 lines) | rquickjs runtime; installs `__platypus_op` (:207), `__platypus_commit` (:224), `__platypus_report_error` (:230); `eval`, `call_global`, timers | Compiled code calls Rust directly (`#[platypus_api]` shims); timers/microtasks are `platypus_ts_rt::event` | Built for compiled code; not wired |
| `ComponentHost` (components.rs, 387) | Component lifecycle over JS globals `__platypus_registered`, `__platypus_create`, `__platypus_hooks`, `__platypus_tick`, `__platypus_animate`, `__platypus_dispatch`, `__platypus_destroy`; `reload()`, `load_game()` with `__platypus_hot_begin/_end` | `platypus_ts_rt::host::NativeHost` (same hook order, rooted instances, contact hooks) | Built; not wired |
| `ScriptRuntime` trait (lib.rs:52) | Backend abstraction (QuickJS required on iOS per DESIGN.md) | A native backend would implement the same seam in `platypus_app` | `platypus_app` uses the concrete type, not the trait |
| prelude.js (48), conformance.rs (306) | Timers prelude; backend conformance suite (also used by platypus_ui tests) | `platypus_ts_rt::event` timers | Built |
| Op protocol types (`platypus_core::ops`, re-exported) | `Op`, `OpBuffer`, `Commit` | Typed Rust calls; `platypus_ui::opcodes::Op` for UI | Built |

### 2.2 `crates/platypus_app` (the only production consumer; owned by another session)

`main.rs` (2378 lines) is typed to `QuickJsRuntime` (field `scripts` :436, `components` :437).
About 600-700 lines are QuickJS-coupled.

| Area | Where | What | Native replacement | Missing |
|---|---|---|---|---|
| Startup | `run()` :195-230 | `QuickJsRuntime::new`, `__platypus_info` eval (info.rs:147), `load_bundles` (:2303: `vendor.js` + `game.js` or one bundle), `registered_classes`, `spawn_starting_components`, `typeof` probes of `__platypus_ui_event`/`__platypus_input`/`__platypus_audio`/`__platypus_scene`/`__platypus_physics` | Load a compiled game library (object linked into the app, or a JIT module, see plan phase C); `NativeHost::spawn`; `Application`/`SystemInfo` from `platypus_engine_api` | A "script host" seam in `platypus_app` with a native backend |
| Per-frame | :701-760, :797, :894, :1937-1965 | `fixed_update` → `apply_script_output`; `__platypus_scene` (moves), `__platypus_physics` (contacts, rays), `scripts.tick` (timers), `update_active` (`__platypus_tick`/`__platypus_animate`), `__platypus_particles`, `__platypus_audio`, `editor.pump`, `pre_render`, `__platypus_screen`, `__platypus_focus` | `NativeHost` hooks; scene/physics/audio are Rust-side state the compiled code reads directly (`platypus_scene::api`, `platypus_physics_api` `take_contacts`, `platypus_engine_api::audio`); `ts_rt_timers_advance` + `ts_rt_microtasks_run` | The calls in `platypus_app` |
| Events | :2113-2117, :1497-1501 | `__platypus_ui_event(type, path, data)`; `__platypus_input` | `platypus_ui_native` events (capture/bubble on the component tree); `platypus_engine_api::input` state | Wiring |
| Op dispatch | `apply_script_output` :929-1158 | UI commits (`drain_targeted_commits`: screen and world panels) and op families: UI host ops (`loadFont`, `uiScale`, style sheets, focus, pointer capture, scroll, `dragImage`), `render.*` (~25, world_view.rs:724-1157), `asset.readText`, `app.*`, `prefs.*`, `screen.set`, `editor.*` (editor.rs:125), `cursor.*`, `scene.*` (`ScriptScene::apply`), `audio.*`, `cloth.*`, `physics.*` | `platypus_ui_native` → `UiTree` typed ops; `#[platypus_api]` namespaces for app/prefs/screen/cursor/scene/audio/physics (built); `render.*` → `platypus_engine_api::Camera`/`Gizmos`/`QualitySettings` exist, the rest of `render.*` (meshes, lights, particles, sky, probes, mirrors, post) is **not declared yet** | `render.*` and `editor.*` as `#[platypus_api]`; cloth |
| Hot reload | `reload_scripts` :498-599, `reload_in_place` :614, `BundleWatcher` :241, `IRON_RELOAD_AT_FRAME` :2056 | In place (Fast Refresh + class patching) or full (`__platypus_save_state` → new runtime → `__platypus_restore_state`) | `platypus_ui_native::Runtime::refresh` (keeps hook state); `platypus_scenes` saves/loads scenes from `@property` tables; compiled programs **restart** today (platypus-api.md:364) | JIT reload of compiled code with state kept (plan phase C) |
| Assets | :1130-1224 | `__platypus_asset_text`, `__platypus_models`, `__platypus_textures`, `__platypus_assets_changed` | `platypus_engine_api::Assets` (built; `onChanged` needs closure APIs) | Closure-taking APIs |
| editor.rs (393) | `pump()` :91 | `__platypus_editor_fs`, `__platypus_log`; editor UI needs `dist/game.js` | An `Editor` `#[platypus_api]` namespace + compiled TSX editor | All |
| game_input.rs (155), physics_sync.rs (217) | Engine-side only; their tests use `QuickJsRuntime` | `platypus_engine_api::input`, `platypus_physics_api` | Tests to port |

### 2.3 The `#[platypus_api]` QuickJS bindings

- `crates/platypus_api_macros/src/lib.rs` (1304 lines; about 400 are QuickJS): flag parsed at :116,
  helpers `js_param_ty` :322 .. `to_js` :469, per-function closures :1021-1180, namespace init
  :1204-1249, `register_quickjs(ctx)` :1254-1272 (installs `__platypus_raw`, the classes, `globalThis[ns]`).
- `crates/platypus_api`: `default = ["quickjs"]` (Cargo.toml:8), `pub use rquickjs` (lib.rs:16-17).
  Every crate that does not say `default-features = false` pulls rquickjs, including
  `tools/platypus_ts_rt` (Cargo.toml:9), `tools/platypus_ts_ui` and the test crate.
- Namespaces marked `quickjs`: `platypus_engine_api` (17 namespaces: Keyframe, Application, SystemInfo,
  Assets, TimeSince/TimeUntil, Color, TimeSpan/DateTime, Gizmos, input, LayerMask, Debug/Log/Console,
  HideFlags, PlayerPrefs, Random, Screen/QualitySettings, Time), `platypus_math` (all 14), `platypus_scene`
  (`Scene`). Not marked: animation, audio, Cameras, `platypus_scenes`, `platypus_physics_api`.
- **Callers: tests only** (`platypus_engine_api/tests/{batch1,engine}.rs`, `platypus_math/tests/quickjs.rs`,
  `platypus_scene/tests/api.rs`, `platypus_api/tests/*`). Removing them changes no program.

### 2.4 JavaScript and TypeScript run by QuickJS

| Package | Lines (src, of which tests) | Replacement | Status |
|---|---|---|---|
| `js/engine` | 11,468 (3,010) in 33 files | Rust (`platypus_*_api`, `platypus_scenes`, `platypus_animation`, `platypus_math`) + generated `.d.ts` | Most ported; `rendering.ts` (1,425), particles, lines, models, textures, cloth not yet |
| `js/react` | 1,798 (1,003): host 353, events 465, style 472, index 331 | `crates/platypus_ui_native` (hooks, keyed diff, effects in React order, portals, events, controlled fields, `refresh`) + `tools/platypus_ts_ui` (compiled TSX bridge, `lower/jsx.rs`) | Built; missing `memo`, `forwardRef`, `useReducer`, `useSyncExternalStore`, context from TSX; not wired into `platypus_app` |
| `js/editor-ui` | 3,221 (743) in 18 files | Compiled TSX over `platypus_ui_native` | Not started |
| `editor/` | 381 (main.tsx, demo-scene.ts) | Compiled TSX | Not started |
| `scripts/dev.ts` (65) | Bun dev build (`vendor.js` + watched `game.js`, `// @platypus-dev`) | The compiler as the watcher (platypus-native-engine.md "Dev loop without Bun") | Not started |
| `examples/*` | 19 games; 18 import `@iron/engine`, 18 `@iron/react` | Compiled TS/TSX | `sine-circle-demo` compiles in the test crate (`sine_demo_full`); none builds through `platypus_ts` in `platypus_app` |
| `tests/fixtures/*.js` | Built bundles for Rust integration tests | Native tests | Keep until their tests are ported |

### 2.5 Tooling that uses QuickJS on purpose

- `tools/platypus_ts_tests` depends on `platypus_script` (Cargo.toml:21) for the comparison benchmarks
  (`bin/bench.rs`, `sine_bench.rs`, `ui_bench.rs`). These are the oracle for "faster than QuickJS"
  and stay until QuickJS is deleted (then they compare against bun only).
- `crates/platypus_ui` tests (`*_quickjs.rs`, 5 files), `platypus_physics` and `platypus_scene` tests drive
  QuickJS fixtures. Port each to compiled or native tests before deleting `platypus_script`.

### 2.6 Hot reload, Fast Refresh and scene-state saving today (docs/scripting.md 224-333)

1. `bun run dev` rewrites `dist/game.js`; `BundleWatcher` settles 150 ms; `App::reload_scripts`.
2. In place (dev build, `vendor.js` unchanged): `load_game` between `__platypus_hot_begin`/`_end`;
   classes patched on live components, Fast Refresh re-renders; world, physics, audio, React state stay.
3. Full: a fresh runtime loads the bundle; swapped only if load and bootstrap succeed.
4. State: `__platypus_save_state` → JSON `ReloadState` (all scenes, components, `@property` values and
   plain fields with references remapped, DontDestroyOnLoad, quality, clock); `__platypus_restore_state`;
   restored components get `onReload()`. React state is lost on this path.
5. **Compiled programs restart with no state** (platypus-api.md:364).

Native pieces for the same: `platypus_scenes` saves/loads scenes (same JSON format as `scenes.ts`),
`PROPS_<Class>` tables give every `@property` offset, `platypus_ui_native::Runtime::refresh` keeps hook
state across a code swap. Missing: swapping compiled code in a running process (needs the JIT path,
plan phase C) and saving plain (non-`@property`) fields.

### 2.7 Mods and user scripts

Nothing loads them today. The design choice is open (it is the main question of
[platypus-js-dynamic-tier.md](platypus-js-dynamic-tier.md)):

- **Typed TypeScript compiled at load time** by `platypus_ts` into a JIT module: same speed as built-in
  code, same API (the manifests), sandboxed by which manifests the host passes. Needs: codegen over
  `cranelift_module::Module` generically (object or JIT), symbol tables for the engine shims, module
  isolation (struct-id ranges, unloading). Rejects untyped or dynamic code at load with a compiler
  error.
- **Untyped JavaScript**: needs a dynamic tier (the spike shows it can beat QuickJS by 1.2-12.8x on
  small programs, but a complete one is a large project) or QuickJS kept for mods only.

## 3. Removal order

Each step keeps the old path working until the last one. "Unblocks" says what the step makes
possible. Steps in `crates/platypus_app` need its owner (another session) and are marked so.

| # | Step | Touches | Unblocks |
|---|---|---|---|
| R1 | `platypus_api` without QuickJS by default: `default = []`, tests enable `quickjs` explicitly | `crates/platypus_api`, test `Cargo.toml`s | Engine and tool crates build without rquickjs (faster builds); proves the bindings are test-only |
| R2 | Codegen generic over `cranelift_module::Module` (object or JIT), byte-identical objects | `tools/platypus_ts/src/codegen.rs` (no behaviour change) | Load-time compile (mods), in-process reload, the dynamic tier sharing codegen |
| R3 | `platypus_ts::jit::load(src, manifests)` + engine shim symbol tables from `#[platypus_api]` | `tools/platypus_ts` (new module), `crates/platypus_api_macros` (a `symbols()` table) | A host can run a `.ts` file with no build step |
| R4 | A native script host behind the same seam as `QuickJsRuntime` + `ComponentHost` (`NativeHost`, timers, microtasks, scene/physics/audio sync) | **`crates/platypus_app` (needs its owner)** | First example running with no QuickJS |
| R5 | Examples ported one by one (start with `hello`, `boxes`, `sine-circle-demo`, `physics`, `arcade`) | `examples/*` | Coverage of the API surface; finds the gaps (`render.*`) |
| R6 | `render.*` / `editor.*` / cloth / particles as `#[platypus_api]` namespaces | `crates/platypus_engine_api` (and `platypus_render` owner for render) | Examples with 3D content |
| R7 | UI through `platypus_ui_native` + compiled TSX in `platypus_app` (screen and world panels) | **`crates/platypus_app`** | `js/react` no longer needed by ported examples |
| R8 | Reload of compiled code: JIT recompile, swap the class table in `NativeHost`, keep the heap, `platypus_scenes` save/restore when layouts change, `Runtime::refresh` for UI | `tools/platypus_ts`, `platypus_ts_rt::host`, **`platypus_app`** | Dev loop without QuickJS or bun |
| R9 | Editor UI as compiled TSX (`js/editor-ui`, `editor/`) | `editor/`, `js/editor-ui` → TSX | `platypus_editor` without QuickJS |
| R10 | Mods: typed TS at load (R3) behind a capability list; decide untyped JS (dynamic tier or none) | new crate / `platypus_app` | The last reason to keep QuickJS |
| R11 | Delete: `platypus_script`, QuickJS bindings in `platypus_api_macros` (~400 lines), `js/engine`, `js/react`, `js/editor-ui`, `scripts/dev.ts`, QuickJS fixtures and tests; benchmarks compare against bun only | many | No rquickjs in the dependency tree |

R1 and R2 are safe now and unblock the rest; R4/R7/R8 are the critical path and sit in `platypus_app`.
