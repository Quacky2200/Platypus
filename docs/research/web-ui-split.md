# Splitting TypeScript and UI out of Platypus: `web-ui`

Status: in progress on branch `web-ui-split` (worktree `.claude/worktrees/web-ui`), library at
`C:\Users\quack\Desktop\web-ui` (its own git repo and cargo workspace, no remote yet).

## Goal

Everything that turns TypeScript + TSX + CSS into a running UI lives in one library, `web-ui`:
the compiler (`oxc` -> IR -> Cranelift JIT), its runtime, the `#[ts_api]` binding macro, the UI tree
(Taffy, Parley, CSS), the native React reconciler and the UI renderers (wgpu, GLES, CPU). Apps add
their own Rust plugins (`#[ts_api]` namespaces) and a TS entry point.

- **Linux:** `libweb_ui.so` (cdylib), shared by every app that uses it, so its pages are counted once
  in PSS across processes. A launcher is a few lines (Rust or C).
- **Windows / macOS:** linked statically, built like any cargo app.
- **C:** `web_ui.h` + `web_ui_start(&options)`: the library is the entry point (it owns the event
  loop and returns an exit code), the C program only registers plugins and points at a bundle.
- **Platypus** becomes a web-ui app: the engine crates are ts_api plugins plus a host extension.

## Decisions (2026-11-05, with the user)

- `platypus_api` -> `ts_api` (crate, `#[ts_api(..)]` attribute, `ts_api_result*` helpers).
- Engine features of the compiler/runtime go behind a **generic host extension**; web-ui keeps no
  Platypus concepts (`Component`, `GameObject`, physics contacts, renderer tiers).
- web-ui starts as a fresh repo; its first commit names the Platypus commit the files came from.
- Path dependencies: Platypus names web-ui crates once, in root `[workspace.dependencies]` (and the
  `tools/` workspace's), so the worktree (deeper than `main`) and `main` differ in one prefix only.

## Crates

| web-ui crate | from Platypus |
|---|---|
| `ts_api`, `ts_api_macros` | `platypus_api`, `platypus_api_macros` |
| `web_ui_core` | the UI half of `platypus_core`: `Value`, `NodeId`, `Commit`, ops, asset fs (`read_asset`, `with_asset_fs`, `decode_data_url`, `RangeRead`), `project::Blending` |
| `web_ui_ts` | `tools/platypus_ts` minus engine knowledge (see Extension) |
| `web_ui_rt` | `tools/platypus_ts_rt` minus `host.rs` (the component lifecycle moves to Platypus) |
| `web_ui_dom` | `platypus_ui` |
| `web_ui_react` | `platypus_ui_native` |
| `web_ui_bridge` | `tools/platypus_ts_ui` |
| `web_ui_render` | `platypus_render` `gpu.rs`, `surface.rs`, `offscreen.rs`, `ui/`, the UI part of `testing.rs` |
| `web_ui_render_gles`, `web_ui_render_cpu` | `platypus_render_gles`, `platypus_render_cpu` |
| `web_ui_build` | module graph + bundling of `platypus_build` (CSS embedded in the bundle) |
| `web_ui` | facade: `App` (Rust), the C ABI (`web_ui_register`, `web_ui_start`), cdylib + staticlib |

Platypus keeps: scene, scenes, kinds, physics, audio, animation, math, pack, platform (gamepads),
the world renderer (`platypus_render` = `world/` over `web_ui_render`), the `*_api` plugin crates,
`platypus_app`, `platypus_build` (engine steps: assets, cooking, packaging) and the editor.

## The host extension

Measured on `main` (`40f1bfe9`): the compiler knows the engine in these places only.

- `lower/components.rs`: text rewrites (`desugar`, `children_helpers`, `tree_helpers`), the
  `Component` prelude, hook names (`HOOKS`, `CONTACT_HOOKS`), helper names kept from dead-code
  removal (`is_scene_helper`).
- `lower/arrays.rs`, `lower/engine_objs.rs`: `getComponent` / `addComponent` / `getComponents` /
  hierarchy searches lowered to `ts_rt_component_*` runtime calls by class struct id.
- `lower/strings.rs`: `__registerClass` -> `ts_rt_register_class`.
- `layout.rs`, `classmeta.rs`, `builtins.rs`: the hook tables of exported classes, d.ts text.
- `renderer.rs`: the tier table (`Camera` -> world2d ...).
- `engine_objs.rs`: `new X({..})` over a handle class with a static `create` is generic ts_api;
  only the fallback of an option to the object's `transform` is engine-specific.

`web_ui_ts::Extension` (one per host, passed in `JitOptions` / build options):

```rust
pub trait Extension: Sync {
    /// Rewrites the program text after the built-in desugarings; None: unchanged.
    fn rewrite(&self, src: &str, cx: &ExtCx) -> Result<Option<String>, String>;
    /// TypeScript put in front of the program (the base class of host classes).
    fn prelude(&self, src: &str, cx: &ExtCx) -> String;
    /// Top-level helpers dead-code removal must keep.
    fn keeps(&self, name: &str) -> bool;
    /// Host classes: base class, lifecycle hook names, attach/get/getAll method names and the
    /// runtime symbols they lower to (the host provides those symbols to the JIT).
    fn host_classes(&self) -> Option<&HostClasses>;
    /// Program facts the host wants at build time (Platypus: the renderer tier).
    fn analyze(&self, uses: &[&str]) -> serde_json::Value;
}
```

Run time: `web_ui_rt` keeps rooting, the microtask queue and struct allocation public; Platypus's
`platypus_components` crate (from `host.rs`) owns the class table and the Unity-order lifecycle and
supplies `ts_rt_component_*` / `ts_rt_register_class` to the JIT symbol table. Hook calls stay
direct calls through function-pointer tables: no lookup per call.

## CSS

Today `platypus-build` concatenates imported CSS into `build/game.css`. web-ui embeds it: every
`import "./x.css"` the module graph sees (with its `@import`s and `url()` assets) goes into a CSS
section of the bundle, applied when the program starts. A loose `game.css` is still read when the
bundle has none.

## Order

1. Rename `platypus_api` -> `ts_api` in place.
2. Host extension in the compiler; `host.rs` out of the runtime into `platypus_components`.
3. Split `platypus_core` (-> `web_ui_core`) and `platypus_render` (UI vs world).
4. Move the crates to `web-ui`; Platypus uses path dependencies.
5. `web_ui` facade, C ABI, cdylib, embedded CSS, `examples/hello-rust` and `examples/hello-c`.
6. Docs, CLAUDE.md, TODO.md; the full sweep.
