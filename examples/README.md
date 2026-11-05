# Platypus examples

Each folder is a complete game (`project.json`, `tsconfig.json`, `src/`, optional `assets/`) with its own README that explains how it is built and run.

| Example | What it shows |
|---|---|
| [hello](./hello/README.md) | Smallest project: one Component, a `src/modules` import |
| [boxes](./boxes/README.md) | UI box model: flex/grid, gradients, shadows, blur |
| [text](./text/README.md) | Text styles, runtime fonts, CJK/emoji fallback |
| [alpha-probe](./alpha-probe/README.md) | Translucent fills, borders and glass over a blue field: how colours blend (sRGB or linear) |
| [css](./css/README.md) | Stylesheets: selectors, pseudo-classes, `var()`, `clip-path` |
| [form](./form/README.md) | Controlled inputs, validation, keyboard and gamepad navigation |
| [flow](./flow/README.md) | CSS floats: a picture and pull-quote with text wrapping around them |
| [table](./table/README.md) | A sortable table from a grid of `display: contents` rows |
| [article](./article/README.md) | Floats and a sortable table in one article |
| [windows](./windows/README.md) | Several OS windows, each with its own UI, talking by events |
| [clock](./clock/README.md) | Twelve switchable clock faces in 2D/3D with a React switcher |
| [shapes](./shapes/README.md) | `LineRenderer2D` and `Shapes.*` 2D vector gallery |
| [splines](./splines/README.md) | `Spline`, `SplineAnimate` and a mouse-driven spline editor |
| [sine-circle-demo](./sine-circle-demo/README.md) | Tweened generative curve with streamed music |
| [raindrop-demo](./raindrop-demo/README.md) | Rain ripples from procedural sprites, weather modes, looping music |
| [emoji](./emoji/README.md) | Large self-running stage show with choreographed acts and synthesised sfx |
| [fishtank](./fishtank/README.md) | Aquarium: boids, fog, procedural 2.5D scenery |
| [platformer](./platformer/README.md) | 2D platformer (Fox Run): sprites, Rigidbody2D, follow camera |
| [physics](./physics/README.md) | Rapier bodies drawn as UI boxes, collision sounds |
| [world](./world/README.md) | 3D tour: cameras, lights, WorldPanel, glTF, rope, particles, spatial audio |
| [scenes](./scenes/README.md) | JSON scenes, `SceneManager`, `dontDestroyOnLoad`, level switching |
| [arcade](./arcade/README.md) | Small complete game: React HUD, two scenes, settings, PlayerPrefs |
| [animation](./animation/README.md) | Animator blend tree, glTF skinned mesh, code-built skinned mesh |
| [classics](./classics/README.md) | Stanford Bunny and teaset glTF models under lights |
| [cloth](./cloth/README.md) | Cloth simulation: flag, tablecloth, curtain |
| [cornell](./cornell/README.md) | Cornell box with faked bounce lighting |
| [materials](./materials/README.md) | PBR material showroom, mirrors, reflection probes |
| [quality](./quality/README.md) | Quality levels and single graphics settings menu |
| [layers](./layers/README.md) | Layers, culling masks, minimap camera, LODGroup |
| [light-units](./light-units/README.md) | Physical light units (lux, lumens) and exposure |
| [tunnel](./tunnel/README.md) | Auto exposure between bright and dark areas |
| [dark-room](./dark-room/README.md) | Eye-adaptation settings bench with a metering graph |
| [fog](./fog/README.md) | Fog modes, height fog, volumetric fog |
| [fog-teapots](./fog-teapots/README.md) | Teapots receding into fog; glTF load and retint |
| [fog-blocks](./fog-blocks/README.md) | Blocky terrain with chunk meshes and a cloud `FogVolume` |
| [fog-world](./fog-world/README.md) | Endless streamed voxel world, fog as render distance |
| [fog-park](./fog-park/README.md) | Victorian park built from primitives, mood switch |
| [fog-raider](./fog-raider/README.md) | Retro low-res tomb with close black fog |
| [godrays-cell](./godrays-cell/README.md) | Prison cell: window beams and dust |
| [godrays-clouds](./godrays-clouds/README.md) | Sunset rays through a cloud layer |
| [godrays-forest](./godrays-forest/README.md) | Beams through a procedural forest canopy |
| [godrays-lift](./godrays-lift/README.md) | Lift doors flooding a dark corridor with light |
| [godrays-room](./godrays-room/README.md) | Derelict hall with a day-night sun path |
| [godrays-shed](./godrays-shed/README.md) | Alien spot beam (pyramid shape), exposure options |
| [godrays-sunset](./godrays-sunset/README.md) | Flat at sunset with a day cycle and city skyline |
| [postfx](./postfx/README.md) | Every built-in `PostProcess` effect switchable, a custom WGSL pass |
| [postfx-look](./postfx-look/README.md) | Stacked looks: noir, dream, comic, old TV, arcade |
| [postfx-aberration](./postfx-aberration/README.md) | Chromatic aberration |
| [postfx-focus](./postfx-focus/README.md) | Depth of field, tilt-shift, blur |
| [postfx-outline](./postfx-outline/README.md) | Ink outlines along silhouettes and creases |
| [postfx-film](./postfx-film/README.md) | Vignette, film grain, colour grading |

## Writing your own game

Start from [hello](./hello/README.md). A game is a folder with `project.json` (only `name` is required), `tsconfig.json`, `src/main.ts(x)` and optional `src/modules/`, `assets/`, `scenes/`. Engine and UI names are globals, so sources import only relative files and `.css`.

- Build, `project.json` fields, modules and packaging a single exe: [docs/platypus-build.md](../docs/platypus-build.md)
- Engine API (`#[ts_api]` classes available to scripts): [docs/platypus-api.md](../docs/platypus-api.md)
- Multiple OS windows: [docs/windows.md](../docs/windows.md)
- Limits of the compiled TypeScript subset: [docs/platypus-ts.md](../docs/platypus-ts.md)

Note: the example sources may be restored to their original form later, so the code excerpts in the per-example READMEs should be re-checked then.
