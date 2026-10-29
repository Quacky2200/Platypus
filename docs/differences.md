# Differences from browsers

Where Platypus's CSS and rendering knowingly behave differently from a browser (or from the spec),
and why. **Read this before changing one of these areas**: each entry is a deliberate trade-off or
a known gap, not an accident. When you fix one, delete its entry; when you add one, say why.

## QuickJS backend (removed)

QuickJS was removed on 2026-10-28; compiled TypeScript is the only backend and `platypus-app` refuses a QuickJS bundle (see `docs/platypus-build.md`).
Rows below that mention QuickJS or `js/engine` describe the "Before" column, kept for the history of a decision.

## Script API vs Unity, s&box and C# (M5.a)

The TS API follows Unity / s&box names and units (`Vector3`, `Quaternion`, Euler degrees,
`Debug.log`, `DateTime`, ...). Where it can't match:

| Area | Platypus | Unity / s&box / C# | Why |
|---|---|---|---|
| Handedness | Right-handed, y up, forward is -z; positive pitch (about +x) tips forward **up** | Unity: left-handed, forward +z, positive pitch looks down. s&box: z up, forward +x | glam, rapier and wgpu are right-handed; Euler order matches Unity's (z, then x, then y) |
| Value types | Immutable classes: `v.add(w)`, `v.scale(s)`; no operator overloading | `v + w`, `v * s`; mutable structs | JS has no operators or structs; immutability stops `transform.position.x = 1` silently doing nothing |
| Time resolution | Milliseconds everywhere (`DateTime`, `TimeSpan`, `Time`); `f` format digits beyond three print as zeros; `TimeSpan.parse` drops sub-millisecond digits | 100 ns ticks | JS `Date` and engine time are millisecond clocks; no µs/ns timing, so don't profile with them |
| Months | `DateTime.month` is 1 to 12 | Same in C# (JS `Date` uses 0 to 11) | Follows C# |
| Formatting | `DateTime.toString` uses the invariant culture (`MM/dd/yyyy`, English names) | Current culture | No culture data in the runtime |
| `Time.timeScale` | Scales `Time.deltaTime`, `Time.time` and the `dt` components get; physics isn't scaled yet | Scales physics too | Physics time scaling comes with the Rigidbody component (M5.a2) |
| Hook names | `onUpdate` (per frame), `onFixedUpdate` (60 Hz), `onLateUpdate`, `onPreRender`, `onStart`, `onDestroy` | Unity: `Update`, `FixedUpdate`, ...; s&box: `OnUpdate`, `OnFixedUpdate`, ... | JS naming; same order and meaning |
| Screen coordinates | Screen points are UI pixels from the **top-left** (like pointer events); viewport points and `Camera.rect` are 0 to 1 from the top-left too | Unity: pixels and viewport from the bottom-left | Matches the UI and pointer events, so `screenPointToRay(event.clientX, ...)` just works |
| World panel input | Screen UI takes the pointer wherever it has an element; full-screen layout boxes need `pointerEvents: none` to let the pointer through to world panels | Unity: only graphics marked as raycast targets block; s&box: similar to Platypus | The CSS way, no extra flags |
| Transform setters | Each set sends an op; world values are worked out in JS from the parent chain (shear from non-uniform scale under rotation is dropped) | Native | Rust owns the world; scripts mirror locals |

## Collisions (compiled games, 2026-10-22)

`Collision`, `Collision2D`, `ContactPoint`, `ContactPoint2D`, `Collider` and `Collider2D` follow Unity (`docs/platypus-api.md`,
physics). What is left out or differs:

| Area | Platypus | Unity | Why |
|---|---|---|---|
| `GetContacts(array)` / `GetContacts(List)` | Not there: `contacts` (a new array) or `contactCount` with `getContact(i)` (no array) | Fills the caller's array | An engine call cannot fill a script's array; `getContact(i)` is the allocation-free path |
| Keeping a `Collision` | Valid until the next contact dispatch (the next fixed tick); after that it reads as empty | A new object, unless `reuseCollisionCallbacks` (the default) reuses it the same way | Handles into one per-dispatch buffer, no allocation per contact |
| `Collision.articulationBody`, `body`, `ContactPoint2D.friction` / `bounciness`, `Collision2D.shapeCount`, `ContactPoint.thisCollider` for colliders made without a component | Not there (a `PhysicsBody.addBox` collider's `Collider` is the null handle: `gameObject` still works on the `Collision`) | Present | No articulations, no per-contact materials kept, no custom 2D shapes, every Unity collider is a component |
| `Collider.enabled`, `contactOffset`, `material`, `sharedMaterial`, `Raycast`; `Collider2D.density`, `Distance`, `IsTouching`, `OverlapPoint`, `Raycast` | Not there; friction and bounciness are `dynamicFriction` / `bounciness` on each collider class | Present | Collider components have no enable switch or material assets yet |
| `Collision2D.enabled`, `ContactPoint2D.enabled` | Always `true` | False when an effector or contact modification turns the response off | Platypus has neither |
| Impulses | `Collision.impulse` and `ContactPoint.impulse` are along the contact normals (the push this object got in the last step); `ContactPoint2D.tangentImpulse` is the friction impulse rapier keeps for warm starting, projected on the normal turned a quarter clockwise | PhysX / Box2D solver impulses | rapier's numbers; friction is solved per manifold, so every point of one carries the manifold's share |
| `relativeVelocity` | The other side's velocity minus this one's: going into the contact for Enter, as the bodies move for Stay and Exit | The velocity at the contact | Read from the bodies when the hook runs |
| Contact points | Halfway between the two surfaces; points within rapier's prediction distance count (`separation` may be a little above 0); none in Exit or trigger hooks | PhysX: on the surface, within `contactOffset` | rapier's manifolds |
| Trigger hooks | Get the other `Collider` / `Collider2D`; one whose parameter is typed `Collision` gets a `Collision` (no contact points) | Always the `Collider` | Older Platypus games typed it that way |

## Native Shapes, Spline and SplineAnimate (2026-10-22)

They were TypeScript (`js/engine`, and a compiled copy in `facade.ts`); they are native Rust now, called by compiled code.

| Area | Platypus | Before | Why |
|---|---|---|---|
| `Spline.knots` | A read-only snapshot: a fresh array of fresh `{ position, tangentIn, tangentOut, mode }` each time (edit through `setPosition`, `setTangents`, `setMode`, `insertKnot`, `removeKnot`) | The live array of the spline's own knot objects | A native spline is data in Rust |
| `Spline.version`, `destroy()` | `version` is read-only; a `Spline` is a handle number nothing frees by itself, `destroy()` frees it | A plain field; garbage collected | Handles are numbers |
| Knot indexes | A whole number in range; anything else is ignored (the old code threw on `1.5`) | Any number | Typed API |
| `TangentMode`, `SplineLoopMode`, `SplineAlignment` | Strings in scripts, indexes on the wire; `TangentMode.parse(s)` / `.name(i)` convert; a compiled call with a name that is not one reads the first value | Literal unions | Enums travel as indexes (plan decision 6) |
| `SplineAnimate` update | Once a frame after the scripts' `onUpdate`s (next to `animation::update`); only for an object active in the hierarchy | In component order with the other `onUpdate`s | A native component; a game that moves the spline in its own `onUpdate` sees the object placed the same frame |
| `SplineAnimate` in scene files | All fields saved, `spline` as an inline `Spline` asset | `spline` not saved | Decision 4 of the plan |
| `SplineAnimate` base | A handle class like `Camera` (no `extends Component`) | A `Component` subclass | A bundled game's `GameObject` is not native yet |
| `Shapes` | The namespace `Shapes`; `polygon(...points)` takes any number of points or one `...array` | A class of statics | Plan decision 5 |
| `NaN` arguments | `Shapes.bezier(.., 0)` and a `NaN` `t` give NaN points, as in JavaScript | The compiled copy answered 0 for these (a typed-array read out of range is 0) | Edge cases outside every example |

## Lists in the engine API (2026-10-22)

List properties (`SkinnedMeshRenderer.bones`, `Cloth.pins` / `sphereColliders` / `capsuleColliders`, `Camera.layerCullDistances`, `LineRenderer.positions`, `LineRenderer2D.points`, `EdgeCollider2D.points`) are native getters and setters.

| Area | Platypus | Before | Why |
|---|---|---|---|
| Assigning a list | Replaces the list; reading gives a copy, so `cam.layerCullDistances[3] = 5` changes the copy only (assign the array back) | The same for the numbers; `Cloth.sphereColliders = [..]` and `capsuleColliders = [..]` appended to the old ones | A property set is a replace, as in TypeScript |
| `Cloth.pinBox` | `ClothBox | null` (`{ min, max }` or `null` to clear), plain objects work | `ClothBox` with `Vector3` fields, `clearPinBox()` | One record property |
| `Sprite.grid(.., pivot?, pixelsPerUnit?)` | A left-out (or `undefined`) pivot or pixels-per-unit keeps the sprite's own | The facade compared them with `null`, so a left-out value overwrote them with `undefined` | Fixed in the native version |
| `QualitySettings.names`, `Scene.getRootGameObjects()` | Native lists; `levelCount`, `levelName(i)` are gone (`names.length`, `names[i]`) | Facade loops over those | |
| `Model.materials`, `Model.animations` | Native lists of handles (2026-10-23); `materialCount` / `materialAt(i)` stay | Not built natively (no glTF animations) | `examples/fog-white-room` |

## Models, animation and renderers (compiled games, 2026-10-23)

| Area | Platypus (native) | `js/engine` (QuickJS, deleted) | Why |
|---|---|---|---|
| `getComponent(MeshRenderer)` on a skinned part | Finds nothing: a `SkinnedMeshRenderer` is a renderer of its own, as in Unity | `SkinnedMeshRenderer extends MeshRenderer`, so it is found | Engine handles have no subclassing yet; Unity's shape. Ask for `SkinnedMeshRenderer` (`fog-white-room` does both) |
| Options objects (`Material.create({ mainTextureScale })`, `new X({ .. })`) | A value-class key takes a plain object (`{ x: 4, y: 4 }`, `Vector2Like`), as the TypeScript's `MaterialOptions` did | - | Was declared as the class, so `tsc` refused plain objects |
| `Animation.destroy()` | Empties the component's slot (players are numbered by place); `getComponent(Animation)` is then `undefined` | Removes the component | |
| `Animator.cullingMode` / `Animation.cullingType` (2026-10-23) | Unity's modes; "seen" is the last frame's drawing (one frame late): any renderer on or below the object in a camera's frustum, or casting into a shadow map with its shadow able to reach a view; skinned meshes judged by a box holding every bone's image of their bind-pose box (larger than Unity's bounds); every camera counts, render-texture ones (how a secondary window shows the world) included. While culled no bone transform is written, so anything on the bones stops following; Platypus's safeguard never culls an object with a physics component, `AudioSource`, `ParticleSystem`, `Light` or script component strictly below it (one warning each; rule in `animation-native.md`) | Not there (always animates) | Unity culls on the current frame's renderer bounds and has no safeguard (attached colliders go stale); one frame late keeps the renderer's results the only source |
| Culled animation and bones | While culled, bone transforms are not written, so colliders and objects attached to bones stay where they were (Unity the same for `cullUpdateTransforms`); `basedOnRenderers` pauses the legacy `Animation` (its times stand still) | - | |

## Intl backends (compiled TypeScript)

| Area | Browser / bun | Platypus today | Notes |
|---|---|---|---|
| ICU | ICU4C: every locale, `availableFormats`, all calendars | ICU4X `compiled_data`: every locale's data is linked (no trimming yet) | Planned: baked data for a main-market locale list (`icu-lite`, default) and an optional `icu-full` cargo feature using `rust_icu`/ICU4C for exact results; see `docs/research/platypus-ts-intl-datetime.md` and `tools/platypus_ts/TODO.md` |
| Date component options (`{ year, month, day, weekday, hour... }`) | Matched against CLDR `availableFormats` by ICU's pattern generator | Mapped to the nearest ICU4X field set with the requested widths applied (`toLocale*String`, `Intl.DateTimeFormat`); rare combinations differ or are rejected, see "Intl date formatting" below | Exact only with `icu-full` or a generated `availableFormats` table |
| Locale choice | Best fit among the locales the engine has | The first tag in the list, root collation/formatting when the locale has no data | The per-game locale list is not wired from `package.json` yet |

## Intl date formatting (compiled TypeScript, `icu-lite`)

What differs from the reference engine today (measured by `bench/datefmt.ts`, 15 locales, five zones;
the columns below are left out of that test). The cause is either the structural one (ICU4X formats
from fixed field sets, ICU4C picks a pattern from `availableFormats`) or a CLDR version difference.

| Case | Browser / bun | Platypus | Cause |
|---|---|---|---|
| `{ month: "short", day: "numeric" }`, `{ weekday: "short", month: "short", day: "numeric" }` in de and ja | `19. Okt.`, `10月19日` | `19.10.`, `10/19`: the month name width is not applied to a numeric field set | structural |
| `{ hour: "numeric", hour12: true }` in de; midnight in ja | `6 AM`, `午前0時` | `6 Uhr morgens`, `午前12時` | structural / CLDR |
| Numeric month, day and hour padding | per locale from `availableFormats` | looked up in a table generated from the reference engine for 31 locales (`intl_pad.rs`); other locales keep ICU4X's padding | structural (table is the stand-in) |
| `dateStyle: "short"` in pl; times in es (`6:30` against `06:30`) | CLDR of the engine | CLDR 48 of ICU4X | CLDR version |
| ja, ko and zh `timeStyle` full/long | `6時30分00秒`, `오전 6시 30분 0초`, `GMT-4 06:30:00` | `6:30:00`, `오전 6:30:00`, `GMT-406:30:00` | CLDR version |
| fr `timeStyle` long | `UTC−4` (U+2212) | `UTC-4` | CLDR version |
| `timeZoneName: "shortGeneric"` / `longGeneric` in en-GB | `United Kingdom Time` | `UK Time` | CLDR version |
| en-GB with `timeZoneName` | `3:30:00 GMT-7` | `03:30:00 GMT-7` | structural |
| Zone names for old instants (Lord Howe 1970) | `Australian Eastern Standard Time` | `Lord Howe Standard Time` | zone history in the data |
| U+202F (narrow no-break space) before AM/PM and in some patterns | plain space | replaced by a plain space, as engines do | compatibility |
| `era`, `dayPeriod`, `calendar`, `numberingSystem`, `formatRange` | supported | `RangeError` / not built | open |
| `resolvedOptions()` of `DateTimeFormat` | The full set of resolved fields | `locale`, `calendar`, `numberingSystem`, `timeZone` only | open |
| Default calendar other than Gregorian (th, fa, ar, he, ps) | that calendar's years and names | ICU4X's calendar output, the width adjustment is skipped | open |

Exact results for every option and locale need `availableFormats`: the optional `icu-full` build
(ICU4C through `rust_icu`) or a generated table; see `docs/research/platypus-ts-intl-datetime.md`.

## Compiled TypeScript (platypus_ts)

The ahead-of-time compiler gives JavaScript's answer, with these exceptions (see
[platypus-ts.md](platypus-ts.md)):

| Area | Platypus | JavaScript | Why |
|---|---|---|---|
| `Object.freeze` | Returns the object; later writes are not rejected | Writes fail silently (or throw in strict mode) | Structs have no frozen flag |
| `typeof` of a nullable | A null value reports `"undefined"`; only the literal `typeof null` is `"object"` | `"object"` for null | The compiler does not tell null from undefined |
| `JSON.parse` | Parses into the static type: fields the type lacks are dropped; a shape mismatch (wrong kind, missing property, string outside a literal union) throws a `TypeError` after the text proved to be valid JSON | Builds whatever the text holds | Values are typed structs, not dictionaries |
| `JSON.stringify` | Fields in declaration order (array-index keys first), not source order; only `?` fields are omitted when null (a `T \| undefined` field prints `null`); no replacer, reviver or `toJSON` | Own properties in insertion order; `undefined` properties omitted | Keys are fixed at compile time |
| `RegExp` groups that did not take part | `m[i]` is null, so `m[1].length` dereferences null (a fault) | `undefined`; `m[1].length` throws a `TypeError` | The compiler does not tell null from undefined |
| `RegExp` `exec` `groups` | `groups === null` is true when the regex has no named groups | `groups` is `undefined` | Same: null stands for undefined |
| `RegExp` match as an array | A match is a `RegExpMatchArray` object seen as its `(string \| null)[]` array (`m[i]`, `length`, destructuring, spread, the array methods); tuple types work elsewhere (an array when the elements agree, else an object with a field per element) | An array with extra properties | The match keeps `index`, `input` and `groups` beside its elements |
| `RegExp` replacement functions | The parameters after the match (groups, named groups) need the regex's groups known at compile time (a regex literal, `new RegExp("literal")`, or a function-local `const` from one); module-level `const` regexes do not carry them; rest parameters are a compile error | Any function, any regex | The function is inlined and its arity is checked against the groups |
| `matchAll` | Eager: an array of match objects, from a copy of the regex | A lazy iterator | No iterator objects; `for...of`, spread and `map` still work |
| Generator return type | Must be written (`Generator<T>`, `IterableIterator<T>`, ...) | Inferred | The yield type picks the generated classes |
| `yield*` | Only as a statement; a `yield` inside another `yield`'s operand is an error | An expression | Rewritten to a loop of yields |
| `return value` in a generator | Only for `Generator<T, T>` (the return type equals the yield type, or void); a finished generator without one gives the zero value | `value` is the returned value of any type | One `IteratorResult` type per yield type |
| `generator.return(v)` | Runs `finally` blocks through a marker error; `catch` clauses in generator bodies are rewritten to rethrow it | Same result | `throw(e)` is supported |
| `IteratorResult.value` when done | The zero value (0, null) | `undefined` | The compiler does not tell null from undefined |
| `Iterable<T>` annotations | Accept generators only (not arrays, strings or other iterables) | Any iterable | Maps to the generated classes |
| `Symbol()` values | `description` is null (not `undefined`) when absent; `Symbol.keyFor` gives null for an unregistered symbol; no symbols as property or Map keys; no other well-known symbols as values | Available | A prelude class, and null stands for undefined |
| Untyped generators | An error: the return type must be written | Inferred | The yield type picks the generated classes |
| `Promise<T>` | One generated class per value type; `T` must be written somewhere; `Promise<Promise<T>>` and `Promise<A \| B>` unsupported | Any value | Typed classes, no dynamic values |
| `then` | `then(f, g)`: both callbacks must return the same kind of result (both a value, both a promise, or both nothing) | Different result types per callback | The callback's return type picks the helper |
| `Promise` statics | `resolve`, `reject`, `all`, `race`, `any`, `allSettled`; `Promise.reject(e)` is the promise type the context expects (a return type, an annotation), else `Promise<number>` unless `Promise.reject<T>(e)`; the `AggregateError` message of `any` is "All promises were rejected" (bun's is empty) | `withResolvers` | Not built yet |
| `resolve()` and `resolve(v)` | `resolve()` with no value only for `Promise<void>` / `Promise<number>`; `resolve(v)` takes a value, not a promise | Any value or thenable | No thenable adoption for non-Promise values |
| Methods taken off an object literal | A method, getter or setter of an object literal may use `this` (the object, as in JavaScript); a method read off the object and called alone (`const f = o.m; f()`) keeps its binding to the object, where JavaScript has `this` undefined (strict mode) | `this` is undefined | A bound closure, as arrow properties are |
| Async iteration | `for await` over arrays and iterables works; no async generators | Supported | Async generators are not built yet |
| Top-level `await` across modules | ECMAScript order (an importer runs after the module it imports has finished, a sibling runs meanwhile; bun-compared, `M9-TS-3202..3207`). A module cycle through a late module counts the cyclic edge as finished; an importer awaits its late dependencies in import order. The host starts the registered classes, loads the start scene and runs component hooks only after every module has finished; timers, microtasks and the UI mounted before the first `await` run meanwhile; a module still pending after about ten seconds is named in the log; a rejected top-level `await` stops the startup and names the module and the error | Same order | Same order |
| `register(Class)` | A run-time call in call order, anywhere (`if`, loops, functions, after an `await`); the argument must name a class (`register(cls)` of a variable is a compile error: a class is not a value). `addComponent(Class)` works for a class that never registered; a scene or prefab naming a class that never registered gets a warning naming it | Any class value | Same |
| Unhandled rejections and escaped exceptions | Logged once as an error after the microtasks drain (`Unhandled promise rejection: ..`, the program goes on); an exception escaping a timer or microtask is logged (`Uncaught exception in a timer or microtask: ..`) (`M9-TS-2916..2919`) | Reported (`unhandledrejection`, the console) | No `unhandledrejection` event for scripts |
| Timers | Virtual time from `ts_rt_timers_advance(now_ms)` (the game clock), at least 1 ms; `setTimeout` takes no extra arguments; none fire until the host advances them | Real time, extra arguments | The host owns the clock and the loop |
| `Promise` / `async` on WebAssembly | Not available (coroutines) | n/a | Not a target in the next year |
| Coroutines on WebAssembly | Not available | n/a | Not a target in the next year |
| `Date()` without `new` | Not supported | Returns the current time as a string | Use `new Date().toString()` |
| `Date.parse` of exotic legacy strings | A few return NaN (a bare number such as `"0"`, `"Oct 19"` without a year) | Engines guess a year or a value | The legacy tokenizer is strict where engines are lax; ISO and the common legacy forms match bun |
| `Date` constructor arguments | `new Date(y, m, ...)` takes plain arguments, at most seven | Spread allowed | Lowered to a fixed-arity runtime call |
| `Date` in typed JSON | Not supported in `JSON.stringify` / `JSON.parse` types yet | Stringifies through `toJSON`; parse gives a string | The JSON descriptors have no `Date` kind |
| `Date` local time | Uses jiff's tz database; years beyond +-9999 are mapped by 400-year Gregorian cycles; a skipped local time uses the offset before the jump | Engines' own zone code | Same results as engines for the zones checked (five, against bun) |
| `Intl.Collator` options and locales | The options argument must be an object literal with literal values, and locales a string or an array of strings, both read at compile time; anything else is a compile error | Any expression | The options become a `key=value;` string for the runtime |
| `Intl.Collator` members | Only `new Intl.Collator(...)` and `.compare`; no `resolvedOptions` or `supportedLocalesOf` | The full API | Step I1 of the `Intl` work |
| Locale data | ICU4X (CLDR) data for every locale is linked in; an unsupported locale id falls back to root collation, not the default locale; the CLDR version can differ from the engine's, so rare orderings may differ | The engine's own ICU, which falls back to the default locale | No data trimming yet; checked against bun for en, de, sv, tr, ja, zh |
| Default locale | `en-US` until the host calls `ts_rt_intl_set_locales`; the game's `package.json` list is not wired to it yet | The environment's locale | Needs `platypus_app` |
| `Intl.Segmenter` | `grapheme`, `word` and `sentence` on ICU4X; `isWordLike` is `false`, not `undefined`, for non-word granularities | `undefined` there | Typed field |
| `Intl.Locale` | Getters of absent parts (`script`, `region`, `calendar`, `hourCycle`, ...) give `null`; `"und"` maximizes to `en-Latn-US` by a special case | `undefined` | Typed fields; null stands for undefined |
| `Intl.DisplayNames` | Only `region`, `script` and `language`; CLDR names can differ from bun's (for example `zh-Hans`); no `currency`, `calendar`, `dateTimeField` or `fallback: "none"` | The full API | Later steps of the `Intl` work |
| `Intl` options (`Segmenter`, `Locale`, `DisplayNames`) | Object literals with literal values, read at compile time (a `{} as X` cast is looked through) | Any expression | Shared `options_spec` |
| `Intl` not built | `formatRange`, `Intl.supportedValuesOf` (`resolvedOptions` exists on `NumberFormat` and `DateTimeFormat` with the common fields only; `formatToParts` exists on `NumberFormat` and `DateTimeFormat` only; `Intl.getCanonicalLocales` is built) | Available | Later steps of the `Intl` work |
| `Temporal` calendar and types | ISO calendar only; `Duration`, `PlainDate`, `PlainTime`, `PlainDateTime`, `Instant`, `ZonedDateTime`, `PlainYearMonth` (months and years only), `PlainMonthDay` (no `month` getter), `Now` | All calendars and types | Written in TypeScript (`lower/temporal.ts`) |
| `Temporal` names | `Temporal.X` is rewritten to the class `Temporal_X` by text before parsing, so a string literal containing `Temporal.Name` is rewritten too | Real namespace | Text rewrite in `lower/temporal.rs` |
| `Temporal` options and methods | `round` exists, but no rounding options in `until` / `since`, no `relativeTo` for `Duration.round` (time units only); no `toLocaleString`, `total` with `relativeTo`, `overflow: "reject"` (constrain only), `disambiguation` / `offset` options | The full API | Not built |
| `Temporal` zones and durations | `ZonedDateTime.until` with calendar units across a clock change uses the local date-times; `Duration` math with years, months or weeks needs a `PlainDate`; `from` of an object literal needs a literal, and a `ZonedDateTime` literal needs `timeZone` | Uses the zone's rules and the relative-to option | Simplified in `temporal.ts` |
| `PluralRules`, `ListFormat`, `RelativeTimeFormat` members | Only the constructor and `select` / `format`; no `supportedLocalesOf`, `resolvedOptions`, `formatToParts`, `formatRange` or `selectRange`; `ListFormat.format` takes a `string[]`, not an iterable; options and locales are read at compile time like the collator's | The full API | Step I4 of the `Intl` work |
| `Error` `cause` | `cause: Error \| null`: only an `Error` can be a cause; an absent cause is null, not told apart from `undefined` (test with `== null`) | Any value; `undefined` when absent | Typed field; null stands for undefined |
| `Error.stack`, `captureStackTrace` | `stack` is the error's first line (`toString()`), with no frames; no `captureStackTrace` | The first line, then the call frames | No stack capture in compiled code |
| `Intl.NumberFormat` options and locales | Same as the collator: an object literal with literal values, locales a string or an array of strings, read at compile time; `Number.prototype.toLocaleString` takes the same arguments | Any expression | Shared `options_spec` |
| `Intl.NumberFormat` members | Only `new Intl.NumberFormat(...)` and `.format`; `formatToParts` exists; `resolvedOptions` gives `locale`, `numberingSystem`, `style`, `currency`, `notation`, `signDisplay` and the digit counts (the numeric ones are 0, not absent, when they do not apply); no `formatRange` or `supportedLocalesOf`; Number values only (no BigInt, no strings) | The full API | Step I2 of the `Intl` work |
| `NumberFormat` `style: "unit"`, compact currency | `style: "unit"` raises a `RangeError`; `notation: "compact"` with `style: "currency"` is not built | Supported | Not yet done |
| `NumberFormat` `useGrouping: "always"` | Does not force grouping in locales that group only from five digits (es, pl, pt-PT) | Forces grouping | ICU4X's decimal formatter treats `always` as `auto` |
| `NumberFormat` NaN text | `"NaN"` in every locale except `ar` and `ru`, which are localized | Localized per locale | Only those two locales were handled |
| `NumberFormat` currency code after the number with NaN or infinity | A space is put before NaN or infinity (`KWD NaN`) | `KWDNaN` | The placeholder swap does not copy the engine's automatic spacing rule |
| Typed arrays | `Int8Array` .. `Float64Array` and `Uint8ClampedArray`, with `ArrayBuffer`, `DataView`, `subarray`, `buffer`, `byteOffset` and `new X(buffer, offset, length)`. No `SharedArrayBuffer`, resizable buffers, transfer, `Atomics`, `DataView` bigint accessors or `BigInt64Array` / `BigUint64Array`; a read past the end gives 0, not `undefined`; no `constructor` property; `X.of` and `Array.from(typed)` are missing | Views share an `ArrayBuffer`; out-of-range reads give `undefined` | Views are headers pointing into the owner's block; missing values are 0 as for `number[]` |
| `.buffer` | An owning typed array returns a new `ArrayBuffer` object each call over the same memory, so `a.buffer === a.buffer` is false | The same object | The buffer is a prelude object made on demand |
| `DataView` errors | Out-of-range access is a `RangeError` with Platypus's own message | bun's message | Messages not matched |
| `structuredClone` | Arrays, objects and typed arrays are copied; a class instance keeps its class and a derived class's own fields stay shared; `Map`, `Set` and functions are compile errors; cyclic data does not end | A plain object for a class instance; handles Map, Set and cycles | One generated clone function per static type |
| `WeakRef`, `WeakMap`, `WeakSet` | `WeakRef<T>` exists for classes of the program and holds its target strongly (`deref()` always finds it); `WeakMap` and `WeakSet` are `Map` and `Set`; no `FinalizationRegistry` | Weak references | A collector may keep weak targets as long as it likes; keeps the types simple |
| `globalThis` | Not available | Available | Globals are program variables, not properties of an object |
| Callback return literals | A callback returning a string literal is typed `string` | The literal type | Inferred callback return types widen |
| BigInt | `bigint` is a leaf object on `num-bigint`. Not built: `BigInt64Array` / `BigUint64Array`, `toLocaleString`, bigints as `Map` / `Set` keys, bigints in `JSON` types (a compile error). Mixing a bigint with a number in arithmetic, unary `+` and `>>>` are compile errors. | Mixing, unary `+` and `>>>` throw a `TypeError` at run time; `&&` / `\|\|` give an operand | Types are checked at compile time |
| `Object.assign` / `{ ...dict }` into an object | Copies the properties of objects and the entries of dictionaries into the target's fields by name; a key that is not a field of the target cannot be added (fixed layout): a `console.warn` names the key and the struct once per call site and the entry is skipped; a dictionary value of a type the field cannot hold is a `TypeError` | Adds the property | Objects are fixed structs |
| `Object.keys/values/entries`, `in`, `hasOwn` | Keys are the struct's fields, fixed at compile time; `hasOwn` and `in` need a string literal key | Keys are looked up at run time | The program is typed and closed; run-time dictionaries need `Map` |
| `null` and `undefined` | One value: a missing field, `let x: T \| undefined`, `?.` on null and an absent optional parameter are all the same null; `x === undefined` is `x === null` | Two values | Typed slots have one empty state (the dynamic-values plan, `docs/research/platypus-dynamic-values.md`, keeps them apart) |
| A null property access (`a.b` with `a` null, also after `a!`) | Reads through a null pointer: an access violation that ends the process (the crash dialog), not a catchable error | A `TypeError` that `try` / `catch` can catch | No null checks before loads yet (`x!` is a plain cast); planned (tools/platypus_ts/TODO.md) |
| `let x: number;` without a value | Starts as 0 (an object type: null) | `undefined` | Typed slots start zeroed |
| `==` / `!=` | The same as `===` / `!==` (no coercion between types) | Loose equality | `tsc --strict` code rarely relies on it |
| `any`, `unknown`, `never`, conditional and mapped types | A compile error that names the line (`` `unknown` is not supported yet ``); `x as any` where `any` cannot lower is a no-op cast | Dynamic values and type-level computation | Not built: the dynamic-values plan |
| A template-literal type (`` `url(${string})` ``, `Cursor`'s open forms) | Lowered as `string` and not checked by the compiler; `tsc` checks it | Checks the string's shape | Only `tsc` sees the pattern; `M9-TS-3525` |
| Union member order (`number \| string` against `string \| number`) | Two tagged unions that do not convert into each other | The same type | Tags follow the written order; not built (T8d in tools/platypus_ts/TODO.md) |
| Generic methods overridden in a subclass | The base class's copy is called, not the override (no virtual dispatch of generic methods) | The override | Each specialisation is a separate function |
| Generic constraints (`T extends X`) | Not checked (the compiler trusts `tsc`) | Checked by `tsc` only (erased at run time) | Same as JavaScript at run time; a program `tsc` rejects may still compile |
| Interfaces declared twice (declaration merging) across files | Renamed apart by the module graph, not merged | One merged interface | The module graph renames top-level names per file |
| Deep recursion | Not checked: the native stack overflows (a crash, not a `RangeError`) | `RangeError: Maximum call stack size exceeded` | No stack-depth guard yet |

## SVG (M3d step 5.1)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Rendering | Rasterized on the CPU with resvg at the drawn size (cached; re-rasterized when content, style or size changes) | Native vector rendering | Full SVG support quickly; GPU paths are a TODO ("Later") |
| Animation | CSS transitions/animations on SVG content work (each frame re-rasterizes); SMIL (`<animate>` ...) is ignored; `d` only morphs between paths with the same commands | SMIL and path morphing | resvg has no SMIL; see TODO "Later" |
| Hit testing | The whole `svg` box; shapes inside aren't targets and `pointer-events` on shapes is ignored | Per shape | Content isn't laid out by Platypus |
| `<style>` inside SVG, `class` in markup | Ignored; Platypus stylesheets style SVG content instead | Supported | Styles are resolved by Platypus's cascade |
| Text | The bundled Inter only | System and web fonts | resvg gets one font database |
| Images in SVG (`<image>`), filters | `<image>` isn't loaded from the game folder; SVG filters render via resvg | Supported | Not wired yet |

## Stylesheet authoring (M3d group 10)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Nested at-rules | `@media` inside a rule works; other at-rules there (`@starting-style`, `@supports`, ...) are skipped with a warning | Allowed | Top-level `@starting-style` works |
| `@import` conditions | Media queries work; imports with `supports()` or `layer()` are skipped; `@import` anywhere in a sheet still loads (browsers ignore late ones) | Conditional and layered imports | No `@supports` or cascade layers |
| `@media` | Size, aspect ratio, orientation, resolution, colour scheme, reduced motion, pointer/hover; `em` in queries is 16px; `prefers-reduced-motion` stays `no-preference` and `pointer` stays `fine` until the app reports otherwise; unknown features never match | Many more features (`color`, `display-mode`, ...) | The ones games use |
| Nested rule order | Flattened after the parent rule; declarations written after nested rules still belong to the parent | Same (the spec's "nested declarations" rule) | — |

## Game extensions (M3d group 9)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| `sound-in` / `sound-out` | Not supported | s&box only (not CSS) | CSS stays about display; play UI sounds from event handlers |
| UI scale | `UiScale.height(1080)` / `fit` / `fixed` zooms all screen UI (layout in UI pixels) | CSS `zoom` per element | One switch for the whole HUD, like s&box's ScreenPanel |
| `background-image-tint`, `border-image-tint` | s&box properties, no CSS equivalent; multiply image pixels by a colour; gradients and masks aren't tinted | — | Common in game UI (one white icon, many colours) |

## Floats (`docs/ui-tree.md`, "Floats")

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Container | Only `display: block` / `flow-root` containers lay floats out; boxes are `flex` by default, so set `display: block` on the container | Every `div` is block | Platypus's default `display` is Taffy's `flex` |
| Floats in text | A float is a block child placed before the text that wraps around it (also with bare text in the same block); a float between runs of one `text` element is not supported | Floats anywhere inside inline content | Text children are flattened to runs |
| Line shortening | Lines are shortened by floats their height touches; a word too wide for the room moves the line below the float | Same | — |
| `shape-outside`, `float: inline-start/end` in rtl | Not supported; `inline-start/end` mean left/right | Shaped wrap; logical sides | Taffy places rectangles |
| Minimum content width | Sizing a container by its text (`min-content`) ignores floats beside it | Counts floats | Intrinsic sizes are measured without a formatting context |

## Tables (`docs/ui-tree.md`, "Tables")

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Elements | None: a grid of `display: contents` rows (`examples/table`) | `table`, `tr`, `td`, `display: table*` | The grid already covers content-sized columns, wrapping, spans and striping |
| Not available | `display: table*`, `border-collapse` / `border-spacing`, `<col>` widths, `position: sticky` headers, `vertical-align` on cells (use `align-content` on a grid cell), a box for the row (`contents` rows have no rect, background or border) | Yes | — |
| Spare width | Auto columns grow equally up to their content size | In proportion to their max-content widths | Grid track sizing |

## Scrolling extras (M3d group 8)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| `scrollbar-gutter: stable both-edges` | Reserves the end edge only | Both edges | Taffy reserves one side |
| `scrollbar-gutter` on the x axis | Only vertical scrollbars get a gutter | Same (gutters follow the block axis' bar) | — |
| `overscroll-behavior: none` | Same as `contain` | Also disables bounce / pull-to-refresh | Platypus has no overscroll effects |
| `scroll-padding` | px lengths only (no percentages); `auto` is 0 | Percentages of the scrollport; `auto` is UA-chosen | No UA padding; percentages unneeded so far |

## Transitions (M3d step 7a)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| What interpolates | Computed values as text, token by token: same-shaped values only (same functions, same number of shadows/layers); mixed units via `calc()` | Typed interpolation per property (e.g. transforms with different functions go through matrices) | One generic interpolator covers most real uses |
| `auto`, `currentColor`, keywords | Change at once | Some interpolate (`currentColor` in colours, `interpolate-size`) | Not representable as plain tokens |
| Colour space | Premultiplied sRGB | Same for legacy colours; Color 4 spaces otherwise | Matches what games usually expect |
| Shorthands | `transition: background 1s` also covers `backgroundColor` and the reverse (name prefixes) | Exact shorthand expansion | No longhand tables |
| `transition-behavior: allow-discrete` | Accepted, no effect | Discrete properties flip at 50% | Rarely needed |
| Inherited values | A transition on a parent's `color` animates; children follow through inheritance | Same | — |
| Animations | `@keyframes` by name from any stylesheet (the last wins); no `animation-composition`, `animation-timeline`, `animation-range` or the Web Animations API | Full | Enough for UI motion |
| `@starting-style` | Top-level blocks only; applies to new elements and to an element whose own `display` leaves `none` (not to its descendants) | Also nested in rules, and descendants of a newly shown ancestor | Nesting comes with group 10 |
| `:outro` | s&box's exit state (no standard equivalent): removed elements linger while it animates; structural selectors still count them | — | Lets exit transitions finish without reconciler work |
| Cost | Each moving element is restyled (selector matching included) every frame | Browsers update animated values without a full restyle | Simple; fine for UI-sized numbers of elements |

## Dialogs (M3d step 5j)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Opening | `open` and `modal` attributes (React state) | `show()` / `showModal()` methods; `modal` isn't an attribute | Declarative fits React; scripts own the state |
| Closing | Escape / backdrop clicks send `cancel` then `close`; the script clears `open` | The browser closes the dialog, then fires `close` | The engine never changes React-owned attributes |
| UA style | Centred with `left/top: 50%` + `translate: -50% -50%`; no border, padding or background | `margin: auto`, `fit-content`, a border, padding and a canvas background | Platypus's elements are unstyled; authors setting `translate` or insets take over the centring |
| `:modal`, `popover`, `returnValue`, `inert` | No `:modal` (use `dialog[modal]`), no popovers, no return value; outside the modal only focus and pointer input are blocked | All supported | Not needed yet |

## Cursors (M3d step 5i)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Disabled fields | `not-allowed` | The arrow (`default`) | Clearer in game UI; override with `cursor` |
| Image formats | PNG, JPEG, `.cur` | Also SVG, `.ani`, `image-set()` | SVG comes with step 5.1; animated cursors aren't planned |
| Keyword shapes | winit's nearest cursor per platform | Each browser's own set | Some keywords (e.g. `vertical-text`, `alias`) fall back to similar shapes on Windows |

## Selection (M3d step 5h)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Extent | A selection stays inside one text block (one `text` element and its spans) | Selections run across elements | Each block has its own Parley layout; cross-block ranges need a document-wide position model |
| `::selection` | Only `color` and `background(-color)`; each comes from the nearest element (the block or an ancestor) with a `::selection` rule that sets it; spans inside the block can't restyle their part | Highlight inheritance per element, including spans | Enough for theming; per-span styles need per-run highlight drawing |
| `user-select: contain` | Same as `text` | Keeps the selection inside the element | Selections can't leave a block anyway |
| Ctrl+A outside fields | Does nothing | Selects the whole page | No page-wide selection (see Extent) |
| Default | Text is selectable, as in browsers | Same | Set `userSelect: "none"` on menus and HUDs where double-clicks shouldn't highlight |

## Lists (M3d step 5g)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Outside markers on `text` items | Drawn inline, like `inside` | Hang outside the line box | A text element's children are inline content; the marker can't be positioned outside it. Wrap the text in a box item for hanging markers |
| Elements | `list` + any element children; no `ul`/`ol`/`li`/`p`/`h1` | HTML elements | Platypus's element set stays small; use `list` and `text` with classes |
| `list-style: none` | The list's default 24px indent goes too (flush left), unless the author sets its start padding | The UA's `padding-left: 40px` stays; sites reset it by hand | What most pages want; set `paddingLeft` to keep an indent |
| Counters | Markers number items by position; no `counter-reset` / `counter-increment` / `counter()`, `start` or `reversed` | Full CSS counters | Not implemented yet |
| `::marker` | All properties apply | Only a few (colour, font, content, ...) | More permissive |

## Transforms (M3d step 5e)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| `transform-style: preserve-3d` | Children's 3D transforms combine before flattening (each plane lands exactly), but they still paint in tree order | Planes in a 3D context are depth-sorted and intersect | No depth buffer or plane splitting; order elements by hand (or with `z-index`) |
| Text under rotation, skew or perspective | Glyph masks are sampled nearest-neighbour in the element's plane | Re-rasterized glyphs | Turned text looks slightly jagged; fine for motion, softer than a browser at rest |
| Text under a scale and move only | Re-rasterized at the on-screen size every frame and placed in window space, as at rest | Re-rasterized at the end; during a transition the cached text is composited (scaled bitmap) | Scaled and sliding text stays sharp throughout, and there is no pop when a transition ends; costs a few extra glyph rasters (cached per 0.25 px size) |
| Anti-aliasing under scale/perspective | One window pixel of feathering is computed in the element's plane | Exact | Strongly scaled-down or steep planes get softer or harsher edges |
| `visibility: collapse` | Same as `hidden` | Removes table rows/columns | Platypus has no tables |
| `position: fixed` inside a transformed element | Still fixed to the window | The transformed ancestor contains it | Fixed nodes paint at the top level |
| Points behind the viewer | A plane crossing behind the eye (w ≤ 0) can draw wrongly | Clipped at the eye plane | Rare with sensible `perspective` values |

## Effects (M3d group 5)

| Area | Platypus | Browsers / spec | Why, and what fixing it would take |
|---|---|---|---|
| `filter` order | Always blur, then all colour functions (folded into one matrix), then `drop-shadow()`s, whatever order is written | Each function applies in written order | One layer pass, one matrix. A strict chain needs a pass (or an intermediate layer) per function. `brightness(0.5) invert(1)` and `invert(1) brightness(0.5)` *are* ordered correctly: it's only blur and shadows that move |
| Several `drop-shadow()`s | All drawn beneath the content, the first listed on top | Each shadows the result so far (a later shadow also shadows earlier shadows) | Rarely visible; would need one layer per shadow |
| Mask modes | The top mask layer's `mask-mode` applies to every layer | Per layer | Masks render into one texture; per-layer modes need one texture per mode, or converting luminance to alpha while drawing each layer |
| `mask-composite` | Layers combine with normal alpha blending (≈ `add`) | `add`, `subtract`, `intersect`, `exclude` | Not implemented |
| `mask-clip` / `mask-origin` | Always the border box | Configurable | Not implemented |
| Soft fades and translucency | Translucent colours, opacity groups, blurs and shadows blend on **sRGB values** by default, like browsers (25% white over `#3060c0` is `100,136,208`). `"blending": "linear"` in `project.json` (or `set_blending` on the renderers) blends in linear light instead, as Platypus did before (`142,157,210`; a faint fade, alpha 0.025 over black, is ~42/255 grey instead of ~6) | sRGB | Linear light is kept as a choice for games tuned to it. In both modes the 3D world itself is lit and blended in linear light. Images are filtered between texels in linear light (sub-pixel difference); the CPU renderer is always sRGB. See `rendering.md` "Blending" |
| Filter colour space | Colour filters work on sRGB values (like browsers) | Spec default is linearRGB (`color-interpolation-filters`) | Matches what browsers actually do |
| `backdrop-filter: blur()` edges | Only the backdrop inside the element's box is blurred (like browsers); at the box's edges the blur reflects the backdrop (wgpu and GLES; the CPU renderer repeats the edge pixels instead) | Chrome mirrors the backdrop at the edges | The CPU renderer's box blur still clamps; the difference shows only as a slightly flatter fade near the edges |

## Backgrounds and borders (M3d group 3)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| `background-origin` | Images and gradients position against the **border box** | Default `padding-box` | Not implemented; `background-clip` is |
| `radial-gradient()` shape and position | `circle` or `ellipse`, with `at` a position (`circle at 30% 20%`); always sized to the farthest corner from the centre (`closest-side` and other size keywords don't parse) | Any size keyword, length and `at` position | Shape and `at` parsed in `paint.rs`; the centre drawn in `gradient.rs` and the CPU renderer |
| Gradient stops | The first 8 stops are drawn (a two-position stop like `red 0 45deg` counts as two); the rest are dropped with a warning | Unlimited | The shader's stop table is fixed (`MAX_STOPS`) |
| `background-repeat: space` for gradients | The gaps aren't kept (tiles repeat edge to edge) | Gaps as for images | Gradient tiles repeat in the shader without the gap logic images have |
| `border-style` default | `solid`: a width alone draws a border (as in s&box) | `none`: no border without a style | Matches s&box authoring; the `border` shorthand still follows CSS (no style, no border) |
| Gradients in `border-image` | Pieces stretch; `repeat`/`round`/`space` don't tile them | Tile like images | Image pieces tile; gradient pieces don't yet |
| `clip-path: url()` | Unsupported | References an SVG `<clipPath>` | Platypus has no SVG documents |
| Blurred shadows on elliptical corners | Drawn with circular corners (the mean radius) | Elliptical | The analytic blurred-box shader is circular-only |
| Background colour with `background-clip: text` | Dropped | Clipped to the text | Only the topmost gradient layer fills text |

## Drag and drop (M5.e)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| Drag image | The element drawn live at 0.6 opacity, so it must stay rendered | A snapshot taken at `dragstart` | No offscreen snapshot of a subtree yet |
| `getData` | Works in every drag event | Only in `drop` (protected mode elsewhere) | The data stays in the scripts, so there's nothing to protect |
| `dragover` | Sent when the pointer moves | Also every ~350 ms while still | Nothing changes without movement here |
| `dropEffect` / `effectAllowed` | Kept for scripts; the cursor only shows accepted (`grabbing`, `copy` for files) or not (`no-drop`) | Pick the cursor and filter drops | Enough for the editor; easy to add |
| `dragenter` | Doesn't need to be prevented | Must be prevented (with `dragover`) to accept | Fewer handlers for the same result |
| System drags | Files only (paths); on Windows the position comes from `GetCursorPos`, elsewhere the last pointer position | Files, text, URLs, live position | winit 0.30 reports files without positions |

## Events and attributes of elements

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| `pseudoElement` of transition and animation events | Always `""`; the event goes to the element | The pseudo-element's name (`::before`) | Generated boxes are not event targets |
| `animationiteration` / `animationend` `elapsedTime` | `n * duration` and the active duration, as in the spec | Same | — |
| `onFocus` / `onBlur` | Listen to `focusin` / `focusout` (they bubble); `focus` / `blur` are never sent | Same in React; the DOM has both | React semantics |
| `relatedTarget` | `{ id }` of the other element | The element | Scripts hold ids, not elements |
| `title` | Stored on the element (`[title]` selectors work) | Shows a tooltip | No tooltip UI yet |
| `cols` | A textarea is `cols` x 8 logical pixels wide when CSS sets no width | `cols` average characters of the font | Fixed character width, like the default sizes of fields |
| `type` on `input` | Stored lower-cased (`input[type="password"]` selectors work); every input is a text field | Changes the control (masking, numbers, checkboxes) | Only text editing exists |
| `dataTransfer` | See "Drag and drop" | | |

## Layout (M3d group 2)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| `box-sizing` default | `border-box` (as in Taffy and s&box) | `content-box` | Matches s&box |
| `z-index` | Orders siblings only; every z-indexed element is its own stacking context | Stacking contexts nest; z-index can lift an element above its parent's siblings | Simpler model; `isolation` is accepted and has no effect |
| `position: fixed` | Ignores ancestor opacity, filters, transforms and clips (but not `display: none` / opacity 0); a transformed or filtered ancestor doesn't contain it | Ancestors with transforms/filters (and `perspective`, `contain`, `will-change`) are its containing block | Fixed nodes are laid out by the root and paint at the top level |
| `contain` | Only `layout` and `paint` containment (`strict` / `content` mean both); `size`, `inline-size` and `style` are accepted and ignored | Size containment sizes the box as if empty | Taffy has no size containment |
| `will-change` | Only makes a containing block (when it names `transform`, `filter`, `perspective`, `backdrop-filter`, ...); no stacking or layer hints | Also a stacking context and a compositing hint | Nothing to promote |
| Absolute boxes inside an opacity / blend / mask group inside a clip | A static `overflow` clip outside the group still clips an absolute box inside it whose containing block is further out | The box escapes the clip | The group is composited (and clipped) as one layer; clips inside the group, or groups outside the clip, are fine |

## Text (M3d group 4)

| Area | Platypus | Browsers / spec | Why |
|---|---|---|---|
| `text-decoration-skip-ink` | Parsed, no effect | Underlines skip descenders | Needs glyph outlines in the decoration pass |
| `text-overflow: ellipsis` | Cuts whole glyphs; uses the block's style for the `…` | Same, but uses the style at the cut | Close enough for UI text |
| `background-clip: text` | Fills the element's own text only, with the topmost gradient layer | Descendants' text too, all layers | |
| `text-decoration` | Inherited (it propagates visually anyway) | Not inherited, but propagates | Same result in practice |
