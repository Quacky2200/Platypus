# Platypus TS: dynamic `any` / `unknown` values (design)

Status: design, 2026-10-22, not built. Decision (user): platypus_ts gets a real dynamic value for `any` and `unknown`, as close to TypeScript as possible. Statically typed code keeps its fast path; only values typed `any`/`unknown` pay. This builds on the measured `iron-js` spike (`git show iron-js:docs/research/iron-js-dynamic-tier.md`, `iron-js:tools/iron_js/src/rt.rs`), which proved the value encoding and its GC rule; here the same encoding is used inside the typed tier.

## 0. What exists today

- **Types.** `ir::Ty` has `Num, Bool, Lit, Str, BigInt, Null, Obj(sid), Arr, Typed, Fn, Cell, Nullable, Handle, Map, Set, JsValue`. `JsValue` is "reserved" but already used as an opaque pointer for boundary types (`ExtTy::Ptr`, `ValueArr`, `ValueList`, `RecordsJson` in `ExtTy::to_ir`, `ir.rs:477-493`).
- **`any`/`unknown` today:** no arm in `Lowerer::ty` (`lower/types.rs:70`); falls to "unsupported type annotation" (`types.rs:458`). `object` becomes `PlatypusStyle` when the UI prelude is present (`types.rs:80`), and `Record<string, unknown>` too (`types.rs:397`; `examples/arcade/src/main.tsx:475` relies on it). `x as any` is effectively a no-op (`expr.rs:137-210`, every failing `self.ty(..)` falls through to `Ok(v)`).
- **Unions** (`lower/tagged.rs`): a heap struct `{$tag, $0, $1, ..}` (`make_union`, `wrap_in_union`, `project_union`); narrowing is path-based (`cur.narrowed`, `narrowings()`, `narrow_value()`), trusting `tsc --strict`.
- **Objects:** every GC object starts with a header (`mark | heap id | kind`, `platypus_ts_rt/src/lib.rs:44-80`): structs `struct_base + sid + 1`, plus `HDR_ARRAY`, `HDR_ARRAY_PTR`, `HDR_STR`, `HDR_CLOSURE(_ENV)`, `HDR_CELL(_PTR)`, `HDR_MAP`, `HDR_BIGINT`, `HDR_BYTES`, `HDR_VIEW`, `HDR_CORO`. Every pointer is self-describing except: the element kind of a non-pointer array, a typed array's `TypedKind`, a Map's value kind (`MapObj.val_ptr` is only a flag), and a closure's signature. Struct pointer slots come from a 64-bit mask (`ts_rt_register_struct`), so a struct has at most 64 fields.
- **Interfaces/classes** (`lower/ifaces.rs`): dynamic interface access is `CallVirtual` through `Module::vtables` (one address per struct id); `instanceof` is `InstanceOf(x, table)` over `Module::itables`; tables are filled by a fixpoint (`finish_ifaces`). This is the mechanism to reuse.
- **JSON** (`lower/json.rs`, `platypus_ts_rt/src/json.rs`): typed by a descriptor blob; `JSON.parse` requires a context type (`json.rs:377`) and validates it. `Map`/`Set`/`Record<string, V>` stringify as `{}` (desc 7), which is wrong for records today.
- **`Record<string, V>`** is `Ty::Map(Str, V)`; `dicts.rs`/`object.rs` handle `in`, `delete`, `for..in`, `Object.*`, `Object.assign`.
- **Exceptions:** `catch (e)` is always `Ty::Obj(Error)` (annotation ignored, `stmt.rs:589`); `throw` accepts only Error subclasses (`stmt.rs:289`); `Heap.exc` is one root slot.
- **No runtime dynamic lookup** and no `x is T` predicates. platypus-build runs `tsc` with `"strict": true` (`platypus_build/src/lib.rs:782`), so `noImplicitAny` and `useUnknownInCatchVariables` are on.

## 1. Representation and type rules

### 1.1 The value word

`Ty::Any` is one 64-bit word, pointer-favouring NaN-boxing (as JavaScriptCore), with **undefined = 0**.

| Kind | Encoding | Test |
|---|---|---|
| GC object (string, struct, array, map, closure, bigint, ...) | raw pointer (top 16 bits 0, >= 0x10000) | `w >= 0x10000 && w >> 48 == 0` |
| number | f64 bits + 2^49, NaN canonicalised before boxing | `w >> 48 != 0` |
| `undefined` | `0` | `w == 0` |
| `null` | `0x02` | `w == 2` |
| `false` / `true` | `0x06` / `0x07` | `w \| 1 == 7` |
| internal "absent" (lookups only) | `0x0E` | |

Undefined = 0 means zeroed memory is `undefined` (new `any` fields, absent optional `a?: any`, `new Array<any>(n)` slots) and `== null` / `??` / `?.` is `(w | 2) == 2`. Raw pointers mean every runtime routine takes a dynamic string/array unchanged, a narrowed `any` *is* the typed pointer, and boxing an object is free. Handles and values with no pointer get small boxes (`HDR_HANDLE {header, class index, id}`); string-literal-union values become their string (`TagStr`). An `any` fits every existing 8-byte slot.

### 1.2 GC rooting

- `mark_ptr` (`lib.rs:389`) skips words that are not object words (`is_object_word`, from the spike). Typed slots hold pointers or 0, so they behave as before.
- Add `Ty::is_traced()` (`is_ptr() || Any`) wherever `is_ptr` decides tracing: `frames.rs::plan`, the `codegen.rs:624` struct mask, `codegen.rs:642` global roots, array `ptr_elems`, `HDR_CELL_PTR` cells, map `val_ptr`/key kind, `spill_flags`. Do not make `Any` `is_ptr()` (null is 2 for `any`, 0 for pointers).
- `Heap.exc` becomes a dyn word (already a root). UI state slots (`ExtTy::Ptr`) can hold dyn words.

### 1.3 Self-description gaps closed in the runtime (zero cost for typed code)

- **Arrays:** `ts_rt_arr_new_gc(cap, ptr_elems)` becomes `(cap, elem_kind)`: 0 f64, 1 pointer, 2 bool, 3 literal, 4 nullable f64, 5 any (`HDR_ARRAY_DYN`, traced, 0 = undefined), 6 handle. Untraced kinds trace like `HDR_ARRAY`; every `== HDR_ARRAY` match (8 sites) goes through `is_array_kind`.
- **Typed arrays:** encode `TypedKind::code()` in the header kind (or a byte in `HDR_VIEW`).
- **Maps:** `val_ptr` becomes a value kind (same codes); flags `REC` (plain object) vs `new Map()`, and `SET`; key kind `K_DYN` (SameValueZero over dyn words).
- **Closures:** `Closure` gains a fourth word, the dyn trampoline address of its signature (0 if the module never boxes a function). 24 bytes already rounds to 32, so no memory cost, and closures keep identity.

### 1.4 Type-system rules (`lower/types.rs`)

- **Becomes `Ty::Any`:** `any`, `unknown`, `{}`, `Object`, `Function`; `object` except when a style is expected (Q3); `T | any`, `T | unknown`; `Nullable(Any)` normalises to `Any`. `unknown` and `any` are the same type (tsc has already enforced `unknown`'s restrictions). `unify(Any, T)` is `Any`.
- **`coerce()` gets two arms before the union arms:**
  - **Box (T -> Any)**, `ExprKind::Box`: `Num` canonicalise NaN then add 2^49; `Nullable(Num)` null or number; `Bool` `6 + b`; `Lit` `TagStr`; pointers as they are (nullable 0 -> null; absent optional field read by the dyn getter -> undefined); union struct: switch on `$tag` and box the member (an `any` never holds a union struct); `Handle` `HDR_HANDLE`; `Void` undefined; `Null` null.
  - **Unbox (Any -> T)**, `ExprKind::Unbox(e, Checked|Trusted)`: `Num` check + subtract; `Bool`; `Str` header check; class `Obj(C)` struct + `itables[C][sid]`; interface/plain-object struct: same struct id or one the interface accepts -> the pointer, a `REC` map -> a **copy** built by name (typed JSON descriptor walk, `ts_rt_dyn_to_typed_gc`); `T[]`: matching elem kind -> pointer, dyn array -> converting copy; `Lit` string + `StrTag` membership; unions dispatch on dyn kind then `wrap_in_union`; `Fn(S)`: trampoline == S's -> the closure, else an adapter closure; nullable targets accept 0 and 2. Mismatch throws `TypeError`. **Trusted** is for narrowed reads (`typeof`, `instanceof`, `===` literal); **Checked** everywhere else (assignments, arguments, `as T`, user type guards).
- **`as`:** `x as any` boxes (fix the silent no-op); `anyV as T` is a checked unbox.
- **Narrowing:** add `cur.dyn_narrowed: Vec<(String, Ty)>` next to `cur.narrowed`; extend `tagged.rs::narrowings()` for `typeof p === "number"|"string"|"boolean"|"bigint"`, `p instanceof C`, `p === null|undefined|literal`, with `!`/`&&`/`||` as today; user guards `x is T` are new (the predicate lowers as `Bool`; a call narrows the argument path with a checked unbox). `Array.isArray`, `"k" in x`, `typeof x === "object"|"function"` narrow nothing representation-wise (an `any[]` view cannot alias a `number[]`, Q8).
- **Generics** are monomorphised, so `T = any` is one more specialisation; rename the mangle `"js"` to `"any"`.
- **Index signatures:** `{[k: string]: any}`, `Record<string, any|unknown>` are `Ty::Map(Str, Any)` with `REC`.
- **Implicit any** stays a compile error (tsc strict), with a better message: "needs a type annotation; write `: any` for a dynamic value".

## 2. Dynamic operations

All in a new `lower/dynamic.rs`, via the `RtFn::Str("ts_rt_dyn_*")` runtime-function node (`_gc` names may collect; `frames.rs` roots the operands).

### 2.1 Plain dynamic objects
One representation: `MapObj` with `REC`, `K_STR` keys, `any` values (ordered, tombstones, live iteration; `dicts.rs` already lowers get/set/`in`/`delete`/`for..in`). Produced by untyped `JSON.parse`, object literals expected as `Any`, `{}` as any, spread into any, `Object.assign({}, ..)`. JS key order (array-index keys ascending, then insertion) is applied when listing keys (`ts_rt_map_keys_js_gc`). Hidden classes and inline caches stay a measured phase-7 option (Q6).

### 2.2 Structs seen through `any`
Per-struct generated dispatch functions in a new `lower/dyn_tables.rs`, registered in `Module::vtables`: `S$dynget(this, key)` (fields boxed by type, getters, methods as bound closures, expandos, else `ABSENT`), `S$dynset(this, key, v)` (checked unbox into a field, setters, expandos), `S$dyncall(this, key, argv, argc)`, `S$dynkeys(this)` (declared non-private fields, present optionals, expandos). A struct is dyn-visible when `coerce` boxes it (and its subclasses); generating the bodies boxes more, resolved by a fixpoint (`finish_dyn`, like `finish_ifaces`). **A program that never boxes pays nothing.** Expandos live in a weak runtime side table `HashMap<obj ptr, REC map>`, cleared in `collect()`.

### 2.3 Lowering per operation
- **Get** `x.k`/`x[k]` -> `ts_rt_dyn_get_gc`: runtime switch on kind (immediates: `TypeError` for undefined/null with JSC text, else undefined; string `length`/index; array `length`/index; `REC` lookup; Map/Set `size`; struct via `vtables[dynget][sid]`; closure `length`/`name`). Numeric key on array/string has an inline fast path.
- **Set:** new `Place::Dyn(obj, key)` in `expr.rs::place_of` -> `ts_rt_dyn_set_gc`; arrays in-bounds or append with elem-kind conversion (wrong type is a `TypeError`); compound assignment and `++` read, apply dyn arithmetic, write.
- **`delete`, `in`, `for..in`, `Object.keys/values/entries/assign/hasOwn`** on `any`: runtime functions (`dicts.rs::lower_in`, `object.rs`, `for..in` in `stmt.rs` get `Any` arms).
- **`for..of`** over `any`: arrays, strings (code points), Map (`[k, v]`), Set, structs with `next` (dyncall); else `TypeError: x is not iterable` (`lower/iter.rs::begin_iter` dyn iterator).
- **Spread/destructuring:** `[...a]` -> `any[]`, `{...a}` -> `REC` copy, `f(...a)` -> argv; `const {a, b = 1, ...r} = x` gets with default when value is `undefined` (0 only), `r` is a `REC` copy; `const [p, q] = x` index gets.
- **Optional chaining:** `chain_guard` (`expr.rs:796`) tests `(w|2)==2`.
- **Calls:** `ExprKind::DynCall { f, this, args }`, args in rooted frame spill slots (no argv allocation); `ts_rt_dyn_call_gc(f, this, argv, argc)`; per-signature trampolines `dyncall$S`; `x.m(..)` -> `ts_rt_dyn_call_method_gc` (struct -> `dyncall`, `REC` -> get then call, strings/arrays/Map/Set/numbers -> a builtin method table in a new `dynmethods.rs` over the existing `ts_rt_str_*`/array/map routines; callbacks are dyn closures). `new x()` on `any` is a compile error.
- **Arithmetic:** `- * / % **`, bitwise, unary `-`/`+` with an `Any` side: `ToNumber` both then typed `Bin`, result `Num`; inline number fast path, else `ts_rt_dyn_to_num` (`ToPrimitive` via `valueOf`/`toString` dyncall, JS number parsing, true 1, null 0, undefined NaN). `+`: inline both-numbers, else `ts_rt_dyn_add_gc` (result `Any`, or `Str` when the other side is statically a string).
- **Comparison:** `<` etc. -> `ts_rt_dyn_lt`; `===` inline word equality with fix-ups (`ts_rt_dyn_strict_eq`: NaN, -0, strings by content, handles by id); `==` with an `any` operand is real loose equality (`ts_rt_dyn_loose_eq`); typed `==` stays `===` as today.
- **Truthiness** on `Any`: `w == 7` true; `w < 0x10` false; number `d != 0 && d == d`; else `ts_rt_dyn_truthy`.
- **`typeof`:** inline number/immediate tests, else header switch; null -> `"object"`, undefined -> `"undefined"` (`strings.rs::lower_typeof` gets an `Any` arm).
- **Strings:** template strings, `String(x)`, `x + ""` -> `ts_rt_dyn_to_str_gc` with JS `ToString` (arrays join with `,`, objects `[object Object]` or their own `toString`, functions fixed text).
- **JSON:** `JSON.parse(s)` untyped or `as any/unknown` -> `ts_rt_json_parse_dyn_gc` (same JSC-compatible parser, dyn builder); typed contexts keep today's validating parse (Q10). `JSON.stringify(any, replacer?, space)` -> `ts_rt_json_stringify_dyn_gc` (`REC` in JS key order, arrays by elem kind, structs via `dynkeys`+`dynget` with `toJSON` dyncall, undefined/functions skipped; traversal stack rooted in `Heap.temps`). Descriptor code 11 (`any`); desc 7 stringifies a `REC` map as its entries (fixes today's `{}` for `Record<string, V>`).
- **`instanceof` on `any`:** program classes via `itables`; `Array`/`Map`/`Set`/`RegExp`/`Date`/`Error` header or class checks. `Array.isArray(any)` is a header test.
- **Exceptions:** `throw v` accepts any type; `catch (e)`, `catch (e: unknown)`, `catch (e: any)` bind `Any` (`stmt.rs:606`); `(e as Error).message` is a checked unbox (1 load + 1 table byte); prelude `__uncaught(e: unknown)` uses `String(e)`. Host paths (`host.rs:700,763`, `ts_api/src/callback.rs:190`) only take/drop the exception; the app's uncaught logging formats non-Error values with `ts_rt_dyn_to_str`.

## 3. Interop

- **`any` -> typed parameter/return/field:** checked unbox; mismatch is a `TypeError` at the boundary (TS is unchecked here; unchecked wrong pointer types are memory corruption in native code, Q1). A plain dynamic object passed where an interface goes is copied by name.
- **Typed -> `any`:** boxing, free for pointers. `T[]` -> `any` aliases; `T[]` -> `any[]` is a boxing copy (Q8); a struct -> `any` aliases.
- **Native `#[ts_api]` calls** take only typed parameters; an `any` argument gets a checked unbox to the manifest's script type (`coerce_ext`). No `any` type is added to the manifest. `console.log`/`Debug.log` of `any` use `ts_rt_dyn_inspect_gc` (a bun-like subset).
- **Handles:** boxed in `HDR_HANDLE`; `===` compares ids; dynamic member access uses generated `H$dynget`/`H$dyncall` per boxed handle class (via `call_ext_fn`); unboxing checks the class index.
- **React/UI:** state slots may hold dyn words. `Record<string, unknown>`/`any` where a `PlatypusStyle` goes gets a prelude conversion `__style_of_dyn(r): PlatypusStyle` written in TS over `__se_num`/`__se_str` (`ui_dts.rs:343`), keeping `examples/arcade` working (Q3).

## 4. Phasing

One agent and one commit group each. Each phase adds `tools/platypus_ts_tests/bench/dynamicN.ts` + `gen_dynamicN_expected.ts` (run in bun, as `gen_json_expected.ts`) + `tests/lang/dynamic.rs` (every GC mode, and `PLATYPUS_TS_GC=stress`); proposed IDs `M9-TS-3400..3499` (re-check the maximum at the time).

**Guard test (phase 0, kept forever):** compiling `bench/bench.ts` and one example gives IR and object code identical to a build before the change (no `Box` nodes, no dyn tables, no extra frame slots): the contract that typed code keeps its fast path.

- **P0 representation and plumbing:** rename `Ty::JsValue` to `Ty::Opaque`; add `Ty::Any`, `is_traced()`, `ExprKind::Box`/`Unbox`; runtime `platypus_ts_rt/src/dynv.rs` (constants, encode/decode, `is_object_word`, typeof, truthy, strict eq, to_str, to_num, add; ported from the spike with undefined = 0), the `mark_ptr` filter, array elem kinds, closure trampoline word; compiler: `ty()` arms, `coerce` box/unbox, `IsNull`/`Truthy`/`typeof`/`===`/template/`String()` on `Any`, `as any`, typeof/instanceof/equality narrowing. Tests: round-trips, `typeof`, truthiness, null vs undefined, checked-unbox errors, GC stress.
- **P1 `catch (e: unknown)`, untyped `JSON.parse`, `Record<string, any>`:** throw any value; `Any` catch variable; prelude; map value kinds and `REC`; `Record<string, any>` through `dicts.rs`; dyn JSON parse/stringify; descriptor 11; records stringify as objects; minimal dynamic gets (`.k`, `[i]`, `length`, `?.`). Tests: bun-compared JSON round-trips (key order, nested arrays, unicode, null vs missing), `catch` of string/number/object, `e instanceof RangeError`.
- **P2 property access on everything:** `dyn_tables.rs`, `finish_dyn`, expandos; `Place::Dyn`; computed keys; `in`; `delete`; `for..in`; `Object.*`; destructuring; spread; struct/array <-> dynamic conversions; Map/Set through `any`; typed-array kinds.
- **P3 calls:** trampolines, `DynCall`, `dyncall` tables, `any` -> `Fn(S)` adapters, `dynmethods.rs` (push, map, filter, forEach, join, slice, indexOf, includes, split, trim, toFixed, get/set/has, ...), `for..of` over `any`.
- **P4 operators with JS semantics:** arithmetic, `+` coercion, relational, loose `==`, `ToPrimitive`, `++`/`--`, compound, bitwise; a generated operator x operand-kind table compared to bun.
- **P5 type-system polish:** user type guards; `object`/`{}`/`Function`; the `PlatypusStyle` conversion; `Promise<any>`/`Generator<any>`; generics with `any`; `noImplicitAny` messages; docs (`docs/platypus-ts.md`, `docs/differences.md`).
- **P6 handles and natives:** `HDR_HANDLE`, handle-class dyn tables, checked `any` arguments to `#[ts_api]`, `console.log(any)`, uncaught non-Error reporting in `platypus_app` (`main.rs:1027`, `windows.rs:878`).
- **P7 performance (only if the benchmark asks):** per-site inline caches, per-name dyn vtables for method calls, hidden-class `REC` objects.

## 5. Cost and performance

- **Typed code: zero.** No new instructions, same layouts, array header constants chosen at allocation; the GC mark does 2 extra ALU ops per traced slot. Enforced by the P0 guard test.
- **Boxing:** number ~3 instructions; objects/strings free; nullable one select; handle one 24-byte allocation. **Unboxing:** checked number compare+branch+subtract; class header load + itable byte; trusted free; interface <- dynamic object is an O(fields) copy.
- **Gets:** `REC` ~15-25 ns; struct through `any` ~5-15 ns; with P7 inline caches ~2-3 ns (the spike measured 14 ns per ~10 property operations). Dynamic call ~10-20 ns.
- **Benchmark to add:** `tools/platypus_ts_tests/bench/dyn_bench.ts` + `src/bin/dyn_bench.rs` (platypus vs bun, checksums must agree): parse a ~200 KB level JSON and sum fields three ways (typed `JSON.parse`, `any` walk, `any` walk after conversion); 10^6 `o.x += 1` on a `REC` object, a class instance through `any`, and a typed instance; 10^6 closure calls through `any`. Targets: typed paths unchanged; `any` walks within 3x of bun; P7 only if struct-through-`any` gets exceed ~10 ns.

## 6. Remaining differences from JavaScript (for `docs/differences.md`)

1. Checked conversions: `const n: number = anyV` throws `TypeError` when `anyV` is not a number (also typed parameters/returns, `as T`, native calls). Optional/nullable parameters accept undefined and null.
2. Copies: dynamic object -> interface/struct type and `T[]` -> `any[]` are copies; `T[]` -> `any`, class -> `any`, `Record` -> `any` alias.
3. Writes through `any` into a typed array or field convert to the declared type; a mismatch is a `TypeError`.
4. null vs undefined: a typed nullable boxes as `null`, except absent optional fields, which box as `undefined`. Typed code still does not tell them apart.
5. Classes are not values: no `new` on `any`, no prototypes, `Object.create`, `defineProperty`, `Symbol` keys, `Proxy`. Deleting a declared non-optional field is a `TypeError`. Expandos work.
6. Detached methods keep `this`; `fn.toString()` is a fixed text; `fn.length` is the declared count.
7. Arrays stay dense: writes past the end throw `RangeError`; no holes.
8. `console.log` formatting is a subset of bun's; error texts are JSC's for the common cases only.
9. `==` is real loose equality only when an operand is `any`; typed `==` stays `===`.

## 7. Open questions (recommendation each)

- **Q1 checked or unchecked `any` -> T:** checked (`TypeError`); unchecked is memory-unsafe natively and the check is 1-2 instructions.
- **Q2 `catch (e)` type:** always dynamic (`unknown`, as tsc strict); optionally keep the `Error` pointer representation invisibly when the program provably throws only Errors. Existing `(e as Error)` code keeps working.
- **Q3 `Record<string, unknown>` / `object` as `PlatypusStyle`:** make them dynamic plus the `any` -> `PlatypusStyle` prelude conversion at style sites, so arcade keeps working.
- **Q4 adding properties to class instances/typed objects through `any`:** expando side table (JS-compatible); alternative a `TypeError`.
- **Q5 dynamic object -> interface:** a copy by field name (consistent with typed `JSON.parse`), not a rejection.
- **Q6 dynamic object representation:** the `REC` `MapObj` first; hidden classes/inline caches only if the P7 benchmark needs them.
- **Q7 `noImplicitAny`:** keep it a compile error, message suggesting `: any`.
- **Q8 `T[]` -> `any[]`:** a copy (aliasing would let `any[]` writes break the typed array; TS covariance is unsound).
- **Q9 an int32 tag:** none; keeps the number test a single shift.
- **Q10 typed `JSON.parse` stays validating** (stricter than TS); `JSON.parse(s)` alone or `as any` gives a dynamic value.

## Key files

`tools/platypus_ts/src/ir.rs` (Ty::Any, Opaque rename, is_traced, Box/Unbox/DynCall) with `codegen.rs` and `frames.rs`; `lower/types.rs` (ty, union, unify, coerce arms); `lower/tagged.rs` (dyn narrowing) and `lower/ifaces.rs` (model for new `lower/dyn_tables.rs`; new `lower/dynamic.rs` fed from `expr.rs` member_of/place_of/arith/lower_compare/lower_call, `stmt.rs` try/throw, `json.rs`, `dicts.rs`, `object.rs`); `tools/platypus_ts_rt/src/lib.rs` (mark_ptr filter, array elem-kind headers, closure trampoline word, HDR_HANDLE), `map.rs`, `json.rs`; new `platypus_ts_rt/src/dynv.rs`, `dynmethods.rs`. Reference: `git show iron-js:tools/iron_js/src/rt.rs` and `iron-js:docs/research/iron-js-dynamic-tier.md`.
