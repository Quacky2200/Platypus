# platypus_ts: working file

Tracks the ahead-of-time TypeScript compiler (`tools/platypus_ts`, `tools/platypus_ts_rt`,
`tools/platypus_ts_tests`). The repo-wide plan is `TODO.md` M9.b2; **this file is where progress is
kept up to date** and where a fresh session (or a context after compaction) starts. Update the
"Now" and "Log" sections at the end of every step. Design and measurements:
`docs/research/platypus-ts-spike.md`. Test IDs: `TESTS.md` section "M9.b" (`M9-TS-*`, next free: **731**).

## How to pick this up

- Branch `claude/recursing-wright-856ffa` (worktree `.claude/worktrees/recursing-wright-856ffa`).
  Commit hashes were rewritten once by another session; do not rely on old ones.
- Do not touch `crates/platypus_render` or `crates/platypus_app` (another session works there).
- Shared notes folder for hand-overs: `C:\Users\quack\AppData\Local\Claude-Data\claude_pzs8sxrjxfjjc\side-session-notes\local_988df2ba-1803-40db-870f-74c0086082a3`.
- Shell: `export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"`. `tools/` is its own cargo workspace;
  run cargo from `tools/`.

```bash
cd tools && cargo fmt --all
```
```bash
cd tools && cargo clippy --workspace --all-targets -- -D warnings
```
```bash
cd tools && cargo test --workspace
```
```bash
cd tools && PLATYPUS_TS_GC=stress cargo test -p platypus_ts_tests
```
```bash
cd tools && cargo run --release -p platypus_ts_tests --bin bench
```
```bash
cd tools && cargo run --release -p platypus_ts_tests --bin sine_bench 2000
```

- Every change that touches lowering, codegen or the runtime is also run with `PLATYPUS_TS_GC=stress`
  (collect at every allocation, poison freed memory). It has caught real bugs three times.
- CLI: `tools/build/release/platypus_ts.exe file.ts --dump-ir [--prefix p_] [--struct-base N] [-o out.obj]`.
  Build it with `cargo build --release -p platypus_ts`.
- Commits: one per step, message ends with `Co-Authored-By: Claude Sonnet 5.5 <noreply@anthropic.com>`;
  run fmt, clippy, tests (and the zombie fixture's `bun test` if `fixtures/` changed) first; update
  `TESTS.md` and `TODO.md`; keep `docs/research/platypus-ts-spike.md` current.
- Machine gotchas: bash heredocs mangle apostrophes and backslashes. For multi-line edits write a
  Python script to the scratchpad with the Write tool and run it (that is how most edits here were
  made). Files are CRLF on disk: read with `newline=''` and keep the line ending. rustfmt reflows
  long lines, so patch text that matches the formatted code. A running `cargo build` of the same
  target locks files; do not edit during a background build.
- Panics inside `extern "C"` abort the process; test fallible logic through a Rust-level function.

## Map of the code

| Path | What |
|---|---|
| `platypus_ts/src/lower/mod.rs` | program-level lowering: declarations, `ensure_lowered`, bindings (`Binding::Direct/Boxed`), `lower_body_into` |
| `platypus_ts/src/lower/types.rs` | annotations to IR types, structs, `coerce`, `to_bool` |
| `platypus_ts/src/lower/expr.rs` | expressions, places (assignment targets), calls, `Math` |
| `platypus_ts/src/lower/stmt.rs`, `arrays.rs` | statements; array methods with inlined callbacks, `Iter`, `emit_return` |
| `platypus_ts/src/lower/{strings,object,maps,json,regexp,date,intl,prelude}.rs`, `throws.rs` | strings and numbers; `Object.*`/spread; `Map`/`Set`; typed JSON; `RegExp` and the regex string methods; `Date`; `Intl`; TS prelude classes (`Error`s, `RegExp`, `Date`, `Intl_*`); which functions can throw |
| `platypus_ts_rt/src/{string,string_more,map,json,regexp,date,intl,intl_number}.rs` | runtime: WTF-8 strings, collections, JSON, `regress`, `jiff`, ICU4X |
| `platypus_ts/src/lower/scan.rs`, `closure.rs` | which variables closures capture; lifting closures to functions, function values |
| `platypus_ts/src/ir.rs` | the IR and its dump |
| `platypus_ts/src/frames.rs` | shadow-stack frame plan; **the spill rule is the GC's correctness argument** |
| `platypus_ts/src/codegen.rs` | IR to Cranelift object (frames, spills, `eval_args`, registration in `ts_init`) |
| `platypus_ts/src/layout.rs` | generated Rust bindings (`HDR_*`, `TAG_*`, structs, `extern "C"`) |
| `platypus_ts_rt/src/lib.rs` | GC heap, roots, arrays/strings/closures/cells, libm calls |
| `platypus_ts_tests/` | `build.rs` compiles `fixtures/sim.ts` plus `bench/{sine,gap,closures,globals,loops,switches,destructure,params,mathlib,arraylib,classes,inherit,props,engine}.ts` (prefix and struct base each); `tests/{sim_port,gc,closures,globals,loops,switches,destructure,params,mathlib,arraylib,classes,inherit,props,engine}.rs`; `src/bin/{bench,sine_bench}.rs` |

Adding a test module: a `bench/<name>.ts`, an entry in `build.rs`'s list `(name, prefix, struct_base)`
(bases are 1000 apart: sim 0, sine 1000, gap 2000, closures 3000, globals 4000, loops 5000, switches 6000, destructure 7000, params 8000, mathlib 9000, arraylib 10000, classes 11000, inherit 12000, engine 14000, scene 15000, strings 16000, errors 17000, stdlib 18000, collections 19000, json 20000, regexp 21000, regexp2 22000, date 23000, intl 24000, numfmt 25000, next 26000), a `pub mod` in
`src/lib.rs`, a test file. Expected numbers come from running the same TS in bun.

## Rules we decided (do not undo without asking)

- A box in `TODO.md` is ticked only when the whole item is done; finished parts are ticked sub-items.
- Items marked **STOP** get a written design note and the user's review before code.
- GC roots: a pointer held while a later operand can allocate is spilled to a frame slot unless it is
  a local no later operand assigns, a global or null (`frames.rs`). Any new IR node that evaluates
  operands must use `eval_args` and be counted in `frames.rs` (`count_expr`/`count_stmts`,
  `may_alloc`, `children`).
- Each module linked into one program gets its own `struct_base`; headers carry the heap id.
- Numbers are f64 everywhere; `number | null` uses a reserved NaN payload.
- Measured claims go in the doc with how they were measured; correct the doc when one turns out wrong
  (the "33 bullets in flight" claim was wrong and was fixed).

## Now

**State (2026-10-19, end of a long session; read this first after a compaction):**

- Committed on this branch (newest last): components for the Rust host (`NativeHost`), generics (monomorphisation), `platypus_ui` typed `Op`,
  `platypus_ui_native` reconciler (hooks, keyed diff, effects in React's order, portals, events, controlled fields, `refresh`),
  `platypus_ts_ui` bridge + JSX/hooks rewrite (`jsx.rs`, `ui.ts`), QuickJS bindings for values and strings, performance review
  (`docs/research/platypus-ts-sine-demo.md`), `platypus_engine_api` (Color, Time, input, Gizmos, Camera, Screen, QualitySettings), namespace
  properties, compiled calls of those (`M9-TS-593..595`). Next free test ID **731**.
- **Uncommitted at the time of writing** (checks were running; re-run fmt, clippy, `cargo test --workspace`, `PLATYPUS_TS_GC=stress`, then commit):
  the whole `SineDemo` compiling and running (`bench/sine_demo_full.ts`, `tests/sine_demo_full.rs`, `M9-TS-596..597`), `compile_check` bin,
  `engine_objs.rs` (`new GameObject(..)`, `addComponent(Camera, {..})`), options objects for engine calls, `Record<"a"|"b", V>`,
  `boolean | undefined` (`bench/nullbool.ts`, `M9-TS-598..600`), `platypus_engine_api::audio`, optional-chain literal widening, line numbers on
  statement errors, `register({ A, B })`, field-type inference from API reads, `docs/research/platypus-native-engine.md`, TESTS/doc updates, and the
  build-size fix below. `build.rs` soft-fails only `sine_demo_full` (it compiles now; the guard can go).
- **Disk finding (user report: PC chugging).** The bottleneck was the disk, not RAM/CPU: `tools/target` held 1,109 PDBs = 86 GB (56 test
  executables, each with its own debug info) and the root `target` 8.6 GB. Fixed: `debug = false` for `[profile.dev]`/`[profile.test]` in
  `tools/Cargo.toml`, `debug = "line-tables-only"` in the root `Cargo.toml`; `tools/target` deleted and rebuilt. The 56 test files became one binary (one link of ~500 MB instead of 56 in parallel, which was the memory spike); since 2026-10-22 they are five by area (`platypus_ts_tests/tests/{lang,stdlib,engine,engine_api,ui}/`, `tests/common/cases.rs` shared); tests that run compiled code take `platypus_ts_tests::serial()` because module globals are shared between threads. Run one with `cargo test -p platypus_ts_tests --test <group> <name>`. Still possible: `incremental = false`.
- **Answered (2026-10-19, `platypus-native-engine.md` section 5):** keep `tsc` on the generated `.d.ts` (a `platypus_ts` checker much later); QuickJS stays as a legacy/mod path; `.d.ts` are generated by the `platypus_ts` tool, not the engine. **Review near the end (differences):** `getComponent(Base)` finds exact classes only; a component added by `addComponent` gets `onAwake` when the calling hook returns; component ids continue across `NativeHost` instances on a thread; QuickJS has no handle classes yet.
- **Port to native, batch 1 (2026-10-19):** `random`, `layers`, `log`, `prefs`, `assets`, `app`, `clock` (TimeSince / TimeUntil / Time.realtimeSinceStartup) are Rust (`platypus_engine_api`), bun-compared in compiled code (`M9-TS-605..626`, `bench/add_engine_module.py` registers a new program) and tested in QuickJS (`M9-API-37..43`). Infrastructure added: `#[optional]` parameters, `#[js_name]`, `new Class(args)` for handle classes. `input` and `time.ts` (`TimeSpan`, `DateTime`) are done too (`M9-TS-627..650`, `M9-API-44..45`). `Task` is a compiler prelude over `setTimeout` (`M9-TS-651`). `math.ts` is done too, as `crates/platypus_math` (`M9-TS-652..674`, `M9-API-46..47`; the scene and the rest of the engine API use its f64 types now). **Left of the batch:** closure-taking APIs (`input.on`, `Assets.onChanged`, `Application.onFocusChanged`). **Scene (design `docs/research/platypus-native-scenes.md`): steps 1 to 3 done: GameObject / Transform members (`M9-TS-698..702`), component lookups and lifecycle (`703..711`), `instantiate`, prefabs, `SceneManager` and the scene file loader/saver in `crates/platypus_scenes` over `crates/platypus_kinds` (`712..718`). Left: reload state (compiled programs restart instead), the `Light` / `MeshRenderer` / physics kinds as those are ported, scene events and `loadSceneAsync` as script callbacks (closure APIs), `$asset` / `Material` / `Texture` values.** Differences to review: components of one object are listed kind by kind (not in add order); `live` references exist only in `instantiate` data; `GameObject.scene` is `SceneManager.sceneOf(go)`; lists are count plus indexer. **Animation: phases 2, 3 and 4 done (curves, clips, `Animation`, `Animator`, builder: `M9-TS-676..697`; `docs/animation-native.md`)**; earlier detail: (curves, clips, sampling: `M9-TS-676..683`; the `Animation` component: `M9-TS-684..691`; the classes-plus-builder shape was agreed, see the note's section 8); left: `fromPoints` (arrays), more components in the registry, `platypus_app` wiring. (design note `docs/research/platypus-native-animation.md` waits for the user: builder shape for the controller and events, `@property`-only animation of compiled components, arrays across the boundary first). Then scenes, physics, audio, rendering components per the order the user gave. Review near the end: the API differences listed in `docs/platypus-api.md` ("The first batch").
- **Waiting for the user:** `docs/research/platypus-native-engine.md` ("nothing left in TypeScript but generated types": animation, scenes, math,
  physics/audio components, app/time/input to Rust; games without `node_modules`); `docs/research/platypus-ui-native.md` was approved and is built.
- **Next, in order:** (1) commit the above; (2) done: `@property` save/load and `getComponent`/`addComponent` (`M9-TS-601..604`; `getComponent(Base)` is exact-class only); (3) a union of classes (an array of `Tween<number>`, `Tween<Color>` calling `update`), the one edit left in the demo copy;
  (4) per the plan: animation then scenes in Rust with the TS as the oracle; (5) handle classes in the QuickJS bindings; (6) `platypus_app` wiring
  (hand-over note in the shared notes folder).
- Dev tools: `cargo run -p platypus_ts_tests --bin compile_check file.ts` (compile against every engine API, `PLATYPUS_TS_TRACE=1` names the function
  being generated), `--bin sine_bench`, `--bin ui_bench`, `--bin bench`. Edit files with the Write/Edit tools or Python scripts (bash heredocs
  with apostrophes break on this machine; the user asked for this).

Next, in order:

1. **Strings** (design `docs/research/platypus-ts-strings.md`, decided: stored as WTF-8, behaviour exactly JavaScript's; lone surrogates become U+FFFD at the
   engine boundary and `localeCompare`/`Intl`/`RegExp` are compile errors until built, as the note proposes, not yet confirmed by the user).
   - [x] Slice 1a, runtime core (`platypus_ts_rt/src/string.rs`): inline-bytes `Str` with ASCII / well-formed flags and UTF-16 length, cursor, `charCodeAt`,
     `codePointAt`, `charAt`, `slice`, `substring`, `indexOf`/`lastIndexOf`/`includes`/`startsWith`/`endsWith`, `<` ordering, concat, `fromCharCode`/`fromCodePoint`;
     differential tests against bun (`tests/gen_strings.ts` -> `strings_expected.json`, `M9-TS-162..172`).
   - [x] Slice 1b, rest of the runtime (`platypus_ts_rt/src/string_more.rs`): `trim*`, `split`/`join`, n-ary concat for template literals, `repeat`, `padStart/End`, `at`,
     `toUpperCase/LowerCase`, `String(number)`, `toFixed`, `toString(radix)` (integers), `parseInt`/`parseFloat`/`Number(s)`; differential tests `M9-TS-173..177`.
     Not done: `toString(radix)` of fractions, `toPrecision`/`toExponential`, `normalize`, `localeCompare` (compile errors in slice 2).
   - [x] Slice 2, compiler (`lower/strings.rs`, `codegen.rs`): `string` locals/fields/arrays/params, literals as static objects (writable data, no heap id), `+`/`+=`,
     template literals, `===`/`<` (code unit order), `s.length`, the methods of slice 1, `String(x)`/`Number(s)`/`parseInt`/`parseFloat`, `String.fromCharCode/fromCodePoint`,
     `join`, truthiness, `switch`; a literal is a union tag or a `string` by context (`let` and array literals widen); `M9-TS-178..182`.
     Known gaps: `typeof`, `codePointAt` past the end gives NaN (not the null NaN), `s?.length`, string results of object-literal fields without a declared type,
     `join` of non-string arrays, template/`+` chains allocate one string per `+` (`replace`/`match` came with RegExp R2, `localeCompare`/`normalize` with Intl and the string slices below).
   - [x] Slice 3, boundary: `string` in `#[platypus_api]` (`&str` parameter read from the script string object, no copy unless it has a lone surrogate, which becomes U+FFFD;
     `String` result through a per-thread buffer the compiled call copies into a new script string), manifest/d.ts `string`, `GameObject.name` / `find` in `platypus_scene`;
     `M9-API-21..22`, `M9-TS-183..186`. Strings are not available in the QuickJS bindings (a compile error in the macro). `string | null` parameters are not supported.
     **The strings item is done.** Remaining string work lives in the standard library item below.
2. **Annotate `platypus_physics`, `platypus_audio`, `platypus_script`**; wire `platypus_scene::api::enter` into `platypus_app` (needs its owner:
   another session works there); retire the ops the new API replaces.
3. Extending an engine class (`Component`) and the compiled-component vertical slice; `typeof`, class expressions.
4. **A bare-minimum JavaScript standard library, native** (`docs/research/platypus-ts-stdlib.md`; **decisions approved 2026-10-18**: exceptions as checked returns in
   may-throw functions only, `regress` for RegExp, ICU4X with a per-game locale list for Intl, typed JSON plus a dynamic `JsonValue`, tiers 1-3 in order;
   rule: JavaScript's exact results, what is not built is a compile error). Progress, in order:
   - Tier 1: [x] A. Error classes from a TS prelude (`lower/prelude.rs`). [x] B. `throw` / `try` / `catch` (checked returns, pending exception in the runtime, `throws.rs`). [x] C. `finally` with
     `return`/`break`/`continue` through it (a completion record per `try`). [x] D. runtime errors (`RangeError` from `repeat`/`toFixed`/`toString(radix)`/`fromCodePoint`/padding) catchable (`__rtThrow`); `M9-TS-187..197`.
     Not done: `error.stack`/`cause`, `Error(...)` without `new`, throwing non-Error values, catch variable typed `unknown`. [x] E. `typeof` (a nullable value that is null is `"undefined"`; the compiler does not tell `null` from `undefined`). [x] F. `Object.keys/values/entries (for-of)/assign/freeze/hasOwn`, `in`,
     object spread (new shapes, source order) and rest, array rest (`lower/object.rs`; `freeze` does not reject later writes). [x] G. `Array`: `toSorted toReversed findLast findLastIndex
     reduceRight flat flatMap`, default `sort()`, `entries/keys/values` in for-of; `Number` statics and constants, `isNaN`/`isFinite`; `Math` sinh..log1p. [x] H. `console.log/info/debug/warn/error`
     of strings, numbers, booleans, null (to a host hook, stderr by default). **Tier 1 is done** (`M9-TS-199..205`). Not done: `with`, `toSpliced`, `copyWithin`, `Array.from` with `{length}`, `Object.fromEntries`, `console` of objects.
   - Tier 2: [x] `Map`/`Set`/`WeakMap`/`WeakSet` (`platypus_ts_rt/src/map.rs`, `lower/maps.rs`; weak = strong; `M9-TS-206..214`; not done: `new Map(map)`, `Map` of booleans/literals `get`, map spread, `groupBy`). [x] `JSON.stringify` / `JSON.parse` typed from the program's types (descriptor blobs, `platypus_ts_rt/src/json.rs`, `lower/json.rs`; bun's `SyntaxError` texts; shape mismatch is a `TypeError`; `M9-TS-215..221`). Not done: a dynamic `JsonValue` for untyped parse, replacer/reviver, `toJSON`, `Date`.
   - **Next: Tier 3** (adds crates, needs network for `cargo`): [~] `RegExp`: R1 done (literals, `new RegExp`, `test`/`exec`, `lastIndex`, flags, named groups, match objects as arrays; `M9-TS-222..231`); R2 done (`String.match/matchAll/search/replace/replaceAll/split` with regexes, `$` patterns, replacement functions; `M9-TS-232..239`); the `RegExp` item is complete except `RegExp.escape` use from TS and `Symbol.replace`-style hooks (not planned). Limits: a replacement function with group parameters needs a regex literal or a function-local `const` declared from one. (regress; regex literals validated at compile time; `exec/test/match/matchAll/replace/replaceAll/split/search`, named groups, `lastIndex`, flags `gimsuyd`; UTF-16 indices: match ASCII strings by byte, others as a UTF-16 buffer). [x] `Date` (jiff; `M9-TS-240..253`: constructor forms, `Date.now/UTC/parse`, every getter and setter in local time and UTC, `toISOString/toJSON/toUTCString/toDateString`, ISO and legacy parsing, arithmetic and comparison, the OS zone or `ts_rt_date_set_zone`; bun-compared in five zones). `toString`/`toTimeString`/`String(date)`/`date + x` done with ICU4X zone names (`M9-TS-276`). Left for the Intl step (the user asked for it too): `toLocale*String`, `Intl.DateTimeFormat` and the other Intl date formatting; `Date` inside typed JSON. [ ] `Intl`, `localeCompare`, `normalize`, `toLocale*` (ICU4X).
   - Intl (the user asked for all of it, and for more locales than `en`): I2 done (`Intl.NumberFormat` and `Number.toLocaleString`: decimal, percent, currency, compact, scientific/engineering, digit options, grouping, sign display, rounding modes; `M9-TS-264..275`; not yet: `style: unit`, compact currency, `formatToParts`, `formatRange`, `resolvedOptions`, BigInt, `useGrouping: always` in locales that group from five digits). I1 done (`normalize`, `localeCompare`, `Intl.Collator` incl. `sort(c.compare)`, `toLocaleUpperCase/LowerCase`; `M9-TS-254..263`; options must be object literals; `ts_rt_intl_set_locales` takes the game's list, wiring it from `package.json` waits for `platypus_app`); next: I3 `Intl.DateTimeFormat`, `Date.toString/toTimeString` (zone names), `Date.toLocale*String`; I4 done for `PluralRules`, `ListFormat`, `RelativeTimeFormat` (`M9-TS-296..312`, bun-compared in 17 to 20 locales); still open: `Locale`, `DisplayNames`, `Segmenter` (needs iterators), `supportedLocalesOf`, `resolvedOptions`, `formatToParts`. ICU4X (`icu` 2.x, compiled data for every locale; the per-game list in `package.json` picks which are accepted, `en` by default), compared against bun.
   - Done from that list (2026-10-19): `encodeURI`/`decodeURI`/`encodeURIComponent`/`decodeURIComponent`/`escape`/`unescape` (`M9-TS-287..291`), `Error` `cause` and `AggregateError` (`M9-TS-292..295`; `stack` and `captureStackTrace` are open). `BigInt` done (`M9-TS-360..369`: `Ty::BigInt`, `lower/bigint.rs`, `platypus_ts_rt/src/bigint.rs` on `num-bigint`; not: `BigInt64Array`, `toLocaleString`, Map keys by value, JSON). Found on the way and fixed (`M9-TS-370..375`): `a && b` / `a || b` give the operand when both sides have one type (conditions stay boolean through `lower_cond`; mixed sides such as `obj && obj.x > 0` as a value are still boolean); a nullable string tests for empty. Still to do: (arbitrary precision, `bigint` type, literals `10n`, mixed-type errors, `BigInt.asIntN`; a runtime bignum), `encodeURI`/`decodeURI`/`encodeURIComponent`/`decodeURIComponent` (`URIError` "URI malformed"; small, do first), the missing errors (`AggregateError`, `cause`, `stack`, `Error.captureStackTrace`; the other classes exist; there is no `ParseError` in JavaScript, `SyntaxError` is it), generators and iterators done (answers recorded 2026-10-19; `M9-TS-376..393`): well-known symbol member names and the iterator protocol for classes (`lower/iter.rs`), stackful coroutines on `corosensei` (`platypus_ts_rt/src/coro.rs`), generators by source rewriting (`lower/generators.rs`: `function*`, methods, `*[Symbol.iterator]()`, `yield`, `yield*` as a statement, `next(value)`, `return()`, `try/finally`, exceptions). Open: `Symbol()` values, `generator.throw()`, a returned value (`return v` in a generator is dropped), `yield*` as a value, `finally` blocks on `return()`, untyped generators, `async`/`Promise`/timers on the same coroutines (next), `Proxy` (not until needed).
   - Bytes across the engine boundary (the user asked: e.g. Rust handing an image to script, or script handing bytes to Rust): `Uint8Array` is `Ty::Bytes` already; add a manifest type `bytes` (`&[u8]` in as pointer + length, `Vec<u8>` out into a new `Uint8Array`, like strings), then `btoa`/`atob`, `TextEncoder`/`TextDecoder`, and `data:` URIs through `platypus_core::decode_data_url`. Images are better passed as texture/asset handles than as bytes; bytes are for files, network and cooked data.
   - **Intl date formatting decision (2026-10-19, `docs/research/platypus-ts-intl-datetime.md`):** ICU4X now with lightweight main-market data; `rust_icu` as an optional `icu-full` build.
     - [~] I3a (`icu-lite`): done (`M9-TS-277..286`): `dateStyle`/`timeStyle`, `toLocaleDateString/TimeString/String`, `new Intl.DateTimeFormat(locale, options).format`, component options (year/month/day/weekday/hour/minute/second, `hour12`/`hourCycle`, `fractionalSecondDigits`, `timeZone`, `timeZoneName`). The numeric padding of month/day/hour comes from a table generated from the reference engine (`bench/gen_intl_pad_table.ts` -> `platypus_ts_rt/src/intl_pad.rs`, 31 locales). Open: `formatToParts`, `formatRange`, `resolvedOptions`, `era`, `dayPeriod`, other calendars and numbering systems, the columns listed in `docs/differences.md` ("Intl date formatting").
     - [ ] Lightweight data: generate a baked ICU4X data crate for the main-market locale list with `icu4x-datagen` and use it instead of `compiled_data` (today every locale is linked: large binary). Keep the per-game locale list from `package.json` as the accept list.
     - [ ] (optional, LATER; the user said to ignore it for now, 2026-10-19) `icu-full` cargo feature of `platypus_ts_rt` (off by default): backend `rust_icu`/ICU4C for exact `availableFormats` (`udatpg_getBestPattern` via `rust_icu_sys`), all locales, calendars, `formatToParts`, `formatRange`. First a spike: build `rust_icu_sys` + `rust_icu_udat` on this Windows machine against a source-built ICU 77 (static, versioned symbols, `icu-config`, bindgen/LLVM); see the note for the prerequisites. Decide after the spike whether I1/I2 (collator, NumberFormat) also get an `icu-full` path.
   - Done (2026-10-19): `Symbol()` values (a prelude class `Symbol_`: identity, `description`, `toString`, `Symbol.for`/`keyFor`, `typeof`; `M9-TS-394..398`), `generator.throw()`, `return(v)` running `finally` blocks (a marker error that user `catch` blocks rethrow), `return value` (`Generator<T, T>`) (`M9-TS-399..407`). Not done: symbols as property keys or Map keys, other well-known symbols as values, `yield*` as a value, untyped generators.
  - Done (2026-10-19): `Promise<T>`, `async`/`await`, `new Promise<T>(...)`, `.then/.catch/.finally`, `Promise.resolve/reject/all/race/any/allSettled`, `then(f, g)`, `queueMicrotask`, `setTimeout/setInterval/clear*` on stackful coroutines with the microtask and timer queues in the runtime (`event.rs`), bun-compared tick order (`M9-TS-408..430`). Host contract: call `ts_rt_microtasks_run()` after every call into script and `ts_rt_timers_advance(now_ms)` once per frame (`ts_rt_run_loop()` for tests). Not done: async iterators and async generators, top-level `await`, awaiting a thenable that is not a Promise, a `Promise<A | B>` (a union as the value type of a *written* `Promise<>` works for `T | null`), `Promise.reject` typed from context, `setTimeout` extra arguments. Done 2026-10-22: generic promises, `for await` over arrays, `Promise.withResolvers`, unhandled-rejection reports, the host loop in `platypus_app` (see 'Promises audit').
  - Done (2026-10-19): `ArrayBuffer`, `DataView`, `new Float32Array(buffer, offset, length)`, `subarray`, `.buffer`, `.byteOffset` as views sharing memory (`HDR_VIEW`, `M9-TS-431..441`). `.buffer` of an owning array makes a new `ArrayBuffer` object each time (same memory, not `===`). Not done: `SharedArrayBuffer`, resizable buffers, `BigInt64Array`, `DataView` BigInt accessors, `Atomics`, transfer, passing buffers across the engine boundary (needs `platypus_app`).
  - Done (2026-10-19): bytes across the engine boundary in the runtime (`ts_rt_typed_from_host_gc`, `ts_rt_typed_data`, `ts_rt_typed_byte_len`; the generated Rust bindings for exported functions with typed-array parameters are not done), `structuredClone` (`clone.rs`, no Map/Set/cycles, class instances keep their class), `WeakRef<T>` for program classes (strong), `WeakMap`/`WeakSet` (strong; already there) (`M9-TS-442..455`). Not done: `FinalizationRegistry`, `globalThis`, real weak references.
  - Done (2026-10-19): `Intl.Segmenter` (grapheme, word, sentence; `intl_segment.rs`), `Intl.Locale` (parts, extension keywords, options, maximize/minimize; `intl_locale.rs`), `Intl.DisplayNames` for region, script and language (`intl_names.rs`) (`M9-TS-456..479`). Differences: `"und"` maximizes to `en-Latn-US` by a special case, CLDR names can differ from bun's (`zh-Hans` is "Simplified Mandarin Chinese"), `isWordLike` is false for non-word granularities (undefined in JS), DisplayNames types currency/calendar/dateTimeField are not built, `fallback: "none"` is not built. Not done: `formatToParts`, `formatRange`, `resolvedOptions`, `Intl.supportedValuesOf`, `Intl.getCanonicalLocales`.
  - Done (2026-10-19): `Temporal` on the ISO calendar: `Duration`, `PlainDate`, `PlainTime`, `PlainDateTime`, `Instant`, `ZonedDateTime` (IANA zones and fixed offsets through jiff: `tz.rs`), `Now` (`lower/temporal.ts` is the prelude, `lower/temporal.rs` routes `Temporal.X.from(v)` and string arguments to the parse functions; `M9-TS-480..515`). Not done: `PlainYearMonth`, `PlainMonthDay`, other calendars, `round()`, `toLocaleString`, `Duration.total` with `relativeTo`, `largestUnit`/`smallestUnit` rounding options beyond `largestUnit`, `overflow: "reject"` options (constrain is the only mode), `ZonedDateTime.until` with calendar units across a clock change, `disambiguation`/`offset` options, time zone transitions API, `Temporal.Calendar`/`TimeZone` objects.
  - Done (2026-10-20): Temporal `PlainYearMonth`, `PlainMonthDay`, `round` (PlainTime, PlainDateTime, Instant, ZonedDateTime, Duration: `smallestUnit`, `roundingMode`, `roundingIncrement`, `largestUnit` for durations), `Intl.getCanonicalLocales` (`M9-TS-516..536`). Still open: rounding in `until`/`since`, `Duration.total` with `relativeTo`, `FinalizationRegistry` (no type for the held value), `globalThis`, `formatToParts`/`formatRange`/`resolvedOptions`.
  - Done (2026-10-20): `Intl.NumberFormat.formatToParts` (the formatted text is split by the locale's digits, separators, signs, percent, currency and compact suffix: `intl_number.rs`) and `Intl.DateTimeFormat.formatToParts` (ICU4X parts: `intl_parts.rs`) (`M9-TS-537..549`). Still open: `formatRange`, `resolvedOptions`, `fractionalSecond` part.
  - Done (2026-10-20): `resolvedOptions()` of `Intl.DateTimeFormat` (locale, calendar, numberingSystem, timeZone) and `Intl.NumberFormat` (locale, numberingSystem, style, currency, notation, signDisplay, integer/fraction/significant digits); the other fields (`year`, `hour12`, `roundingMode`, `useGrouping`...) are not in the result (`M9-TS-550..555`).
  - Tier 4 (the user listed these; `NaN`, `Infinity`, `isNaN`, `isFinite` already work): iterators / generators and `Symbol` (`Symbol.iterator`, `asyncIterator`, `toPrimitive`...), typed arrays: `Int8Array`..`Float64Array` and `Uint8ClampedArray` done (`M9-TS-313..359`: `Ty::Typed(kind)`, `lower/typed.rs`, `platypus_ts_rt/src/typed.rs`); open: `ArrayBuffer`, `DataView`, views that share memory (`subarray`, `buffer`, `new X(buffer, offset, length)`), `BigInt64Array`, `Array.from(typed)`, `X.of`, `copyWithin`, a read past the end gives 0 instead of undefined, `Temporal` (jiff has the model), real `WeakRef` / `FinalizationRegistry` (WeakMap/WeakSet are strong now), `Promise`/`async`/timers, `structuredClone`, `globalThis`.

## Asset packs (design 2026-10-21)

Design `docs/research/platypus-asset-packs.md`, steps `docs/research/platypus-asset-packs-plan.md`. Done: `crates/platypus_pack` (the `.ppk`
container, LZ4 block codec, writer, reader, `Vfs` with `DirSource`/`PackSource`; `M9-API-1100..1112`). Next: step 1 (manifest), then
`platypus-build pack` (steps 2 to 5), then the `platypus_app` seam (step 6). Waiting for the user: codec (LZ4 assumed), DLC and mods policy,
unreferenced assets, both forms for code-literal textures, one shared pack, the cooker's RGBA8 fallback; encryption is out of scope.
Differences to keep in mind:

- [x] Dynamic asset paths built at run time (virtual file system plan, phase 0, 2026-10-22): `include` / `exclude` take globs (default `assets/**`), the build infers `assets/sfx/*.*` from template literals and `+` chains (`platypus_ts::assetpaths`), reports what each reached, and `build/assets.plan` makes the dev app warn about a loaded file a packaged build would miss (`M9-TS-2550..2553`, `M9-API-1143`). Packaged `emoji` checked in a screenshot run.
- [x] Pack build, phase 1 (2026-10-22): `platypus-build package --packs` writes `data/assets.dat` + `scene-<slug>.dat` + `index.json` with the reference-counting rule, `platypus-build pack-info`; deterministic; source forms only (`M9-TS-2554..2559`). The runtime reads them since phase 2 (below).
- [x] Runtime reads through the `Vfs`, phase 2 (2026-10-22): `platypus_app` mounts `index.json`'s packs (`assets.dat`, then scene packs) or the folder, `platypus_core::set_asset_fs` / `read_asset` is the one read call for every loader, streamed sounds read through the chunk table, not-found messages name where they looked, dev case warning (`M9-API-1144..1149`, `1205`). Packaged `emoji` and `scenes` with `--packs` run with no not-found errors. Not done: patches/DLC/mods overlays, cooked forms from packs, an asset watcher over packs, `guid:` sources rescanning after a `.meta` change.
- [x] `Assets.exists` / `list` / `packOf`, phase 3 (2026-10-22): `#[platypus_api]` in `platypus_engine_api::assets`, answering from `set_asset_index` (filled from the mounted `Vfs`), QuickJS parity (`M9-API-1151`, `M9-TS-2557`). `list` returns the paths as one newline-separated string (the API macro has no `string[]` result yet); `js/engine/src/assets.ts` (the TypeScript `Assets` bundled games import) does not have them yet.
- [x] zstd as the second codec (`--codec zstd`, `Codec::Zstd = 3`); `platypus-build package` picks the newest host by modified time and prints it (`M9-TS-2559`, `2560`).
- [ ] **Flip `platypus-build package` to write packs by default** (and add `--loose` for debugging and modding) once packs have been exercised on real games (decision 8 of the virtual-fs plan).
- [ ] Revisit: textures named in code are stored in source form only for now; the cooked form, and both, come with the cooker move (decision 5: both forms for now, consider cooked only to cut bloat).
- [ ] DirectStorage and `CODEC-1` (GPU codecs) come later; LZ4 stays the default codec.
- [ ] A cooked texture's GUID differs from its source's (`Assets.pathToGuid` answers the source GUID).
- [ ] Packs hold `IRTX` v1 (largest mip first) until step 12; no mip streaming before that.
- [ ] `platypus-build pack` needs a built `platypus_app` for cooking until `crates/platypus_cook` (step 15).
- [ ] The in-crate LZ4 decoder reads at 0.9 to 1.9 GB/s; `lz4_flex` (same format, a new crate) would be about twice that.
- [x] DirectStorage / codec research done (`docs/research/platypus-directstorage-and-codecs.md`, 2026-10-21): decision: LZ4 stays the
  default everywhere, zstd (codec 3) recommended as the second codec (needs the user's OK for the crate), xz/LZMA and Brotli out;
  the Windows GPU codec (GDeflate as codec 4, or DirectStorage 1.4 zstd, still preview) is **pending**.
- [ ] Next measurement: `CODEC-1` (plan step 17): headless D3D12 probe, cold cache, 2 GB photographic BC textures + 500 MB meshes:
  CPU LZ4 on 8 threads vs DirectStorage stored/LZ4 (custom queue) vs GDeflate GPU vs zstd GPU, into wgpu buffers; add a GPU codec
  only if it is 1.5x faster in wall time (or half the CPU time) with a pack at most 0.8x the LZ4 pack. Needs the user's OK for the
  DirectStorage/`gdeflate`/`zstd` downloads.

### End-of-project review (asset packs)

- [ ] **Windows DirectStorage**: revisit `CODEC-1` once DirectStorage 1.4 is GA and vendor zstd drivers ship (announced for 2H 2026);
  DirectStorage needs the DX12 backend while Platypus defaults to Vulkan on Windows (decide the default, or Vulkan's
  `VK_EXT_memory_decompression` GDeflate path instead); redistribute `dstorage.dll` / `dstoragecore.dll` under the SDK licence.

## Windows API (design 2026-10-21)

Design `docs/research/platypus-windows.md`, steps `docs/research/platypus-windows-plan.md` (tests `M9-API-1400..1499`, `M9-TS-2800..2899`; families and messages `M9-API-1495..1499`, `M9-TS-2831..2850`).
A `Window` handle class in `platypus_engine_api::window` (`Window.main`, `Window.create({ title, width, height, style: "tool", parent,
key })`, `title`, `style`, `parent`, `isChild`, `handle`, `nativeHandle`, `close()`, `onCloseRequested` + `cancelClose()`, ...), each
window with its own UI root (`createRoot(win)`), multi-window `platypus_platform` with a fake backend, secondary windows offscreen in
screenshot runs, `examples/windows`.

- [x] Step 1: `platypus_platform::windows` pure types, `plan`, `place`, `WindowTree`.
- [x] Step 2: `platypus_engine_api::window` (Rust tests).
- [x] Step 3: `#[options]`: QuickJS options objects for optional parameters.
- [x] Step 4: compiled side: `engine_ts.d.ts` supplement, `UiRoot.mount(window, ..)` (`ts_ui_mount_window`), `bench/windows.ts`.
- [x] Step 5: multi-window platform (`Backend`, `run_commands`, `FakePlatform`, winit runner, `docs/platform.md` "Multi-window"). Decisions: `RunnerBackend` (winit + virtual) instead of `WinitBackend`+`VirtualBackend` fields; `Backend::take_events` echoes virtual `Resized`/`Moved`; the main window still gets `CloseRequested` through `window_input` first.
- [x] Step 6: `Gpu::create_window_surface` + `RenderError::Unsupported` (no test: needs a real window).
- [x] Steps 7 to 10: `platypus_app` routing, drawing, screenshots, hot reload, QuickJS games.
- [x] Step 11: `examples/windows` (native, TypeScript globals; a main window, owned tool windows, an isolated Inspector over messages).
- [x] Step 12: docs (`docs/windows.md`, linked from `docs/README.md`). Optional leftovers, open: D1 embedded `child` style, D2 modal dialogs, D3 `Camera.targetWindow`, D4 transparent windows, D5 hidden main window in screenshot runs, `Application.wantsToQuit`; phases B and C of the event domains; a manual check of the winit runner on a real display.
- [x] Step 1b: static accessor properties on handle classes in QuickJS (`Window.main` is a property in both backends).
- [ ] Steps F1 to F4: window families and event domains (per-family input, focus, pause, timers, UI roots; one winit loop,
  per-domain queues; threads / processes per domain are later phases).
  - [x] F1: families in the API (`DomainId`, `root` / `isIsolated` / `domain` / `run`, `Window.domainFocused` /
    `onDomainFocusChanged`, the cross-family guard with the setup exception, `window::end_frame` / `run_queued` /
    `can_set_up`). Decisions: tool / dialog / popup / child default to the creating domain's root (the main window in the
    main domain); a parent from another family is ignored with a warning; a window made for another domain's family is
    open to its creator's setup (`run`, `on*`, `createRoot`) until `end_frame`.
  - [x] F2 (input per domain; compiled and QuickJS), F3 (timers per domain), F4 (the app).
- [ ] Steps M1 to M3: messages (`postMessage`, `onMessage<T>`, `request<R>`, `onRequest`, `broadcast`; JSON of the static type,
  no handles), and the isolated Inspector in `examples/windows`.
  - [x] M1 (the queue in `platypus_engine_api::window`), [x] M2 (typed messages in compiled code: generic facade functions
    `__xm_Window_postMessage<T>` / `onMessage<T>` / `onRequest<T, R>`, `__xt_Window_onMessage` / `onRequest` / `broadcast`; handles in a
    payload are a compile error). `request<R>` is written per reply type by `facade.rs` (a promise class exists per written
    type, so `Promise<R>` cannot be generic); `Lowerer::infer_targs_skip` lets a facade method infer from the arguments after
    the receiver, and a closure's own parameter annotations fix type parameters. [x] M3 (the QuickJS prelude `window::QUICKJS_PRELUDE`, installed by `window::register_quickjs`; `js/engine/src/window.ts` is step 10).
  - [x] Typed replies (`M9-TS-2854`): `request<R>` with a program's interface or alias failed with `unknown type`, because the classes
    written per promise type sit in front of the program and name it; interfaces and aliases are now registered before any class
    is declared (`lower/mod.rs`, step 0).
  - [x] Named events (`M9-TS-2855..2867`): `win.emit(name, payload?)`, `win.on(name, (payload: T, from) => ..)`, `win.ask<R>(name, payload?, timeoutMs?)`,
    `win.answer(name, (payload: T, from) => R)` and `Window.emitAll`, with the envelope `{"$e":name,"d":payload}` as a separate layer from the raw messages (the host routes envelopes to `onEventJson` / `onAnswerJson` listeners of the name; raw listeners never see them; an unanswered `ask` fails at once with `no answer for "name"`)
    (`facade.ts` chunk, `ASK_TEMPLATE` in `facade.rs`; a call without payload or a handler without parameters uses the `..0` facade
    variants with a dummy type argument). QuickJS: `QUICKJS_PRELUDE`. Open idea: a shared event-type map so one name fixes the payload type
    at both ends; `answer` handlers cannot return a promise yet.
- Answered by the user (2026-10-21): `isChild` = has a parent; hot reload keeps keyed windows; secondary windows vsync off; no OS
  windows for secondary windows in screenshot runs; D1..D4 optional later.

## Backlog

The full list with sub-tasks, sizes and STOP gates is `TODO.md` M9.b2. Open top-level items: type
sidecar, GC (verifier, cheaper frames, escape analysis, pauses), closures, classes, generics, structural typing, strings, language features, library
surface, JS-semantics conformance, `JsValue`, direct engine calls, hybrid release builds,
performance, cross targets and CI.

## Debug info and a debugger (design 2026-10-31: `docs/research/platypus-ts-debug-info.md`)

- [x] 1. `Stmt::Line`, Cranelift srclocs, per-function address tables in `Game`; crash report prints the bundle line.
- [x] 2. Optional `game.pdebug` sidecar beside the exe (`build`; `package --debug-info`): `file:line` and source excerpts in reports.
- [x] 3. Unwind info for JIT frames; the crash report's call stack (a stack scan, `unwind::stack_words`).
- [ ] 3b. `RtlVirtualUnwind` over the host's frames faulted inside ntdll (a read of -1) for some builds of a test exe, from `walk_pcs`
  (removed): find out why (a JIT unwind-info detail such as the `mov rsp, rbp` epilogue, or a host frame?) before relying on the system unwinder.
- [x] 3c. Zombie Horders start-up crashes (about 1 start-up in 5): the JIT did not probe big stack frames; fixed with Cranelift
  inline stack probes (research doc, "Zombie Horders crashes"). Left: report a real stack overflow (0xC00000FD) in the crash
  handler; a thread with no stack left cannot run it, so it needs a reserved-stack guarantee or a watchdog process.
- [ ] 4. Debug build mode (spilled locals, break hook): pause, resume, step, breakpoints that survive hot reload.
- [ ] 5. A debugger: Debug Adapter Protocol server (VS Code, then the editor).
- [ ] Per-compile cost of the line info (every run pays it, sidecar or not): measure `Stmt::Line` markers against compile time and
  memory (`PLATYPUS_TS_TIMING`, md-reader), then cut: one marker per statement only where the line changes, store ranges as
  deltas, keep the table only when a crash can use it (lazy: recompile with srclocs on the first crash is not possible, so
  consider a build flag that drops it from shipped hosts).

## Log

- 2026-10-19 Performance review and the sine-demo swap-out check: `docs/research/platypus-ts-sine-demo.md` (verbatim `SineCircle` 33x QuickJS, UI list 8-34x React/QuickJS, sim 44x). Field types are now inferred from constants and arithmetic (`M9-TS-592`).

Newest first. One line per finished step: what, test IDs, numbers if measured.

- 2026-10-19 `Date.toString/toTimeString/String(date)/date + x` (`M9-TS-276`): ICU4X root zone names, whole-minute offsets, the name follows the daylight state for old dates. Design note for `Intl.DateTimeFormat` written, awaiting review.
- 2026-10-19 Intl I2 (`M9-TS-264..275`): `Intl.NumberFormat`, `Number.toLocaleString` (decimal, percent, currency, compact, scientific, digit options, grouping, sign display, rounding modes), 15 locales against bun.
- 2026-10-19 Intl I1 (`M9-TS-254..263`): `normalize`, `localeCompare`, `Intl.Collator` (also as `sort` comparator), `toLocaleUpperCase/LowerCase` on ICU4X.
- 2026-10-19 `Date` (`M9-TS-240..253`): spec calendar maths, `jiff` local time (OS zone or `ts_rt_date_set_zone`), constructors, statics, getters/setters, ISO and legacy parsing; five time zones against bun.
- 2026-10-19 `RegExp` R2 (`M9-TS-232..239`): `match/matchAll/search/replace/replaceAll/split` with regexes, `$` patterns, replacement functions. R1 (`M9-TS-222..231`): the `regress` engine, literals, `exec/test`, flags, named groups.
- 2026-10-19 JSON (`M9-TS-215..221`): typed `stringify`/`parse` through descriptor blobs; bun's `SyntaxError` texts (67 cases).
- 2026-10-19 `Map`/`Set`/`WeakMap`/`WeakSet` (`M9-TS-206..214`): insertion-ordered hash table, SameValueZero, exact live iteration.
- 2026-10-19 Standard library tier 1 E-H (`M9-TS-199..205`): `typeof`, `Number` statics, `Math` extras, `Array` gaps, `console`, `Object.*`, spread/rest.
- 2026-10-18 Exceptions, tier 1 A-D (`M9-TS-187..198`): `Error` classes from a prelude, `throw`/`try`/`catch`/`finally` as checked returns in may-throw functions only, runtime `RangeError`s catchable.
- 2026-10-18 Strings (`M9-TS-162..186`): WTF-8 storage with JavaScript's UTF-16 behaviour; runtime, compiler and the engine boundary (`&str`/`String`); design `docs/research/platypus-ts-strings.md`.

- 2026-10-18 Array library (`M9-TS-92..110`): runtime routines for pop/shift/unshift/slice/splice/concat/reverse/fill/sort,
  inline loops for indexOf/findIndex/reduce/at, `length = n`, `Array.from/of/isArray`, `new Array(n)`. The sort
  roots its buffer; the first stress run found the merge buffer traced before it was zeroed (spike doc 15.2).
- 2026-10-18 Math library and operators (`M9-TS-85..91`): ceil/trunc/round/fround/clz32 inline, libm functions, JS `pow`,
  `random`, `**`, `~`, `??=`/`||=`/`&&=` (spike doc 15.1).
- 2026-10-18 Default, optional and rest parameters and spread (`M9-TS-77..84`): the caller fills in defaults (lowered in a
  scope of the earlier parameters) and builds the rest array; array/call spread copies (spike doc 14.2, 14.3).
- 2026-10-18 Destructuring (`M9-TS-71..76`): `bind_pattern` for declarations, parameters, `for...of`, callbacks;
  defaults via `Cond(IsNull)` (spike doc section 14.1). Rest elements and assignment patterns are open.
- 2026-10-18 `switch` and enums (`M9-TS-64..70`): clause index then ordered bodies (fall-through, default
  anywhere), enums folded to constants, bit-operation constant folding (spike doc section 13).
- 2026-10-18 `platypus_scene` on `#[platypus_api]` (`M9-API-12..15`, `M9-TS-151..155`): the `Scene` namespace, handle encoding that is never 0,
  `platypus_api` `glam` feature; compiled TypeScript drives the real scene. Not wired into `platypus_app`.
- 2026-10-18 Value classes across the boundary and the engine context (`M9-TS-147..150`, `M9-API-07..11`): `values(..)` in
  `#[platypus_api]`, fields as scalars, results through a pointer (`CallExternInto`), `platypus_api::context` (thread-local world).
- 2026-10-18 The compiler calls the engine API (`M9-TS-142..146`): `--manifest`, `CallExtern`, `Ty::Handle`, number conversions at the call;
  `tools/platypus_demo_api` proves it end to end. Rule found: a live handle is never 0.
- 2026-10-18 `#[platypus_api]` first slice in the engine workspace (`crates/platypus_api`, `platypus_api_macros`; `M9-API-01..06`): manifest,
  `.d.ts`, JSON, `extern "C"` shims, typed QuickJS bindings (150 ns a call against 245 ns for a simplified op).
- 2026-10-18 `@property` reflection tables (`M9-TS-139, 140`): compile-time metadata, a generated `PROPS_<Class>` constant per class.
- 2026-10-18 Methods as values (bound closures over the receiver) and arrow-function fields (`M9-TS-137, 138`).
- 2026-10-18 Inheritance (`M9-TS-120..136`): prefix layout, `$init` constructors with `super(...)`, direct calls by whole-program
  analysis and `CallVirtual` tables otherwise, `instanceof` tables, downcast by narrowing, abstract classes (classes note 14).
  Found: a `Seq` whose value was a void call never ran it.
- 2026-10-18 Plain classes (`M9-TS-111..119`): fields, constructors (parameter properties, initialiser order, early return),
  methods as `this`-first functions, getters/setters, statics as globals, `#private`, `this` in arrows (classes note 13).
- 2026-10-18 Engine-boundary answers recorded (section 13): #[platypus_api] replaces __platypus_op, whole engine ported to Rust.
- 2026-10-18 Engine-boundary design note written (`docs/research/platypus-ts-engine-boundary.md`); awaiting review.
- 2026-10-18 Loops (`M9-TS-57..63`): `for(;;)`, `do...while`, labelled `break`/`continue`, labelled blocks,
  per-iteration `let` cells in `for` heads (spike doc section 12).
- 2026-10-18 Classes design note written (`docs/research/platypus-ts-classes.md`); awaiting review.
- 2026-10-18 Mutable module-level variables (`M9-TS-47..56`): `let`/`var` are data slots (never folded),
  `Place::Global`, `GlobalVar` read node, `let x: T | undefined;`, closures stored in and called
  through globals; a `let` global is a stable GC operand only when no later operand calls or stores
  (two hazard tests fail under the old rule). Found on the way: global slots were 1-byte aligned.
- 2026-10-18 Spike tracking file created (this file). Closures, GC review, GC and the sine benchmark
  are done as described below; next is the first task above.
- 2026-10-18 TODO split into honest sub-tasks; measured feature counts (spike doc section 11).
  Gameplay code is classes: `extends Component`, `@property`, `this`, getters/setters.
- 2026-10-18 Closures (`M9-TS-34..46`): name scan, cells, lifted lambdas, function values; all run
  normally, in stress mode and with collection off. Busy sim 0.94 us/step.
- 2026-10-18 GC review (`M9-TS-29..33`): pointer comparisons rooted, struct id ranges per module,
  heap ids in headers, array data in the threshold, frame-plan overrun guard.
- 2026-10-18 GC root analysis tightened (`M9-TS-27, 28`): a field read or reassigned local held as an
  argument was freed; both tests failed first.
- 2026-10-18 Precise mark-sweep GC with shadow-stack roots, globals, host handles, stress mode
  (`M9-TS-20..26`). Busy sim 0.88 us/step; sine curve 0.043 ms/frame.
- 2026-10-18 Sine-circle curve benchmark (`M9-TS-19`): native ~25x QuickJS (0.045 vs 1.13 ms/frame),
  level with bun. Constant folding, integer `%` fast path, symbol prefixes.
- 2026-10-18 Spike: oxc to typed IR to Cranelift object; `sim.ts` compiles, 13/13 ported tests
  (`M9-TS-01..13`), busy sim ~46x QuickJS and 4.5x bun (50 parked bullets, 10 zombies).

- **Stack values (2026-10-20):** `src/sroa.rs` replaces value-class objects (`Vector2/3/4`, `Quaternion`, `Color`, ...) that never leave their function by number locals (no allocation, no GC frame slot). Escaping uses (argument to a script function, return, store in a field / array / closure) keep the object; aliases that are changed keep one object. Left: passing values by scalars across script calls and in fields of script classes (needs a calling-convention change), a benchmark against the heap version. Engine objects (handles) stringify as `{}` in `JSON.stringify` and cannot be parsed.

- **Review (JSON of value classes, 2026-10-20):** by decision, `JSON.stringify` of `Vector2/3/4`, `Quaternion`, `Color`, `Rect`, ... gives the compact array of numbers and `JSON.parse` reads it back (and the object form too); the TypeScript classes (and bun) give `{x,y,z}`. QuickJS side to match when handle/value classes are bound there.

- **Native arrays across the boundary (2026-10-20):** an `#[platypus_api]` function may take `&[f64]` (a `number[]`, read in place) or `&[V]` for a declared value type (a `Vector3[]`, read into `V`s), manifest types `f64s` / `values:Name`. Used by `AnimationCurve.fromPoints(points: Vector2[], mode?)` (a difference to the TypeScript `[time, value]` pairs). Not yet: arrays as results, arrays in QuickJS, flat inline storage of `Vector3[]` in compiled code (benchmark first).
- **Physics, first part (2026-10-20):** `crates/platypus_physics_api` is `Physics` (gravity, `createBody`, world colliders, immediate `raycast`, `step`), `Body` (pose, velocity, forces, mass, kind, colliders, character controller, attach to a GameObject) and `Collider` / `RaycastHit`, a typed layer over `platypus_physics::Physics::apply`. Left: joints, contacts as component hooks, Rigidbody and collider components (kinds), scene sync of kinematic bodies, 2D.
- **Component model (2026-10-20):** contact hooks take Unity's `Collision` / `Collision2D` and trigger hooks the other `Collider` / `Collider2D` (2026-10-22, native handle classes, `M9-TS-1004..1005`; a parameter typed as the other kind gets that); `onTriggerStay` and `onCollisionStay` exist only natively (made by the host). Not in scene.ts, so not built: component `enabled`, `onEnable/onDisable`, `getComponentsInChildren/InParent`. `onReload` has no native meaning until state-keeping reload exists for compiled code. `go.children` / `GameObject.roots` are fresh arrays (a snapshot), as the TypeScript ones are live lists.

- **Physics components (2026-10-20):** `platypus_physics_api` now has `Rigidbody`, `BoxCollider`, `SphereCollider`, `CapsuleCollider`, `MeshCollider`, `CharacterController` (`components.rs`), `FixedJoint`, `CharacterJoint`, `HingeJoint`, `SpringJoint`, `RopeJoint`, `Rope` (`joints.rs`), `Rigidbody2D`, the five 2D colliders, `Physics2D` (`physics2d.rs`), `ForceMode` / `RigidbodyConstraints` constants, kinds for scene files (`register_kinds()`), and for the hooks `take_contacts()` + `collider_info(collider)` (object, class, component, 2D, Rigidbody) + `object_destroyed(object)` + `start_pending()`. Differences to the TypeScript, for review: there is no `Collider` base class (each class repeats the members; no `Collider`-typed variable); `material` is `dynamicFriction` / `bounciness` (scene files keep `material: { .. }`); `HingeJoint.limits` / `.motor` are flat (`limitsMin`, `motorForce`, ...); joints are made when a property is set (and on the first step), not in `onStart`; `Rope` is built on the first step and has no line or capsule drawing yet (`pointCount` / `pointAt(i)`, `linkCount` / `linkAt(i)` replace `points()` / `links`); `MeshCollider.sharedMesh` is not there (`setMesh(points, indices, convex)`); `Physics2D.raycast` answers at once and its hit has `colliderObject`, not `collider`; polygon / edge `points` are `setPoints` + `pointCount` / `pointAt`; `Rigidbody` mass is sent only once the object has a collider (the TypeScript added it as extra mass first); `CharacterController.velocity` is per physics step; `ForceMode` / `ForceMode2D` arguments are numbers (the namespace constants `ForceMode.Impulse` work, `ForceMode2D` is 0 / 1); a collider's world scale is read when it is made and remade at the next step if it changed; a body follows its object only when the world pose differs by more than 1e-4 m. Not done: layers / collision filtering and overlap queries (the TypeScript has none), `onCollisionStay`, joint breaking, a `Collider` supertype in the manifest, QuickJS bindings for the new classes.
- **Audio, rendering data, effects, models (2026-10-20, `M9-TS-1210..1239`):** `audio` / `AudioSource` / `AudioListener` in `platypus_engine_api`, `platypus_render_api` (Light, MeshRenderer, SkinnedMeshRenderer, Material, Texture, Mesh, Sprite, SpriteRenderer, LODGroup, Mirror, ReflectionProbe, PlaneRenderer, WorldPanel, LineRenderer(2D), ParticleSystem, Cloth, CameraFollow/Orbit, RenderSettings, Model, Primitives), Cursor, Screen, QualitySettings and Gizmos shapes; asset kinds, the `assets` table and object extras (`"model"`) in scene files. Review list (differences to the TypeScript, details in `docs/platypus-api.md`): handle-based arrays are call-style (`addBone`, `addLod`, `setPositions`, `addPin`); `Material` is mutable (`copy()` for `with`); `RenderTexture` is a `Texture`; `mesh: "cube"` goes through `Mesh.fromString`; `whenLoaded()` promises, glTF animations and `Sprite.grid` as an array are not there; `Cloth` rebuilds on scene load or `rebuild()`; the TypeScript `on(..)` unsubscribe functions are tokens. Left: wiring all of it into `platypus_app` / `platypus_render` (the renderer reads the registries and queues), the QuickJS bindings for classes with callbacks, glTF animations, `Time` / `Task` helpers that take closures, physics colliders for primitives and models (hooks exist: `set_primitive_collider_hook`, `set_model_collider_hook`), Rigidbody-style components (physics agent).
- **Closures given to the engine (2026-10-20):** `Callback<(args)>` parameters in `#[platypus_api]` (`platypus_api::callback`), `ExtTy::Fn(params, ret)`, bridge installed by `platypus_ts_rt`; used by `input.on*`, `Assets.onChanged`, `Application.onFocusChanged`, `SceneManager.onScene*` (`M9-TS-1200..1207`). Review: the subscribe functions return a numeric token and a separate `off(token)` instead of an unsubscribe function; closures return nothing; no QuickJS binding for callbacks; `input.on('keydown')` event objects became positional arguments. Pitfall found: a Rust closure capturing only a field of a guard struct drops the guard early (edition 2021 disjoint captures).
- **QuickJS handle classes (2026-10-20):** handles are opaque objects in QuickJS (`platypus_api::qjs`), a deliberate difference from the old number form. Differences to review at the end: (1) no identity: `go.transform.gameObject === go` is false (each read makes a new object; compare with a future `equals` or the id); (2) `Scene` is both the scene helper namespace and the `SceneManager` scene class in QuickJS (merged, class wins); (3) value-class JSON: QuickJS has no types, so `JSON.parse` gives plain arrays or objects and `X.fromJSON(v)` (not a reviver) makes the instance, while functions accept the array form directly; (4) `JSON.stringify(prefab)` calls `Prefab.toJSON` and so gives a string of the prefab text, like the TypeScript; (5) two methods of one class with the same script name overwrite each other in QuickJS (overloads work for statics and namespace functions only); (6) every module has the `quickjs` flag now (`platypus_native`); (7) a class static named `name`, `length` or `prototype` would clash with the function's own properties.
- **QuickJS for everything (2026-10-20):** callbacks are kept `Function`s (reference counted; the engine's listener state must be cleared before the runtime is dropped, QuickJS asserts otherwise); an exception in a callback is recorded, not thrown; typed arrays and array results are not built; value-class methods with one script name still overwrite each other in QuickJS (class methods dispatch by count).
- **JSX always available (2026-10-20, `M9-TS-1500..1511`):** the compiler embeds the `Ui` manifest (`src/lower/ui_manifest.json`, checked against `platypus_ts_ui`), so JSX and hooks compile for any caller; a host must still link `platypus_ts_ui` for the `ts_ui_*` symbols. JSX syntax errors come from the TSX parse with a line. Fixed on the way: a single-line `const [n, setN] = useState(..);` lost its `;`; `{cond && <x/>}` rendered a bool child (now an empty fragment when false). Difference: `{a && <x/>}` only handles `&&` with an element on the right; `||` / `??` with elements are not rewritten.
- **React front end for the examples (2026-10-20, `M9-TS-1600..1614`, agent L):** differences from TypeScript to review: (1) `PlatypusStyle` is a list of `StyleEntry` (a struct cannot have more than 64 fields: the collector's pointer mask), so `style.width` on a `PlatypusStyle` value is not readable, only written and spread; `Record<string, unknown>` is not mapped yet. (2) `number | string` is the prelude class `StyleValue` (numbers and strings convert where one is expected; no other use of such unions). (3) `ReactNode` is `UiElement[]`: a child that is text, a number or a list is wrapped into an element. (4) A function with fewer parameters passes for one with more (`onClick={() => ..}` for `(e) => void`) by reinterpreting the closure: the caller's extra arguments are ignored (sound on the x86-64 and AArch64 calling conventions the JIT targets; revisit for a target where the callee pops arguments). (5) Static setters (`static set x(v)`) compile but assignment to them is rejected. (6) Node commands from `ref.current.focus()/blur()/scrollTo()` are queued (`platypus_ts_ui::take_node_commands`); the host that owns the `UiTree` must apply them. (4b) Likewise a function with any return type where `void` is expected (`onClick={openPalette}` for a function returning a `Window`, `M9-TS-2847..2848`): the result is dropped. (7) `useState(f)` calls `f` for the first value when `f` is an arrow or a top-level function (React's lazy initialiser), so a state that holds a function cannot be made this way.

- **Engine facade (2026-10-20, `M9-TS-1700..1706`):** TypeScript `@platypus/engine` shapes the native API holds differently are expanded by the compiler (`lower/facade.rs`) or written in TypeScript over the native functions (`lower/facade.ts`, appended to a program that names them). Object-valued `RenderSettings.skybox` / `.bloom` (off, on with defaults, then each key; `null` is off, `sunDirection: null` is `skyHasSun = false`); `new GameObject(name, { parent, position, rotation, scale, layer, active })` (position, rotation, scale are local to the parent); `GameObject.createPrimitive`; `Camera.main` (a no-argument static function reads as a property); a facade function `__x<kind>_<Class>_<member>` is a method (`m`), property read (`g`) or write (`s`, also an option key), static call (`t`, `$<argc>` and `$o` variants) or static property (`p`), and `__xn_<Class>` a positional constructor; inside the facade `x.name$n(..)` is the native member. Differences: `getComponents(EngineClass)` is not supported (user classes only); `whenLoaded` polls once per timer tick; `onCollisionEnter2D` fills the same hook slot as `onCollisionEnter` (its thunk passes a `Collision2D`); `AnimatorController` is the native `AnimationController` with the TypeScript builder methods.
- **Build tool (2026-10-20, `tools/platypus_build`, `M9-TS-1800..1861`):** differences and left-overs: (1) the module graph is a source-to-source bundler: it renames every identifier with a colliding name in the file consistently, even a shadowing local (correct, but names in the lowered text are not the source names); (2) `import * as ns` of a project file, `export * as ns`, anonymous `export default function/class` are errors; (3) interface merging across files is not modelled (same-named interfaces in two files are renamed, not merged); (4) the dev watcher polls (200 ms) instead of using `notify`; (5) `--cook` runs the `platypus-app` binary found under `build/`; (6) asset copying follows string literals of the source that name existing files, so a path built at run time is not copied (the game folder is still readable by the app); (7) the `types` declarations carry no doc comments (the manifests have none), and a namespace named like a class is declared next to it.

- **JIT and symbols (2026-10-20, `M9-TS-1900..1904`, `M9-API-600`):** `jit::compile_and_load(src, &JitOptions)` compiles into the process with `cranelift-jit` (same `codegen::generate`, generic over the Cranelift module; the object writer's output is byte-identical). Imports resolve from `platypus_ts_rt::symbols()` (generated by the crate's build script from its `#[no_mangle]` exports, also `platypus_ts_ui::symbols()`), `platypus_api::symbol` (a namespace registers its shim addresses when `Namespace::to_json` runs, so reading the manifests is enough) and `JitOptions::symbols`. `Game::classes()` is the run-time form of the bindings' `classes()` (`classmeta::class_meta` is the data). Differences: a `Game`'s code and data are never freed (old closures may still be reachable after a hot reload), class names and property tables are leaked per load; the JIT ISA uses the CPU's features (`cranelift-native`), the object writer the generic ones; `#[no_mangle]` exports of the runtime must be `pub` and not inside an inline `mod`.
- **platypus_app native backend (2026-10-20, `M9-API-610..614`):** `crates/platypus_app/src/native.rs` runs a game compiled in process (see `docs/platypus-api.md`, "Native games in `platypus_app`"). `platypus examples/hello --screenshot out.png --frames 5` with a `build/game.pbundle` prints `hello` / `tick` from compiled code; a TSX program with `UiRoot.mount(0, <App/>)` renders into the screen. Differences / left: `App` keeps its QuickJS fields (empty for a native game) and branches at the pump points instead of a trait object; UI events are delivered after the interaction layer's default actions, so `preventDefault()` has no effect yet; world panels (`createRoot(panel)`), the 3D renderer fed from the `platypus_render_api` registries, physics sync of `platypus_physics_api`, audio, asset completion callbacks, `Screen`/`QualitySettings` change closures and input listeners are not wired; every registered class gets a starting GameObject (no `spawn: false`); top-level expression statements used to be dropped by the compiler (so a game had to mount from a `const booted = boot()` until L lowers `createRoot().render(..)` to `UiRoot.mount`; they run now, see the top-level statements entry).
- **Hot reload of compiled games (2026-10-20, `M9-API-615..617`):** see `docs/platypus-api.md`. Left: an `onReload` hook, state that is not a `@property` (module variables), references between components across the swap, scene `dontDestroyOnLoad` objects, world panels, the physics and audio worlds of a native game (they are reset, not saved).
- **3D from native games (2026-10-20, `M9-API-620`):** cameras, lights and mesh renderers (primitives, script meshes, materials, textures) are wired (`render_sync.rs`); see `docs/platypus-api.md` for the list that is not. `examples/world`-style games also need `RenderSettings`, models, sprites, particles and world panels.

- **Round 2, example fixes (2026-10-21, `M9-TS-2000..2008`):** tuples are objects with fields `0`, `1`, .. (no `.length`, no rest patterns, no assignment through `t[0]`); a method call drops arguments it has no parameter for (the component hook thunks pass `dt` to every hook) but evaluates them before the named ones; `for (const t of [a, b])` over unrelated types unrolls the body; `AnimationCurve.fromPoints` takes `[t, v]` pairs through the facade; subscribe calls return an unsubscribe function (the native token form is `fn$n(..)`); closure parameter defaults need a number, string or pointer type (a defaulted `boolean` is not nullable yet).

- **Single-file packages (2026-10-21, `M9-API-1000..1011`, `M9-TS-2400..2440`):** `platypus-build package <game>` writes `build/<exe>.exe`: the host (`platypus_app`) with icon and version info set in its PE resources (`editpe`, no `rc.exe`) and a payload (`platypus_core::payload`: deflate entries, table of contents, trailer) appended; at start `platypus_app` extracts the payload once to `<cache>/Platypus/games/<exe>-<hash>/` and runs from it (chosen over an in-memory file system because ~100 call sites read the disk). Left / differences: macOS `.app` + `Info.plist` + `.icns`, Linux `.desktop` and icon, window class (winit's default), Windows `AppUserModelID`, a Windows manifest (DPI, long paths), cooked textures/models in the payload (`.platypus/cooked`), the payload is not encrypted or signed (only a CRC per entry; sign the exe, see `docs/research/platypus-code-signing.md`), an in-memory asset layer and no first-start extraction, assets are the default `assets/` folder plus string-literal assets plus `platypus.include`, a debug host is ~77 MB (ship release), signing is only the `--sign-cmd` hook (TODO list in the research doc: `{out}` placeholder, verify step, real certificate test), mobile targets are a plan in `docs/platypus-build.md`.
- **Engine names as globals (2026-10-21, `M9-TS-2300..2312`):** `types/platypus.d.ts` declares every manifest class/namespace, the UI names and the hand-written TypeScript shapes (`lower/engine_ts.d.ts`) as globals; `@platypus/engine`, `@platypus/react` and `react` are re-export modules, so imports still type-check and are erased; the 19 examples have no engine import and pass `tsc` with no `node_modules` (the build writes `build/.tsc/{tsconfig.json,platypus.d.ts}`). Differences / left: (1) `engine_ts.d.ts` is hand-written and can drift from `facade.ts`, `engine_objs.rs` and `components.rs` (the test `M9-TS-2307` over the examples is the guard); a native-to-TypeScript table in the manifests would remove it. (2) Options-object overloads are generated for every call with trailing parameters, so the file has some noise and `Sound.play(volume?, rate?, ..)` and `play({ volume })` both show. (3) A native function named like a reserved word (`TimeUntil.in`) is left out of the namespace (a class static keeps it). (4) Reads of `ParticleSystem.startLifetime`, `MeshRenderer.mesh` and similar are typed as the union a setter accepts. (5) `Collider` is an interface with only `gameObject`; contact hooks are not members of `Component` (a game writes them without `override`). (6) `setInterval`, `queueMicrotask` and other host globals are not declared (only `setTimeout`, `clearTimeout`, `console`). (7) `gen_ambient.py` and `ambient.d.ts` were edited by hand together (no Python on this machine to re-run it); re-run it once to confirm they agree. (8) The examples' own `package.json` scripts (`bun build`, `tsc -p .`) and the root `bun run build` no longer produce working games: bun would not bundle the engine without the imports; use `platypus-build`.

- **`project.json` is the only game config (2026-10-21, `M9-API-1321..1322`, `M9-TS-2722..2725`):** only `name` is required (error naming the file); omitted fields default silently (`${firstName}'s Game`, `${firstName}'s Company`, `private`, `0.1.0`); an explicit `""` stays empty and `build`/`package` warn per empty field (never `check`; `package` also warns when company/copyright/license are defaults); `package.json` is not read when a `project.json` exists and the `platypus` section fallback is deprecated; games are found by `project.json`; the 20 examples lost their `package.json` and `node_modules` and carry only `name` plus real settings (title, icon, layers, quality, startScene). Left: remove the `package.json` fallback in a later release; `platypus-build set <field> <value>` for editing.
- **Project file (2026-10-21, `M9-API-1300..1320`, `M9-TS-2700..2721`):** `project.json` replaces the `platypus` section of `package.json` (`docs/platypus-build.md`, "`project.json`"; loader in `platypus_core::project`, used by `platypus-build` and `platypus_app`), with template variables, platform sections, `license` and `platypus-build init`. Left: `${...}` in the `.scene.json` or CSS files is not supported; platform `android`/`ios` sections resolve only for a future mobile packager; the macOS full name comes from `id -F`/`dscl` and the Linux one from `getent` and are untested here; `Comments` is the only license field in the PE (no `.rc`-style license resource); the window title is only the initial value (the TypeScript Window API is the next task); no `platypus-build set <field> <value>` editor and no schema file for editors.

- **Top-level control flow (2026-10-21, `M9-TS-2849..2853`):** `if`/`else`, `for`, `for-in`, `for-of`, `while`, `do-while`, `switch`, `try`/`catch`, labelled statements, `throw` and bare blocks at the top of a module were silently dropped (the `_ => {}` arm in `lower_program`); they are now lowered into `init` in source order (`TopItem::Stmt`, the same lowering as function bodies), also in imported modules. Statements the compiler cannot lower are an `unsupported top-level statement` error with a line; empty statements, imports/exports and type-only declarations are the only ones skipped. `examples/windows` starts from plain top-level code instead of `start()`. Left: `register(..)` inside a top-level `if` is not seen by the component scan (it only reads direct top-level `register` calls).

- **External packages (2026-10-21, `M9-TS-2870..2879`):** only three pieces of `docs/research/platypus-external-packages.md` were built: bare imports resolve to `src/modules/` (a TypeScript package is used by copying its source in; `tsc` gets matching `paths`), plain top-level functions nothing refers to are not lowered, and a module's declarations cannot capture engine global names (renamed per module). Decided NOT to build, for now: `node_modules` resolution, a `packages` section in `project.json`, an allow list, licence generation and a copyleft check, `check-packages`, fetching from JSR/npm, a dynamic `any`, native plugins. Left in what was built: unreferenced classes and top-level `const` initialisers are still compiled (they may have side effects or be reached through registration); the module root is `<entry folder>/modules`; a package whose code is outside the subset still fails with the compiler's usual error.
- **Fog, god rays and exposure family (2026-10-21, `M9-API-1501..1506`, `M9-TS-2880..2884`):** the native API for `RenderSettings` fog / height fog / light shafts / auto and fixed exposure, `Camera` exposure read-back and physical camera, `Light` volumetric / pyramid spot / light meter, `FogVolume`, `RenderStats`, `GraphicsSettings`, the quality pipeline fields, `Mesh.createFlat`, `Random.pick`; the facade static write `__xw_<Namespace>_<name>` (object-valued namespace properties from variables and `cond ? {..} : null`). 17 more examples build (`docs/research/platypus-examples-on-platypus-ts.md` section 10). Open: indexing a struct by a literal-union key (`state[key]`), `keyof typeof x`, `arr.indexOf(tag)`, `Record<string, V>`, `new Array(n).fill(x)`, `Array.from({ length }, f)`, spreads into `Math.min`, `x[i]! += d`, `.flat()` (the examples were edited instead); an indexed write at an array's length is out of bounds (no check: it crashed a program silently); scene-file keys for fog and exposure; `platypus_app` must be rebuilt after a compiler change (it lowers and compiles the bundle itself).

## TypeScript gaps (audit 2026-10-29)

Every known gap against TypeScript / ESM, once, with how it fails and the effort (S/M/L). From the read-only audit of
2026-10-29 and the native editor's build; `[x]` is fixed (with its test), silent differences are also rows of
`docs/differences.md`. Order agreed for what is left: (1) the editor's gaps and `--check-unused` (done), (2) the editor's native
API (done), (3) cheap diagnostics and library holes, (4) null access as a catchable `TypeError`, (5) dynamic `any` / `unknown`
P0-P1, (6) generics and modules, (7) a checker of our own and a conformance harness.

**Fixed for the editor (2026-10-29):**
- [x] Type guards `x is T` / `asserts x` (were "unsupported type annotation"); `filter` with a guard keeps `T`, reading a
  tagged union's member out (`M9-TS-3500`, `3512`).
- [x] Unions of interfaces: common fields, a tag compared with a literal narrows (also after an early return), an object
  literal or spread takes its member; `"k" in s` on a union of object literal types (`M9-TS-3501`, `3506`).
- [x] Generic inference from the context: a closure's body from the call's expected result, an object argument's plain
  properties before its closures (generic components), an object literal argument after the arguments that fix `T`,
  `readonly T[]`, generic aliases, defaults of a bare generic interface (`M9-TS-3502`, `3512`).
- [x] `Record<Exclude<U, X>, V>`, `Record<I["k"], V>`, `obj[k]` with `k` a `const` of literals, computed keys of a literal
  union in object literals (`M9-TS-3503`, `3504`; computed keys of other kinds are still an error, S11 below).
- [x] `new Map(pairs)` of a non-literal array, template parts of different types, `JSON.stringify` of a tagged union
  (parsing into one: J2 below) (`M9-TS-3505`, `3506`).
- [x] `Promise.resolve` / `reject` typed by the context; a defaulted literal closure parameter left out; `const` arrows of a
  block calling each other (written types); `Error.stack` as the first line; a call typed `null` is no longer dropped
  (`M9-TS-3507`, `3511`).
- [x] Closures inside a callback passed at the top level share its variables (`M9-TS-3512`).
- [x] `any` / `unknown` / `never` say so with the line, also through a bare `T = unknown` default (`M9-TS-3508`); the
  test ID range 3400..3499 stays with the dynamic-values plan (the editor's tests use 3500..).
- [x] `platypus-build build|check --check-unused` lowers the functions nothing calls (`M9-TS-3509`).

**Indexed access types (2026-10-30, `lower/types_index.rs`):**
- [x] T8: `T["a" | "b"]`, `T[keyof T]` over fields of different types are the union of their types; `o[k]` reads into it
  (`M9-TS-3528`). `PlatypusStyle["prop"]` for every property kind, through the prelude's `__PlatypusStyleProps`
  (`M9-TS-3525`); `Alias["k"]` of a type literal, nested `A["b"]["c"]`, `typeof x["k"]`, `typeof a.b` (T6's `typeof`
  part), tuple `T[0]` / `T[number]` / `T["length"]` (`M9-TS-3526`, `3527`); `T[K]` in generic functions, methods and
  aliases, a copy per set of key literals (`TypeEnv::keys`, `M9-TS-3529`); errors with the line (`M9-TS-3530`).
- [ ] T8b: a key parameter whose argument is a `string` (or a value of type `keyof X`) is an error ("which keys `K` is
  is not known in this call"); `T[K]` in a generic class's own type parameters (not a method's) gets no key literals. S-M.
- [ ] T8c: `o[k] = v` through a key naming fields of different types is an error (TypeScript wants their intersection). S.
- [ ] T8d: unions are ordered (`number | string` and `string | number` are two tagged unions that do not convert into
  each other); `T[K]` follows field order so it meets `o[k]`. M.
- [ ] T8e: a `StyleValue` (`number | string`, e.g. `PlatypusStyle["opacity"]`) compared with a number or string
  ("cannot compare"). S.

**Found by the editor, still open:**
- [ ] Arrays of mixed types (`["row", sel && "selected"]`, `(string | false)[]`) become tuples, so `.filter(Boolean).join()`
  fails: make an untyped array literal of differing element types an array of their union. M. (The editor uses `cx(..)`.)
- [ ] An array of `A` where `(A & { up?: true })[]` is expected: struct conversion of array elements. M.
- [x] `{...(cond ? obj : {})}`: in an object literal with a known type it already worked; in JSX the taken arm's props
  are set and `{}` sets none (`lower_ui_fields`, `M9-TS-3532`; the editor's Project rows use it).
- [x] `new Set(cond ? set : [])` / `new Map(..)`: each arm makes the collection (`lower_new_collection`, `M9-TS-3531`).
- [x] `useState(shown)` with a local `shown` named like a top-level function (the editor's `scene-edit.ts` `shown`)
  was taken as a lazy initialiser; the JSX pass now resolves the name through the enclosing functions' and blocks'
  bindings (`Finder::locals`, `M9-TS-3533`).
- [ ] A self- or forward-referencing closure needs its return type written (`const f = (..): ReactNode =>`). TypeScript
  asks for it too under `noImplicitAny` (TS7023, "referenced directly or indirectly in its own return expression"), so the
  editor keeps those annotations. Still a gap: a function value whose result differs from the expected one (an element
  where `ReactNode` goes) is not adapted (no adapter closure). M.
- [ ] A top-level function parameter's non-literal default needs a type (`(d = defaultSceneCamera().distance)`). S.
- [ ] A top-level `const f = (a, b: Lit = "x") =>` (a declared function) used as a value where `b` is optional: the
  default is not applied through the closure. S-M.
- [ ] A generic arrow `<T>(x: T) =>` in a `.ts` module fails to parse once bundled with TSX. S.
- [ ] `addComponent` of a class that is not registered panics across the FFI (aborts the process) instead of throwing. S.

**Syntax:** S1 class expressions (error, M); S2 decorators other than a bare `@property` (error, M-L); S3 `static {}` blocks,
`accessor` fields (error, S-M); S4 assigning to a static accessor (error, S); S5 tagged templates and `String.raw` (error, M);
S6 `import()`, `import.meta`, `new.target`, `#x in obj` (error, M each); S7 `<T>x` casts in `.ts` (error, S); S8 local
`class` / `interface` / `type` in a function (error, M); S9 `namespace` / `module` / `declare` (error; decided not to build);
S10 `catch ({ message })` (error, S); S11 computed keys other than literal unions and `Record<string, V>` (error, M); S12 JSX
spread children, element-valued and namespaced attributes (error, S-M); S13 optional chains other than member, call and `!`
(error, S); S14 some operators (`>>>` / unary `+` on bigint, others "unsupported operator": not enumerated) (error, S).

**Types and generics:** T1 `any` / `unknown` / `never` (clear error now; needs D1, L); T2 conditional, `infer`, mapped
and `this` types (error, L; a template-literal type is a `string`, unchecked); T3 a generic interface with `extends` (error, M); T4 index signatures other than
`[k: string]: V`, call signatures in object types (error, M); T5 rest tuple elements (error, M); T6 qualified type names other
than `Intl.X` / `React.X` (error, S; `typeof a.b` is done); T7 `typeof` of a non-constant (error, S); T8 done (above); T9 intersections of non-objects or clashing fields (error, M); T10 a generic
function as a value or exported (error, M); T11 generic statics, accessors, constructors (error, M); T12 an override of a
generic method is not dispatched virtually (**silent**, M); T13 constraints not checked (silent, relies on `tsc`); T14 an engine
subclass handle does not convert to its base (`const c: Collider2D = go.getComponent(BoxCollider2D)!`) ("type mismatch",
S-M); T15 interfaces declared twice across files are renamed, not merged (silent, M); T16 at most 64 fields per struct (error,
M); T20 `Promise<A | B>`, `Promise<Promise<T>>`, a promise type never written (error, M); T21 `x as any` is a no-op (silent,
needs D1); T22 `let x: number;` starts at 0 (silent); T23 `let x = null` without a type, untyped parameters, a recursive
function without a return type (error, S).

**Classes:** C1 a subclass before its base (error, S); C2 extending engine classes other than `Component` (error, M); C3
`instanceof` of engine classes (error, M); C4 `register(cls)` of a variable (error, L); C5 `getComponents(EngineClass)` (not
built, S).

**Async and generators:** A1 async generators (error, L); A2 `yield*` as an expression, `yield` in a `yield` operand,
`return yield` (error, M); A3 untyped generators, a return type other than the yield type (error, M); A4 `Iterable<T>`
annotations take generators only (type error, M); A5 `Promise.all` over different types unless awaited at once (error, M);
A6 `Promise.withResolvers` in generic code (error, S); A7 `then(f, g)` with different result kinds (error, S; the
`Promise.reject` typing part is fixed); A8 `AbortController` (missing, M, M6); A9 timers run on the game clock, no extra
`setTimeout` arguments (silent, S); A10 unhandled rejections are logged (settled, `M9-TS-2916`; the doc row was wrong); A11 no
coroutines on WebAssembly (L).

**Modules:** M1 `import * as ns` of a project file (error, M); M2 `export * as ns` (error, S-M); M3 anonymous
`export default function` / `class` (error, S); M4 no `node_modules` (by design); M5 a module may not capture engine global
names (silently renamed), unused top-level functions are skipped (see `--check-unused`); M6 JSON imports and import attributes
(not handled, S-M); M7 import cycles deeper than 32 (error, S).

**Library:** L1 `console.log` of objects and arrays, `console.table/time/assert/group/trace` (error, S-M; M with D1); L2
`toPrecision` / `toExponential` (error, S); L3 `Object.create/defineProperty/getPrototypeOf/setPrototypeOf/is/groupBy`,
`Map.groupBy` (error, S for `is` to L); L4 `Object.freeze` does not freeze (silent, S); L5 array `with`, `toSpliced`,
`copyWithin` (error, S); L6 `sort()` without a comparator over other element kinds (error, S); L7 `indexOf` over object arrays
(S); L8 `Map.get` over boolean or literal values, `new Map(map)`, map spread (error, S); L9 `structuredClone` of `Map` / `Set` /
functions, cycles (error / silent, M); L10 weak collections are strong, no `FinalizationRegistry` (silent, M); L11 `globalThis`,
`Proxy`, `Reflect`, `eval` (decided not to build); L12 symbols as keys, other well-known symbols (M); L13 typed-array gaps
(`docs/differences.md`, M); L14 BigInt gaps (M); L15 legacy string methods, `String.raw`, `isWellFormed` (error, S); L16
`Error.stack` frames, `captureStackTrace` (the first line is built; S); L17 `Object.keys` / `in` / `hasOwn` over fixed fields
(silent, needs D1).

**Dynamic values (`docs/research/platypus-dynamic-values.md`, P0-P7):** D1 no dynamic value (L); D2 untyped `JSON.parse`
(error, M, P1); D3 `catch (e: unknown)` ignores the annotation, `e` is always an `Error` (silent, M, P1); D4
`Record<string, unknown>` / `object` become `PlatypusStyle` with the UI prelude (silent special case, M).

**Run time:** R1 `throw` of a non-`Error` (error, M, P1); R2 **a null property access is an access violation that ends the
process, not a catchable `TypeError`** (fatal, M-L: null checks before loads, or the trap turned into a `TypeError`); R3 `null`
and `undefined` are one value (silent, L, needs D1); R4 `==` is `===`, `-0`, NaN payloads (silent, M); R5 deep recursion
overflows the native stack (crash, M; inferred); R6 a destroyed component's pending `await` goes on (by design).

**JSON, RegExp, Date, Intl:** J1 replacer, reviver, `toJSON` (error, M); J2 `JSON.parse` into a union (error, M); J4 JSON of
`Map` / `Set` / `Record` is `{}`, `Date` in JSON types (silent / error, M); J5 RegExp groups that did not take part are null,
module-level regex constants lack group info, eager `matchAll` (silent, M); J6 `Date()` without `new`, spread arguments, exotic
legacy strings (error / NaN, S); J7 Intl options read at compile time, missing members (error, M-L); J8 Temporal ISO calendar
only, rewritten by text (silent, M).

**Tooling:** G1 no checker of our own: the compiler trusts `tsc`, which is optional (L: `tsgo` or `--check`,
`docs/research/platypus-ts-type-checking.md`); G2 functions nothing calls are skipped (`--check-unused` added); G3 one error per
build (M); G4 poor locations: generator byte offsets, facade chunks, lazily lowered functions (S; type annotations name the line
now); G5 Rust `Debug` names in unsupported-type messages (S; `any` / `unknown` / `never` fixed); G6 no conformance harness
against bun or test262 (L); G7 no compile cache (M); G8 stale docs (settled 2026-10-29: generics and destructuring assignment
work, `M9-TS-3513`; unhandled rejections are logged; string `replace` / `match` / `localeCompare` / `normalize` and setters in
object literals exist).

## Language and library features the examples work around (to review and prioritise)

Every item is something an example had to be rewritten for (or still avoids) because platypus_ts does not take plain TypeScript there. Source:
`docs/research/platypus-examples-on-platypus-ts.md` sections 1c, 6 and 10 and the agents' notes above. Tick an item when the workaround can go.

**Language features**
- [x] Computed property access with a literal-union key: `state[key] = !state[key]` where `key: keyof typeof state` or a union of some names (`M9-TS-3165`, `3169`).
- [x] `Record<string, V>` (a `Map<string, V>`) and `Record<Union, V>` as dictionaries, indexed by any string (`M9-TS-3167`, `3168`).
- [x] Tables indexed by a tag/enum-like string (`TINTS[state.tint]`, `(typeof TINTS)[Tint]`, `(typeof NAMES)[number]`, `M9-TS-3166`, `3170`).
- [x] `string[]` parameters of `#[platypus_api]` functions (`&[String]` / `Vec<String>`): macro, manifest type `strings`, `ExtTy::StrArr`, `string[]` declarations, QuickJS arrays, test API `Words` (2026-10-22, `M9-API-1621`, `M9-TS-3323`). No function of `platypus_engine_api` takes a joined or newline string today, so nothing was converted (`AnimationClip.curveAt` returns a `|`-joined triple, a result, not an input list).
- [x] `Object.assign` / `{ ...dict }` into an object by key at run time (dictionary sources, type check, once-per-site warning for a key that is no field), several sources, `Object.keys/entries` of objects verified (2026-10-22, `M9-TS-3319..3322`).
- [x] `this` in object literal methods and accessors is the literal's object (2026-10-22, `M9-TS-3317/3318`); only a detached method keeps its binding.
- [x] `flat(depth)` with a run-time depth flattens exactly that deep, as JavaScript; the result is an array of a union of the nesting levels (2026-10-22, `M9-TS-3313..3316`).
- [x] `Array.prototype.indexOf` on arrays of string literals; `Array.from({ length }, f)` and `Array.from(xs, f)`; `new Array(n).fill(x)`; `.flat(depth)` (2026-10-22, `M9-TS-3020..3034`).
- [x] Spread arguments: `Math.min(a, ...list)`, `Math.max(...xs)`, `f(...xs)` into rest and fixed parameters (not into function values or `new`).
- [x] A static function as a value: `const hex = Color.hex`, `xs.map(Math.abs)` (2026-10-22, `M9-TS-3128/3129/3135`).
- [x] `null as T | null`, `[] as T[]`, `{} as T` casts in object literals/consts (`M9-TS-3130`).
- [x] Class fields without an annotation whose initialiser is a constructor call (`private a = new Float32Array(n)`), `NaN`, `-Infinity`, `Math.*`, `new Int8Array(n).fill(x)`, `[] as T[]` (`M9-TS-3131/3137`).
- [x] Non-null assertion on an assignment target: `x[i]! += d`; closures over destructured `const [a, b]` (`M9-TS-3132/3133`; the closures already worked).
- [x] Tuples: `.length`, rest patterns, spread of tuples (into arrays and fixed parameters), assignment through `t[0]`, tuple parameters (2026-10-22, `M9-TS-3149..3151`). A tuple type as a rest parameter (`...args: [string, number?]`, `M9-TS-3195`).
- [x] Defaulted closure parameters of type `boolean` may be left out (`M9-TS-3154`).
- [ ] `register(Class)` inside a top-level `if` is not seen by the component scanner. Kept as a clear compile error (decided 2026-10-22): the host starts registered classes before any code runs, so a registration that depends on a run-time condition cannot be made lazily without changing that contract.
- [x] Arrays: an indexed write at `arr.length` (`arr[arr.length] = x`) has no bounds check and crashed a game with a segfault. Fixed 2026-10-22: it appends like JavaScript; a write further out (or negative / NaN) throws a `RangeError`.
- [x] A skipped (unreachable) function is only checked by `tsc`, never by our compiler: `platypus-build build --check-unused` (and `check --check-unused`) lowers the program's own functions too (2026-10-29, M9-TS-3509). All 54 examples and the editor pass it.
- [x] `Texture | null` vs `Texture | undefined` and `string | undefined` from indexing: handle properties are `T | null`, name-valued properties read as the union (`#[nullable]`, `#[ts_type]`, `M9-TS-3164`).

**Library / API gaps that forced rewrites**
- [x] `input.gamepads()` / `input.on("gamepadconnected" | "gamepaddisconnected", ..)` as in QuickJS: `{ id, name, connected, buttons, axes }` and the event `{ gamepad }` (`M9-TS-3198`; `buttons` / `axes` are built by the facade over the native per-name readers, not a native record: `platypus_api` has no dictionary results). `form` already used `input.gamepads()`.
- [x] `await SceneManager.loadSceneAsync(..)` returns a promise (a facade over the native token form `loadSceneAsync$n`; it was there since the facade work but never rejected and hung because `Scene.path` was set after `onSceneLoaded` ran; fixed 2026-10-22, `M9-API-1512/1513`). `Prefab.load(path)` too (`M9-API-1517`).
- [x] `UiTarget.type` needs the host to set a resolver (`platypus_ts_ui::set_target_kind_resolver`): `platypus_app` does it before every event (`native::set_target_kind`, commit `e8b3847`, test `native.rs`), so `examples/form` Enter-to-save works.
- [x] Scene-file keys for fog (QuickJS `scenes.ts` saves them; `platypus_render_api` now does, `M9-TS-3201`); `scenes.ts` saves no exposure keys, so there are none to match.
- [x] `Light.measureIlluminance` resolves after the next physics step (`Physics.afterNextStep(f)`, `M9-TS-3199`, `3200`).
- [x] `Mesh.createFlat` needs all four lists (positions, triangles, normals, uv); allow a partial data object (2026-10-22, `M9-TS-3187`).
- [x] `answer` handlers (and raw `onRequest` handlers) may return a promise: the reply goes out when it resolves, a rejection fails the asker (`M9-TS-2894..2896`).
- [x] Facade chunk triggers for `.on(` / `.emit(`: the window chunks now also need `Window` in the program (`//# when a&Window|..`, `M9-TS-3197`).
- [x] A local typed as a string-literal union assigned from an optional field of the same type (`let mode: M = "auto"; if (o.mode !== undefined) mode = o.mode;`) panicked Cranelift (`declared type of variable .. doesn't match`); also with `as M` and with a `const` copy. Fixed 2026-10-22: `Nullable(Lit)` is an `i32` like `Lit` with tag 0 reserved as "no tag" (`M9-TS-3000..3012`); `as M` lowers literals as members; `Map.get` and `join` over members; `keyof typeof X` / `keyof Iface` are the key union; a non-member literal in a typed declaration is a compile error; `Spline.mode` is `TangentMode` again.
- [x] `Partial<T>`, `Required<T>`, `Pick<T, K>`, `Omit<T, K>`, `Readonly<T>` over interfaces, aliases, records, classes and engine classes, as option bags too (2026-10-22, `M9-TS-3120..3127`). [x] `ReturnType<typeof f>` / `typeof f<string>`, `Parameters`, `Awaited`, `NonNullable`, `Exclude`, `Extract` (`M9-TS-3177`).
- [x] Classes assigned to an interface (`class C implements I`, `I[]` holding different classes, `extends` between interfaces): dispatch by the object's struct id through per-interface tables (2026-10-22, `M9-TS-3140..3147`, `docs/platypus-ts.md` "Interfaces implemented by classes").
- [x] Object-literal getters (`get scene() { .. }`, `M9-TS-3180`). [ ] Setters in object literals, computed keys in object literals (other than a `Record<string, V>` literal). [x] Spread call arguments for function values (`rnd(...range)` works for declared functions with fixed or rest parameters and for tuples). [x] Destructuring assignment (`[a, b] = [b, a]`, `M9-TS-3152/3153`).
- [x] `Map.get` / `Set` over engine handles as values; `.pop()` / `.shift()` / `.at()` over handle arrays (`M9-TS-3184`, `3189`). [ ] `indexOf` over object arrays.
- [ ] Line numbers in errors from facade chunks and from code appended after the user's source point past the file.
- [x] Facade chunk classes (`Spline`, `SplineAnimate`) were not `@property` aware (no scene saving): both are native now and `SplineAnimate` saves its fields and spline (2026-10-22).

## The original example sources (2026-10-22)

Goal: the sources on `main` compile as written (a later step restores them). Measured with `platypus-build check` on a scratch copy:
**38 of 40 build and pass `tsc`**; `physics` and `light-units` stay on the `Rigidbody` / `BoxCollider` form (their only `tsc` errors are
the retired `physics` object API). Done for it (details: `docs/platypus-ts.md` "The original example sources", `docs/research/platypus-examples-on-platypus-ts.md`
section 12, tests `M9-TS-3174..3189`): inference of `T = void` from a closure, `ReturnType` / `Parameters` / `Awaited` / `NonNullable` / `Exclude` /
`Extract` and instantiation expressions, unions of different types with narrowing by path, a written return type on an inlined callback,
self-referring constant objects, object-literal getters, `x.constructor` construction, generic arrows in functions, big `Record<Union, V>`
as dictionaries, strings to union members (`StrTag`), values of union type keep the union in literals, `a && b` string and object,
`LineRenderer2D.points`, `playback.onEnded = f`, `new Color(...rgba)`, handle `Map` values and `pop`, partial `Mesh.createFlat`.
Not done: narrowing for `instanceof` / number truthiness on tagged unions (`in` and a tag compared with a literal narrow since 2026-10-29); a `Record<Union, V>`
with up to 48 keys is still a struct.

## Dictionaries and leftovers (2026-10-22, `M9-TS-3190..3195`)

`Object.keys` / `values` / `entries` / `fromEntries`, `for ... in`, `in` with a run-time key and `delete` over `Record<Union, V>` structs
(optional properties are there while set) and `Record<string, V>` dictionaries; `["a", "b"] as const` as an array of union members;
`flat(depth)` with a depth known at run time; a tuple as a rest parameter. Left: `for ... in` over arrays, `Object.assign` into a
dictionary, setters in object literals, `register(Class)` in a top-level `if` (a clear error; see its item above).

## Promises audit (2026-10-22)

Every async or promise-returning API of `js/engine/src` against the compiled tier (`platypus_engine_api`,
`platypus_scenes`, `facade.ts`, `engine_ts.d.ts`). "Facade" = a promise written in TypeScript over native
callbacks in `tools/platypus_ts/src/lower/facade.ts`.

| API (QuickJS tier) | Where | Compiled tier before | Now |
|---|---|---|---|
| `SceneManager.loadSceneAsync(path \| data, mode): Promise<Scene>` | `scenes.ts` | Facade promise, but it never rejected and never resolved for files loaded by the host (`Scene.path` was set after `onSceneLoaded`) | Resolves in the tick that read the file, rejects with an `Error` (missing, bad JSON, outside the folder); `M9-API-1512/1513` |
| `Prefab.load(path): Promise<Prefab>` (cached per path) | `scenes.ts` | Missing (only `Prefab.fromData`) | Facade over `SceneManager.loadPrefabAsync` + `onPrefabLoaded`; no per-path cache; `M9-API-1517` |
| `Texture.whenLoaded()`, `Model.whenLoaded()` | `textures.ts`, `models.ts` | Facade polling every frame, resolved also when loading failed | Rejects with an `Error` when failed (QuickJS rejects with the source in the message) |
| `physics.raycast(origin, dir, max): Promise<RayHit \| null>` (after the next tick) | `physics.ts` | `Physics.raycast` answers at once (`HitRef`) | Unchanged on purpose: a different shape, no promise to return |
| `Physics2D.raycast` (async) | `physics2d.ts` | sync like the 3D one | Unchanged (same) |
| `Light.measureIlluminance(point, normal?): Promise<number>` | `rendering.ts` | Facade promise, answers at once | Resolves after the next physics step (`Physics.afterNextStep`); before: the next microtask (the QuickJS one waits for the physics tick) |
| `Task.delay`, `delaySeconds`, `frame` | `clock.ts` | Prelude class over `setTimeout` | Unchanged |
| `Window.request`, `Window.ask` | `window.ts` | Facade promise per reply type | Unchanged |
| `Window.answer(name, f)`, `onRequest(f)` handlers | `window.ts` | A returned promise was serialised as JSON | An async handler replies when it resolves and rejects the asker on failure (`..P` facade, `Window.replyError`) |
| `setTimeout`/`setInterval`/`queueMicrotask` | prelude | Existed; an exception in a callback vanished | Reported (`Uncaught exception in a timer or microtask`) |
| Unhandled promise rejection | none (silent) | Silent | Logged once as an error after the microtask drain |
| `audio` `onEnded`, `input.on`, `Assets.onChanged`, `Application.onFocusChanged`, scene events | `audio.ts` etc. | Closures (`Callback`) | Unchanged (callbacks, not promises) |
| `fetch`, `WebSocket`, file reads | none yet | none | M6 (Script IO) |
| `AbortController` / `AbortSignal` | none in either tier | none | Not implemented (report only) |

Behaviours checked 2026-10-22 (all with GC stress where it applies):

- `string[]` results of `#[platypus_api]` (2026-10-22, `M9-API-1619/1620`): `Vec<String>` is `list:string` in the manifest (count from the shim, strings through `platypus_api_result_str`), `ExtTy::StrList`, `Assets.list` returns `string[]`; QuickJS `Assets` in `js/engine` hands `exists` / `list` / `packOf` to the native namespace. Not done: string array *parameters*.
- Language leftovers (2026-10-22, `M9-TS-3216..3225`, `bench/lang2_cases.ts`): `for...in` over arrays (indices as strings), `Object.assign` into dictionaries (and `{ [k: string]: V }` as a dictionary type), setters in object literals (hidden `_p_set_` closure field, `Place::Thunked`), narrowing kept inside closures over a never-assigned variable, `in` / `instanceof` / truthiness narrowing of unions (tag tests and member truthiness), `flat(depth)` with a run-time depth (throws when smaller than the nesting: documented). Not done: `this` inside object-literal accessors and methods.
- Top-level `await` (2026-10-22): the module's code is an implicit async function started by `ts_init` (`docs/platypus-ts.md` "Tuples, destructuring assignment and top-level `await`", `M9-TS-3157..3161`). Per-module ECMAScript ordering and gating, the startup state for the host, and `register()` as a run-time call: `M9-TS-3202..3215`, `M9-API-1615..1618`. `Promise.all([a, b])` over different value types, awaited on the spot, gives a tuple (`M9-TS-3155/3156`).
- `async onStart()` / `onUpdate` etc.: work (the component hooks return a promise that is dropped; a rejection is reported as unhandled). `M9-API-1513`.
- Async JSX handlers (`onClick={async () => ..}`): work; a callback typed to return nothing may return a value (it is evaluated and dropped). `M9-API-1515`.
- `for await (const x of xs)` over arrays/iterables: works (rewritten). Async generators, `AsyncGenerator`, `Symbol.asyncIterator`: compile error naming async generators.
- `await` inside `try`/`catch`/`finally`: works (`M9-TS-2910`).
- `Promise.withResolvers<T>()`: works (not inside generic code).
- `await new Promise(r => setTimeout(r, ms))` with `resolve` passed as the callback: works (`setTimeout` takes `(arg?: number) => void`).
- Hot reload: pending timers and awaits of the old code are dropped (`ts_rt_events_clear`); the new code starts its own. `M9-API-1516`. (Before, an old timer fired into the new world.)
- A component destroyed while it awaits: the coroutine continues (no cancellation, JavaScript semantics), engine calls on the dead object are no-ops; check `go.exists()`. `M9-API-1514`.
- Generics: `load<T>(): Promise<T>`, `Promise<T[]>`, `Promise<Map<string, T>>` (when `Promise<Map<string, number>>` is written once), generic async classes and recursion, `T` inferred from `Promise<T>` parameters and promise-returning closures: work. Still failing, each with a message: `Promise.all([a, b])` over different value types when not awaited on the spot, `Promise<Promise<T>>`, `Promise<Alias>` where the alias is never written in a `Promise<..>` (`the program never writes Promise<..>`), `Promise.withResolvers` inside generic code, generic *methods* (`map<U>(..)` in a class: "generic methods are not built").

- [ ] **Asset packs default (decided 2026-10-22, `docs/research/platypus-virtual-fs-plan.md` section 6):** `platypus-build package` stays loose (`data/`) until packs are tried and tested on real games; then flip it to write `assets.dat` + scene packs by default with `--loose` for debugging/modding. Also revisit storing cooked-only textures (currently both forms) to reduce bloat; DirectStorage / `CODEC-1` later.
- [ ] **Enums as indexes (decided 2026-10-22, `docs/research/platypus-generated-dts-plan.md` section 6):** native name/index converters per enum, constant literals converted at compile time.

## Every declaration generated from Rust (plan: `docs/research/platypus-generated-dts-plan.md`, decisions in its section 6)

- [x] P0: ratchet over the hand-written `.d.ts`, `facade.ts` chunks and Python (`M9-TS-3100`), no name declared twice (`M9-TS-3101`), API surface snapshot with `PLATYPUS_BLESS=1` (`M9-TS-3102`).
- [x] P1a: native `Shapes` (`platypus_math::outlines`); `#[platypus_api]` enums, records, list results, `#[rest]`, `#[options(after = ..)]`, `like`, `#[nullable]` and doc comments (manifest JSON and `types_dts.rs`); `Shapes` facade chunk and its `engine_ts.d.ts` lines deleted; `platypus_native::install` in the app's QuickJS runtime (`M9-API-1600..1602`, `1610..1614`, `M9-TS-3103`, `3110`, `3111`).
- [x] P1b: native `Spline` (`platypus_math::spline`, `platypus_engine_api::spline`; the `Spline` facade chunk and its `engine_ts.d.ts` lines deleted; `M9-API-1603..1605`, `M9-TS-3112`, `3114`).
- [x] P1c: native `SplineAnimate` component (`platypus_engine_api::spline`: kind with every field saved, the `Spline` inline asset in scene files, host `update` next to `animation::update`; the last facade chunk and its `engine_ts.d.ts` lines deleted; the QuickJS component is a thin script component over `SplineAnimate.pose` / `progress` / `facing`; `M9-API-1606`, `1607`, `M9-TS-3113`, `3115`, `3116`).
- [x] P2a: enums and nullables (2026-10-22). `#[platypus_api]` got `enums(extern X = ..)` (an enum another namespace declares), explicit variant names (`Windowed = "Windowed"`), `Option<Enum>` parameters (a left-out or unknown name is none), `Vec<Enum>` results, `Option<f64>` / `Option<String>` / `Option<V>` (`number | null`, `string | null`, `Color | null`; the none mark is NaN, NaN fields or `platypus_api_result_none()`), `#[nullable(undefined)]` and `aliases(A = B)`; the manifest carries `opt:`, `list:enum:` and `aliases`, the compiler lowers them (`ExtTy::Opt`, `EnumList`, `Manifest::aliases`, `Record<EnumName, V>` through `lit_members`), `types_dts.rs` prints them. `FogMode`, `VolumeDetail`, `PrimitiveType` (+ `PrimitiveMesh`), `FilterMode`, `TextureWrapMode`, `FullScreenMode`, `ShadowQualityMode`, `WindowStyle`, `LoadSceneMode` are native enums; `Camera.main`, `Window.parent` / `focused` / `at` / `find` / `byHandle` are `T | undefined`; `Window.openError`, `RenderSettings.fixedExposure`, `LineRenderer2D.fillColor` are native nullables (the `color?` field type of `gen_components.py`); `AnimatorController` is an alias. 7 facade chunks (47 -> 40) and about 40 lines of `engine_ts.d.ts` deleted. Tests `M9-API-1622..1628`, `M9-TS-3340`. Left from 2a: `Collision2D` (an alias of `Collision`, which waits for records), the `WindowOptions` / `TextureOptions` bags (2b).
- [x] P2b, first part (2026-10-22): result records with `string` fields (`Screen.windowSize` / `currentResolution` / `resolutions`, `Animator.getCurrentAnimatorStateInfo()`), `#[options(name = "X", only)]` with parameter docs (`TextureOptions`, `WindowOptions`), `readonly` array parameters; 2 more facade chunks (40 -> 38). `M9-API-1629..1631`, `M9-TS-3341`, `3342`. Left for P2b: records as parameters (`RenderSettings.skybox` / `bloom` / `heightFog` / `volumetricFog` / `autoExposure`, `setLODs`, `Cloth`, `ParticleSystem`, `Mesh.create`, `loadFont`, `GameObjectOptions`): a JSON wire form for `Option<Record>` is the plan (docs/research/platypus-generated-dts-plan.md section 10); `Gamepad.buttons` / `axes` need dictionary records.
- [x] P2b, second part (2026-10-22): records as parameters: `Option<R>` of a `records(..)` record (optional keys, `Option<Option<T>>`, `pairs`, `<V>Like`, `as "key"`) is JSON text on the wire; compiled code stringifies the struct or writes a literal key by key (`null` kept); `RenderSettings.skybox` / `bloom` / `heightFog` / `volumetricFog` / `autoExposure` are native record properties (4 facade chunks, `split_rule`, `lower_split_assign`, ~90 lines of `engine_ts.d.ts` gone); the enums `VolumeDenoising`, `ExposureMetering`. `M9-API-1632`, `1633`, `M9-TS-3343`. Left: `&[Handle]` parameters, `Vec<Record>` parameters, records holding handles (`setLODs`, `Cloth`, `ParticleSystem` keys, `bones`), `MeshData`, `loadFont`, the compiler-read option bags, `Gamepad` dictionaries, `Collision` (plan section 11).
- [x] P4a: UI tables in Rust (`crates/platypus_ui/src/tables`: style properties with types and docs, attributes, elements, events, React names), tested against `apply_style`, `Attrs::set`, the event slots and the event kinds (`M9-TS-3301..3308`); `ui_dts.rs` writes the UI declarations and the compiled prelude from them (`M9-TS-3309`, `3310`); `gen_ambient.py`, `ambient.d.ts` and `ambient.ts` deleted; the ratchet is strict for `.d.ts` except `engine_ts.d.ts` and lists the hand-written TypeScript compiled into games with a reason each (`M9-TS-3100`). Deliberate surface additions: `PlatypusStyle.flexFlow` / `WebkitBackgroundClip`; `PlatypusProps.draggable` / `defaultValue` / `readOnly` / `open` / `modal` / `closedBy` and `onContextMenu` / `onCancel` / `onClose` / `onGotPointerCapture` / `onLostPointerCapture`. Fixed on the way: `scale: 1` and `perspective: 800` (numbers, as declared) were rejected by `apply_style`; `onFocus` / `onBlur` listened to `focus` / `blur`, which the interaction layer never sends (now `focusin` / `focusout`, `M9-TS-3311`).
- [x] P4a-events: the UI events and attributes end to end (`M9-TS-3324..3334`). `onFocus` / `onBlur` (+ `Capture`) tested through a real `Interaction` in a compiled game and in the native reconciler (`platypus_ui_native::event_of`; non-bubbling events still run the target's own handlers, which `onMouseEnter` / `onPointerEnter` needed); new props `onTransitionRun` / `Start` / `End` / `Cancel`, `onAnimationStart` / `Iteration` / `End` / `Cancel`, `onDragStart` / `Drag` / `DragEnter` / `DragOver` / `DragLeave` / `Drop` / `DragEnd` (with `dataTransfer`: the compiled `UiDataTransfer`, `files`, `setDragImage` through `Ui.dragImage`), `onMouseMove` / `onMouseEnter` / `onMouseLeave`, and the fields `relatedTarget`, `propertyName`, `animationName`, `elapsedTime`, `pseudoElement`, `dataTransfer`, `dropped`; `animationiteration` / `animationend` report the spec's `elapsedTime`. `title`, `cols` and `type` are implemented (stored, selectable; `cols` sizes a textarea) and no attribute is `Compat` any more. The tables test requires every `EventKind` to have a prop or a reason in `NOT_EXPOSED`. Deliberate surface additions: `PlatypusAnimationEvent`, `PlatypusTransitionEvent`, `PlatypusDragEvent`, `PlatypusDataTransfer`, `PlatypusDroppedFile`, `PlatypusFocusEvent.relatedTarget` and the 17 props above plus `onMouseEnter` / `onMouseLeave` / `onMouseMove`. Left: tooltips for `title`, `type` behaviour (password masking, numbers), `pseudoElement` for generated boxes.
- [ ] P4b: the hook helpers of `ui.ts` (`useState`, `useReducer`, `useRef`, `useMemo`, `useContext`, `useImperativeHandle`, `useSyncExternalStore`) as Rust lowering over the `platypus_ts_ui` slots: they are generic over the program's types, so each use is monomorphised by the compiler; not done (see the ratchet entry in `generated_dts.rs`).
- [x] P2 enums and nullables, records and option bags, lists, subscriptions and promises, remaining logic; P3 built-ins; P5 delete `engine_ts.d.ts` and make the ratchet empty (2026-10-22: `facade.ts`, `engine_ts.d.ts`, the supplement mechanism and `WINDOW_REPLIES` are gone; `builtins.rs` is the compiler's declaration table; `M9-TS-3100` is strict).
- Found while porting: the old compiled `Shapes.bezier(.., 0)` answered (0, 0) where JavaScript (and the native one) gives NaN, and a NaN spline index read 0 from a `Float64Array` where JavaScript reads `undefined`; both are edge cases outside any example.

## Merges of main and md-reader-example (2026-10-22)

`1fdd708` merge of `main` (docs only), `16bb297` merge of `md-reader-example` (md-reader, launcher, smooth scrolling,
scrollIntoView alignment, scroll-padding, Application.args / quit(code)), `7754703` native ports and the compiler features the
two examples needed (M9-TS-3361..3382), `b7730d0` the examples as platypus-build games with `project.json` `prebuild`
(M9-TS-3383). 43 examples build. Open questions for the user:
- **`prebuild` runs bun** (`bun gen-docs.ts` / `bun gen-games.ts` write `src/*.gen.ts` and `content/`): the examples compile
  as written, but a build needs bun on the PATH again. Alternatives: port the generators to platypus_ts once M6 file IO exists,
  or have the readers load the docs / screenshots at run time through `Assets` (changes the example sources).
- **Known limits met on the way**: `null` and `undefined` are one value (`console.log(r?.length)` prints `null`;
  `Number(x)` of a missing string is NaN, as for `undefined`); a copy where an object of another shape goes is still a copy
  (Ref<T> became `{ current: T }` so refs are shared); errors in lazily lowered functions can carry the caller's line.

## Resume point (2026-10-27, main merged into the Platypus rename)

**Done:** the rename Iron -> Platypus (compatibility: `game.irbundle`, a `package.json` `iron` section,
`iron.scene`/`iron.prefab`, `@iron/*` imports, PlayerPrefs `iron.quality`, the old "Iron" settings folder; magics
`IRONPAYL`/`IRPK`/`IRTX`/`IRMD` kept; `M9-REN-01..04`, `docs/platypus-build.md`), the two-line copyright + SPDX header,
`NOTICE`, the Platypus icon as the default (`M9-ICO-01..06`). Then main (CPU/GLES renderers, two hosts) was merged in
("Merge main (CPU/GLES renderers, two hosts) into the Platypus rename") and renamed again: crates
`platypus_render_cpu` / `platypus_render_gles`, hosts `platypus-game` (games) and `platypus-app` (`"type": "app"`,
`src/bin/platypus_ui_app.rs`), `PLATYPUS_ANGLE_DIR` / `PLATYPUS_GLES_DUMP`; both hosts carry the icon (`build.rs`
links the `.res` into every bin). The single `platypus` binary is gone; `cargo build -p platypus_app` builds
`platypus-game.exe`, `platypus-app.exe` and `platypus_editor.exe`. 2026-10-28: main's next 19 commits (post-processing,
CSS floats, grid tables, Transform hierarchy, raycast masks and tags, taffy 0.15.1 from crates.io) merged the same way
("Merge main (post-processing, CSS floats, tables, Transform hierarchy) ...") and renamed.

**Run:** one area `cargo test -p platypus_ts_tests --test <lang|stdlib|engine|engine_api|ui> [filter]`; all of them
`cargo test -p platypus_ts_tests`. Keep one command shape: `-p platypus_ts_tests` and `--workspace` unify features differently
(`simd-adler32` / `miniz_oxide` via platypus_build), so switching rebuilds platypus_ts_tests and relinks its 11 binaries (~3-5 s);
a different `CARGO_INCREMENTAL` rebuilds every path crate.

**Left:** P4b, the `ui.ts` hook helpers as IR lowering; `temporal.ts` in Rust (decision 2); both on `ALLOWED_TS` in
`generated_dts.rs`. A handle of a subclass does not convert to its base class's type (`const c: Collider2D =
go.getComponent(BoxCollider2D)!` is "type mismatch"). Ideas not done: rust-lld for the tools workspace (~1 s per test link),
`required-features` on the five bins so test builds stop linking them.

**Verify:** the list above; re-bless with `PLATYPUS_BLESS=1 cargo test -p platypus_build --test generated_dts`. Some files are CRLF
(animator.rs, input.rs, platypus_build/src/lib.rs, this file): use the Edit tool.
