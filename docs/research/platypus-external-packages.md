# External TypeScript packages in games built by platypus_ts

**Decision (2026-10-21): only `src/modules` aliases, skipping of unreferenced functions and separation of engine global names were built; the rest of this note is parked.** A bare import is a file of the game's `src/modules/` folder (a TypeScript package is used by copying its source in), `tsc` gets the matching `paths`, top-level functions nothing refers to are not lowered, and a module's declarations never capture the engine's global names. No `node_modules` resolution, no `project.json` packages section, no allow list, no licence generation or copyleft check, no `check-packages`, no JSR fetch, no dynamic `any`, no native plugins. See `docs/platypus-build.md`, "Modules and packages".

Status: research, 2026-10-21. No code changed. Question from the user: games are TypeScript compiled ahead of time
by `tools/platypus_ts` (oxc -> IR -> Cranelift) and built by `tools/platypus_build` with no bun and no `node_modules`
(only `tsc` for type checking). Today any bare import other than `@platypus/*` and `react` is an error. How could a game
use third-party packages (pathfinding, utilities, validation, state machines, ECS, i18n, markdown, seedable RNGs), and
what would actually compile?

Short answer:

- **Only TypeScript source written in the compiler's subset can work.** platypus_ts needs types inside every function
  body (it lowers `number` to f64, objects to structs, generics by monomorphisation). A package that ships `.js` plus
  `.d.ts` gives types for its exports only, not for its locals and helpers, so **compiled JavaScript packages cannot
  be used, whatever their `.d.ts` say**. CommonJS, UMD, prototype code, `any`-heavy code and Node built-ins are out.
- Honest estimate (not measured; phase 0 below measures it): **well under 5% of popular npm packages would compile
  unmodified**; small, game-style TypeScript libraries (a binary heap, A* on a grid, a seeded PRNG, an easing table,
  a tiny FSM) written against the subset compile at a high rate, and many need edits of a few lines.
- Recommendation: **phase 1 = vendored sources + tsconfig `paths` + `node_modules` resolution of TypeScript source
  packages, with tree-shaking at the module level and errors that name the package file and line** (size M). It also
  makes JSR packages (which are published as TypeScript source) usable, which is the best supply of compatible code.

## 1. What the compiler accepts today, and what that means for packages

### 1.1 How the build works (the parts that matter here)

- `tools/platypus_ts/src/modules.rs` follows relative imports from the entry, erases `@platypus/*` / `react` imports
  (`is_library`), renames colliding top-level names (`helper` -> `helper_2`), drops `export` from non-entry files and
  writes **one source text** (`Bundle::source`). A bare specifier fails in `Graph::load`:
  `"{path}: unsupported import \"{spec}\" (only relative files, @platypus/* and react)"`.
- `resolve_path` already tries `x`, `x.ts`/`x.tsx` for a `.js`/`.jsx` specifier, and `x/index.ts(x)`. Every file is
  parsed as TS or TSX (`parse_module`), so a `.js` file is parsed as TypeScript without annotations.
- That text is lowered as **one program** (`lower/mod.rs`). Step 3 of the lowering lowers **every declared function**
  (the loop over `fstate == 0` around `lower/mod.rs:635`); only generic specialisations are made on demand. So an
  unsupported construct in a package function the game never calls still fails the build. There is no tree-shaking.
- `platypus-build build` (`tools/platypus_build/src/lib.rs`, `build`) bundles, runs `tsc` with a generated
  `dist/.tsc/tsconfig.json` (`tsc_config`: `extends` the game's file, `"types": []`, `exclude: ../../node_modules`,
  `files: ["platypus.d.ts"]`), compiles, and writes `dist/game.pbundle`, which **contains the source text**
  (`docs/platypus-build.md`, "`game.pbundle`"). The shipped exe JIT-compiles it. So package source is shipped as source.
- `Bundle::locate_in` rewrites `line N` in an error to `file:line` of the original file; lowering errors carry a
  line for statements (`tools/platypus_ts/TODO.md`, "line numbers on statement errors"), not for every expression error.

### 1.2 The subset, from the code and the trackers

Supported and relevant to libraries (detail in `tools/platypus_ts/TODO.md` "Now", `TODO.md` M9.b2):
numbers, strings (exact JS semantics), booleans, arrays, `Map`/`Set`, typed arrays, `ArrayBuffer`/`DataView`, classes
with inheritance, `super`, abstract classes, getters/setters, statics, `#private`, `instanceof`, `typeof`, enums,
generics by monomorphisation (functions, classes, interfaces, aliases), closures, destructuring in declarations and
parameters, default/optional/rest parameters, spread in arrays/objects/calls into rest parameters, `try`/`catch`/
`finally`, error classes, generators and iterators (`Symbol.iterator`), `async`/`await`/`Promise`, timers, `RegExp`
(regress), `Date`, `Intl` (ICU4X), `Temporal`, `BigInt`, typed `JSON`, `structuredClone`, top-level statements.

Not supported (each is a compile error today; messages from `lower/*.rs`):

| Construct | Where it fails | Common in npm code? |
|---|---|---|
| `any`, `unknown`, `never`-heavy types, mapped types, conditional types, `keyof T`, template literal types, `typeof x` in types | `lower/types.rs` `ty()`: `"unsupported type annotation"` (`Record<string, unknown>` only as a style bag) | Very (every validation, utility-type and plugin library) |
| `Record<string, V>`, index signatures | `"Record needs a union of string literals as its keys; use Map<string, V>"` | Very (dictionaries, i18n resources) |
| Structural typing: passing an object of one interface where another is expected, unions of interfaces | open STOP item "Structural typing" in `TODO.md` M9.b2; structs are nominal by shape | Very (options objects, AST node unions in parsers) |
| `interface A extends B` | `"interface {name}: extends is not supported"` | Common |
| Class expressions (mixins `const M = (B) => class extends B {}`) | `"a class expression is not supported"` | Mixin libraries, some ECS |
| Decorators other than `@property` | `"class {name}: decorators are not supported yet"` | DI/ORM-style libraries |
| Destructuring assignment `[a, b] = [b, a]` | `"destructuring assignment is not supported"` | Occasional |
| Computed property keys, `obj[key]` with a string key on an object | `key_name`: `"computed property key"` | Very (dynamic objects) |
| Untyped callback parameters in type positions | `"untyped callback parameter"` | Rare in TS, universal in JS |
| `import * as ns` of a project file, `export * as ns` | `modules.rs` | Common as a library entry pattern (`export * as utils from`) |
| `Proxy`, `Reflect`, `eval`, `new Function`, `with`, `arguments`, `delete`, `Object.defineProperty`, prototype assignment, `globalThis` | not built; tier-4 STOP items | Proxy: state libraries (immer, valtio, mobx); prototype: older libs |
| `require`, `module.exports`, UMD wrappers, dynamic `import()` | no CommonJS model at all | Most of npm's older packages |
| Node built-ins (`fs`, `path`, `Buffer`, `process`, `crypto`, `events`), DOM (`window`, `document`, `fetch` before M6) | no such globals; tsc fails first with `"types": []` | Common in tooling, rare in pure-logic libs |
| `JSON.parse` without a target type, `console.log` of objects | `json.rs`, `"console.{method} of an object ... is not supported yet"` | Common in debug paths |

Rule of thumb: a package compiles if it is **TypeScript source, `strict`, with concrete types, classes and plain
functions, no `any`, no dictionaries keyed by `string` on plain objects (uses `Map`), no type-level programming,
no Node/DOM, ESM**. That is the style Platypus's own examples are written in, not the style of most npm libraries.

### 1.3 Where the source has to come from

- **`.ts` in the tarball.** Some packages publish `src/*.ts` next to `dist/` (for source maps), some set a
  `"source"` field (microbundle/parcel convention), a few publish only `.ts`. Most publish compiled `.js` + `.d.ts` only.
- **JSR** (`jsr.io`, the Deno/JSR registry) publishes TypeScript source by design (`@std/*` and many small
  libraries). `npx jsr add @std/data-structures` / `pnpm add jsr:@std/...` installs it into `node_modules/@jsr/...`
  as an npm-compatible package (the npm compatibility layer serves compiled JS plus `.d.ts`; the original `.ts` is at
  `https://jsr.io/@scope/name/<version>/<file>.ts`). Vendoring JSR source is the cleanest route; a `platypus-build`
  fetch of JSR source is a phase 3 idea.
- **Git / copy-paste vendoring** of a small library into the game (`packages/<name>/`). For game-sized utilities
  this is the realistic main path.

### 1.4 Six packages assessed

Judged from each package's published layout and well-known source style (not compiled in this research; phase 0
does that). "Edits" means changes to the package source in a vendored copy.

| Package | Shape | Verdict | Why |
|---|---|---|---|
| `@std/data-structures` `BinaryHeap` / `RedBlackTree` (JSR) | TS source, ESM, generics with a comparator, `#private`, `Symbol.iterator`, generators | **Likely compiles with 0 to few edits** | Classes, generics and generators are all in the subset. Risks: `implements Iterable<T>`, a `static from()` with overloads and `ArrayLike<T>` / `Iterable` unions, `readonly` modifiers (fine). The best first target for a test. |
| `@std/random` seeded PRNG (`randomSeeded`, pcg32 on `BigInt`) or a hand-sized mulberry32 / sfc32 | TS source, numeric, bit ops | **Compiles** (mulberry32 in minutes; the `@std` one depends on `BigInt` 64-bit arithmetic, which exists, and on `Uint8Array` / `DataView`, which exist) | Pure arithmetic is the compiler's best case; check `>>> 0` / `Math.imul` results against bun (the JS-semantics conformance item). |
| `mitt` (event emitter, TS source in its repo; npm ships JS + `.d.ts`) | about 100 lines, `Map`, generics `Emitter<Events extends Record<EventType, unknown>>`, `EventType = string \| symbol`, `keyof Events`, `'*'` overloads | **Fails today; vendored and rewritten in ~30 lines it works** | `Record<string \| symbol, unknown>`, `keyof`, indexed access by a type parameter (`Events[Key]`), overload signatures. Typical of "small but type-level" libraries. |
| `zod` (generics-heavy) | TS source in repo, compiled JS on npm; deep conditional/mapped types, `ZodType<Output, Def, Input>` hierarchy, `any` in internals, dynamic shape objects | **Will not compile, and should not be the goal** | Type-level programming is the whole API (`z.infer<typeof schema>`); runtime walks objects by string keys. A validation story for Platypus is typed `JSON.parse` (already built) plus hand checks, or a future `JsonValue` (`TODO.md` "JsValue fallback"). |
| `pathfinding` (PathFinding.js) | CommonJS JS, `function Grid() {}` + `Grid.prototype.x = ...`, `require('heap')` | **Impossible as is**; port to TS classes (~400 lines for A* + grid + heap) | No CommonJS, no types, prototype assignment. Pathfinding is a good candidate for a hand-written Platypus-style TS package (or a Rust `#[ts_api]` module, which would also be faster). |
| `dotenv` / anything using `fs`, `path`, `process`, `Buffer` | CommonJS, Node built-ins | **Impossible by design** | No Node runtime. File access is M6 Script IO (`docs/research/networking-and-files.md`). tsc already rejects it (`"types": []` means no `@types/node`). |
| `marked` (markdown; TS source since v5, ESM JS + `.d.ts` on npm) | Classes (`Lexer`, `Parser`, `Renderer`), regex tables built at run time (`edit(regex).replace(...).getRegex()` on `.source`), a discriminated union of token interfaces, `any` in extensions/options | **Fails; a large port** | Token unions need structural typing / unions of interfaces; options objects with `any`; dynamic regex composition is fine (RegExp is built). Realistic only after structural typing lands. |
| (classes/mixins) `miniplex` or `bitecs` (ECS) | Mapped types (`With<E, P>`), dynamic component stores keyed by string or `Symbol`, `Proxy` in some versions | **Fails** | Platypus already owns the world in Rust (`platypus_scene`, components); an ECS library fights the core rule. Not worth targeting. |

Takeaway: target **algorithms and data structures written as typed classes** (heaps, trees, grids, A*, noise,
PRNGs, easing, tweening, FSMs, i18n message tables keyed by literal unions). Do not target the type-level
ecosystem (zod, ts-pattern, type-fest), proxy-based state (immer, valtio, mobx), or anything CommonJS.

## 2. Declaring and finding packages

### 2.1 Three sources, one resolver

1. **Vendored folder in the game** (recommended default): `packages/<name>/` with its own `package.json` (or none:
   then `index.ts`). Committed with the game, watched by `platypus-build dev` (the watcher skips `node_modules` but not
   `packages`), editable when a construct must be rewritten. Imported by bare name through a mapping.
2. **tsconfig `compilerOptions.paths` (+ `baseUrl`)**: what editors and `tsc` already understand. `"paths":
   { "astar": ["./packages/astar/src/index.ts"], "@std/*": ["./packages/std/*"] }`. platypus_ts must implement the same
   matching: exact keys, a single `*` wildcard, longest prefix wins, candidates tried in order, relative to `baseUrl`
   or to the tsconfig's folder (TS 4.1+). This makes the compiler and `tsc` agree by construction.
3. **`node_modules`** installed by the developer with any tool (`npm`, `pnpm`, `bun install`). `platypus-build` only
   reads files; the shipped build never needs `node_modules` because the source is in `game.pbundle`.

`project.json` gets one new optional section, for the cases tsconfig cannot express:

```json
{
  "packages": {
    "allow": ["astar", "@std/data-structures"],
    "source": { "some-lib": "src/index.ts" }
  }
}
```

- `allow`: bare imports outside this list are an error (supply-chain guard, section 4; default when absent: every
  package that resolves is allowed, with a note listing them).
- `source`: per-package entry override for a package whose `package.json` does not point at its `.ts`.
- No `dependencies` list of its own: versions stay in `package.json` + the lockfile of whatever installer the
  developer used (npm, pnpm and bun all write one). Platypus does not invent a second package manager. For vendored
  folders, the version is the folder's `package.json` `version` (or the commit it was copied from, noted in a
  `packages/<name>/SOURCE` line).

### 2.2 Resolution rules to implement in `modules.rs`

New function `resolve_bare(from, spec) -> Result<String, String>`, called from `Graph::load` where the
`unsupported import` error is today (and from `target`, which must use the same result: store the resolved path per
`(module, spec)` instead of recomputing it).

Order:

1. `is_library(spec)` (`@platypus/*`, `react`, `react/*`) stays first and stays erased.
2. tsconfig `paths` (read once in `platypus_build::GameConfig::load`, passed in as data; `modules.rs` stays I/O-free
   through `ReadFn`).
3. `project.json` `packages.source` override.
4. `node_modules` lookup: from the importing file's folder upward, `node_modules/<name>` (`<name>` is `@scope/pkg`
   or `pkg`; the rest is the subpath). Stop at the game folder's parent chain end. Canonicalise the found path
   (pnpm uses symlinks into `node_modules/.pnpm/...`) so one package reached twice is one module.
5. Inside the package, with its `package.json`:
   - `exports` (string, subpath map `"./x"`, patterns `"./*"`, condition objects). Conditions tried in order:
     **`"platypus"`** (a condition Platypus defines, for packages that want to ship a Platypus entry), **`"source"`**,
     **`"types"`** (only to find a sibling `.ts`, see below), `"import"`, `"default"`. `"require"` / `"node"` /
     `"browser"` are never picked. A subpath not in `exports` is an error, as in Node.
   - Without `exports`: `"source"`, then `"module"`, then `"main"`, then `index`.
   - **From a `.js` or `.d.ts` target to `.ts`**: try the same path with `.ts`/`.tsx`, then the mirror under `src/`
     for `dist/`/`lib/`/`esm/`/`build/` (`dist/foo.js` -> `src/foo.ts`). If only `.js` + `.d.ts` exist, fail with the
     "compiled JavaScript" error of section 3.1 instead of parsing JS as TS.
6. Relative imports inside a package resolve as today (`resolve_path`), plus `.mts`/`.cts` -> no (`.cts` is
   CommonJS by definition; `.mts` may be added).

Also in `modules.rs`:

- **Collisions with engine globals.** `emit` only reserves the entry's names and imported library names. A package
  that declares `class Vector2`, `Random`, `Color`, `Rect`, `Debug`, `Log`, `Time`, `Timer`, `Scene`, `Input` (all
  common in libraries) would shadow the engine in the one program (the docs note this for games). Reserve every
  manifest/prelude global name (the list `types_dts` writes) and rename non-entry declarations that collide. Size S.
- `import * as ns` / `export * as ns` of a project or package file: libraries use both as entry patterns. Lowering
  a namespace object needs a struct of functions; a cheaper version rewrites `ns.f(...)` to the resolved `f` when
  `ns` is only used as `ns.<name>` (error otherwise). Size M.
- **Package boundary in errors**: keep `Bundle::files` with the original paths (`node_modules/astar/src/grid.ts`),
  which `locate_in` already prints.

### 2.3 How `tsc` sees the types

- The generated `dist/.tsc/tsconfig.json` (`tsc_config`) extends the game's config, so the game's `paths` already
  apply. `exclude: node_modules` only affects `include` globs; imported files are still checked, and `"types": []`
  only stops automatic `@types/*` (good: no `@types/node` sneaking in).
- Two changes: set `"moduleResolution": "bundler"` and `"customConditions": ["platypus", "source"]` in the generated
  config so `tsc` picks the same `exports` entries as platypus_ts; and **check package sources as source**: when a
  package resolves to `.ts`, tsc type-checks it with the game's strictness. A package that is not `strict`-clean
  then fails `tsc`; offer `packages.skipCheck: ["name"]` (sets nothing in tsc itself, but the build reports tsc errors
  inside that package as warnings).
- `moduleDetection: force` stays. The generated `types/platypus.d.ts` globals are visible inside package files too,
  which is right (a package may use `Vector3` if written for Platypus) but also means a package's own `Vector2` shadows
  the global inside that file (same rule as games).
- `isolatedModules`-style concerns: none (platypus_ts reads types itself).

### 2.4 Licences

Packages end up as source inside `game.pbundle`, inside the packaged exe (`platypus-build package`), so the shipped
game **distributes third-party source**. Most permissive licences (MIT, ISC, BSD, Apache-2.0, Zlib, 0BSD) require
keeping the copyright notice and licence text.

- Build step: for every package folder in the bundle (the nearest `package.json` above each file outside the game's
  `src/`), read `name`, `version`, `license` (SPDX expression), `author`/`repository`, and the licence file
  (`LICENSE*`, `LICENCE*`, `COPYING*`, `NOTICE*` for Apache-2.0). Write `dist/THIRD-PARTY-NOTICES.txt`; `package`
  copies it next to the exe (same place as the game's own licence today, `tools/platypus_build/src/package.rs`
  `LICENSE_NAMES`) and the pbundle gets a `"packages": [{name, version, license}]` list so the app can show
  credits.
- Policy checks (warnings by default, errors with `packages.licenses: "strict"`): no licence or `UNLICENSED` ->
  error; GPL/AGPL/LGPL/MPL in a game whose `license` is `private` -> error naming the package (copyleft obligations
  for a closed game; the user decides, section 5 Q4); unknown SPDX ids -> warning. Reuse `platypus_core::project`'s
  SPDX list.
- Platypus's own runtime crates (regress, jiff, ICU4X data, etc.) need the same notices file; this step is a natural
  place to append them once (`cargo about`-style list generated at release time, open question Q5).

## 3. Failure modes and UX

### 3.1 Errors (each names the package, the file and the line)

| Situation | Message (proposed) |
|---|---|
| Not installed | `src/ai.ts:3: cannot find package "astar" (looked in tsconfig paths, project.json packages, node_modules from src/ upward)` |
| Only compiled JS | `src/ai.ts:3: package "pathfinding@0.4.18" ships only JavaScript (main: lib/PathFinding.js); platypus_ts needs TypeScript source. Vendor its source into packages/, point project.json packages.source at a .ts entry, or use a TypeScript package` |
| CommonJS / UMD | detected by `require(`/`module.exports`/`exports.` at top level: `... is CommonJS (module.exports at lib/x.js:12); not supported` |
| Node / DOM global | `node_modules/foo/src/io.ts:8: "fs" is a Node built-in; games have no Node runtime (file access: Script IO, M6)` (a fixed list of Node built-in names incl. `node:*`) |
| Unsupported construct inside a package | the lowering error with `file:line` plus a prefix: `in package "mitt@3.0.1": node_modules/mitt/src/index.ts:14: unsupported type annotation (keyof Events)` |
| Not allowed | `src/ui.ts:2: package "left-pad" is not in project.json packages.allow` |

Two compiler changes make the last one good: (a) every `bail!` in `lower/types.rs` and `expr.rs` should carry a
span (today many type errors have no line: `"unsupported type annotation"`); a span-carrying error type is size M
and helps games too; (b) name the construct (`keyof`, `mapped type`, `conditional type`, `any`) instead of the
generic message (size S, a `match` on `TSType` variants).

### 3.2 `platypus-build check-packages`

`platypus-build check-packages <game> [--all-exports] [--json]`: resolves every package the game imports (or every
package in `node_modules` with `--installed`), and for each one:

1. Resolution: which file was picked and why (`exports` condition, `source` field, `dist -> src` mirror), or the
   reason it is unusable (JS only, CommonJS, Node built-ins).
2. Compile check of **all exports** (not just what the game uses): bundle a synthetic entry
   `import { a, b, c } from "pkg"; export { a, b, c };` and compile with tree-shaking off, collecting **all**
   errors instead of the first (the lowering stops at the first `bail!`; collecting per function is size M:
   lower each function, record the error, continue).
3. Output: a table `package | version | entry | status (ok / partial / no) | first errors by construct`, and a
   summary of constructs (`keyof: 4, any: 11, Record<string,V>: 2`), which doubles as a prioritised backlog for the
   compiler. Licence column from section 2.4.

The same command run on a corpus (phase 0: ~30 real packages vendored into `tools/platypus_ts_tests/packages/`) turns
the estimates of section 1.4 into measurements.

### 3.3 Escape hatches for dynamic JavaScript

- **QuickJS legacy path.** Games built the old way (bun bundles `node_modules` into `dist/game.js`, run by
  `crates/platypus_script`) can use most npm packages that do not need Node built-ins, today. That path is slated to
  become a "mods and legacy" path (`docs/research/iron-native-without-quickjs.md` on branch `iron-js`). A hybrid
  (compiled game + a QuickJS island running one JS package, values crossing as JSON or `JsValue`) is possible but
  needs the "calls between native and QuickJS, GC handles across the two heaps" item of `TODO.md` M9.b2 `JsValue`
  fallback (STOP). Not recommended for phase 1.
- **The dynamic tier** (`docs/research/iron-js-dynamic-tier.md` on branch `iron-js`, `tools/iron_js` spike):
  NaN-boxed `JsValue` lowered to the same IR, 1.2x to 12.8x QuickJS on its subset; verdict there was no-go for a
  complete tier now. A package-level use would be "compile this package's JS through the dynamic tier and call it
  through `JsValue`-typed declarations". It needs everything in that note's section 5 (prototypes, getters, `new`,
  every builtin with dynamic arguments): months. Park it; keep packages typed.
- **Cheaper middle ground**: `JsValue` only for `any`/`unknown` *values* inside typed code (that note's section 6,
  point 4). It would let `unknown`-typed internals (event payloads, JSON blobs, `Record<string, unknown>` options)
  compile, which is the most common reason small TS libraries fail. Worth a decision (Q2).
- **Port to Rust**: for heavy algorithmic packages (pathfinding, noise, physics helpers) an `#[ts_api]` module
  in Rust is faster and available to both backends (`docs/platypus-api.md`).

### 3.4 Tree-shaking and compile cost

- Needed for correctness, not just size: since every non-generic function is lowered, `import { BinaryHeap }` from
  a package with 40 exports compiles all 40, and one unsupported helper fails the build.
- Implement in `modules.rs` before emit: a reachability pass over top-level declarations (an identifier-use graph:
  `Occs` already records every identifier occurrence per module) starting from the entry's top-level statements and
  exports, following identifier uses to declarations across the renamed names; drop unreachable top-level
  functions, classes, consts with side-effect-free initialisers (literals, arrow/function expressions, `new` of a
  local class, calls to `Object.freeze`), types and interfaces. Keep top-level statements with side effects
  (respect `package.json` `"sideEffects": false` to drop them too). Class members are not shaken (virtual calls make
  it hard; the class itself is). Size M.
- Cost: lowering + Cranelift measured 21 to 25 ms for 330 to 460 lines including the prelude
  (`docs/research/platypus-examples-on-platypus-ts.md` section 0); a 3,000-line package fully lowered would add roughly
  0.15 to 0.3 s per build by extrapolation (unmeasured). Tree-shaking keeps it to what is used. Monomorphisation can
  multiply generic code: the "code-size budget and instantiation depth limit" item of `TODO.md` M9.b2 Generics
  matters more with libraries.
- Shipped size: the pbundle holds source text; tree-shaken source only, so unused package files are not shipped
  (also fewer licence obligations to list: list only packages with reachable code).

### 3.5 Hot reload

- `platypus-build dev` watches the game folder except `node_modules` (`docs/platypus-build.md`, "The dev loop"). Vendored
  `packages/` reload like game code. An `npm install` changes `node_modules` without a rebuild: also watch the
  lockfile (`package-lock.json`, `pnpm-lock.yaml`, `bun.lock`) and `tsconfig.json` / `project.json`, and rebuild
  on change.
- Reload recompiles the whole program today; package code is recompiled each time (cheap after tree-shaking). A
  later cache of lowered package modules keyed by file hash is possible but not needed yet.
- State-keeping reload serialises scenes; package objects held in component fields are only kept if they are
  `@property` data. Package singletons (module-level state, e.g. an i18n table) are rebuilt by the new code's
  `init`: document that packages must not rely on module state surviving a reload.

## 4. Security and supply chain

- **Install scripts**: `npm install` runs `preinstall`/`install`/`postinstall` of every dependency on the
  developer's machine. Platypus never runs them, but the developer's installer does. Recommend `npm install
  --ignore-scripts` / `pnpm` (which since v10 does not run dependency build scripts unless allowed) / bun (runs
  scripts only for `trustedDependencies`) in the docs, and have `check-packages` warn about packages that declare
  install scripts (read from `package.json` `scripts`).
- **Compiled code is not sandboxed**: package code becomes native code in the game's process with the full engine
  API (and, after M6, files and network). It can do anything the game can. The protections are review and
  provenance, not isolation: vendor what you use, pin versions with a lockfile, keep `packages.allow` short, and
  read the diff on update (vendored source makes that a normal code review).
- **Smaller attack surface than npm-in-general**: no Node runtime, no `eval`, no dynamic `import()`, no
  `require`, no `Function` constructor, no `process.env`: a whole class of loaders and obfuscated payloads cannot
  compile. `check-packages` can also print the engine APIs a package calls (from the lowering's call targets), a cheap
  "what does this touch" report.
- **Integrity**: record in the pbundle the SHA-256 of each package file that was bundled; `check-packages --verify`
  compares against the lockfile's `integrity` field when the installer kept the tarball hash (npm and pnpm do;
  vendored folders have no upstream hash, the `SOURCE` line names where they came from).
- Typosquatting and dependency confusion: `allow` list plus the "resolved from" line in build output
  (`astar -> node_modules/astar@1.2.0/src/index.ts`) makes surprises visible.
- Mods are a separate question (untrusted code at run time); this note is about code the developer chose at build
  time.

## 5. Plan

Sizes: S = up to ~300 lines of Rust with tests, M = ~300 to 1,000, L = more. Test IDs: a new prefix `M9-PKG-*` in
`TESTS.md` (avoids the crowded `M9-TS-*` ranges).

### Phase 0: measure (S, research)

- Vendor ~30 small TypeScript packages (JSR `@std/*` data structures, random, collections, a few npm TS-source
  packages, mitt, a TS A*, a TS FSM, an i18n table library) into `tools/platypus_ts_tests/packages/` (needs the user's
  OK for downloads), run a throwaway "compile all exports" script, and record per construct what fails. Replaces
  the estimates in section 1.4 with numbers.

### Phase 1: vendored and source packages (M, recommended first)

1. `modules.rs`: `resolve_bare` with tsconfig `paths`/`baseUrl`, `project.json` `packages.source`, `node_modules`
   lookup upward, `package.json` `exports` (conditions `platypus`, `source`, `import`, `default`; subpath patterns),
   `source`/`module`/`main`, `.js`/`.d.ts` -> `.ts` and `dist/` -> `src/` mirrors; resolved paths stored per
   `(module, spec)`; realpath dedupe. (M)
   - Tests: `M9-PKG-01` paths exact and wildcard, longest prefix; `02` node_modules upward from a nested file;
     `03` scoped package; `04` exports string / subpath / pattern / conditions order; `05` `require`-only exports is
     an error; `06` `main: dist/x.js` with `src/x.ts` resolves to the `.ts`; `07` JS-only package error text;
     `08` the same package reached through a symlinked path is one module; `09` two versions in nested
     node_modules both bundle with renamed names.
2. Engine-global collision renaming in `emit`. (S) Test `M9-PKG-10`: a package `class Random` does not shadow the
   engine `Random` in the game file.
3. Errors: CommonJS detection, Node built-in names, `in package "name@version":` prefix, `packages.allow`. (S)
   Tests `M9-PKG-11..14`.
4. Module-level tree-shaking (reachability over top-level declarations, `sideEffects`). (M) Tests `M9-PKG-15`:
   an unused function with an unsupported construct does not fail the build; `16`: a top-level statement with side
   effects is kept; `17`: `sideEffects: false` drops it; `18`: a class reached only through a type annotation is
   kept as a type, its methods are not required.
5. `platypus_build`: pass tsconfig `paths` into the bundler; `customConditions` + `moduleResolution: bundler` in
   `tsc_config`; watch lockfiles; print the resolved packages in the build report. (S) Test `M9-PKG-19` (generated
   tsconfig text), `M9-TS` style integration test with an example game using a vendored `packages/heap`.
6. Licences: `THIRD-PARTY-NOTICES.txt`, the pbundle `packages` list, copyleft-vs-private check. (S) Tests
   `M9-PKG-20..22`.
7. Docs: `docs/platypus-build.md` "Packages" section, a `docs/differences.md` note ("packages must be TypeScript
   source"), an example game using one vendored package (e.g. A* in `examples/platformer` or a new tiny one).

Phase 1 total: M to L (about 1,200 to 1,600 lines with tests).

### Phase 2: make more packages compile (L, compiler work that games need too)

In order of how often each blocks libraries (to be confirmed by phase 0): span-carrying lowering errors and
construct names (M); `interface extends` (S to M); `Record<string, V>` and index signatures as `Map`-backed objects
(M, part of the Structural typing STOP item); structural typing (L, STOP: design note first); `import * as ns`
(M); `keyof` / indexed access by a type parameter under monomorphisation (M); overload signatures (S: lower the
implementation, check calls against the overloads in tsc only); class expressions for mixins (M); `unknown` as a
`JsValue` value type (M to L, after the decision Q2).

### Phase 3: tooling (M)

`platypus-build check-packages` (all-exports compile, error collection per function, construct summary, licence and
install-script columns, `--json`) (M); `platypus-build vendor <pkg>` that copies a package's TypeScript source from
`node_modules` or JSR into `packages/<name>/` with its licence and a `SOURCE` line (S to M; network fetch needs the
user's OK per the project rules).

### Phase 4 (optional, gated on decisions)

- QuickJS islands for JS-only packages in compiled games (L, needs the `JsValue` fallback STOP item).
- Dynamic tier for packages (L+, `iron-js` branch note section 5).
- A cache of lowered package modules for faster reloads (M).

### Open questions for the user

1. **Vendored-first or node_modules-first?** Recommendation: support both in phase 1, document vendoring
   (`packages/`) as the default because it is reviewable, hot-reloads, and survives without an installer.
2. **`unknown`/`any` as a dynamic `JsValue` in typed code**: build it (unblocks many small libraries and untyped
   JSON), or keep the rule "what is not typed is a compile error"?
3. **`packages.allow`**: required (opt-in list, safer) or optional (every resolvable package allowed, with a note)?
4. **Copyleft in closed games**: error, warning, or allowed? (GPL/LGPL/MPL source compiled into a `private` game.)
5. **Platypus's own third-party notices** (Rust crates, ICU data) in the same `THIRD-PARTY-NOTICES.txt`, generated at
   release time?
6. **JSR support**: worth a `platypus-build vendor jsr:@std/...` fetch command, or document `npx jsr` + vendoring?
7. **Package-provided engine extensions**: should a package be allowed to ship `#[ts_api]` Rust code (native
   plugins) later, or are packages TypeScript only? (Affects the `platypus` export condition's meaning.)
8. **Should tree-shaking also apply to game code** (unused game functions with unsupported constructs stop failing),
   or only to package files? Recommendation: everything, it is the same pass.
