# platypus_ts: the engine boundary (design note, for review)

Status: **proposal, no code written.** This is the design note for `TODO.md` M9.b2 "Direct engine
calls" (a STOP item), set by the direction recorded in `platypus-ts-classes.md` section 12: the engine is
native, TypeScript sees it through declaration files (`.d.ts`), scripts call native functions
directly, QuickJS stays an option. Questions that need a yes or no are in section 11.

## 1. What this has to achieve

1. Compiled gameplay code calls the engine (`transform.position = p`, `Physics.raycast(...)`,
   `Time.deltaTime`, `Mathf.sin`) as a plain native call: no `Value` conversion, no op queue, no
   string-named dispatch.
2. A user class can `extends Component` and be driven by the engine (`onUpdate(dt)` and the other
   hooks) with no name lookup and no JS engine.
3. The engine's API exists **once**: a native implementation, from which the declaration files, the
   compiler's knowledge of the engine, and (while it is kept) the QuickJS bindings are generated.
   Nothing is written twice.
4. The existing examples keep running while this is built, one module at a time.

## 2. What exists today (measured by reading the code, 2026-10-18)

- `js/engine/src` is 11,468 lines of TypeScript. Scripts reach Rust only through
  `__platypus_op(name, ...args)`: about 85 distinct op names in these families: `render` 19, `physics`
  15, `audio` 15, `scene` 9, `prefs` 4, `app` 3, `cursor` 3, plus `asset`, `cloth`, `screen`,
  `loadFont`, style sheets. Arguments are the generic `Value` (null, bool, number, string, array),
  queued in an `OpBuffer` and applied by name once per frame. `platypus_core/src/ops.rs` already says hot
  paths should move to a binary encoding.
- **The TypeScript keeps a mirror of the world.** `Transform` stores `_position`, `_rotation`,
  `_scale` in TS and sends `scene.local` (three arrays) on every set; Rust sends moves back in a
  batch, `__platypus_scene(moved)`, as flat numbers. Physics contacts, raycast results, input and audio
  endings come back the same way, as batches into `__platypus_physics`, `__platypus_input`, `__platypus_audio`.
- **Rust owns the component list, JS holds the instances.** `ComponentHost` creates components by
  class name (`__platypus_create`), asks which hooks a class has (`__platypus_hooks`), and calls
  `__platypus_dispatch(id, "onUpdate", dt)` by name. A component that throws is reported and disabled.
- The measured cost of crossing: in the sine-circle comparison (`platypus-ts-spike.md` section 8) the
  QuickJS frame spent about 0.12 ms of 1.26 ms in `__platypus_op` plus draining it, for one op.
- Where the 11.5k lines go, roughly: thin wrappers over ops (physics, audio, rendering component
  classes, app, prefs, screen); and **real logic that lives only in TypeScript**: `math.ts` (1,025:
  vectors, quaternions, colours, matrices), `scenes.ts` (1,044: JSON scene load, `SceneManager`,
  `instantiate`), `animation.ts` (921: curves, clips, blend trees, `Animator`), `time.ts`/`clock.ts`
  (`DateTime`, `TimeSpan`, `Time`, `Task`), `input.ts`, `random.ts`, `lines.ts`/`gizmos.ts`, and the
  hot-reload and property reflection code (`hot.ts`, `property.ts`).

The consequence: the aim "none of the engine in TypeScript" is not a binding exercise. About half of
`js/engine` is logic that has to be rewritten in Rust (or stay as library source compiled with the
game). That is the bulk of the work in this item, and it is mostly mechanical.

## 3. The shape: declarations over a native API

```text
Rust engine functions (#[ts_api])  --generates-->  engine_api.json   (the manifest)
                                                       |-- platypus.d.ts          (for tsc, editors, users)
                                                       |-- extern "C" shims   (the symbols to link)
                                                       |-- compiler extern table (platypus_ts)
                                                       |-- QuickJS bindings   (while QuickJS is kept)
```

The Rust side is the source of truth, written once, annotated with what the generators cannot guess
(f32, handle versus value, nullable, doc comment). The compiler reads the **manifest**, not the
`.d.ts`: a `.d.ts` says what TypeScript needs (`readonly x: number`), not which symbol to call or how
a `Vector3` is passed. The `.d.ts` is generated, so it cannot drift, and carries the doc comments.

This is a proposal with a real alternative (section 11, question 1): hand-write the `.d.ts` and the
Rust, and test that they agree. That is simpler to start and drifts.

## 4. Four kinds of declared type

The compiler needs to know, per declared type, how its values are represented. Four kinds cover
`js/engine`:

### 4.1 Handle classes (identity objects owned by the engine)

`GameObject`, `Transform`, `Rigidbody`, `Collider`, `AudioSource`, `Camera`, `Light`, `MeshRenderer`,
`Animator`, ... A value is the engine's **generational id**, a 64-bit integer (`ObjectId` in `platypus_scene` is an index plus a
generation already; today scripts use their own numeric ids that the engine maps to it, and a handle
would be the engine id itself, which removes that map). Proposal: a new IR type `Handle(class)`, an `i64` in a register. It is not a heap object, so it
costs no allocation and no GC root; `===` compares ids; zero is `null`/`undefined`.

Methods and accessors become direct calls: `t.position` is `ts_eng_Transform_position(h, &out)`,
`t.position = v` is `ts_eng_Transform_set_position(h, x, y, z)`. A stale handle (the object was
destroyed) must not crash: the engine checks the generation and the call does nothing / returns the
default. The arena already behaves like this: a stale id misses.

Because the engine owns the data, the TypeScript mirror goes away: there is one copy of a transform,
in Rust, and `transform.position` reads it. (The `moved` batch exists only to keep the mirror true.)

### 4.2 Value classes (immutable data)

`Vector2/3/4`, `Quaternion`, `Color`, `Matrix`, `Rect`, `Bounds`, `Ray`, `Vector3Int`. Properties of
these are `readonly`, methods return new values, `equals` exists for comparison, so identity is not
meaningful in correct code. Where the spec says "float", the engine's glam types are `f32`.

Representation (question 2, section 11):

- **Phase 1 (recommended): ordinary compiled structs.** The `.d.ts` is `declare class Vector3 { readonly
  x: number; ... }` and the compiler gives it the struct layout it already has (a header and f64
  slots). Methods are **intrinsics**: `a.add(b)` is inlined arithmetic on the fields, `a.normalized`
  an inline sequence, `Quaternion.slerp` a call to a Rust function taking scalars. Engine calls pass
  and return scalars (`set_position(h, x, y, z)`; results written through an out-pointer into a
  caller-allocated object). Allocation per vector operation is the known cost, as in section 7 of the
  classes note.
- **Phase 2: scalar replacement.** A value class that does not escape a function is replaced by its
  fields (three locals). This is the "escape analysis" item under Performance; the contract does not
  change.
- Not recommended: passing aggregates by value through `extern "C"`. Win64 and SysV disagree about
  small structs and `f32x3` is not a register type; scalarising at the boundary avoids both.

**f32 consequence.** Fields stay f64 in the script heap (the IR has one number type), and are narrowed
to f32 when they cross into the engine. So `t.position = v; t.position.x` now reads back the
f32-rounded value (`0.1` becomes `0.10000000149011612`), where today it returns exactly what was set
because the TypeScript mirror stores f64. That is what Unity does, and it is the correct reading of
"the engine is native", but it is a visible change; it needs a yes (question 3).

### 4.3 Singletons and namespaces

`Time`, `Input`, `Physics`, `Debug`, `Application`, `Screen`, `Random`, `Mathf`, `PlayerPrefs`,
`QualitySettings`, `SceneManager`. Static members are direct calls with no receiver: `Time.deltaTime`
is `ts_eng_Time_deltaTime()`; static properties with setters likewise. Constants are inlined from
the manifest (`Mathf.PI`, enum members).

### 4.4 Extern base classes (`Component`)

`Component` is declared, not compiled. A user class `class Player extends Component` gets the prefix
layout of the classes note with the **native component header first**: the compiler reserves a fixed
prefix (the component's `id` and its `gameObject` handle, 16 bytes after the object header) that the
engine fills when it creates the component. Everything after the prefix is the user's fields.
Inherited members (`this.transform`, `this.gameObject`, `this.getComponent(T)`) are extern calls on
the handle in the prefix.

## 5. Component creation and hooks

Today: class name string in, instance by id, hooks by name. Native:

- Each compiled module exports a **class table** (static data), one entry per `register()`ed class:
  `{ name, class_id, instance_size, create: fn(handle_prefix) -> object, hooks: [onAwake, onStart,
  onUpdate, onLateUpdate, onFixedUpdate, onPreRender, onDestroy, ...] }`, each hook a function
  address or null. Which hooks a class has is known at compile time, so the `__platypus_hooks` probe
  disappears and a missing hook costs nothing.
- Rust's `ComponentHost` keeps one **host root** per component (a pointer into the script heap,
  `ts_rt_root_add`), and calls `hook(this, dt)` directly. Order, enabled/active checks and `onStart`
  once are unchanged: only the call changes.
- `@property` is the reflection table of the classes note (name, type, offset), which Rust uses to
  save, load and edit components with no JS. This replaces the decorator metadata and the scene
  JSON code in `scenes.ts`.
- **Errors.** Today a component that throws is reported and disabled. Native code has no exceptions
  (`try`/`throw` is an open design item): a trap (null `this`, an out-of-range index is already
  null/NaN) cannot unwind. Until that design exists a native script fault is a fault in the game
  process. This is a real regression in robustness for development, and the dev loop note (JIT, with
  checks on) should address it.

## 6. Callbacks and events

- **Closures to Rust** (event listeners, `Task` continuations, `onCollision` registered at run time):
  a function value is already a `Closure{func, env}`; the engine receives it as an opaque pointer,
  roots it (`ts_rt_root_add`) for as long as it holds it, and calls `func(env, args...)`. The
  signature is the manifest's, so Rust knows the argument types.
- **Batches become direct calls.** Contacts and ended sounds are delivered as calls to the owning
  component's hook (`onCollisionEnter(other)`) when the engine steps; raycasts return their hit
  through an out object; input state is read by getters. The `__platypus_physics(moved, contacts, rays,
  characters)` style batch disappears with the mirror.
- **Timers and `async`** (`Task`, `setTimeout`) need the event-loop contract of the open `async`
  item. Nothing here decides it.

## 7. Strings, arrays, nullable, generics

- **Strings** cross the boundary in many places (object names, tags, asset paths, scene names,
  `PlayerPrefs` keys). The strings item (STOP: UTF-16 or UTF-8) must come first for these. Until then
  the boundary can only pass numbers, booleans, handles, enums and arrays of those; string-taking
  engine functions are not callable from compiled code. This is the main ordering constraint on this
  item.
- **Arrays**: a script array is a runtime `Array` (len, cap, 8-byte slots). Engine functions that
  return lists (`getComponents`, raycast hits) fill an array the caller allocated and passed in, or
  call back per element. Engine functions never allocate in the script heap behind the collector's
  back except through the non-collecting `ts_rt_*` constructors.
- **Nullable** handles are 0; nullable numbers use the reserved NaN; nullable value classes are the
  null pointer. The manifest says which a function returns.
- **Generics**: `getComponent<T extends Component>(cls: ComponentClass<T>): T | undefined` takes a
  class and returns an instance. With generics blocked on the type sidecar, the compiler treats
  `getComponent(Player)` and `addComponent(Player, { ... })` as **intrinsics**: the class argument
  becomes the class id, the result type is the class. The same for `findObjectOfType`. These are the
  only generic engine calls in the examples.

## 8. Where each part of `js/engine/src` goes

| Module | Lines | Becomes |
|---|---|---|
| `math.ts` | 1,025 | Declared value classes (4.2); the arithmetic is intrinsics, the rest Rust (glam). `Mathf` is a singleton |
| `scene.ts` | 785 | `GameObject`, `Transform`, `Component` as handles / extern base over `platypus_scene`; the mirror is deleted |
| `scenes.ts` | 1,044 | Rust: scene JSON load/save through the `@property` reflection table, `SceneManager`, `instantiate`, prefabs |
| `physics.ts`, `physics2d.ts`, `physics-components.ts` | 1,862 | Declared handle classes over `platypus_physics`; the logic that wraps ops is deleted |
| `rendering.ts`, `models.ts`, `textures.ts`, `particles.ts`, `lines.ts`, `gizmos.ts`, `cloth.ts` | about 2,700 | Declared component classes whose fields are engine properties (setting `material.color` calls the engine) |
| `audio.ts`, `audio-components.ts` | 500 | Declared classes over `platypus_audio` |
| `animation.ts` | 921 | **Rust port** (curves, clips, blend trees, transitions); the largest single port |
| `time.ts`, `clock.ts` | 629 | `DateTime`/`TimeSpan` become value classes with native methods; `Time` a singleton; `Task` waits for the async design |
| `input.ts`, `app.ts`, `screen.ts`, `quality.ts`, `cursor.ts`, `random.ts` | about 1,100 | Singletons over existing Rust state; `Random` needs a seedable API |
| `hot.ts`, `property.ts`, `assets.ts`, `index.ts` | rest | Replaced by the class table and reflection table, or by the dev loop note |

The `.d.ts` that replaces all of this is generated, and most of the 11.5k lines are deleted rather
than translated: the Rust that implements them mostly exists (`platypus_scene`, `platypus_physics`,
`platypus_audio`), because the TypeScript was already a thin client of it. The genuinely new Rust is
`math` (glam is there; the API surface is not), `scenes`, `animation`, `time`.

## 9. QuickJS next to it

QuickJS stays for code the compiler cannot or should not compile (untyped code, mods, a fallback
while a module is being ported). It reaches the engine through bindings **generated from the same
manifest**: `Transform.position` in QuickJS calls the same native function. That has two uses:

- the migration path: `js/engine` shrinks module by module into `.d.ts` plus generated glue, and the
  examples keep running throughout;
- one behaviour: a native script and a QuickJS script see the same engine, with the f32 rounding.

Calls between native modules and QuickJS code (a QuickJS class extending a native component, the
reverse) belong to the hybrid-builds note; this note only requires that neither side blocks the
other.

## 10. The UI and the dev loop

Out of scope here, but they constrain this note:

- The UI tree is already native (`platypus_ui`, driven by `__platypus_commit` opcodes). A UI library in the
  compiled subset would call `ts_eng_ui_*` functions; the React reconciler is not needed for that.
  What it needs from the boundary: typed props and style objects (styles as `Record<string, ...>` are
  the hard case: they need structural typing and string keys), and event callbacks (6, closures to
  Rust).
- A JIT dev loop (Cranelift in-process) links the same extern table at run time instead of at link
  time; the manifest makes that a table of function addresses. Nothing in this note blocks it.

## 11. Decisions I need from you

1. **Source of truth**: Rust annotated with `#[ts_api]` generating the manifest, `.d.ts`, shims and
   QuickJS bindings (recommended); or hand-written `.d.ts` and Rust checked against each other.
2. **Value classes**: phase 1 ordinary structs with scalarised engine calls, scalar replacement
   later (recommended); or build register-passed vectors into the IR from the start.
3. **f32 readback**: accept that `position.x` reads back f32-rounded (as in Unity)?
4. **Strings first**: is it right that string-taking engine functions wait for the strings item, and
   that the first engine calls from compiled code are the numeric ones (`Transform`, `Time`, `Mathf`,
   `Rigidbody`, `Input` axes)?
5. **Robustness**: native faults take the process down until `try`/`throw` is designed. Acceptable
   for the spike, with the dev loop checks as the answer later?
6. **Generics**: treat `getComponent`/`addComponent`/`findObjectOfType` as compiler intrinsics until
   generics exist?
7. **What stays TypeScript**: anything, or is the rule "no engine logic in TypeScript at all", with
   `animation.ts`, `scenes.ts` and `math.ts` ported to Rust? (Compiling them with `platypus_ts` as
   library source is the cheaper alternative and is allowed by the design above.)

## 12. Proposed sub-tasks

| # | Sub-task | Size | Needs |
|---|---|---|---|
| 1 | The manifest format and `#[ts_api]` (or the hand-written equivalent), generating `.d.ts` | M | decision 1 |
| 2 | Compiler: extern table, `Handle` type, singleton calls, direct calls with scalar arguments | M | 1 |
| 3 | First vertical slice: `Time`, `Mathf`, `Vector3` intrinsics, `Transform` position/rotation, `Input` axes, compiled `Component` subclass with `onUpdate`, driven by `ComponentHost` | M | 1, 2, plain classes |
| 4 | Class table and component creation/hooks without names | M | 3 |
| 5 | Port: delete the transform mirror; physics, audio, rendering component classes as declarations | L | 3 |
| 6 | Rust ports: `scenes`, `animation`, `time`, `random` | L | 5 |
| 7 | QuickJS bindings generated from the manifest; `js/engine` reduced to glue | M | 1 |
| 8 | Strings and arrays across the boundary | M | strings item |
| 9 | Closures to Rust (listeners), batches replaced by hooks | M | 4 |
| 10 | The arcade or platformer example running with its gameplay compiled natively | M | 4-6 |

A useful first milestone is sub-task 3: one example component, compiled, moving a `Transform` every
frame with no JS engine in the loop. It shows the whole boundary working, and it gives a number to
set against the 0.12 ms crossing above.

## 13. Answers (from the user, 2026-10-18)

1. **Source of truth: Rust annotated with `#[ts_api]`**, generating the manifest, the shims and the
   QuickJS bindings, *instead of* `__platypus_op`. The user's point, which this note had only half said:
   this speeds up QuickJS as well. The QuickJS bindings become ordinary native functions registered
   with typed arguments (numbers as `f64`, handles as integers), not strings plus a `Value` array in an
   op queue. A consequence to design for: ops are queued and applied once per frame today, so a script
   never sees its own write until the engine has applied it (hence the TypeScript mirror); a direct
   call applies immediately, so reads see writes at once and the mirror can go in QuickJS too.
2. **Value classes: phase 1 ordinary structs with scalarised engine calls**, scalar replacement later.
3. **f32 readback accepted**, as in Unity.
4. **String-taking engine functions wait for the strings item**; the first engine calls from compiled
   code are the numeric ones. ("For now, until implemented": recorded as a limit that lifts when
   strings land.)
5. **Native faults take the process down for now**; the dev loop's checks are the later answer.
6. **Everything in `js/engine` is ported to Rust.** Anything left in TypeScript is user-made, and may
   be ported if it is portable (a Bun library written in TypeScript is the example given). So the
   "compile engine modules with `platypus_ts` as library source" alternative in question 7 is not taken
   for the engine; it remains how *user* libraries are compiled.

**Question 6 answered later: yes**, generics-as-intrinsics (`getComponent`, `addComponent`,
`findObjectOfType`) is the working default. The paragraph below describes the state before that.

**Not answered / assumed.** The reply numbered six items for seven questions; "6" matched question 7
(what stays TypeScript) in content. Question 6, generics (`getComponent`, `addComponent`,
`findObjectOfType` as compiler intrinsics until generics exist), is therefore **still open**; the
recommendation stands as the working default and is easy to change, because it is three functions.

**What this makes next.** Sub-task 1 of section 12: the manifest format and `#[ts_api]`, generating
the `.d.ts`, the `extern "C"` shims and the QuickJS bindings. It lives in the engine workspace (a
proc-macro crate and a generator), not in `tools/`, and the engine crates that carry ops today
(`platypus_scene`, `platypus_physics`, `platypus_audio`, `platypus_script`) are where the first annotated functions
go. `platypus_render` and `platypus_app` carry the `render.*`, `app.*`, `cursor.*` and `screen.*` ops and
belong to another session's area, so they are left for last, and for whoever owns them.

## 14. First slice of `#[ts_api]`: built (2026-10-18)

Sub-task 1 of section 12, in its smallest useful form: two new crates in the engine workspace,
additive (nothing else depends on them, `__platypus_op` is untouched).

- `crates/ts_api_macros`: `#[ts_api(namespace = "Demo", quickjs, handles(GameObject = ObjectId))]`
  on an inline `pub mod`. It keeps the module's code and adds `MANIFEST`, one `extern "C"` shim per
  `pub fn` (`platypus_Demo_add`), and with `quickjs` a `register_quickjs(ctx)`.
- `crates/ts_api`: the manifest types (`Namespace`, `Function`, `Param`, `Ty`), the `Handle` trait
  (the Rust id type implements `to_raw`/`from_raw`), and the generators `to_dts()` (the TypeScript
  declaration, doc comments as JSDoc) and `to_json()` (the table the compiler will read).
- Supported so far: functions over `f64`, `f32`, `i32`, `u32`, `bool`, `()` and declared handle
  types. Handles are `number & { __handle }` in TypeScript, a `u64` through the C ABI and a plain
  number in QuickJS. No registry or linker trick: everything the macro needs is in the one module it
  is attached to.
- **QuickJS gets typed functions**, not `__platypus_op(name, ...args)`: `Demo.add(1, 2)` is a native
  closure taking `f64`s. Measured with `cargo test -p ts_api --release -- --ignored --nocapture`:
  1,000,000 calls take about 150 ns each through the typed function and about 245 ns through a
  simplified `__platypus_op` (a name string, a rest array of numbers, a queue push; the real op also
  converts each argument to a `Value`, so the real gap is larger). Most of the remaining 150 ns is
  QuickJS's own call overhead; the large win is for compiled code, which calls the shim directly.
- Tests: `crates/ts_api/tests/api.rs` (`M9-API-01`..`06`).

**Not built yet** (next, in order): the compiler reading the JSON manifest (an extern call node, the
`Handle` type, f32/i32/u32/bool conversions at the call), value classes (`Vector3`) scalarised across
the boundary, strings, methods on handles (`transform.position`) and the engine context those need (the
shims are free functions, so anything that touches the world must reach it through state the host
sets up, for example a thread-local current world; that design is open), and annotating the real
engine crates. The crates that carry ops today are `platypus_scene`, `platypus_physics`, `platypus_audio` and
`platypus_script`; `render.*`, `app.*`, `cursor.*` and `screen.*` live in `platypus_render` and `platypus_app`
and are left for their owner.

## 15. The compiler calls the engine API: built (2026-10-18)

Sub-task 2 of section 12, in its first form (`M9-TS-142`..`146`). A TypeScript module compiled with
the manifest of an `#[ts_api]` namespace calls it directly.

- `platypus_ts --manifest api.json` (or `compile_with_manifests`) reads the JSON `Namespace::to_json`
  writes (`tools/platypus_ts/src/manifest.rs`). `Demo.scale(3, 4)` becomes `CallExtern`, an import of the
  symbol `platypus_Demo_scale` with the manifest's C signature: no op, no `Value`, no name lookup.
- Numbers stay `f64` in scripts and are converted **at the call**: `f32` demotes on the way in and
  promotes on the way out; `i32`/`u32` take a saturating, truncating conversion (`-5.7` becomes `-5`,
  as `as i32` does in Rust); `bool` is a byte.
- **`Ty::Handle(name)`** is the new IR type for an engine object id: a 64-bit integer, never a GC
  pointer (not traced, not rooted, `===` compares ids). It works in locals, parameters, object
  fields, arrays, `T | null` and closure cells. In a `T | null` slot, **0 is null, so no live
  handle may ever be 0** (the first version of the demo API handed out id 0 for its first entity and
  `if (target === null)` caught it; generations now start at 1). This is a rule for every engine
  handle type.
- An engine function is assumed not to call back into scripts and so not to allocate in the script
  heap: `CallExtern` is not an allocation point in the frame plan. The first engine function that runs
  a script hook changes that (closures to Rust, section 6).
- The end-to-end proof is `tools/platypus_demo_api` (a small world of entities written with
  `#[ts_api]`) linked into `platypus_ts_tests`, whose `build.rs` compiles `bench/engine.ts` with that
  crate's manifest. Every test runs normally, in stress GC mode and with collection off.

Not built yet: value classes across the boundary, strings, methods on handles (`transform.position`),
the engine context the real shims will reach the world through, and annotating `platypus_scene` and the
other engine crates.

## 16. The engine context and value classes: built (2026-10-18)

**Decision: a thread-local current world.** Taken as the recommended default when the question in
section 12's follow-up went unanswered and the work was told to continue. A shim has no world argument
(scripts have none), so the host makes the world current on the scripting thread and API functions
reach it through `ts_api::context`: `enter(&SLOT, &mut world)` returns a scope guard that restores
the previous world when dropped, and `with(&SLOT, |w| ..)` runs a closure on it or answers `None` when
there is none (so a function called outside a frame returns its default instead of crashing). While
the closure runs the world is *not* current, so an API function called from inside it gets `None`
instead of a second `&mut` to the same world (`M9-API-07`..`09`). The alternative (an explicit
context pointer as every shim's first argument) costs an argument on every call and a parameter in
every signature; it is still possible later because the shims are generated.

**Value classes** (`M9-API-10`, `11`; `M9-TS-147`..`150`): `values(Vector3 = Vec3 [x: f32, y: f32,
z: f32])` declares a Rust type that implements `ValueType<N>` as a class scripts can construct and
read.

- Scripts: `new Vector3(1, 2, 3)` is an object with those fields (`v.x`), a struct in the script heap
  like any other; the generated `.d.ts` declares the class.
- Across the C ABI a value parameter is passed as one scalar per field (`v_0, v_1, v_2`); a value result
  is written through a trailing pointer to room for the fields. Scalars rather than a struct avoid the
  Win64/SysV disagreement about small structs, as the design said.
- The compiler flattens the parameter into `Field` reads of a temporary and gives the extern a
  trailing out-pointer; `CallExternInto` calls it with a stack slot, then widens the scalars into the
  fields of a freshly allocated object (allocated before the call, held in a rooted local). Every
  argument goes into a temporary first when a value is among them, so evaluation order is kept.
- All fields of one value class have the same scalar type. QuickJS bindings do not support value
  classes yet (the macro refuses `quickjs` together with `values(..)`).
- This is "phase 1" of the design: each vector is a heap object; scalar replacement later removes the
  allocation without changing the contract.

Not built yet: methods on value classes as inline intrinsics (`a.add(b)`), strings, methods on
handles (`transform.position`), annotating the real engine crates, and QuickJS support for values.

## 17. The first real engine crate: `platypus_scene` (2026-10-18)

`crates/platypus_scene/src/api.rs` declares the `Scene` namespace with `#[ts_api]`, next to the
op-based `ScriptScene`, which is untouched. It is the first engine crate on the new path, and
compiled TypeScript now drives the real scene (`tools/platypus_ts_tests/bench/scene.ts`).

- Functions (no strings yet): `create`, `destroy`, `exists`, `count`, `parent`, `setParent`,
  `enabled`/`setEnabled`, `active`, `layer`/`setLayer`, and position, rotation and scale in local
  space plus position and rotation in world space, as `Vector3` and `Quaternion` values on
  `GameObject` handles (glam's `Vec3` and `Quat`, through the new `ts_api` `glam` feature).
- **The handle encoding.** `ObjectId` is an index and a generation; its raw handle is
  `((generation + 1) << 32) | index`, so **no live object is 0** (generation 0 and index 0 is a real
  object, and 0 must mean "no object" in `GameObject | null`). `from_raw(0)` decodes to a generation
  no slot has, so it misses everywhere.
- **The world.** `platypus_scene::api::enter(&mut scene)` makes a scene current on the thread (the
  `ts_api::context` thread-local); the functions answer with defaults (no object, the origin, the
  identity rotation) when there is none or the id is stale.
- **Not wired into the running engine.** `platypus_app` is another session's area and was not touched:
  nothing calls `enter` yet, and the engine's scripts still use `__platypus_op`. Wiring it is one line
  around script execution (`let _scope = platypus_scene::api::enter(&mut scene);`) plus linking the shims
  into the game executable.
- Adding the dependency changed nothing else: `cargo check --workspace --all-targets` passes.

Next for this crate: strings (names, `find`), methods on handles (`transform.position` instead of
`Scene.position(object)`), component handles, and the same treatment for `platypus_physics` and
`platypus_audio`.

## 18. Handle classes (2026-10-18)

Handles now have members: `parent.transform.localPosition = v` instead of
`Scene.setLocalPosition(parent, v)`. They are declared in ordinary `impl` blocks of the handle's Rust
type inside the `#[ts_api]` module.

- **Syntax.** `#[property] pub fn x(self) -> T` is a getter; `pub fn set_x(self, v: T)` next to it (same
  type) is the setter, and a getter alone is read-only. Other `self` functions are methods, functions
  without `self` are statics of the class (`GameObject.create()`), and plain `pub fn` outside an
  `impl` stay namespace functions (`Scene.count()`). `self` is by value; handles are `Copy`.
- **Distinct newtype per class over the same id.** `Transform` is `TransformRef(ObjectId)`, not a
  second id space: it shares the object's raw handle, so `go.transform` returns the same number and
  `t.gameObject` returns the object. One Rust type per class keeps the impls apart and the shims
  typed, without a second table of ids.
- **Manifest.** `FnKind` (`Static`, `Method`, `Get`, `Set`) per function and a `ClassDef` per class
  (`Namespace.classes`). In the JSON a class function lists its receiver first, as a parameter named
  `this`; parameter names are camelCase (`keepWorld`).
- **Declarations and shims.** `declare class Transform { private constructor(); get position():
  Vector3; set position(value: Vector3); setParent(parent: Transform, keepWorld: boolean): void; }`;
  a handle with no class still emits the `number & { __handle }` alias. Shims are
  `platypus_<Class>_<jsName>` with `this: u64` first; value results still use the trailing out pointer.
- **QuickJS.** Bindings are generated only for namespace functions. A handle is a plain number there,
  so a class would need a wrapper object; that is left until the QuickJS side needs it.
- **Compiler.** The manifest parser fills `ClassFns { statics, methods, getters, setters }`. Lowering
  calls the class symbols through the same `CallExtern` path as namespace functions
  (`emit_extern`, `call_ext_fn`): a property read is the getter call, `obj.prop = v` is a `Place::Ext`
  that calls the setter (assigning a read-only or unknown property is a compile error), a method call
  puts the receiver first, and `Class.f()` is the static. Compound assignment (`+=`) on a value
  property is not supported yet.
- **platypus_scene.** `GameObject` (`create`, `destroy`, `exists`, `transform`, `enabled`,
  `activeInHierarchy`, `layer`) and `Transform` (`gameObject`, `position`, `rotation`, `localPosition`,
  `localRotation`, `localScale`, `parent`, `setParent(parent, keepWorld)`) next to the `Scene`
  namespace functions. Tests `M9-API-16`..`19`, `M9-TS-156`..`158`.

```ts
const parent = GameObject.create();
const child = GameObject.create();
parent.transform.localPosition = new Vector3(10, 0, 0);
child.transform.setParent(parent.transform, false);
const p = child.transform.position; // world position, a new Vector3
```

Not built yet: methods on value classes as intrinsics (`a.add(b)`; built in section 19), strings
(names, `find`; built in section 20), classes in the QuickJS bindings, and the same treatment for `platypus_physics` and
`platypus_audio`.

## 19. Value-class methods (2026-10-18)

Value classes have members: `a.add(b).scale(2)`, `v.magnitude`, `q.rotate(v)`.

- **Syntax.** A free function in the `#[ts_api]` module marked `#[method(Vector3)]` or
  `#[getter(Vector3)]`, where `Vector3` is declared in `values(..)` and the first parameter is that
  value. A getter takes only the value. Anything else is a compile error.
- **Why free functions with an attribute.** The value type is a foreign glam type (`Vec3`), so an
  inherent `impl` is not possible in `platypus_scene`. An extension trait per value would work but is
  heavier: a trait, an impl and an import for each function, and the macro would have to find them.
  The attribute keeps the function a plain `pub fn` that Rust code can call too.
- **Manifest and declarations.** The functions go into `Namespace.classes` under the value's name, the
  receiver first as `this` (`value:Vector3`). `to_dts` declares them inside `declare class Vector3`,
  not as a second class and without a handle alias. Shims are `platypus_Vector3_add` and so on; the
  receiver travels as the value's fields like any value parameter, and results come back through the
  out pointer.
- **Compiler.** The manifest parser adds them to `ClassFns` of that name without registering a handle
  type. `value_member` (lower/expr.rs), `lower_method` (lower/arrays.rs) and `member_of` call them as
  direct extern calls. A field of the same name wins over a getter.
- **platypus_scene.** `Vector3`: `add`, `sub`, `scale`, `dot`, `cross`, `distance`, `lerp`, `magnitude`,
  `normalized`; `Quaternion`: `mul`, `rotate`, `inverse`. Tests `M9-API-20`, `M9-TS-159`..`161`.
- **Not built.** Static functions on value classes (`Vector3.zero`), operators (`a + b`), and
  compound assignment on value properties.

```ts
const a = new Vector3(1, 2, 3);
const b = new Vector3(4, 5, 6);
const s = a.add(b).scale(2);           // (10, 14, 18)
const m = new Vector3(3, 4, 0).magnitude; // 5
const q = new Quaternion(0, 0.7071068, 0, 0.7071068); // 90 degrees round y
const v = q.rotate(new Vector3(1, 0, 0)); // (0, 0, -1)
```

## 20. Strings across the boundary (2026-10-18)

`#[ts_api]` functions take `&str` (or `String`) parameters and return `String`; the strings item of
the compiler is now complete (see [platypus-ts-strings.md](platypus-ts-strings.md)).

- **Type.** Manifest, JSON and d.ts say `string` (`ts_api::Ty::Str`; compiler `ExtTy::Str`).
  `string | null` parameters are not supported. A string in a `quickjs` namespace is a compile error.
- **In.** A parameter is a `*const u8` to the script string object (header `ts_api::ScriptStr`,
  layout equal to `platypus_ts_rt::Str`, bytes inline after the 48-byte header). The shim reads it with
  `ts_api::script_str`, which borrows the bytes as `&str` without a copy when the string is well
  formed, and otherwise copies it with each lone surrogate replaced by one U+FFFD. The collector is
  non-moving and the caller roots the string, so the pointer is valid for the call. The engine must
  copy what it keeps. Shims with string parameters are `unsafe extern "C"`.
- **Out.** A `String` result adds a trailing `out_len: *mut i64` and returns `*const u8` into a
  per-thread buffer (`ts_api::return_string`). The compiled call copies it at once into a new script
  string with `ts_rt_str_new_gc`, so it can collect: the root analysis counts it as allocating.
  Codegen passes a trailing stack slot for the length.
- **platypus_scene.** `GameObject.name` (get and set) and `GameObject.find(name)`. Tests `M9-API-21`..`22`
  (crates/ts_api/tests/strings.rs) and `M9-TS-183`..`186` (tools/platypus_ts_tests/tests/scene.rs).
- **Not built.** `string | null` parameters; strings in the QuickJS bindings.

## 21. Components for the host (2026-10-19)

Section 5 built in its smallest form: `Component` is a prelude class, `register(X)` becomes exported
constructor and hook thunks, the bindings carry a class table and `platypus_ts_rt::NativeHost` runs the
lifecycle over it with the same rules as `ComponentHost` (see `docs/platypus-ts.md`, "Components for the
host"). The host first lived in `platypus_ts_rt` so the engine depended on one crate; it has since moved to
`crates/platypus_components` (the runtime keeps a generic `HostClass`, see `docs/research/web-ui-split.md`). `platypus_app` wiring (enter the
scene, call `update`/`fixed_update`/`pre_render`, call `ts_rt_timers_advance(now_ms)` once per frame) is
the contract for the session that owns it. Tests `M9-TS-556..561`.
