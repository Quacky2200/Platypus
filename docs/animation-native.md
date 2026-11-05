# Native animation (`platypus_animation`)

Animation without a script engine. Design and review: [research/platypus-native-animation.md](research/platypus-native-animation.md).
This is the only implementation: the TypeScript one (`js/engine/src/animation.ts`) was deleted with QuickJS.

## Crates

- `crates/platypus_animation`: curves (`curve.rs`), clips and the arenas (`clip.rs`), bindings resolved per animated root
  (`bind.rs`), pose blending (`pose.rs`), the `Animation` component (`player.rs`), controller data (`controller.rs`), the
  `Animator` (`animator.rs`), culling (`culling.rs`), bench timers (`profile.rs`) and the property registry (`registry.rs`).
  No script engine in it.
- `crates/platypus_engine_api/src/animation.rs`, `animator.rs`: the script API (`#[ts_api]`), for compiled code.

## Frame

The host calls `platypus_engine_api::animation::update(dt, other)` after the components' updates. Players go depth first through the
scene (inactive objects skipped); each blends its clips into one `Pose` (weighted sums per transform channel, quaternions
sign-aligned then normalised, other properties weighted averages) and writes it once. Clip events that came due are returned
as `FiredEvent { object, function_name, parameter }` for the host to deliver to the object's components (and kept for
`Animations.takeEvents()`). `other` is a `PropertySink` for components the registry does not know (a game's compiled ones).
The compiled-game host calls `update_culled(dt, other, visible)` instead (see Culling).

**Bindings (Unity's `Animator.Rebind`).** A clip's curves are resolved once per (clip, animated root) (`bind.rs`): each
`Transform` curve gets a bone slot (an index into the root's bone table) and a channel number (position x y z, rotation
x y z w, scale x y z, Euler x y z; a property that is none of them only touches its bone, as before), each component curve
its object and an interned (class, property) key, and each bone's quaternion curves are listed for aligning blends. A root's
bindings are kept against `Scene::tree_version(root)`, a number that changes when a name or a parent link changes anywhere
under the object (a rename, a re-parenting, a new child, a destroyed object; moves and flags don't touch it), and a clip's
against `World`'s edit count (`clip_mut` / `curve_mut` bump it). Roots that are gone are swept once their number doubles.
`Pose` is then a dense per-bone array of sums and weights (no strings): `add_aligned` samples every curve once and takes each
bone's dot product from those values; curves of one bone with the same key times and wrap modes (a glTF channel's x, y, z,
w) share one key search (`Curve::locate`, then `at`; `evaluate` is the two together, so values are unchanged); rotations
are still blended component-wise and normalised (nlerp). Players fill one scratch `Pose` kept in the `World`; `apply`
writes each touched bone once and empties it, keeping its buffers. Clip lengths are kept until a clip or curve changes or
a curve is added. The output is the same bit for bit as the string-keyed `Pose` it replaced, which stays as a test oracle
(`legacy_pose.rs`, `M5-ANI-08`). `Scene::set_local` marks moved objects through a per-object flag and stops at objects
already stale and listed (`M5-OBJ-10`).

**Culling (Unity's `Animator.cullingMode`, `Animation.cullingType`).** `Animator.cullingMode` is the enum
`AnimatorCullingMode`: `alwaysAnimate` (the default: as before, visibility is never asked), `cullUpdateTransforms` (while
unseen the state machine, clock, parameters and events go on but no transform or component property is written, so once
seen the pose is where it would have been, `M5-ANI-09`, `M5-ANI-11`) and `cullCompletely` (while unseen nothing runs, no
events fire and the clock stops; it holds its last pose and resumes from there, `M5-ANI-10`). `Animation.cullingType` is
`AnimationCullingType`: `alwaysAnimate` or `basedOnRenderers` (paused while unseen, like `cullCompletely`, `M5-ANI-12`).
*Seen* means any renderer on the animated object or below it was drawn last frame: inside a camera's frustum (and not past
its fog's cull distance), or a shadow caster in a light's shadow map whose shadow can fall in a view (the box swept along
a directional light meets the frustum; for spot and point lights, the light reaches the frustum and the box is in its
range), so a figure off screen whose shadow is on screen keeps animating. An object with no renderers is always seen.
How it is wired: `culled_roots()` lists the objects whose components cull; the app (`crates/platypus_app/src/anim_cull.rs`)
watches only the renderers under them, giving their meshes a `MeshInstance::visibility_key`; `WorldRenderer::seen()` /
`take_seen()` reports which were seen (skinned meshes judged by their bind-pose box under every bone, a box that holds
them); the next frame's update asks through `platypus_animation::Visibility` (`M5-ANI-13`, `M5-CUL-03`). This is one frame
late. Every camera counts, those drawing into render textures too (`M5-CUL-06`): secondary windows draw no world of their
own (`Camera.targetWindow` is not built, `docs/windows.md`), so a window shows a 3D view through an `image` of a render
texture, whose camera is drawn, and judged, with the main window's frame.

**The safeguard (Platypus's; Unity has none).** A culled animator writes no bone transforms, so what hangs on its bones would
be left behind. The exact rule: an animated object is never culled (it counts as always seen, and its renderers are not
watched) while any object *strictly below it* has a physics component (`Rigidbody`, any collider, `CharacterController`, a
joint or rope, 2D ones too), an `AudioSource`, a `ParticleSystem`, a `Light` or a script component (any class of the game).
Components on the animated object itself do not count (culling never moves it unless a clip animates path `""`; a script
driving the animator usually sits there). A warning on target `animation`, once per animated object, names the first such
object and component: ``animation culling is off for `Figure`: `Hand` below it has a BoxCollider, which a culled animation
would leave behind``. The answer is worked out from the registries (`anim_cull::blockers`: each component walks up its
ancestors) only when a component is added or removed anywhere (`platypus_kinds::components_version`, bumped by every owner) or
the culled objects or their `Scene::tree_version`s change, so a frame costs one tree-version read per culled object
(`M5-ANI-14`). Objects with none of these components (meshes, other renderers, cameras, empty bones) still go stale while
culled.
The renderer culls skinned meshes by that same box (`WorldRenderer::world_box`) in every view, so an unseen figure is also
not drawn (`M5-CUL-05`). `MeshRenderer.isVisible` / `SkinnedMeshRenderer.isVisible` read the same answer: a renderer is
watched once a script has asked, and the host reports the frame's seen renderers back with `set_seen_renderers`
(`M5-CUL-04`). `update_culled` groups players and animators per object in a map, instead of scanning all of them for each.

**The walk and threads.** The update walks only towards animated objects: the objects with players or animators and the
objects above them (a crowd's bones are not visited). With `platypus_animation::set_threads(n)` above 1 (the app sets the
machine's threads, up to 16; `PLATYPUS_ANIM_THREADS=1` turns it off) a frame runs in two halves (`parallel.rs`): the walk lists
the jobs in order; animators advance their state machines, blend their clips and work out their bones' new local
transforms on up to `n` threads (every n-th animator to a thread, at least 8 a thread), reading the scene only, each into
its own lists of transforms and events; then, in the walk's order, players run and the animators' transforms are written
and their events kept. The result is the same bit for bit as in turn (`M5-ANI-17`). The frame runs in turn instead while
any clip moves more than transforms (`GameObject.active` changes the walk; component properties go through a sink that is
not shared; `World::only_transforms`, kept by edit count) or an animated object is under another one (their bones could
overlap). The world's caches are locks for this: `binds` (read on the frame's path, written only to bind), the clip
lengths, the scratch pose; property names are `Arc<str>`. Bench timers: `animation: animators in parallel`; `pose add` and
`pose apply` are only timed in turn.

**Updating by hand (Unity's `Animator.Update(float)`).** `animator.update(dt)` advances the clock by `dt`, runs the
transitions, fires the events (kept for `Animations.takeEvents()`) and writes the pose at once, whatever its culling mode and
whether it was seen. The frame's update also advances an enabled animator, so calling `update` on one moves it twice as fast:
the manual pattern is Unity's, `animator.enabled = false` (the frame's update then skips it, it holds its pose, and it is not
watched for culling) plus `update(dt)` from a script (`M5-ANI-15`). There is no `updateMode` (Unity's `Normal` /
`AnimatePhysics` / `UnscaledTime`): the frame's update uses the scaled frame time.

**Bench timers.** With `--bench`, `profile.rs` adds `animation: update (all)`, `animation: pose add`, `animation: pose
apply` and `animation: set_local` to `syncParts` (`sync_profile` merges them); each is a clock read per call, and
`set_local` is timed per bone, so its own share is partly the timer.

## Script API

| Class | Made by | Notes |
|---|---|---|
| `Keyframe` | `new Keyframe(time, value, inTangent?, outTangent?)` | value class |
| `AnimationCurve` | `new AnimationCurve()`, `.linear/easeInOut/constant(..)` | `addKey(time, value?)`, `addKeyframe(k)`, `removeKey`, `key(i)`, `evaluate(t)`, `length`, `preWrapMode`, `postWrapMode` |
| `AnimationClip` | `new AnimationClip(name?)` | `setCurve(path, "Transform", "localPosition.x", curve)`, `removeCurve`, `addEvent(time, fn, parameter?)`, `wrapMode`, `frameRate`, `length`, `sampleAnimation(go, t)`, `toJson()` / `AnimationClip.fromJson(text)` |
| `Animation` | `go.addComponent(Animation)` | `addClip`, `play`, `crossFade`, `stop`, `isPlaying`, `state(name)` (`AnimationState`: `time`, `speed`, `weight`, `enabled`, `normalizedTime`), `clip`, `playAutomatically`, `cullingType` |
| `AnimationParameter`, `AnimationCondition`, `AnimationTransition`, `AnimationBlendTree`, `AnimationNode` | `new X(..)` with `{ option }` setters | the pieces of a controller; `AnimationNode.blend(name, tree)` |
| `AnimationController` | `new AnimationController()` or `AnimationController.build()...create()` | `addParameter/addNode/addTransition`, `defaultState`, counts and `...At(i)`, `toJson` / `fromJson` |
| `AnimationBuilder` | `AnimationController.build()` | chained: `parameter`, `state`, `blendState` + `child`, `transition` + `when` / `exitTime` / `duration` / `offset`, `defaultState`, `create` |
| `Animator` | `go.addComponent(Animator)` | `controller`, `speed`, `cullingMode`, `enabled`, `update(dt)`, `set/getFloat/Integer/Bool`, `setTrigger`, `resetTrigger`, `play`, `crossFade`, `isInTransition`, `currentStateName` / `currentNormalizedTime` / `currentLength` and `next...` |

Targets of a curve: `Transform` (`localPosition.x|y|z`, `localScale.x|y|z`, `localRotation.x|y|z|w`, `localEulerAngles.x|y|z`),
`GameObject` (`active`) and registered components (`Camera`: `fieldOfView`, `nearClipPlane`, `farClipPlane`, `depth`,
`orthographicSize`, `orthographic`, `backgroundColor.r|g|b|a`). Register more with `platypus_animation::register_type` and
`register_setter`.

An `Animator` state keeps its normalized time (Unity's), not seconds: a blend tree's length changes with its weights, so the
phase stays put when they change (a walk breaking into a run carries on mid-stride) and then advances at the blend's pace
(`M5-ANI-07`). `platypus_animation::gltf` turns a glTF animation's channels into a looping clip (`Model.animations`), each rotation
key flipped to its predecessor's side (`q` and `-q` are one turn; mixed sides sampled between them pass near zero, a wild pose
for a frame), cubic keys flipping with their tangents (`M5-SKN-04`). `Animation.destroy()` empties the component's slot (players
are numbered by place).

## Differences from `animation.ts`

Object literals became classes and a builder; a clip target is a class-name string; an event parameter is a number; JSON is text;
`addKey` has no tangents (`addKeyframe` does); `fromPoints` waits for arrays across the boundary; a parameter default is a number
(the controller's `addParameter(name, type, true)` takes a flag too);
`getCurrentAnimatorStateInfo()` is properties; the default clip starts in the first update and events are delivered after the
frame's pose is written; `Animator` objects update after `Animation` objects on the same object. Tests `M9-TS-676..697`.
