# platypus_api: the script API declared once in Rust

Crates: [platypus_api](../crates/platypus_api/src/lib.rs) (manifest types and generators) and
[platypus_api_macros](../crates/platypus_api_macros/src/lib.rs) (the `#[platypus_api]` attribute). Tests:
[api.rs](../crates/platypus_api/tests/api.rs) (`M9-API-01`..`06`),
[context.rs](../crates/platypus_api/tests/context.rs) (`M9-API-07`..`09`) and
[values.rs](../crates/platypus_api/tests/values.rs) (`M9-API-10`, `11`) and
[classes.rs](../crates/platypus_api/tests/classes.rs) (`M9-API-16`..`18`). Design and the road ahead:
[platypus-ts-engine-boundary.md](research/platypus-ts-engine-boundary.md).

One module of ordinary Rust is the single source for a namespace of the script API: the manifest, the
TypeScript declarations and the `extern "C"` shims that compiled scripts link to. They are all generated from the same
functions, so they cannot drift apart.

> **QuickJS removed (2026-10-28).** `platypus_api` has no `qjs.rs` and no `quickjs` feature; `#[platypus_api(..)]` no
> longer takes `quickjs` and generates no `register_quickjs`; the `platypus_native` crate (`install`,
> `register_quickjs`) and `window::QUICKJS_PRELUDE` are deleted. Where this page still mentions QuickJS (a "QuickJS"
> column, "in QuickJS" notes, the old `quickjs` flag), that is the history of how a type or feature was bound; only the
> compiled (`extern "C"`) side exists. Tests that exercised the QuickJS bindings (`M9-API-05`, `-06`, `-23`..`-26`,
> `-29`..`-34`, `-36`..`-47`, `-100`..`-112`, `-121`..`-125`, `-1151`, `-1430`..`-1432`, `-1500`, `-1613`, `-1625`) were
> removed; the manifest, shim and declaration parts of the others stay.

## Usage

```rust
use platypus_api::{Handle, platypus_api};

#[derive(Clone, Copy)]
pub struct ObjectId(pub u64);
impl Handle for ObjectId {
    fn to_raw(self) -> u64 { self.0 }
    fn from_raw(raw: u64) -> Self { ObjectId(raw) }
}

#[platypus_api(namespace = "Demo", handles(GameObject = ObjectId))]
pub mod demo {
    use super::ObjectId;

    /// Adds two numbers.
    pub fn add(a: f64, b: f64) -> f64 { a + b }

    /// The parent of an object.
    pub fn parent_of(object: ObjectId) -> ObjectId { ObjectId(object.0 + 1) }
}
```

Arguments: `namespace = ".."` (required) and
`handles(TsName = RustType, ..)` (Rust types that implement `platypus_api::Handle`) and
`values(TsName = RustType [field: scalar, ..], ..)` (value classes, below). The module must be
inline (`pub mod x { .. }`).

### Value classes

A small Rust type that scripts see as a class of numbers (`glam::Vec3` as `Vector3`). The type
implements `platypus_api::ValueType<N>` (`N` fields; the parts travel as `f64`):

```rust
impl ValueType<3> for Vec3 {
    fn from_parts(p: [f64; 3]) -> Self { Vec3 { x: p[0] as f32, y: p[1] as f32, z: p[2] as f32 } }
    fn to_parts(self) -> [f64; 3] { [self.x as f64, self.y as f64, self.z as f64] }
}

#[platypus_api(namespace = "Demo", values(Vector3 = Vec3 [x: f32, y: f32, z: f32]))]
pub mod demo {
    pub fn cross(a: Vec3, b: Vec3) -> Vec3 { /* .. */ }
}
```

The `glam` feature (`platypus_api = { features = ["glam"] }`) implements `ValueType` for `Vec2`, `Vec3`,
`Vec4` and `Quat`, so `platypus_scene` declares `Vector3 = Vec3 [x: f32, y: f32, z: f32]` and
`Quaternion = Quat [..]` without writing the impls.

Each field is `f32`, `f64`, `i32` or `u32`, and all fields of one class have the same type. Functions
may take and return the Rust type; scripts write `Demo.cross(new Vector3(1, 0, 0), v)`. `to_dts`
declares `declare class Vector3 { constructor(x: number, ..); x: number; .. }` and `to_json` lists
the class under `values` (fields with their types), with `value:Vector3` as the parameter and result
type.

**Methods and getters.** A free function marked `#[method(Vector3)]` or `#[getter(Vector3)]` becomes a
member of the value class. `Vector3` must be declared in `values(..)` and the function's first
parameter must be that value (else a compile error); a getter takes only the value. Free functions
are used because the value type is a foreign type (glam's `Vec3`), so it cannot get an inherent
`impl`.

```rust
#[method(Vector3)]
pub fn scale(a: Vec3, s: f32) -> Vec3 { a * s }    // a.scale(2)
#[getter(Vector3)]
pub fn magnitude(a: Vec3) -> f32 { a.length() }    // v.magnitude
```

- **Manifest and `.d.ts`.** The functions go to `Namespace.classes` under the value's name, with the
  receiver first as `this` (type `value:Vector3`). `to_dts` declares them inside `declare class
  Vector3` (not as a second class, and no handle alias): `scale(s: number): Vector3; get magnitude():
  number;`. Without the attribute the function stays a namespace function.
- **Shims.** `platypus_Vector3_scale(this_0, this_1, this_2, s, out)`: the receiver travels as the value's
  fields like any value parameter, and a value result goes through the trailing out pointer.
- **Not built.** Static functions on a value class (`Vector3.zero`), operators (`a + b`) and compound
  assignment on value properties.

### Handle classes

The methods, properties and statics of a handle type are ordinary `impl` blocks of its Rust type
inside the module. The type must be declared in `handles(..)`; the class gets the handle's TypeScript
name.

```rust
#[platypus_api(namespace = "Scene", handles(GameObject = ObjectId, Transform = TransformRef))]
pub mod scene_api {
    impl ObjectId {
        pub fn create() -> ObjectId { /* .. */ }          // static: GameObject.create()
        pub fn destroy(self) { /* .. */ }                 // method: go.destroy()
        #[property]
        pub fn enabled(self) -> bool { /* .. */ }         // getter: go.enabled
        pub fn set_enabled(self, enabled: bool) { /* .. */ } // setter: go.enabled = false
        #[property]
        pub fn transform(self) -> TransformRef { TransformRef(self) }
    }
    impl TransformRef { /* .. */ }

    pub fn count() -> u32 { /* .. */ }                    // namespace function: Scene.count()
}
```

| Rust | Script |
|---|---|
| `#[property] pub fn x(self) -> T` | getter `obj.x` |
| `pub fn set_x(self, v: T)` next to a getter `x` of the same type | setter `obj.x = v` (a getter alone is read-only) |
| other `pub fn f(self, ..)` | method `obj.f(..)` |
| `pub fn f(..)` without `self` | static `Class.f(..)` |
| plain `pub fn` outside an `impl` | namespace function `Scene.count()` |

`self` is taken by value (handles are `Copy`). Two classes can sit on the same id, each with its own
Rust newtype: `Transform` is `TransformRef(ObjectId)` and shares the object's raw handle, so
`go.transform` returns the same number. A class replaces the plain handle alias in the declarations.

- **Manifest.** `Function.kind` is a `FnKind` (`Static`, `Method`, `Get`, `Set`); `Namespace.classes`
  holds a `ClassDef { name, functions }` per class. The JSON gains `"kind"` per function and
  `"classes":[{"name":..,"functions":[..]}]`. A class function lists its receiver first, as a
  parameter named `this` (`handle:Transform`). Parameter names are camelCase in the manifest and the
  declarations (`keep_world` is `keepWorld`).
- **`.d.ts`.** `declare class Transform { private constructor(); static f(): T; method(): T; get x():
  T; set x(value: T); }`. A handle without a class still emits `declare type X = number & { .. }`.
- **Shims.** `platypus_<Class>_<jsName>` with `this: u64` first: `platypus_Transform_setPosition(this, x, y, z)`
  (a value result still goes through a trailing out pointer). A setter is `set` plus the capitalised
  property name. Namespace functions keep `platypus_<Namespace>_<jsName>`.

**Static properties.** Inside an `impl`, `#[property] pub fn main() -> T` (no `self`, no parameters) is a static getter and
`pub fn set_main(v: T)` next to it (same type) the static setter: `Window.main` reads like a property in both backends. The
manifest has a `get` / `set` function of the class without a `this` parameter (the compiler keeps a static getter with the
class statics, so `ext_static_prop` reads it, and the setter in `static_setters`). The `.d.ts` says `static readonly main: T;`
(`static get` / `static set` for a writable one), and `platypus-build types` the same. A static function
and a static property of one class cannot share a script name (compile error). A static and an instance member may (`Window.focused`
and `win.focused`): the instance one lives on the prototype. Tests: `crates/platypus_api/tests/static_props.rs` (`M9-API-1490`, `1491`).

## What the macro adds

The module's code is kept as it is. Inside the module the macro adds:

| Item | What |
|---|---|
| `MANIFEST` | A `const Namespace`: name, handle names, value classes, handle classes and, per function, `kind`, `name`, `rust_name`, `symbol`, `doc`, `params`, `ret` |
| `extern "C"` shims | One `#[no_mangle] pub extern "C" fn` per `pub fn` (including those in `impl` blocks of declared handles), named `platypus_<Namespace>_<jsName>` (`platypus_Demo_add`) or `platypus_<Class>_<jsName>` for a class. Private functions are not exported |

Doc comments on the functions become the JSDoc of the declaration. Functions cannot be `async` or
generic; `self` is allowed only in a handle class's `impl`, by value.

## Types

| Rust | TypeScript | C ABI |
|---|---|---|
| `f64` | `number` | `f64` |
| `f32` | `number` | `f32` (narrowed when it crosses) |
| `i32` / `u32` | `number` | `i32` / `u32` |
| `bool` | `boolean` | `bool` |
| `()` | `void` | `()` |
| declared handle | `number & { readonly __handle: "Name" }` (a class: `Name`) | `u64` (`Handle::to_raw`) |
| declared value class | a class with the declared fields (`new Vector3(1, 2, 3)`) | one scalar per field as parameters (`v_0, v_1, v_2`); a result through a trailing out pointer to room for the fields |

| `&str` / `String` | `string` | a parameter: `*const u8` to the script string object; a result: `*const u8` plus a trailing `out_len: *mut i64` | a JS string |
| `&[f64]` / `&[V]` | `number[]` / `Vector3[]` | `*const u8` to the script array | a JS array (elements: instances, objects or arrays) |
| `&[String]` / `Vec<String>` | `string[]` | `*const u8` to the script array of strings; the shim copies them (`platypus_api::script_strs`), so the function may keep them | a JS array of strings |

Any other type is a compile error that lists the supported ones. Names follow `snake_case` to
`camelCase` (`set_time_scale` is `setTimeScale`), done by `camel` in both crates.

### Strings

A function may take `&str` (or `String`) parameters and return `String`. The manifest, JSON and d.ts
type is `string` (`platypus_api::Ty::Str`); `string | null` parameters are not supported.

- **In.** A string parameter is a `*const u8` pointing at the script string object: the header
  `platypus_api::ScriptStr` (layout equal to `platypus_ts_rt::Str`, a test in `platypus_ts_tests` checks it) and
  the bytes inline after its 48 bytes. The shim reads it with `platypus_api::script_str`, which
  borrows the bytes as `&str` with no copy when the string is well formed. Script strings are WTF-8,
  so one with a lone surrogate is copied with each lone surrogate replaced by one U+FFFD. A null
  pointer reads as `""`. The collector is non-moving and the caller roots the string, so it stays
  valid for the call. The engine must copy what it keeps (`set_name` stores an owned `String`).
- **Out.** A `String` result adds a trailing `out_len: *mut i64` to the shim, which returns
  `*const u8` into a per-thread buffer (`platypus_api::return_string`). The compiled call copies it at
  once into a new script string (`ts_rt_str_new_gc`), before any other engine call.
- **Unsafe shims.** A shim with a string parameter is `unsafe extern "C"`, since it dereferences the
  pointer.

```rust
pub fn find(name: &str) -> ObjectId { /* name is borrowed: copy it to keep it */ }
pub fn name(self) -> String { /* the shim returns it through return_string */ }
```

Tests: `M9-API-21`..`22` (crates/platypus_api/tests/strings.rs).

## Generating the files

```rust
let dts = demo::MANIFEST.to_dts(); // declare namespace Demo { function add(a: number, b: number): number; }
let json = demo::MANIFEST.to_json(); // {"namespace":"Demo","functions":[{"name":"add","symbol":"platypus_Demo_add",..}]}
```

`to_dts` writes a `declare type` per handle without a class, a `declare class` per handle class and
value class, plus a `declare namespace` with the JSDoc. `to_json` is the table a compiler reads to call
each symbol directly: the `handles` list (handle type names), the `values` list, the `classes` list
and per function the name, symbol, kind, parameter names and types (`f64`, `bool`, `handle:GameObject`, ...)
and the result type.

**Handle rule.** Compiled scripts store a handle as a raw `u64` and use 0 as null in `T | null`, so
the id 0 means "no object": a handle type must never produce a live id 0 (start generations or
indices at 1).

## The engine context

A shim has no world argument (scripts have none), so the host makes the world current on the thread
that runs scripts and API functions reach it through `platypus_api::context`. `enter` returns a scope
guard that restores the previous world on drop; `with` runs a closure on the current world, or
returns `None` when there is none. While the closure runs the world is not current, so a nested
call gets `None` instead of a second `&mut` to it.

```rust
thread_local! { static WORLD: Cell<*mut World> = const { Cell::new(std::ptr::null_mut()) }; }
let _scope = platypus_api::context::enter(&WORLD, &mut world); // while scripts run
// inside an API function:
let pos = platypus_api::context::with(&WORLD, |w| w.position(id)).unwrap_or_default();
// a nested `with(&WORLD, ..)` inside that closure returns None
```

## Status

This is the script API of every engine crate (`platypus_scene::api`, `platypus_engine_api`, `platypus_render_api`,
`platypus_physics_api`, `platypus_scenes`, `platypus_math`); the op channel it once sat next to (`__platypus_op`) is gone. See
[scene-and-objects.md](scene-and-objects.md#the-typed-script-api-platypus_sceneapi). The `platypus_ts` compiler reads the JSON manifest and calls the shims directly
(`--manifest`, see [platypus-ts.md](platypus-ts.md); the test namespace is `tools/platypus_demo_api`). Value
classes (`Vector3`) with methods and getters (`a.add(b)`), handle classes (`GameObject`, `Transform`)
and the engine context exist, and so do strings across the boundary (`GameObject.name` and `find`);
static functions and operators on value classes are not built.

Compiled code calls the shim directly. (The measured call cost against the QuickJS `__platypus_op` path, about 150 ns
against 245 ns, no longer has a counterpart; the QuickJS bindings for values, strings and handles
(`M9-API-23..26`, `-100..-113`) were removed.)

## Namespace properties and value classes

- **Namespace properties.** In a namespace (no `impl`), `#[property] pub fn color() -> Color` is a getter and `pub fn set_color(c: Color)`
  next to it the setter: scripts write `Gizmos.color = c` and `Time.deltaTime`. The manifest has them as functions of kind `get` and
  `set` with no receiver, the `.d.ts` as `let color: Color;` (`const` when read only), the shims as `platypus_Gizmos_color()` /
  `platypus_Gizmos_setColor(..)`, and `platypus_ts` calls the getter on a read and the
  setter on an assignment (`Place::NsProp`, also for `+=`). A value class's statics are the properties and functions of a namespace
  with the same name (`Color.white`, `Color.hex("#ff0000")`).
- **Value classes.** Methods (`#[method(Vector3)]`) and getters (`#[getter(Vector3)]`) are free Rust functions with the
  receiver's fields first; the compiler turns `a.add(b)` into a call of that shim.

## The engine API beyond the scene

[platypus_engine_api](../crates/platypus_engine_api/src/lib.rs) declares, with `#[platypus_api]`, what the sine-circle demo and most gameplay
code use besides `Scene`: `Color`, `Time`, `input`, `Gizmos`, `Camera`, `Screen` and `QualitySettings`, with the names and behaviour of
the former `js/engine` (deleted). Each keeps its state per thread and has plain Rust functions for the host: `time::advance(real_dt)`,
`input::{key_down_event, key_up_event, mouse_button_event, mouse_move, end_frame}`, `gizmos::take_frame()`, `camera::cameras()`,
`screen::{state, quality, set_size}`, which `platypus_app` calls. Tests `M9-API-27..36` (`-29`..`-34` and `-36` tested the QuickJS side and were removed).

`platypus_engine_api` also has `audio` (`load(path, stream)`, `Sound.play(loop)`, `Playback.pause/resume/stop`, recorded as
`AudioCommand`s for the host). A Rust parameter written `r#loop` is the script's `loop`. Tests `M9-TS-593..597`.

### UI scale (`UiScale`)

`platypus_engine_api::ui_scale` declares `UiScale` (`none()`, `fixed(factor)`, `height(pixels)`, `fit(width, height)`):
how screen UI is scaled to the window. The host reads `ui_scale::current()` when it sizes the screen on every renderer
and resets it on a hot reload ([input-and-events.md](input-and-events.md#ui-scale)).

## Optional parameters and renamed members

- `#[optional(a, b)]` on a function marks trailing parameters a call may leave out. The manifest says
  `optional`, the declaration writes `seed?: number`, and the function sees a default: **NaN** for a
  number (so `seed.is_nan()` means "left out"), an empty string, `false` or a zero handle. A compiled
  call fills them in.
- `#[options]` (with `#[optional(..)]`) marks a function whose optional parameters a script passes as one plain object, by camelCase name:
  `Window.create({ title: "Palette", style: "tool", minWidth: 200 })`. Compiled code maps a trailing
  object literal onto the optional parameters by name (`call_ext_args`); positional calls keep working. The manifest and the `.d.ts`
  do not say more. A function with `#[options]` cannot have optional value-class parameters (a value is itself a plain
  object). Tests `M9-API-1433..1434` (`-1430`..`-1432`, the QuickJS side, were removed).
- `new Rng(seed)`: a handle class whose static `create` has parameters is constructed with them
  (`new Class(args)` calls `create(args)`).
- `#[js_name = "graphicsDeviceVendorID"]` gives the script name when `snake_case` to `camelCase` does not
  spell it. A Rust keyword is written raw (`r#in` for `TimeUntil.in(2)`).

## The first batch moved out of `js/engine` (2026-10-19; `js/engine` is deleted)

`platypus_engine_api` now also declares `Rng` / `Random`, `LayerMask`, `Debug` / `Log` / `Console`,
`PlayerPrefs`, `Assets`, `Application` / `SystemInfo`, `TimeSince` / `TimeUntil`. Each keeps its state per
thread and has host functions: `layers::set_game_layers`, `log::set_sink`, `prefs::{load, take_changes}`,
`assets::{set_assets, changed}`, `app::{set_app, set_system, set_focus, take_requests}`. Differences from
the TypeScript, all because a call has fixed typed parameters:

- logging functions take one string (compiled code writes a template literal); objects are not printed as JSON;
- `getMask` takes up to four names; a left-out lookup (`guidToPath`) returns `""` instead of `undefined`;
- `Assets` has `count()` / `pathAt(i)` / `guidAt(i)` / `importer(path)` as JSON text / `takeChanges()` (a
  newline-separated string to poll) instead of `all`, an object and `onChanged(fn)`. It also has `exists(path)`,
  `list(glob)` (a `string[]` of the sorted paths matching a glob, `*` / `?` inside one folder name, `**` across folders,
  case-sensitive; a `Vec<String>` result of the macro, table below) and
  `packOf(path)` (`assets`, `scene-level1`, `""` for a loose file). They answer from the index the host fills with
  `platypus_engine_api::assets::set_asset_index` (every path of the mounted packs or game folder, `asset_db::index`), so they see assets
  without a `.meta`;
- `SystemInfo.screenWidth` / `screenHeight` / `screenRefreshRate` / `screenDpi` replace `SystemInfo.screen`;
- `Application.onFocusChanged(fn)`, `Task` and the vector-valued `Random` members wait for closures and the
  Rust math types.

### Input, TimeSpan and DateTime

- **Input is per event domain** (window family): `input` reads the keys of the family whose code is running; main-game components always read the main window's, an isolated window's keys never reach it, focus loss clears a domain's keys, and callbacks with no domain tag read the main domain's. See `docs/windows.md` ("Input belongs to the window's event domain").
- `input` (it followed the deleted `js/engine/src/input.ts`): events (`key_down`, `key_up_event`, `mouse_move`, `mouse_delta`, `wheel`,
  `gamepad_*`, `blur`) arrive as they happen and become "this frame" at `input::tick()`, which `time::advance` calls (the engine
  tick). Mouse buttons are keys (`MouseLeft`...). Gamepads are read by id (`gamepadCount`, `gamepadId(i)`, `gamepadName(id)`,
  `gamepadButton(id, name)`, `gamepadAxis(id, name)`); `input.gamepads()` gives `{ id, name, connected, buttons, axes }` (the standard
  buttons and axes by name, `0` when not reported; written in TypeScript over the readers above by the facade) and
  `input.on(kind, fn)` takes every kind of input event and returns the unsubscribe function: `keydown` / `keyup` (`{ code, key, repeat }`; `key` is the layout's text, from `key_down_with_text` / `key_up_with_text`), `gamepadconnected` / `gamepaddisconnected` (`{ gamepad }`), `gamepadbuttondown` / `gamepadbuttonup` (`{ gamepad, button }`) and `gamepadaxis` (`{ gamepad, axis, value }`). The kind is the enum `InputEventType`, the event the record `InputEvent` (fields of other kinds are empty). `Application.args` (the strings after `--` on `platypus-app`'s command line, `AppInfo.args`) and `Application.quit(exitCode?)` (`Request::Quit(i32)`; `platypus-app` exits with the code) are natives too. Tests: `M9-TS-3361..3383` (`tools/platypus_ts_tests/tests/{engine_api/app,ui/ui,lang/readers}.rs`).
- `TimeSpan` (field `totalMilliseconds`) and `DateTime` (fields `epochMilliseconds`, `utc`; local time through the OS zone or
  `datetime::set_zone`) are value classes. A static `create` makes `new TimeSpan(ms)` and `new DateTime(y, m, ...)` round and
  carry as TypeScript does (compiled code calls it; the engine builds its own results with the fields).
  Differences: `parse` logs an error and returns NaN instead of throwing, `DateTime.parse` knows ISO 8601 only (no whitespace),
  `subtract(TimeSpan)` is `subtractTimeSpan`, `DateTime.fromDate(Date)` and `Task` are not there.

### The math types (`platypus_math`)

`crates/platypus_math` declares `Mathf`, `Vector2/3/4`, `Vector2Int/3Int`, `Quaternion`, `Matrix4x4`, `Rect`, `RectInt`,
`Ray`, `Plane`, `Bounds` and `Pose` as the deleted `js/engine/src/math.ts` had them (the scene, input, gizmos and colour APIs use its
types, so they live below `platypus_scene`). Values are **doubles** (the TypeScript's), computed in the same order, so
results are bit for bit the same as bun's for everything but `Math.hypot` of three or more numbers, where engines differ
in the last bit: the native version scales by the largest and sums the squares (two numbers use the C library's `hypot`,
as bun does), and the tests that compare with bun run the original with the same algorithm. The scene's `Vector3` /
`Quaternion` are now these f64 types too (positions are still stored as f32 in the scene), and `Color` is f64.

- `new Vector3(x, y, z)` calls the static `create` (a missing component is 0).
- A name with several versions (`Quaternion.euler(x, y, z)` and `euler(vector)`) is chosen by argument types in compiled code.
- A value class argument may be another value class with the same fields (a `Vector2Int` where a `Vector2` goes).
- Differences: the `...Like` parameters are the classes; `toArray()`, `from(v)`, `new Matrix4x4(array)` and `.m` are not
  there (`Matrix4x4.fromColumns` and `get(row, col)` are); `Mathf.smoothDamp` returns a `Vector2` (value, velocity);
  `Plane.raycast` / `intersect` answer NaN where the TypeScript returned `null`; `RectInt.positions()` is not there;
  `Ray`, `Plane`, `Bounds` and `Pose` keep flat numbers and give their vectors from getters; `Random.insideUnitSphere` and
  friends can now be added (the vectors exist).

### Defaults for left-out parameters

`#[optional(keep_world = true, fade = 0.3)]` gives a left-out parameter a value instead of NaN / false / empty: a compiled call
passes it (`GameObject.setParent(parent, keepWorld = true)` is declared this way). The manifest says `"default": 1`
(a flag is 0 or 1).

## Scenes (`platypus_scenes`, `platypus_kinds`)

`platypus_scenes` is `scenes.ts` in Rust: `SceneManager` (`loadScene(text, mode?)`, `getActiveScene`, `setActiveScene`,
`createScene`, `unloadScene`, `moveGameObjectToScene`, `mergeScenes`, `sceneOf`, `dontDestroyOnLoad`, `instantiate`), `Scene`
(`name`, `path`, `isLoaded`, `rootCount` / `rootAt(i)`, `serialize()`) and `Prefab` (`fromObject`, `fromData`, `instantiate`,
`toJSON`). The file format is `scenes.ts`'s `platypus.scene` version 1, read and written with `serde_json`.

The loader does not know component classes. `platypus_kinds` is a per-thread registry of *kinds* (`create`, `on`, `save`, `load`,
`awake`, `object`, `destroy` over raw `u64` handles). A kind may also say what its fields are for the editor's Inspector:
`register_fields(kind, Vec<FieldDoc>)` (`FieldDoc`: `name`, `ty` such as `f64`, `bool`, `string`, `enum:a|b|c`, `color`, `v3`,
`texture`, the `default` as a saved value, and a `doc` line) and `fields_of(kind)` read it back. `platypus_render_api`'s build
script writes it for the generated components, and `kind_fields.rs` of `platypus_physics_api` (the physics components) and of
`platypus_scenes` (below) write it by hand; a kind with none is typed from its saved values. `platypus_scenes` registers `Camera`, `Animation` (`clip`, `clips`,
`playAutomatically`, `cullingType`) and `Animator` (`controller`, `speed`, `cullingMode`, `enabled`; not its parameter values
or current state), with the shared asset kinds `AnimationController` and `AnimationClip` (the `toJson` text, the controller's
clips inline: one `assets` entry however many components use it; a prefab or copy keeps the live one; fields a file lacks keep
their defaults, `M5-ANI-16`); a compiled
program's `NativeHost` registers one kind per class it has (properties go through the `PROPS` table: numbers, booleans, strings,
component and `GameObject` references, and the value classes `Vector2/3/4`, `Quaternion`, `Color`, `Rect`, ... written in
place). A component whose kind is not registered (a game's script class, in the editor, which has not compiled the game's code) is kept
with a warning (`take_warnings()`) as an `Opaque` component of its object: its JSON data with references turned into
placeholders, a stand-in handle so references to it get file ids, and its index among the object's components. The saver
writes it back in its place, and `instantiate` copies carry it. `opaque_components(object)` lists them, `remove_opaque` drops
one (`M5-SCN-10`). References are placeholders for
kinds (`{"$ref":"GameObject","object":raw}`, `{"$ref":"Component","kind","handle"}`) that the loader and saver map to file ids.
`instantiate` is save plus load with external references kept live. The compiler lowers `GameObject.instantiate(go, { position,
rotation, parent })` and `prefab.instantiate({..})` onto the numeric `SceneManager.instantiate` / `Prefab.instantiate`.

Differences: the components of one object are saved kind by kind, not in the order they were added; `loadSceneAsync` is a host
request (`request_load` / `take_load_requests` / `finish_load`); scene events are read with `take_events()` until closure APIs
exist; compiled programs do not keep state across a hot reload (they restart); `$asset`, `Material` and `Texture` values are not
loaded yet.

## Physics components (`platypus_physics_api`)

`crates/platypus_physics_api` declares physics for compiled scripts: the low-level `Physics` namespace (`createBody`,
world colliders, `raycast` that answers at once, `createJoint`, `step`) and the components of `physics-components.ts` and
`physics2d.ts` as handle classes over the GameObject (`go.addComponent(Rigidbody, { mass: 2 })`,
`go.getComponent(BoxCollider)` through the static `findOn`). `Physics.raycast(origin, direction, maxDistance?, layerMask?)`
answers a `RaycastHit` (`gameObject`, `transform`, `rigidbody`, `collider`, `point`, `normal`, `distance`) or `null` on a miss; the
mask keeps the colliders whose GameObject is on those layers, and a collider the origin starts inside is not hit (as in Unity).
`maxDistance` is unlimited by default. `Physics.raycastAll(origin, direction, maxDistance?, layerMask?)` answers every
`RaycastHit` on the ray, nearest first (Unity leaves the order open), and `Physics.overlapSphere(center, radius, layerMask?)`
every `PhysicsCollider` the sphere touches or contains, triggers included, in creation order (each has `gameObject` and
`transform`). Differences: no `out` parameter or bool, no `QueryTriggerInteraction`, and no `overlapBox` / `overlapCapsule` /
`sphereCast` yet. Layout: `comp.rs` (component data, the one body per
GameObject shared by its Rigidbody and colliders, joints, rope, per-step sync), `components.rs`, `joints.rs`,
`physics2d.rs` (the generated-looking class impls: same members repeated per class), `kinds.rs` (`register_kinds()`
puts the 18 classes into `platypus_kinds` for scene files and `instantiate`).

- **Every property change reaches the rapier world at once.** A collider is remade when its shape changes.
- **`step()`** (`Physics.step()`): builds pending joints and ropes (`start_pending`), moves bodies to where scripts put their
  objects (pose differs by more than 1e-4 m), remakes colliders whose object was scaled, steps the world, writes the moved
  bodies back into the scene and keeps the contacts. Objects that are gone from the scene lose their physics. Then it runs the
  closures `Physics.afterNextStep(f)` was given, once each (what `Light.measureIlluminance` waits for).
- **For the host:** `take_contacts()` (collider numbers, `speed`, the relative `velocity` going in, `sensor`),
  `collider_info(collider)` (the object, the collider class name, its component handle, `two_d`, the Rigidbody component) to
  dispatch `onCollisionEnter` / `onTriggerEnter` (and the `2D` names), `object_destroyed(object)` for destroyed GameObjects,
  `register_kinds()`.
- **What the contact hooks get** (`contacts.rs`, the namespace `PhysicsContacts`, 2026-10-22): Unity's shape, as handle classes.
  `Collision` (`gameObject` and `transform` of the other side, `collider`, `rigidbody` (null for none), `relativeVelocity`,
  `impulse`, `contactCount`, `contacts`, `getContact(i)`), `ContactPoint` (`point`, `normal`, `separation`, `impulse`,
  `thisCollider`, `otherCollider`), `Collision2D` (also `otherCollider`, `otherRigidbody`, `enabled`, 2D vectors) and
  `ContactPoint2D` (`normalImpulse`, `tangentImpulse`, `relativeVelocity`, `collider`, `otherCollider`, `rigidbody`,
  `otherRigidbody`, `enabled`). `Collider` and `Collider2D` are every collider component seen through its shared members
  (`gameObject`, `isTrigger`, `attachedRigidbody`, `bounds`, `closestPoint`, `closestPointOnBounds` / `offset`); the handle is
  the component's own number, and `go.getComponent(Collider)` finds the first collider of any shape (`findOn`). The host calls
  `contacts::collision(own, other, object, velocity, touching)` once per event (the handle), `collider_component(collider)`
  for a trigger's `other`, and `clear_collisions()` at the start of each dispatch: a `Collision` is valid until the next
  dispatch, like Unity's reused ones. `relativeVelocity` is the other side's velocity minus this one's (going in for Enter,
  current for Stay and Exit). Contact points are read from rapier's narrow phase the first time a script asks
  (`platypus_physics::Physics::contact_points`: world point halfway between the surfaces, normal towards the side asking, the
  solver's normal impulse and the friction impulse it keeps for warm starting) into one buffer kept between dispatches, so
  a hook that does not look at them costs nothing; Exit and trigger collisions have none. Unity members left out are in
  `docs/differences.md` ("Collisions"). Tests `M4-PHY-08`, `M9-TS-1004`, `M9-TS-1005`.
- Differences to the TypeScript are listed in `tools/platypus_ts/TODO.md` ("Physics components"). Tests `M9-API-200..209`,
  `M9-TS-1100..1105`.
Readiness port: `Physics2D.ignoreCollision`, `ForceMode2D`, 2D collider `bounds` / `closestPoint` and `HingeJoint2D` are
in `platypus_physics_api` (details in [physics-and-audio.md](physics-and-audio.md#2d-physics)). `Camera.allCameras` and
`EventList` (`platypus_engine_api/src/event_list.rs`: no type arguments; `addListener` returns a token for
`removeListener(token)`; `RDY-EVT-01`) are in the engine API.

## Audio (`audio`, `AudioSource`, `AudioListener`)

`audio.rs` (it replaced the deleted `js/engine/src/audio.ts`) has `audio.load(path, stream?)`, `audio.play(path, {..})`, `Sound.play({ volume, rate, pan, loop,
fadeIn, start, bus, emitter })`, `Playback` (`setVolume/Rate/Pan(v, fade?)`, `pause/resume/stop(fade?)`, `seek`, `playing`, `paused`,
`onEnded(f)`), `Bus` (`audio.createBus({ volume, parent })`), `Emitter` (`audio.createEmitter(position, { minDistance, maxDistance, bus })`),
`audio.setListener`, `audio.setMasterVolume`. Each change is an `AudioCommand`; the host takes them with `audio::take_commands()` and
runs `command.to_op()` on `platypus_audio::Mixer::apply` (a `Load` carries the path and stream flag: the host reads the file and calls
`Mixer::load`). When `Mixer::poll` reports finished playbacks the host calls `audio::ended(&[PlaybackRef])` (runs `onEnded`).

`audio_components.rs` is `audio-components.ts`: `AudioSource` and `AudioListener` are engine components (static `attach` / `findOn`, so
`go.addComponent(AudioSource, { clip, spatialBlend: 1 })` and `getComponent` work), with scene-file kinds `AudioSource`
(`clip` as `{"$type":"Sound","src","guid"}` or a path, `playOnAwake`, `spatialBlend`, `minDistance`, `maxDistance`, `volume`, `pitch`,
`panStereo`, `mute`, `loop`) and `AudioListener`; registered by `platypus_scenes` (`audio_components::register_kinds`). The host calls
`audio_components::pre_render()` once a frame after transforms are final (emitters follow their objects, the listener follows its object;
only real moves are sent). `objects::process_destroyed` removes the components of destroyed objects.

Differences from the TypeScript: `on: Bus | Emitter` is `bus` and `emitter`; `playback.onEnded = f` is `playback.onEnded(f)`; a clip given as a
path is `src.clipPath = "x"` and `src.playOneShotPath("x")` (calls are not chosen by argument type for handle classes); the static
`AudioListener.volume` is `AudioListener.getVolume()` / `setVolume(v)`; `outputBus` is none (0) for the main mix; `audio.masterVolume`
and `audio.soundCount` are read-only properties; `createEmitter` takes a `Vector3`.

## Rendering data (`platypus_render_api`)

[platypus_render_api](../crates/platypus_render_api/src/lib.rs) is `rendering.ts`, `textures.ts` (and, below, `particles.ts` ...) as data: nothing
draws, the app reads per-thread registries and a command queue. **No change to `platypus_render` or `platypus_app` yet**: wiring is theirs.

- **Components** (engine components like `Camera`: static `attach` / `findOn`, `gameObject`, one property per field, and a scene-file
  kind each): `Light`, `PlaneRenderer`, `MeshRenderer`, `SkinnedMeshRenderer`, `Mirror`, `ReflectionProbe`, `LODGroup`, `WorldPanel`,
  `SpriteRenderer`, `LineRenderer2D`. The host reads them with `platypus_render_api::lights()`, `mesh_renderers()`, ... (`Vec<(Ref, Data)>`)
  and removes the components of destroyed objects with `platypus_render_api::destroyed(&objects)`. `Camera` gains `clearFlags`, `rect`,
  `cullingMask`, `layerCullDistances`, `layerCullSpherical`, `antialiasing`, `targetTexture`, `sceneGrid`.
- **`PostProcess`** (`postfx.rs`, a handle class over a registry, not a component): `PostProcess.create(effect)` with the enum
  `PostEffect` (the built-in effects; their table and WGSL are `EFFECTS` / `postfx/*.wgsl`), `createCustom(source, parameters?)`,
  `enabled`, `order`, `camera` (`Camera | null`), `setFloat` / `setColor` / `setVector` and their getters. The host reads the enabled
  passes with `post_passes()` when `post_version()` changes. See [postprocessing.md](postprocessing.md). Tests `M7-FX-01`..`03`, `09`.
- **Assets** (handles with asset kinds): `Texture` (`Texture.load(src, { filterMode, wrapMode, mipmaps, anisoLevel, linear })`,
  `Texture.create(w, h, colors)`, `Texture.createRenderTexture(w, h)`), `Mesh` (`Mesh.create({ vertices, triangles, normals?, uv? })`,
  built-in meshes by name), `Material` (`new Material({ color, metallic, ... })` or `Material.create({ .. })`, `copy()`, `withColor(c)`),
  `Sprite`. The host takes `take_commands()` (`Texture(t)` upload, `Mesh(m)`, ... removals), reads `texture_info(t)` / `mesh_info(m)`
  and reports file loads with `texture_loaded(t, w, h)`.
- **Generated code.** `build.rs` writes `generated.rs` (into `OUT_DIR`) from the field tables in `gen/components.rs` (type, default, doc, hooks); it runs with every build, so
  there is nothing to regenerate. Hand-written parts are `resources.rs`.
- **Scene files.** `platypus_kinds::AssetKind` (shared kinds go into the file's `assets` table as `{"$asset":"texture1"}`, inline kinds
  such as `Material` / `Sprite` are `{"$type":"Material",..}`); components save assets as `{"$handle":"Texture","id":N}` placeholders
  and the saver / loader (`platypus_scenes::data`) translate, one handle per table entry. `instantiate` and in-memory prefabs keep the
  handles. `mesh: "cube"` is saved as the string. The example `level1.scene.json` loads completely.
- **Compiler help.** A string where a handle is expected calls the class's `fromString` (`mesh: "sphere"` is `Mesh.fromString`); an
  optional array parameter left out is empty; `Class.create({ .. })` on a class whose `create` takes nothing is `new Class({ .. })`;
  an optional handle left out is a zero handle.
- **Effects.** `ParticleSystem` (main settings as properties, `startLifetime` etc. as a value plus `...Max` and
  `setStartLifetimeRange(min, max)`, `setStartColorRange`, `shapeType` / `shapeRadius` / `shapeAngle` / `shapeSize`, `addBurst`,
  `addSizeKey`, `addColorKey`, `setTextureSheet`, `play` / `stop(clear)` / `pause` / `clear` / `emit(n)`, `onStopped(f)`): the data is in
  `particle_systems()`; scripts' `play` / `emit` ... are queued for the host (`take_particle_controls()`), which reports back with
  `report_particles(system, count, alive)` (runs `onStopped`, and `stopAction: "destroy"` destroys the object). `LineRenderer`
  (`setPositions(points)`, `redraw()`; a tube through a `MeshRenderer` of its own that scene files leave out; the host calls
  `pre_render_lines()` before drawing). `Cloth` (`rebuild()` swaps the object's `MeshRenderer` mesh for an engine-filled copy that
  `mesh_info` marks `engine`; `destroy` puts the original back; the physics side reads `pins`, `pin_box` and the collider lists).
  `Primitives.create(kind, { name })` with `PrimitiveType.Cube` ... (`set_primitive_collider_hook` lets the physics side add the
  collider). Differences: `Cloth` rebuilds on a scene load or an explicit `rebuild()`, not on `onStart`; `LineRenderer.positions`
  and the other arrays are call-style (`setPositions`, `addPin`, `addSphereCollider`).
- **Models** (`models.rs`): `Model.load(path)` (once per file; queues `RenderCommand::Model`), `isLoaded` / `failed`, `materialCount` /
  `materialAt(i)`, `meshCount` / `meshAt(i)`, `instantiate({ position, rotation, scale, parent, colliders })`. The host reads the file,
  makes its meshes and textures with `engine_mesh()` / `engine_texture(w, h)` and reports the TypeScript's description JSON with
  `model_loaded(model, Some(json))` (`None`: it failed); copies asked for earlier are built then (a GameObject per node, a mesh renderer
  per primitive, or child objects when a node has several; skinned nodes get a `SkinnedMeshRenderer` with bones and bind poses; glTF
  materials). `colliders: "mesh" | "convex"` goes through `set_model_collider_hook`. Scene files carry a copy as the object's `"model"`
  (`platypus_kinds::ObjectExtra`: the model's own nodes are left out and built again on load). `materials` and `animations` are lists
  (handles); the file's animations become looping clips when it loads (`platypus_animation::gltf::clip_from_channels`: paths by node
  names from the copy's root, rotation keys made sign-continuous, cubic keys with the file's tangents), and a copy gets an
  `Animation` holding them that plays the first, as `models.ts` does.
- **Screen, QualitySettings, Gizmos** (`platypus_engine_api`): `Screen` has `scale`, `aspect`, `fullScreenMode`, `setResolution(w, h, { mode,
  refreshRate })`, `windowWidth` / `windowHeight`, `resolutionCount` + `resolutionWidth(i)` ..., `currentResolution...`,
  `supportedFullScreenModes` (a `FullScreenMode[]`, an enum list) and `onChange(f)` (token, `offChange`). The host feeds it with `screen::update(w, h,
  scale)` (runs the listeners) and `set_display(..)` and takes the window changes scripts asked for with `take_requests()`.
  `QualitySettings` has the named levels (`levelCount`, `levelName(i)`, `levelJson(i)`, `getQualityLevel()`, `setQualityLevel(i)` /
  `setQualityLevelByName(name)`, `onLevelChanged(f)`) and every setting with the TypeScript's validation (unknown names ignored,
  numbers clamped). The game's `platypus.quality` manifest goes through `screen::configure_quality(&json)`, the player's saved choice
  (`platypus.quality` in PlayerPrefs) through `apply_default_quality()`; the renderer reads `quality()` when `quality_version()` changed.
  `Gizmos` has `drawRay`, `drawWireCube`, `drawWireDisc`, `drawWireSphere`, `drawFrustum`, `matrix` and `lineCount`.
- **Settings and behaviours.** `RenderSettings` (namespace properties: `ambientLight`, `ambientMode`, `reflectionIntensity`,
  `skyboxEnabled` + `skyZenith` / `skyHorizon` / `skyGround` / `skySunDirection` / `skyHasSun` / `skySunColor` / `skySunSize`,
  `bloomEnabled` + `bloomThreshold` / `bloomIntensity` / `bloomScatter`, `tonemapping`, `postExposure`; the host reads
  `settings()` when `version()` changed). Scene files apply their `renderSettings` when the scene becomes active and the active scene
  saves the current ones; the keys are the ones `scenes.ts` saves (`ambientLight`, `ambientMode`, `reflectionIntensity`, `skybox`, `bloom`, and the fog's `fog`, `fogMode`, `fogColor`, `fogDensity`, `fogStartDistance`, `fogEndDistance`, `heightFog`, `cloudLayer`, `volumetricFog`, `fogCulling`; the TypeScript tier saves no exposure keys either). `Cursor` (`visible`, `lockState`, `setCursor(path, { hotspotX, hotspotY })`, `clearCursor()`;
  host: `platypus_engine_api::cursor::state()`) with `CursorLockMode.None/Locked/Confined` strings. `CameraFollow` and `CameraOrbit` are
  components whose update the host runs: `late_update_camera_follows(dt)` and `late_update_camera_orbits(dt)` once a frame.
- **Differences from the TypeScript.** `RenderSettings.skybox = { .. }` is the `sky...` properties; `Cursor.setCursor(texture)` takes a
  path only. `Material` is mutable (a shared handle, like Unity's `sharedMaterial`; `with(..)` is
  `copy()`); `Texture.load` mipmaps / linear given as their defaults let the importer decide; `RenderTexture` is a `Texture`
  (`createRenderTexture`, `isRenderTexture`); `whenLoaded()` is a facade promise (polled each frame; rejects when loading failed); `Sprite.create(texture, ppu?)`
  with `pivot` / `setRect(..)` setters and `Sprite.gridCell(texture, columns, rows, index)` instead of `grid`; a `SkinnedMeshRenderer` is not
  found by `getComponent(MeshRenderer)`; arrays of objects are `addBone(b)` / `addLod(..)` style calls (`bones`, `lods`, `points`);
  `Mirror.normal` has a `hasNormal` flag; `specularColor = null` is `clearSpecularColor()`; enum-like strings (`Light.type`) ignore
  unknown names instead of storing them.

## Closures given to the engine (`platypus_api::callback`)

A parameter of type `Callback<(A, B)>` (elements `f64`, `i32`, `u32`, `bool`, `String` or `HandleArg<T>` for a declared handle) is a
script closure the engine keeps and calls later: `input.onKeyDown((code, repeat) => ..)`. Manifest type `fn(f64,string):void`
(`ExtTy::Fn(params, result)` in the compiler; the old `fn:void` form still reads), declaration `(a: number, b: string) => void`.
The closure returns nothing.

- **Rust side.** `cb.call((1.0, "x".into()))`, `Callback::from_fn(rust_closure)` (host code and tests), `Listeners<A>` (a table with
  `add` / `remove` / `snapshot`; tokens are unique across all events) and `emit(&snapshot, args)`. Emit after the state borrow is
  released, from a snapshot, so a listener may subscribe, unsubscribe or call the API.
- **Runtime.** The shim wraps the closure pointer with `Callback::from_script`, which roots it (a host root) until the last clone is
  dropped. The runtime tells `platypus_api` how to root, make a script string and take a thrown exception through
  `callback::install_bridge`; `platypus_ts_rt` installs it with its heap. A string argument is a new rooted script string for the call.
  An exception thrown by a callback is swallowed and listed by `callback::take_errors()`.
- **Differences.** The TypeScript `on(..)` returned an unsubscribe function; here `onX(f)` returns a token (a number, never 0) and a
  matching `off(token)` removes it: `input.off`, `Assets.offChanged`, `Application.offFocusChanged`, `SceneManager.offSceneEvent`.
  `input.onKeyDown` gets `(code, repeat)` (no layout `key`); gamepad events get the id as a number. `Assets.onChanged` gets the paths as
  one string, one per line. Scene events get the `Scene` handle and the mode as `"single"` / `"additive"`. Listeners are called at once
  (`SceneManager` after the load or unload finished); `take_events()` still works.
## Handles and the engine namespaces

A handle (an engine id such as a GameObject, a component, a texture) is a `u64` on the compiled side (`Handle::to_raw`); `0` is
`null` in `GameObject | null`. Plain data (numbers, strings, arrays, value classes) is encoded normally. (The QuickJS
form of a handle, an opaque `HandleBox` object with a JS class per handle, `platypus_api::qjs`, `platypus_native::install` and
`register_quickjs` were removed with QuickJS, with their tests `M9-API-100..113`, `-120`..`-125`.)

- *Arrays.* `&[String]` (or `Vec<String>`, owned) is an array of strings (manifest type `strings`, `ExtTy::StrArr` in the compiled lowering; an optional one left out is empty; `M9-API-1621`, `M9-TS-3323`); `&[f64]` an array of numbers; `&[V]` an array of value instances, objects or arrays. Typed arrays and array results are not built.
- *Namespaces.* Every `#[platypus_api]` module of the engine is a namespace the compiler links: math (value classes: `Mathf`, `Vector2/3/4`, `Vector2Int/3Int`, `Quaternion`, `Matrix4x4`, `Rect`, `RectInt`, `Ray`, `Plane`, `Bounds`, `Pose`, `Color`, `Keyframe`, `TimeSpan`, `DateTime`, `TimeSince`, `TimeUntil`); scene and objects (`Scene`, `GameObject`, `Transform`, `SceneManager`, `Prefab`, `HideFlags`, `LayerMask`); services (`Time`, `input`, `Random` / `Rng`, `Debug`, `Log`, `Console`, `PlayerPrefs`, `Assets`, `Application`, `SystemInfo`, `Cursor`, `Screen`, `QualitySettings`, `Gizmos`, `Editor`, `EditorScene` (snapshots and scene files for the editor; `platypus_scenes`, see [editor.md](editor.md))); animation (`AnimationCurve`, `AnimationClip`, `Animation`, `Animator`, `AnimationController` and its builders); audio (`audio`, `AudioSource`, `AudioListener`); physics (`Physics`, `Rigidbody`, colliders, joints, `Rope`, `Physics2D` and its 2D classes); rendering data (`Camera`, `Light`, `MeshRenderer`, `SkinnedMeshRenderer`, `SpriteRenderer`, `LineRenderer`, `LineRenderer2D`, `LODGroup`, `Mirror`, `ReflectionProbe`, `PlaneRenderer`, `WorldPanel`, `ParticleSystem`, `Cloth`, `Material`, `Sprite`, `Texture`, `Mesh`, `Model`, `Primitives`, `RenderSettings`). The compiler gets their manifests from the engine crates (`platypus_ts` `engine_manifests`).

**Callbacks.** A `Callback<(..)>` parameter (listener APIs: `input.onKeyDown(f)`, `Assets.onChanged`, `SceneManager.onSceneLoaded`,
`Screen.onChange`, `ParticleSystem.onStopped`, `Playback.onEnded`, ...) takes a script closure, rooted as described under "Callbacks" above; the listener
APIs return the numeric token and `off(token)` removes it. An exception in a callback is not thrown into the engine: it is recorded for
`platypus_api::callback::take_errors()` and the other callbacks still run. Reset the engine state that holds listeners (`input::reset`,
`assets::reset`, `app::reset`, `screen::reset`, `audio::reset`, `platypus_render_api::reset`, `platypus_scenes::reset`) before replacing the
program (a hot reload does).

**Optional parameters** of value classes are NaN-filled when left out, optional arrays are empty, optional handles `null`. Methods of one
class with the same script name (`#[js_name]`) are chosen by argument types.

- **Engine facade (2026-10-20):** the compiler maps the TypeScript object shapes onto the per-field API (`RenderSettings.skybox = { .. }`, `GameObject` option keys, `Camera.rect` / `layerCullDistances` / `main`, `Screen.windowSize` / `resolutions`, `QualitySettings.names`, `Sprite.grid`, `LODGroup.setLODs`, Cloth / SkinnedMeshRenderer array options, `new Material(color, emission, smoothness)`, the `AnimatorController` builder); see `tools/platypus_ts/TODO.md` and `tools/platypus_ts/src/lower/facade.rs`. `Camera.main()` is a native static (`CameraRef::main`).
### Native games in `platypus_app` (compiled in process)

`platypus_app` runs one kind of game: a folder with `build/game.pbundle` (or a `.pbundle` / `.ts` / `.tsx` path).
[native.rs](../crates/platypus_app/src/native.rs) JIT-compiles the bundled TypeScript with `platypus_ts::jit` and drives it. A QuickJS
bundle (`build/game.js`) is refused.

- **Bundle** (written by `platypus-build`): JSON `{"format":1,"title","entry","source":"<one TS/TSX text>","css":"game.css"|null,
  "assets":[..]}`. `Bundle::read` / `Bundle::parse`. The stylesheet is still `build/game.css`, loaded by `load_game_css`.
- **Compile.** Manifests: each engine crate's `manifests()` (all engine namespaces, which also registers their shim addresses in
  `platypus_api::symbol`) plus `platypus_ts_ui::manifest_json()` and `root_manifest_json()`; extra symbols: `platypus_ts_ui::symbols()`; the
  runtime's come from `platypus_ts_rt::symbols()` (a build script lists its `#[no_mangle]` exports). Each reload gets a new
  `struct_base` (100,000 apart).
- **Per frame and event** (`Native`): `fixed_tick` (`NativeHost::fixed_update`, `platypus_physics_api::step`, `dispatch_physics`),
  `frame` (`Time` via `platypus_engine_api::time::advance`, `ts_rt_timers_advance(now_ms)`, microtasks, `NativeHost::update`),
  `pre_render`, `ui_events` (a `UiEvent` becomes a `platypus_ui_native::Event` for `Runtime::dispatch`), and after each of them the
  mounts (`platypus_ts_ui::take_mounts`, `UiRoot.mount(target, element)`), `Runtime::flush`, script faults and UI errors to the log.
  Every call runs inside `platypus_scene::api::enter(&mut app.scene.scene)`, including `Native::settle` (and the world panels'
  and windows' `panel_settle` / `window_settle`), which renders the components a UI event's handlers changed and runs their
  effects: a `useState` initializer there may make a `GameObject` or read `GameObject.roots` (`M9-API-1518`).
- **Console.** `console.log` goes to the `script` log target through `ts_rt_set_console`.
- **Host wiring** (`native.rs`, `native_pump.rs`, `game_input.rs`):
  - *Input.* Keys, mouse, wheel and gamepad events go to `platypus_engine_api::input` (`game_input::apply_native`, the same argument lists as `__platypus_input`), so `getKey*` and the `onKeyDown` closures work. UI events run through the compiled handlers *inside* the interaction layer's dispatch (`Native::ui_event`, with `Runtime::dispatch_handlers`; world panels go through `native_render::panel_event` the same way), so `preventDefault()` reaches the default actions; `Native::settle` commits afterwards. `platypus_ts_ui::take_node_commands()` (`ref.focus()`, `blur()`, `scrollTo()`, `scrollBy()`, `scrollIntoView()`) is applied to the UI by `App::apply_node_commands`.
  - *Physics.* `fixed_tick` calls `platypus_physics_api::step()` (it writes body poses into the scene) and `dispatch_physics`.
  - *Audio.* `AudioCommand`s go to the mixer through `audio_op` (`Load` reads the file, the others `to_op()`); the mixer's finished sounds go back as `audio::ended` (`onEnded`, `AudioSource` state); `AudioSource` / `AudioListener` follow their objects in `pre_render`.
  - *Window and settings.* The window, `Screen`, quality levels and cursor are `native_render.rs`. `Application` requests (target frame rate, run in background, quit) and focus / pause (`onApplicationFocus` / `Pause`, `Application.onFocusChanged`) are applied. `PlayerPrefs` start from the saved prefs and their changes are written back. The manifest's `Application` info, layer names, quality levels and asset GUIDs are set by `native_pump::configure_engine`, before the program's top level, again after a reload.
  - *Scenes and assets.* `SceneManager.loadSceneAsync` requests are read from the game folder (`Native::set_root`), scene events are dispatched after every call into the program, `platypus.startScene` loads at start, `Assets.onChanged` runs when the asset watcher reloads files. `register(.., { spawn: false })` is read from the source text (the compiler erases `register`).
  - *Exit.* `App::drop` clears the panel reconcilers (`native_render::clear_state`) and runs `Native::shutdown` (drops the program's closures and resets the engine state) while the program's heap is alive; thread-local destructors run in no fixed order and used to panic at exit.
  - *Still missing:* `onReload`, component references between components across a reload, model completion callbacks.

**Hot reload of a native game** (`Native::reload`, triggered by a change of `build/game.pbundle` or `build/.platypus-reload`,
`PLATYPUS_RELOAD_AT_FRAME=N` in screenshot runs; about 30 ms for hello): the new source is compiled first (a failure, or a
`{"ok":false,"error"}` note from `platypus-build dev`, keeps the old game). Then the scenes are saved with `platypus_scenes::serialize`
(compiled components through their `@property` tables), the old `NativeHost` is dropped without `onDestroy`, the engine's per-thread
state is reset (`input`, `assets`, `app`, `screen`, `audio`, `animation`, `camera`, `platypus_render_api`, `platypus_physics_api`,
`platypus_scenes`) because it holds the old code's closures, the new program's top level runs, the objects it made are removed and the
saved scenes load again (`load_scene`: components come back on their objects with their fields, `onAwake` runs; classes new in this
program get a starting object). The UI keeps its hook state through `Runtime::refresh`, except components whose `__uiSignatures`
hook list changed. Differences to QuickJS: no `onReload` hook (the compiled runtime has no such hook), non-`@property` fields start over, module variables of objects the
top level made hold removed objects, component references between components are not restored.

**3D from a native game** (`crates/platypus_app/src/render_sync.rs`): each frame `sync` takes the resource commands
(`platypus_render_api::take_commands`: textures from files or pixels through `WorldViews::texture_op`, then `texture_loaded`; script meshes
through the `mesh` op), then sends the components that changed to the renderer, by calling `WorldViews::apply` with the same arguments
the QuickJS `render.*` ops carry, or for the many-item kinds typed calls (`set_renderer` for mesh and skinned renderers with their
materials, `set_stroke` for `LineRenderer2D`), so there is one renderer path. Native objects get script ids with `ScriptScene::adopt`.
Cameras look along -z (as in the QuickJS path, unlike Unity).

The sync is incremental. Every registry counts its changes: the generated components and assets of `platypus_render_api`
(`<name>s_version()`, bumped by `attach_*`, `create_*`, `remove_*`, `reset_*` and `with_*`, the mutable access every setter uses;
getters, `<name>s_on` and the scene-file save read through `read_*` and do not count) and the cameras (`camera::version()`; the exposure
the renderer reads back does not count). Per kind, `components()` keeps a `Track`: the versions it last synced at (a kind whose
registries, and for renderers the materials and the upload counter, did not change is not walked at all) and the key last sent per id (the
registry data, with the material or sprite asset). A walked kind iterates the registry by reference (`for_each_*`, nothing copied),
sends a component only when its data differs, and drops the ids it no longer saw (`WorldViews::remove_component`); a component that
cannot be sent (no mesh yet, no material) is dropped and retried on the next change. Transforms, layers and active flags are read when
the views are built, so moving objects costs the sync nothing. Line renderer tubes are rebuilt only when their width, sides, smoothing
or points change (`pre_render_lines` skips them all while the registry's version stays), world panel and `TextMesh` specs go to the app
only when their registry changed (`Extras::panels` / `texts` are `None` otherwise), and the second resource pass runs only when
`pre_render_lines` queued something. `PLATYPUS_RENDER_SYNC=full` rebuilds every component every frame instead (the old way, for comparing);
`M9-API-1700` checks the incremental views against a full rebuild every frame of a game making random changes. `--bench` reports the
sync's parts (`syncParts`, `platypus_app/src/sync_profile.rs`).

Round 2 (render side): `sync` also turns sprite renderers, `LineRenderer2D`, planes, mirrors, reflection probes (sent when new or
changed; `probeRender` on a new render request), LOD groups, skinned mesh renderers (ids from `1 << 24`; bind poses from the mesh or from
the model's skins), line renderers (`pre_render_lines` makes their tube meshes, which are plain mesh renderers), particle systems
(settings as the TypeScript's `[key, value, ...]` pairs when they change, then `take_particle_controls`; `world_view.update_particles`
results go back through `native_render::report_particles` into `report_particles`, so `onStopped` and `stopAction` work), models
(`RenderCommand::Model` -> `WorldViews::model_op` -> `model_loaded`; model meshes and textures keep the host's ids from `1 << 30`, which
`mesh_arg` passes straight through), render textures (`renderTexture` op), gizmos (`take_frame`; applied on a new frame of them or
after a camera changed),
`RenderSettings` (ambient, sky, bloom, post) and `QualitySettings` (quality, shadow settings) whenever their version counters change.
Mesh renderer ids are the registry handles, so `LODGroup` renderer lists match. A cloth starts when the host first sees it with a mesh
renderer on its object (`rebuild()`), and its `cloth.*` ops go to `ClothSystem` through `Extras`.

The app side is `crates/platypus_app/src/native_render.rs` (`impl App`, called from `frame()` through `sync_native_render`):
`CameraFollow` / `CameraOrbit` late updates and `platypus_engine_api::animation::update_culled` (animation culling:
`culled_roots()` lists the objects whose `Animator.cullingMode` / `Animation.cullingType` culls, `WorldViews::animation_sight`
(`anim_cull.rs`) answers from what the renderer saw of their renderers last frame (`WorldRenderer::take_seen`, read after each
frame is drawn) and watches them for this frame; an animated object with a physics component, `AudioSource`, `ParticleSystem`,
`Light` or script component below it is never culled (the safeguard, `anim_cull::blockers`, one warning per object); animators
are posed on several threads (`platypus_animation::set_threads`, from `PLATYPUS_ANIM_THREADS` or the machine's threads up to 16);
`Animator.update(dt)` poses one by hand and `Animator.enabled = false` takes it out of the frame's update; see
`docs/animation-native.md`), the quality levels from `package.json`
(`configure_quality` and `apply_default_quality`, once), `UiResources` requests (`addStyleSheet`, `removeStyleSheet`, `loadFont` through
the same code as the QuickJS ops, on the screen and every panel), `Cursor` (version counter) and `Screen` (window requests, the display
state and size changes), and world panels: `Native::take_panel_mounts` hands over `createRoot(panel).render(..)` roots; each panel has its
own `UiTree` (`panels.rs`) and its own `platypus_ui_native::Runtime` (flushed every frame); pointer and key events of a panel
(`App::native_panel_events`) go to that runtime. `WorldPanel` components give the panel's size, density and tint through `Panels::apply`.
Not done: animation events are not delivered to components;
colliders of models (`colliders: "mesh"`) need the physics hook (agent H).

## The FPS counter (`crates/platypus_app/src/fps.rs`)

A raw frame-rate overlay drawn by the app for every game: lime green
(`#32ff32`), bold, 13px, `0 1px 2px #000` shadow, no background, in the top right corner 10px from the top and right edges,
right-aligned, text like `144 FPS`. `FpsCounter` counts frames over windows of 0.25 s of real time and shows `round(frames / elapsed)`
(no smoothing, no cap). `FpsOverlay` is a tiny `UiTree` (one absolutely placed box, `pointer-events: none`, so no hit testing or focus)
drawn with `render_overlay` after the world and the game's UI, in `frame()` and in `save_screenshot`, at window resolution and window
scale factor (not the UI scale, not the render scale).

On or off, in order: `--fps` / `--no-fps` on the command line; off with `PLATYPUS_FPS=0` (or `false`, `off`, `no`) or `"platypus": { "fps": false }`
in `package.json`; otherwise on, except in `--screenshot` runs (off unless `--fps`, so goldens and the `screenshots` tool's renders don't change).
The editor's `project.json` turns it off (`"fps": false`); `--fps` still shows it. A short screenshot run shows the partial window.

## Fog, light shafts and exposure in native games (2026-10-21)

The render features of `js/engine` that `docs/world-rendering.md` describes (fog, height fog, light shafts, auto exposure, light
units, soft shadow levels, fog volumes) are declared natively too, and `render_sync.rs` sends the renderer the same `render.fog`,
`render.post`, `render.light`, `render.fogVolume`, `render.exposureReset` and `render.quality` ops, so the pictures match.

- **`RenderSettings`** (`platypus_render_api::render_settings`): `fog`, `fogMode`, `fogColor`, `fogDensity`, `fogStartDistance`,
  `fogEndDistance`, `fogCulling`; `heightFogEnabled` + `heightFogDensity` / `heightFogBaseHeight` / `heightFogFalloff`;
  `volumetricFogEnabled` + `volumetricFogIntensity` / `Anisotropy` / `Steps` / `Distance` / `Albedo` / `SharpDistance` / `SharpSamples` /
  `Resolution` / `Denoising` / `HistoryWeight` / `Samples` / `MotionSamples`; `autoExposureEnabled` + `autoExposureMinEv` / `MaxEv` /
  `Compensation` / `SpeedDarkToLight` / `SpeedLightToDark` / `Metering` / `LowPercent` / `HighPercent` / `DarkAdaptation` / `DarkBelow` /
  `Hold` + `clearAutoExposureCurve()` / `addAutoExposureCurvePoint(ev, stops)`; `fixedExposure` (`number | null`, an `Option<f64>`),
  `exposureTransition`, `freezeExposure()`. Setting a field of the height fog, light shafts or auto exposure turns it on with the
  defaults for the rest, as the sky does.
- **`cloudLayer`** (`CloudLayerSettings`, every key optional: `altitude`, `size`, `coverage`, `opacity`, `softness`, `tint`,
  `windSpeed`, `windDirection`, `fadeDistance`): HDRP's cloud layer, drawn with the background behind the distance fog. The sync sends
  it as the `clouds` op; `WorldViews::advance_clouds` carries it on the wind each frame (`M7-CLOUD-04`).
- **Object-valued properties** are native records: `skybox`, `bloom`, `heightFog`, `volumetricFog`, `cloudLayer` and `autoExposure` take an object or
  `null` and read back as one (`Option<Record>`, JSON on the wire, see "Records as parameters" below); `fixedExposure` is native
  (`number | null`). `FogMode`, `VolumeDetail`, `VolumeDenoising` and `ExposureMetering` are enums (`#[platypus_api]` `enums(..)`, string
  unions in the declarations). The per-field natives (`heightFogDensity`, `skyZenith`, ...) are still there.
- **`Camera`**: `fog` (show the fog, on), `exposure` / `sceneBrightness` (EV100, read back from the GPU a frame or two late, NaN
  before: the app calls `camera::set_exposure`), `resetExposure()`, `aperture` / `shutterSpeed` / `iso` and `physicalExposure`.
- **`Light`**: `volumetricIntensity`, `spotShape` (`cone` | `pyramid`), `aspectRatio`; `Light.illuminanceAt(point, normal?)` and
  `Light.measureIlluminance(point, normal?)` (native, `crates/platypus_render_api/src/light_meter.rs`: the sum over every light, the
  shadowed form casting a physics ray to each, so `platypus_render_api` depends on `platypus_physics_api`; `measureIlluminance` is a
  `#[promise]` that resolves after the next physics step, through `Physics.afterNextStep`, as the TypeScript tier does;
  `M9-API-1645`). **`FogVolume`** is a component (`size`, `density`, `color`,
  `blendDistance`, `shape`).
- **`QualitySettings`** also has `autoExposure`, `volumetricFog`, `hdr`, `additionalLights`, `additionalLightShadows` (per level in
  `project.json`'s `quality.levels`; Low has no light shafts) and `softShadowQuality` `medium`. `RenderStats` (meshes, culled
  meshes, quads, ..., `fogCulled`, `lights`, `shadowMaps`, `shadowCasters`) is filled by the app after each frame; `GraphicsSettings.lightUnits` is
  `physical` when `project.json` says `"lightUnits": "physical"`.
- **Meshes**: `Mesh.createFlat(data)` / `mesh.setFlat(data)` (positions, triangles, normals, uv: typed arrays are copied to
  `number[]` at the boundary; a list left out is empty) over `Mesh.createFlatArrays(..)`. Script meshes
  now flip the texture v coordinate on upload (Unity's bottom-up, as `js/engine`'s `Mesh.set` does). `Random.pick(list)`.
- **Compiler**: an optional object-typed option (`new GameObject(name, { rotation })` with `rotation?: Quaternion`) is skipped when
  missing; `x === false` on an optional boolean is false for a missing value; a typed array is copied where the engine takes
  `number[]`.
- **Scene files** now save and apply the fog keys (`fog`, `fogMode`, `fogColor`, `fogDensity`, `fogStartDistance`,
  `fogEndDistance`, `heightFog`, `cloudLayer`, `volumetricFog`, `fogCulling`); `scenes.ts` saves no exposure keys, so neither do native scenes.

## 2D shapes, text in the world and splines in native games (2026-10-22)

`js/engine`'s `LineRenderer2D` fills, `TextMesh`, `Shapes`, `Spline` / `SplineAnimate` and `Camera.screenToWorldPoint` work in compiled
games too. The renderer-side parts send the same ops as the QuickJS classes (`render.stroke`, `render.text`), so the pictures match.

- **`LineRenderer2D`** (generated, `gen/components.rs`): besides `points`, `width`, `color`, `loop` it has `fillRule`
  (`evenodd` | `nonzero`), `sortingOrder` and `fillColor`. `fillColor` is a native `Option<Rgba>` (`Color | null`; the generator's `color?` field type): a `Color` fills the (closed) shape, `null` takes the fill away. Scene files
  save `fillColor` as a colour or `null`. `render_sync.rs` sends `stroke id object width color loop points fill rule order`; the
  renderer keeps a shape's triangles while its points and rule are unchanged (a native game sends every shape every frame).
- **`TextMesh`** (generated): `text`, `fontSize`, `color`, `font` (`""`: Inter), `fontWeight`, `alignment`, `maxWidth`,
  `pixelsPerUnit`, `resolution`. The host gives each one a panel (`panels.rs::apply_text`, ids from `render_sync::TEXT_BASE`) that
  goes with its component (`native_render::sync_texts`).
- **`Shapes`** (`rectangle`, `roundedRectangle`, `ellipse`, `circle`, `arc`, `pie`, `regularPolygon`, `polygon`, `triangle`,
  `bezier`, `curve`, `closedCurve`) is native Rust now (`platypus_math::outlines`, see "Enums, records, lists, rest parameters and
  docs"). **`Spline`** (knots, tangent modes, `evaluate` / `tangent` by distance, `segmentPoint`,
  `sample`, `nearest`, `version`) and **`SplineAnimate`** are native Rust too (see "Enums, records, lists, rest parameters and
  docs"); the TypeScript copies in `facade.ts` are gone.
- **`Camera.worldToScreenPoint(p)`, `screenPointToRay(p)` and `pixelRect`** (UI pixels, over the camera's `rect`) are the other
  way and the camera's box on screen; the editor's Scene view uses them for icons and handles.
- **`Editor`** (`editor.rs`, `editor/open.rs`, `editor/project.rs`) is what the editor game asks of the host: `logSince(seq)`,
  `openFile(path, line, col)`, `listDir`, `copyIn`, `move` (each file call answers at once, `lastError` says why one failed),
  the properties `project` and `isAvailable`. It works only after `platypus-app editor --project <folder>`; see [editor.md](editor.md).
- **`Camera.screenToWorldPoint(p)`** is a facade method (frustum maths over `rect`, `Screen.width` / `height`, `fieldOfView` or
  `orthographicSize` and the transform). The other `viewportTo...` / `screenPointToRay` members are not there yet.
- **Facade chunk requirements.** A chunk header may end with `@ Name ..`: `//# when Spline @ Vector3`. The chunk is added only
  when the engine API has every name (class, value class, handle or namespace) and the program names a trigger, so a comment that
  mentions `LineRenderer2D` no longer pulls a chunk written over it into programs without that class.
- **Compiler changes this needed** (`tools/platypus_ts`): interface and type-literal *method signatures* are function-valued
  properties (`at(t: number): Move;`, optional ones too); named tuple members (`[t: number, run: () => void]`); a method shorthand
  in an object literal (`{ at(t) { .. } }`) is a property whose closure takes its parameter types from the declared
  field; closures inside closures in a **top-level initialiser** now capture the outer closure's locals; errors in top-level
  declarations and object types carry their line; the JSX rewrite keeps the lines of what follows each multi-line element, so
  later error lines are right; `clipPath` is an attribute of `PlatypusProps` (inline SVG `<g clipPath="url(#id)">`).
- **Still not there**: `Partial<Class>` (the examples use a small options interface and set the fields), a string-literal union
  local assigned from an optional field (compiler panic: `Spline` keeps `mode` a `string`), `Array.from({ length }, f)`,
  `.flatMap`, `.flat()`, `Record<string, V>` as a dictionary (use `Map`), `Map.get` over engine handles (box them), object
  accessors (`get x() {}`) in object literals, destructuring assignment, spread call arguments, classes assigned to interfaces
  that have function properties (use an abstract base class), `String(n)[i]` indexing (use `charAt`).

## Enums, records, lists, rest parameters and docs (2026-10-22, generated declarations P0/P1)

Part of making every declaration generated from Rust (plan: [platypus-generated-dts-plan.md](research/platypus-generated-dts-plan.md)):
`#[platypus_api]` and the manifest got what `Shapes`, `Spline` and `SplineAnimate` needed, so their TypeScript copies
(`facade.ts`, `engine_ts.d.ts`) could go. Tests: [records_enums.rs](../crates/platypus_api/tests/records_enums.rs)
(`M9-API-1610`..`1614`).

| Rust | Script | C ABI |
|---|---|---|
| `enums(TangentMode = Tangent [Auto, Bezier, Linear])` on the attribute, a Rust enum `Tangent` | the string union `"auto" \| "bezier" \| "linear"` (variant names with a lower-case first letter), plus the global `TangentMode.name(i)` / `TangentMode.parse(s)` | the index, `i32` (-1: not a name; the engine reads it as the first value) |
| `records(SplineHit = SplineHit [segment: f64, u: f64])`, a Rust struct with those public fields (`f64`, `bool`, `string` (a `String`), a declared value class or enum) | a plain object `{ segment, u }` (an `interface` in the d.ts) | a result only: the numbers go to `platypus_api`'s per-thread buffer (`return_flat`) and the strings to its string list (`return_strings`, in field order, element after element for a list), the compiled call reads them with `platypus_api_result(i)` / `platypus_api_result_str(i)` and builds the object |
| `-> Vec<V>` / `-> Vec<Record>` | `V[]` / `Record[]` | the shim returns the element count; the numbers of every element are in the same buffer |
| `-> Vec<String>` | `string[]` | the shim returns the count and keeps the strings in `platypus_api`'s per-thread list (`return_strings`); the compiled call copies them out with `platypus_api_result_str(i)`, one new script string each (manifest `list:string`; QuickJS gets an array of strings; `Assets.list`, `M9-API-1620`) |
| `#[rest(points)]` on a function whose last parameter is `&[V]` | `...points: V[]` (QuickJS: every remaining argument; compiled: the arguments packed into an array, or one `...array`) | the array pointer, as for `&[V]` |
| `#[options(after = points)]` with `#[optional(points, closed = false, ..)]` | `new Spline(points?, { closed?, tension? })`: the object after `points` gives the parameters after it | the positional shim |
| `like` on the attribute | value *parameters* are declared `Vector3Like` (a plain `{ x, y, z }` does) | - |
| `#[nullable]` on a handle getter and its setter | `T \| null` in the declaration | a zero handle is `null` as before |
| `#[nullable(undefined)]` (on a handle result, a static property such as `Camera.main`) | `T \| undefined` | the same zero handle (manifest `undefined`) |
| `Option<f64>`, `Option<String>` (a result only), `Option<V>` for a value class | `number \| null`, `string \| null`, `Color \| null` (manifest `opt:f64`, `opt:string`, `opt:value:Color`) | the inner type with a none mark: NaN for a number, NaN in every field for a value (also as a parameter, so a setter takes `null`), and for a string an empty text plus the flag `platypus_api_result_none()` the compiled call reads right after (`Window.openError`, `RenderSettings.fixedExposure`, `LineRenderer2D.fillColor`); QuickJS gets `null` |
| `enums(extern PrimitiveType = Primitive [..])` | an enum another namespace declares (no second set of converters, no second declaration); the module imports the declaring namespace's `primitive_type_*` helpers | the same index |
| `enums(FullScreenMode = Mode [ExclusiveFullScreen = "ExclusiveFullScreen", ..])` | a variant may name its string when it is not the variant with a lower-case first letter | the index |
| a parameter `Option<Enum>` | the enum, usually optional; a left-out or unknown name is `None` (compiled calls pass -1; QuickJS throws a `TypeError` for an unknown name and takes a left-out one as `None`): setters use it to ignore a name that is not one (`RenderSettings.fogMode = "nonsense"`), constructors to tell "left out" (the `.meta` importer's value) from the first value (`Texture.load(.., { filterMode })`) | the index (-1: none) |
| `-> Vec<Enum>` | `Enum[]` (manifest `list:enum:X`) | the count, and the indexes in the result buffer (`Screen.supportedFullScreenModes`) |
| `#[options(name = "TextureOptions", only)]` with `#[optional(..)]` and `/// key: text` lines in the doc | the options object after the leading required parameters has a declared type: `options?: TextureOptions` and `interface TextureOptions { /** text */ key?: T; .. }` (a handle key also takes `null`; one name over several functions merges their keys); `only` leaves out the positional form of the declaration (the compiler still takes it); QuickJS reads the object by key as for a bare `#[options]` (manifest `optionsName`, `optionsOnly`, parameter `doc`) | the positional shim |
| `&[V]`, `&[f64]`, `&[String]` parameters | declared `readonly V[]` (the engine only reads them; a `readonly` array of a game passes) | the array pointer |
| `aliases(AnimatorController = AnimationController)` | `type AnimatorController = AnimationController; declare const AnimatorController: typeof AnimationController;` for a class, only the `type` for an enum (`PrimitiveMesh = PrimitiveType`); the compiler renames the alias to its target in the source, QuickJS sets a second global | - |
| `#[ts_type = "\"low\" \| \"high\""]` on a `String` getter (and setter) | what the property reads as is that union; a class property is an accessor pair (`get x(): "low" \| "high"; set x(v: string)`: any string may still be written, an unknown name is ignored), a namespace property the union alone | - (manifest JSON `tsType`) |
| `///` on the module, an `impl` block (the class), a function, and inside `enums(..)` / `records(..)` (also on record fields) | JSDoc in the generated `.d.ts` (manifest JSON `doc`) | - |

**Enums on the wire.** A compiled call converts a string constant to its index at compile time and anything else with
`Enum.parse(s)`; a result goes through `Enum.name(i)`. The type name of an enum or record is known to the compiler, so
`let m: TangentMode = "auto"` works (the compiler sees a `string`; `tsc` sees the union). The native helpers for Rust code are
generated next to the shims (`tangent_mode_name`, `_parse`, `_index`, `_from_index`, `TANGENT_MODE_NAMES`). QuickJS passes and
returns the strings and throws a `TypeError` for an unknown name (a left-out optional enum is the first value in both).

**Enums of the engine API (2026-10-22, plan phase 2a).** `FogMode`, `VolumeDetail` (`RenderSettings`), `PrimitiveType` (with the
alias `PrimitiveMesh`; `PrimitiveType.Cube` is the enum value `"cube"`), `FilterMode`, `TextureWrapMode` (`Texture`), `FullScreenMode`
(`Screen`), `ShadowQualityMode` (`QualitySettings`), `WindowStyle` (`Window`) and `LoadSceneMode` (`SceneManager`) are
`#[platypus_api]` enums: the string unions in the declarations are generated, `tsc` and the compiler's `Record<FogMode, V>` see the same
names, and a setter ignores a name that is not one. Differences from before: the natives
`Screen.supportedFullScreenModeCount` / `...At(i)`, `RenderSettings.fixedExposureEnabled` / `...Value`, `LineRenderer2D.hasFill` /
`fillValue` and `Window.openErrorText` are gone (the nullable property replaces each pair), `Camera.main` is a property of the
native `Camera` class in QuickJS too (it was a function there), and `Window.create({ style: "floating" })` throws a `TypeError` in
QuickJS (compiled code and the old string API fell back to `normal` with a warning). Tests: `crates/platypus_api/tests/enums_nullables.rs`
(`M9-API-1622`..`1626`, `1630`..`1632`), `crates/platypus_native/tests/enums.rs` (`M9-API-1627`..`1629`, `1633`), `tools/platypus_ts_tests` `M9-TS-3340`, `3341`, `3343`, `tools/platypus_ts` `M9-TS-3342`.

**Records as parameters (2026-10-22, plan phase 2b, second part).** A record declared in `records(..)` may have optional keys and number
lists, and `Option<R>` of a record is a parameter or a result (`opt:record:R` in the manifest). On the wire it is **JSON text**: the
shim parses the text with the record's generated `__from_json_R` (a missing required key, a wrong type or an unknown enum name is
reported on stderr and the argument is `None`) and a result is `__to_json_R`; QuickJS stringifies the object it is given (and
builds a real object with value-class instances for a result). The compiled side makes the text from the argument's static type
(`JSON.stringify` of the record's struct, so a game's own structurally matching `interface`, a variable or a spread work); an object
**literal** (also `null` and `c ? { .. } : null`) is written key by key instead, so a key that is written as `null`
(`RenderSettings.skybox = { sunDirection: null }`) differs from one that is left out (a compiled `T | null` field cannot tell
`undefined` from `null`). Record field types: `f64`, `bool`, `string`, a value class (`<Value>Like` when a plain `{ x, y, z }` does),
an enum, `pairs` (`[number, number][]`) and `Option<..>` of those (`Option<Option<T>>`: the key may also be `null`);
`name: T as "minEV"` names a key that is not the camelCase of the field. The declaration is `interface R { key?: T; .. }`. Records that
only have flat fields can still be plain results (the buffer form above); a record with an optional key or a list is JSON only.

**Records and option bags (2026-10-22, plan phase 2b, first part).** `Screen.windowSize`, `currentResolution` and `resolutions` are native records (`WindowSize`,
`Resolution`, `DisplayMode` in Rust), `Animator.getCurrentAnimatorStateInfo()` returns the record `AnimatorStateInfo` (a record with a string field),
`TextureOptions` and `WindowOptions` are generated from `Texture.load` / `create` and `Window.create` (`#[options(name = ..)]`). Natives that only
fed the facade are gone: `Screen.resolutionCount` / `resolutionWidth(i)` / `resolutionHeight(i)` / `resolutionRefreshRate(i)` and
`currentResolutionWidth` / `Height` / `RefreshRate`.

**Lists (2026-10-22, plan phase 2c, first part).** `Vec<f64>` and `Vec<Handle>` are results (`list:f64`, `list:handle:X`: the count, then the numbers in the
result buffer or the 64-bit ids behind `platypus_api_result_id(i)`), `&[Handle]` is a parameter (`handles:X`, read with `script_handles`: the
script array of ids). A getter that returns a list pairs with the setter that takes the array (`pair_ty` in the macro), so it is one
property (`SkinnedMeshRenderer.bones`, `Cloth.pins` / `sphereColliders` / `capsuleColliders`, `Camera.layerCullDistances`,
`LineRenderer.positions`, `LineRenderer2D.points`, `EdgeCollider2D.points`, `PolygonCollider2D.points`, `QualitySettings.names`);
`Scene.getRootGameObjects()` and `Sprite.grid(..)` return handle lists. `#[like]` on a function declares its value parameters
`<Value>Like` (a plain `{ x, y }` object does) without making the whole namespace `like`. A compiled call that passes `undefined` for
an optional number, value, handle or array parameter leaves it out. Tests: `M9-API-1634`, `M9-API-1635`, `M9-TS-3350`.

**Subscriptions (2026-10-22, plan phase 2d, first part).** A function that takes one `Callback` and returns its `u32` token can say
`#[subscription(off = "off_change")]` (optionally `ts = "(scene: Scene, mode: LoadSceneMode) => void"` for a callback type the
wire types cannot spell). The C symbol and the manifest keep the token (`"subscription": { "off": "offChange", "ts": ".." }`); scripts
get a function that calls `off_change(token)`: QuickJS builds it in the binding, compiled code gets a small function written from the
manifest by `facade::extend_source` (`__xt_Screen_onChange` over `Screen.onChange$n` and `Screen.offChange`), and the declaration
returns `() => void`. `Screen.onChange`, `QualitySettings.onLevelChanged`, `Assets.onChanged`, `Application.onFocusChanged` and
`SceneManager.onSceneLoaded` / `onSceneUnloaded` / `onActiveSceneChanged` use it; the token forms (`input.onKeyDown`, window
listeners) are unchanged. Tests: `M9-API-1636`, `M9-TS-3351`.

**Promises (2026-10-22, plan phase 2d, second part).** `#[promise]` marks a function whose last two parameters are `resolve: Callback<(T,)>` and
`reject: Callback<(String,)>` (nothing else is returned): scripts call it without them and get a `Promise<T>`, `T` the resolve callback's
argument (`"promise": true` in the manifest; the declaration drops the two parameters). QuickJS makes a real promise and passes the two
functions of `Promise::new` as the callbacks (the reject text becomes an `Error`: `qjs::error_rejecter`); compiled code gets a small
function written from the manifest (`new Promise<T>((resolve, reject) => X.f$n(.., resolve, e => reject(new Error(e))))`). Optional
parameters the generated function passes on become `""` / `NaN` / `false` when left out. Used by `SceneManager.loadSceneAsync`,
`Prefab.load` (listeners the engine adds and removes itself, in `platypus_scenes::manager`) and `Texture.whenLoaded()` / `Model.whenLoaded()`
(a waiter list settled by `texture_loaded` / `model_loaded`, so no polling timer). Tests: `M9-API-1637`, `M9-API-1638`, and the
existing `M9-API-1512`..`1517` and `M9-TS` promise tests.

**Lists of records and handles in records (2026-10-22, plan phase 2b/2c).** A record field may be a handle (`MeshRenderer`) or `Vec<Handle>`;
in JSON a handle is its id (a descriptor tag `9` in the compiled JSON blobs, set only when a call of the engine API stringifies or
parses a record: a game's own `JSON.stringify(handle)` is still `{}`; QuickJS walks the object with `qjs::value_to_json`, where a
handle object is its id). `&[R]` parameters and `Vec<R>` results of records that cannot travel flat travel as JSON text
(`records:R`, `ExtTy::RecordsJson`); a getter that returns such a list pairs with the setter that takes it. `#[ts_param(points = "..")]`
sets the declared TypeScript type of a parameter that is wider than the Rust one (`AnimationCurve.fromPoints` takes `Vector2` values
or `[time, value]` pairs: QuickJS reads both, compiled code converts `number[][]` rows). Native now: `LODGroup.setLODs`,
`ParticleSystem.bursts` / `colorOverLifetime` / `sizeOverLifetime` / `shape`. Tests: `M9-API-1639`, `M9-TS-3352`.

**Manifest JSON** (additive): top-level `doc`, `like`, `enums: [{ name, doc, values, nameSymbol, parseSymbol }]`,
`records: [{ name, doc, fields: [{ name, type, doc }] }]`; classes `doc`; functions `doc`, `optionsAfter`, `nullable`, `tsType`;
parameters `rest`; type strings `enum:X`, `record:X`, `list:value:V`, `list:record:R`, `list:string` (a result) and `strings` (a parameter). `tools/platypus_ts/src/types_dts.rs` reads them
(`enum` becomes `type X = ..` and `namespace X { name, parse }`, `record` an `interface`).

**Guard tests** (`tools/platypus_build/tests/generated_dts.rs`): `M9-TS-3100` no hand-written `.d.ts` file under `tools/` (strict:
`engine_ts.d.ts` and `facade.ts` are gone; what the compiler itself declares is the table `tools/platypus_ts/src/builtins.rs`),
`M9-TS-3101` the built-ins declare no engine name again and every class they add members to exists, `M9-TS-3102` the generated
`types/platypus.d.ts` equals a blessed API snapshot
(`tools/platypus_build/tests/golden/api_surface.txt`; `PLATYPUS_BLESS=1 cargo test -p platypus_build --test generated_dts` rewrites it, and the
diff shows exactly which declarations changed).

**Shapes** is native: `platypus_math::outlines` (`crates/platypus_math/src/outlines.rs`, declared as the namespace `Shapes`), ported
from the TypeScript in the same order of operations. The compiled facade copy
is gone; `tools/platypus_ts_tests/bench/outlines.ts` keeps it, renamed `RefShapes`, as a differential oracle (`M9-TS-3110`: every
point of every shape on edge cases equals the native one bit for bit, in every GC mode).
**Spline** is native too: `platypus_math::spline` (`crates/platypus_math/src/spline.rs`, plain maths: knots, `closed`, `tension`, the cached
control points and length table, `locate` / `evaluate` / `tangent` / `sample` / `nearest`) and `platypus_engine_api::spline` (the
`Spline` handle class over a per-thread registry; `new Spline(points?, { closed?, tension?, mode? })` is `create` with
`#[options(after = points)]`). The d.ts comes from the manifest: `TangentMode` (an enum), `SplineKnot`, `SplineHit` and
`SplineNearest` (records), `Spline` (class). Differences from the TypeScript class: `knots` is a read-only snapshot (a fresh array of
fresh objects: edit with `setPosition` / `setTangents` / `setMode` / `insertKnot` / `removeKnot`), `version` is read-only, a handle
is not garbage collected (`Spline.destroy()` frees it), a knot index must be a whole number in range (anything else is ignored, as an
out-of-range one was) and a `NaN` `t` gives NaN points. The oracle is the old compiled TypeScript, kept renamed (`RefSpline`) in
`bench/outlines.ts`: `M9-TS-3112` builds splines of 0 to 6 knots (open and closed, three tensions, auto and linear), edits them
with every mutator and compares every number (`count`, `length`, handles, `segmentPoint`, `segmentDerivative`, `locate`,
`evaluate`, `tangent`, `sample`, `nearest`, the knots) bit for bit. (`M9-API-1603`..`1605`, the QuickJS side of `Spline`, were
not kept.)

**SplineAnimate** is a native component (`platypus_engine_api::spline`, the `SplineAnimate` handle class, kind `SplineAnimate`). Its
fields (`spline`, `container`, `duration`, `loopMode`, `alignment`, `playing`, `elapsed`) are data in the registry; `t` and `place()` are
methods; `SplineAnimate.attach(go)` / `findOn(go)` are what `go.addComponent(SplineAnimate, { duration: 2 })` and
`go.getComponent(SplineAnimate)` compile to. `loopMode` is the enum `SplineLoopMode` (`"loop" | "pingPong" | "once"`) and `alignment`
`SplineAlignment` (`"none" | "path"`). The host calls `platypus_engine_api::spline::update(dt)` once a frame next to
`animation::update` (`native_render.rs::sync_native_render`); it advances every playing component whose object is active in the
hierarchy and puts the object on the path (`place`: the point and the direction from the spline, through the container's
`transformPoint` / `transformDirection` when there is one, then `Quaternion.euler(0, 0, atan2)` on a flat path or `lookRotation`).
Components of destroyed objects go with them (`objects::process_destroyed`).

**Scene files.** The kind saves every field: `loopMode` and `alignment` as names, `container` as an object reference, `spline` as the
inline asset `{"$type":"Spline","closed":..,"tension":..,"knots":[{"position","tangentIn","tangentOut","mode"}]}` (asset kind
`Spline`, `register_kinds()`); loading makes a new spline (`M9-API-1607`). `platypus_scenes::register_engine_kinds` registers both.

The path maths is also exposed as three native statics of the same class: `SplineAnimate.pose(spline, elapsed, duration, loopMode)`
(a `{ position, direction }` record), `SplineAnimate.progress(..)` (`t`) and `SplineAnimate.facing(direction)` (the rotation); the
loop, ping-pong and alignment logic exists once, in Rust (`M9-API-1606`). (The thin QuickJS script component over them is gone.)

Differences from the TypeScript component: the update runs once a frame after the scripts' `onUpdate`s (in the old version in
component order), so a game that moves the spline in its own `onUpdate` sees the object placed in the same frame; a destroyed or
inactive object is skipped; `SplineAnimate` does not `extends Component` in the declarations (it is a handle class like `Camera`).
Tests: `M9-TS-3113` (`SplineAnimate.pose` equals the old component's `t` and direction for every loop mode, duration and elapsed time
in the oracle), `M9-TS-3115` (added with options, found again, the host update moves the object and pings back), `M9-TS-3116` (the
old `js/engine` test: positions, facing, loop modes, a container), `M9-API-1606`, `M9-API-1607`.
