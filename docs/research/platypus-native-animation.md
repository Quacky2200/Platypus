# Animation in Rust: design note (for review before it is built)

Status: **waiting for the user** (2026-10-19). This is the next item of `platypus-native-engine.md` after the first batch and
the math types. It is the first port that is not "same functions, typed": `js/engine/src/animation.ts` (921 lines) is built
from object literals, string-keyed property writes on arbitrary components and callbacks by method name, none of which can
cross the native boundary as they are. This note says what the native version looks like, what it needs under it and what
changes for a game, so the shape can be agreed before the work is done.

## 1. What `animation.ts` does

- `Keyframe`, `AnimationCurve`: keys with tangents joined by cubic Hermite segments, wrap modes before and after, helpers
  (`linear`, `easeInOut`, `constant`, `fromPoints(points, "linear" | "smooth" | "step")`), `evaluate(t)`.
- `AnimationClip`: curves by (relative path, target, property), events (`time`, method name, parameter), wrap mode, frame
  rate, `length`, `sampleAnimation(go, t)`, JSON in and out.
- `Pose`: blends the clips playing on one object for a frame (weighted sums per transform channel, quaternions sign-aligned
  then normalised, other properties weighted averages) and writes the result once.
- `Animation` (legacy component): named states, `play`, `crossFade`, `stop`, `isPlaying`.
- `Animator` + `AnimatorController`: parameters, states (a clip or a 1D blend tree), transitions on conditions or exit time
  with cross-fades, `Any` state, speed parameters, triggers, `getCurrentAnimatorStateInfo`.
- Every frame, after all `onUpdate` and before `onLateUpdate`, the engine calls `__platypus_animate(dt)`.

## 2. Native shape

**A crate `platypus_animation`** with no script engine in it: curves, clips, states, the controller graph, blending, events, JSON
(serde). A system (`AnimationWorld`, per thread like the other engine state) holds the players and has
`update(&mut Scene, dt, &mut dyn PropertySink) -> Vec<FiredEvent>`, which the host calls where it now calls `__platypus_animate`.
Transform channels and `GameObject.active` are written to the scene directly. Nothing in it knows TypeScript.

**The script API** is declared in `platypus_engine_api` (`animation.rs`) with `#[ts_api]`, so compiled code and QuickJS both get
it (same as every other module). Types are handles into the system's arenas:

| Class | Made by | Members |
|---|---|---|
| `Keyframe` | `new Keyframe(time, value, inTangent?, outTangent?)` | value class of four numbers |
| `AnimationCurve` | `AnimationCurve.linear / easeInOut / constant(..)`, `new AnimationCurve()` | `addKey(time, value, inTangent?, outTangent?)`, `removeKey(i)`, `length`, `key(i)`, `evaluate(t)`, `preWrapMode`, `postWrapMode`, `fromPoints(flat numbers, mode)` |
| `AnimationClip` | `new AnimationClip(name)` | `setCurve(path, type, property, curve)`, `removeCurve(..)`, `addEvent(time, functionName, parameter?)`, `wrapMode`, `frameRate`, `length`, `sampleAnimation(go, t)` |
| `Animation` | `go.addComponent(Animation)` | `addClip(clip, name?)`, `play(name?)`, `crossFade(name, fade?)`, `stop(name?)`, `isPlaying(name?)`, `clip`, `playAutomatically`, `state(name)` with `time/speed/weight/enabled/normalizedTime` |
| `AnimatorController` | `new AnimatorController()` | builder: `addParameter`, `addState(name, clip, speed?, speedParameter?)`, `addBlendState(name, parameter)` + `addBlendChild(state, clip, threshold, timeScale?)`, `addTransition(from, to)` returning a `Transition` with `condition(parameter, mode, threshold?)`, `exitTime(x)`, `duration(d)`, `offset(o)`, `defaultState` |
| `Animator` | `go.addComponent(Animator)` | `controller`, `setFloat/Int/Bool/Trigger(name, ..)`, `getFloat/Int/Bool(name)`, `currentState` (name, normalized time, length), `speed` |

Strings are used where TypeScript had unions (`"loop"`, `"Transform"`, `"if"`), as in the rest of the API.

## 3. What changes for a game (the differences to review)

1. **Object literals become builders.** `new AnimatorController({ states: [...], transitions: [...] })` and
   `controller.addTransition({ from, to, conditions: [...] })` cannot cross the boundary. The calls above replace them.
   `AnimationEvent` objects become `addEvent(time, functionName, parameter?)`.
2. **`fromPoints` takes a flat list** (`[t0, v0, t1, v1, ..]`) once arrays of numbers cross the boundary (section 4); until then
   `AnimationCurve` is built key by key.
3. **Properties of components are looked up by name through a registry**, not by writing to a JS object. Engine components
   (`Light`, `MeshRenderer`, `AudioSource`, ...) declare their number properties through their `#[ts_api]` setters, so
   `setCurve("", "Light", "intensity", c)` works exactly when `Light` has `set_intensity`; a value field such as `color.r` is
   read, changed and written back. A compiled component animates its `@property` fields only (they are in the reflection
   table, `NativeHost::set_property`, `M9-TS-601..604`). Any other field is not animatable (TypeScript could write any
   field).
4. **Events** call a method by name on the object's components. Engine components have none worth calling; compiled
   components get a generated thunk per method that takes at most one number, boolean or string (the compiler already generates
   the hook thunks the same way). The system returns the fired events and the host dispatches them, so the system itself
   stays free of script code.
5. `toJSON` / `fromJSON` stay (scenes and prefabs use them) but become Rust (serde_json, same field names), so the scene port
   reads them without a script.

## 4. What it needs under it

- **Arrays across the boundary** (`number[]`, `string[]`, handle arrays, value arrays) in parameters and results. In: a pointer
  to the script `Array`, read like a string; out: a per-thread buffer the compiled call copies into a new script array, like
  strings. This is needed again by physics (raycast hits), the scene (`getComponents`, children) and rendering (vertices), so it
  is built once, in `ts_api` first.
- **Optional value parameters** (`#[optional]` on a `Vector3`: left out means "the default the function documents").
- **Handle classes in QuickJS** (the item already open): QuickJS games use the same API.
- Reading the scene's component list from Rust: `getComponent` exists for compiled components; the animation system needs "the
  components of this object by class name" for events, and the property registry above.

## 5. How it is tested

Same rule as every module: the original is the oracle. The TypeScript runs in bun against the real `scene.ts` with a stubbed
op bus (`js/engine/test` already does this), a script of calls (build a clip, play, advance dt several times, read the
transforms) is written once in a bench program, compiled against the Rust API, and the numbers are compared as strings with
full precision. Curves, blending, cross-fades, transitions, blend trees, events and the JSON round trip each get a program.
GC stress runs as for the rest.

## 6. Phases

1. Arrays across the boundary, optional value parameters (infrastructure, no animation yet).
2. `platypus_animation`: curves and clips, `sampleAnimation`; script API for them; JSON.
3. `Animation` component (states, cross-fade, events, property registry).
4. `Animator` and the controller builder (transitions, blend trees, triggers).
5. `examples/animation` runs on the native path (needs `platypus_app` wiring, another session's area: a hand-over note).

## 7. Questions for the user

1. Is the builder shape in section 3 acceptable for the controller and events, or should `ts_api` grow support for nested
   object literals instead (a larger feature: a manifest type for "options object of these fields", used for
   `new AnimatorController({..})`)? The builders are less work and clearer in the generated types; literals are closer to the
   current code.
2. Is "only `@property` fields are animatable on compiled components" fine?
3. Should phase 1 (arrays) go first and be reviewed separately, or all in one go?

## 8. Revision after review (2026-10-19)

The user's direction: explicit classes **and** a chained builder over them, with the `Animation*` family named by the user.

- Classes (handles with properties and `{ option }` constructors, no arrays needed): `AnimationController`,
  `AnimationParameter(name, type, defaultValue?)`, `AnimationCondition(parameter, mode, threshold?)`,
  `AnimationTransition(from, to, { duration, hasExitTime, exitTime, offset })` with `addCondition`, `AnimationNode`
  (the controller's state; `AnimationState` stays the legacy per-clip play state of `Animation`) and `AnimationBlendTree`.
  `Animation` (clip player) and `Animator` (runs a controller) stay components.
- `AnimationController.build()` returns an `AnimationBuilder` handle whose methods return itself: `parameter`, `state(name, clip,
  { speed, speedParameter })`, `blendState(name, parameter)` + `child(clip, threshold, timeScale?)`, `transition(from, to)` then
  `when(parameter, mode, threshold?)`, `exitTime`, `duration`, `offset`, `defaultState`, and `create()`. It makes the objects
  above, so both ways give the same controller. Clips get `event(time, functionName, parameter?)` over `AnimationEvent`.
- Phase 1 (arrays) is no longer first: nothing in this shape needs an array. It moves to when physics needs it.
- Open: whether `Animator` should be renamed too.

## 9. Status

- **Phase 2 done (2026-10-19):** `crates/platypus_animation` (curves, clips, pose blending with quaternion alignment, `sample`, JSON)
  and the script API for `Keyframe`, `AnimationCurve`, `AnimationClip` (`platypus_engine_api::animation`), `M9-TS-676..683`.
  Differences so far: `addKey(time, value?)` makes flat tangents (use `addKeyframe(new Keyframe(..))` for tangents, as the
  original), a clip's target is a class-name string, an event parameter is a number, `toJson()` is text, `fromPoints` waits for arrays.
  A namespace named like a handle class (`AnimationCurve`) also holds the class statics for the compiler.
- Next: the `Animation` component (states, cross-fade, events) with the property registry for components, then the
  controller classes and builder.

- **Phase 3 done (2026-10-19):** the `Animation` component (`platypus_animation::player`, `platypus_engine_api::animation`:
  `Animation.attach`, `AnimationState`, `Animations.updateAll(dt)` / `takeEvents()`), the property registry
  (`platypus_animation::registry`, `Camera` registered), `M9-TS-684..691`. The host calls `platypus_engine_api::animation::update(dt, other)`
  after the component updates and delivers the returned `FiredEvent`s. Differences: the first update starts the default clip
  (the original did it in `onStart`, before the frame's `onUpdate`); events are delivered after the frame's pose is written
  (the original called the methods in the middle of it).

- **Phase 4 done (2026-10-19):** the controller classes, the builder and the `Animator` (`platypus_animation::{controller, animator}`,
  `platypus_engine_api::animator`), `M9-TS-692..697`; `new Class(a, b, { option })` sets the options through setters after `create`.
  The page for the code is `docs/animation-native.md`. Left: `AnimationCurve.fromPoints` (arrays), engine components beyond `Camera`
  in the registry (as they are ported), `examples/animation` on the native path (needs `platypus_app` wiring).
