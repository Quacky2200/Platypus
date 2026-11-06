# Rendering

The UI side of rendering (the frame plan, the wgpu, GLES and CPU executors, blending, glyphs, images, goldens) lives in
web-ui: web-ui `docs/rendering.md`. This page keeps what belongs to the engine host: how `platypus-app` chooses a
renderer per run, and its memory numbers. The 3D pass is `crates/platypus_render` (`world/`, see
[world-rendering.md](world-rendering.md)); it re-exports `web_ui_render`, so `platypus_render::UiRenderer` still works.

## Choosing the renderer

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

**The tiers** (`crates/platypus_core/src/renderer.rs`, `platypus_core::renderer`) are one table keyed on the API's class and namespace names:
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
## Memory

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