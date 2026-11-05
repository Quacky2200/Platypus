# Physics in Rust for compiled scripts: design note

Status: **decisions taken, two questions open** (2026-10-20). Next after scenes in the order the user gave (components, math, physics,
audio, rendering). `physics.ts` (bodies, colliders, joints, raycasts), `physics-components.ts` (Rigidbody, the collider
components, CharacterController, joints, Rope) and `physics2d.ts` are 1,860 lines over `crates/platypus_physics`, which already
simulates everything. What is left is the script surface, and three pieces of boundary the compiler does not have yet.

## 1. What is needed that does not exist

| Need | Used by | Proposal |
|---|---|---|
| **Arrays of numbers / `Vector3` into the engine** | `convex` and `mesh` shapes, `LineRenderer` points, `AnimationCurve.fromPoints` | `ExtTy::F64Arr` and `ExtTy::ValueArr(Vector3)`: a typed array / `Vector3[]` is passed as pointer and length. Results come back through a count and an indexer, as `Scene.children` does. |
| **Callbacks into script** | `onCollisionEnter/Exit/Stay`, `onTriggerEnter/Exit` | They are component hooks, not closures: `NativeHost` gets hooks 9 to 13 (taking the other collider's component or object). The physics step returns contact events and the host dispatches them after the fixed tick, like `FiredEvent` for animation. `PhysicsBody.onCollisionStart` closures stay QuickJS-only until closure APIs exist. |
| **`raycast` as a Promise** | `physics.raycast` | The world is in process, so the native `Physics.raycast(origin, dir, max)` answers at once (the QuickJS op path keeps its one-tick delay). A hit is read through `RayHit` handles (`collider`, `distance`, `point`, `normal`). This is a difference to list for review. |

## 2. Shape

- `platypus_physics` keeps the rapier world and script-chosen ids. A new `crates/platypus_physics_api` (or a module of `platypus_engine_api`)
  declares `Rigidbody`, `BoxCollider`, `SphereCollider`, `CapsuleCollider`, `MeshCollider`, `CharacterController`, the joints
  and `Physics` (`gravity`, `raycast`, `overlap*`) with `#[ts_api]`, as handle classes whose objects are the GameObjects
  (like `Camera`): `go.addComponent(Rigidbody, { mass: 2 })`, `go.getComponent(BoxCollider)` through `findOn`.
- Each registers a **kind** (`platypus_kinds`), so scene files with physics components load, and `instantiate` copies them.
- A fixed tick is `physics::step(dt)` and then `sync` writes the moved bodies back into `platypus_scene` (the existing
  `take_moved` / `attach` path) and returns the contact events for the host.
- 2D physics is the same registry with a `2D` suffix and the z axis locked, as `physics2d.ts` does today.

## 3. Order

1. Arrays across the boundary (compiler, macro, runtime) with tests against bun.
2. Component hooks for contacts in `NativeHost` and the `Collision` argument.
3. `Rigidbody` + colliders + `Physics.raycast` (compared by a fixed-seed simulation against the QuickJS path: the same
   rapier world, so the numbers must match).
4. Joints, `Rope`, `CharacterController`, then 2D.
5. Kinds for scene files.

## 4. Open questions for review

- A raycast that answers at once changes when a script sees a hit (same tick instead of the next). Keep the one-tick delay for
  compatibility, or take the simpler answer?
- Should `Collision` be a handle class (`collision.other`, `collision.speed`) or the two numbers `(otherObject, speed)`?

## 5. Decisions (user, 2026-10-20)

- `raycast` is synchronous and returns the hit (or none): no Promise, no one-tick delay on the native path.
- Arrays are stored natively (typed arrays and `Vector3[]` are real arrays in the engine and in compiled code), not copied into
  QuickJS-style lists. `points` of a shape is such an array.
- Vectors and points live on the stack: value classes (`Vector3`, `Quaternion`, `Color`, ...) stop being heap objects in compiled
  code (scalar replacement; passed and returned in registers or in a caller-owned slot). This is its own compiler step, done
  before physics because every physics call passes them.
- JSON is as native as possible: `JSON.parse` / `stringify` in compiled code are Rust (they already are); on the QuickJS side
  the bridge copies once per call and gets a native-speed path later with rquickjs work.
- Contact events are component hooks called by name (`onCollisionEnter`, `onCollisionExit`, `onTriggerEnter`, `onTriggerExit`,
  plus `onCollisionStay`).

## 6. Order now

1. Stack value classes in the compiler (with a benchmark against the heap version).
2. Native arrays across the boundary.
3. Collision hooks in `NativeHost`.
4. Rigidbody, colliders and `Physics` queries; then joints, character controller, 2D; then kinds for scene files.

Open: the shape of the `Collision` argument (a value type `Collision { other, collider, speed, sensor }`, recommended, or two
loose parameters), and how value-class `@property` fields are written back once values are on the stack (a fresh value, not
in place).

## 7. More decisions (user, 2026-10-20)

- Physics is all native: no op-based path stays next to it.
- `Rigidbody2D` and the 2D colliders stay separate classes (same registry, z locked).
- Handles in QuickJS: whatever the compiled side treats as internal (engine ids, handles) is exported to QuickJS as an opaque
  internal value, like V8's internal fields; QuickJS encodes plain data (numbers, strings, arrays, value classes) and anything
  that touches the engine goes through those opaque handles. Details to confirm when QuickJS handle classes are built.

## 8. Contact hooks (done, 2026-10-20)

Contacts reach components as hooks called by name: `onCollisionEnter`, `onCollisionExit`, `onCollisionStay`, `onTriggerEnter`,
`onTriggerExit`, `onTriggerStay`. All six take one `Collision`, a prelude class the compiler builds in the hook's thunk (so a
script that only declares the hook costs nothing elsewhere):

```ts
class Collision {
  other: GameObject | null;   // the object on the other side (null: its body is not attached to one)
  collider: Collider | null;  // the other side's collider (a number when the manifest has no Collider handle)
  speed: number;              // impact speed going in (m/s); 0 on exit
  sensor: boolean;            // one side is a sensor (the trigger hooks are used then)
  get gameObject()            // = other (the QuickJS Collision's name)
  get relativeSpeed()         // = speed
}
```

`NativeHost::dispatch_physics(active)` takes `platypus_physics_api::take_contacts()`, maps colliders to objects with
`object_of_collider` and tells the components of both objects (a collider with no object tells nobody).
`dispatch_contacts(&[ContactEvent], active)` is the same with events made by hand. Sensors call the trigger hooks, solid contacts
the collision hooks. `Stay` is made by the host: every pair that began and has not ended gets `onCollisionStay` /
`onTriggerStay` on each dispatch (events first, so a pair that began this tick gets Enter and then Stay). Call it after every
fixed tick. Disabled components and components of inactive objects miss events (the pair still counts as touching). A hook that
throws is a `Fault` and disables the component; a component not started yet gets `onStart` first.

Differences from the QuickJS `physics-components.ts`: `onTriggerEnter` gets a `Collision` (not the other `Collider`; the object is
`collision.other`), there is no `rigidbody` field, and `Stay` / `onTriggerStay` exist. 2D hooks are not made yet.
