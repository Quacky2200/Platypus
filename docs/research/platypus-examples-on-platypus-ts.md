# The 19 examples on platypus_ts: a measured gap report

Written 2026-10-20 from base `3ece0dd`. Analysis and probes only: no compiler or engine code was changed. Goal: every
`examples/*` (4,313 lines of TS/TSX) built by the platypus_ts tool into a game that `platypus_app` runs, with **no bun and no
`node_modules`** in the build: `tsc` only for type checking while developing, plus `platypus build`.

How the numbers were made (so they can be repeated):

- **First errors**: `cc_ui`, a scratch copy of `tools/platypus_ts_tests/src/bin/compile_check.rs` that also passes
  `platypus_ts_ui::manifest_json()` (the committed `compile_check` lacks it, so JSX never parsed). Each example's `main.ts(x)` alone.
- **Next errors**: a throwaway script patched the source mechanically (annotate defaulted parameters, lowercase the capitalised
  elements, rename a class that shadows an engine name, cut `RenderSettings.skybox = ..` statements) and recompiled; plus about
  110 one-feature probes (small snippets, one verdict each). The compiler stops at the first error, so every "next" is a
  measured chain link, not a prediction, up to where it stopped being worth chaining (the engine facade keeps producing errors).
- **Engine surface**: the 87 manifests of `compile_check` (`platypus_scene`, `platypus_engine_api`, `platypus_scenes`, `platypus_physics_api`,
  `platypus_render_api`) dumped to JSON and compared with every `@platypus/engine` import, every `X.member` and every option key of
  `addComponent` / `Material.create` / `Texture.*` / `Model.load` in the examples.
- **Compile time**: release `platypus_ts --time`, all 87 manifests loaded.

## 0. The short version

- **0 of 19 compile today except `hello`** (18 stop on a first error; with the `Ui` manifest wired in).
- The blockers fall in three layers, and the third is the biggest:
  1. **Language** (compiler): defaulted parameters without annotations, static accessors, tuples, `Record` over enums, two
     name collisions with engine names, `number | string` style values and ambient React types, multi-file imports.
     About 1,400 lines of Rust (work items 3 to 6).
  2. **React/JSX** (compiler + `ui.ts` prelude): the JSX rewrite is gated on a manifest, capitalised `Text`/`Input`/...,
     text children of components, `useState(fn)`, number-array state, a real bug when an effect calls a state setter,
     handler arity, event fields. About 600 lines (items 1, 2).
  3. **Engine facade** (`#[ts_api]`): the examples are written against the *TypeScript* `@platypus/engine` (object-valued
     properties like `RenderSettings.skybox = {..}`, `GameObject` option keys `scale`/`parent`, structured particle options,
     asset-or-path parameters, `Screen.windowSize`). The Rust API is lower level. About 1,700 lines (items 7 to 10).
- The rest is host work (JIT, `platypus_app` driving compiled code instead of QuickJS, hot reload: about 2,200 lines, items 11 to 13)
  and the build tool (`platypus build`, assets, CSS, dev loop: about 1,500 lines of Rust plus 400 of declarations, items 14 to 16).
- Total: 16 toolchain items, about 7,400 lines of Rust plus 400 of TS declarations (about 12,500 with tests).
- **Rewrites in the examples: about 120 lines** across 4 examples if every toolchain item lands (section 6). Everything else
  compiles as written.
- Compile time is not a problem: 21 to 25 ms for a 330 to 460 line component program including the prelude; arcade (799 lines)
  is estimated at 40 to 60 ms.

## 1. First errors, all 19 (measured, `Ui` manifest on)

| First error | Examples | Class |
|---|---|---|
| `parameter without a type` (`phase = 0`, `yaw = 0`, `rotation = Quaternion.identity`, `color = Color.white`) | animation, cornell, quality, platformer | language: type from default value |
| `a component takes elements as children, not text` (`<Text>..</Text>`, `<Button>Reset</Button>`) | css, form, text | JSX |
| `unknown name RenderSettings` (statement `RenderSettings.skybox = {..}`) | classics, cloth, materials | engine facade |
| `static accessors are not supported` (`static get current()`) | arcade | language |
| `unknown type PlatypusStyle` | boxes | ambient React types |
| `unknown type Tween` (imported from `./tween.ts`) | sine-circle-demo | module graph |
| `Record needs a union of string literals as its keys` (`Record<Weather, ..>`, numeric enum keys) | raindrop-demo | language |
| `type mismatch: f64 where handle:Scene? is expected` (`await SceneManager.loadSceneAsync(..)`) | scenes | engine API shape |
| `type mismatch: tag where handle:Sound is expected` (`playOneShot("assets/thud.wav", v)`) | world | engine facade (path or handle) |
| `unknown name physics` (the old `physics.createBody({..})` object) | physics | engine facade (legacy API) |
| `compiler panicked` at `is_subclass` (a user `class Scene extends Component` shadows the engine handle `Scene`; the same panic for a user `class Model`) | layers (and quality once past its first error) | compiler bug: name shadowing |
| ok | hello | |

Counts by class: language 7 (animation, cornell, quality, platformer, arcade, raindrop-demo, layers), module graph 1
(sine-circle-demo), JSX 3 (css, form, text), ambient types 1 (boxes), engine facade 6 (classics, cloth, materials, physics, world,
scenes), ok 1 (hello): 19.

### 1a. After the engine facade (agent E, items 7 to 10; measured 2026-10-20)

First error of each `main.ts(x)` as written, and the first error once the example's JSX components and `createRoot` lines are stripped and the language gaps (L) patched mechanically in a scratch copy (`tools/probe/strip.py`, not committed): the second column is what is left on the engine side.

| Example | As written | Engine side (scratch copy) |
|---|---|---|
| classics, cloth | ok (engine part) / language | compiles |
| cornell, layers, platformer, world | language (defaults, `Scene` shadowing, `createRoot`) | compiles |
| arcade | static accessors | compiles (after patching `static get`, `Record`, mixed tuples, `useState(fn)`) |
| animation | defaults, tuple types | compiles (tuples patched to `Vector2`; `AnimationCurve.fromPoints([[t, v]])` needs L's tuples + a pair overload) |
| scenes | handler arity (language) | `loadSceneAsync` / `serialize()` shapes done |
| css, form, text, boxes | JSX / ambient types | `addStyleSheet`, `removeStyleSheet`, `loadFont`, `input.gamepads()` / `input.on` done |
| quality, materials, raindrop-demo, sine-circle-demo | language / module graph | not reached |
| physics | retired `physics` object | rewrite later |

### Chains (what comes after the first fix, measured)

| Example | After the first fix the next errors are |
|---|---|
| animation (246) | `[number, number][]` tuple type -> `AnimationCurve.fromPoints([[t, v]], "smooth")` (native wants `Keyframe` values) -> `RenderSettings.skybox` -> `AnimatorController` (native name `AnimationController`) -> `Model.instantiate({position})` options |
| arcade (799) | `Record<string, unknown>` style panels -> `Record<FullScreenMode, string>` -> `RenderSettings.skybox/.bloom` -> `Screen.windowSize / currentResolution / resolutions / supportedFullScreenModes` -> `QualitySettings.names` -> `GameObject.roots`, `Scene.getRootGameObjects`, `getComponents` -> ParticleSystem `shape/sizeOverLifetime/colorOverLifetime` -> `useState(read)` (function as initial state) -> effect that sets state (bug, section 3) |
| boxes (163) | `PlatypusStyle` -> `children?: ReactNode` (text children) -> `style={card}` with mixed `number \| string` values (`unsupported union of [f64, str]`) -> `key={number}` -> `onClick={fn}` with a 0-parameter function |
| classics (98) / cloth (114) / materials (160) | `RenderSettings.skybox = {..}` -> `GameObject` option `scale` ("GameObject has no property `scale` to set") -> `Model.load` options (`position`, `scale`, `rotation`, `colliders`), `Cloth` options (`pins`, `sphereColliders`, `capsuleColliders`), `Texture.create({linear})` |
| cornell (108) | `new Material(color, emissive, smoothness)` (native `Material.create`) -> `new Color(r, g, b)` with three arguments -> `RenderSettings.bloom` -> `scale` option |
| css (187) / form (248) / text (116) | capitalised `Text/Span/Input/TextArea/Image` -> `type ReactNode`/`PlatypusElement`/`PlatypusKeyboardEvent`/`PlatypusStyle` -> effect setter bug (`useTicker`) -> `addStyleSheet`/`removeStyleSheet`/`loadFont` as functions -> `e.scrollTop`, `e.shiftKey`, `e.target.value` on events -> `useRef<PlatypusElement>` with `.focus()`/`.scrollTo()` -> `input.gamepads()` / `input.on("gamepadconnected")` (css and form were also mangled by my own destructuring-default patch, so their later links are the least certain) |
| layers (208) / quality (197) | rename the shadowing class -> `RenderSettings.skybox` -> `QualitySettings` members (`names`; `unknown name QualitySettings`) -> `Camera.layerCullDistances`, `horizontalFieldOfView`, `rect`; `LODGroup.setLODs`; `LineRenderer { positions }` |
| physics (141) | the whole `physics.*` object: `addCollider({shape: {box: ..}})`, `createBody({kind, position})`, `body.onCollisionStart = fn`, `audio.play(path, {volume, rate, pan})`; `PhysicsBody` type |
| platformer (274) | defaults -> `Collision2D` (type) -> `Sprite.grid`, `EdgeCollider2D { points }`, `ParticleSystem` options, `Texture.load({mipmaps})`, `CameraFollow2D { target }` (user class, fine) |
| raindrop-demo (288) | `Record<Weather, ..>` with numeric enum -> `Sound`/`Playback` options -> `audio.play` option shapes -> `Screen`/`QualitySettings` members -> `hsl()` strings are fine |
| scenes (156) | `loadSceneAsync` returns a task id, not `Promise<Scene>` -> `Scene.getRootGameObjects`, `scene.name` ok -> `platypus.startScene`-style helpers (`@property` is supported) |
| sine-circle-demo (409 + 103) | module graph (`Tween`, `Easings`, `lerpNumber`) -> generic class `Tween<T>` (supported, `M9-TS-562`) -> union of `Tween<number>/Tween<Color>` in one array (known gap in `platypus-ts-sine-demo.md`) -> `LineRenderer2D` options |
| world (285) | `playOneShot(path, v)` path-or-handle -> `Camera.main`, `Model.load({colliders})`, `Sprite.grid`, `RenderSettings.bloom`, `WorldPanel` + `createRoot(panel)` -> `Collision.relativeSpeed` (prelude `Collision` has `speed`) |
| hello (13) | compiles as is (3,175 bytes) |

### 1b. First errors after each language/UI commit (agent L)

Measured with `compile_check` on `main.ts(x)`, one block per commit.

**After commit 1 (JSX front end, hooks, ambient types, defaults, shadowing, static getters)**

| Example | First error |
|---|---|
| animation | error: line 37: unsupported type annotation |
| arcade | error: Record needs a union of string literals as its keys; use Map<string, V> for other keys |
| boxes | ok: 44155 bytes of object code |
| classics | error: line 40: unknown name `RenderSettings` |
| cloth | error: line 54: unknown name `RenderSettings` |
| cornell | error: line 32: GameObject has no property `scale` to set |
| css | error: line 41: call to unknown function `addStyleSheet` |
| form | error: line 97: input has no function `gamepads` |
| hello | ok: 3175 bytes of object code |
| layers | error: line 33: GameObject has no property `scale` to set |
| materials | error: line 42: unknown name `RenderSettings` |
| physics | error: line 25: unknown name `physics` |
| platformer | error: unknown type Collider |
| quality | error: line 37: unknown name `QualitySettings` |
| raindrop-demo | error: Record needs a union of string literals as its keys; use Map<string, V> for other keys |
| scenes | error: line 84: type mismatch: f64 where handle:Scene? is expected |
| sine-circle-demo | error: unknown type Tween |
| text | ok: 43192 bytes of object code |
| world | error: line 57: type mismatch: tag where handle:Sound is expected |

**After commit 3 (top-level statements, createRoot, tuples, enum records, self-calling arrows)**

| Example | First error |
|---|---|
| animation | error: line 43: type mismatch: [[f64]] where [obj#20] is expected |
| arcade | error: line 668: cannot use fn(0 params)->void as void |
| boxes | ok: 44455 bytes of object code |
| classics | error: line 40: unknown name `RenderSettings` |
| cloth | error: line 54: unknown name `RenderSettings` |
| cornell | error: line 32: GameObject has no property `scale` to set |
| css | error: line 41: call to unknown function `addStyleSheet` |
| form | error: line 97: input has no function `gamepads` |
| hello | ok: 3175 bytes of object code |
| layers | error: line 157: Camera has no property `horizontalFieldOfView` |
| materials | error: line 42: unknown name `RenderSettings` |
| physics | error: line 90: type mismatch: obj#11 where obj#7 is expected |
| platformer | error: unknown type Collider |
| quality | error: line 105: an effect returns nothing or a cleanup function, not f64 |
| raindrop-demo | error: line 97: Sprite.create: 4 arguments, 2 expected |
| scenes | error: line 104: no property `name` on str |
| sine-circle-demo | error: unknown type Tween |
| text | error: line 10: call to unknown function `loadFont` |
| world | error: line 221: call to unknown function `addStyleSheet` |

### 1c. Round 2, agent F: all 19 build (measured 2026-10-21)

`cd tools && cargo run -q -p platypus_build -- check ../examples` prints `19 built, 0 failed`. The nine that failed, and the root cause fixed for each (tests `M9-TS-2000..2011`, details in `tools/platypus_ts/TODO.md`):

| Example | Was | Fix |
|---|---|---|
| animation | `[[f64]]` where `Vector2[]` is expected | facade `AnimationCurve.fromPoints([[t, v], ..], mode)` (a `Vector2[]` argument still takes the native call) |
| arcade | a callback with a bare `return;` and a later `return cleanup` was typed void; `as const` rows of mixed `number`/`boolean` | the callback is nullable; mixed array literals and `[number, boolean]` types are tuples (objects with fields `0`, `1`, ..); `Screen.supportedFullScreenModes` is a `FullScreenMode[]` |
| form | `unknown type K` (generic arrow over `keyof`), `e.target.type` | source rewrite `lower/keyed.rs`: the arrow is written once per literal key (`set$name`, ..); `UiTarget.type` through `Ui.evTargetKind` (the host must call `platypus_ts_ui::set_target_kind_resolver`, else it is `""` and the Enter-to-save check in `examples/form` does nothing) |
| materials | closure parameter default without an annotation | the default's literal gives the type; the parameter is nullable and the body applies the default, so calls may leave it out |
| platformer, scenes | `unsupported unary operator` (`void x`); hook thunks pass `dt` to hooks that take none | `void` lowered; a method call drops (evaluates) arguments it has no parameter for |
| quality | subscribe calls returned a numeric token; `Record<ShadowQuality, ..>` indexed by an engine string | the facade wraps `QualitySettings.onLevelChanged`, `Screen.onChange`, `Assets.onChanged`, `Application.onFocusChanged`, `SceneManager.on*` into unsubscribe functions (`fn$n(..)` is the native token form); `Record` index by `string` |
| sine-circle-demo | `{x, y}[]` where `Vector2[]` is expected; `cond ? list : []`; `for (const t of [tweenA, tweenB])` over unrelated generic instances | field-copy coercion for plain objects and lists, conditionals go to the context type, the loop is unrolled per element |
| physics | retired `physics` object | `examples/physics/src/main.tsx` rewritten on `Rigidbody`, `BoxCollider`, `SphereCollider` and the `onCollisionEnter` / `onTriggerEnter` hooks (the only rewrite) |

Also fixed because `world` / `classics` hit it at run time: every non-abstract `Component` subclass now gets creation thunks (not only the `register()`ed ones), so `go.addComponent(Spin)` works; `NativeClass.registered` / `ClassMeta.registered` say which ones `register()` named, and the host should start only those.

Left: `UiTarget.type` needs the host to set the resolver; a defaulted `boolean` closure parameter is not nullable yet; tuples have no `.length` and no rest patterns.

## 2. Per-example table

Imports: E = `@platypus/engine` names, R = `@platypus/react` names, H = `react` names. Every example also `import`s nothing relative except
sine-circle-demo (`./tween.ts`), css (`./ui.css`) and form (`./form.css`). "Needs" lists what the example uses that platypus_ts does
not take today (beyond the first error).

| Example | Lines | Imports (E / R / H) | Features | Needs beyond the first error |
|---|---|---|---|---|
| animation | 246 | 22 engine (Animation, AnimationClip/Curve, Animator, AnimatorController, Camera, CameraOrbit, Color, Component, GameObject, Light, Material, Matrix4x4, Mesh, MeshRenderer, Model, Quaternion, RenderSettings, SkinnedMeshRenderer, Vector3, input, register) / createRoot / useEffect, useState | 2 Component classes, `register`, template literals, closures, tuple arrays, 1 effect with cleanup | tuples, `AnimatorController` name, `Mesh`/`SkinnedMeshRenderer` options (`bones`), `RenderSettings.skybox`, `Matrix4x4` use |
| arcade | 799 | 27 engine (Application, FullScreenMode, PlayerPrefs, QualitySettings, Screen, SceneManager, Scene, Rng, Random, Texture, Mirror, ParticleSystem, property, ...) / createRoot / useEffect, useState | 4 Components, 7 `@property`, `static get`, getters, `private`, `Set` of closures, `Record`, `as const` tuples, `Array.from`, `.sort/.flatMap/.filter`, regexp, 46 template literals, 5 `useState`, 4 `useEffect`, `useState(fn)`, 58 JSX elements, spread styles `{...t.panel}`, conditional children `&&` | everything in the chain above |
| boxes | 163 | none / createRoot, PlatypusStyle / ReactNode, useEffect, useState | 0 engine, 35 JSX elements, `PlatypusStyle` constants, `children: ReactNode`, updater setters, keys, fragments | ambient `PlatypusStyle`, `ReactNode`, `number \| string` style values |
| classics | 98 | 14 engine (Camera, CameraOrbit, Model, ...) / createRoot / none | 2 Components, `Model.load`, glTF assets (4 `.glb`) | RenderSettings, option keys |
| cloth | 114 | 17 engine (Cloth, CapsuleCollider, SphereCollider, Mesh, ...) / createRoot / none | 1 Component | Cloth options |
| cornell | 108 | 12 engine / createRoot / none | 1 Component, `box()` helper | Material ctor, 3-argument Color |
| css | 187 | addStyleSheet, removeStyleSheet / createRoot, Image, Input, Span, Text, TextArea / ReactNode, useEffect, useState | `import "./ui.css"`, 58 JSX elements, `className` x34, fragments, `Array.from`, `onScroll`, `dir`, timer effects | capitalised elements, css import, `e.scrollTop`, `addStyleSheet` |
| form | 248 | Gamepad, input / createRoot, Input, PlatypusElement, PlatypusKeyboardEvent, Text, TextArea / ReactNode, useEffect, useRef, useState | `import "./form.css"`, 39 JSX elements, `className` x24, `useRef` x3 with `.focus()`/`.scrollTo()`, controlled inputs `onChange`, regexp, object spread, `.map` to objects | gamepads API, node refs with methods, key events (`shiftKey`) |
| hello | 13 | Component, register / none / none | one Component with `console.log` | none |
| layers | 208 | 16 engine (LODGroup, LayerMask, Rect, Random, ...) / createRoot / useState | 2 Components (one named `Scene`), `private`, `Random`, bit ops on masks | shadowing fix, LODGroup.setLODs, layer cull API |
| materials | 160 | 17 engine (Mirror, ReflectionProbe, Texture, Model, ...) / createRoot / none | 2 Components, material option objects, glTF | RenderSettings, Texture options |
| physics | 141 | audio, Component, physics, PhysicsBody, register / createRoot / useEffect, useState | legacy `physics` API with nested option objects, `body.onCollisionStart = ...`, `useFrame` timer hook, transform strings | **rewrite candidate** (section 6) |
| platformer | 274 | 24 engine (2D physics, SpriteRenderer, ParticleSystem, Mathf, ...) / createRoot / useEffect, useState | 4 Components, 2D colliders, sprite sheets, `Map`/`Set`, `as const` | Collision2D, Sprite.grid, defaults |
| quality | 197 | 14 engine (LineRenderer, QualitySettings, Time, ...) / createRoot / useEffect, useState | 3 Components (one named `Scene`), `LineRenderer { positions }`, generics `<T>` | shadowing fix, QualitySettings.names |
| raindrop-demo | 288 | 16 engine (Sound, Playback, Sprite, SpriteRenderer, Screen, ...) / createRoot / useEffect, useState | numeric `enum Weather`, `Record<Weather, string>`, 2 Components, async-free timers, `globalThis`, `as const` | numeric-enum Record |
| scenes | 156 | Log, Scene, SceneManager, property, ... / createRoot / useEffect, useState | **async/await**, `try/catch`, `JSON`, `@property` x4, scene JSON files (3) | Promise-returning `loadSceneAsync` |
| sine-circle-demo | 409 + 103 | 15 engine (LineRenderer2D, Sound, Playback, ...) / createRoot / useEffect, useState | generic `Tween<T>`, getters/setters, `Record`, `??` x9, type aliases, a 2nd file | module graph, union of generic classes in an array |
| text | 116 | loadFont / createRoot, PlatypusStyle, Span, Text / ReactNode, useEffect, useState | `loadFont`, `Text`/`Span`, `toFixed/padStart` | capitalised elements, `loadFont` |
| world | 285 | 36 engine (WorldPanel, AudioSource, Rope, ParticleSystem, ...) / createRoot / useEffect, useState | 4 Components, `WorldPanel` with `createRoot(panel)`, `addStyleSheet`, sprite animation class, glTF, wav | `Collision.relativeSpeed`, path-or-handle audio, world-panel roots |

Language and library use across all 19 (counts of occurrences in the sources; `#` examples in brackets):

- Used and **already supported** (probed OK): classes extending `Component`, `override`, `private`, instance getters, `@property`,
  `register({..})`, template literals (270 uses, 16 examples), arrow functions, destructuring, spread, `??`, `?.` (calls too),
  `as const` tuples in `for..of`, `Array.from({length}, fn)`, `map/filter/flatMap/sort`, `Set<() => void>`, regexp literals,
  `Math.*`, `String(n)`, `.toFixed/.padStart`, `setTimeout/clearTimeout`, `Map<string, V>`, `JSON`, `try/catch`, async/await
  (compiles to 35 KB for one function: the Promise machinery), `globalThis`, generics (`Tween<T>`), `Record<"a"|"b", V>`,
  interfaces and aliases, `getComponent(Class)` / `addComponent(Class, {..})` / `findObjectOfType`, `Time.timeScale`, `input.*`,
  `PlayerPrefs`, `Texture.create`, `Rng`, `Model.load`, `Material.create({..})`, `SceneManager.getActiveScene().name`.
- **Not used by any example**: context, portals, `memo`, `forwardRef`, `useMemo`, `useCallback`, `useReducer`, `useLayoutEffect`,
  `Intl`, `Date`, `reduce`, `Object.keys`, `satisfies`, `typed arrays`, class inheritance other than `extends Component`.
  (So the whole native-reconciler surface beyond `useState/useEffect/useRef` is not needed for the examples.)

## 3. `@platypus/react` API against the native UI

"Used by N" counts examples (of 18 that use JSX). Native = `platypus_ui_native` + `jsx.rs` + the prelude `ui.ts`.

| API item | Used by | Native support | Work needed |
|---|---|---|---|
| JSX parsing at all | 18 | partial: only when the `Ui` manifest is passed (`mod.rs:251` `wants_ui = manifest.has("Ui") && jsx::wanted(src)`), so `compile_check` and any tool that forgets it fail with `Expected '>'` | make `jsx::wanted/desugar` independent of the manifest (user decision); keep the `Ui` prelude (`UI_PRELUDE`) separate and added only when `Ui` is present |
| JSX on one line: `const [a, setA] = useState(0); return <box/>;` | 0 (examples are prettier-formatted) | **bug**: the hook rewrite produces `Expected a semicolon` when the declaration and the following statement share a line (probes Y1/Y2); multi-line source works | fix the edit to add its own separator |
| intrinsics `box text span input textarea image list dialog` | 18 | yes (lowercase element names go to `Ui.begin(type)`) | `svg` not checked |
| capitalised `Text Span Input TextArea Image List Dialog` (from `@platypus/react`) | 3 (css, form, text) | **no**: "a component takes elements as children, not text" / `call to unknown function Input` | treat these names as aliases of the intrinsics in the rewriter |
| function components with props, destructured props, defaults in destructuring, no return annotation | 18 | yes (probes Z1, Z4, W1) | `return null` without annotation fails (Z5) |
| text / number / string children of a **user component** (`<Button>Reset</Button>`) | 3 | **no** | allow `children: ReactNode` = list of elements, strings and numbers (wrap text into `#text` elements) |
| `{cond && <el/>}`, `{cond ? <a/> : null}` | 6 | `&&` with a bool: **no** (`Ui.child has no version for (handle:UiElement, bool)`, design note says non-boolean `&&` is an error but the boolean form must work); `? :` with `null` works | add the bool overload (child present or absent) |
| `.map(x => <el key=.../>)` | 14 | yes with a string key; `key={number}` fails (`f64 where str`) | accept numeric keys |
| fragments `<>..</>` | 2 | yes | none |
| `style={{..}}` literal with numbers, strings, ternaries | 17 | yes | `cursor: cond ? "pointer" : undefined` fails (W8) |
| `style={{ ...obj, k: v }}` and `style={obj}` | 3 | yes for a typed object (U3 to U5) | `Record<string, unknown>` panels (arcade `Theme.panel`): retype as the style type |
| typed style values (`PlatypusStyle`, `width: number \| string`) | 2 + many shared constants | **no**: unions of number and string are rejected (`unsupported union of [f64, str]`) | a built-in `StyleValue` (number or string) and an ambient `PlatypusStyle` interface |
| `className`, `id`, `tabIndex`, `disabled`, `checked`, `dir`, `placeholder`, `value`, `data-*` | 6 | yes (probe OK) | none |
| `useState` | 14 | partial: updater form yes; `useState<T>(0/false/"")` yes; **lazy initial value `useState(fn)` no**; **state that is `number[]` no** (`Ui.stateNew has no version for ([f64])`) | add the function overload and a ptr overload for typed arrays |
| `useEffect` with deps and cleanup | 13 | partial: cleanup ok, deps ok, **an effect (or a closure inside it) that calls a state setter fails to compile** (`closure: 2 arguments, 1 expected`; probes Y4, V1, V3, S1, X1, X3; `setN(n + 1)` directly in an effect with no deps works, Y6) | **real bug**, hits 12 of the 13 users; must be fixed first among the hook items |
| `useRef` (object ref, node ref) | 1 | yes for `UiNode`; `ref.current?.focus()` / `.scrollTo(x, y)` need `UiNode` methods | add `focus`, `blur`, `scrollTo` to the `Ui` namespace |
| custom hooks (`useGame()`, `useFrame()`, `useTicker()`) | 6 | yes (probe X2) unless they use the effect setter | none beyond the effect bug |
| handlers: `onClick={() => ..}` inline | 18 | yes | `onClick={fn}` with a 0-parameter function and `onClick={cond ? undefined : fn}` fail (arity: the handler type is `fn(1 param)`) |
| event object fields | 6 | `type target clientX clientY key value stopPropagation preventDefault` | add `shiftKey ctrlKey altKey metaKey code button scrollTop scrollLeft deltaX deltaY pointerId`; `e.target.value` (css, form) |
| `createRoot().render(<App/>)` | 18 | yes (probe OK) | `createRoot(panel)` for world panels (`world`) |
| `ReactNode`, `PlatypusElement`, `PlatypusKeyboardEvent`, `PlatypusStyle`, `React.ReactNode` types | 6 | **no**: `unknown type` / `qualified type name` | an ambient declaration file shipped with the toolchain (compiler prelude) and the same file as `.d.ts` for `tsc` |
| context, portals, `memo`, `forwardRef`, `useMemo`, `useCallback`, `useReducer` | 0 | built (M9-TS-575..588) | none |
| CSS: `import "./x.css"` | 2 | **no** (imports are ignored) | the build tool bundles the CSS (section 5) |
| `addStyleSheet`, `removeStyleSheet`, `loadFont` (functions) | 3 | **no** | `Ui`/engine functions over the existing ops |

## 4. `@platypus/engine` against the native API

Imported by the 19 examples: 118 distinct name uses; 20 names are used by 3 or more examples. Native coverage (87 manifests):
Vector2/3/4, Quaternion, Matrix4x4, Rect(Int), Ray, Plane, Bounds, Pose, Color, Mathf, Time/TimeSince/TimeUntil, TimeSpan/DateTime,
input, Gizmos, Camera, Screen, QualitySettings, audio (`Sound`, `Playback`, `Bus`, `Emitter`), AudioSource/Listener, Random/Rng,
LayerMask, Debug/Log/Console, PlayerPrefs, Assets, Application, SystemInfo, SceneManager (`Scene`, `Prefab`), Physics (`PhysicsBody`,
`PhysicsCollider`, `PhysicsJoint`), Rigidbody + colliders + `CharacterController`, joints and `Rope`, Physics2D (Rigidbody2D and 2D
colliders), Light, Mirror, ReflectionProbe, WorldPanel, SpriteRenderer, LineRenderer(2D), MeshRenderer, SkinnedMeshRenderer, LODGroup,
CameraFollow/Orbit, ParticleSystem, Cloth, Material, Sprite, Texture, Mesh, Model, RenderSettings, primitives, the animation classes.

**Names the examples import that exist in no manifest**:

| Name | Used by | Note |
|---|---|---|
| `physics` (lowercase object: `createBody`, `addCollider`, `body.onCollisionStart`) | physics | legacy API; native is `Physics` / `PhysicsBody`; rewrite or provide a shim |
| `AnimatorController` | animation | native class is `AnimationController` |
| `Collider`, `Collision`, `Collision2D` | platformer, world | types; the prelude `Collision` has `speed` where the examples read `relativeSpeed` |
| `FullScreenMode`, `Gamepad` | arcade, form | types (string unions / interface) |
| `addStyleSheet`, `removeStyleSheet`, `loadFont` | css, world, text | functions |
| `Rect` is a value class (ok); `Rng` ok | | |

**Static members used that the native class or namespace lacks**:

| Member | Used by | Native has |
|---|---|---|
| `RenderSettings.skybox = {..}` (7 examples) and `.bloom = {..}` (3) | animation, arcade, classics, cloth, layers, materials, quality; arcade, cornell, world | per-field functions: `skyZenith`, `skyHorizon`, `skyGround`, `skySunDirection`, `skySunColor`, `skySunSize`, `skyHasSun`, `skyboxEnabled`, `bloomEnabled/Intensity/Scatter/Threshold`; needs an object-assign intrinsic that splits the literal into those setters (and `null` meaning off) |
| `Camera.main` | world | `Camera.findOn`; add a static |
| `GameObject.roots`, `GameObject.createPrimitive`, `GameObject.instantiate` | arcade; world; scenes | the compiler already does `go.children` / `GameObject.roots` (M9-TS-1010) but this report's probe for `Scene.getRootGameObjects()` and `getComponents(Class)` fails |
| `QualitySettings.names` | arcade, quality | `getQualityLevel/setQualityLevel`, no list of names |
| `Screen.windowSize`, `.currentResolution`, `.resolutions`, `.supportedFullScreenModes` | arcade | index based: `resolutionCount/Width/Height/RefreshRate`, `supportedFullScreenModeAt/Count`, `windowWidth/Height`, `currentResolutionWidth/..` |
| `Sprite.grid` | world | `Sprite.gridCell` |
| `input.gamepads()`, `input.on("gamepadconnected", fn)` | form | `gamepadCount/Id/Name/Connected/Axis/Button`, `onGamepadConnected/Disconnected` |
| `Collision.relativeSpeed` | world | `speed` |

**Instance members and option keys not in the native class** (from `addComponent` / `new GameObject` / `Material.create` / `Texture.*` / `Model.load` option literals):

| Where | Missing keys / members | Used by |
|---|---|---|
| `new GameObject(name, {..})` | `scale` (-> `localScale`), `parent` (-> `setParent`), `rotation`/`position` work only partly ("GameObject has no property `scale`/`parent` to set") | animation, classics, cloth, cornell, materials, layers, quality, world, arcade |
| `Camera` | `rect`, `layerCullDistances` (array property), `horizontalFieldOfView` | layers, world |
| `ParticleSystem` | structured options `shape {type,size,..}`, `bursts []`, `colorOverLifetime []`, `sizeOverLifetime []`, array-valued `startLifetime/startSize/startSpeed/startColor` as [min,max] (`type mismatch: [f64] where f64`) | arcade, platformer, world |
| `Cloth` | `pins`, `pinBox`, `sphereColliders`, `capsuleColliders` | cloth |
| `Model.load(path, {..})`, `instantiate` | `position`, `scale`, `rotation`, `colliders`, `whenLoaded` | animation, classics, materials, world |
| `Texture.load/create` | `mipmaps`, `linear` | platformer, world, materials |
| `LineRenderer` | `positions` | quality |
| `LODGroup` | `setLODs([{screenRelativeTransitionHeight, renderers}])` | layers |
| `SkinnedMeshRenderer` | `bones`, `rootBone` | animation |
| `EdgeCollider2D` | `points` | platformer |
| `Scene` / `GameObject` | `getRootGameObjects()`, `getComponents(Class)` | arcade |
| path-or-handle parameters | `playOneShot("assets/..", v)`, `mesh: "cube"`, `material`, `sprite: null` (null where a handle is expected), `audio.play(path, {volume, rate, pan})` | world, arcade, physics, platformer |
| `SceneManager.loadSceneAsync` | returns a task id (`f64`), the examples `await` a `Scene` | scenes |
| `new Material(color, emissive, smoothness)`, `new Color(r, g, b)` | constructors of the TS classes | cornell |

## 5. How would `platypus_app` load a game built by platypus_ts?

What the compiler produces today: one object file with C-ABI exports (`ts_<prefix>init`, `host_new_<Class>`, `host_hook_<Class>_<hook>`,
`__uiSignatures`), non-PIC, Cranelift `ObjectModule` (the module type is used in about 5 places of `codegen.rs`). It imports about 350
`ts_rt_*` functions (runtime and GC in `platypus_ts_rt`), `ts_ui_*` (the UI bridge) and one `platypus_<Class>_<fn>` shim per engine API
function (generated by `#[ts_api]`). There is **no table of symbol names to addresses** anywhere (checked `ts_api_macros`), so
anything that is not a static link needs one.

| Option | How | Build needs | Dev loop | Release | Verdict |
|---|---|---|---|---|---|
| (a) AOT object linked into a game-specific host exe | `platypus build` compiles, then links the object with a prebuilt `platypus_host` static library (platypus_app + runtime + API crates) | a linker and a prebuilt static lib per platform and per toolchain version | slow (a link per change; a hot swap impossible without a DLL) | best speed, only option for iOS | right for release; wrong first step |
| (b) relocatable object / DLL loaded by `platypus.exe` | `platypus build` produces `game.obj` or a DLL; `platypus_app` maps it | a DLL needs a linker and an import table into the exe (Windows cannot import from an exe without an import lib); a raw object needs a hand-written loader (COFF/ELF relocations, trampolines for far calls, 500 to 800 lines per format) and the same symbol table as (c) | medium | fine | the worst of both: the cost of (c)'s symbol table plus a loader, without (c)'s simplicity |
| (c) JIT in the app, from source at startup | `platypus_ts` linked into `platypus_app` (oxc + Cranelift, a few MB); `Module` generic over `cranelift-jit`; symbols resolved from a generated table | nothing but `platypus build` writing a source bundle | **fastest**: compile on every save, state kept by `Runtime::refresh` and `PROPS_<Class>` | fine on desktop and Android; not on iOS (no JIT) | **recommended first** |

**Measured compile time** (release `platypus_ts --time`, all manifests): hello 5.2 ms (parse+lower 4.2, codegen 0.9); the 459-line
`sine_demo_full.ts` 20.8 ms (lower 6.1 + Cranelift 14.7, 46 KB object); the 334-line `ui.ts` UI program 24.7 ms (6.4 + 18.3, 54 KB).
Extrapolating linearly (lowering dominated by the prelude, codegen by program size): arcade (799 lines, JSX heavy) about 40 to 60 ms,
the 19 examples together under one second. Start-up JIT is therefore invisible next to window creation, and a file save to running
code stays under 100 ms.

**Recommendation.**

1. Build (c) first: `platypus_ts` as a library in `platypus_app`, `platypus build` writes `dist/game.pbundle` (the resolved module graph as one
   source text, the CSS and an asset list). `platypus_app` JIT-compiles it at start and on change. This gives the dev loop and a
   shippable desktop build with one mechanism.
2. Add (a) for release and for iOS with the *same* IR: `platypus build --release` writes the object, links it with a prebuilt
   `platypus_host` library. Needs the symbol list anyway (as link names), so (c)'s table is not wasted.
3. Do not build (b).

Needed for (c): a symbol table (`#[ts_api]` emitting `SYMBOLS: &[(&str, usize)]` per namespace, about 150 lines; the runtime's
`ts_rt_*` list generated once, about 100 lines); codegen over `Module` instead of `ObjectModule` (about 150 lines); the static data
(strings, tables) defined through the JIT module (about 100 lines); GC roots registration is unchanged (`ts_<prefix>init`).

### What `platypus_app` calls into QuickJS today, per frame and per event (the native backend needs each)

`main.rs` holds a concrete `scripts: QuickJsRuntime` and a `components: ComponentHost` (platypus_script). Everything below goes through
`ScriptRuntime::call_global` / `eval` / `tick` / `drain_ops` / `drain_targeted_commits`.

| Call | When | Native replacement |
|---|---|---|
| `tick(now_ms)` (timers, microtasks, async jobs) | every frame, `main.rs:717` | `ts_rt_timers_advance(now_ms)` + `ts_rt_microtasks_run()` (exists, `event.rs`) |
| `__platypus_tick(dt)` (script `Time`), `__platypus_animate(dt)` | every frame (`components.rs:205..224`) | `Time` is native; animation is native (`platypus_animation`); nothing to call |
| hooks via `__platypus_dispatch(id, method, ..)`: `onUpdate/onLateUpdate/onFixedUpdate/onPreRender/onStart/onAwake/onDestroy/onEnable/onDisable`, focus/pause | every frame and on lifecycle | `NativeHost::update / fixed_update / pre_render / destroy / application_focus / application_pause` (exists) |
| `__platypus_registered()`, `__platypus_create(class, id, obj, name)`, `__platypus_hooks(id)`, `__platypus_destroy(id)` | scene load, spawn, destroy | `NativeHost::class_names / spawn / destroy / destroy_on_objects` (exists) |
| `__platypus_scene(moved)` | after each physics tick, engine-moved objects | native scripts read the thread-local scene directly; nothing to push, but `platypus_app` must keep `ScriptScene` and the native scene API on the same state (they share `platypus_scene` thread-local state today for the typed bindings) |
| `__platypus_physics(moved, contacts, rays, characters)` | after each fixed tick (`main.rs:797`, `physics_sync.rs`) | `NativeHost::dispatch_physics / dispatch_contacts` (exists; contact hooks) |
| `__platypus_audio(ended)` | finished sounds | `Playback.onEnded` closures (API has `onEnded`); host calls the stored callbacks |
| `__platypus_input(...)` | keys, mouse, gamepad events (`game_input.rs`, `main.rs:1501`) | the `input` namespace is state per thread; add event delivery for `input.onKeyDown/onGamepad..` closures |
| `__platypus_ui_event(type, path, data, strings)` | every pointer/key/focus/scroll event (`main.rs:2117`) | `platypus_ui_native::Runtime::dispatch(&mut Event, &mut UiTree)` (exists) |
| `__platypus_commit(ops, target)` drained as `Commit`s | after any script call that rendered React | not needed: the native runtime applies typed ops to the `UiTree` itself; the app owns one `Runtime` per `UiTree` (screen and each world panel) |
| `__platypus_op(name, ..)` drained as `OpBuffer` (`main.rs:951`) | after scripts ran: `render.*`, `texture.*`, `model.*`, `particles.*`, `cloth.*`, `scene.*`, `loadFont`, `addStyleSheet`, `uiScale`, window ops | the ported namespaces no longer produce ops; the leftovers listed in `platypus-native-engine.md` section 2 (rendering, textures, models, particles, cloth) must be `#[ts_api]` calls that write the same request queues the op handlers fill today |
| `__platypus_textures`, `__platypus_models`, `__platypus_asset_text`, `__platypus_assets_changed` | after a load finished | completion callbacks stored as closures (`Texture.whenLoaded`, `Model.whenLoaded`) and called by the host |
| `__platypus_screen(w,h,scale)` | resize | `Screen.onChange` closures |
| `__platypus_focus(focused, paused)`, `__platypus_reloading`, `__platypus_save_state`, `__platypus_restore_state`, `__platypus_hot_begin/end` | focus, hot reload | `NativeHost::application_focus`, `save_properties/load_properties`, `Runtime::refresh` + `__uiSignatures` |
| `__platypus_info` (manifest, machine) | start | `Application`/`SystemInfo` are native |
| `__platypus_log` (editor) | editor console | `Debug/Log` sink, unchanged |

Integration size: `platypus_app` needs a trait object or enum in place of the concrete `QuickJsRuntime` (about 300 changed lines in
`main.rs`) plus a new `native.rs` (about 900 lines): the loader/JIT driver, the per-frame pump above, the UI `Runtime`s, event
closures, hot reload. That crate is owned by another session; coordinate before starting.

## 6. What has to be rewritten in the examples

Expected: **very little**, and only because the example uses an API that is not part of the new engine. With all toolchain items below
landed, the rewrites are:

| Example | Rewrite | Lines | Why not a toolchain item |
|---|---|---|---|
| physics | replace `physics.createBody/addCollider/onCollisionStart` and `audio.play(path, opts)` with `Rigidbody`/colliders on `GameObject`s (or `Physics.createBody` + `PhysicsBody.addBox/addBall` and `Playback`) | about 45 | the legacy object API is what `platypus-native-engine.md` retires; a shim would be a second API to keep |
| animation | `wave()` builds `[time, value]` pairs for `AnimationCurve.fromPoints`; either support tuple arrays plus that overload (item A6, B5) or build `Keyframe` values | about 12 | only if tuples are skipped |
| form | `input.gamepads()` / `input.on("gamepadconnected", ..)` -> `input.gamepadCount()/gamepadName(i)` and `onGamepadConnected` | about 15 | the TS wrapper object `Gamepad[]` has no native counterpart; a 20-line prelude function is the alternative |
| scenes | `bonus = await SceneManager.loadSceneAsync(path, "additive")` -> the Promise-returning overload (item B3) or a callback | about 4 | none if B3 is done |
| arcade | `Record<string, unknown>` in `Theme.panel` -> the style type; `useState(read)` is fine once A2 lands | about 6 | none if the ambient style type is done |
| any | rename user classes that shadow engine names (`Scene`, `Model`) | 2 lines | none if A4 is done (shadowing fix) |

Total if nothing in the "or" column is done: about 120 lines across 4 examples (physics, animation, form, scenes), plus `package.json`
and `tsconfig.json` changes for all 19 (no `@platypus/react`/`react`/`@types/react` dependencies; a `types` entry for the generated
declarations; the `build` script becomes `platypus build`). With the optional items done: physics only (about 45 lines).

Everything else, 4,150 lines of the 4,313, compiles as written once the toolchain items land.

## 7. Work items

Sizes are estimates of production lines (Rust unless marked TS), tests excluded; add about 60% for tests. Order is by examples
unblocked per line, keeping dependencies. "Unblocks" means the first error goes away and the chain moves on; no example is fully
green until the engine items on its chain are done too (section 1).

| # | Item | Lines | Files | Needs | Unblocks (first error) |
|---|---|---|---|---|---|
| 1 | **JSX/hooks always on.** `jsx::wanted/desugar` independent of the manifest; `UI_PRELUDE` added only with the `Ui` manifest; fix the same-line hook rewrite; capitalised `Text/Span/Input/TextArea/Image/List/Dialog` as intrinsic aliases; text and number children of user components; `{bool && <el/>}`; numeric `key`; 0-parameter handler functions and `fn \| undefined` handlers; `undefined` style values | 350 | `tools/platypus_ts/src/lower/{mod,jsx}.rs`, `ui.ts`, `tools/platypus_ts_ui/src/lib.rs` (overloads) | none | css, form, text, boxes and every TSX file parses (18 files); also fixes `compile_check` |
| 2 | **Hook fixes.** The effect-calls-setter bug; `useState(fn)` lazy initial value; state of `number[]` (`stateNew` ptr overload); `useRef` node methods (`focus/blur/scrollTo`); event fields (`shiftKey ctrlKey altKey metaKey code button scrollTop deltaX deltaY target.value`) | 250 | `jsx.rs`, `ui.ts`, `platypus_ts_ui`, `platypus_ui_native` (event data) | 1 | the 12 examples with effects (animation, arcade, css, form, layers, physics, platformer, quality, raindrop-demo, scenes, sine-circle-demo, world) past their hook step |
| 3 | **Names and parameters.** Parameter and field types from defaults and optional markers (`phase = 0`, `= Color.white`, `= []`); return type of an unannotated component with `return null`; user declarations shadow engine names (`class Scene`, `class Model`: the `is_subclass` panic) | 200 | `lower/{types,classes,mod}.rs` | none | animation, cornell, quality, platformer, layers (5 first errors) |
| 4 | **Ambient types + `PlatypusStyle`.** `ReactNode`, `PlatypusElement`, `PlatypusKeyboardEvent`, `PlatypusStyle`, `React.ReactNode`, `FullScreenMode`, `Gamepad`, `Collision(2D)` as a shipped prelude/ambient declaration (also written as `.d.ts` for `tsc`); a built-in `StyleValue` (number or string) so `PlatypusStyle` fields can be `number \| string`; `Record<string, ..>` from `Record<string, unknown>` panels to a typed style | 300 | `lower/types.rs`, a new `lower/ambient.ts`, `tools/platypus_ts/src/types_dts.rs` | 1 | boxes, text, arcade (themes), form (types) |
| 5 | **Static accessors**, **numeric-enum `Record`** (and `Record<EnumOrAlias, V>`), **tuples** `[number, number]` and arrays of tuples | 460 (60 + 150 + 250) | `lower/{classes,types,enums,arrays}.rs` | none | arcade, raindrop-demo, animation (second link) |
| 6 | **Module graph.** Resolve relative imports and re-exports across files, erase `@platypus/*`/`react` imports (names come from the manifests and the ambient prelude), `import type`, rename collisions, one lowered module; `import "./x.css"` recorded for the build tool | 450 | `tools/platypus_ts/src/modules.rs` (new), `lib.rs` | none | sine-circle-demo; required by `platypus build`; lets examples split into files |
| 7 | **Engine facade, part 1: objects and options.** `RenderSettings.skybox = {..}` / `.bloom = {..}` object assignment split into the per-field setters (`null` = off); `GameObject` option keys `scale/parent/rotation/layer/active`; `Camera.main`; `Screen.windowSize/currentResolution/resolutions/supportedFullScreenModes` and `QualitySettings.names` as getters returning small value structs/arrays; `Material` and `Color` constructor shapes of the TS API; path-or-handle parameters (`Texture`, `Sound`, `Mesh`, `sprite: null`); `Collision.relativeSpeed`; `AnimatorController` alias | 650 | `lower/engine_objs.rs`, `crates/platypus_engine_api`, `platypus_render_api`, `platypus_scenes` | 3 | classics, cloth, materials, cornell, animation (3 first errors, 5 chains) |
| 8 | **Engine facade, part 2: structured components.** `ParticleSystem` structured options (shape, bursts, over-lifetime keys, `[min, max]` ranges); `Cloth` pins and colliders; `Model.load` options and `whenLoaded`; `Texture` `mipmaps/linear`; `LineRenderer.positions`; `LODGroup.setLODs`; `SkinnedMeshRenderer.bones`; `EdgeCollider2D.points`; `getComponents(Class)`, `Scene.getRootGameObjects()`, `GameObject.roots`; `Sprite.grid`; `Camera` `rect/layerCullDistances/horizontalFieldOfView` | 700 | `crates/platypus_render_api`, `platypus_physics_api`, `platypus_scenes`, `lower/{engine_objs,components}.rs` | 7 | arcade, platformer, world, layers, quality, cloth, animation (chains) |
| 9 | **Async API shapes.** `SceneManager.loadSceneAsync(): Promise<Scene>`, `Texture.whenLoaded`, `Model.whenLoaded` as Promise-returning functions over the existing task mechanism; `Task.delay` exists | 150 | `platypus_scenes`, `platypus_render_api`, `lower/promise.rs` | 7 | scenes |
| 10 | **UI functions.** `addStyleSheet/removeStyleSheet/loadFont`, `input.gamepads()/input.on(..)` wrapper, `createRoot(panel)` for world panels | 200 | `platypus_ts_ui`, `platypus_engine_api::input`, `platypus_app/panels.rs` | 1, 6 | css, text, form, world |
| 11 | **Symbols and JIT.** `#[ts_api]` emits a symbol table; runtime symbol table; codegen over `Module` with `cranelift-jit`; JIT driver (`compile_and_load(bundle) -> Game`) | 500 | `ts_api_macros`, `tools/platypus_ts/src/codegen.rs`, new `tools/platypus_ts/src/jit.rs`, `platypus_ts_rt` | none | all (runs the compiled code at all) |
| 12 | **Host wiring.** `platypus_app` native backend: replace `QuickJsRuntime` with a trait object; per-frame pump (section 5); `NativeHost` + UI `Runtime`s (screen and panels); input and physics/audio/screen event closures; completion callbacks; faults reported to the log | 1,200 | `crates/platypus_app/src/{main,native,game_input,physics_sync,lifecycle,panels,editor}.rs` | 11 | all (first runnable example: hello, then boxes) |
| 13 | **Hot reload and state.** Recompile on change, `NativeHost::save_properties/load_properties` across the swap, `Runtime::refresh` with `__uiSignatures`, scene save/restore | 500 | `platypus_app/reload.rs`, `platypus_ts_rt/host.rs`, `platypus_scenes` | 11, 12 | dev loop |
| 14 | **`platypus build`.** Reads `package.json` `platypus` section and `tsconfig.json` (`include`, `jsx`), resolves the graph (6), runs `tsc --noEmit` if present (non-fatal when missing), writes `dist/game.pbundle` (source text, CSS, asset list) and, for release, the AOT object (a); **`platypus types`** writes `types/platypus.d.ts` from the manifests (`Namespace::to_dts` exists) and the ambient declarations of 4 so `tsc` type-checks a game with no `node_modules` | 700 Rust + 400 TS declarations | new `tools/platypus_build` (or `platypus_ts build`), `tools/platypus_ts/src/types_dts.rs` | 4, 6 | all (replaces `bun build`) |
| 15 | **Assets and CSS.** `scripts/assets.ts` (GUID `.meta` files, 159 lines) in Rust; CSS bundling in import order with `url()` rebasing and the data-URL inlining that bun did (`dist/game.css`); copy or cook textures/models/fonts/scenes referenced by the code (`Texture.load("assets/..")`, `Model.load`, `loadFont`, `scenes/*.scene.json`, `*.wav/mp3`) into `dist/`; the engine's `--cook` already exists | 450 | `tools/platypus_build`, `crates/platypus_app/src/{cook,asset_db}.rs` | 14 | css, form, text, world, platformer, scenes, raindrop-demo (assets and CSS) |
| 16 | **Dev loop.** `platypus dev`: file watcher (the `notify` crate) -> rebuild (JIT: just reload the bundle) -> message to the running app; replaces `scripts/dev.ts`, `bun run dev`; `scripts/screenshots.ts` becomes `platypus --screenshot` over all examples | 350 | `tools/platypus_build`, `platypus_app/reload.rs` | 11, 13, 14 | dev loop |
| 17 | **Example updates.** Rewrites of section 6, `package.json` and `tsconfig.json` of the 19 examples, a `check:examples` script that builds all 19 | 120 TS + config | `examples/*` | the rest | all |

Total: about 7,400 lines of Rust and 400 of TS declarations (items 1 to 16), plus about 120 lines of example rewrites (3 percent of
the example code). With tests about 12,500.

Order by value:

1. **Item 1** (350 lines): makes every TSX example parse. The first error of 18 files changes at once.
2. **Item 3** (200) and **item 2** (250): four first errors and one panic, then the one real bug that blocks every example that has an effect.
3. **Item 7** (650) with **item 4** (300): the two layers that most chains end in (`RenderSettings`, `GameObject` options, ambient style types).
   From here `classics`, `cornell`, `cloth`, `materials`, `boxes`, `text`, `css`, `form` are the first candidates to compile fully.
   Then 5, 6, 8, 9, 10 for the rest. Items 11 to 13 can run in parallel with 1 to 10 (different files); do 11 first so any
   compiled example can be *run*, not only compiled.

Dependencies at a glance: 1 -> 2, 4, 10; 3 independent; 5 and 6 independent; 7 -> 8 -> 9; 11 -> 12 -> 13 -> 16; 4 and 6 -> 14 -> 15.

## 8. The build tool, assets and the dev loop

- **`platypus build`** (item 14): reads `package.json` -> `platypus { title, company, identifier }` (already present in every example) and
  `tsconfig.json`; collects `src/**/*.ts(x)` through the module graph; runs `tsc --noEmit -p .` if `tsc` is found (the types come from
  `types/platypus.d.ts` that `platypus types` generates from the manifests: no `@platypus/*`, `react` or `@types/react` packages; JSX needs
  `declare namespace JSX { IntrinsicElements }` in the generated file, plus the `React`-style ambient names of item 4); lowers to one
  bundle; writes `dist/`.
- **What `dist/` must contain** for a platypus_ts game: the source bundle (or the AOT object), `game.css`, and the assets the code names. Found
  in the examples: textures (`boxes/stripes.png`, `platformer/*.png`, `world/coin.png`, `css/images`), models (`classics`, `materials`,
  `animation`, `world`: 8 `.glb`), sounds (`physics/thud.wav`, `world/thud.wav`, `raindrop-demo/*.mp3`), scenes (`scenes/scenes/*.scene.json`),
  fonts (`text`, from `/Charm/`; never committed), the `.meta` GUID files `scripts/assets.ts` writes, and the cooked output of `platypus --cook`.
  Path rules stay those of `platypus_core::resolve_asset` (relative, no `..`).
- **CSS**: `import "./ui.css"` (css, form) is a build-time directive: the tool concatenates the files in import order into
  `dist/game.css` with `url()` rebased and small assets inlined as `data:` URLs, exactly what bun produced and `platypus_app` already
  loads (`load_game_css`). `addStyleSheet(css)` stays a runtime call.
- **Dev loop without bun** (items 11, 13, 16): the running `platypus` watches `dist/` today; the new tool watches `src/`, recompiles in
  about 50 ms, and either sends the new bundle to the app (JIT) or rewrites `dist/`. State is kept the way the QuickJS reload does it:
  `save_properties` before, `load_properties` after, `Runtime::refresh` re-renders every component with the new closures and keeps hook
  state except for components whose `__uiSignatures` entry changed.

## 9. Open points and risks

- The throwaway chains stop being predictive after the engine facade: each fix reveals the next missing key. The item sizes for 7 and 8
  are therefore the least certain (plus or minus 40 percent); the language and UI items are well probed.
- `compile_check` should pass the `Ui` manifest as well (a one-line change that item 1 makes redundant).
- The coordinator's list says `layers` printed nothing; here it prints `compiler panicked` (class named `Scene`), found with
  `RUST_BACKTRACE=1` at `lower/classes.rs:574` (`is_subclass`, "a class is a struct").
- `async` costs 35 KB of generated code for one function (the Promise helpers are generated per use): fine, but check the object size once
  `scenes` compiles.
- A `compiled-as-is` goal conflicts with only two things: the `physics` object API (retired by design) and `Record<string, unknown>`
  dynamic style bags (arcade). Everything else is a missing feature, not a style the examples use against the grain.
- `platypus_app` is another session's area (`CLAUDE.md` of the compiler notes); items 12, 13 and 15 touch it.

## 10. The fog, god rays and exposure family (2026-10-21)

`main` added `fog`, `fog-blocks`, `fog-park`, `fog-raider`, `fog-teapots`, `fog-world`, `godrays-cell` / `-clouds` / `-forest` / `-lift` /
`-room` / `-shed` / `-sunset`, `tunnel`, `dark-room`, `light-units` and an updated `quality`, written for the QuickJS path. All 17 build
with `platypus-build` and render (`tests/output/<name>.png` checked by eye: fog bands, height fog, light shafts and sun cones, god rays
through bars and slats, bloom, auto exposure with its read-outs, light units, fog volumes, `RenderStats`).

**Survey: the native API they needed** (every `tsc` error of the first build, grouped; all done, see `docs/platypus-api.md` "Fog, light
shafts and exposure in native games"): `RenderSettings` fog / height fog / volumetric fog / auto exposure / `fixedExposure` /
`exposureTransition` / `freezeExposure()` / `fogCulling`, `FogMode`, `HeightFogSettings`, `VolumetricFogSettings`; `Camera.fog` /
`exposure` / `sceneBrightness` / `resetExposure()` / `aperture` / `shutterSpeed` / `iso` / `physicalExposure`; `Light.volumetricIntensity`
/ `spotShape` / `aspectRatio` / `illuminanceAt` / `measureIlluminance`; `FogVolume`; `RenderStats`; `GraphicsSettings.lightUnits`;
`QualitySettings` pipeline fields; `Mesh.createFlat` / `setFlat` (and `NumberList`, `FlatMeshData`); `Random.pick`; the types
`PrimitiveMesh` and `BloomSettings`; the SVG attributes of `<svg>` elements in `PlatypusProps`. Replaced in the source: `physics.addCollider`
(a `BoxCollider` on the slab).

**What the compiler did not take, and the source edits** (the same few patterns in many examples):

| Pattern | Edit |
|---|---|
| `state[key] = !state[key]` with a literal-union key (`keyof typeof state`) | an `if` chain over the keys |
| `MODES.indexOf(x)` on an array of string literals, `Record<string, V>` | a loop with `===`; `Record<FogMode \| "off", V>` |
| `const hex = Color.hex` (a static function as a value) | `(s: string) => Color.hex(s)` |
| `null as T \| null` in an object literal, `[] as T[]` | an interface and a typed const |
| `new Array(n).fill(x)`, `Array.from({ length }, f)`, `arr[arr.length] = x` | a loop with `push` (an indexed write at the length is out of bounds and crashed the program) |
| `private a = new Float32Array(n)` | `private a: Float32Array = ..` |
| `Math.min(a, ...list)`, `(x[i]! += d)`, `.flat()`, closures over a destructured `const [a, b]` | loops and plain assignments |
| a table indexed by a tag (`TINTS[state.tint]`), `typeof TINTS[Tint]` | an array and an index |

Compiler fixes made for it: `props.on === false` on an optional boolean, an optional `rotation?: Quaternion` passed into
`new GameObject(name, { rotation })`, a typed array where the engine takes `number[]`, and the facade static write (`__xw_`).

**Not ported** (all three done 2026-10-22, section 12): scene-file keys for fog (`scenes.ts` saves no exposure keys),
`Light.measureIlluminance` waiting for the next physics tick (`Physics.afterNextStep`), and `Mesh.createFlat` taking a data object with only
some of its lists.

## 11. The 2D drawing, text and spline family (2026-10-22)

`main` also added `clock`, `emoji`, `fishtank`, `shapes` and `splines`, written for the QuickJS path. All five build with
`platypus-build` and render (`tests/output/<name>.png`, checked by eye: `clock` has twelve faces, each rendered; `emoji` the wheel, the
curtain, entrances, poses, the night sky and sprite particles; `fishtank` the fog, fish, kelp and bubbles; `shapes` every shape and
both fill rules; `splines` the ribbon, car, ball, wave and the editable spline).

**Survey: the native API they needed** (every `tsc` error of the first builds, grouped): `Shapes` (twelve outline helpers in
`js/engine/src/shapes.ts`), `LineRenderer2D.fillColor` / `fillRule` / `sortingOrder` (the QuickJS class sends `render.stroke
id object width color loop points fill rule order`), `TextMesh` (`render.text`), `Spline` / `SplineAnimate`
(`js/engine/src/spline.ts`), `Camera.screenToWorldPoint`, and the SVG attribute `clipPath` in `PlatypusProps`. Done, see
`docs/platypus-api.md` "2D shapes, text in the world and splines in native games": the renderer-facing parts are native (generated
components, `render_sync` forwarding to the same ops, an unchanged-shape cache in `world_view.rs`); `Shapes`, `Spline` and
`SplineAnimate` are the original TypeScript, compiled into the game by the facade, so both paths run one set of maths.
`Gizmos`, `Mesh` and sprites needed nothing new (the emoji art is `Image`s in world panels, its sounds `audio.load`).

**What the compiler did not take, and the source edits** (new ones; the earlier table is in section 10):

| Pattern | Edit |
|---|---|
| `Partial<LineRenderer2D>` / `Partial<TextMesh>` option bags with `{ ...look }` | an options interface with the fields the example uses, and a helper that sets them |
| an interface implemented by classes (`interface Face`, `implements Face`) | an abstract base class (`extends`, `override`) |
| `const x: Interface = classInstance` through function properties | the same: one class hierarchy |
| `Record<string, Act>` / `Record<PoseName, Act>` tables | a `Map<string, Act>` filled by a function; the literal unions (`PoseName`, `Sfx`, `Phase`, `Scene`) became `string` |
| `Map<string, Sound>` / `Set<Playback>` (engine handles as values) | a small class that holds the handle (`Loaded`, `Voice`) |
| `Array.from({ length: n }, f)`, `.flatMap`, `.flat()`, `.reduce`, `[...a].map(..)`, `.find` on objects | loops with `push`; small named functions |
| `const [a, b] = x` of a mixed tuple, `([x, y], i) =>` parameters, `[p[i], p[j]] = [p[j], p[i]]` | indexes and a temporary |
| `rnd(...range)` (spread call arguments), `new Color(...c)` | a helper that takes the array |
| `{ at(t) { .. } }` methods in object literals | now supported (see below), no edit |
| a closure that returns `cond ? { .. } : null` of object literals | an `if` with `return` and the return type written |
| an object literal passed where a named interface is expected, built earlier without the type (`const from = { x, y, top }`) | annotate the const |
| defaulted `boolean` closure parameters left out at a call | pass them |
| `string[i]`, `String(n)[i]` | `charAt(i)` |
| `a && b` between a string and an object | explicit `if` |
| `under<T>(..)` with `T = void` | a non-generic function |
| a class field initialised from `Number.NaN` / `-Infinity` / `.map(..)` | write the type |

**Compiler fixes made for it** (`tools/platypus_ts`): method signatures in interfaces and type literals, named tuple members, method
shorthand in object literals, closures in closures of top-level initialisers, the line of errors in top-level declarations,
`@ Name` requirements on facade chunks, and the JSX rewrite keeping later line numbers. A string-literal union local assigned from
an optional field still panics Cranelift (`declared type of variable .. doesn't match`); `Spline` avoids it with a `string`.

**Not ported / differences**: `emoji`'s sounds are loaded by name (`assets/sfx/<name>.wav`), which `platypus-build` cannot see, so a
*packaged* build would miss them (run from the folder, the assets are found); `Spline` / `SplineAnimate` keep their fields plain
(not `@property`); `clock` and `fishtank` read time and the pointer as before. Only `shapes`, `clock` and `splines` were rendered
frame by frame to compare faces; `emoji` is random per run, so the shots in `tests/output` show one cast member each.

## 12. The original sources (2026-10-22)

The sources of all examples as they are on `main` (QuickJS-era, with `@platypus/engine` imports) are the target: a later step restores
them in place of the rewrites. Built in a scratch copy with `platypus-build check <copy>/examples --tsc` (the rewritten `project.json`
and `tsconfig.json`, the `src/` of `main`; `windows` is new on this branch): **38 of the 40 build, 38 pass `tsc`.** The two that do
not are `physics` and `light-units`, whose `tsc` errors are all the retired `physics` object API (`physics.addCollider`,
`PhysicsBody.id`); they stay on the `Rigidbody` / `BoxCollider` form by decision.

What the nine that failed first needed (details in `docs/platypus-ts.md` "The original example sources"):

| Example | First error | Fix |
|---|---|---|
| clock | `under<T>(root, () => this.build())`: cannot infer `T` | a closure that returns nothing fixes `T = void`; `pop` over handle arrays; `LineRenderer2D.points` as a property |
| emoji | `ReturnType<typeof store<string>>` | instantiation expressions in `typeof`; then, in order: unions of different types (`number \| [number, number]`), a written return type on an inlined callback, self-referring constant objects (`sfx.play` calls `sfx.preload`), `Map<Sfx, Sound>` handle values, `x ?? []` over a set, `playback.onEnded = f`, `new Color(...rgba)`, a style spread of a plain object, `key={maybeString}`, `a && b` of a string and an object, `Record<Sfx, string>` with 80 keys, object-literal getters |
| fog, fog-teapots | `MODES[...]!` assigned to a field: "str where tag is expected" | a value with a union type keeps it in an object literal (only a written string widens); a string becomes a union member through the tag table (`StrTag`) |
| fog-blocks, fog-world | generic arrow with `new (a.constructor as new (n: number) => T)(..)` | generic arrows declared in a function; `x.constructor` for typed arrays and classes |
| fog-park | `Mesh.createFlat` with three of its four lists | the facade tolerates missing lists (`"uv" in data ? .. : []`, a condition known at compile time lowers one arm) |

Library gaps closed with it: `input.gamepads()` pads have `buttons` / `axes` and `input.on("gamepadconnected", e => ..)` gets `{ gamepad }`;
`UiTarget.type` (the app sets the resolver before every event); fog keys in scene files; `Light.measureIlluminance` resolves after the next
physics step; the window facade chunks need `Window` in the program.
