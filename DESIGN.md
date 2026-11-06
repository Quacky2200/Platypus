# Platypus — Design Document

> Status: **Draft / pre-code**. Captures the decisions made so far. Anything marked *(open)* is still undecided.
>
> Related: [TODO.md](TODO.md) (work plan) · [TESTS.md](TESTS.md) (test plan)

Platypus is an s&box-inspired game engine and editor: a **Rust** engine core, **TypeScript** gameplay, and **React**-based UI panels (the equivalent of s&box's Razor panels), all with fast hot reload. The editor is built using the same UI stack as games.

---

## 1. Goals and non-goals

**Goals**
- Write games in TypeScript, with hot reload that keeps game state.
- Build game UI (HUDs, menus, world panels) with React components and CSS-like styling.
- Build the editor with that same UI stack (dogfooding).
- Run on Windows, Linux and macOS first, then Android, Quest VR, PC VR and iOS.
- Keep big subsystems swappable behind traits so early choices can be revisited.

**Non-goals (for now)**
- AAA rendering parity. The aim is good-looking, efficient rendering; ray tracing is an optional later tier.
- Writing our own physics, audio mixing, text shaping or layout. Use proven crates.
- Networking in early milestones. It is designed for early (see §8) but built later.

---

## 2. Technology stack

| Layer | Choice | Notes |
|---|---|---|
| Window, input, IME, mobile lifecycle | `winit` | Chosen over glfw for mobile/web support and proper IME. |
| Gamepads + rumble | `gilrs` | Weaker on mobile; acceptable since phones are touch-first. |
| Audio playback and mixing | `kira` (on `cpal`) | Steam Audio added later for spatial sound. |
| Graphics | `wgpu` | Vulkan / DX12 / Metal. Shaders in WGSL. |
| UI layout | `taffy` | Flexbox + grid + block. **Yoga kept as a fallback** (§5.3). |
| Text layout / shaping | `parley` + `fontique` (proposed for M3; cosmic-text as fallback) | Rich spans, inline boxes, editing with IME. |
| Glyph rasterization | `swash` into a glyph atlas | Glyphs batch with UI boxes. |
| CSS | `cssparser` + `selectors` (Servo) for stylesheets and matching; Taffy's parser + our own for values | Both CSS files and inline style objects. |
| Physics | `rapier` | Jolt bindings as a possible later option. |
| Scripting language | TypeScript | Gameplay, UI and editor tools. |
| Script engine | Compiled TypeScript (`web_ui_ts`, Cranelift) | QuickJS (the first backend, behind a `ScriptRuntime` trait) was removed 2026-10-28; compiled TypeScript is the only backend. |
| UI framework | React via a custom `react-reconciler` host | |
| Tooling | Bun (bundler, package manager, tests) + `tsc --watch` | Runtime strips types; `tsc` reports type errors. |
| Asset input | glTF 2.0 / GLB | FBX later via Blender or `russimp`. |
| Mesh processing | `meshopt` | |
| Textures | KTX2; BC7 on desktop, ASTC on mobile | |
| File watching | `notify` | Asset hot reload. |
| Networking | `steamworks` (desktop) + `quinn` QUIC (cross-platform) | Behind a transport trait. |
| VR | `openxr` | `bevy_mod_openxr` as a reference. |

---

## 3. Core architecture rule: Rust owns the world, JS owns behaviour

This is the most important rule in the project.

- **Rust owns** entities, transforms, physics bodies, assets, the UI node tree and its layout, audio, and rendering.
- **TypeScript owns** behaviour: component logic, game rules and UI component code.
- **JS only holds handles** (numeric IDs) to engine objects. It never holds pointers, and Rust never holds references into the JS heap.
- **Engine → script callbacks** are looked up by `(component id, method name)` at call time, never stored as JS function references.

Why it matters:
- Throwing away the whole JS context (hot reload) never destroys the game world.
- No dangling references across the boundary.
- Callbacks automatically pick up new code after a reload.

### 3.1 The JS ↔ Rust boundary
- Crossings are **batched**: each React commit and each script frame sends one op buffer, not one call per change.
- Use numeric IDs instead of strings on hot paths. JS strings are UTF-16 internally and Rust strings are UTF-8, so every string crossing is a conversion.
- Heavy per-frame work (physics, animation, transforms, spatial queries) stays in Rust. Avoid thousands of entities calling into JS every frame.

---

## 4. Scripting

### 4.1 `ScriptRuntime` trait (historical)
Removed on 2026-10-28 together with QuickJS (`platypus_script`, `platypus_native`, `@platypus/engine`); compiled
TypeScript (`web_ui_ts`) is the only backend and calls the engine API directly (`docs/platypus-api.md`). The text
below is the original design, kept for the ideas behind the boundary. A thin interface so the JS engine can be
swapped per platform:

```rust
trait ScriptRuntime {
    fn load_bundle(&mut self, name: &str, code: &str) -> Result<(), ScriptError>;
    fn eval(&mut self, code: &str) -> Result<Value, ScriptError>;
    fn call_global(&mut self, name: &str, args: &[Value]) -> Result<Value, ScriptError>;
    fn drain_ops(&mut self) -> OpBuffer;   // ops queued by JS (UI, entity commands)
    fn reset(&mut self);                   // wipe the context for hot reload
    fn set_log_sink(&mut self, sink: LogSink);
}
```

Component calls go through JS host functions installed by `@platypus/engine`
(`__platypus_create`, `__platypus_dispatch(id, method, ...args)`, ...), driven from Rust by
`ComponentHost`, so the trait itself stays engine-agnostic.

Backends considered (none built; compiled TypeScript replaced them):
- **QuickJS (`rquickjs`)**: was the first backend, removed. Small, instant startup, interpreter only. iOS (no JIT
  allowed in apps) would need an interpreter or ahead-of-time compiled game code.
- **`deno_core` (V8)**: fast JIT on desktop and Android.
- **Bun**: experimental backend. Bun is now written in Rust (1.4, Aug 2026) but is still JavaScriptCore underneath and has no stable embedding API.

### 4.2 Component API (gameplay)

```ts
export class Spinner extends Component {
  @property speed = 90;          // shown and editable in the editor inspector

  onUpdate(dt: number) {
    this.transform.rotateY(this.speed * dt);
  }
}
```

- `@property` feeds the editor inspector and serialization.
- `@sync` / `@rpc` reserved for networking (§8).
- A `.d.ts` for the engine API is generated from Rust.

### 4.3 Sandboxing
Game scripts get no filesystem or network access by default; only engine APIs. This is needed for downloadable games, and it's easier with a custom embedded runtime than with a full Node-compatible API.

---

## 5. UI system (React panels)

### 5.1 Pipeline

```
React (TS) ──custom reconciler──▶ op buffer ──▶ Rust UI tree ──▶ Taffy layout ──▶ wgpu draw
                                                    │
                                              hit testing / events ──▶ back to JS
```

- The reconciler host config emits ops: `create`, `setProp`, `setStyle`, `appendChild`, `insertBefore`, `remove`, `setText`.
- Rust owns the node tree, computes layout, renders, and does hit testing. Input events bubble in Rust and are dispatched to JS handlers by node ID.
- Rendering: a batched quad/SDF renderer for rounded boxes, borders and shadows, plus text. `vello` is an alternative *(open)*.

### 5.2 Styling
- A CSS subset (flexbox, grid, box model, colours, borders, radius, shadows, transitions).
- Optionally default to React Native-style behaviour where it's friendlier.
- **Both** CSS files and inline style objects. Bun bundles `import "./ui.css"` into `build/game.css`, which the engine loads with the bundle; `addStyleSheet(cssText)` covers CSS-in-JS. Inline `style` props win over stylesheet rules, as in browsers.

**Visual features we're targeting** (same names and syntax as CSS, camelCase in style objects):

| Area | Properties | Milestone |
|---|---|---|
| Colours | hex, `rgb()/rgba()`, `hsl()/hsla()`, full CSS named colour list, `currentColor` | M2 (box), M3 (`currentColor`) |
| Borders | `border` shorthand (`"2px solid #fff"`), per-side widths, `borderColor` (+ per side later), `borderRadius` with 1–4 values, per-corner and percentage radii | M2 |
| Box shadows | `boxShadow`: comma-separated list, offset, blur, spread, colour, `inset` | M2 |
| Backgrounds | `backgroundColor`, `backgroundImage: url(...)` and `linear-gradient(...)` / `radial-gradient(...)`, `backgroundSize` (`cover`/`contain`/lengths), `backgroundPosition`, `backgroundRepeat` | M2 (gradients), M2/M5 (images via asset loading) |
| Filters | `filter: blur()` (blurs the node and its children), `backdropFilter: blur()` (blurs what's behind, frosted glass); more filter functions later | M2 |
| Opacity | `opacity` as true group opacity (via layers) once layers exist; per-quad multiply until then | M2 |
| Text | `color`, `fontFamily` (with fallbacks), `fontSize`, `fontWeight` (100–900, `bold`), `fontStyle` (`italic`), `lineHeight`, `letterSpacing`, `textAlign`, `textShadow` (list) | M3 |
| Fonts | font imports: `@font-face` in CSS files and a `loadFont({ family, src, weight, style })` API; a bundled default font so games look the same everywhere | M3 |
| Inheritance | `color` and font properties inherit down the tree like CSS | M3 |
| Selectors | type, `*`, `.class`, `#id`, attributes; combinators descendant, `>`, `+`, `~`; `:first-child`, `:last-child`, `:only-child`, `:nth-child()`, `:nth-last-child()`, `*-of-type`, `:root`, `:empty`, `:blank`; `:not()`, `:is()`, `:where()`, `:has()`; `:hover`, `:active`, `:focus`, `:focus-visible`, `:focus-within`; `:enabled`, `:disabled`, `:checked` | M3 |
| Pseudo-elements | `::before` / `::after` as generated children (invisible to React), with `content` | M3 |
| Variables | custom properties `--x` and `var(--x, fallback)` | M3 |
| Media queries | `@media` with size, orientation, aspect ratio, resolution, `prefers-color-scheme`, `pointer`/`hover` | Later |
| Interaction | hit testing, pointer/keyboard events with capture and bubbling, focus and Tab order, `cursor`, `pointerEvents`, text inputs with IME, `overflow` clipping and scrolling | M3 |

**Draw list shape.** Effects mean the draw list is a list of commands, not just quads:
`BoxShadow` (outer, drawn before the box), `Quad` (fill: colour, gradient or image; border; per-corner radii),
`InsetShadow`, `BackdropBlur { rect, radii, sigma }`, `PushLayer { bounds, blur, opacity }` / `PopLayer`, and
later `Text` and `Clip` (for `overflow: hidden`).

**Rendering approach.** Rounded boxes and borders use an SDF shader with per-corner radii. Box and text shadows
use the analytic blurred-rounded-rect technique (one quad each, no offscreen pass). `filter: blur` renders the
subtree into an offscreen layer, blurs it and composites. `backdropFilter` copies the framebuffer region behind
the box, blurs it and draws it clipped to the box shape. Both blurs use a separable Gaussian or dual-Kawase
blur at reduced resolution. Images and fonts load through the asset system and are cached by path.

### 5.3 Layout engine: Taffy first, Yoga as fallback
Layout sits behind its own interface so the engine can be swapped:

```rust
trait LayoutEngine {
    fn create_node(&mut self, style: &Style) -> NodeId;
    fn set_style(&mut self, node: NodeId, style: &Style);
    fn set_children(&mut self, parent: NodeId, children: &[NodeId]);
    fn mark_dirty(&mut self, node: NodeId);
    fn compute(&mut self, root: NodeId, available: Size);
    fn layout(&self, node: NodeId) -> Rect;
}
```

- **Taffy** (default): pure Rust, flexbox + grid + block, follows the CSS spec, used by Bevy, Dioxus and Zed's GPUI.
- **Yoga** (fallback): via Rust bindings to Meta's C++ library, if Taffy has performance or compatibility issues.

**Efficiency plan**
- Layout runs only when something is dirty; Taffy caches results and only recomputes dirty subtrees.
- Style changes from JS arrive batched per commit, so layout runs at most once per frame.
- Text measurement results are cached per (text, font, width).
- Benchmark early (milestone 2): e.g. a 2,000-node editor-like tree, with relayout cost after a single-node change measured against the full tree.

### 5.4 World panels
UI rendered into textures and placed in the 3D world (like s&box WorldPanels). Also the basis for VR menus.

---

## 6. Hot reload

### 6.1 Scripts
1. Bun watches and rebuilds the bundle.
2. **Wipe and rebuild (default):** serialize component state → `ScriptRuntime::reset()` → load new bundle → recreate components → restore properties. Simple and always correct, and it reuses scene serialization.
3. **Patch in place (later optimization):** `Object.setPrototypeOf(instance, NewClass.prototype)`. Instant, but closures, timers, listeners and renamed fields need care.
4. UI components use React Fast Refresh.

### 6.2 Assets
`notify` watches source assets, re-cooks the changed ones, and swaps GPU resources by GUID.

---

## 7. Assets and rendering

### 7.1 Asset pipeline
- **Input:** glTF 2.0 / GLB only to start.
- **Import → cook:** never load source files at runtime.
  - Meshes: `meshopt` optimization, LOD generation, engine binary format.
  - Textures: KTX2, BC7 (desktop) or ASTC (mobile). The cook step takes a target platform.
- **Asset database:** each asset has a GUID in a sidecar `.meta` file, so renames and moves don't break references.
- **`.model` wrapper (JSON):** points to a glTF and adds collision shapes, material overrides and LOD settings. Edited in the editor inspector.
- **Packaging:** cooked assets go into a pack file keyed by content hash. A game ships as a TS bundle plus a pack file.

### 7.2 Lighting, in shippable layers
1. Direct lighting: PBR (glTF metallic-roughness) in WGSL; sun, point and spot lights; cascaded shadow maps.
2. Image-based lighting from HDRIs (irradiance + prefiltered specular, computed at import). **Do this early.**
3. Post-processing: SSAO, bloom, tonemapping (ACES/AgX).
4. Clustered forward rendering for many lights.
5. Baked lighting (later): lightmap UVs (`xatlas`), path-traced baker, denoiser (Open Image Denoise), light probes, reflection probes.
6. Hardware ray tracing (optional high tier): wgpu ray queries (experimental, check status) or raw Vulkan (`ash`).

Quality tiers per platform: mobile and VR use cheaper shadows and no ray tracing.

### 7.3 Shared world geometry
One ray-traceable representation of scene geometry (meshes, materials, per-material acoustic properties) feeds:
- the lightmap baker
- Steam Audio sound propagation and occlusion
- ray tracing acceleration structures
- generated physics colliders

### 7.4 Renderer strategy *(decided 2026-10-17: own renderer)*
- **Own renderer (chosen):** extend `platypus_render` on wgpu. Full control; UI on planes in 3D is the
  existing UI pass rendered to a texture and drawn on a quad. PBR, shadows and the rest are ours
  to build (M5.c/d).
- **Reuse Bevy's rendering and asset crates (not chosen):** faster to good-looking PBR, but brings
  Bevy's ECS and app model, which clash with our loop and script-owned behaviour.

The UI and scripting layers must not depend on renderer internals, so the renderer can improve
underneath them.

### 7.5 Scene storage *(decided 2026-10-17: no ECS)*
A `platypus_scene` crate holds GameObjects in a generational arena with a parent/child transform
hierarchy (glam vectors and quaternions). Components are typed Rust data plus TS script components,
the s&box way. An ECS can go underneath later if profiling asks for it.

---

## 8. Physics, audio, networking

### 8.1 Physics
- `rapier`: rigid bodies, joints, character controller, optional determinism.
- TS components: `Rigidbody`, `Collider`, `CharacterController`; collision events are sent to scripts in the frame's event batch.

### 8.2 Audio
- `kira`: mixer tracks, buses, tweens, streaming; basic panning and distance falloff at first.
- **Steam Audio** (later, C bindings): HRTF, occlusion, geometry-based reverb using §7.3.

### 8.3 Networking
- Server-authoritative, snapshot interpolation, client-side prediction.
- TS API, designed into the component model **from the start**:
  ```ts
  @sync health = 100;
  @rpc.server fire(dir: Vec3) { /* ... */ }
  ```
- Transport trait:
  - **Steam** (`steamworks`): lobbies, relay networking, achievements, **Workshop** (a possible game/asset distribution platform).
  - **QUIC** (`quinn`) with our own relay: mobile and cross-platform play.
- Reference: `lightyear` (Bevy).

---

## 9. Editor

- Built with the same React + Taffy UI stack.
- Core panels: scene hierarchy, inspector (driven by `@property`), viewport(s), asset browser, console.
- Needs from the UI system: docking, tree views, virtualized lists, multiple viewports, undo/redo (command pattern over serialized changes).
- Editor-only TS code (custom inspectors, tools) uses the same runtime.

---

## 10. Platforms and packaging

| Platform | Difficulty | Notes |
|---|---|---|
| Windows / Linux | Easy | exe + bundle + pack. Steam depots. |
| macOS | Medium | `.app`, signing, notarization; needs a Mac or CI Mac. |
| Android | Medium | `cargo-ndk` + Gradle wrapper (or `xbuild`) → signed AAB. Doable from Windows. |
| Quest (VR) | Medium | Android + OpenXR; reuses Android work. |
| PC VR | Medium | OpenXR; swapchain sharing with wgpu is the tricky part. |
| iOS | Medium–Hard | Static lib in an Xcode wrapper; needs a Mac, Apple developer account, and **no JIT** (the QuickJS backend that would have covered this was removed; it needs AOT-compiled game code). Downloadable games are an App Store policy risk. |

`cargo-mobile2` can generate the Xcode and Gradle wrappers.

### 10.1 `Platform` layer
Window, input, gamepad, file paths and app lifecycle go behind a `Platform` layer so `winit` + `gilrs` can be swapped (e.g. for SDL3) if needed.

---

## 11. Milestones

1. **Hello Platypus:** Cargo workspace; winit + wgpu window with a clear colour; `ScriptRuntime` with one backend; Bun builds a TS bundle that logs "hello" from a component.
2. **Boxes:** custom React reconciler → op buffer → Taffy → coloured boxes on screen. **Layout benchmark.**
3. **Real UI:** text, CSS subset, mouse/keyboard/IME events with bubbling, `gilrs` input.
4. **Hot reload:** script wipe-and-rebuild with state restore; React Fast Refresh for panels.
5. **Scene:** entity/component system, serialization, glTF import + cook, PBR + shadows + IBL.
6. **Physics and audio:** `rapier` and `kira` integration with TS components.
7. **Editor shell:** hierarchy, inspector, viewport, asset browser — built in Platypus UI.
8. **Later:** baked lighting, Steam multiplayer, Android/Quest, PC VR, Steam Audio, ray tracing, iOS, packaging and distribution.

Milestones 1–4 together are a useful standalone result: React-driven native game UI in Rust with hot reload.

---

## 12. Open questions

- Text: `cosmic-text` or `parley`?
- Styling: CSS files, style objects, or both? How much of the CSS spec?
- ~~Renderer: own vs reuse Bevy crates (§7.4)?~~ Own renderer (2026-10-17).
- First script backend: QuickJS or `deno_core`? (Decided: neither; compiled TypeScript. QuickJS was built, then removed.)
- Entity model: full ECS or s&box-style GameObject + Component?
- Distribution: Steam Workshop, own platform, or both?
