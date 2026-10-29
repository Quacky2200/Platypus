# Rendering

Crate: [platypus_render](../crates/platypus_render/src) (wgpu). It consumes the GPU-free
`Vec<platypus_ui::DrawCmd>` described in [ui-tree.md](ui-tree.md) and does not depend on winit. Design
background: [DESIGN.md](../DESIGN.md) section 5.2; milestone tests `M2-DRW-*`, `M3-TXT-*`,
`M3-EVT-05` in [TESTS.md](../TESTS.md).

## Files

| File | Role |
|---|---|
| [gpu.rs](../crates/platypus_render/src/gpu.rs) | `Gpu` (instance, adapter, device, queue). `headless()` and `with_surface()` |
| [surface.rs](../crates/platypus_render/src/surface.rs) | `WindowSurface`: swapchain, resize, `present_with` |
| [offscreen.rs](../crates/platypus_render/src/offscreen.rs) | `OffscreenTarget`: texture + `read_pixels` (tests, screenshots) |
| [lib.rs](../crates/platypus_render/src/lib.rs) | `Color`, re-exports, `encode_clear` |
| [ui/mod.rs](../crates/platypus_render/src/ui/mod.rs) | `UiRenderer`, the **wgpu executor**: uploads a `FramePlan`, records and submits it |
| [ui/pipelines.rs](../crates/platypus_render/src/ui/pipelines.rs) | `pipelines::Sources` and `CanvasPipelines`: the canvas pipelines, made lazily per blending (`canvas_format(blending)`) |
| [ui/plan.rs](../crates/platypus_render/src/ui/plan.rs) | The **backend-neutral plan**: `plan()` turns a draw list into a `FramePlan` (no GPU object); `PlanResources` |
| [ui/backend.rs](../crates/platypus_render/src/ui/backend.rs) | The `UiBackend` trait the app-facing renderers implement (`UiRenderer` today) |
| [platypus_render_gles](../crates/platypus_render_gles/src/lib.rs) | `GlesRenderer`: executes a `FramePlan` with GLES 3 through EGL / ANGLE (`egl.rs`, `shaders.rs`) |
| [ui/glyphs.rs](../crates/platypus_render/src/ui/glyphs.rs) | `GlyphCache`: GPU-free glyph rasterizing and packing, with a queue of pixel uploads. A character the font lacks (glyph 0) is drawn as a red box of Inter's own `.notdef` letters, "NO GLYPH", cut out, stood up and stacked one per row (colour page, cached per size like any glyph) |
| [ui/quad.wgsl](../crates/platypus_render/src/ui/quad.wgsl) | Instanced shader: boxes, shadows, glyphs, gradients, clips, images |
| [ui/effects.wgsl](../crates/platypus_render/src/ui/effects.wgsl) | Fullscreen passes: blur, composite, blit |
| [ui/atlas.rs](../crates/platypus_render/src/ui/atlas.rs) | The wgpu glyph atlas: the page textures a `GlyphCache` fills |
| [ui/gradient.rs](../crates/platypus_render/src/ui/gradient.rs) | `platypus_ui` gradients to the GPU struct |
| [ui/image_cache.rs](../crates/platypus_render/src/ui/image_cache.rs) | `ImageCache`, GPU-free: `url()` images decoded, SVGs rasterized at a size, keys, eviction, a queue of `ImageUpload`s |
| [ui/images.rs](../crates/platypus_render/src/ui/images.rs) | The wgpu side: `ImageTextures` (a texture and both samplers' bind groups per key) |
| [testing.rs](../crates/platypus_render/src/testing.rs) | Golden helpers and the shared test GPU |

## GPU setup

`Gpu::headless()` and `Gpu::with_surface(target, w, h)` request a `HighPerformance` adapter and
**fall back to a software adapter** (WARP / llvmpipe) if no hardware adapter exists, so CI and
`--screenshot` work without a GPU. Device limits are `downlevel_defaults` raised to the adapter's
resolution limits. `WindowSurface` prefers an sRGB format, starts with `AutoNoVsync` (the app sets it per frame from `vSyncCount`), tolerates 0x0 sizes
(minimized: frames are skipped) and reconfigures on `Outdated`/`Lost`/`Suboptimal`.

### Choosing the renderer

`platypus-app` is the one host of every game; it picks its renderer per run (`crates/platypus_app/src/host.rs`):

1. `--renderer vulkan|gles|cpu` on the command line (`auto` is the default);
2. else `renderer` in `project.json` (`platypus-build package --renderer` writes it into a packaged exe's copy);
3. else the bundle's **tier**, what the compiled program needs from the renderer, found at build time:
   `ui` starts on GLES, `world2d` and `world3d` on wgpu with the world renderer. A bundle without the field
   (older, a bare `.ts` file) counts as `world3d`. A QuickJS bundle is refused.

| `renderer` | Path | World renderer |
|---|---|---|
| left out | from the tier, as above | when the tier is not `ui` |
| `"vulkan"` | wgpu: Vulkan by default on Windows and Linux, DX12 with `WGPU_BACKEND=dx12`, Metal on macOS | built at once |
| `"gles"` | GLES through EGL (ANGLE linked in on Windows), else wgpu with small GPU allocations | never on GLES ("3D Not Supported") |
| `"cpu"` | `platypus_render_cpu` through softbuffer (no GPU API), else as `gles` | never on the CPU ("3D Not Supported") |

Any other value is a warning (at build and at start) and counts as left out; the old `type` (`game` / `app`)
gets a one-line warning and is ignored. The log says what was chosen and why
(`renderer: gles (the program needs ui)`). A `gles` or `cpu` renderer for a program whose tier is not `ui` is a warning
at build, package and start (`renderer is gles but this game uses the world (Camera, Light): it will show 3D Not
Supported`); at run time such a window shows a black screen with red "3D Not Supported" once the game asks for the
world (a camera or a mesh or texture upload: `WorldViews::needs_world`), the safety net for a wrong choice. The
light paths' wgpu fallback builds the world renderer on first use instead (`lazy_world.rs`), so there the world draws.

**The tiers** (`tools/platypus_ts/src/renderer.rs`) are one table keyed on the API's class and namespace names:
`world2d` for the scene itself (`GameObject`, `Transform`, `Scene`, `SceneManager`, `Prefab`, and the prelude class
`Component`: a component class, registered or not, or a `register(..)` call that runs), cameras (`Camera`,
`Cameras`, `CameraFollow`), sprites (`Sprite`, `SpriteRenderer`, `Texture`), 2D lines and shapes (`LineRenderer2D`,
the TypeScript `Shapes` helpers draw through it) and world text (`TextMesh`); `world3d` for meshes, materials,
lights, primitives, models, particles, cloth and rope, mirrors and reflection probes, fog volumes, render and
graphics settings, post-processing, gizmos, `LODGroup`, `CameraOrbit` and world panels. Everything else (React and
CSS, windows, timers, input, audio, files, maths, `Time`, `Debug`) is `ui`. A program with a scene object or a
component runs the world even if it draws only UI, so it is at least `world2d`; a camera is 2D because a 2D game
needs one too; what makes a world 3D is what it draws. Only code counts: the compiler reports the engine symbols
its code calls after the unused top-level functions were left out (`Output::externs`; facade chunks named only in
strings or comments are not added) and the names it uses without a call (`Output::uses`: `Component`, from
`class X extends Component` or a lowered `register(..)`, found in the parsed program, not its text). The
`GameObject` hierarchy helpers the compiler appends when the text says `.children` (a React or markdown tree is
enough) are lowered only when code calls them. `platypus-build` maps the symbols to their owner, adds `GameObject`
and the component types of the scene and prefab files the build would pack (`"type": "MeshRenderer"`, `"$type":
"Material"`), and writes the highest tier with the names that set it into `build/game.pbundle` (`"renderer":
"world3d", "uses": ["Camera", "Light", ...]`; `platypus-build build` prints it, `check` has a column).
`platypus_build/tests/renderer_tiers.rs` checks the table against the real API: every tagged name exists (or is
the prelude's `Component`), and every class and namespace of `platypus_render_api` is tagged. The examples:

| Tier | Examples |
|---|---|
| `ui` | alpha-probe (translucent fills over a known colour, for reading pixels back: `blending`), article, boxes, css, flow, form, hello (a timer, no component), launcher, md-reader, table, text, windows |
| `world2d` | physics (UI boxes over rapier bodies on GameObjects), raindrop-demo, shapes, sine-circle-demo, splines |
| `world3d` | every other example (38), among them clock (its 3D faces), emoji (`WorldPanel`), fishtank (`RenderSettings` fog), platformer (`ParticleSystem`) |

Until 2D content draws on GLES (TODO.md), `world2d` starts on wgpu like `world3d`.

**The light wgpu path** (the GLES or CPU path's fallback) asks the GPU allocator for `MemoryHints::MemoryUsage`
(small allocation blocks, `platypus_render::set_low_memory`) instead of the default `Performance`, and builds its 3D
`WorldRenderer` only if something asks for it (`lazy_world.rs`). For a clock that took private bytes from ~306 MB to
~135 MB on Vulkan (NVIDIA, Windows). wgpu cannot use ANGLE on Windows (its Windows GL backend is WGL only), so GLES is a
native renderer behind the same draw list (`platypus_ui::DrawCmd`).

### The GLES executor (`crates/platypus_render_gles`)

`GlesRenderer` executes the same `FramePlan` with GLES 3 (`glow`), for `platypus-app`'s light path. Its context
comes from EGL (`egl.rs`): on Windows ANGLE, which draws through Direct3D 11 (a bare context measured ~61 MB private
against ~138 MB for NVIDIA's OpenGL driver), linked into the program (below); without that feature, and elsewhere,
`khronos-egl` loads the library at run time (ANGLE's `libEGL.dll`, Mesa's EGL): `EglContext::headless(Some(dir))`
loads it from `dir` (`SetDllDirectoryW`), else the usual search (next to the program).

What differs from the wgpu executor (`shaders.rs` is a GLSL ES 3.00 port of `quad.wgsl`):

- **Tables are float data textures.** GLES 3.0 has no storage buffers, so the gradient table
  (`u_grads`, 12 texels per `GpuGradient`) and the clip/transform table (`u_clips`, 5 per `GpuClip`) are
  `RGBA32F` textures read with `texelFetch`, one entry per row (polygon clips read their points as a flat
  `vec4` array across rows, as in WGSL).
- **Instances** are instanced vertex attributes re-pointed at the draw's byte range (ES 3.0 has no
  base-instance draw); six vertices per instance, positions from `gl_VertexID`.
- **Canvases** are `RGBA8` textures holding sRGB values (the default `Blending::Srgb`) or `SRGB8_ALPHA8` for
  `linear` (`GlesRenderer::set_blending`; `u_globals.w` tells the shaders which); the glyph pages are
  `R8` and `SRGB8_ALPHA8`, filled from the shared `GlyphCache`'s upload queue. The final blit has a mode
  (`u_mode`: 0 copy, 1 encode, 2 decode) that converts between the canvas and the target (`Target::srgb`: whether
  the target encodes on write), see "Blending" above.
- **`gl_FragCoord` is y up**; the fragment shader turns it into the plan's y-down logical pixels.

Done: the quad pass (boxes, borders and their styles, gradients, outer and inset shadows, glyphs, wavy
decorations, clips and polygon clips, transforms), the glyph atlas, images (`fs_image`: repeat, space,
`cover`/`contain`, rounded clips, `image-rendering: pixelated`, tints, border-image pieces; PNG and JPEG
from the shared `ImageCache`, SVGs rasterized at their drawn size; `SRGB8_ALPHA8` textures) and layers
(`effects.rs`, a port of `effects.wgsl`): `Copy` is a `glBlitFramebuffer`, `Blur` two passes of the
separable Gaussian (reads clamp to a bounds uniform from `plan::blur_reach`, flipped to GL's y up: the canvas, or the element's box for a `contained` backdrop blur), `Composite` opacity, clip, the `filter` colour matrix, drop
shadow tint and shift, the mask texture and the 15 blend modes (a separate program that mixes the layer with a
copy of its parent), so opacity groups, `filter`, drop shadows, masks, `mix-blend-mode` and `backdrop-filter`
work. Scissor rectangles are flipped to GL's bottom-left origin; a clear wipes the whole canvas as a wgpu load op
does; canvases filter linearly (a shifted drop shadow samples between texels). `render_overlay` clears
the canvas transparent and blends it onto what the target holds (premultiplied, `ONE`/`ONE_MINUS_SRC_ALPHA`).
`render_over` (the GLES side of wgpu's seeded canvas) instead starts the canvas as a copy of the target (a blit into a
plain `RGBA8` texture, then a draw that decodes it into the sRGB canvas) and replaces the target with the result, so a
`backdrop-filter` blurs what someone else drew there: a compositor drawing its shell over its windows (M2-GLES-14).
`EglContext::window` makes a context on a winit window (Win32 HWND, Xlib; anything else is an error). Its
surface is plain, not ANGLE's sRGB one (reading that back returned unencoded bytes, so colours were off), so
`Target::srgb` is false: in `srgb` the blit copies the canvas, in `linear` it encodes itself. `platypus-app`'s light path (`gles_gfx.rs`) draws its one window this way
(no 3D world, world panels or secondary windows), swaps with vsync, skips a frame whose draw lists equal the
last one's, and falls back to wgpu when EGL is missing (`PLATYPUS_ANGLE_DIR` names ANGLE's folder; `PLATYPUS_GLES=0`
forces wgpu). `tests/vs_wgpu.rs` draws scenes through both executors and compares pixels
(`M2-GLES-02..13`: identical so far, 0 to 1 in 255); it and `tests/context.rs` need an EGL driver
(the linked-in ANGLE, or `PLATYPUS_ANGLE_DIR` without it) and skip without one. They also skip, with a printed
note, when the driver can't make a GL object (glow's "Unable to create ... object", seen on Mesa's llvmpipe) or the
renderer can't start (`vs_wgpu.rs` runs the GLES side through `or_skip`; `context.rs` skips when it can't make its
texture).

Kept small (see "Memory" below): the display comes from `eglGetPlatformDisplayEXT` with Direct3D 11, the
hardware device and **no D3D debug layer** (`egl::angle_display_attribs`; ANGLE turns the layer on whenever its
asserts are compiled in, which mozangle's build does; `PLATYPUS_ANGLE_DEBUG=1` leaves it to ANGLE); the config
asks for no depth, stencil or samples and the log says what it got (`EGL config: 32 bits, depth 0, stencil 0,
0 samples`). Programs compile the first time a frame needs them (`ProgramKind`: `Quad` and `Blit` at start,
the image, blur and composite ones later; `M2-GLES-14`), and the colour glyph page stays 1x1 until a colour
glyph (emoji) arrives. A second after the first frame `gles_gfx.rs` trims the driver once
(`EglContext::trim_device`: `IDXGIDevice3::Trim` on ANGLE's device, what ANGLE does for a suspended UWP app;
`PLATYPUS_GLES_TRIM=0` turns it off; `M2-GLES-15`). Screenshot runs never trim (they stop sooner).

**Static ANGLE (feature `static-angle`, on by default; Windows).** `platypus_app` and `platypus_render_gles` compile
ANGLE (Mozilla's fork, the `mozangle` crate, `egl` feature: EGL + GLES over Direct3D 11) into the program, and `egl.rs`
calls `egl*` directly (`khronos-egl`'s `static` + `no-pkg-config`, declared as a Windows-only dependency so the
features never reach other platforms); `PLATYPUS_ANGLE_DIR` is then ignored. **Building it needs MSVC and libclang**
for mozangle's bindgen: set `LIBCLANG_PATH` to the `bin` folder of an LLVM 20 or newer install for the command
(for example `LIBCLANG_PATH='C:\llvm-23.1.4\bin' cargo build --release -p platypus_app`; it is not set in any
committed file). LLVM 23.1.4 builds it with nothing else (a clang older than 20 against Visual Studio 18's STL would
also need `BINDGEN_EXTRA_CLANG_ARGS="-D_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH -D_MSVC_STL_USE_ABORT_AS_DOOM_FUNCTION"`).
Without libclang, `cargo build -p platypus_app --no-default-features` leaves ANGLE out: GLES then loads `libEGL.dll`
(`PLATYPUS_ANGLE_DIR`) and falls back to wgpu without it. ANGLE's C++ takes about a minute and a half cold (cached
afterwards, once per profile) and adds ~7 MB to each exe: mozangle links `libEGL`/`libGLESv2` whole-archive. The linker prints `LNK4217` warnings (ANGLE's EGL entry points marked dllimport but
defined locally; harmless). `M2-GLES-01..13` run on Windows without any DLL (identical to wgpu, max diff 1 in 255).
ANGLE is BSD-3-Clause and bundles more third-party code: `THIRD-PARTY-NOTICES.md` (repository root, generated by
`tools/platypus_notices`; its ANGLE part is `tools/platypus_notices/hand-written.md`) has their notices, the host prints
it with `platypus-app --licenses`, and `platypus-build package` writes it beside every packaged exe.

`crates/platypus_render_cpu` is a second consumer of that draw list: `vello_cpu` + `softbuffer`, no GPU API at all. It
draws boxes, borders (also per side), gradients, images and SVGs (decoded lazily, downscaled to the size drawn,
dropped when unused), shadows, text, clips, layers, blur and backdrop blur (only inside the element's box), and affine and perspective transforms (a
perspective subtree is drawn flat, cached and warped). **`renderer: "cpu"`** (or `--renderer cpu`; never chosen
automatically) makes it `platypus-app`'s third UI-only path (`cpu_gfx.rs`): the window through softbuffer (resized with
each present), the UI and the FPS counter as two layers of one render, an unchanged frame neither drawn nor presented,
the loop capped at the monitor's refresh rate, 60 Hz when unknown (softbuffer has no vsync; `gles_gfx::frame_cap`), "3D Not Supported" when the game asks for the world, wgpu's light
path when softbuffer can't make a surface. `--screenshot` writes the pixels the CPU drew (no GPU involved). What it
does not draw (each logged once): `border-image`, `mask`s (the layer is drawn unmasked), drop shadows and colour
matrices on layers (`filter: drop-shadow()`, `grayscale()`, `sepia()`, ...), border styles other than solid (drawn
solid), the 3D world, world panels and secondary windows. Its memory is in "Memory" below. Next to the GLES
picture of `css` the CPU one differs only where the list above says: its dashed borders (the `:empty` slot and the
filled slot) are drawn solid. It is far slower than GLES on animated UI: `launcher`'s drifting background holds
~30 fps at 1280x720 and ~18 fps at 1920x1080 on about two cores, where GLES holds 60 fps at ~9% of one core, and
the launcher's glows and frosted (`backdrop-filter`) controls come out darker (colour-matrix filters on layers).
Radial gradients are drawn as a two-point radial from the first stop's radius to the last's, so
`transparent 44px, red 52px` fades over those 8 px only. Its first integration test is
`tests/gradients.rs` (`M2-CPU-01`). It stays an opt-in.

### Memory

Three numbers, all from `GetProcessMemoryInfo` (`PROCESS_MEMORY_COUNTERS_EX2`) or the matching counters:

- **Private working set**: the process's own pages in RAM (`\Process(platypus-app)\Working Set - Private`). This is
  Task Manager's "Memory" column (active private working set), so it is the number to hold UI apps to.
- **Private bytes**: memory committed to the process (`PrivateMemorySize64`, Process Explorer's "Private Bytes",
  Task Manager's "Commit size"). On the GPU paths it includes what the graphics driver commits for itself, most of
  it never resident.
- **Working set**: every resident page, the shared images of the exe and DLLs (the NVIDIA driver's are over 190 MB
  of image) included.

**Measuring.** Build the release host (`cargo build --release -p platypus_app`) and the example, start
`build/release/platypus-app examples/<game> --renderer gles` (a window opens), wait ~8 s without touching it, read the
three numbers (PowerShell: `$p.PrivateMemorySize64`, `$p.WorkingSet64`, and `WorkingSetPrivate` of
`Win32_PerfFormattedData_PerfProc_Process` for the pid), close the window. `PLATYPUS_MEM_TRACE=1` prints the three at
every start-up stage to stderr (the compiler's phases, the host's set-up, the window, the EGL context, the GLES
renderer, the first frames, the driver trim; `mem_trace.rs`); `PLATYPUS_MEM_TRACE=regions` also lists the heaps and the
largest committed private allocations once the frames run. A run varies by about 2 MB; about one run in eight the
trim frees nothing and private bytes stay ~14 MB higher.

Measured 2026-10-28 (RTX 5080, Windows 11, release, static ANGLE, 1280x720 window, FPS counter on), MB as private
working set / private bytes / working set:

| Game (`ui` tier) | GLES before | GLES now | CPU | light wgpu, Vulkan | light wgpu, DX12 |
|---|---|---|---|---|---|
| css | 41.6 / 134 / 93 | **29.5** / 100 / 87 | 17 / 20 / 46 | 91 / 163 / 166 | 99 / 238 / 174 |
| launcher | 45.2 / 151 / 100 | **34.8** / 138 / 94 | 37 / 41 / 69 | 97 / 200 / 176 | 113 / 276 / 192 |
| md-reader | 60.4 / 149 / 118 | **45.5** / 119 / 103 | 35 / 38 / 70 | 70 / 175 / 128 | 83 / 269 / 142 |
| hello | 39.0 / 128 / 88 | **23.3** / 95 / 72 | | | |
| boxes, form, text | 41-45 / 131-183 / 93-97 | **25-28** / 98-129 / 75-78 | | | |

The "light wgpu" columns are the GLES choice with `PLATYPUS_GLES=0` (wgpu with small GPU blocks, no world renderer)
and `WGPU_BACKEND`; `--renderer vulkan` (wgpu with the world) is ~80 / 331 / 135 on css. ANGLE from a VS Code install
(`--no-default-features`, `PLATYPUS_ANGLE_DIR`) measures the same as the linked-in one (css 31 / 102 / 81, launcher
36-38 / 142-196 / 89-96): what remains is the driver and our own memory, not the ANGLE build.

**Where it goes** (css on GLES, private bytes / private working set, from the trace): the host before any renderer
~10 / ~7.5 MB (process start 2.4, engine manifests 3.3, compile and JIT ~2, UI, fonts and CSS ~1; the compiler's
arenas are freed after the JIT, a peak of 53 MB private for md-reader's 2.4 MB source falls to 12 MB; audio is not
started for a game that plays nothing, fontique scans lazily); the window +1.5; the EGL context (the Direct3D 11
device, NVIDIA's user-mode driver, ANGLE's renderer) **+60 / +9**: the same as a bare ANGLE context in a tiny
program, so inherent; the GLES renderer +5 / +4; the first frame +37 / +7, all of it the first instanced draw (an
empty frame costs +11, a trivial fragment shader as much as the full one, glyph uploads ~2): the driver's
compiler and command memory, of which the trim gives back ~14 MB of private bytes. What changed: no debug layer
(-12 private, -10 private working set), lazy programs and colour page (-8 private), the trim (-14 private), the
dropped source text (md-reader -2.4). Tried without effect: frame latency 1, ANGLE feature level 10.1 or 11.0,
compacting the heaps, program binaries (no `d3dcompiler_47.dll` at all on a second run, same memory). Open: a
DirectComposition surface (`EGL_DIRECT_COMPOSITION_ANGLE`, ~4 MB less private, not adopted: it changes how the
window composes), a glyph mask page that grows from 512 px (~3-5 MB), program binaries for start-up time (~0.7 s
of linking).

Frame latency (`desired_maximum_frame_latency`) is 1 by default and 2 on DX12, where 1 made live
resizing jitter. `PLATYPUS_FRAME_LATENCY` (1 or 2 only; more just adds input lag) overrides it. On
Windows the window class background is set to black (in `platypus_platform`), so any area exposed by a
resize before the next frame is black, not white.

## Colour and units

- Draw commands are in **logical pixels**; `RenderTarget { view, format, size (physical), scale }`
  says how to map them. Shaders receive `globals.viewport` and `globals.scale`, and every quad
  grows by one physical pixel for anti-aliasing.
- Colours are premultiplied by the plan in the frame's blending space (below) and blended with
  `PREMULTIPLIED_ALPHA_BLENDING`.

### Blending: sRGB (default) or linear light

How translucent UI colours combine is one setting for a whole frame, `platypus_core::project::Blending`:

- **`srgb`** (the default, as browsers): `out = src * a + dst * (1 - a)` on the 0-255 sRGB values. A 25%
  white fill over `#3060c0` gives `100,136,208`.
- **`linear`** (Platypus before 2026-10-30): the same sum on linear-light values, encoded afterwards. The
  same fill gives `142,157,210`: fades and translucent fills look lighter and greyer than in a browser.

It is chosen by `"blending": "srgb" | "linear"` in `project.json` (an unknown value warns and means `srgb`),
and by embedders with `UiRenderer::set_blending` / `GlesRenderer::set_blending` (default `srgb`). It holds
for everything the UI renderer composites: the screen UI (alone or over the 3D world), world panels' textures
and secondary windows. The 3D world pass is not affected: lighting, translucent materials, particles, bloom
and fog stay in linear light, and the world reaches the UI as an encoded picture, like an image.

**One place says it.** `PlanParams::blending` goes into `FramePlan::blending`. The plan premultiplies every
colour (fills, borders, text, decorations, shadows, drop-shadow and image tints, the clear colour) in that
space (`canvas_premultiplied`); executors read only `FramePlan::blending` to pick their canvas format and one
shader switch.

**How `srgb` works.** The canvases are plain 8-bit textures (wgpu `Rgba8Unorm`, GLES `RGBA8`) holding
premultiplied sRGB values, so the hardware's premultiplied blend is the browser's. Each area then follows:

| Area | In `srgb` |
|---|---|
| Solid fills, borders, decorations | Blended on sRGB values by the hardware |
| Gradients | Interpolated in premultiplied sRGB (as before, as CSS) and kept in sRGB (no conversion to linear) |
| Opacity groups, layers | The composite multiplies and blends canvas values: in sRGB |
| `filter: blur()`, drop shadows, `backdrop-filter` | The blur averages canvas values: sRGB, as browsers |
| Box shadows | Analytic coverage times the sRGB colour |
| Filter colour functions, blend modes, luminance masks | Already worked on sRGB values; the conversions around them are skipped |
| Images, colour glyphs (emoji) | Their textures stay sRGB-format (decoded on sampling); the shader re-encodes the sample. Filtering between texels is in linear light, a sub-pixel difference from browsers |
| Text | Mask coverage times the sRGB colour, as browsers. The coverage boost for dark text, which made up for linear blending's thin dark text, is off |
| `groove`/`ridge`/`inset`/`outset` shade | Half the colour in sRGB (0.22 of it in linear light: the same look) |

**Edges.** The final blit decodes the canvas when the target encodes on write (wgpu sRGB surfaces and
offscreen targets, world panel textures, GLES `SRGB8_ALPHA8` targets) and copies it as is otherwise (the GLES
default framebuffer). Over the 3D world the canvas is seeded from the world (`render_over`), encoding it.
Blending a canvas onto an sRGB-encoding target in hardware would happen in linear light, so in `srgb` the app
always draws screen UI over the world with `render_over` (the world into the backdrop texture first), and the
FPS counter joins the same draw list under a scale transform. `render_overlay` onto an encoding target
composites in linear light in either mode (for embedders: use `render_over`); onto a plain target (the GLES
window) it is exact.

**The CPU renderer** (`vello_cpu`) blends 8-bit sRGB by itself: it is always `srgb` (`linear` is logged once
and ignored).

**`linear`** keeps the earlier path: `Rgba8UnormSrgb` / `SRGB8_ALPHA8` canvases, colours converted to linear,
gradients and images decoded, filters and blend modes converting to sRGB and back, the dark-text boost on.

## `UiRenderer::render`

One call per frame: `render(gpu, &RenderTarget, clear, &[DrawCmd]) -> RenderStats`. It has three
stages, and the first is **backend-neutral**: `plan::plan(cmds, &PlanParams, &mut dyn PlanResources)`
returns a `FramePlan` (`instances`, `gradients`, the clip/transform `clips` table, `steps`, and the
count of frame-sized `textures`) and holds no GPU object. What planning needs from a renderer
comes through `PlanResources`: `glyph` (place a glyph in the atlas), `image` (an image's natural
size) and `svg_at` (an SVG rasterized for a size). `WgpuResources` in mod.rs is the wgpu one; a
second renderer (the GLES one for `platypus-app`, through ANGLE) supplies its own, uses the same
`GlyphCache` and `ImageCache` (their pixels are queued as uploads, written by the renderer after planning;
`ImageInfo::key` names an image's texture in `Step::Draw`), and executes the same `Step`s over its own textures. `UiBackend`
(backend.rs) is the app-facing trait; the wgpu `UiRenderer` implements it. The planner is tested
without a GPU (`tests/plan.rs`, `M2-PLN-01..07`, plus the blending plan test `M2-BLEND-01`); the golden tests
(`tests/ui.rs`, pinned to `Blending::Linear`) check the wgpu executor, and `tests/blending_over.rs`
(`M2-BLEND-*`) its `srgb` default over the world.

### 1. Plan: commands to instances and steps (`plan.rs`)

Walk `cmds` once, producing:

- `instances: Vec<Instance>` (9 x `vec4`, matching `struct Instance` in quad.wgsl). `params.x` is
  the **kind**: `KIND_QUAD`, `OUTER_SHADOW`, `INSET_SHADOW`, `IMAGE`, `GLYPH`, `KIND_BORDER`,
  `KIND_GRADIENT_PIECE`, `KIND_WAVY`. `rect`/`radii` (horizontal) and `radii_y` (location 7) are the shape, `p0/p1` kind-specific data (border widths, shadow source box, atlas rect),
  `color0/color1` fill and border, `params.y/z/w` blur sigma / gradient index / clip index, and
  `xf.x` (location 8) the transform index (1-based, 0 = none): the instance's rect and data are in
  that transform's plane.
- `gradients: Vec<GpuGradient>` (up to 8 stops each, extras dropped with a warning).
- `steps: Vec<Step>`: the ordered render program, over a pool of full-size textures:

| Step | Meaning |
|---|---|
| `Clear{target,color}` | Deferred (applied as the load op of the next pass on that target) |
| `Draw{target, range, image}` | One instanced draw of `instances[range]`; with `image: Some((url, pixelated))`, the image pipeline for one background image (nearest-neighbour bind group when `pixelated`) |
| `Copy{src,dst}` | Snapshot a texture (for backdrop blur) |
| `Blur{tex,temp,area,sigma,contained}` | Separable Gaussian: horizontal into `temp`, vertical back. `plan::blur_reach(area, radius, contained, size)` gives the rows the horizontal pass writes and the bounds (first x, first y, last x, last y) that texel reads clamp to: the whole canvas, or with `contained` exactly `area`, repeating its edge pixels |
| `Composite{src,dst,area,opacity,clip,blend,matrix,tint,shift,mask}` | Alpha-composite a layer, scissored, with clip; `blend: Option<(mode, backdrop)>` for a blend mode; `matrix` is the `filter` colour matrix, `tint`/`shift` make a drop-shadow copy, `mask: Option<(texture, MaskMode)>` multiplies by a mask |

**Batching.** Consecutive boxes, shadows and glyphs stay in one `Step::Draw` regardless of draw
order, because the instance order is the paint order and a single pipeline draws all kinds
(the shader switches on `kind`). A batch only ends when something needs a different pass: a layer
push/pop, a backdrop blur, or a background **image** (needs its own texture bind group): colour is
drawn, the batch is flushed, each image layer is drawn alone (bottom first), then the border is
drawn on top. Image layers are clipped to `ImageFill.area`/`radii`. `images::repeat_tiles` expands
`round` and `space` into whole tiles (the gap goes to the image shader in `p1.xy`).

**Blend modes.** A `PushLayer` with a blend mode other than normal composites with
`Step::Composite.blend`: the parent is copied first as the backdrop, then `composite_blend_pipeline`
(`fs_composite_blend` in effects.wgsl, binding 5 is the backdrop) mixes source and backdrop itself
in sRGB with a replace blend state.

**Layers.** `PushLayer` flushes, takes a pooled texture, clears it to transparent and makes it
`current`. `PopLayer` flushes, optionally adds `Blur` (`filter: blur`, sigma = blur x scale), then
`Composite` into the parent with `opacity`, scissored to the part of `bounds` the surrounding clip
allows. `take_texture`/`pool` reuse textures between frames. Unbalanced pushes/pops log a warning
and are repaired. Open layers live on `layer_stack` as `OpenLayer` (own texture, mask, parent, bounds,
blur, opacity, blend, matrix, shadows, outer clips).

**Filters.** The order is fixed: blur, then drop shadows, then the colour matrix (at composite),
whatever order the CSS lists them. `PopLayer` blurs the layer, then for each `LayerShadow` (last
first, so the first listed ends on top) copies the layer, blurs the copy by the shadow's sigma and
composites it under the layer with `tint` (the source's alpha in the shadow colour, linear
premultiplied) and `shift` (offset x scale, physical px). The layer itself then composites with
`matrix`. `filtered_source` in effects.wgsl does the tint and the matrix: it un-premultiplies, goes to
sRGB, applies the 4x5 matrix (`CompositeParams.matrix` as WGSL columns plus `offset`), clamps and
converts back, like CSS filter functions in browsers. `shift.z`/`shift.w` switch matrix and tint on.

**Masks.** `BeginMask(mode)` (inside a layer, after its content) flushes, takes a second pooled
texture, clears it and makes it `current`, so the following quad (the mask layers) draws there;
`OpenLayer.texture` keeps the content texture. `PopLayer` then passes `mask: Some((mask, mode))` to
every `Composite` of that layer, shadows included. The composite bind group gets the mask at
binding 6 (the source texture when there is no mask); `mask_at` returns 1 with no mask, the mask's
alpha for `Alpha`, or its sRGB luminance times alpha for `Luminance`, and `fs_composite` and
`fs_composite_blend` multiply the colour by it.

**Backdrop filter.** `Copy` the current target, blur the copy over the element's `rect` only, as a
`contained` `Blur` (reads stay in the box, so a neighbouring glass box does not bleed in; skipped
when `blur` is 0, a colour-only filter), then composite it back using a **rounded-rect clip** of the
box, with `BackdropBlur.matrix` as the composite's `matrix`. The result is frosted glass of
whatever was already drawn. A `contained` blur reflects taps that fall past the box's edge back
in, as Chrome does (`effects.wgsl` `blur_tap`, the GLES `BLUR_FRAGMENT`; `M2-DRW-12`). Layer
blurs and drop shadows clamp to the canvas instead.

**Clip table.** `PushClip` appends a `GpuClip { rect, rx, ry, bounds, parent }` to a per-frame
table and pushes its index on `clip_stack`; `bounds` is the intersection of the whole chain.
Instances do **not** break batches for clips: each carries its clip index in `params.w`
(`tag()` stamps the pending instances when the clip stack changes). In the shader,
`clip_coverage` multiplies coverage along the parent chain (up to 32 deep) using an approximate
elliptical rounded-box signed distance. A polygon clip (`clip-path: polygon()`/`path()`) stores its
points after the entry, one `vec4` each (x, y, contour start, contour end); `info.y` is the kind,
`info.z` the first point vec4 and `info.w` the count. `clip_word` and `sd_polygon` (in both
shaders) evaluate it. `info.y` also carries the transform the shape is in, as `kind + 4 x transform
index` (kind 0 box, 1 nonzero, 2 evenodd); `clip_coverage_with` splits it with `% 4` and `/ 4`.
Consequences:

- Anything whose bounds do not intersect the active clip's bounds is **culled** in the plan.
- The vertex shader shrinks each quad to the clip bounds, so overdraw outside the clip costs nothing.
- A layer starts with an empty clip stack (its content is unclipped) and is clipped **when
  composited** (`Composite.clip`), so a blur inside a clipped layer is cut off correctly.
- `BackdropBlur` registers its own rounded clip entry (which is why `RenderStats.clips` counts it).

**Transforms.** `PushTransform(h)` stores the homography and its inverse as an entry in the same table
(`Clips::push_transform`, packed into the `GpuClip` fields; nothing chains to it and its bounds are
unlimited) and pushes `(index, h)` on `xf_stack`; `PopTransform` pops it. `tag()` stamps pending
instances with the clip index and `xf.x` together, so transforms do not break batches either. A
`PushClip` inside a transform stores its shape in the element's plane (`Clips::push` with the
current transform; its `bounds` are `Homography::map_rect` of the shape) and the shader tests it in
that plane. In the plan, culling bounds and `PushLayer`/`BackdropBlur` bounds go through `world()`
(`map_rect` with the current homography). Shaders:

- `vs_main` maps each corner of the (grown) quad with `xf_apply` (a perspective divide, so straight
  edges stay straight). The shrink-to-clip-bounds step is skipped when `xf` is set, as those bounds
  are in window space.
- Fragments compute the window point, then `local_point` maps it back with `xf_unapply` to the
  instance's plane for the shape, glyph atlas lookup and gradients (`shade`, `glyph`, `fs_image`).
  `clip_coverage` runs on the window point and un-applies each clip's own transform.
- Glyph masks of text under a rotation, skew, uneven scale, mirror or perspective are sampled
  nearest-neighbour in the plane, so such text is slightly jagged ([differences.md](differences.md)).
  Text under a map that only scales evenly and moves takes the path under **Text** instead.

**Text.** For each `GlyphRun`, glyph positions are snapped to whole pixels vertically and to
quarter pixels horizontally (`SUBPIXEL_BINS` = 4; the sub-pixel offset is baked into the cached
raster). `GlyphAtlas::glyph` returns an atlas entry; each glyph becomes a `KIND_GLYPH` instance whose
`p0` is the atlas rect. `params.y` is 0 for a normal mask, 1 for a colour glyph, 2 for a crisp mask
(`font-smooth: never`; the shader thresholds coverage at 0.5). A `GlyphRun.stroke` draws a second
glyph per glyph from an outline-stroked raster (`atlas.glyph(..., stroke_px)`) in the stroke colour
over the fill.

Under a transform that only scales evenly (`s > 0`) and translates (`Homography::as_scale_translate`),
a `DrawCmd::Text` is drawn like a browser does: glyphs are rasterized at `font_size * s * scale`, their
positions mapped (`g.x * s + tx`, `g.y * s + ty`), the stroke width scaled by `s`, and the instances
placed in window space. They are marked `xf[0] = -1`, which `tag()` turns into transform 0 instead of
the current one. `background-clip: text` fill rects are mapped with `world()` too. Rasters are cached
by size in 0.25 px steps (the atlas key), so a short scale animation rasterizes only a few sizes.
(`M3-XF-03`, golden `M3-XF-02`.)

**Text decorations** (`TextCmd.decorations`): `solid` is one quad, `double` two (the second
`2 x thickness` below), `dotted`/`dashed` a `KIND_BORDER` instance with a one-sided styled-border
gradient entry, and `wavy` a `KIND_WAVY` instance (a sine wave of the line's thickness, drawn by the
shader inside a band 3 x thickness tall).

### 2. Upload

`globals`, `instances`, `gradients` and `clips` buffers are written with `write_buffer`; each grows
to the next power of two when too small. Empty storage buffers get a dummy element (wgpu forbids
zero-sized bindings).

### 3. Record and submit

Steps are encoded in order into one command encoder, then a final **blit** moves the canvas
(texture 0, in `canvas_format(blending)`: `Rgba8Unorm` for `srgb`, `CANVAS_FORMAT` = `Rgba8UnormSrgb` for `linear`)
to the real target, whose format can differ (`blit_pipeline` is cached per format and `Blit` mode: `Copy`
(`fs_blit`), `Decode` (`fs_blit_decode`, sRGB values to a target that encodes on write) or `Encode`
(`fs_blit_encode`)). Pool textures are keyed by size and format. Blur and composite parameters use small per-pass uniform buffers. Textures are
returned to the pool afterwards. `RenderStats` (`draw_calls`, `passes`, `layers`, `instances`,
`clips`) lets tests assert batching behaviour (for example that plain boxes share a draw call and
no layer is created).

## Shaders

[quad.wgsl](../crates/platypus_render/src/ui/quad.wgsl):

- `vs_main`: six vertices per instance (two triangles), no vertex buffer data besides the instance;
  growth for AA and for outer shadows (`3 * sigma`); clipped to the clip bounds.
- `fs_main` -> `shade(kind)`:
  - **Box**: signed-distance rounded rectangle with per-corner elliptical radii (`sd_rounded_box`)
    for the outer edge and for the inner (padding) edge (`inner_rx`/`inner_ry`, reduced by border
    widths); gradient composited over the fill; border
    is `outer - inner` coverage. Backgrounds extend under the border (`background-clip: border-box`).
  - **Styled border** (`KIND_BORDER`, `styled_border`): picks the side by nearest edge and width,
    then draws solid, dashed, dotted, double, groove, ridge, inset or outset with dashes and dots
    fitted per side. Side data lives in the gradient table entry.
  - **Shadows**: closed-form Gaussian blurred rounded box (`erf`-based, `blurred_box`), no offscreen
    pass. Shadows use circular mean radii (`circular()`). Outer shadows are masked out of the casting box; inset shadows are drawn inside the padding
    box minus the blurred hole.
  - **Glyph**: coverage from the R8 atlas page, or the RGBA page for colour emoji.
  - **Gradients**: `gradient_color` for linear (angle/corner), radial and conic (kind 2: angle from up,
    clockwise, centre in `info2.yz`), evaluated per pixel from `GpuGradient`; `info2.w` wraps stops
    for `repeating-*`. `KIND_GRADIENT_PIECE` draws a stretched gradient border-image piece.
- `fs_image`: separate entry point/pipeline; samples one texture tile with repeat flags, clipped to
  the box shape. For `border-image` it draws pieces as `KIND_IMAGE` with a texture sub-rect in
  `color0` (`images::border_image_axis` tiles an edge); piece edges are snapped to pixels.

[effects.wgsl](../crates/platypus_render/src/ui/effects.wgsl): `vs_fullscreen` (one oversized triangle),
`fs_blur` (1D Gaussian with `radius = ceil(3 sigma)`, max `MAX_BLUR_RADIUS` = 128; texel reads clamp to a bounds uniform), `fs_composite`
(`filtered_source`, opacity, `mask_at` and clip coverage), `fs_composite_blend` (the same against a backdrop copy), `fs_blit` / `fs_blit_decode` / `fs_blit_encode`. Both files read the blending from `Globals.srgb` (`srgb_canvas()`, `sampled()`, `canvas_to_srgb` / `canvas_from_srgb` skip or apply the conversions; the composite's `CompositeParams.mask.y` carries it); the GLES ports in `shaders.rs` and `effects.rs` do the same through `u_globals.w`. The same `Clip` struct and coverage function are duplicated
in both shader files; keep them in sync with `GpuClip` in mod.rs.

## Glyph atlas and images

- [glyphs.rs](../crates/platypus_render/src/ui/glyphs.rs) (GPU-free) and [atlas.rs](../crates/platypus_render/src/ui/atlas.rs) (wgpu): `GlyphCache` places glyphs and queues their pixels as `GlyphUpload`s, `GlyphAtlas` writes them into two page textures before the frame's draws. Two pages (2048 R8 coverage, 1024 RGBA colour),
  `etagere` bucketed allocation, 1 px padding. Keys include font blob id, glyph id, size x 4,
  subpixel bin, variable-font coordinates, synthetic bold/skew, stroke width (x4; 0 for the fill). Strokes use swash's `Stroke`. Rasterized with `swash`. When a page
  fills, the whole atlas is cleared at the start of the next frame and visible glyphs re-rasterize.
- [images.rs](../crates/platypus_render/src/ui/images.rs): `ImageCache` keyed by URL; failures are
  cached too so they are reported once. `url()` values are `data:` URLs (decoded with
  `decode_data_url`) or paths resolved by `platypus_core::resolve_asset` against `set_asset_root` (the
  app sets it to the game folder). `UiRenderer::image_size(gpu, url)` loads through this cache so the app can report natural sizes for `image` elements. `tile_rect` implements `background-size`
  (`auto`/`cover`/`contain`/lengths) and `background-position`. `ImageSamplers` holds two repeating
  samplers, `smooth` (linear) and `nearest` (`image-rendering: pixelated`); each `LoadedImage` has a
  bind group for each (`bind_group`, `bind_group_nearest`), and `Step::Draw` picks one by its flag.

**SVG.** `.svg` URLs and `data:image/svg+xml` URLs (inline `<svg>` elements) are rasterized by resvg at the tile's physical size (`ImageCache::svg_at`, sizes bucketed to 8 px) and cached; `data:` SVGs are evicted after 120 unused frames (`begin_frame`). See [svg.md](svg.md).

**Gradient tiling.** A gradient fill is laid out like an image: `tile_rect` (natural size = the box)
gives one tile from `background-size`/`position`, `repeat_tiles` its repeat axes (`round` rescales;
`space` gaps are not kept), and `gradient::build` is built for that tile. The instance carries the
tile in `p1` and the repeat axes in `params.y` (bit 1 = x, bit 2 = y); `shade` in quad.wgsl wraps the
pixel into the tile on repeating axes, skips the gradient outside the tile on others, and evaluates
`gradient_color` against the tile instead of the box.

## Offscreen and screenshot path

`OffscreenTarget::with_format(gpu, w, h, format)` creates a `RENDER_ATTACHMENT | COPY_SRC` texture;
`read_pixels` copies to a buffer with 256-byte row padding, maps it, and returns tightly packed RGBA8
(top row first). The app's `--screenshot` path ([main.rs](../crates/platypus_app/src/main.rs)
`save_screenshot`) renders the draw list again into an `Rgba8UnormSrgb` offscreen target (the blit decodes the
`srgb` canvas into it, so the PNG holds the same sRGB values the window shows) and writes
a PNG with `platypus_render::testing::write_png`. It still opens a window (frames run until the target
count), unlike tests, which are fully headless.

## Image tint

`ImageFill.tint` and `DrawCmd::BorderImage.tint` (`Option<Rgba>`) are passed in the image
instance's `color1` as linear premultiplied RGBA; all zeros means no tint. `image_tint` in
quad.wgsl returns white for zeros, and `fs_image` multiplies the sampled premultiplied pixel by it
on both the tiled and the repeating path. Golden M3-GAME-01.

## Golden testing

See [testing.md](testing.md). In short: `assert_golden(id, w, h, &pixels, tolerance)` in
[testing.rs](../crates/platypus_render/src/testing.rs) compares against `tests/golden/<id>.png`;
`PLATYPUS_UPDATE_GOLDEN=1` rewrites references. `shared_gpu()` returns one `Gpu` per test binary behind a
mutex because creating devices from parallel threads can deadlock some Vulkan layers.
