# Scene and objects

Crates and packages: [platypus_scene](../crates/platypus_scene/src) (Rust), the component lifecycle in
[native.rs](../crates/platypus_app/src/native.rs) (`NativeHost`), the API crates `platypus_engine_api` /
`platypus_scenes`, plus the wiring in
[platypus_app/main.rs](../crates/platypus_app/src/main.rs). Design background:
[DESIGN.md](../DESIGN.md) section 7.5. Milestone M5.a. How cameras and planes reach the screen is in
[world-rendering.md](world-rendering.md).

> **QuickJS removal (2026-10-28).** `platypus_script` (`ComponentHost`), `js/engine` (`@platypus/engine`) and the
> `scene.*` op channel (`ScriptScene::apply`, `App::scene_op`, `__platypus_scene`, `__platypus_flush`, the JS mirror of
> objects and the transform batching) are gone. Compiled games use the typed `Scene` API ("The typed script API"
> below) and `NativeHost` for components. Sections that describe ops, the JS mirror, `ComponentHost` and the
> `@platypus/engine` TypeScript API are kept as the description of the data model and the semantics, not of a call path.

## GameObjects in Rust

[lib.rs](../crates/platypus_scene/src/lib.rs) has `Scene`, a generational arena of GameObjects. Each has a
name, an enabled flag, a parent, children and a local `Transform` (position, rotation, scale).
There is no ECS: parent and child links plus a dirty flag per object are enough for small scenes.

- `ObjectId` carries a generation, so the id of a destroyed object misses instead of finding whatever
  reused the slot.
- `create`, `destroy` (returns the object and its descendants, parent first), `set_parent(id, parent,
  keep_world)` (returns `false` for a cycle), `set_local`, `set_world`, `set_enabled`, `find`, `walk`
  (depth-first, in order).
- `active(id)` is true when the object and all its ancestors are enabled.
- World transforms are worked out lazily from the hierarchy. `update_world` refreshes the dirty ones
  (the app calls it before drawing); `world_matrix(id)` composes through the parents.
- `take_moved` returns the objects whose world transform changed since the last call, including new
  ones.

Components do not live in the scene. Script components are in `NativeHost`, physics bodies in
`platypus_physics`, cameras, meshes, lights, sprites, particles and panels in the app. They refer to
objects by id. Bodies are tied to their objects by `physics.attach` (see below).

## Script ids and engine ids

Scripts hold numbers, never engine references. [script.rs](../crates/platypus_scene/src/script.rs) has
`ScriptScene`, which wraps a `Scene` and maps the script's object ids (`ScriptObject`, a `u32`) to
`ObjectId`s, like UI nodes and physics bodies.

| Range | Who picks the id |
|---|---|
| `1 ..` | Scripts (`new GameObject(...)` counts up from 1) |
| `1 << 30` (`ENGINE_IDS`) and up | The engine, for objects it creates for scripts (`create_engine_object`) |

Component ids split too: the host counts the engine's own up from 0, and scripts allocate
`addComponent` ids from `1 << 24`. Ids go through `f64` in `Args::id`, because `f32`
can't hold integers past 2^24.

`spawn_starting_components` in `main.rs` makes one GameObject per **starting** component class
(registered without `spawn: false`), named after it, holding that component. Games build the rest from
`onStart`, or load a scene file ([Scenes](#scenes)).

## `scene.*` ops

Scripts change the scene with ops. `App::tick` matches the `scene.` prefix and calls
`ScriptScene::apply(name, args)`. A bad op logs `scene.<name>: ...` and changes nothing.

| Op | Arguments | Effect |
|---|---|---|
| `scene.create` | `id name parent\|null [p] [q] [s]` | New object. Reusing an id destroys the old object first. |
| `scene.destroy` | `id` | Destroys the subtree. Unknown ids are ignored. |
| `scene.parent` | `id parent\|null` | Re-parents and keeps the **local** transform. Scripts that keep the world position send `scene.local` after. A cycle is an error. |
| `scene.local` | `id [p] [q] [s]` | Sets the local transform. Missing parts are identity. A zero quaternion is an error. |
| `scene.locals` | flat array, 11 numbers per object | Sets many local transforms: `id px py pz qx qy qz qw sx sy sz` each, local. Unknown or destroyed ids are skipped, the quaternion is normalized, and a degenerate one becomes identity. |
| `scene.enabled` | `id bool` | |
| `scene.name` | `id name` | |
| `scene.layer` | `id 0..31` | Sets the render layer (an error outside 0 to 31). Stored on the object, default 0; `ScriptScene::layer(id)` reads it. Cameras, lights, mirrors and probes filter on it (see [world-rendering.md](world-rendering.md#layers-and-culling)). |
| `scene.addComponent` | `object component class` | Returns `SceneEvent::ComponentAdded`. |
| `scene.removeComponent` | `component` | Returns `SceneEvent::ComponentRemoved`. |

`apply` returns an optional `SceneEvent` for what the rest of the engine must do (the native host handles
the same cases):

- `ComponentAdded`: the host adopts the instance the script already created.
- `ComponentRemoved`: `onDestroy`, then the instance is dropped.
- `Destroyed(objects)`: the components on those objects go, then `WorldViews::forget_objects` and
  `Panels::forget_objects`, so cameras, planes, panels and components on those objects go too.

## The move batch

Scripts keep a mirror of every object's name, parent and local transform, so reads need no round
trip. Only changes the **engine** makes (physics moving a body) go back. `set_world_from_engine`
records the object, and `take_batch` returns a flat array, 11 numbers per object:

```
id, px, py, pz, qx, qy, qz, qw, sx, sy, sz      (local transform)
```

(This batch was sent to JS as `__platypus_scene(moved)` once per fixed tick; compiled code now reads the engine's
moves from the scene itself and the app drops the batch.) Physics is
the usual source: `Attachments::step` ([physics_sync.rs](../crates/platypus_app/src/physics_sync.rs)) calls
`set_world_from_engine` for each attached body that moved. Script-made changes are never echoed back.

The other direction is `ScriptScene::take_script_moved`. Every `scene.local`, `scene.locals`, `scene.parent` and
`scene.create` with a transform records the object in `script_moved`. The call returns those objects and
all their descendants (as script ids, once each) and empties the list. Physics calls it before each step
so attached bodies follow objects that scripts moved
([physics-and-audio.md](physics-and-audio.md#components-on-gameobjects-m5a2)).

## The typed script API (`platypus_scene::api`)

[api.rs](../crates/platypus_scene/src/api.rs) declares a `Scene` namespace with `#[ts_api]` (see
[platypus-api.md](platypus-api.md)), next to the op-based `ScriptScene`, which is untouched. Compiled
TypeScript ([platypus-ts.md](platypus-ts.md)) calls its generated shims directly, with no op queue. The
functions: `create`, `destroy`, `exists`, `count`, `parent`, `setParent(object, parent, keepWorld)`,
`enabled`/`setEnabled`, `active`, `layer`/`setLayer`, and position, rotation and scale in local space
(`localPosition`, ...) plus `position` and `rotation` in world space, with `Vector3` and `Quaternion`
value classes (glam's `Vec3` and `Quat`) on `GameObject` handles. Strings cross the boundary
(see [platypus-api.md](platypus-api.md#strings)): `GameObject.name` and `GameObject.find(name)`.

Next to those namespace functions there are two handle classes, declared in `impl` blocks inside the
same module (see [platypus-api.md](platypus-api.md#handle-classes)):

| Class | Rust type | Members |
|---|---|---|
| `GameObject` | `ObjectId` | static `create()`, `find(name)` (the first object with that name, or null); static `findWithTag(tag)` (the first active object with the tag, or null), `findGameObjectsWithTag(tag)`; methods `destroy()`, `exists()`, `compareTag(tag)`; properties `transform` (read-only), `name`, `enabled`, `activeInHierarchy` (read-only), `layer`, `tag` (Unity's tag, "Untagged" by default; saved as `tag` in scene files and copied by `instantiate`) |
| `Transform` | `TransformRef(ObjectId)` | `gameObject` (read-only), `position`, `rotation` (world), `localPosition`, `localRotation`, `localScale`, `parent` (read-only, a `Transform`, 0 for a root), method `setParent(parent, keepWorld)` |

`TransformRef` shares the object's raw handle, so `go.transform` is the same number as `go` seen as
another class and `t.gameObject` gives the object back. Shims are named `platypus_GameObject_create`,
`platypus_Transform_setPosition(this, x, y, z)` and so on, with the receiver first. A script writes
`child.transform.setParent(parent.transform, false)` and `go.transform.position = new Vector3(1, 2, 3)`;
the old `Scene.setParent(..)` style functions remain.

The value classes have math methods too, declared as free functions marked `#[method(Vector3)]` /
`#[getter(Vector3)]` / `#[method(Quaternion)]` (see [platypus-api.md](platypus-api.md#value-classes)):

| Class | Members |
|---|---|
| `Vector3` | methods `add`, `sub`, `scale(s)`, `dot`, `cross`, `distance`, `lerp(b, t)`; getters `magnitude`, `normalized` (the zero vector stays zero) |
| `Quaternion` | methods `mul(b)` (`b` applied first, like Unity's `a * b`), `rotate(v)`; getter `inverse` |

Scripts write `a.add(b).scale(2)`, `v.magnitude` and `q.rotate(v)`. Shims are `platypus_Vector3_add` and so
on, with the receiver's fields first. They live in the `Scene` namespace module for now (a second
crate sharing `Vector3` will move them to a math namespace). No operators (`a + b`) and no static
functions (`Vector3.zero`) yet.

- **Handles.** `ObjectId`'s raw handle is `((generation + 1) << 32) | index`, so no live object is 0
  (0 is `null` in `GameObject | null`; `from_raw(0)` gets a generation no slot has and misses).
- **The scene.** `platypus_scene::api::enter(&mut scene)` makes a scene current on the thread until the
  returned scope is dropped (the `ts_api::context` thread-local). With none, or for a stale id, the
  functions answer with defaults: no object, `false`, `0`, the origin, the identity rotation, scale one.
- **Wired in.** The host calls `enter` around every call into the game (`Native`, see
  [scripting.md](scripting.md)). Tests: [tests/api.rs](../crates/platypus_scene/tests/api.rs) (`M9-API-12..15`, `19`)
  and the compiled `bench/scene.ts` in `tools/platypus_ts_tests` (`M9-TS-151..161`; the math methods are
  `M9-TS-159..161`, the macro side `M9-API-20` in `ts_api`; names and `find` are `M9-TS-183..186`).

## Components and the lifecycle

`NativeHost` ([native.rs](../crates/platypus_app/src/native.rs), `platypus_ts_rt`) keeps the registered classes and
the live component instances; the hooks of a class are known from its compiled form, so a missing hook costs nothing.
Components on inactive objects are skipped. A component that throws is logged and disabled; the others keep running.
(The QuickJS `ComponentHost` with `__platypus_create` / `__platypus_hooks` / `__platypus_run` and the batched
transform flush are gone.)

**Order.** `App::tick` runs, per frame:

1. For each fixed 60 Hz tick (at most 5 catch-up ticks): `Native::fixed_tick` (`onStart` for new components, then
   `onFixedUpdate(dt)` for all); `native_after`; `step_physics` (bodies follow moved objects, rapier, objects follow bodies).
2. `Native::frame`: `Time` advances, timers and microtasks run, `onStart` for new components and `onUpdate(dt)` for all,
   then animation (players pose their objects, see [animation-native.md](animation-native.md)), then
   `onLateUpdate(dt)` for all. `dt` is the frame length, capped at `MAX_FRAME_DT` (0.1 s).
3. `update_particles`, `update_audio`, `Input::Refresh` for the screen and every panel.
4. Drawing: `Native::pre_render` runs `onPreRender()`, then `sync_native_render`, then the world and UI are drawn.

`onAwake` is not called by the host's loop. `GameObject.addComponent` calls it right after creating the
instance and setting its props, and so does spawning a starting component, both before any `onStart`.
So components can set up things other components look for right away (a physics component makes its
body there).

`onDestroy` runs when the component or its object is removed. `onStart` always runs before the first
other hook, in whichever step reaches the component first.

`onApplicationPause(paused)` and `onApplicationFocus(focused)` are optional too. The host calls them
on components of active objects when the window loses or gains focus, or the game pauses in the background
(pause first, then focus, as in Unity); see [platform.md](platform.md#focus-and-pausing).

`onReload` is optional and only runs after a hot reload that kept state. It replaces `onAwake` and
`onStart` for script components brought back from the saved scenes, and runs once all objects are
built. Re-attach listeners and callbacks there ([scripting.md](scripting.md#hot-reload)).

## TypeScript API overview

All of it is declared by the native API crates (`#[ts_api]`; the declarations game code sees are in the generated
`types/platypus.d.ts`). The TypeScript files named below (`scene.ts`, `math.ts`, ...) in `js/engine` were deleted with
QuickJS; their behaviour is kept as the specification of the native classes. The names follow Unity and
s&box.

**Scene**.

- `GameObject`: `new GameObject(name, { parent, position, rotation, scale })`, `name`, `layer` (0 to 31,
  range-checked; see `LayerMask`), `parent`,
  `children`, `components`, `setParent(parent, keepWorld = true)`, `addComponent(Class, props)`,
  `getComponent`, `removeComponent`, `destroy`, `findChild`. Statics: `find(name)`, `roots`, `byId`,
  `findObjectOfType`, `createPrimitive(type, options)` (an object with the mesh and collider of a built-in
  primitive).
- `Transform`: `position`, `rotation`, `scale` (world) and local versions, `forward`, `right`, `up`,
  `eulerAngles`, `translate`, `rotate`, `lookAt`, `transformPoint`, `inverseTransformPoint`,
  `transformDirection`. Native games (the `#[ts_api]` `Transform`) also have Unity's hierarchy:
  `parent` (`null` at a root, as on `GameObject`), `childCount`, `getChild(i)`, `root`,
  `find("a/b")` (also `..`, `.` and a leading `/` from the roots), `localToWorldMatrix` /
  `worldToLocalMatrix` (position, rotation and world scale, no shear), `isChildOf`, `detachChildren`, `siblingIndex` with `setAsFirstSibling` /
  `setAsLastSibling`, `setParent(parent, keepWorld = true)` (keeps the world scale too),
  `transformVector` / `inverseTransformVector` / `inverseTransformDirection`,
  `setPositionAndRotation`, `setLocalPositionAndRotation` and `rotateAround`. `transform.parent = x`
  is not an assignment: call `setParent`. `getComponentInChildren`, `getComponentsInChildren`,
  `getComponentInParent` and `getComponentsInParent` take a class of the game (the walk is a
  generated helper per class).
- `Component`: `id`, `gameObject`, `transform`, `getComponent`, `destroy`, and the optional hooks
  `onStart`, `onUpdate(dt)`, `onLateUpdate(dt)`, `onFixedUpdate(dt)`, `onPreRender()`, `onDestroy()`.
- Optional hooks `onAwake()` and `onReload()` (above) too.
- `register({ Name: Class }, { spawn })`. Registering names a class for scene files. Unless `spawn` is
  `false`, it is also a starting component (the engine makes one on its own object). Engine classes such as `Camera`
  are not registered by games: scenes find them by class name (`platypus_kinds`).
- `@property`: a standard decorator for fields and
  getter/setter pairs that marks what a component saves with its scene. See [Scenes](#scenes).
- `GameObject` also has `scene`, `root`, `GameObject.dontDestroyOnLoad(go)` and
  `GameObject.instantiate(original, { parent, position, rotation })`.

**Math** (`platypus_math`). Immutable `Vector2`, `Vector3`, `Vector4`,
`Vector2Int`, `Vector3Int`, `Quaternion` (Euler degrees in Unity's order, `lookRotation`, `slerp`),
`Matrix4x4`, `Ray`, `Plane`, `Rect`, `RectInt`, `Bounds`, `Pose`, `Color` (`Color.hex` converts sRGB to
linear), and `Mathf`. Coordinates are right-handed, with -z as `Vector3.forward` for cameras
([world-rendering.md](world-rendering.md#coordinates)).

**Dates and spans**. C#-style `DateTime` and `TimeSpan`. **Resolution is one millisecond.** There are no ticks, microseconds or nanoseconds, a
`TimeSpan` rounds to whole milliseconds, and `f` format digits past three print as zeros. Don't use
them to profile.

**Time and timers**. `Time` (`time`, `deltaTime`,
`unscaledDeltaTime`, `fixedDeltaTime`, `frameCount`, `timeScale`, `realtimeSinceStartup`) advances once a frame in `Native::frame`. `timeScale` scales the `dt` components get; physics is not scaled yet. `TimeSince`
(`now`, `ago`, `seconds`, `over`), `TimeUntil` (`in`, `seconds`, `done`, `fraction`) and awaitable
`Task.delay`, `delaySeconds` and `frame` (resolves at the next tick). `Task.delay` uses `setTimeout`,
so it runs on engine time.

**Logging**. `Debug`, `Log` and `Console` all end in `console.*`,
so they reach the app's log sink. Values print readably: vectors via `toString`, errors with their
stack, other objects as JSON (cycles marked). `Log.trace` is `console.debug`.

**Random**. `Random` (`value`, `range`, `rangeInt`, `bool`,
`insideUnitCircle`, `insideUnitSphere`, `onUnitSphere`, `rotation`, `initState`) and a seedable `Rng`
(mulberry32, C#'s `Next` and `NextDouble`, `pick`, `shuffle`). Not for security.

**Input**. On top of the keyboard and gamepad state
([input-and-events.md](input-and-events.md#gamepads-and-game-input-m3c-step-6)): `getKey`,
`getKeyDown` and `getKeyUp` (true for one tick; key repeats are not presses), `getAxis("Horizontal" |
"Vertical")` (keys and left stick, the larger wins), `getButton` and `getButtonDown` (`Jump`, `Fire1`,
`Fire2`, `Submit`, `Cancel`; `getButtonDown` is keys only for now). The mouse: `mousePosition`
(UI pixels), `mouseDelta`, `mouseScrollDelta`, `getMouseButton`/`Down`/`Up` and the axes `"Mouse X"`,
`"Mouse Y"` and `"Mouse ScrollWheel"` ([details](input-and-events.md#the-mouse-for-gameplay-m5a2)).
Press sets roll over once per tick, so they follow `Time`.

**Screen**. `Screen.width`, `height` (UI pixels, what
pointer events use), `scale` and `aspect`, set by the host when the size changes. Window mode, resolutions and `setResolution` are in [platform.md](platform.md).

**Application and system**. `Application` (`name`, `productName`,
`subtitle` (the manifest's `subtitle`, empty when unset), `version`, `companyName`, `identifier`, `engineVersion`, `platform`, `systemLanguage`, `args` (the command line's arguments after `--`, `platypus-app game -- --level 2`), `isMobilePlatform`, `fps` (the measured frame rate: the number the `--fps` counter shows, counted by the host whether or not it is drawn; games should read it rather than time frames themselves), `targetFrameRate`, `runInBackground`, `isFocused`, `isPaused`, `onFocusChanged(fn)`, `showVideoSettings()` (opens the engine's video-settings dialog over the paused game; see [platform.md](platform.md#startup-video-settings-prompt)), `onVideoSettingsChanged(fn)` (after the player applied it),
`quit(exitCode)`, which `platypus-app` exits with; the last four are in [platform.md](platform.md#focus-and-pausing). `crash(message?, plain?)` ends the game as a crash: it saves a crash report beside `Player.log`, shows the crash dialog (with `plain`, the system's message box) and exits with code 3. It is for errors a game can't recover from and for trying the dialog, see [platform.md](platform.md#crash-report-file-and-dialog)) and `SystemInfo` (OS, CPU, memory, architecture, GPU name, vendor, backend
and driver, `screen` size, refresh rate and DPI). The app builds the info (`Info`, [info.rs](../crates/platypus_app/src/info.rs)) from the
game's `project.json` (see docs/platypus-build.md: `title`, `company`, `identifier`,
`icon`, a PNG used for the window icon, and `display`, the default window mode and size; the title is the window title) and the machine,
and hands it to the native `Application` / `SystemInfo` before the program's top level runs (the `__platypus_info` script of
the QuickJS days, with its `isEditor` and `project` fields, is gone). GPU and screen fields are added once the window and renderer exist;
`prefs` and `display` are added too (see [platform.md](platform.md)). `quit()` sets the app's `done` flag.

**Cursor**. `Cursor.visible`, `lockState`
(`CursorLockMode.None`, `Locked`, `Confined`) and `setCursor(image, hotspot)`; see
[input-and-events.md](input-and-events.md#script-cursor-m5a2).

**Rendering, physics and audio components** are in [world-rendering.md](world-rendering.md),
[assets-and-models.md](assets-and-models.md), [particles.md](particles.md) and
[physics-and-audio.md](physics-and-audio.md#components-on-gameobjects-m5a2): `Camera`, `PlaneRenderer`,
`MeshRenderer`, `Light`, `SpriteRenderer`, `ParticleSystem`, `WorldPanel`, `Rigidbody`, colliders, joints,
2D physics, `AudioSource` and `AudioListener`.

## Scenes

Code: `crates/platypus_scenes` (`SceneManager`, `Scene`, `Prefab`, `instantiate`, the `platypus.scene` loader and
saver) and `platypus_kinds`. (It was `scenes.ts` and `property.ts` in `js/engine`; the format is unchanged.) Rust's
`platypus_scene::Scene` is still one flat world of objects. A "scene" here is a set of root GameObjects kept together.

**Scenes and roots.** Every root GameObject has `_scene`. Children belong to their root's scene. New
roots go into the **active** scene. A game starts with one empty scene named `Main`.

**File format.** `SceneData` is plain JSON (`format: "platypus.scene"`, `version: 1`). Objects nest through
`children`. Ids are unique in the file and are what references point at (components share the
numbering with objects).

```json
{
  "format": "platypus.scene", "version": 1, "name": "Level 1",
  "renderSettings": { "ambientLight": { "$type": "Color", "value": [0.1, 0.1, 0.1, 1] } },
  "assets": { "mesh1": { "type": "Mesh", "vertices": [], "triangles": [], "normals": [], "uv": [] } },
  "objects": [{
    "id": 1, "name": "Door", "position": [0, 0, -5], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1],
    "layer": 2, "enabled": false,
    "components": [{ "id": 2, "type": "Door", "properties": {
      "speed": 2, "target": { "$ref": "GameObject", "id": 3 } } }],
    "children": [{ "id": 3, "name": "Handle" }]
  }]
}
```

`layer` (default 0) and `enabled` (default true) are only written when they differ. `model` marks a
model copy (below). `type` is the registered name or an engine class name.

**What is saved.** Each object's name, local transform, layer, enabled flag, children and components.
For a component, only its `@property` values. The engine's own components have `@property` on all
their settings, so a `MeshRenderer` or `Rigidbody` round-trips. Subclasses inherit their parents'
properties (the list lives in the class's decorator metadata).

**Value encoding.** Numbers, strings, booleans, `null`, arrays and plain objects go as they are. The
rest are tagged:

| Value | Saved as |
|---|---|
| `Vector2/3/4`, `Vector2Int/3Int`, `Quaternion`, `Color`, `Rect`, `RectInt` | `{ "$type": "Vector3", "value": [x, y, z] }` |
| A `GameObject` or `Component` in the file | `{ "$ref": "GameObject" or "Component", "id": n }` |
| `Material`, `Sprite`, `Sound` | `{ "$type": ... }` with their fields |
| A `Mesh` from `Mesh.create`, a `Texture` loaded from a file or made with `Texture.create` | once in `assets`, used as `{ "$asset": "mesh1" }`, so users share one copy. A made texture keeps its `width`, `height` and `pixels` (sRGB bytes, top row first, as a hex string) with its options, so a point-filtered texture comes back point filtered |

Also saved: the active scene's `RenderSettings` (ambient light and mode, reflection intensity, skybox,
bloom, fog), applied when that scene becomes active.

**Load order.** `loadObjects` runs in three passes, like Unity: (1) build every object and add every
component *asleep* (`_addComponentAsleep`, no `onAwake`); (2) set every property, resolving
references through the file ids; (3) call `onAwake` on all components, then apply `enabled: false`.
This lets a component refer to one that comes later in the file, and `onAwake` sees finished values.
A component type that isn't registered is kept with a warning, as data (see `Opaque` in [platypus-api.md](platypus-api.md)), and saved back in place.

**SceneManager.** Unity's API:

- `loadScene(data, "single" | "additive")` and `loadSceneAsync(dataOrPath, mode)`. `single` (the
  default) unloads every other scene first, except `DontDestroyOnLoad` objects, and makes the new one
  active. Bad data (wrong `format` or `version`) throws; the async version rejects.
- `createScene(name)`, `unloadScene(scene)` (never the last one), `getActiveScene`, `setActiveScene`,
  `sceneCount`, `getSceneAt`, `getSceneByName`, `moveGameObjectToScene(root, scene)`,
  `mergeScenes(source, destination)`.
- `onSceneLoaded(f)`, `onSceneUnloaded(f)`, `onActiveSceneChanged(f)`; each returns a function that
  removes the listener.
- `Scene`: `name`, `handle`, `path`, `isLoaded`, `getRootGameObjects()`, `rootCount`, and
  `serialize()` (returns `SceneData`; nothing writes files yet, so scripts keep or log it).

**Reading files.** `loadSceneAsync("scenes/level.scene.json")` asks the host to read the file (the QuickJS
`asset.readText` op and `__platypus_asset_text` answer are gone). The app
resolves the path inside the game folder (`resolve_asset`, so escaping it is refused), reads it and
answers asynchronously. `null` (missing,
unreadable or outside the folder) rejects the promise. Bundled data
(`import level from "./level.scene.json"`) needs no read.

In compiled games `loadSceneAsync(path, mode)` and `Prefab.load(path)` are promises too: the program asks
(`platypus_scenes::request_load` / `request_prefab`), the host reads the file from the game folder in the same
tick (`Native::load_requested_scenes`), the scene loads (its `path` is set before `onSceneLoaded` runs) and
the promise resolves; any failure (missing file, bad JSON, path outside the folder) rejects with an `Error`
(`onSceneLoadFailed` natively). Code after the `await` runs in that tick.

**DontDestroyOnLoad.** `GameObject.dontDestroyOnLoad(go)` moves the root of `go` to a scene of that
name. A `single` load leaves it alone, and it isn't counted in `sceneCount`.

**Instantiate.** `GameObject.instantiate(original, { parent, position, rotation })` saves the object
with the same code, then loads it again. References between objects inside the copy point at the
copies. References to things outside it stay pointing at the originals (saved as `live` ids, which
only `instantiate` uses). With no parent the copy goes into the original's scene at the original's
**world** placement; with a parent it keeps the local transform. `position` and `rotation` override.

**Prefabs.** `Prefab` (Unity's prefab asset) is one object tree in the scene file's format, kept for
placing copies. `PrefabData` is `{ format: "platypus.prefab", version: 1, name, root, assets? }`, where
`root` is a `GameObjectData` and `assets` holds the meshes and textures it shares by id.

- `Prefab.fromObject(go)` captures an object and everything under it as it is now (the same `Saver`
  as scenes, so the same limits). References to objects outside the tree are dropped. The root's own
  position and rotation are removed, so a copy starts from the origin. The data is copied, so later
  changes to the original don't leak in. A model part can't be captured on its own.
- A prefab file keeps its `assets` table when loaded (`prefab_from_data`), `prefab_json` writes it back, and
  instantiating hands it to the loader, so a copy finds its shared meshes and textures. `manager::prefab_file_json(object)`
  builds a file straight from an object (the editor's `EditorScene.savePrefab`): the same `Saver` fills `assets`, as for
  a scene, and the root loses `position`, `rotation` and `prefab`.
- `Prefab.fromData(data)` reads saved data (`toJSON()`'s output, or a bundled `.prefab.json`); other data
  is refused. `Prefab.load(path)` reads a file from the game folder through `asset.readText`, once per
  path.
- `prefab.instantiate({ parent, position, rotation })`, or `GameObject.instantiate(prefab, options)`,
  loads a fresh copy of the tree (like a scene load, into the active scene or under `parent`). References
  inside it point at the copy's objects. A copy from a file remembers its path: `GameObject._prefab` is
  saved as `GameObjectData.prefab`, for tools. Saved scenes still hold the object in full.

**Start scene.** `startScene` in the game's `project.json` is handed to the host (`Native::start_scene`), which loads that file additively, makes it
active and drops `Main` if nothing is in it. Starting components still spawn next to it. A failure
is logged as `start scene <path>: ...`.

**Model copies.** An object from `Model.instantiate` has `_model` (`src`, `colliders`) and its
node objects are `_modelPart`. Saving writes `model` and skips the parts. Loading calls
`Model._buildInto`, so the copy is rebuilt from the model file (waiting for the model to arrive, as
`instantiate` does).

**Hide flags.** `GameObject.hideFlags` holds `HideFlags` bits (Unity's values: `HideInHierarchy` 1,
`HideInInspector` 2, `DontSaveInEditor` 4, `NotEditable` 8, `DontSaveInBuild` 16,
`DontUnloadUnusedAsset` 32, and the combinations `DontSave` 52 and `HideAndDontSave` 61). The engine
reads two: scene saving skips an object with any `DontSave` bits (and its children, with no file id for it
or its components), and `Camera.main` skips a `HideInHierarchy` camera. Tools read the rest: the editor's
Hierarchy and Scene view list `visibleObjects()`, which leaves out `HideInHierarchy` objects and what is under them. The editor's own scene camera and frame host are
`HideAndDontSave`. The flags are not saved, and a hot reload restores only saved objects.

**Limits of scenes.**

- Nothing writes files yet (M6 Script IO), so a saved scene comes from `serialize()`'s output.
- Because model copies are rebuilt from the model, objects or components a user added **under a
  model's nodes** are lost when saved. Components on the model's root object are kept. A part can't be
  copied with `instantiate` on its own; copy the root.
- A value the encoder can't handle (a class instance, a texture made from pixels, a model's mesh) is
  saved as `null` with a `scene: ...` warning. Only `Mesh.create` meshes and file textures are kept.
- A reference to an object outside what is saved becomes `null` (except in `instantiate`).
- Hot reload keeps scenes by saving and loading them again; see [scripting.md](scripting.md#hot-reload). It adds `onReload` and plain fields to what is saved.

## Limits

- Hot reload saves every scene before the swap and rebuilds them in the new runtime
  ([scripting.md](scripting.md#hot-reload)). `reload_scripts` still builds a new `ScriptScene` and
  `WorldViews` and drops all panels. Restored script components get `onReload` instead of `onAwake` and
  `onStart`; the bootstrap closure (`spawn_starting_components`) only runs when nothing could be restored.
- Only local transforms cross to scripts, one batch per fixed tick.
- Physics bodies are separate from GameObjects; the `Rigidbody` and collider components link them with
  `physics.attach`, and a body keeps its object's pose only while attached. Scale is not synced to
  bodies.
- `Time.timeScale` doesn't scale physics.

## Tests

`M5-OBJ-01..03` are in [tests/scene.rs](../crates/platypus_scene/tests/scene.rs) (hierarchy, world
transforms, moved sets). `M5-SCN-01..04` cover scenes (round trips, `SceneManager`, `instantiate`) in `crates/platypus_scenes` and the
`platypus_ts_tests` groups; `M5-PRE-01` (prefabs: capture, copies, references, JSON, files) likewise.
`examples/scenes` is the manual check. `M5-SCN-02` and `M5-OBJ-06..08` (the QuickJS scene fixture: ops, move
batch, lifecycle order) and the JS units (`js/engine/test`) were removed with QuickJS. See [testing.md](testing.md).
