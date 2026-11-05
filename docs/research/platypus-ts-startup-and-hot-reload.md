# platypus_ts: start-up and hot reload (research notes)

Status: discussion notes, 2026-10-23. Nothing here is built; the measurement is queued. Related: `platypus-ts-type-checking.md`, `docs/benchmarks.md`, `docs/platypus-build.md` ("`game.pbundle`").

## 1. The problem

emoji starts in 2.2 s native against 1.2 s on QuickJS (about 4,700 lines of its own source plus the preludes it uses). The large TypeScript `game.pbundle` adds about a second at every launch. Script run time is not the issue (native script time is 5 to 25x below QuickJS, `docs/benchmarks.md`).

## 2. What the build produces today

`dist/game.pbundle` is JSON holding the bundled TypeScript source text (`source`). `platypus-build build` resolves modules, strips imports, runs `tsc` and writes that text; it does **not** compile to machine code. At launch `platypus_app` compiles the text in process with the Cranelift JIT, so every launch pays parse, inference/lowering (including the preludes the game references, loaded by the `wants_*` checks), Cranelift code generation and JIT linking. An `--aot` path (`dist/game.o` plus `game_bindings.rs`) exists but needs a native link step.

The hot-reload path recompiles the source in process when it differs (the old code is kept on error), which is why the compiler runs at launch.

## 3. The idea (user)

Compile ahead of time and keep a checksum: if the source is unchanged, load the stored result; recompile at run time only when it differs (hot reload, or a new host build). The dev loop is unaffected.

## 4. Cache levels (rising effort)

1. **Cache the intermediate form** (parsed/typed/lowered IR). Skips parsing, inference and lowering; still runs Cranelift at launch. Simple and safe.
2. **Cache relocatable object code.** Cranelift can emit object files (`--aot` already writes `dist/game.o`). A small loader maps the object and patches host function addresses (they change per host build, so raw JIT output cannot be stored). Skips the whole compiler; needs a loader per platform object format.
3. **Link the object into the host at package time.** Fully AOT; needs a linker step in `platypus-build package`. The right answer for shipped games, and for iOS, which forbids writable-executable memory (the doc already outlines it).

**Cache key:** source hash + compiler version + host engine-interface version. On mismatch compile from the source text, as now; on a cache error, fall back the same way.

## 5. Measurements (2026-10-23, release `--bench`, median of 5 runs)

Native start-up, process start to first frame (ms):

| Phase | emoji (4701 lines) | fog-world | godrays-room | hello |
|---|---:|---:|---:|---:|
| **Total** | 877 | 607 | 468 | 386 |
| Headless GPU device | 342 | 335 | 331 | 330 |
| Renderer set-up | 32 | 32 | 31 | 31 |
| Cranelift code generation | 174 | 32 | 33 | 3 |
| Type inference + lowering to IR | 36 | 5 | 6.5 | 0.5 |
| Source rewrites (JSX, async, components, preludes; own oxc parses) | 31 | 3 | 4 | 0.2 |
| Final oxc parse | 2 | 0.4 | 0.5 | <0.1 |
| Throw analysis + IR passes (sroa) | 3.3 | 0.4 | 1 | <0.1 |
| Engine manifests (build 6 + read 6) | 12 | 12 | 12 | 12 |
| JIT linking / finalising | 0.8 | 0.2 | 0.2 | <0.1 |
| Asset index (`.meta` files) | 93 | 1.5 | 1.5 | 1.4 |
| Log / asset mount | 17 | 1.2 | 1.1 | 1.1 |
| Bundle read | 0.5 | 0.2 | 0.1 | 0.1 |
| Program top level + starting components | 2.5 | 0.9 | 0.5 | <0.1 |
| First frame | 121 | 178 | 41 | 1 |

About 440 prelude lines were added (73 for hello).

**What a cache would remove:**
- (a) Cached lowered IR (after sroa): removes the manifest read, source rewrites, parse, lowering, throw analysis and sroa: about 75 to 80 ms on emoji, 15 to 18 ms on fog-world and godrays-room, 7 ms on hello. Code generation stays.
- (b) Cached relocatable object code: removes all of (a) plus code generation: about 250 ms on emoji, 48 ms on fog-world, 51 ms on godrays-room, 10 ms on hello. Loading and relocating the object stays (a few ms at most).
- Neither touches the GPU device (about 330 ms, paid by both backends), renderer set-up, the asset index or the first frame.

**Reading the numbers**
- The 2.2 s emoji start-up from the first benchmark did not reproduce: it is now 0.9 to 1.0 s native against 1.2 to 1.5 s on QuickJS. Compile cost is at most about 250 ms of that (emoji), 50 ms or less for the others.
- The biggest single item is the GPU device (about 330 ms), then the first frame, and for emoji the asset index (93 ms: it reads the `.meta` files). `.meta` files are now git-ignored and generated, so the index could be built lazily or cached.
- A compile cache is worth considering mainly for large games (emoji: about 250 ms of 877) and for packaged games (level 3). It is not the main start-up cost for small ones.

## 6. QuickJS script time, like for like

From `opMarshalMs` and `opsApplyMs` in `docs/benchmarks/results.json`: op marshalling (copying `__platypus_op` arguments out of JavaScript, inside script time) is at most 0.31 ms (raindrop-demo), godrays-room 0.17, fishtank 0.12, the rest 0.1 or less. Applying ops (counted under "other") is at most 0.27 ms (godrays-clouds). So QuickJS script time is nearly all JavaScript plus direct `#[ts_api]` native calls. Separating those or the in-JS animation, spline and camera-follow components (which native counts under "sync") would need a timer in every binding, which is not cheap and was not done.

## 7. Render sync (resolved separately)

The slowdown found by the benchmark was `sync_native_render` rebuilding the renderer's view of the scene every frame, not the language. It is now incremental (registry versions; commits `31be6440`, `97756c28`, `a8dd52dc`): fog-world went from 0.86x to 2.36x QuickJS speed and no clock face is slower native. `PLATYPUS_RENDER_SYNC=full` keeps the old full rebuild for comparison.

## 8. Open items

- Decision on a compile cache: measured gain is small for small games, about 250 ms for emoji (section 5); level 3 for packaged games.
- Cheaper start-up wins found: lazy or cached asset index (93 ms on emoji), engine manifest build and read (12 ms for every game), the 330 ms headless GPU device in `--bench` (windowed start-up not measured).
- Hot-reload behaviour with a stale cache (key mismatch, partial write: write atomically as `game.pbundle` already is).
- Whether preludes should be chosen by reachability rather than by source text mentions.
