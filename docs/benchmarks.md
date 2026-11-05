# Benchmarks

> The QuickJS columns below are from before the QuickJS removal (2026-10-28); `platypus-build bench` now times only the native backend.

Frame times of every example on the native backend (platypus_ts, JIT-compiled `game.pbundle`) against the QuickJS original
of the same example on `main` (its own `bun run build`), measured before this branch is merged back. Both run in the
same release host, without a window (`platypus-app --bench`: an offscreen target, no vsync, no frame cap; each frame waits for
the GPU work of the frame before it), at 1280x720 and a fixed step of 1/60 s, with the same scripted input
(`tools/bench/inputs/<name>.json`). How it works and the flags: `docs/platypus-build.md`, "Benchmarks". Raw numbers:
`docs/benchmarks/results.json`.

Re-run (about 55 minutes for all 45, `crowd` alone about 7; `--only a,b` re-runs some and keeps the rest; `--timeout`
is per run, 600 s by default):

```bash
cd tools && cargo run -q -p platypus_build -- bench ../examples --timeout 900
```

The columns: fps over the measured frames, frame ms (mean, median, p95, p99, worst), and the mean frame split into
**script** (game code: hooks, timers, the React UI it renders), **sync** (native only: `sync_native_render`, the
per-frame copy of the compiled game's registries into the renderer, plus animation, splines and camera follow, which
QuickJS games do inside script time), **render** (UI layout and draw list, the world, the GPU wait) and **other**
(physics, audio, applying script output). Start-up is process start to the first frame (it includes the JIT
compile for native games); peak MB is the peak working set.

## Findings

Final run of 2026-10-23 at `01c1daf1` (all 45 examples in one batch, `--timeout 900`, nothing else building or running),
against `main` (`5b8aa521`). Machine: AMD Ryzen 9 5900X (12 cores, 32 GB), NVIDIA GeForce RTX 5080 (Vulkan), Windows 11
Pro 22631. crowd, fog-white-room, launcher, md-reader and windows exist only on this branch (native only, QuickJS n/a).
No build or run failed and no run logged errors.

- **Best speed-ups:** raindrop-demo 7.22x, fishtank 5.37x, godrays-room 4.88x, world 4.45x, splines 3.38x, physics
  2.57x, fog-world 2.57x, animation 2.49x, clock 2.06x, fog-raider 2.00x. They are the script-heavy examples: script time
  is 20-75x lower native (raindrop-demo 2.92 to 0.07 ms, fishtank 2.90 to 0.09, godrays-room 4.42 to 0.06, world 1.78 to
  0.04, splines 0.90 to 0.04).
- **Nothing is slower than QuickJS beyond noise.** The 15 examples within 0.9-1.1x in the full run were re-run twice
  (`--only .. --no-host-build`); fps of the full run / re-run 1 / re-run 2:

  | Example | native fps | QuickJS fps | speed-up | spread |
  |---|---|---|---|---:|
  | boxes | 3200 / 3211 / 3118 | 3208 / 3214 / 2616 | 1.00 / 1.00 / 1.19 | 1.00-1.19x |
  | cloth | 924 / 813 / 832 | 885 / 862 / 872 | 1.04 / 0.94 / 0.95 | 0.94-1.04x |
  | cornell | 2223 / 2178 / 2160 | 2133 / 1920 / 2140 | 1.04 / 1.13 / 1.01 | 1.01-1.13x |
  | css | 2637 / 2478 / 2599 | 2402 / 2008 / 2374 | 1.10 / 1.23 / 1.09 | 1.09-1.23x |
  | dark-room | 356 / 357 / 357 | 349 / 348 / 350 | 1.02 / 1.03 / 1.02 | 1.02-1.03x |
  | fog | 1401 / 1394 / 1356 | 1383 / 1377 / 1260 | 1.01 / 1.01 / 1.08 | 1.01-1.08x |
  | fog-blocks | 2640 / 2474 / 2662 | 2640 / 2389 / 2287 | 1.00 / 1.04 / 1.16 | 1.00-1.16x |
  | fog-teapots | 2093 / 2507 / 2286 | 2290 / 1992 / 1680 | 0.91 / 1.26 / 1.36 | 0.91-1.36x |
  | form | 3401 / 3389 / 3192 | 3509 / 3149 / 2687 | 0.97 / 1.08 / 1.19 | 0.97-1.19x |
  | godrays-lift | 1501 / 1493 / 1477 | 1490 / 1443 / 1422 | 1.01 / 1.03 / 1.04 | 1.01-1.04x |
  | godrays-shed | 226 / 225 / 225 | 221 / 221 / 225 | 1.02 / 1.02 / 1.00 | 1.00-1.02x |
  | hello | 8387 / 8543 / 8370 | 8586 / 8549 / 5840 | 0.98 / 1.00 / 1.43 | 0.98-1.43x |
  | materials | 605 / 600 / 568 | 567 / 541 / 514 | 1.07 / 1.11 / 1.10 | 1.07-1.11x |
  | text | 2304 / 2266 / 2278 | 2322 / 2281 / 2272 | 0.99 / 0.99 / 1.00 | 0.99-1.00x |
  | tunnel | 637 / 633 / 633 | 634 / 628 / 623 | 1.00 / 1.01 / 1.02 | 1.00-1.02x |

  Four read below 1.0x in the full run: fog-teapots 0.91x, form 0.97x, hello 0.98x, text 0.99x. Each crosses 1.0x in
  its re-runs (fog-teapots 0.91-1.36x, form 0.97-1.19x, hello 0.98-1.43x) or sits on it (text 0.99-1.00x). They are UI,
  2D or simple 3D scenes at 2,000-8,500 fps with 0.01 ms of script and sync, so the frame is the draw and the GPU; the
  spread comes from QuickJS's side moving (hello's QuickJS 8586 / 8549 / 5840 fps) as much as native's. Of the clock faces,
  Alarm clock read 0.88x and Morphing 1.15x (they read 1.08x and 0.82x in the previous batch, and 0.95-1.11x in two
  re-runs then): draw-bound faces with 0.15-0.20 ms of QuickJS script, noise. Every other face is 1.25-3.86x.
- **GPU-bound scenes are equal** (godrays-shed, godrays-lift, tunnel, dark-room, text: 0.99-1.04x in every run). Each
  frame waits for the previous frame's GPU work and the **render** column holds that wait, so QuickJS's script time
  overlaps the GPU (godrays-shed: 2.2 ms of QuickJS script, the same fps).
- **Sync** (`sync_native_render`) is 0.09 ms or less in every example but crowd (5.7 ms: animation of 500-2,000 skinned
  figures). fog-white-room's 0.58 ms of Animator work in stage 2 is 0.05 ms now (861 to 1672 fps).
- **QuickJS moves between batches.** QuickJS did not change between the stages, yet its fps moved by a median of +2%
  from stage 2 to this run, from 0.81x (shapes) to 1.40x (hello); in the re-runs above it moved up to 30% on the
  fast scenes. Native fps changes of 5-10%, and the attribution of a change to one optimisation below, are approximate.
- **Start-up** is 0.42-0.67 s native and 0.43-0.85 s QuickJS for all but two: emoji 1.76 s native (QuickJS 1.11 s;
  cold file reads right after its native build, as in the previous batch, and its compile share is about 250 ms) and
  md-reader 1.26 s. Breakdown and a compile cache: `docs/research/platypus-ts-startup-and-hot-reload.md`.
- emoji is random (each run picks other performers), so its numbers move between runs.
- css's original needs the Charm font, which is kept out of git; the runner copies it in from the checkout's `Charm/`
  folder.
- **Like-for-like script time:** a QuickJS game's **script** column holds the JavaScript, the native API calls it makes
  (math, scene, rendering data: QuickJS games call the same `#[ts_api]` functions) and its animation, spline and camera
  follow components; native counts those components under **sync**. `opMarshalMs` and `opsApplyMs` (JSON fields of that time, now removed) stayed at 0.3 ms or less.

## Stages

Native fps and speed-up over QuickJS at three points: **1** the first full run at `41fb369` (43 examples;
`git show 41fb369:docs/benchmarks/results.json`), **2** after the incremental render sync (`31be6440`, run committed in
`193c5a16`), **3** this final run at `01c1daf1`. Between 2 and 3 came the Animator work (bound tracks `b722f24e`,
`mark_moved` `6fe4775d`, buffer reuse `3abe1f58`), animation culling (`b168dc3c`, `4d374463` and the commits after it),
skinned-mesh frustum culling (`7d0cd929`) and the crowd costs (bone cache and packed uploads `a3dee1e0`, parallel
animators `4a9a8ca7`). Each stage's speed-up uses that stage's QuickJS run. "What changed" names the change when the
numbers point at it (the **sync** column, or the animation timers); a blank means the change is within the noise between
batches. Of the examples, only animation, crowd, fog-white-room and md-reader use `Animator` / `Animation`; the
crowd-cost and skinned-culling commits matter only to skinned meshes.

| Example | native fps 1 | speed-up 1 | native fps 2 | speed-up 2 | native fps 3 | QuickJS fps 3 | speed-up 3 | What changed |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| animation | 1841 | 2.02x | 2300 | 2.41x | 2617 | 1051 | 2.49x | 1-2: sync; 2-3: animator (bound tracks, buffer reuse) |
| arcade | 2037 | 1.64x | 2203 | 1.65x | 2502 | 1472 | 1.70x |  |
| boxes | 2394 | 1.13x | 3256 | 1.07x | 3200 | 3208 | 1.00x | 1-2: noise (QuickJS +43%) |
| classics | 2401 | 1.33x | 2881 | 1.47x | 2855 | 2095 | 1.36x |  |
| clock | 725 | 1.08x | 1437 | 2.03x | 1586 | 770 | 2.06x | 1-2: sync (0.62 to 0.06 ms) |
| cloth | 800 | 0.99x | 914 | 1.09x | 924 | 885 | 1.04x |  |
| cornell | 1871 | 1.01x | 2225 | 1.11x | 2223 | 2133 | 1.04x |  |
| crowd | n/a | - | n/a | - | 49 | n/a | - | new after stage 2; built with skinned culling, bone cache and packed uploads, parallel animators |
| css | 2386 | 1.08x | 2635 | 1.12x | 2637 | 2402 | 1.10x |  |
| dark-room | 338 | 1.04x | 355 | 1.03x | 356 | 349 | 1.02x | 1-2: sync (0.23 to 0.04 ms); GPU-bound |
| emoji | 477 | 1.68x | 819 | 2.27x | 704 | 381 | 1.85x | 1-2: sync (0.50 to 0.03 ms); random show |
| fishtank | 886 | 3.26x | 1381 | 5.12x | 1474 | 275 | 5.37x | 1-2: sync (0.29 to 0.04 ms) |
| fog | 1295 | 1.08x | 1405 | 1.10x | 1401 | 1383 | 1.01x | 1-2: sync (0.14 to 0.01 ms) |
| fog-blocks | 2267 | 0.93x | 2915 | 1.30x | 2640 | 2640 | 1.00x |  |
| fog-park | 223 | 1.07x | 376 | 1.61x | 409 | 233 | 1.76x | 1-2: sync (1.95 to 0.02 ms) |
| fog-raider | 1670 | 1.40x | 2176 | 1.61x | 2711 | 1355 | 2.00x | 1-2: sync (0.21 to 0.01 ms); 2-3: render 0.40 to 0.34 ms, not attributed |
| fog-teapots | 2157 | 1.02x | 2522 | 1.12x | 2093 | 2290 | 0.91x | noise (re-runs 0.91-1.36x) |
| fog-white-room | n/a | - | 861 | - | 1672 | n/a | - | 2-3: animator (bound tracks, mark_moved, buffer reuse, culling): sync 0.58 to 0.05 ms |
| fog-world | 323 | 0.86x | 978 | 2.43x | 1081 | 421 | 2.57x | 1-2: sync (1.83 to 0.03 ms) |
| form | 3293 | 0.97x | 3400 | 0.97x | 3401 | 3509 | 0.97x |  |
| godrays-cell | 386 | 1.23x | 398 | 1.20x | 398 | 337 | 1.18x | 1-2: sync (0.59 to 0.03 ms) |
| godrays-clouds | 206 | 1.16x | 324 | 1.79x | 371 | 190 | 1.96x | 1-2: sync (1.87 to 0.02 ms) |
| godrays-forest | 276 | 1.09x | 416 | 1.89x | 417 | 239 | 1.75x | 1-2: sync (1.44 to 0.02 ms) |
| godrays-lift | 1435 | 1.03x | 1499 | 1.01x | 1501 | 1490 | 1.01x |  |
| godrays-room | 475 | 2.73x | 810 | 4.58x | 875 | 179 | 4.88x | 1-2: sync (0.80 to 0.04 ms) |
| godrays-shed | 222 | 1.00x | 226 | 1.00x | 226 | 221 | 1.02x | 1-2: sync (0.33 to 0.06 ms); GPU-bound |
| godrays-sunset | 802 | 1.12x | 987 | 1.28x | 957 | 778 | 1.23x | 1-2: sync (0.25 to 0.02 ms) |
| hello | 8156 | 1.20x | 6223 | 1.02x | 8387 | 8586 | 0.98x | noise (QuickJS re-runs 5840-8590 fps) |
| launcher | 655 | - | 655 | - | 657 | n/a | - |  |
| layers | 1392 | 1.11x | 1759 | 1.41x | 2006 | 1271 | 1.58x | 1-2: sync (0.21 to 0.01 ms); 2-3: render 0.54 to 0.47 ms, not attributed |
| light-units | 1428 | 1.08x | 1566 | 1.12x | 1543 | 1331 | 1.16x |  |
| materials | 485 | 1.00x | 615 | 1.08x | 605 | 567 | 1.07x |  |
| md-reader | 1350 | - | 1506 | - | 1554 | n/a | - |  |
| physics | 1889 | 2.51x | 2029 | 2.68x | 1961 | 762 | 2.57x |  |
| platformer | 2095 | 1.46x | 3043 | 1.94x | 3055 | 1574 | 1.94x |  |
| quality | 1972 | 1.14x | 2067 | 1.20x | 2162 | 1796 | 1.20x |  |
| raindrop-demo | 1696 | 6.19x | 1967 | 6.87x | 2125 | 294 | 7.22x |  |
| scenes | 2717 | 1.25x | 2579 | 1.46x | 2814 | 2302 | 1.22x | noise (QuickJS -19% in stage 2, +30% in stage 3) |
| shapes | 2850 | 1.01x | 2900 | 0.99x | 3448 | 2383 | 1.45x | 2-3: not attributed (QuickJS fell 19% with no change: noise) |
| sine-circle-demo | 3045 | 1.52x | 3169 | 1.33x | 3172 | 2418 | 1.31x |  |
| splines | 2360 | 2.91x | 2745 | 3.35x | 2761 | 818 | 3.38x |  |
| text | 2250 | 1.01x | 2303 | 1.00x | 2304 | 2322 | 0.99x |  |
| tunnel | 613 | 1.01x | 637 | 1.03x | 637 | 634 | 1.00x | 1-2: sync (0.27 to 0.01 ms); GPU-bound |
| windows | 1367 | - | 1906 | - | 1859 | n/a | - |  |
| world | 1093 | 4.07x | 1112 | 3.94x | 1253 | 281 | 4.45x |  |

<!-- bench:begin (written by `platypus-build bench`) -->
Machine: AMD Ryzen 9 5900X 12-Core Processor (32 GB RAM), NVIDIA GeForce RTX 5080 (Vulkan, DiscreteGpu), Microsoft Windows 11 Pro 10.0.22631.

Commit 01c1daf1 against main (5b8aa521); every run 1280x720 px, a fixed step of 1/60 s per frame (`--fixed-dt`), no window, no vsync or frame cap, release host.

## Summary

| Example | Native fps | QuickJS fps | Speed-up | Native median / p99 ms | QuickJS median / p99 ms | Native script / sync ms | QuickJS script ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| [animation](#animation) | 2617 | 1051 | 2.49x | 0.36 / 0.49 | 0.89 / 1.33 | 0.01 / 0.01 | 0.56 |
| [arcade](#arcade) | 2502 | 1472 | 1.70x | 0.34 / 0.71 | 0.60 / 1.51 | 0.02 / 0.01 | 0.34 |
| [boxes](#boxes) | 3200 | 3208 | 1.00x | 0.30 / 0.45 | 0.29 / 0.72 | 0.01 / 0.01 | 0.02 |
| [classics](#classics) | 2855 | 2095 | 1.36x | 0.34 / 0.37 | 0.45 / 0.69 | 0.01 / 0.01 | 0.14 |
| [clock](#clock) | 1586 | 770 | 2.06x | 0.55 / 1.07 | 1.09 / 3.50 | 0.07 / 0.06 | 0.77 |
| [cloth](#cloth) | 924 | 885 | 1.04x | 1.09 / 1.90 | 1.14 / 1.89 | 0.01 / 0.01 | 0.06 |
| [cornell](#cornell) | 2223 | 2133 | 1.04x | 0.44 / 0.49 | 0.46 / 0.52 | 0.01 / 0.01 | 0.03 |
| [crowd](#crowd) | 49 | n/a | - | 9.82 / 44.49 | n/a | 2.64 / 5.68 | n/a |
| [css](#css) | 2637 | 2402 | 1.10x | 0.37 / 0.86 | 0.36 / 3.67 | 0.01 / 0.01 | 0.04 |
| [dark-room](#dark-room) | 356 | 349 | 1.02x | 2.74 / 3.53 | 2.75 / 4.16 | 0.10 / 0.04 | 0.67 |
| [emoji](#emoji) | 704 | 381 | 1.85x | 1.20 / 3.92 | 2.23 / 9.98 | 0.05 / 0.03 | 0.85 |
| [fishtank](#fishtank) | 1474 | 275 | 5.37x | 0.65 / 0.95 | 3.63 / 3.96 | 0.09 / 0.04 | 2.90 |
| [fog](#fog) | 1401 | 1383 | 1.01x | 0.71 / 0.99 | 0.71 / 0.93 | 0.01 / 0.01 | 0.25 |
| [fog-blocks](#fog-blocks) | 2640 | 2640 | 1.00x | 0.36 / 0.56 | 0.37 / 0.41 | 0.01 / 0.01 | 0.05 |
| [fog-park](#fog-park) | 409 | 233 | 1.76x | 2.34 / 3.16 | 4.29 / 4.96 | 0.01 / 0.02 | 2.27 |
| [fog-raider](#fog-raider) | 2711 | 1355 | 2.00x | 0.36 / 0.40 | 0.73 / 0.81 | 0.01 / 0.01 | 0.38 |
| [fog-teapots](#fog-teapots) | 2093 | 2290 | 0.91x | 0.53 / 0.72 | 0.43 / 0.47 | 0.01 / 0.01 | 0.05 |
| [fog-white-room](#fog-white-room) | 1672 | n/a | - | 0.59 / 0.71 | n/a | 0.04 / 0.05 | n/a |
| [fog-world](#fog-world) | 1081 | 421 | 2.57x | 0.83 / 4.51 | 1.25 / 15.23 | 0.22 / 0.03 | 1.61 |
| [form](#form) | 3401 | 3509 | 0.97x | 0.29 / 0.32 | 0.28 / 0.30 | 0.01 / 0.01 | 0.01 |
| [godrays-cell](#godrays-cell) | 398 | 337 | 1.18x | 2.50 / 3.00 | 2.96 / 3.38 | 0.06 / 0.02 | 1.99 |
| [godrays-clouds](#godrays-clouds) | 371 | 190 | 1.96x | 2.67 / 3.20 | 5.19 / 6.76 | 0.01 / 0.02 | 2.40 |
| [godrays-forest](#godrays-forest) | 417 | 239 | 1.75x | 2.36 / 3.13 | 4.03 / 6.69 | 0.01 / 0.02 | 1.89 |
| [godrays-lift](#godrays-lift) | 1501 | 1490 | 1.01x | 0.66 / 1.02 | 0.66 / 0.86 | 0.01 / 0.01 | 0.10 |
| [godrays-room](#godrays-room) | 875 | 179 | 4.88x | 1.11 / 1.43 | 5.64 / 6.49 | 0.06 / 0.04 | 4.42 |
| [godrays-shed](#godrays-shed) | 226 | 221 | 1.02x | 4.32 / 5.23 | 4.35 / 6.64 | 0.06 / 0.06 | 2.18 |
| [godrays-sunset](#godrays-sunset) | 957 | 778 | 1.23x | 0.93 / 2.37 | 1.17 / 2.22 | 0.02 / 0.02 | 0.38 |
| [hello](#hello) | 8387 | 8586 | 0.98x | 0.12 / 0.13 | 0.12 / 0.13 | 0.01 / 0.01 | 0.02 |
| [launcher](#launcher) | 657 | n/a | - | 2.01 / 3.18 | n/a | 0.01 / 0.01 | n/a |
| [layers](#layers) | 2006 | 1271 | 1.58x | 0.49 / 0.53 | 0.78 / 0.83 | 0.01 / 0.01 | 0.30 |
| [light-units](#light-units) | 1543 | 1331 | 1.16x | 0.64 / 0.82 | 0.72 / 1.29 | 0.01 / 0.01 | 0.10 |
| [materials](#materials) | 605 | 567 | 1.07x | 1.62 / 3.12 | 1.73 / 3.25 | 0.01 / 0.01 | 0.12 |
| [md-reader](#md-reader) | 1554 | n/a | - | 0.56 / 1.26 | n/a | 0.01 / 0.01 | n/a |
| [physics](#physics) | 1961 | 762 | 2.57x | 0.42 / 1.02 | 1.24 / 2.42 | 0.25 / 0.01 | 0.85 |
| [platformer](#platformer) | 3055 | 1574 | 1.94x | 0.32 / 0.40 | 0.63 / 0.80 | 0.04 / 0.02 | 0.30 |
| [quality](#quality) | 2162 | 1796 | 1.20x | 0.47 / 0.70 | 0.52 / 0.72 | 0.02 / 0.01 | 0.14 |
| [raindrop-demo](#raindrop-demo) | 2125 | 294 | 7.22x | 0.45 / 0.54 | 3.35 / 4.12 | 0.07 / 0.09 | 2.92 |
| [scenes](#scenes) | 2814 | 2302 | 1.22x | 0.35 / 0.39 | 0.43 / 0.48 | 0.01 / 0.01 | 0.10 |
| [shapes](#shapes) | 3448 | 2383 | 1.45x | 0.29 / 0.31 | 0.42 / 0.49 | 0.01 / 0.01 | 0.08 |
| [sine-circle-demo](#sine-circle-demo) | 3172 | 2418 | 1.31x | 0.29 / 0.41 | 0.33 / 0.63 | 0.02 / 0.01 | 0.11 |
| [splines](#splines) | 2761 | 818 | 3.38x | 0.33 / 0.57 | 1.19 / 1.44 | 0.04 / 0.04 | 0.90 |
| [text](#text) | 2304 | 2322 | 0.99x | 0.43 / 0.58 | 0.42 / 0.78 | 0.01 / 0.01 | 0.01 |
| [tunnel](#tunnel) | 637 | 634 | 1.00x | 1.65 / 2.14 | 1.65 / 2.11 | 0.01 / 0.01 | 0.35 |
| [windows](#windows) | 1859 | n/a | - | 0.52 / 0.67 | n/a | 0.01 / 0.01 | n/a |
| [world](#world) | 1253 | 281 | 4.45x | 0.79 / 0.87 | 4.17 / 4.74 | 0.04 / 0.01 | 1.78 |

## Per example

### animation

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2617 | 0.38 | 0.36 | 0.47 | 0.49 | 1.93 | 0.01 | 0.01 | 0.35 | 0.01 | 516 ms | 165 |
| QuickJS | 1051 | 0.95 | 0.89 | 1.19 | 1.33 | 2.69 | 0.56 | 0.00 | 0.38 | 0.01 | 524 ms | 165 |

### arcade

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2502 | 0.40 | 0.34 | 0.52 | 0.71 | 1.97 | 0.02 | 0.01 | 0.36 | 0.01 | 493 ms | 165 |
| QuickJS | 1472 | 0.68 | 0.60 | 1.22 | 1.51 | 2.74 | 0.34 | 0.00 | 0.33 | 0.01 | 504 ms | 166 |

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| idle | 300 | 1960 | 1281 | 1.53x | 0.48 / 0.77 | 0.65 / 1.89 | 0.03 | 0.43 |
| steer | 720 | 2829 | 1570 | 1.80x | 0.34 / 0.51 | 0.60 / 1.30 | 0.02 | 0.30 |

### boxes

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 3200 | 0.31 | 0.30 | 0.37 | 0.45 | 0.53 | 0.01 | 0.01 | 0.29 | 0.01 | 434 ms | 163 |
| QuickJS | 3208 | 0.31 | 0.29 | 0.34 | 0.72 | 0.79 | 0.02 | 0.00 | 0.28 | 0.01 | 472 ms | 165 |

### classics

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2855 | 0.35 | 0.34 | 0.36 | 0.37 | 1.98 | 0.01 | 0.01 | 0.32 | 0.01 | 463 ms | 163 |
| QuickJS | 2095 | 0.48 | 0.45 | 0.56 | 0.69 | 2.03 | 0.14 | 0.00 | 0.33 | 0.01 | 543 ms | 165 |

### clock

Input: the right arrow clicked through all twelve faces.

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1586 | 0.63 | 0.55 | 1.03 | 1.07 | 2.58 | 0.07 | 0.06 | 0.45 | 0.05 | 674 ms | 232 |
| QuickJS | 770 | 1.30 | 1.09 | 3.46 | 3.50 | 5.00 | 0.77 | 0.00 | 0.50 | 0.04 | 646 ms | 228 |

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Analog | 240 | 1962 | 725 | 2.71x | 0.50 / 0.62 | 1.37 / 1.50 | 0.04 | 0.95 |
| Dials | 240 | 1878 | 1179 | 1.59x | 0.52 / 0.67 | 0.83 / 1.25 | 0.01 | 0.33 |
| Meter | 240 | 1428 | 1014 | 1.41x | 0.69 / 0.83 | 0.99 / 1.04 | 0.02 | 0.32 |
| Sundial | 240 | 1855 | 1484 | 1.25x | 0.53 / 0.66 | 0.67 / 0.72 | 0.03 | 0.19 |
| Sand timer | 240 | 2086 | 1410 | 1.48x | 0.47 / 0.60 | 0.71 / 0.74 | 0.03 | 0.32 |
| Lava lamp | 240 | 956 | 290 | 3.30x | 1.03 / 1.18 | 3.47 / 3.58 | 0.42 | 2.79 |
| Underwater | 240 | 1852 | 882 | 2.10x | 0.53 / 0.65 | 1.13 / 1.18 | 0.02 | 0.60 |
| Chandelier | 240 | 1336 | 749 | 1.78x | 0.74 / 0.87 | 1.33 / 1.39 | 0.02 | 0.55 |
| Rainbow | 240 | 1789 | 702 | 2.55x | 0.55 / 0.68 | 1.42 / 1.47 | 0.05 | 0.92 |
| Morphing | 240 | 2225 | 1929 | 1.15x | 0.42 / 0.60 | 0.51 / 0.54 | 0.02 | 0.14 |
| Flip | 240 | 1623 | 420 | 3.86x | 0.58 / 0.82 | 2.32 / 2.95 | 0.14 | 1.93 |
| Alarm clock | 240 | 1180 | 1338 | 0.88x | 0.86 / 0.97 | 0.75 / 0.79 | 0.02 | 0.17 |

### cloth

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 924 | 1.08 | 1.09 | 1.81 | 1.90 | 2.60 | 0.01 | 0.01 | 0.35 | 0.71 | 456 ms | 163 |
| QuickJS | 885 | 1.13 | 1.14 | 1.86 | 1.89 | 2.67 | 0.06 | 0.00 | 0.36 | 0.71 | 505 ms | 165 |

### cornell

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2223 | 0.45 | 0.44 | 0.47 | 0.49 | 2.04 | 0.01 | 0.01 | 0.43 | 0.01 | 448 ms | 188 |
| QuickJS | 2133 | 0.47 | 0.46 | 0.49 | 0.52 | 2.09 | 0.03 | 0.00 | 0.44 | 0.01 | 512 ms | 188 |

### crowd

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 49 | 20.27 | 9.82 | 42.98 | 44.49 | 57.05 | 2.64 | 5.68 | 11.92 | 0.02 | 582 ms | 482 |
| QuickJS | n/a |||||||||||| 

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 250, view 1 (street, A), alwaysAnimate | 600 | 119 | - | - | 8.35 / 10.40 | - / - | 0.96 | - |
| 250, view 2 (over A), alwaysAnimate | 600 | 125 | - | - | 7.81 / 11.08 | - / - | 1.00 | - |
| 250, view 3 (both), alwaysAnimate | 600 | 111 | - | - | 8.81 / 10.95 | - / - | 1.07 | - |
| 250, view 4 (over B), alwaysAnimate | 600 | 117 | - | - | 8.40 / 10.48 | - / - | 0.97 | - |
| 250, view 4 (over B), cullUpdateTransforms | 600 | 130 | - | - | 7.61 / 8.72 | - / - | 0.84 | - |
| 250, view 4 (over B), cullCompletely | 600 | 127 | - | - | 7.69 / 9.98 | - / - | 0.91 | - |
| 250, view 3 (both), cullCompletely | 600 | 98 | - | - | 10.01 / 12.25 | - / - | 1.03 | - |
| 1000, view 4 (over B), cullCompletely | 600 | 29 | - | - | 33.90 / 37.02 | - / - | 4.97 | - |
| 1000, view 4 (over B), alwaysAnimate | 600 | 26 | - | - | 38.31 / 40.96 | - / - | 4.61 | - |
| 1000, view 4 (over B), cullUpdateTransforms | 600 | 29 | - | - | 34.05 / 40.08 | - / - | 4.75 | - |
| 1000, view 3 (both), alwaysAnimate | 600 | 23 | - | - | 43.09 / 50.13 | - / - | 5.28 | - |
| 1000, view 1 (street, A), cullCompletely | 600 | 30 | - | - | 32.59 / 41.54 | - / - | 5.28 | - |

### css

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2637 | 0.38 | 0.37 | 0.39 | 0.86 | 1.12 | 0.01 | 0.01 | 0.35 | 0.01 | 487 ms | 165 |
| QuickJS | 2402 | 0.42 | 0.36 | 0.38 | 3.67 | 3.76 | 0.04 | 0.00 | 0.36 | 0.01 | 491 ms | 166 |

### dark-room

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 356 | 2.81 | 2.74 | 3.37 | 3.53 | 6.47 | 0.10 | 0.04 | 2.67 | 0.01 | 500 ms | 213 |
| QuickJS | 349 | 2.86 | 2.75 | 3.94 | 4.16 | 9.18 | 0.67 | 0.00 | 2.18 | 0.02 | 529 ms | 211 |

### emoji

Input: the show is random: each run picks other performers.

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 704 | 1.42 | 1.20 | 3.84 | 3.92 | 31.96 | 0.05 | 0.03 | 1.26 | 0.08 | 1760 ms | 192 |
| QuickJS | 381 | 2.62 | 2.23 | 8.19 | 9.98 | 32.12 | 0.85 | 0.00 | 1.68 | 0.09 | 1110 ms | 200 |

### fishtank

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1474 | 0.68 | 0.65 | 0.86 | 0.95 | 2.30 | 0.09 | 0.04 | 0.54 | 0.01 | 486 ms | 165 |
| QuickJS | 275 | 3.64 | 3.63 | 3.82 | 3.96 | 5.29 | 2.90 | 0.00 | 0.69 | 0.05 | 520 ms | 167 |

### fog

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1401 | 0.71 | 0.71 | 0.73 | 0.99 | 2.17 | 0.01 | 0.01 | 0.69 | 0.01 | 473 ms | 189 |
| QuickJS | 1383 | 0.72 | 0.71 | 0.74 | 0.93 | 2.43 | 0.25 | 0.00 | 0.46 | 0.01 | 516 ms | 190 |

### fog-blocks

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2640 | 0.38 | 0.36 | 0.49 | 0.56 | 2.14 | 0.01 | 0.01 | 0.35 | 0.01 | 482 ms | 166 |
| QuickJS | 2640 | 0.38 | 0.37 | 0.39 | 0.41 | 1.96 | 0.05 | 0.00 | 0.32 | 0.01 | 570 ms | 166 |

### fog-park

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 409 | 2.44 | 2.34 | 3.02 | 3.16 | 5.69 | 0.01 | 0.02 | 2.40 | 0.01 | 536 ms | 211 |
| QuickJS | 233 | 4.30 | 4.29 | 4.47 | 4.96 | 8.69 | 2.27 | 0.00 | 2.01 | 0.01 | 712 ms | 214 |

### fog-raider

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2711 | 0.37 | 0.36 | 0.38 | 0.40 | 1.96 | 0.01 | 0.01 | 0.34 | 0.01 | 465 ms | 164 |
| QuickJS | 1355 | 0.74 | 0.73 | 0.75 | 0.81 | 2.34 | 0.38 | 0.00 | 0.35 | 0.01 | 512 ms | 166 |

### fog-teapots

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2093 | 0.48 | 0.53 | 0.58 | 0.72 | 2.08 | 0.01 | 0.01 | 0.45 | 0.01 | 534 ms | 165 |
| QuickJS | 2290 | 0.44 | 0.43 | 0.45 | 0.47 | 2.09 | 0.05 | 0.00 | 0.38 | 0.01 | 581 ms | 166 |

### fog-white-room

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1672 | 0.60 | 0.59 | 0.62 | 0.71 | 2.27 | 0.04 | 0.05 | 0.51 | 0.01 | 649 ms | 244 |
| QuickJS | n/a |||||||||||| 

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| idle | 300 | 1690 | - | - | 0.58 / 0.73 | - / - | 0.03 | - |
| jog, sprint, jump and roll | 720 | 1665 | - | - | 0.59 / 0.66 | - / - | 0.04 | - |

### fog-world

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1081 | 0.93 | 0.83 | 1.05 | 4.51 | 10.75 | 0.22 | 0.03 | 0.67 | 0.01 | 632 ms | 210 |
| QuickJS | 421 | 2.38 | 1.25 | 9.22 | 15.23 | 17.74 | 1.61 | 0.00 | 0.75 | 0.02 | 852 ms | 174 |

### form

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 3401 | 0.29 | 0.29 | 0.30 | 0.32 | 0.33 | 0.01 | 0.01 | 0.27 | 0.01 | 444 ms | 165 |
| QuickJS | 3509 | 0.28 | 0.28 | 0.29 | 0.30 | 0.35 | 0.01 | 0.00 | 0.26 | 0.01 | 484 ms | 166 |

### godrays-cell

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 398 | 2.51 | 2.50 | 2.55 | 3.00 | 6.19 | 0.06 | 0.02 | 2.42 | 0.01 | 493 ms | 204 |
| QuickJS | 337 | 2.96 | 2.96 | 3.07 | 3.38 | 6.80 | 1.99 | 0.00 | 0.95 | 0.03 | 548 ms | 204 |

### godrays-clouds

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 371 | 2.69 | 2.67 | 2.98 | 3.20 | 7.54 | 0.01 | 0.02 | 2.65 | 0.01 | 475 ms | 214 |
| QuickJS | 190 | 5.28 | 5.19 | 5.78 | 6.76 | 10.47 | 2.40 | 0.00 | 2.85 | 0.03 | 615 ms | 209 |

### godrays-forest

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 417 | 2.40 | 2.36 | 2.77 | 3.13 | 4.60 | 0.01 | 0.02 | 2.35 | 0.01 | 488 ms | 211 |
| QuickJS | 239 | 4.18 | 4.03 | 4.82 | 6.69 | 7.64 | 1.89 | 0.00 | 2.28 | 0.01 | 681 ms | 209 |

### godrays-lift

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1501 | 0.67 | 0.66 | 0.69 | 1.02 | 2.22 | 0.01 | 0.01 | 0.64 | 0.01 | 466 ms | 195 |
| QuickJS | 1490 | 0.67 | 0.66 | 0.70 | 0.86 | 2.27 | 0.10 | 0.00 | 0.56 | 0.01 | 507 ms | 194 |

### godrays-room

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 875 | 1.14 | 1.11 | 1.28 | 1.43 | 3.66 | 0.06 | 0.04 | 1.03 | 0.01 | 490 ms | 215 |
| QuickJS | 179 | 5.57 | 5.64 | 5.88 | 6.49 | 10.08 | 4.42 | 0.00 | 1.10 | 0.05 | 564 ms | 216 |

### godrays-shed

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 226 | 4.42 | 4.32 | 5.01 | 5.23 | 5.68 | 0.06 | 0.06 | 4.29 | 0.01 | 500 ms | 204 |
| QuickJS | 221 | 4.51 | 4.35 | 5.75 | 6.64 | 7.04 | 2.18 | 0.00 | 2.30 | 0.04 | 574 ms | 208 |

### godrays-sunset

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 957 | 1.04 | 0.93 | 1.84 | 2.37 | 4.17 | 0.02 | 0.02 | 0.99 | 0.01 | 507 ms | 230 |
| QuickJS | 778 | 1.29 | 1.17 | 2.02 | 2.22 | 5.30 | 0.38 | 0.00 | 0.90 | 0.01 | 884 ms | 239 |

### hello

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 8387 | 0.12 | 0.12 | 0.12 | 0.13 | 0.15 | 0.01 | 0.01 | 0.10 | 0.01 | 419 ms | 162 |
| QuickJS | 8586 | 0.12 | 0.12 | 0.12 | 0.13 | 0.32 | 0.02 | 0.00 | 0.10 | 0.01 | 432 ms | 163 |

### launcher

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 657 | 1.52 | 2.01 | 2.24 | 3.18 | 4.49 | 0.01 | 0.01 | 1.48 | 0.03 | 535 ms | 167 |
| QuickJS | n/a |||||||||||| 

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| idle | 300 | 1870 | - | - | 0.52 / 0.86 | - / - | 0.01 | - |
| carousel | 660 | 508 | - | - | 2.05 / 3.20 | - / - | 0.01 | - |

### layers

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2006 | 0.50 | 0.49 | 0.51 | 0.53 | 2.08 | 0.01 | 0.01 | 0.47 | 0.01 | 470 ms | 164 |
| QuickJS | 1271 | 0.79 | 0.78 | 0.81 | 0.83 | 2.38 | 0.30 | 0.00 | 0.48 | 0.01 | 518 ms | 166 |

### light-units

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1543 | 0.65 | 0.64 | 0.70 | 0.82 | 2.34 | 0.01 | 0.01 | 0.62 | 0.01 | 498 ms | 196 |
| QuickJS | 1331 | 0.75 | 0.72 | 1.09 | 1.29 | 2.45 | 0.10 | 0.00 | 0.64 | 0.01 | 514 ms | 196 |

### materials

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 605 | 1.65 | 1.62 | 1.71 | 3.12 | 3.47 | 0.01 | 0.01 | 1.62 | 0.01 | 537 ms | 164 |
| QuickJS | 567 | 1.76 | 1.73 | 1.83 | 3.25 | 3.38 | 0.12 | 0.00 | 1.64 | 0.01 | 579 ms | 165 |

### md-reader

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1554 | 0.64 | 0.56 | 1.01 | 1.26 | 5.20 | 0.01 | 0.01 | 0.59 | 0.04 | 1263 ms | 181 |
| QuickJS | n/a |||||||||||| 

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| idle | 300 | 1932 | - | - | 0.52 / 0.54 | - / - | 0.01 | - |
| scroll | 800 | 1448 | - | - | 0.60 / 1.28 | - / - | 0.01 | - |

### physics

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1961 | 0.51 | 0.42 | 0.89 | 1.02 | 2.29 | 0.25 | 0.01 | 0.15 | 0.10 | 429 ms | 164 |
| QuickJS | 762 | 1.31 | 1.24 | 2.25 | 2.42 | 3.66 | 0.85 | 0.00 | 0.16 | 0.30 | 474 ms | 165 |

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| idle | 300 | 2960 | 1332 | 2.22x | 0.34 / 0.54 | 0.77 / 0.97 | 0.16 | 0.48 |
| drop 40 | 600 | 1678 | 628 | 2.67x | 0.61 / 1.04 | 1.55 / 2.42 | 0.30 | 1.04 |

### platformer

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 3055 | 0.33 | 0.32 | 0.38 | 0.40 | 1.85 | 0.04 | 0.02 | 0.26 | 0.01 | 479 ms | 164 |
| QuickJS | 1574 | 0.64 | 0.63 | 0.75 | 0.80 | 2.21 | 0.30 | 0.00 | 0.27 | 0.07 | 741 ms | 165 |

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| idle | 300 | 3128 | 1609 | 1.94x | 0.32 / 0.40 | 0.60 / 0.83 | 0.04 | 0.30 |
| run and jump | 660 | 3023 | 1559 | 1.94x | 0.33 / 0.39 | 0.63 / 0.79 | 0.04 | 0.30 |

### quality

Input: each quality level clicked in the menu (this stops the demo's own cycling).

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2162 | 0.46 | 0.47 | 0.53 | 0.70 | 2.05 | 0.02 | 0.01 | 0.42 | 0.02 | 461 ms | 164 |
| QuickJS | 1796 | 0.56 | 0.52 | 0.65 | 0.72 | 2.10 | 0.14 | 0.00 | 0.40 | 0.01 | 523 ms | 165 |

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Low | 300 | 2473 | 1840 | 1.34x | 0.40 / 0.45 | 0.52 / 0.73 | 0.01 | 0.14 |
| Medium | 300 | 1963 | 1548 | 1.27x | 0.50 / 0.67 | 0.64 / 0.85 | 0.01 | 0.14 |
| High | 300 | 2226 | 1915 | 1.16x | 0.39 / 0.70 | 0.52 / 0.60 | 0.01 | 0.14 |
| Ultra | 300 | 2054 | 1940 | 1.06x | 0.47 / 0.75 | 0.51 / 0.54 | 0.04 | 0.14 |

### raindrop-demo

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2125 | 0.47 | 0.45 | 0.52 | 0.54 | 1.99 | 0.07 | 0.09 | 0.29 | 0.02 | 472 ms | 169 |
| QuickJS | 294 | 3.40 | 3.35 | 3.69 | 4.12 | 14.55 | 2.92 | 0.00 | 0.34 | 0.14 | 714 ms | 181 |

### scenes

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2814 | 0.35 | 0.35 | 0.37 | 0.39 | 1.90 | 0.01 | 0.01 | 0.33 | 0.01 | 483 ms | 165 |
| QuickJS | 2302 | 0.43 | 0.43 | 0.44 | 0.48 | 1.93 | 0.10 | 0.00 | 0.32 | 0.01 | 517 ms | 165 |

### shapes

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 3448 | 0.29 | 0.29 | 0.30 | 0.31 | 1.80 | 0.01 | 0.01 | 0.26 | 0.01 | 452 ms | 162 |
| QuickJS | 2383 | 0.42 | 0.42 | 0.44 | 0.49 | 1.92 | 0.08 | 0.00 | 0.33 | 0.01 | 462 ms | 162 |

### sine-circle-demo

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 3172 | 0.32 | 0.29 | 0.37 | 0.41 | 1.80 | 0.02 | 0.01 | 0.27 | 0.01 | 472 ms | 165 |
| QuickJS | 2418 | 0.41 | 0.33 | 0.61 | 0.63 | 2.27 | 0.11 | 0.00 | 0.29 | 0.01 | 506 ms | 166 |

### splines

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2761 | 0.36 | 0.33 | 0.54 | 0.57 | 1.84 | 0.04 | 0.04 | 0.27 | 0.01 | 459 ms | 164 |
| QuickJS | 818 | 1.22 | 1.19 | 1.41 | 1.44 | 2.69 | 0.90 | 0.00 | 0.29 | 0.04 | 466 ms | 163 |

### text

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 2304 | 0.43 | 0.43 | 0.45 | 0.58 | 0.61 | 0.01 | 0.01 | 0.41 | 0.01 | 473 ms | 163 |
| QuickJS | 2322 | 0.43 | 0.42 | 0.44 | 0.78 | 1.24 | 0.01 | 0.00 | 0.41 | 0.01 | 482 ms | 165 |

### tunnel

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 637 | 1.57 | 1.65 | 1.82 | 2.14 | 5.27 | 0.01 | 0.01 | 1.54 | 0.01 | 499 ms | 213 |
| QuickJS | 634 | 1.58 | 1.65 | 1.82 | 2.11 | 5.35 | 0.35 | 0.00 | 1.22 | 0.01 | 609 ms | 213 |

### windows

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1859 | 0.54 | 0.52 | 0.56 | 0.67 | 2.05 | 0.01 | 0.01 | 0.51 | 0.01 | 651 ms | 166 |
| QuickJS | n/a |||||||||||| 

### world

| Backend | fps | mean | median | p95 | p99 | worst | script | sync | render | other | start-up | peak MB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| native | 1253 | 0.80 | 0.79 | 0.83 | 0.87 | 2.47 | 0.04 | 0.01 | 0.72 | 0.03 | 524 ms | 193 |
| QuickJS | 281 | 3.56 | 4.17 | 4.51 | 4.74 | 6.19 | 1.78 | 0.00 | 0.90 | 0.88 | 517 ms | 197 |

| Segment | frames | native fps | QuickJS fps | speed-up | native median / p99 | QuickJS median / p99 | native script | QuickJS script |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| idle | 300 | 1262 | 290 | 4.34x | 0.79 / 0.85 | 4.09 / 4.39 | 0.04 | 1.77 |
| pointer sweep | 600 | 1248 | 277 | 4.51x | 0.79 / 0.88 | 4.24 / 4.83 | 0.04 | 1.78 |
<!-- bench:end -->

## Animator

Hand-written (kept by `platypus-build bench`). The native `Animator` made cheap (2026-10-23), one commit per step, and
Unity's animation culling. Measured with the release host (`cargo build --release -p platypus_app`), no window, on the
machine above (AMD Ryzen 9 5900X, NVIDIA GeForce RTX 5080 on Vulkan, Windows 11 Pro 22631):

```bash
build/release/platypus-app.exe examples/fog-white-room --bench --bench-input tools/bench/inputs/fog-white-room.json \
  --fixed-dt 0.0166667 --bench-out out.json
```

The numbers are `syncParts` means in ms a frame: "animation, splines, camera follow" (the whole update; splines and
camera follow are next to nothing here) and the animation timers added in the first step (`animation: pose add`,
`pose apply`, `set_local`, which is timed per bone, so part of its share is the timer). Median of 3 runs per step (5
for the last; 1 for "before"), segments "idle" (one clip) and "jog, sprint, jump and roll" (2 to 4 clips blended).

| Step | Commit | Idle: update | add | apply | set_local | Moving: update | add | apply | Frame mean idle / moving |
|---|---|---|---|---|---|---|---|---|---|
| Before (timers only) | `8b0d59aa` | 0.375 | 0.202 | 0.163 | 0.029 | 0.635 | 0.456 | 0.166 | 0.97 / 1.24 |
| Bind tracks once per (clip, root) | `b722f24e` | 0.058 | 0.016 | 0.033 | 0.029 | 0.073 | 0.027 | 0.033 | 0.64 / 0.67 |
| `mark_moved` stops at listed objects | `6fe4775d` | 0.033 | 0.017 | 0.008 | 0.003 | 0.049 | 0.028 | 0.007 | 0.61 / 0.63 |
| Reused pose buffer, shared key search, kept clip lengths | `3abe1f58` | 0.029 | 0.017 | 0.008 | 0.003 | 0.042 | 0.030 | 0.007 | 0.63 / 0.64 |
| With culling (default `alwaysAnimate`) | `b168dc3c` | 0.027 | 0.014 | 0.008 | 0.003 | 0.039 | 0.027 | 0.007 | 0.62 / 0.63 |

Overall 14x idle and 16x moving for the 65-bone mannequin (about 650 curves a clip). The first step is nearly all of it
(the string work per curve per frame); the rest are microseconds and near the noise between runs (add moved between
0.012 and 0.017 ms idle across runs of one build). Frames of `examples/animation` and fog-white-room rendered at fixed
frames before and after every step are identical byte for byte.

**A crowd and culling.** `tools/bench/crowd` (a bench scene, not an example: copy the mannequin into its `assets/`):
50 mannequins jogging, each with its own `Animator` (65 bones, one looping clip), the camera on them or turned away;
`tools/bench/crowd/bench.json` switches the mode (keys 1 / 2 / 3) and the view (L). Same build as the last row, median of
3 runs:

| Segment | Animation update (ms) | Per character | Frame mean (ms) |
|---|---|---|---|
| `alwaysAnimate`, in view | 0.812 | 0.016 | 1.65 |
| `alwaysAnimate`, looking away | 0.808 | 0.016 | 1.65 |
| `cullUpdateTransforms`, looking away | 0.098 | 0.002 | 0.94 |
| `cullCompletely`, looking away | 0.085 | 0.002 | 0.92 |
| `cullUpdateTransforms`, in view | 0.834 | 0.017 | 1.80 |
| `cullCompletely`, in view | 0.837 | 0.017 | 1.78 |

Culled, what is left is walking the scene for players and the state machines (`cullUpdateTransforms`) and the watch over
the renderers (about 0.02 ms). In view with culling on, the frame costs about 0.14 ms more: the renderer works out a box
for each of the 100 skinned renderers from its 65 bones in every view, then tests it against the view and the shadow
maps (a follow-up in TODO.md).

**examples/crowd.** Two busy sidewalks of animated mannequins (two blended clips of 65 bones each), measured with the
release host, windowless `--bench`, 1280x720, fixed step 1/60, on the machine above, `tools/bench/inputs/crowd.json`,
600 frames a segment:

| Segment | fps | Frame mean (ms) | Animation update (ms) | Render (ms) |
|---|---:|---:|---:|---:|
| 250 a block, view 1 street A, `alwaysAnimate` | 38.1 | 26.2 | 14.8 | 10.2 |
| 250, view 3 both, `alwaysAnimate` | 38.7 | 25.9 | 14.4 | 10.3 |
| 250, view 4 over B, `alwaysAnimate` | 39.4 | 25.4 | 14.3 | 9.9 |
| 250, view 4, `cullUpdateTransforms` | 53.6 | 18.7 | 7.9 | 9.3 |
| 250, view 4, `cullCompletely` | 51.8 | 19.3 | 8.1 | 9.7 |
| 250, view 3, `cullCompletely` | 38.5 | 26.0 | 14.4 | 10.2 |
| 1000, view 4, `cullCompletely` | 12.2 | 82.2 | 32.2 | 42.7 |
| 1000, view 4, `alwaysAnimate` | 9.3 | 108.0 | 58.3 | 44.1 |
| 1000, view 4, `cullUpdateTransforms` | 12.1 | 83.0 | 32.6 | 42.9 |
| 1000, view 3, `alwaysAnimate` | 9.1 | 109.8 | 57.8 | 45.4 |
| 1000, view 1, `cullCompletely` | 13.3 | 75.0 | 24.8 | 42.1 |

With block A off screen (view 4) culling halves the animation (A's figures stop); in view 3, with everything seen, the
modes cost the same. Render time does not drop with culling: bone matrices for every skinned renderer are still built
(about 18 ms for 2000 figures) and uploaded (about 6 ms) every frame, seen or not (a follow-up in TODO.md). Before this
work the render took 14.6 ms for 500 figures (skinned meshes never culled, bone matrices per renderer) against 8.6-10 ms
now. Animation is about 28 µs a figure.

**Crowd costs, fixed (2026-10-23).** Same machine and flags, a shorter input (`warmup` 120, each segment 200 frames after
its own warm-up: 250 a block in view 4 through the three modes and view 3, then 1000 a block likewise), run before the work
(at `9778101e`), after the bone matrix changes (`a3dee1e0`) and after the parallel animation
(threads: the machine's, up to 16; `PLATYPUS_ANIM_THREADS=1` for the update in turn). Frame mean / animation update / render, in ms:

| Segment | Before | Bones kept, drawn ones sent once | + animators in parallel | In turn (`PLATYPUS_ANIM_THREADS=1`) |
|---|---|---|---|---|
| 250, view 4, `alwaysAnimate` | 25.4 / 14.0 / 10.0 | 18.4 / 13.5 / 4.1 | 8.6 / 2.9 / 4.6 | 18.6 / 12.9 / 4.7 |
| 250, view 4, `cullUpdateTransforms` | 16.3 / 7.6 / 7.2 | 12.8 / 7.4 / 4.2 | 8.2 / 2.2 / 4.7 | 12.1 / 6.7 / 4.3 |
| 250, view 4, `cullCompletely` | 17.0 / 7.6 / 7.8 | 11.4 / 7.1 / 3.2 | 7.9 / 2.1 / 4.6 | 11.1 / 6.5 / 3.4 |
| 250, view 3, `cullCompletely` | 25.8 / 14.0 / 10.2 | 19.3 / 13.6 / 4.7 | 9.2 / 2.8 / 5.2 | 18.6 / 12.9 / 4.4 |
| 1000, view 4, `cullCompletely` | 80.5 / 31.4 / 41.5 | 59.1 / 31.8 / 19.5 | 34.5 / 6.8 / 20.6 | 55.7 / 28.1 / 20.5 |
| 1000, view 4, `alwaysAnimate` | 105.0 / 57.5 / 42.1 | 83.1 / 57.6 / 20.2 | 38.7 / 12.0 / 21.4 | 81.2 / 55.1 / 21.3 |
| 1000, view 4, `cullUpdateTransforms` | 80.2 / 32.5 / 41.5 | 57.9 / 32.5 / 19.1 | 33.6 / 7.5 / 20.1 | 55.3 / 29.0 / 20.4 |
| 1000, view 3, `cullUpdateTransforms` | 107.1 / 57.6 / 43.4 | 89.4 / 57.9 / 25.0 | 44.5 / 12.3 / 25.8 | 87.1 / 55.0 / 25.9 |

In fps, 1000 a block: 9.5 → 25.9 (`alwaysAnimate`, view 4), 12.4 → 28.9 (`cullCompletely`, view 4), 9.3 → 22.5 (view 3).
Where it went:

- **Bone matrices** (`platypus_app/src/bone_cache.rs`): kept between frames and worked out again per skeleton only when a bone
  moved (`Scene::moved_since`); one shared list for every view instead of a copy each. Measured with temporary timers at 1000
  a block: building them went from 15.2 to 3.1 ms with every figure moving (a bone's objects are looked up once, not hashed
  every frame) and 2.7 ms with half culled (one check per bone); the world draw from 17.5 to 7.7 ms.
- **Uploads** (`platypus_render/src/world/bone_pack.rs`): only the runs of drawn skinned meshes (a view or a shadow map) are
  converted and sent, once a frame for every pass.
- **Animation** (`platypus_animation/src/parallel.rs`): animators advance and blend on several threads, each working out its
  bones' new local transforms; writing them stays in turn, in the walk's order (bit for bit as before, `M5-ANI-17`). At 1000 a
  block: 6.7 ms in parallel (16 threads: 6.0) and about 4 ms writing. The walk now only goes where animated objects are
  (it used to visit every bone of every figure), which also helps in turn.

Frames rendered at the end of each segment are identical before and after, apart from the HUD's lines driven by the wall
clock (fps and ms, and a stats line it samples on a timer).

**examples/crowd in the final run** (`01c1daf1`, the full batch above: `tools/bench/inputs/crowd.json`, the same input as
the first examples/crowd table, 600 frames a segment, the machine's threads for the animation). fps, frame mean, the
animation update (`syncParts` "animation, splines, camera follow"), render and script, in ms:

| Segment | fps | Frame mean | Animation update | Render | Script |
|---|---:|---:|---:|---:|---:|
| 250, view 1 (street, A), alwaysAnimate | 119.1 | 8.4 | 2.8 | 4.6 | 0.96 |
| 250, view 2 (over A), alwaysAnimate | 124.5 | 8.0 | 2.8 | 4.2 | 1.00 |
| 250, view 3 (both), alwaysAnimate | 110.5 | 9.0 | 2.8 | 5.2 | 1.07 |
| 250, view 4 (over B), alwaysAnimate | 117.2 | 8.5 | 2.7 | 4.8 | 0.97 |
| 250, view 4 (over B), cullUpdateTransforms | 130.3 | 7.7 | 2.4 | 4.4 | 0.84 |
| 250, view 4 (over B), cullCompletely | 127.3 | 7.9 | 2.4 | 4.5 | 0.91 |
| 250, view 3 (both), cullCompletely | 98.4 | 10.2 | 3.1 | 6.0 | 1.03 |
| 1000, view 4 (over B), cullCompletely | 29.2 | 34.2 | 8.7 | 20.5 | 4.97 |
| 1000, view 4 (over B), alwaysAnimate | 26.0 | 38.4 | 12.2 | 21.6 | 4.61 |
| 1000, view 4 (over B), cullUpdateTransforms | 29.1 | 34.4 | 9.0 | 20.6 | 4.75 |
| 1000, view 3 (both), alwaysAnimate | 23.0 | 43.5 | 11.9 | 26.3 | 5.28 |
| 1000, view 1 (street, A), cullCompletely | 30.3 | 33.0 | 7.3 | 20.4 | 5.28 |

Against the first examples/crowd table (same input): 250 a block, view 1 went from 38.1 to 119 fps; 1000 a block, view 4
`alwaysAnimate` from 9.3 to 26.0, `cullCompletely` from 12.2 to 29.2; 1000, view 3 `alwaysAnimate` from 9.1 to 23.0. The
animation update at 1000 a block is 12.2 ms (`alwaysAnimate`) and 8.7 ms (`cullCompletely`, block A off screen); render is
about 21 ms with one block in view and 26 ms with both. Script (the HUD and the crowd's own logic) is about 1 ms at 250 and
5 ms at 1000 a block.
