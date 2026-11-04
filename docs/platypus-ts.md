# platypus_ts: the ahead-of-time TypeScript compiler

Code: [tools/platypus_ts](../tools/platypus_ts/src) (compiler), [tools/platypus_ts_rt](../tools/platypus_ts_rt/src/lib.rs)
(runtime and GC), [tools/platypus_ts_tests](../tools/platypus_ts_tests) (fixtures, tests, benchmarks).
Progress and next steps: [tools/platypus_ts/TODO.md](../tools/platypus_ts/TODO.md). Why it exists, the
measurements and the design notes: [platypus-ts-spike.md](research/platypus-ts-spike.md),
[platypus-ts-classes.md](research/platypus-ts-classes.md), [platypus-ts-engine-boundary.md](research/platypus-ts-engine-boundary.md).
Design note for `Intl.DateTimeFormat` options (step I3a is built on it): [platypus-ts-intl-datetime.md](research/platypus-ts-intl-datetime.md).

This compiler is the engine's script backend. It compiles a typed subset of TypeScript (games written
as ordinary classes, functions, components and React UI) to native code, either in process with the
Cranelift JIT (`platypus_app` loads `build/game.pbundle`, built by `platypus-build`) or to an object file
(`--aot`). The QuickJS backend was removed (2026-10-28; [scripting.md](scripting.md)): this is the only script backend. The move, its results and its limits are in [platypus-ts-native.md](platypus-ts-native.md); the
benchmarks are in [benchmarks.md](benchmarks.md). The sections below were written during the spike and
still describe the design.

## Running it

Run cargo from `tools/` (shell setup as in [CLAUDE.md](../CLAUDE.md)).

```bash
cd tools && cargo fmt --all
cd tools && cargo clippy --workspace --all-targets -- -D warnings
cd tools && cargo test --workspace
cd tools && PLATYPUS_TS_GC=stress cargo test -p platypus_ts_tests      # collect at every allocation
cd tools && cargo test -p platypus_ts_tests --test engine contacts # one area (lang, stdlib, engine, engine_api, ui), one filter
cd tools && cargo run --release -p platypus_ts_tests --bin bench   # bun vs native, 10,000 steps (the QuickJS column is gone)
cd tools && cargo run --release -p platypus_ts_tests --bin sine_bench 2000
```

`PLATYPUS_TS_GC` is read by `gc_mode_from_env` in [platypus_ts_tests/src/lib.rs](../tools/platypus_ts_tests/src/lib.rs):
`stress` collects at every allocation of generated code and poisons freed memory, `off` never
collects on its own. Any change to lowering, codegen or the runtime should also pass in stress mode.

The CLI (`cargo build --release -p platypus_ts`, binary `tools/build/release/platypus_ts`):

```
platypus_ts file.ts [--dump-ir] [--prefix p_] [--struct-base N] [-o out.obj] [--bindings out.rs]
                [--opt none|speed|speed_and_size] [--time N] [--manifest api.json]...
```

| Flag | Effect |
|---|---|
| `--dump-ir` | Print the typed IR (`ir::dump`) |
| `--prefix p_` | Exported functions are named `ts_<prefix><name>` (default no prefix) |
| `--struct-base N` | Number object headers from `N` (default 0); modules sharing one process need ranges at least 1000 apart |
| `-o out.obj` | Write the object file |
| `--bindings out.rs` | Write the generated Rust bindings |
| `--opt` | Cranelift `opt_level`, default `speed` |
| `--time N` | Compile `N` times and print the best time of each phase |
| `--manifest api.json` | Repeatable. An `#[platypus_api]` namespace the source may call (the JSON from `Namespace::to_json`, see [platypus-api.md](platypus-api.md)); the library entry point is `compile_with_manifests(src, opt, prefix, struct_base, &manifests)` |

## Pipeline

```
 .ts source --oxc parser--> AST --lower/--> Platypus IR (ir.rs) --codegen.rs (Cranelift)--> object file
                                                  |                                      + linked with platypus_ts_rt
                                                  +--layout.rs--> Rust bindings (repr(C) structs, extern "C")
```

`compile_with(src, opt, prefix, struct_base)` in [lib.rs](../tools/platypus_ts/src/lib.rs) runs it on a
thread with a 64 MiB stack (lowering recurses with source nesting) and returns an `Output`: object
bytes, bindings text, IR dump and the time of each phase.

- **No type checker.** The compiler reads the source's own annotations and infers the rest bottom
  up, pushing the expected type into literals. The program must already pass `tsc --strict`; narrowing
  is trusted, not proved.
- **The IR** is structured (blocks, loops, labelled `Break`/`Continue`, no CFG) and every expression
  carries its type. Numbers are `f64`, bools `i8`, string-literal union members `i32` (`Ty::Lit`; `M | undefined` is the same `i32`, tag 0 is reserved for "no tag"),
  everything else a 64-bit pointer. `T | null` is the pointer with 0 as null; `number | null` is an
  `f64` with one reserved NaN payload (`NULL_NUM_BITS`).
- **Strings.** `string` is a pointer to the runtime's `Str` (`Ty::Str`). A literal is a `Ty::Lit` tag
  in a string-literal-union context and a `Ty::Str` static object when the context is `string`
  (`ExprKind::Str(i)`: bytes in the object file's writable data, header `HDR_STR` with no heap id, so
  the collector ignores it; `Module.strs`). A tag used as a string converts (`ExprKind::TagStr`: a table
  of the tags' texts, or a static string when the tag is constant). `let` initialisers and array
  literals widen a literal to `string`; `const` keeps the literal type. Runtime calls are
  `RtFn::Str(symbol)`: a generic call to a `ts_rt_str_*` function; names ending `_gc` may collect and
  count as allocating in the root analysis (`frames.rs`). See "Strings in the compiler" below.
- **Objects** are `[header][8-byte slot per field]`; every field is one 8-byte slot, so offsets are
  trivial and the generated Rust structs match exactly.
- **Codegen** targets the host triple (no PIC, no unwind info). Exported functions are `ts_<prefix><name>`
  with the C calling convention; `ts_<prefix>init` runs the top-level initialisers and registers
  the struct pointer masks and global roots with the runtime. Runtime routines are imports
  (`ts_rt_*`).
- **Property tables.** For each class with `@property` fields the bindings also hold
  `pub const PROPS_<Class>: &[(&str, usize, &str)]`: name, byte offset in the object, type name
  (`number`, `boolean`, `string`, `object:<Class>`, ..., `?` appended when nullable), base class
  first. A host reads and writes properties at those offsets with no script running (a scene loader
  can set a field before any code does). The IR carries them as `Module::props`.
- **Engine calls.** `Module::externs` holds one `ExternDecl` per called symbol: a flattened scalar
  parameter list (a value class is already split into its fields) and, for a value result, an
  optional out pointer `(scalar type, count)` that codegen appends as the last parameter.
- **Value-class members.** The manifest parser adds a class named like a value class (`Vector3`) to
  `ClassFns` of that name without registering a handle type. `value_member` in
  [lower/expr.rs](../tools/platypus_ts/src/lower/expr.rs) finds the method or getter of the struct behind
  a value class; `lower_method` (lower/arrays.rs) and `member_of` call it as a direct extern call with
  the receiver first (`a.add(b)`, `v.magnitude`), before any user-defined target is tried. Fields
  still win over getters. Not supported: static functions on value classes, operators, compound
  assignment on value properties.
- **String arguments and results.** A `string` in the manifest is `ExtTy::Str`. As an argument the
  call passes the script string pointer, and codegen adds a trailing stack slot for the length
  when the extern has a string result. The callee borrows the bytes (`platypus_api::script_str`), so
  the string stays rooted for the call; a lone surrogate reaches the engine as U+FFFD. A string result
  comes back as bytes plus length from a per-thread buffer and is copied at once into a new script
  string with `ts_rt_str_new_gc`. That call can collect, so the root analysis (`frames.rs`) counts
  it as allocating. See [platypus-api.md](platypus-api.md#strings). Not supported: `string | null`
  parameters.
- **Linking** is the host's job: the test crate's `build.rs` writes the `.obj`, passes it with
  `cargo:rustc-link-arg`, and includes the bindings.

## Source files

| File | What |
|---|---|
| [manifest.rs](../tools/platypus_ts/src/manifest.rs) | Reads the `platypus_api` JSON manifests into a table of callable functions (`Manifest`, `ExternFn`), the handle type names, the value classes (`values`) and the handle classes (`classes`: `ClassFns` with `statics`, `methods`, `getters`, `setters`) |
| [lib.rs](../tools/platypus_ts/src/lib.rs) | `compile_source` / `compile_with_prefix` / `compile_with` / `compile_with_manifests`, `Output`; unit tests for lowering errors (`M9-TS-14..16`, `55`, `56`, `70`, `84`, `110`, `119`) |
| [ir.rs](../tools/platypus_ts/src/ir.rs) | The IR: `Ty`, `Module` (with `vtables` and `itables`), `Func`, `Stmt`, `Expr`/`ExprKind` (including `CallVirtual` and `InstanceOf`), `RtFn` (runtime calls), `field_offset`, `dump` |
| [frames.rs](../tools/platypus_ts/src/frames.rs) | Shadow-stack frame plan and the spill rule (see GC roots below) |
| [codegen.rs](../tools/platypus_ts/src/codegen.rs) | IR to Cranelift object: frames and spills, `eval_args`, `ts_init`, runtime calls, the method tables and `instanceof` byte tables (local data), indirect calls |
| [layout.rs](../tools/platypus_ts/src/layout.rs) | Rust bindings: `HDR_*` header constants, `#[repr(C)]` structs, `TAG_*`, `NULL_NUM_BITS`, `extern "C"` declarations |
| [bin/platypus_ts.rs](../tools/platypus_ts/src/bin/platypus_ts.rs) | The CLI above |
| [lower/mod.rs](../tools/platypus_ts/src/lower/mod.rs) | `lower_source`: collecting declarations, `lower_fn` (lowered on demand), bindings (direct or boxed), module-level variables |
| [lower/types.rs](../tools/platypus_ts/src/lower/types.rs) | Annotation to IR type, struct layouts (a union of objects becomes one struct), `coerce`, `to_bool` |
| [lower/expr.rs](../tools/platypus_ts/src/lower/expr.rs) | Expressions, assignment targets (places), calls, `new`, optional chains, `Math`, constant folding |
| [lower/stmt.rs](../tools/platypus_ts/src/lower/stmt.rs) | Statements: variables, destructuring, loops, labels, `switch` |
| [lower/arrays.rs](../tools/platypus_ts/src/lower/arrays.rs) | Array methods; callbacks are inlined into loops, `Iter` for `for...of` and spread |
| [lower/scan.rs](../tools/platypus_ts/src/lower/scan.rs) | Which names do closures capture (by name, so it only over-approximates) |
| [lower/closure.rs](../tools/platypus_ts/src/lower/closure.rs) | Closures lifted to `lambda(env, params...)`, function values, cells |
| [lower/strings.rs](../tools/platypus_ts/src/lower/strings.rs) | Strings: `+`/`+=`, template literals, comparisons, `length`, string and number methods, `String`/`Number`/`parseInt`/`parseFloat`, `String.fromCharCode`/`fromCodePoint`, truthiness |
| [lower/maps.rs](../tools/platypus_ts/src/lower/maps.rs) | `Map`, `Set`, `WeakMap`, `WeakSet`: construction, methods, live `for...of`, snapshots for spread and `Array.from` |
| [lower/json.rs](../tools/platypus_ts/src/lower/json.rs) | `JSON.stringify` / `JSON.parse`: the type descriptor blobs (format documented at the top of the file) |
| [lower/clone.rs](../tools/platypus_ts/src/lower/clone.rs) | `structuredClone`: one lifted clone function per type |
| [lower/regexp.rs](../tools/platypus_ts/src/lower/regexp.rs) | `RegExp`: literals, `new RegExp`, `test`/`exec`, the match object, and the String methods that take a regex (`match`, `matchAll`, `search`, `replace`, `replaceAll`, `split`) |
| [lower/date.rs](../tools/platypus_ts/src/lower/date.rs) | `Date`: constructors, `Date.now`/`UTC`/`parse`, getters and setters (local and UTC), the string conversions, and arithmetic and comparison on the time value |
| [lower/intl.rs](../tools/platypus_ts/src/lower/intl.rs) | `Intl`, steps I1 to I4: `localeCompare`, `normalize`, `toLocaleUpperCase`/`toLocaleLowerCase`, `new Intl.Collator`, `NumberFormat`, `DateTimeFormat`, `PluralRules`, `ListFormat` and `RelativeTimeFormat` (`lower_new_intl`); locale and options arguments are read at compile time (`options_spec`) |
| [platypus_ts_rt/src/intl_number.rs](../tools/platypus_ts_rt/src/intl_number.rs) | The `NumberFormat` runtime: option parsing, rounding and padding on a `fixed_decimal` `Decimal`, ICU4X formatting, scientific/engineering by hand |
| [platypus_ts_rt/src/intl_date.rs](../tools/platypus_ts_rt/src/intl_date.rs) | The `DateTimeFormat` runtime and `Date.prototype.toLocale*String`: ECMA-402 option defaults, mapping options to an ICU4X field set, pattern width rewriting (`adjust_pattern`), time zones through jiff |
| [platypus_ts_rt/src/intl_misc.rs](../tools/platypus_ts_rt/src/intl_misc.rs) | The `PluralRules`, `ListFormat` and `RelativeTimeFormat` runtime (options parsing, tables of formatters by id, bun's `RangeError` texts) |
| [platypus_ts_rt/src/intl_segment.rs](../tools/platypus_ts_rt/src/intl_segment.rs) | The `Intl.Segmenter` runtime: grapheme, word and sentence boundaries from ICU4X's segmenter, returned to the prelude as arrays of pieces |
| [platypus_ts_rt/src/intl_locale.rs](../tools/platypus_ts_rt/src/intl_locale.rs) | The `Intl.Locale` runtime: canonical tags, subtags and unicode extension keys, `maximize` / `minimize` |
| [platypus_ts_rt/src/intl_names.rs](../tools/platypus_ts_rt/src/intl_names.rs) | The `Intl.DisplayNames` runtime: region, script and language names from CLDR data |
| [lower/temporal.rs](../tools/platypus_ts/src/lower/temporal.rs) | `Temporal`: the `Temporal.X` to `Temporal_X` text rewrite, `Temporal.X.from(v)` routing, string arguments where a Temporal object is expected |
| [lower/temporal.ts](../tools/platypus_ts/src/lower/temporal.ts) | The `Temporal_*` classes, written in TypeScript and compiled in front of programs that mention `Temporal.` |
| [platypus_ts_rt/src/tz.rs](../tools/platypus_ts_rt/src/tz.rs) | IANA time zones for `ZonedDateTime`: canonical zone name, offset at an instant, local time to instant (compatible disambiguation), fixed-offset zones |
| [lower/bigint.rs](../tools/platypus_ts/src/lower/bigint.rs) | `bigint`: literals, binary and unary operators, comparison with bigints and numbers, `BigInt(x)`, `BigInt.asIntN`/`asUintN`, `toString(radix)`, `Number(big)` |
| [platypus_ts_rt/src/bigint.rs](../tools/platypus_ts_rt/src/bigint.rs) | The `BigInt` runtime: the object layout, operations on `num_bigint::BigInt`, parsing, conversions, JavaScriptCore's error texts |
| [platypus_ts_rt/src/uri.rs](../tools/platypus_ts_rt/src/uri.rs) | `encodeURI` / `decodeURI` and their Component forms, `escape` / `unescape`; `URIError`s with JavaScriptCore's texts |
| [platypus_ts_rt/src/intl_pad.rs](../tools/platypus_ts_rt/src/intl_pad.rs) | Generated by `bench/gen_intl_pad_table.ts` from the reference engine's ICU4C: which locales pad a numeric month, day and hour (`Padding`, 31 locales) |
| [throws.rs](../tools/platypus_ts/src/throws.rs) | `analyze`: which functions can throw (`Module.may_throw`, `any_throw`), by fixpoint over the call graph |
| [lower/prelude.rs](../tools/platypus_ts/src/lower/prelude.rs) | The Error classes, `RegExp` / `RegExpMatchArray`, `Date`, `Intl_Collator`, `Intl_NumberFormat`, `Intl_DateTimeFormat`, `Intl_PluralRules`, `Intl_ListFormat` and `Intl_RelativeTimeFormat`, as TypeScript source, compiled in front of programs that mention errors, regexes (or the string methods that take them), `Date`, `Intl` or bigints (each `Intl` class only when the program mentions it) |
| [lower/enums.rs](../tools/platypus_ts/src/lower/enums.rs) | Enums folded into constants (numeric) or string tags |
| [lower/tagged.rs](../tools/platypus_ts/src/lower/tagged.rs) | Unions of different types (`number \| [number, number]`): the tagged struct, wrapping, projecting, `typeof` / `Array.isArray` on a union and narrowing by path |
| [lower/accessors.rs](../tools/platypus_ts/src/lower/accessors.rs) | Getters in object literals (`get scene() { .. }`): the hidden closure field and the read through it |
| [lower/classes.rs](../tools/platypus_ts/src/lower/classes.rs) | Classes: fields, constructors, methods, accessors, statics, `#private`, inheritance (prefix layout, virtual dispatch, `instanceof`) |
| [lower/iter.rs](../tools/platypus_ts/src/lower/iter.rs) | Well-known symbol member names (`symbol_key`, `well_known`) and the iterator protocol over classes with `@@iterator` |
| [lower/generators.rs](../tools/platypus_ts/src/lower/generators.rs) | `desugar` (generators by source rewriting, before parsing), the generated `Generator$K` classes, the `__co*` intrinsics; `Coroutine` is a prelude handle class |
| [platypus_ts_rt/src/coro.rs](../tools/platypus_ts_rt/src/coro.rs) | Stackful coroutines (corosensei): resume/yield, shadow-stack relinking, GC scanning of suspended frames |
| [lower/asyncs.rs](../tools/platypus_ts/src/lower/asyncs.rs) | `desugar` for `async` functions, methods and arrows (before parsing, after generators), `await`, `new Promise<T>`; generates the `Promise$K` classes and their helper functions as TypeScript text |
| [lower/promise.rs](../tools/platypus_ts/src/lower/promise.rs) | Routes `__await`, `__asyncRet`, `.then` / `.catch` / `.finally` (by the callback's return type) and `Promise.resolve` / `reject` / `all` / `race` onto the generated helpers |
| [platypus_ts_rt/src/event.rs](../tools/platypus_ts_rt/src/event.rs) | The microtask queue and the timers (`queueMicrotask`, `setTimeout`, `setInterval`), and the host entry points `ts_rt_microtasks_run`, `ts_rt_timers_advance`, `ts_rt_run_loop` |

## Supported language

| Feature | Lowering |
|---|---|
| Types | `number`, `boolean`, `string`, interfaces and type aliases as structs, discriminated unions as one struct with every variant's fields, string-literal unions as tags (also `keyof typeof OBJ` / `keyof Iface`; a literal outside the declared union in a typed declaration is a compile error), `T \| null`, `T[]`, the typed arrays (see "Typed arrays"), `bigint` (see "BigInt"), `Map<K, V>`, `Set<T>`, `WeakMap`, `WeakSet` (see "Standard library tier 2"), `RegExp` and `Date` (see "Standard library tier 3"), `Record<string, V>` after `satisfies`, function types |
| Functions | `function` and arrow declarations; return types inferred; calls are direct |
| Default, optional, rest parameters | The caller fills in defaults (lowered in a scope of the earlier parameters), a missing optional is null, the rest array is built at the call site |
| Closures | Captured variables live in heap cells; the closure is `{func, env}` with `env` an array of cells; called as `func(env, args...)`. A top-level function used as a value gets a wrapper |
| Array callbacks | `map`, `filter`, `forEach`, `find`, `some`, `every` (and `reduce`, `findIndex`, `includes`) are index loops with an arrow callback's body inlined; no closure object. A non-arrow callback is called as a closure per element |
| Array library | `push`, `pop`, `shift`, `unshift`, `slice`, `splice`, `concat`, `reverse`, `fill(x, start?, end?)`, `sort(cmp)` and default `sort()` (stable merge sort in the runtime), `indexOf` / `lastIndexOf` / `includes` (with `fromIndex`, over numbers, strings, union members, booleans and objects), `at`, `length = n`, `Array.from/of/isArray` (`Array.from({ length: n }, f)`, `Array.from(xs \| set \| map.keys(), f)`), `new Array(n)` / `new Array<T>(n)` / `new Array(n).fill(x)` (n zeroed slots), `flat(depth)` (any depth: a run-time one gives a union array), `flatMap`; `xs[i] = v` inside the array stores, at `i === xs.length` it appends (as JavaScript), anywhere else (past the end, negative, NaN) it throws a `RangeError` (JavaScript would make holes; the error classes come along when a program writes `x[i] = ..`); an out-of-range read gives `undefined`, never garbage; typed arrays still ignore out-of-range writes. Spread arguments: `Math.min(a, ...xs)` / `Math.max(...xs)` (arrays and typed arrays), `f(...xs)` into rest parameters and, as the last argument, into fixed parameters (element `j`, or the default, or null past the end; `lower_fn_args_spread`); not for calls of function values or `new`. See "Standard library tier 1" for the rest |
| Loops and labels | `while`, `for(;;)`, `do...while`, `for...of`, labelled `break`/`continue` and blocks; per-iteration `let` cells when a closure captures the variable |
| `switch` | The clause index is chosen by `===` compares, then the bodies run in order (fall-through, `default` anywhere) |
| Enums | Numeric and string enums fold to constants, no run-time cost |
| Destructuring | Declarations, parameters, `for...of` variables and callbacks; defaults via `IsNull` |
| Spread | Arrays, objects (field by field, in source order, new shapes allowed) and calls into a rest parameter |
| Module variables | A literal `const` is folded; any other `const`, and every `let`/`var`, is a data slot written by `ts_init` (`Place::Global`) |
| `Math` and operators | Inline instructions or `ts_rt_*` (libm, JS `pow`, `random`); `**`, `~`, `??=`, `\|\|=`, `&&=`, bit operations with ToInt32; the right side of `&&` / `\|\|` may return void (`cond && f()`): it is evaluated, runs `f`, and counts as false like `undefined` |
| Classes | A struct plus functions: constructor `C$init(this, ...)` (`new C(x)` allocates, then calls it), methods with `this` as first parameter, getters/setters, statics as globals, `#private`, parameter properties, `this` in arrows; `extends`, `super(...)`, `super.m()`, overriding, `abstract`, virtual dispatch, `instanceof` (see below); methods as values (`obj.m` is a closure over the receiver, so unlike JS a detached method keeps its `this`); arrow-function fields (the constructor scan includes field initialisers); `@property` on instance fields as compile-time metadata |
| Optional chaining, `!`, `??`, `?:` | Inline null tests |
| Engine API calls | With a manifest, `Namespace.fn(args)` is `CallExtern`: a direct call of the manifest's symbol (`platypus_<Namespace>_<fn>`) with the manifest's C signature, arguments checked by count and type. Numbers stay `f64` in scripts and convert at the call: `f32` demotes in and promotes out, `i32`/`u32` use a saturating truncation, `bool` is a byte. Handles are `Ty::Handle(name)`: a 64-bit id, not a GC pointer (not traced or rooted, `===` compares ids). In a `T \| null` slot 0 is null, so a live handle is never 0. An engine function must not call back into scripts or allocate in the script heap. Handle classes (the manifest's `classes`, see [platypus-api.md](platypus-api.md#handle-classes)) are called through the same path: `emit_extern` makes the call from already-lowered arguments and `call_ext_fn` adds the receiver in front of the call's own arguments (counts are checked without it). `obj.prop` on a handle is the getter's call (`member_of`); `obj.prop = v` is a `Place::Ext` whose write calls the setter (a property with no setter, or an unknown one, is a compile error; compound assignment on a value property is not supported); `obj.method(..)` (`lower_method`) calls the method; `Class.f(..)` on a class name that is not a local is the static. Value classes (`new Vector3(1, 2, 3)`) are ordinary structs with one `number` field per manifest field; as an argument one is passed as its flattened field reads (every argument goes through a temporary first, to keep evaluation order), and a result is a new object filled through a trailing out pointer by `CallExternInto` (a stack slot, then the scalars widened into the fields) |

**Strings in the compiler** ([lower/strings.rs](../tools/platypus_ts/src/lower/strings.rs), slice 2).
- `+` and `+=` convert numbers, booleans and null by JavaScript's rules (`ts_rt_num_to_str_gc` and
  static texts); two constants fold at compile time. Template literals are one `concat_n_gc` over an
  array of parts. A chain `a + b + c` still allocates once per `+`.
- `===`/`!==` call `ts_rt_str_eq` (null-safe; codegen emits it for `Str` operands); `<`, `<=`, `>`, `>=`
  call `ts_rt_str_compare` (returns `f64`, code unit order). `s.length` is `ts_rt_str_length`.
  Truthiness is "non-empty" (and non-null).
- String methods map to runtime functions by a table, with default arguments filled in by the
  compiler (`slice`, `substring`, `indexOf`, `padStart`, `split`, `trim`, `toUpperCase`, `at`, ...).
  Number methods `toString` and `toFixed`; globals `String`, `Number`, `parseInt`, `parseFloat`;
  `String.fromCharCode` (`ts_rt_str_from_char_code_gc`) and `String.fromCodePoint`; `join` of a
  `string[]`.
- Compile errors: unsupported string methods (`localeCompare`, `normalize`, `replace`, `match`, ...).
  String literals with lone surrogates work (oxc encodes them as U+FFFD plus hex; `Module::strs` is
  WTF-8 bytes).
- `join` of a `number[]` is `ts_rt_str_join_nums_gc`; a top-level `let s = "x"` widens to `string`.
  Runtime functions that JavaScript makes throw raise a runtime error (see "Exceptions").
- Not done: `codePointAt` past the end (gives plain NaN, not undefined), optional chaining on
  strings, object-literal fields with no declared type, `join` of other non-string arrays.
- The binding generator emits `TAG_*` constants only for texts that are valid, distinct identifiers.
- Tests: `bench/strings.ts` with `tests/strings.rs` (M9-TS-178..182, prefix `str_`, base 16000);
  expected values come from `bench/gen_strings_expected.ts` (bun) into `strings_expected.json`.

**Exceptions** ([throws.rs](../tools/platypus_ts/src/throws.rs), [lower/prelude.rs](../tools/platypus_ts/src/lower/prelude.rs),
the `Try` handling in [lower/stmt.rs](../tools/platypus_ts/src/lower/stmt.rs); design in
[platypus-ts-stdlib.md](research/platypus-ts-stdlib.md) section 4.1). `throw`, `try`/`catch`/`finally` and the
Error classes. Exceptions are checked returns, not unwinding, and only in functions that can throw.
- **Throw.** `Stmt::Throw(e)` stores the exception object in a per-thread slot of the runtime
  (`ts_rt_throw`; the slot is a GC root) and jumps to the innermost handler, or, with none, to the
  function's exception exit: pop the shadow-stack frame and return a zero value. `Stmt::Try(body,
  catch_var, handler)` sets the handler for its body.
- **Checks.** `throws.rs` computes `Module.may_throw` by a fixpoint over the call graph, counting
  only functions reachable from the exported functions and `ts_init` (closures through `MakeClosure`,
  virtual calls through every method-table entry). A closure or virtual call may throw if anything
  reachable throws (`Module.any_throw`). After such a call codegen calls `ts_rt_exc_pending()` and
  branches to the handler or the exit. **Cost model: a function that cannot throw, and a program with
  no `throw` at all, pay nothing** (no check, no extra code); a call to a function that can throw costs
  one load and a branch.
- **Catch.** `ts_rt_exc_take()` moves the exception into the catch local, typed `Error` (the base
  class); `e instanceof RangeError` narrows like any class. Only Error objects can be thrown (a
  compile error otherwise).
- **Host.** An uncaught exception is `ts_rt_exc_pending()` after an exported function returns; the host
  takes the error object with `ts_rt_exc_take()`.
- **`finally`.** Each `try ... finally` has a completion record: a `kind` variable and a `done`
  label. A `return`/`break`/`continue` in the try or catch stores its exit (and return value) and
  leaves to `done`; the finalizer then runs once and the recorded exit is taken through any enclosing
  finalizers (`FinCtx`, `Exit`, `fin_applies`, `leave_through_finally`, `emit_jump`). An exception
  takes a catch-all handler that runs the finalizer and rethrows.
- **Error classes.** A TypeScript prelude (`lower/prelude.rs`) is compiled in front of programs that
  mention errors: `Error` (`name`, `message`, `toString`), `EvalError`, `RangeError`, `ReferenceError`,
  `SyntaxError`, `TypeError`, `URIError`, and `__rtThrow`. Line numbers in compile messages skip the
  prelude.
- **Runtime errors.** Runtime functions that JavaScript makes throw (`repeat(-1)`, `toFixed(101)`,
  `toString(37)`, `fromCodePoint(-1)`, absurd padding) record a pending runtime error
  (`ts_rt_error_kind`, `ts_rt_error_message_gc`; messages equal bun's) and return a harmless value.
  The compiled call is followed by a check that calls `__rtThrow(kind, message)`.
- `cause` and `AggregateError` are built (see "URI functions and error causes"). Not built: `error.stack`,
  `Error(...)` without `new`, throwing non-Error values.
- Tests: `bench/errors.ts` with `tests/errors.rs` (M9-TS-187..197, prefix `err_`, base 17000);
  expected values come from `bench/gen_errors_expected.ts` (bun) into `errors_expected.json`.

**Standard library tier 1, steps E-H** ([lower/object.rs](../tools/platypus_ts/src/lower/object.rs), the expression and
call lowering in `lower/expr.rs`, `arrays.rs`, `strings.rs`; design in [platypus-ts-stdlib.md](research/platypus-ts-stdlib.md)).
JavaScript's answer exactly; anything not built is a compile error.
- **`typeof`.** Static, from the type; the result is a string-literal tag, so comparisons fold to
  constants. `typeof undeclared` is `"undefined"`. The compiler does not tell null from undefined: a
  nullable value that is null reports `"undefined"`, and only the literal `typeof null` is `"object"`.
- **`Number` and `Math`.** `Number.isInteger/isNaN/isFinite/isSafeInteger/parseFloat/parseInt`, the
  constants (`EPSILON`, `MAX_SAFE_INTEGER`, `MIN_SAFE_INTEGER`, `MAX_VALUE`, `MIN_VALUE`,
  `POSITIVE_INFINITY`, `NEGATIVE_INFINITY`, `NaN`), global `isNaN`/`isFinite`; `Math.sinh cosh tanh
  asinh acosh atanh expm1 log1p` (runtime libm functions).
- **`Array`.** `toSorted toReversed findLast findLastIndex reduceRight flat flatMap`; `sort()` with no
  comparator (numbers and strings, compared as strings in code unit order, stable:
  `ts_rt_arr_sort_default`); `for (const [i, x] of xs.entries())` and `keys()`/`values()` in `for...of`;
  array rest in patterns (`[first, ...others]`).
- **`console`.** `log info debug warn error` of strings, numbers, booleans and null, joined by spaces and
  sent to a host hook (`ts_rt_set_console`; stderr by default; levels 0 to 4). Objects are a compile error.
- **`Object`.** Keys are the struct's fields, fixed at compile time. `Object.keys/values/assign/freeze/
  hasOwn`, `for (const [k, v] of Object.entries(o))`; `values` and `entries` need one common value type;
  `hasOwn` and `in` need a string literal key. Object spread in source order with new shapes
  (`{ ...a, z: 3 }`) and object rest (`const { x, ...rest } = o`). `freeze` does not reject later writes.
  Class fields initialised with a string literal are `string`.
- **Fixed.** A `break` inside a callback inlined into a `try`/`finally`; the string scan cursor is one
  atomic word.
- Not built: Array `with`, `toSpliced`, `copyWithin`, `Object.fromEntries`,
  `console` of objects.
- Tests: `bench/stdlib.ts` with `tests/stdlib.rs` (M9-TS-199..205); expected values come from
  `bench/gen_stdlib_expected.ts` (bun) into `stdlib_expected.json`.

**Standard library tier 2: `Map`, `Set`, `WeakMap`, `WeakSet`** ([lower/maps.rs](../tools/platypus_ts/src/lower/maps.rs),
runtime in [platypus_ts_rt/src/map.rs](../tools/platypus_ts_rt/src/map.rs); design in
[platypus-ts-stdlib.md](research/platypus-ts-stdlib.md)).
- **Types.** `Ty::Map(K, V)` and `Ty::Set(K)`. Keys are numbers, strings, booleans, handles or objects
  (by identity); a string-literal union key is stored as a string. The old stub that treated `Set<T>` as
  an array is gone. A collection needs its types from a `new Map<K, V>()` argument, an annotation, or
  `new Map([[k, v], ...])` / `new Set(array)` (element type inferred); otherwise it is a compile error.
  The argument may be a conditional of a collection and an array (`new Set(c ? set : [])`): each arm makes the
  collection, an empty `[]` taking the other arm's type (`M9-TS-3531`).
- **Raw words.** Keys and values cross into the runtime as 64-bit words: new IR nodes `ExprKind::Bits`
  (typed value to word) and `Unbits` (word back to the type). The map's key kind (0 numbers, 1 strings,
  2 objects, 3 plain words) tells equality, hashing and the collector which words are pointers.
- **Methods.** `get` (a nullable result for pointer and number values; a compile error for boolean or
  literal-union values, where null cannot be told from a value), `set`/`add` (chained), `has`, `delete`,
  `clear`, `size`, `forEach`, `keys()`/`values()` as snapshot arrays, spread and `Array.from` of a Set or
  of `map.keys()`, and live `for (const [k, v] of map)`, `for (const x of set)` and
  `.entries()/.keys()/.values()` in `for...of`. A `for...of` and `forEach` keep a cursor (below), so
  deleting and adding during the loop behave as in JavaScript.
- **Weak collections** are the strong versions: a script cannot tell the difference except by memory.
- Not built: `new Map(otherMap)`, map spread, `Map.groupBy`, `entries()` outside `for...of`.
- Tests: `bench/collections.ts` with `tests/collections.rs` (M9-TS-209..213), expected values from
  `bench/gen_collections_expected.ts` (bun) into `collections_expected.json`; runtime unit tests
  M9-TS-206..208 in `map.rs`; compile errors M9-TS-214.

**Standard library tier 2: `JSON`** ([lower/json.rs](../tools/platypus_ts/src/lower/json.rs), runtime in
[platypus_ts_rt/src/json.rs](../tools/platypus_ts_rt/src/json.rs)). Driven by the program's static types.
- **Descriptors.** For each type the compiler writes a byte blob (`Module::blobs`, `ExprKind::Blob`):
  a kind byte per type (number, boolean, string, literal union, array, nullable, object) and, for an
  object, a header word and its fields. Struct header words are written as id + 1; the module's
  `struct_base` is added at code generation. The runtime (`ts_rt_json_stringify_gc`,
  `ts_rt_json_parse_gc`) walks the blob.
- **`JSON.stringify(v, null, indent)`.** Numbers (non-finite becomes `null`, `-0` becomes `0`), booleans,
  strings with JSON escapes (lone surrogates as `\udXXX`), literal unions, arrays, nullable values
  (`null`), objects in key order (array-index keys first), `Map`/`Set` as `{}`. A field declared with
  `?` (`Field.optional`) is left out when null. A subclass instance prints its own fields: the
  descriptor lists the class and its subclasses and the object's header picks one. Indentation by a
  number (clamped to 10) or a string.
- **`JSON.parse`.** Needs its type from the context: `const x: T = JSON.parse(s)` or `JSON.parse(s) as
  T`. Objects, arrays and strings are built without collecting mid-parse (one collection check
  before). Fields the type lacks are skipped. Syntax errors follow JavaScriptCore's parser and
  messages exactly (`SyntaxError: JSON Parse error: ...`; 67 malformed texts checked against bun). A
  shape mismatch (wrong kind, missing property, string outside a literal union) is a `TypeError`, raised
  only after the whole text proved to be valid JSON, so a syntax error anywhere wins.
- **Errors.** Runtime errors reach the program through `__rtThrow(kind, message)` (1 `RangeError`,
  2 `TypeError`, 3 `SyntaxError`, 4 `URIError`).
- Also in this step: `join` of boolean arrays.
- Not built: reviver, replacer, `toJSON`. See [differences.md](differences.md).
- Tests: `bench/json.ts` with `tests/json.rs` (M9-TS-215..221), expected values from
  `bench/gen_json_expected.ts` (bun) into `json_expected.json`.

**Standard library tier 3: `RegExp`, step R1** ([lower/regexp.rs](../tools/platypus_ts/src/lower/regexp.rs), runtime in
[platypus_ts_rt/src/regexp.rs](../tools/platypus_ts_rt/src/regexp.rs)). The engine is the `regress` crate (an
ECMAScript regex engine; `platypus_ts_rt` uses it with the `utf16` feature). `platypus_ts` depends on `platypus_ts_rt`
only to validate regex literals at compile time (`regex_validate`).
- **Runtime.** A per-thread registry of compiled regexes, each named by a number; equal (pattern, flags)
  share one entry. Positions are UTF-16 units: an ASCII string is matched as bytes, any other as UTF-16
  text; `u`/`v` regexes match by code point pairs. Sticky `y` checks that the match starts at `lastIndex`.
  Capture group names are scanned from the pattern. Functions: `ts_rt_regex_new`, `_flags_gc`,
  `_source_gc`, `_exec_gc`, `_match_index`, `_match_end`, `_groups_gc`. Syntax errors use JavaScriptCore's
  wording (mapped from regress's messages); bad flags say `Invalid flags supplied to RegExp constructor.`
- **Objects.** A regex is an ordinary object of the prelude class `RegExp` with the fields `source`,
  `flags`, `global`, `ignoreCase`, `multiline`, `dotAll`, `unicode`, `unicodeSets`, `sticky`, `hasIndices`,
  `lastIndex` and `id`. `/re/flags` literals (validated at compile time), `new RegExp(p, f)`, `RegExp(p, f)`
  and the copy `new RegExp(re, f)` build it.
- **`test` and `exec`.** Both follow `lastIndex` for `g`/`y` regexes and reset it on a miss. `exec` returns
  a `RegExpMatchArray` (`matches: (string | null)[]`, `index`, `input`, `groups: Map<string, string | null>
  | null`). The match object is also seen as its array (`match_view`): `m[i]`, `m.length`, destructuring,
  spread, `for...of` and the array methods. `m.groups.name` reads the groups map by name (a property read on
  a `Map<string, V>`). `String(re)` and objects with `toString()` convert to strings.
- **Limits** (see [differences.md](differences.md)): a group that did not take part is null where JavaScript
  has undefined, so `m[1].length` dereferences null instead of throwing a `TypeError`; `groups === null`
  is true where JavaScript has undefined; a match is an object seen as its array, not an array (see
  "Tuples" below for tuple types).
- Tests: `bench/regexp.ts` with `tests/regexp.rs` (M9-TS-222..231), expected values from
  `bench/gen_regexp_expected.ts` (bun) into `regexp_expected.json`.

**Step R2: the String methods that take a regular expression** (section "the String methods that take a regular
expression" in [lower/regexp.rs](../tools/platypus_ts/src/lower/regexp.rs), hooked in from
[lower/strings.rs](../tools/platypus_ts/src/lower/strings.rs); runtime appended to
[platypus_ts_rt/src/regexp.rs](../tools/platypus_ts_rt/src/regexp.rs)).
- **Methods.** `search`, `match`, `matchAll`, `replace`, `replaceAll` and `split` accept a `RegExp` or a string.
  A string becomes a regex: `match`, `search` and `matchAll` treat it as a pattern; `replace`, `replaceAll` and
  `split` match it literally (`ts_rt_regex_escape_gc`). Results are bun's, with UTF-16 indices.
- **`match`.** With a `g` regex it returns the matched texts and resets `lastIndex` to 0; without `g` it returns
  the match object, as `exec` does. **`matchAll`** is eager: it returns an array of match objects (so `for...of`,
  spread and `map` work) from a copy of the regex. A non-global regex throws bun's `TypeError`, as does
  `replaceAll` with a non-global regex.
- **Replacement strings** support `$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn` and `$<name>` (`GetSubstitution`).
- **Replacement functions** are inlined. The parameters are (match, groups..., offset, string, namedGroups). A
  function with more than the match as a parameter needs the regex's groups at compile time: a regex literal in
  the call, `new RegExp("literal")`, or a function-local `const` declared from one. Otherwise, and for too many
  parameters, it is a compile error; rest parameters are not supported.
- **`split(re, limit)`** includes the captures; `split(string, limit)` now works too.
- **Runtime.** `ts_rt_regex_search`, `_match_all_gc`, `_replace_gc`, `_split_gc`, `_escape_gc` and `_advance`
  (the next index after an empty match, a whole pair under `u`/`v`); `join` works on `(string | null)[]`.
- **Limits:** a group that did not take part is a null element; module-level `const` regexes carry no group
  information for replacement functions (see [differences.md](differences.md)).
- Tests: `bench/regexp2.ts` with `tests/regexp2.rs` (M9-TS-232..239), expected values from
  `bench/gen_regexp2_expected.ts` (bun) into `regexp2_expected.json`.

**Standard library tier 3: `Date`** ([lower/date.rs](../tools/platypus_ts/src/lower/date.rs), runtime in
[platypus_ts_rt/src/date.rs](../tools/platypus_ts_rt/src/date.rs); small hooks in `lower/{expr,arrays,strings,prelude,mod}.rs`).
- **Object.** `Date` is a prelude class with one field, `t`: the time value in ms (NaN is an invalid date). It
  is added when the source mentions `Date`. `===` is identity; `-`, `+d`, `Number(d)`, `<` and `>` use `t`.
- **Supported.** `new Date()`, `(ms)`, `(string)`, `(date)` and `(y, m, d?, h?, mi?, s?, ms?)` (plain
  arguments, at most seven); `Date.now`, `Date.UTC`, `Date.parse`; every getter and setter, local and UTC;
  `getTime`, `valueOf`, `setTime`, `getTimezoneOffset`; `toISOString` (RangeError `Invalid Date` for an invalid
  date, as JavaScriptCore), `toJSON`, `toUTCString` / `toGMTString`, `toDateString`.
- **Calendar.** MakeDay, MakeTime and TimeClip follow the spec. Local time uses jiff's tz database (jiff 0.2
  in `platypus_ts_rt`): the OS zone, or the IANA name given to `ts_rt_date_set_zone` (the tests use it; a host can
  too). A repeated local time (clock set back) reads as the earlier instant; a skipped one (clock set forward)
  is read with the offset from before the jump, as engines do. Times beyond jiff's range (years +-9999) are
  mapped by whole 400-year Gregorian cycles. `getTimezoneOffset` truncates to whole minutes toward zero (old
  local mean time), like engines.
- **Parsing** (`ts_rt_date_parse`). ISO forms first: a date alone is UTC, a date-time without an offset is
  local; expanded years, `24:00`, lowercase `t`/`z`, a space separator, offsets with or without a colon, a
  fraction truncated to ms. Then a legacy tokenizer: month names by their first three letters, weekdays
  ignored, `M/D/Y`, `Y/M/D`, `Y-M-D`, AM/PM, `GMT`/`UTC`/`Z`/`EST`/`EDT`/`CST`/`CDT`/`MST`/`MDT`/`PST`/`PDT`,
  `+hhmm`, parenthesised comments, and two-digit years (0-49 are 20xx, 50-99 are 19xx). Checked against bun
  for about 76 strings in five zones.
- **Runtime.** `ts_rt_date_now`, `_field`, `_offset`, `_from_parts`, `_set`, `_clip`, `_iso_gc`,
  `_utc_string_gc`, `_date_string_gc`, `_to_string_gc`, `_parse`, `_set_zone`.
- **`toString` / `toTimeString`** (`ts_rt_date_to_string_gc`; `String(date)`, template strings and `date + x`
  use the same text): `Mon Oct 19 2026 10:30:00 GMT-0400 (Eastern Daylight Time)`, equal to bun's. The prelude
  `Date` class has a `toString` method (`__dateToString`), so generic stringification works, and `date + x`
  concatenates that string like JavaScript. The offset is written in whole minutes toward zero (`GMT-0456`
  for old local mean time). The zone name (`zone_name`) comes from ICU4X's root English long specific names
  through the zone's IANA id. For old dates, and dates whose raw offset has no name, the name follows whether
  the zone was on daylight time then: a recent winter or summer day with the same daylight state is used for
  the lookup, as engines do. A zone without an IANA id (a fixed offset) prints `GMT+hh:mm`.
- **Compile errors until Intl step I3:** `toLocaleString`, `toLocaleDateString`, `toLocaleTimeString`.
- **Limits** (see [differences.md](differences.md)): `Date()` without `new`; a few exotic legacy strings;
  `Date` inside typed JSON.
- Tests: `bench/date.ts` with `tests/date.rs` (M9-TS-240..253, 276), expected values from
  `bench/gen_date_expected.ts` (bun) into `date_expected.json`.

**Standard library tier 3: `Intl`, step I1** ([lower/intl.rs](../tools/platypus_ts/src/lower/intl.rs), runtime in
[platypus_ts_rt/src/intl.rs](../tools/platypus_ts_rt/src/intl.rs); hooks in `lower/{strings,expr,types,prelude,mod,arrays}.rs`).
The runtime uses ICU4X (`icu` 2.x in `platypus_ts_rt`) with its compiled CLDR data.
- **Supported.** `String.prototype.localeCompare(that, locales?, options?)`, `normalize(form?)` (NFC, NFD, NFKC,
  NFKD; lone surrogates are left alone), `toLocaleUpperCase` / `toLocaleLowerCase(locales?)` (language-sensitive:
  `tr`, `az`, `lt`, `el`, ...) and `new Intl.Collator(locales?, options?)` with `.compare(a, b)`.
- **Collator object.** `Intl.Collator` is the prelude class `Intl_Collator` (the type name maps to it, in
  `types.rs`), added when the source mentions `Intl.`. It holds an id into the runtime's collator table;
  `compare` is a closure field, so `arr.sort(c.compare)` works. No other `Collator` method is built.
- **Locales.** A string, or an array literal or array of strings; the first tag wins. A bad tag raises
  `RangeError: invalid language tag: <tag>`. No argument means the default locale, which the host sets with
  `ts_rt_intl_set_locales("en-US,de,ja")` (the first is the default; `en-US` if never called). The per-game
  list from `package.json` is not wired to it yet (that needs `platypus_app`).
- **Options** (collation): `usage`, `sensitivity` (base, accent, case, variant), `numeric`, `caseFirst`,
  `ignorePunctuation`. The argument must be an object literal with literal values: the compiler turns it into
  a `key=value;` string, so the runtime never sees an object. Anything else is a compile error. Bad values
  raise bun's `RangeError` text.
- **Runtime.** `ts_rt_intl_set_locales`, `ts_rt_str_normalize_gc`, `ts_rt_str_locale_case_gc`,
  `ts_rt_collator_new`, `ts_rt_collator_compare`, `ts_rt_str_locale_compare`.
- **Checked against bun** for en, de, sv, tr, ja and zh sorts, special casing and normalization.
- **Limits** (see [differences.md](differences.md)): all locales' data is linked (no trimming yet); an
  unsupported locale id falls back to root collation; `resolvedOptions`, `supportedLocalesOf` and the other
  `Collator` methods are not built; ICU4X's CLDR version can order a few things differently from the engine's.
- **Still to come:** `Locale`, `DisplayNames`, `Segmenter` (the other classes are in steps I2 to I4 below).
- Tests: `bench/intl.ts` with `tests/intl.rs` (M9-TS-254..263), expected values from
  `bench/gen_intl_expected.ts` (bun) into `intl_expected.json`.

**Standard library tier 3: `Intl`, step I2: `NumberFormat`** ([lower/intl.rs](../tools/platypus_ts/src/lower/intl.rs),
runtime in [platypus_ts_rt/src/intl_number.rs](../tools/platypus_ts_rt/src/intl_number.rs); hooks in
`lower/{strings,prelude,expr,mod}.rs`). `platypus_ts_rt` enables ICU4X's `unstable` feature (the experimental
currency and percent formatters) and `fixed_decimal` with `ryu`.
- **Supported.** `Number.prototype.toLocaleString(locales?, options?)` (in `strings.rs`) and
  `new Intl.NumberFormat(locales?, options?)` with `.format(n)`. `Intl.NumberFormat` is the prelude class
  `Intl_NumberFormat`; `format` is a closure field, so `numbers.map(nf.format)` works. Locales and the
  options literal are read at compile time like the collator's (`lower_new_intl`, `options_spec` are shared).
- **Options.** `style` (`decimal`, `percent`, `currency`; `unit` raises a `RangeError` for now), `currency`,
  `currencyDisplay` (`symbol`, `narrowSymbol`, `code`, `name`), `currencySign` (`standard`, `accounting`),
  `notation` (`standard`, `scientific`, `engineering`, `compact`), `compactDisplay`, `signDisplay` (all five),
  `useGrouping`, `minimumIntegerDigits`, `minimumFractionDigits` / `maximumFractionDigits`,
  `minimumSignificantDigits` / `maximumSignificantDigits`, `roundingMode` (all nine), `trailingZeroDisplay`.
  Defaults follow ECMA-402: currency digits come from an ISO 4217 table; compact keeps whole numbers from two
  integer digits and otherwise two significant digits.
- **How.** Rounding and padding are done on a `fixed_decimal::Decimal`; ICU4X then formats it (decimal,
  compact with the exponent chosen first, percent). Currency and non-finite values use a placeholder: the
  style's formatter formats the digit 7 and the compiler-side code swaps in the number string, because
  ICU4X's currency formatter forces the currency's own fraction digits. Scientific and engineering are
  assembled by hand (`1.235E6`, with the locale's digits and decimal mark). NaN and infinity are text (`NaN`,
  `∞`; NaN is localized only for `ar` and `ru`).
- **`formatToParts`.** `nf.formatToParts(n)` formats the number, then splits the string using the locale's digits,
  decimal separator and group separator (`number_parts` in `intl_number.rs`). Types: `integer`, `group`, `decimal`,
  `fraction`, `minusSign`, `plusSign`, `percentSign`, `currency`, `compact`, `exponentSeparator`, `exponentInteger`,
  `exponentMinusSign`, `nan`, `infinity` and `literal`. The result is `Intl.NumberFormatPart[]`, the prelude class
  `Intl_Part { type, value }` (`INTL_PARTS` in `lower/prelude.rs`, decoded from a separator-joined string).
- **`resolvedOptions`.** `nf.resolvedOptions()` gives the prelude class `Intl_Resolved` (`ts_rt_number_format_resolved_gc`
  in `intl_number.rs`, passed as `key=value;` pairs and parsed in the prelude): `locale`, `numberingSystem` (always
  `latn`), `style`, `currency`, `notation`, `signDisplay`, `minimumIntegerDigits`, and either the fraction digits or
  the significant digits that apply. Fields not listed (`currencyDisplay`, `useGrouping`, `roundingMode`,
  `roundingIncrement`, `trailingZeroDisplay`, ...) are not in the result. The numeric fields are `0` when they do
  not apply (JavaScript leaves them out).
- **Limits** (see [differences.md](differences.md)): `style: "unit"`, compact currency,
  `formatRange` and `supportedLocalesOf` are not built; Number values only (no BigInt,
  no strings).
- Checked against bun for 15 locales and many option combinations.
- Tests: `bench/numfmt.ts` with `tests/numfmt.rs` (M9-TS-264..275), expected values from
  `bench/gen_numfmt_expected.ts` (bun) into `numfmt_expected.json`.

**Standard library tier 3: `Intl`, step I3a: date formatting** ([lower/intl.rs](../tools/platypus_ts/src/lower/intl.rs),
runtime in [platypus_ts_rt/src/intl_date.rs](../tools/platypus_ts_rt/src/intl_date.rs); hooks in
`lower/{date,strings,prelude,expr,mod}.rs`, `Date::icu_parts` in `date.rs`). Design and the open choice between
ICU4X lite and a full build: [platypus-ts-intl-datetime.md](research/platypus-ts-intl-datetime.md).
- **Supported.** `Date.prototype.toLocaleString` / `toLocaleDateString` / `toLocaleTimeString(locales?, options?)`
  and `new Intl.DateTimeFormat(locales?, options?)` with `.format(date)`. `format` is a closure field like the
  collator's and NumberFormat's; the prelude class is `Intl_DateTimeFormat`. The `Intl` prelude classes are now added
  one by one, so a program only gets the ones it mentions. Locales and the options literal are read at compile
  time (`options_spec`); options must be an object literal with literal values.
- **Options.** `dateStyle` / `timeStyle`, the components `weekday year month day hour minute second
  fractionalSecondDigits`, `hour12`, `hourCycle`, `timeZone` (an IANA name through jiff; the default is the
  program's zone) and `timeZoneName` (six widths). `era`, `dayPeriod`, `calendar` and `numberingSystem` raise a
  `RangeError`. Defaults follow ECMA-402 ToDateTimeOptions per entry point: `toLocaleString` date and time,
  `toLocaleDateString` date, `toLocaleTimeString` time, the constructor date. `dateStyle` / `timeStyle` exclude
  components, and a style the entry point does not allow is a `TypeError` with JavaScriptCore's text.
- **How.** The options map to the nearest ICU4X field set (`FieldSetBuilder`: date fields, length from the month or
  weekday width, time precision, zone style) and a `DateTimeFormatter<CompositeFieldSet>`. Styles use that
  formatter directly (ICU4X's patterns are exact). For component options the formatter's pattern is read and its
  numeric and text field widths are rewritten to the requested ones (`adjust_pattern`: `dd.MM.y` to `d.M.y`,
  2-digit, weekday and month name widths), then formatted through `FixedCalendarDateTimeNames` with the new
  pattern (Gregorian locales only). Whether a numeric month, day or hour is padded comes from `intl_pad.rs`,
  generated from the reference engine's ICU4C for 31 main-market locales (other locales keep ICU4X's padding).
  The table is the first piece of the dev-time `availableFormats` generator planned in the design note. U+202F is
  replaced by a plain space, as engines do.
- **Time zones.** ICU4X gets the real offset and instant, so an offset with no zone name prints `GMT+01:00`. That
  differs from `Date.toString`, which looks up the name of a recent day with the same daylight state.
- **`formatToParts`.** `dtf.formatToParts(date)` asks ICU4X for part annotations through a `PartsWrite` sink
  (`intl_parts.rs`). Types: `era`, `year`, `month`, `day`, `weekday`, `dayPeriod`, `hour`, `minute`, `second`,
  `timeZoneName` and `literal`; there is no `fractionalSecond` part. The result is `Intl.DateTimeFormatPart[]`
  (the same `Intl_Part` class).
- **`resolvedOptions`.** `dtf.resolvedOptions()` gives `Intl_Resolved` (`ts_rt_dtf_resolved_gc` in `intl_date.rs`):
  `locale` (the first requested tag, or the default), `calendar` (`gregory`), `numberingSystem` (`latn`) and
  `timeZone` (the canonical IANA name; the system zone when none was given, `UTC` for `utc`). The component fields
  (`year`, `hour`, `hour12`, `hourCycle`, `dateStyle`, ...) are not in the result.
- **Limits** (see [differences.md](differences.md), "Intl date formatting"): `formatRange`,
  `era`, `dayPeriod` and other calendars are not built; component combinations ICU4X cannot
  match differ from ICU4C. Exactness would come from `icu-full` or a generated table.
- Tests: `bench/datefmt.ts` (made by `bench/gen_datefmt_program.py`) with `tests/datefmt.rs` (M9-TS-277..286), five
  time zones and 15 locales, expected values from `bench/gen_datefmt_expected.ts` (bun) into
  `datefmt_expected.json`. Columns ICU4X cannot match are dropped from the test and listed in `differences.md`.

**Standard library tier 3: `Intl`, step I4: PluralRules, ListFormat, RelativeTimeFormat**
([lower/intl.rs](../tools/platypus_ts/src/lower/intl.rs), runtime in
[platypus_ts_rt/src/intl_misc.rs](../tools/platypus_ts_rt/src/intl_misc.rs); hooks in `lower/{expr,mod,prelude}.rs`).
- **Supported.** `new Intl.PluralRules(locales?, { type, minimumFractionDigits, maximumFractionDigits }).select(n)`
  (cardinal or ordinal; NaN and infinity are `"other"`), `new Intl.ListFormat(locales?, { style, type }).format(items)`
  (`items` is a `string[]`: arrays only, not iterables) and
  `new Intl.RelativeTimeFormat(locales?, { style, numeric }).format(value, unit)` (the unit is singular or plural,
  `"day"` or `"days"`). Bad option values, units and tags raise `RangeError` with bun's texts.
- **Classes.** The prelude classes are `Intl_PluralRules`, `Intl_ListFormat` and `Intl_RelativeTimeFormat`, each
  added only when the source mentions it. Like the collator's `compare`, the method is a closure field over the
  runtime's table id, so it can be detached (`const f = lf.format`) or passed as a callback.
- **Options.** Locales and the options literal are read at compile time (`options_spec`). `options_spec` now looks
  through `as`, `satisfies`, `!` and parentheses, so `{ type: "ordinal" as any }` is still a literal.
- **Runtime.** `ts_rt_plural_new` / `_select_gc`, `ts_rt_list_new` / `_format_gc`, `ts_rt_rtf_new` / `_format_gc`,
  on ICU4X's plural rules, list formatter and relative-time formatter.
- **Limits** (see [differences.md](differences.md)): `supportedLocalesOf`, `resolvedOptions`, `formatToParts`,
  `formatRange` and `selectRange` are not built (`Locale`, `DisplayNames` and `Segmenter` came later, below).
- Checked against bun in 17 to 20 locales (every result equal).
- Tests: `bench/intl4.ts` (made by `bench/gen_intl4_program.py`) with `tests/intl4.rs` (M9-TS-296..312), expected
  values from `bench/gen_intl4_expected.ts` (bun) into `intl4_expected.json`.

**Standard library tier 3: `Intl.Segmenter`, `Intl.Locale`, `Intl.DisplayNames`**
(runtime in [platypus_ts_rt/src/intl_segment.rs](../tools/platypus_ts_rt/src/intl_segment.rs),
[intl_locale.rs](../tools/platypus_ts_rt/src/intl_locale.rs) and [intl_names.rs](../tools/platypus_ts_rt/src/intl_names.rs);
prelude classes `INTL_SEGMENTER`, `INTL_LOCALE` and `INTL_DISPLAYNAMES` in `lower/prelude.rs`).
- **Segmenter.** `new Intl.Segmenter(locales?, { granularity })` with `grapheme`, `word` or `sentence` boundaries from
  ICU4X's segmenter. `segment(text)` returns an iterable of `{ segment, index, input, isWordLike }`, plus
  `containing(i)`. `isWordLike` is `false`, not `undefined`, for the non-word granularities.
- **Locale.** `new Intl.Locale(tag, options?)` gives the canonical `toString()`, `language`, `script`, `region`,
  `baseName`, and `calendar`, `collation`, `hourCycle`, `caseFirst`, `numeric` and `numberingSystem` (from the tag's
  `-u-` extension or the options), and `maximize()` / `minimize()`. `"und"` maximizes to `en-Latn-US` by a special
  case. Getters of absent parts give `null`, not `undefined`.
- **DisplayNames.** `new Intl.DisplayNames(locales, { type })` for `region`, `script` and `language` only, with
  `.of(code)`. CLDR names can differ from bun's (for example `zh-Hans`).
- **Options.** As for the other `Intl` classes: object literals with literal values, read at compile time; a
  `{} as X` cast is looked through.
- **Limits** (see [differences.md](differences.md)): `formatRange`, `resolvedOptions` and
  `Intl.supportedValuesOf` are not built; `DisplayNames` has no `currency`,
  `calendar` or `dateTimeField` type and no `fallback: "none"`.
- **`Intl.getCanonicalLocales`.** Takes a string or an array of strings, canonicalizes each tag (as `Intl.Locale`
  does), drops duplicates and throws a `RangeError` for an invalid tag (`INTL_CANON` in `lower/prelude.rs`).
- Tests: M9-TS-456..479; `getCanonicalLocales` M9-TS-516..536; `formatToParts` M9-TS-537..549; `resolvedOptions` M9-TS-550..555.

**Temporal, ISO calendar** ([lower/temporal.rs](../tools/platypus_ts/src/lower/temporal.rs), classes in
[lower/temporal.ts](../tools/platypus_ts/src/lower/temporal.ts), zones in [platypus_ts_rt/src/tz.rs](../tools/platypus_ts_rt/src/tz.rs)).
- **Rewrite.** `Temporal.X` becomes the prelude class `Temporal_X`, as a text rewrite of the program before parsing (a
  string literal containing `Temporal.Name` is rewritten too). The classes are plain TypeScript in `temporal.ts`,
  compiled in front of any program that mentions `Temporal.`.
- **Classes.** `Duration`, `PlainDate`, `PlainTime`, `PlainDateTime`, `Instant` (bigint nanoseconds),
  `ZonedDateTime` (nanoseconds plus a zone name) and `Now`. `tz.rs` supplies the canonical zone name, the offset at
  an instant, local time to instant with the `compatible` disambiguation, and fixed-offset zones such as `+05:30`.
- **`from`.** The lowerer routes `Temporal.X.from(v)`: a string goes to the parse function, an object literal builds
  the class from the literal and then validates and constrains it, any other object is copied. A `ZonedDateTime`
  literal needs `timeZone`. A string is also accepted where a Temporal object is expected
  (`date.equals("2024-01-01")`), and an object literal passed as a `Duration` or `PlainDate` parameter is typed as the
  class itself.
- **`PlainYearMonth`.** `add` / `subtract` take years and months only (a duration with units below months is a
  `RangeError`), `until` / `since` with `year` or `month`, `toPlainDate({ day })`, `with`, `compare`, `equals`.
- **`PlainMonthDay`.** Reference year 1972; `with`, `toPlainDate({ year })`, `equals`. There is no `month` getter,
  as in the engines.
- **`round`.** On `PlainTime`, `PlainDateTime`, `Instant` and `ZonedDateTime`: `smallestUnit`, `roundingMode`
  (all nine modes) and `roundingIncrement`; `round("minute")` string shorthand goes through
  `Temporal_RoundOptions`. `Duration.round` handles time units only, with `largestUnit` and no `relativeTo`.
  Rounding is not available in `until` / `since`.
- **Limits** (see [differences.md](differences.md)): no other calendars, `toLocaleString`, `total` with
  `relativeTo`, rounding in `until` / `since`, `overflow: "reject"`, or `disambiguation` / `offset` options.
- Checked against bun. Tests: `bench/temporal.ts` and `bench/zoned.ts` with `tests/temporal.rs` and `tests/zoned.rs`
  (M9-TS-480..515); `PlainYearMonth`, `PlainMonthDay` and `round` M9-TS-516..536.

**URI functions and error causes** ([platypus_ts_rt/src/uri.rs](../tools/platypus_ts_rt/src/uri.rs), hooks in
`lower/{strings,prelude}.rs`).
- **URI functions.** `encodeURI`, `encodeURIComponent`, `decodeURI` and `decodeURIComponent` (the non-component
  forms keep the reserved characters `; / ? : @ & = + $ , #`; `decodeURI` leaves escapes of them in place), and the
  legacy `escape` / `unescape`. They are runtime calls (`ts_rt_uri_encode_gc`, `ts_rt_uri_decode_gc`,
  `ts_rt_escape_gc`, `ts_rt_unescape_gc`) and may throw.
- **URIError.** Kind 4 of `__rtThrow`. A lone surrogate gives "String contained an illegal UTF-16 sequence."; a bad
  or truncated escape, an overlong, surrogate or too-large UTF-8 sequence gives "URI error", as JavaScriptCore does.
- **Error cause.** `new Error(message, { cause })` (every Error class takes the second argument; the
  `ErrorOptions` interface is in the prelude). The field is `cause: Error | null`, so a cause must be an `Error`
  (anything else is not supported). `undefined` and `null` are not told apart: an absent cause reads null, test it
  with `== null`.
- **AggregateError.** `new AggregateError(errors, message?, options?)` with `errors: Error[]` (a copy of the
  argument) and `name` `"AggregateError"`.
- `Error.stack` is the error's first line (`toString()`), without frames (`M9-TS-3507`). **Not built:** `Error.captureStackTrace`.
- Tests: `bench/uri.ts` with `tests/uri.rs` and `bench/errors2.ts` with `tests/errors2.rs` (M9-TS-287..295), expected
  values from `bench/gen_uri_expected.ts` and `bench/gen_errors2_expected.ts` (bun). `bench/add_module.py` scaffolds
  a new bun-compared test module.

**Typed arrays** ([lower/typed.rs](../tools/platypus_ts/src/lower/typed.rs), runtime in
[platypus_ts_rt/src/typed.rs](../tools/platypus_ts_rt/src/typed.rs), element access in `typed_load` / `typed_store` in
`codegen.rs`).
- **Types.** `Ty::Typed(TypedKind)` for `Int8Array`, `Uint8Array`, `Uint8ClampedArray`, `Int16Array`, `Uint16Array`,
  `Int32Array`, `Uint32Array`, `Float32Array` and `Float64Array`. The object is the array header with the `HDR_BYTES`
  tag: `len` elements, `cap` bytes, raw little-endian data. The compiler passes the kind code (the index in
  `TypedKind::ALL`) to the runtime functions.
- **Elements.** Reads and stores are inline code, loaded or stored by width and converted to or from a double. A store
  wraps integers as `ToInt8` .. `ToUint32`, rounds to `f32` for `Float32Array`, and clamps and rounds half to even for
  `Uint8ClampedArray`. NaN, infinities and values of 2^63 or more store 0 in the integer kinds.
- **Constructors.** `new X()`, `new X(length)`, `new X(number[])`, `new X(typed)` and `X.from(...)` with the same three
  sources.
- **Members.** `length`, `byteLength`; `fill`, `set(source, offset)`, `slice`, `reverse`, `sort` (numeric, with or
  without a comparator), `toSorted`, `toReversed`, `toString`; `for...of` and spread.
- **Everything else** (`indexOf`, `join`, `reduce`, `every`, `map`, `filter`, ...) runs on a `number[]` copy; `map` and
  `filter` convert the result back to the same typed kind.
- **GC.** A runtime function that allocates copies its source out first, because a temporary source is not rooted.
- Tests: `bench/typed.ts` with `tests/typed.rs` (M9-TS-313..359, 47 tests, bun-compared, run in normal, stress and
  GC-off modes), expected values from `bench/gen_typed_expected.ts` and `bench/gen_typed_program.py`.
- **Buffers and views.** `ArrayBuffer` and `DataView` are prelude classes
  ([lower/prelude.rs](../tools/platypus_ts/src/lower/prelude.rs)) over a `Uint8Array` that owns the bytes.
  `new Float32Array(buffer, byteOffset, length)`, `subarray`, `.buffer` and `.byteOffset` make views that share
  memory through the runtime header `HDR_VIEW`: a typed array whose `data` points into the owner's block and which
  keeps the owner alive. It has the same field layout as `Array`, so inline element access is unchanged.
  `.buffer` of an owning array returns a new `ArrayBuffer` object each time over the same memory (not `===`).
- **DataView.** `get` / `set` for `Int8` .. `Float64`, big-endian by default; out-of-range access is a `RangeError`
  (the messages differ from bun's).
- **Bytes across the engine boundary.** `ts_rt_typed_from_host_gc(kind, ptr, byte_len)` copies host bytes into a typed
  array (root it until it is handed over); `ts_rt_typed_data(a)` and `ts_rt_typed_byte_len(a, kind)` read it back
  (valid until the next allocation). Generated Rust bindings for exported functions with typed-array parameters are
  not done.
- Tests: `bench/buffers.ts` with `tests/buffers.rs` (M9-TS-431..441).
- **Not built:** see "Typed arrays" in [differences.md](differences.md).

**`structuredClone`** ([lower/clone.rs](../tools/platypus_ts/src/lower/clone.rs)). One lifted function per type, so a
recursive type gives a recursive function. Strings, numbers, booleans and bigints are shared; arrays, objects and
typed arrays are copied. A class instance keeps its class (JavaScript gives a plain object), and fields that only a
derived class declares stay shared. `Map`, `Set` and functions are compile errors; cyclic data does not end.
Tests: `bench/clone.ts` with `tests/clone.rs` (M9-TS-442..455, with `weak.rs`).

**`WeakRef<T>`** is generated by `prelude::weak_refs` for every class of the program and holds its target strongly,
so `deref()` always finds it. `WeakMap` and `WeakSet` are `Map` and `Set` (strong). There is no
`FinalizationRegistry` and no `globalThis`. Tests: `bench/weak.ts` with `tests/weak.rs`.

**BigInt** ([lower/bigint.rs](../tools/platypus_ts/src/lower/bigint.rs), runtime in
[platypus_ts_rt/src/bigint.rs](../tools/platypus_ts_rt/src/bigint.rs); the `num-bigint` crate; hooks in
`lower/{expr,arrays,types,prelude}.rs`).
- **Type.** `Ty::BigInt`, a leaf GC object (nothing to trace): header `HDR_BIGINT`, sign (-1, 0 or 1), byte count,
  then the magnitude bytes, little endian. Zero has no bytes; truthiness is "sign is not 0".
- **GC.** Every operation reads its operands into `num_bigint::BigInt` values first and allocates the result last
  (`read` / `alloc`), because a temporary operand is not rooted.
- **Literals.** `123n`, `0xFFn`, `0b101n`, `0o17n`, `1_000n` compile to a runtime parse of the literal's text
  (`ts_rt_bigint_literal_gc`).
- **Operators.** `+ - * / % **` (division truncates, the remainder takes the dividend's sign), `& | ^ ~ << >>` (two's
  complement semantics, `>>` rounds toward negative infinity), unary `-`, `++`/`--` and compound assignment. Binary
  operations are one call `ts_rt_bigint_op_gc(op, a, b)`. `>>>` and unary `+` are compile errors, as is mixing a
  bigint with a number in arithmetic (JavaScript throws a `TypeError` at run time).
- **Comparison.** `< <= > >= == != === !==` between bigints, and between a bigint and a number, exactly (no rounding
  through a double; NaN is never equal or ordered, infinities order beyond every bigint).
- **Conversions.** `BigInt(number | string | boolean | bigint)`, `Number(big)` (nearest double), `String(big)`,
  template strings, `big.toString(radix)`, `valueOf`, `BigInt.asIntN(bits, big)` / `asUintN`, `typeof` (`"bigint"`).
- **Errors** (JavaScriptCore's texts, raised like other runtime errors): `RangeError` "Not an integer",
  "0 is an invalid divisor value.", "Negative exponent is not allowed", "toString() radix argument must be between 2
  and 36", "Out of memory: BigInt generated from this operation is too big"; `SyntaxError` "Failed to parse String to
  BigInt".
- **Prelude.** The Error classes are compiled in when the source mentions `BigInt` or `bigint`, or has a literal like
  `10n`.
- **Related fix.** A `map` whose callback returns string literals (`v ? "1" : "0"`) now makes a `string[]`.
- Tests: `bench/bigint.ts` with `tests/bigint.rs` (M9-TS-360..368, bun-compared), expected values from
  `bench/gen_bigint_expected.ts`; M9-TS-369 is a unit test in `lib.rs`.
- **Not built:** see "BigInt" in [differences.md](differences.md).

**Inheritance.** `class B extends A` gives `B`'s struct the fields of `A` first (prefix layout), so a
`B` pointer is an `A` pointer and a subclass can be used wherever its base is expected. The compiler
sees the whole program, so a method call `a.m()` is resolved exactly: if one implementation is
reachable from the static class of `a` and its subclasses, it is a direct `Call`. Otherwise it is
`CallVirtual(table, args)`: codegen reads the struct id from the receiver's header and indexes
`Module::vtables[table]`, a local data table with one 8-byte function address per struct id, and
makes an indirect call. `super.m()` is always direct. `x instanceof C` is `InstanceOf(x, table)`:
null is false, otherwise the struct id indexes `Module::itables[table]`, one byte per struct id (1 for
`C` and its subclasses). A subclass must be declared after its base. Details and limits:
[platypus-ts-classes.md](research/platypus-ts-classes.md) section 14.

Not supported (each is a compile error naming the source line): decorators other than a bare `@property` on an instance field (no `@property` options, none on accessors), class expressions, extending engine classes other than `Component`,
throwing anything but an `Error` object, unsupported string methods (see above), object rest of unknown shapes, `for await`, async generators, `any`/`unknown`/`JsValue` and anything
dynamic. Generics (with the limits in "Generics" below) and destructuring assignment to existing variables are supported (`M9-TS-3513`, `M9-TS-3152`). `instanceof` works on the program's own classes only. Known
deviations from JavaScript (`null === null` of two nullable numbers, `==` as `===`, libm last-bit
differences) are listed in [platypus-ts-spike.md](research/platypus-ts-spike.md) sections 4 and 15.

## Utility types and structural typing ([lower/utility.rs](../tools/platypus_ts/src/lower/utility.rs))

- **`Partial<T>`, `Required<T>`, `Pick<T, K>`, `Omit<T, K>`, `Readonly<T>`** over interfaces, aliases, records, classes (data
  fields only) and engine classes (their settable properties, as `tsc` sees them: manifest setters, `setX` methods and the facade's
  `__xs_Class_x` functions). `Readonly<T>` is `T`. The others are ordinary structs (one per shape); an object literal builds one
  directly, a full value of another struct goes where it is expected by a **field-by-field copy** (`coerce`, `copy_fields`), so
  writes through the copy do not reach the original. A struct for a class or an engine class is filled in once the classes are laid
  out (`finish_utility_types`), so they may be named before the class is declared.
- **Absent fields.** An optional field that is not there is null. In `{ ...base, ...look }` a null optional field of a spread leaves
  what an earlier part of the literal set (JavaScript keeps absent keys too; an *explicit* `undefined` would overwrite there, here it
  cannot be told from absent).
- **Option bags for engine classes.** `addComponent(Class, { a: 1, ...look })` and `new GameObject(name, look)` with `look` a
  `Partial<LineRenderer2D>` (a literal with spreads, or a variable): every field that is there calls its setter (`apply_options_value`).
  The same for script components (`component_fields_from`).

## Conveniences the examples rely on

- **Functions as values** ([lower/fnvalue.rs](../tools/platypus_ts/src/lower/fnvalue.rs)): `const hex = Color.hex`, `xs.map(Math.abs)`,
  `apply(Util.twice, 3)`. A static method of a class of the program is its function; a static of the engine API (`Color.hex`,
  `Mathf.lerp`; parameters a call may leave out need a default) and a `Math` function become a closure over a small wrapper function
  (`wrapper_value`, shared per function). `Math.min/max/hypot` as values take two numbers.
- **Casts of empty literals.** `null as T | null`, `[] as T[]` and `{} as T` give the literal the asserted type (`{} as T` leaves
  its required fields zero: set them before reading).
- **Field types** from `new Float32Array(n)`, `new Array<T>(n)`, `new Map<K, V>()`, `new Int8Array(n).fill(-1)`, `NaN`,
  `-Infinity`, `Math.PI`, `Math.sqrt(..)`, `Date.now()`, `LIST.map(() => new Turn())`, `new Other()`, `[] as T[]` and `null as T | null` (also inside an object literal).
- **Assignment targets** may sit under `!` and casts: `x[i]! += d`, `m.get(k)![0]! *= 3`, `(x[i] as number) -= 1`.
- **Strings iterate by code point**: `[...s]`, `Array.from(s)`, `Array.from(s, f)`, `for (const c of s)` (`ts_rt_str_chars_gc`).
- **`Math.hypot(...xs)`** with any number of arguments (folded two at a time).
- **Optional calls.** `o.cb?.(x)` on a function that may be missing does nothing when it is (`call_closure_value`). So does
  `f?.(x)` where `f` is a plain identifier (a local, parameter or captured variable): nothing is called when `f` is missing.
- **Computed style keys** in JSX: `style={{ [props.side]: 8 }}` (the key is a string or a union member).
- **Conditional branches.** `ok ? [a, b] : []` (the empty array takes the other branch's type); an engine object's options can be
  `cond ? { position, parent } : { position }` (each branch sets its own properties).
- **Types** (`M9-TS-3361..3383`): intersections (`A & B`); recursive object-shaped aliases (a placeholder struct while the alias resolves:
  `alias_resolving` / `alias_hits`); discriminated unions whose variants give one field different types (the field is a tagged union,
  `Disc` in `lower/mod.rs`; tag tests and `switch` cases narrow it, an object literal picks the member by its tag); `s ?? 1` as a
  `string | number` union with `Number()` / `String()` over unions (`union_map` in `tagged.rs`); negative numeric literal types
  (`-1 | 1`); literal unions from or to a missing string; `RegExpExecArray`; a copy to `T | null` keeps `null`; a generic `F` is
  inferred from a closure argument (`id<F>(f: F)`); closures infer their result (`ret_inferred`) and default-parameter types from the
  default; a recursive function with no value return is `void`, a recursive component without a return type is `UiElement`.
- **Expressions and statements.** `filter(Boolean)` / `Boolean(x)`, `x || null`, `new Set(set)` / `new Map(map)`, `find` over literal arrays,
  `re.exec(s)?.[0]`, `clearTimeout(undefined)`; nested function declarations are hoisted (`stmt.rs`). The text-level passes (window
  message glue, Temporal names, `WeakRef` classes, native aliases) skip string literals and comments (`lower/literals.rs`).
- **JSX.** `<Fragment key>`; a component may return `ReactNode` (text, a list or `null` are coerced, `react_node_of` in `types.rs`);
  `memo(function Name(..) {..})`.

## Interfaces implemented by classes ([lower/ifaces.rs](../tools/platypus_ts/src/lower/ifaces.rs))

`interface Shape { area(): number }`, `class Circle implements Shape`, a `Shape[]` or a `Shape` parameter holding circles and squares,
`shape.area()` (and `interface Animal extends Named`, which works for every interface now: the extended members come first).

- **Which interfaces.** An interface named in a class's `implements` (and the ones it extends) is *dynamic*; other interfaces stay
  plain structs. A value of a dynamic interface is a **pointer to the object itself** (a class instance or a plain object): no copy and
  no wrapper, so `shape instanceof Circle`, identity and writes through the interface behave as in JavaScript. Every object starts with
  the id of its struct, so a member access goes through a table indexed by that id (`CallVirtual`, the mechanism classes use for
  overridden methods), one table per (interface, member, kind): **call** (`shape.area(..)`: the class method itself, or a wrapper that
  calls an arrow-function field), **get** (`shape.name`, a method or optional method read as a value: the field read, the accessor, a
  bound method, or null), **set** (`shape.level = v`: the field store or the setter).
- **What goes in a table.** A struct counts once a value of it is assigned to the interface type (an assignment, an argument, an
  array element, a return: `assignable`/`coerce` check that it has the required members and record it), with its subclasses and the
  structs of the interfaces that extend it. The tables are filled after the whole program is lowered (`finish_ifaces`, repeated while
  that lowers more code). An object literal expected to be the interface gets a struct of its own.
- **Defaults.** A class method with default parameters called through an interface that makes the argument optional gets a wrapper that
  fills the default in.
- **Not supported.** Spreading an interface value, `JSON.stringify` of it and the like are a compile error naming the interface
  (`check_iface_structs`); `Object.keys(x)` gives the interface's keys. A class used as the value of an interface that no class names
  in `implements` is a "type mismatch" (name the interface in `implements`: that is what makes it dynamic). Generic interfaces
  (`implements Comparable<T>`) are not dynamic.

## Tuples, destructuring assignment and top-level `await`

- **Tuples.** A tuple of one element type is an array; a tuple of mixed types is an object with a field per element (`Tuple`, fields
  `"0"`, `"1"`, ..). `t.length` is the number of elements, `t[0] = x` and `t[1] += y` write the field (the index is a number written in
  the source), `const [a, ...rest] = t` binds the rest as a tuple (or an array when the remaining elements agree), `[...t, x]` and
  `f(...t)` read the elements one by one (into fixed parameters; not into a rest parameter). Tuple types in rest parameters
  (`...args: [number, string]`) are not built.
- **Destructuring assignment** (`lower_destructure_assign`): `[a, b] = [b, a]`, `[xs[i], xs[j]] = [xs[j], xs[i]]`, `[n, s] = pair()`,
  `({ x, y } = o)`, defaults, rest, nested patterns, element and member targets. An array literal on the right with an element per
  target is not built: its elements go into temporaries first (so a swap reads both before writing), then are assigned in order.
- **Top-level destructuring declarations** (`const [a, b] = ..`, `let { x, y } = ..`) make every name a module variable.
- **`await Promise.all([a, b])` over different value types** is a tuple: when the operand of `await` is `Promise.all` of an array
  literal whose promises (or plain values) have different types, the elements are evaluated (every promise starts), then awaited one
  after the other into a tuple (`await_all_tuple`). Not the same as JavaScript only in this: a promise that rejects while an earlier
  one is pending is reported as unhandled until its turn comes. `Promise.all` of mixed promises that is not awaited on the spot is
  still a compile error (no promise class stands for a tuple); `Promise<Promise<T>>` too.
- **Top-level `await`** (the entry module and the modules it imports) follows ECMAScript's module evaluation order. The module graph
  is still one program (`modules.rs` bundles it, dependencies first), but when some module awaits at its top level every file starts
  with a marker `__platypus_module(position, "path", own await, [late dependencies])` (written on the `// ---- path` line, so line numbers
  hold) and the lowering splits the top-level code into one segment per module (`lower_async_module` in `lower/mod.rs`). A module that
  has a top-level `await`, or imports a module that finishes late, runs as an async closure of its own (`tla$module<n>`, run by
  `__async$number`, started where the module's text begins); its promise is a module variable. Such a closure first awaits the promises
  of the late modules it imports, in import order, and then runs its statements. The other modules run in place, so a sibling that
  does not import the pending module runs while it waits (`a, b1, c, b2, main`, as in bun, `M9-TS-3202..3207`). A program with one
  file and a top-level `await` is a single module called `main`. Module variables are still globals, so functions declared anywhere
  see them as they are set. The code before a module's first `await` runs inside `ts_init`; the rest continues when the awaited
  promise settles, i.e. when the host drains microtasks and timers (`ts_rt_run_loop`).
  **Startup state.** Each late module reports to the runtime (`__modWatch`, the prelude in `asyncs.rs`; `startup.rs` in `platypus_ts_rt`):
  `startup_state()` is `Done`, `Pending(modules still evaluating, in start order)` or `Failed { module, message }`. A rejected
  top-level `await` fails the module and every module that imports it; the first failure is the one kept and is logged once through the
  console as an unhandled promise rejection that names the module (the host adds its own message, see "Components for the host"). **Not the same as ECMAScript:** an import cycle through
  a late module treats the cyclic edge as already finished; an importer waits for its late dependencies in import order, not all at
  once (the result is the same unless two of them reject).
- **Declared types.** `gen/components.rs` marks the handle-valued properties of generated components (`Texture`, `Mesh`, `Material`,
  `Sprite`, `GameObject`) `#[nullable]` (`Texture | null`, as in the QuickJS declarations) and the name-valued ones (`Light.type`,
  `LineRenderer2D.fillRule`, `TextMesh.alignment`, ..) `#[ts_type]`; `QualitySettings.shadows`, `softShadowQuality` and
  `anisotropicFiltering` too (`docs/platypus-api.md`). Spreading into function values / `new C(...xs)` / `f(a, ...xs)` is built:
  the elements of an array or tuple feed the parameters after the plain arguments (null past the end).
- **`return f()` of a void call** (a generic function with `T = void`) evaluates the call and returns nothing.
- **Defaulted closure parameters of type `boolean`** may be left out at a call, like numbers and strings.

## Tables, dictionaries and types from values

- **An object indexed by a key.** `state[key]` (read, write, `+=`, `++`) where `key` is a string-literal union member or a `string`
  names the field: a chain of comparisons over the fields, `Place::Keyed`. A key that is a *parameter written as a union of some
  names* (`key: "fog" | "lowRes"`) can only name those fields (`FnCtx::lit_sets`); otherwise all fields of the object are named. When the
  named fields have different types a read has the union of their types (`o[k]` is `T["a" | "b"]`, `M9-TS-3528`); writing through
  such a key is an error with the line (TypeScript wants the intersection; not built, T8c). Tables by tag (`TINTS[tint]`,
  `NAMES[k]`) are the same.
- **Indexed access types** (`lower/types_index.rs`, `M9-TS-3525..3530`). `T["k"]` resolves for interfaces, aliases of type literals
  and classes; `T["a" | "b"]` and `T[keyof T]` are the union of the fields' types; `A["b"]["c"]` nests; `typeof x["k"]` and
  `typeof a.b.c` work. A tuple takes `T[0]`, `T[number]` (the union of its element types) and `T["length"]`. `PlatypusStyle["prop"]`
  reads the declared type of the property from the prelude's `__PlatypusStyleProps` (keywords, `Length` = `StyleValue`, numbers,
  `Cursor`), and `keyof PlatypusStyle` is the property names. In a generic function, method or alias `T[K]` follows the key
  literals the call gives: the type environment (`TypeEnv::keys`) keeps them and `get(o, "x")` is a copy per set of literals,
  returning `T["x"]` (`inst_name_keyed`). Errors name the line: a missing property, a tuple position past the end, a key parameter
  whose argument is a plain `string`, a write through a key naming differently typed fields. Not built: T8b..T8e in
  `tools/platypus_ts/TODO.md` (keys of a generic class's own type parameters, ordered unions, comparing a `StyleValue`).
- **`Record<string, V>`** is a `Map<string, V>`: an object literal builds the map (`{ [k]: v }` too), `dict[k]` and `dict.name` read the
  entry (`undefined` when missing: the value type must be a number, string, object or union member, as for `Map.get`), `dict[k] = v`,
  `dict[k] += n` and `dict.name = v` write it (`Place::MapEntry`). A record struct (`Record<"a" | "b", V>`, an object literal of one
  type) goes where a `Record<string, V>` is expected (assignment, `as Record<string, V>`) as a copy.
- **Types written from values.** `(typeof NAMES)[number]` with `NAMES = ["a", "b"] as const` is the union (and the keys of
  `Record<Sfx, V>`); `typeof TABLE` is the type of a top-level constant whose initialiser's type can be found from its syntax;
  `(typeof TINTS)[Tint]` is the type its named fields share; `Record<Tint, V>` takes `keyof typeof TINTS` (also through an alias) as
  its keys. `object` is a style bag (`PlatypusStyle`) when the program has the UI prelude. Optional tuple elements
  (`[number, number, "c"?]`) are nullable fields, left out in a literal that is shorter. `s[i]` on a string is the character or
  `undefined`. An array of nothing but `null` (`Array.from({ length }, () => null)`) goes where `(T | null)[]` is expected.
- A `Pick`/`Partial` of an interface that is still being laid out (reached again through its own members) is filled in at the end.
- **Entries of objects and dictionaries** ([lower/dicts.rs](../tools/platypus_ts/src/lower/dicts.rs), `object.rs`): `Object.keys` /
  `values` / `entries` (as values, arrays of `[key, value]` pairs) and `for (const k in o)` over a struct (its fields) or a
  `Record<string, V>`; `Object.fromEntries(pairs)` is a dictionary; `key in o` with a literal key (decided by the type) or a key
  known only at run time; `delete o.k` / `delete o[k]` (a dictionary loses the entry; a struct can lose an *optional* property,
  which becomes undefined). An optional property (`Partial<Record<Tag, V>>`, `x?: T`) is there only while it is not null, for
  `keys`, `values`, `entries`, `in` and `for ... in` (as for `JSON.stringify`). `for (const i in xs)` over an array gives the
  indices as strings (`"0"`, `"1"`, ..); `Object.assign(dict, a, b)` sets the properties of objects and the entries of other
  dictionaries into a `Record<string, V>` (`{ [key: string]: V }` is the same type; `M9-TS-3216/3217`); `Object.assign(obj, ..)`
  into an object copies the properties of object sources and the entries of dictionary sources (several sources, later ones win):
  a dictionary's keys are matched against the struct's field names at run time (`assign_map_into_struct`, a chain of key
  comparisons per call site), a value that does not fit the field is a `TypeError` naming the key and the struct, and a key that
  is no field cannot be added to a fixed layout, so a dev warning (`console.warn`, once per call site: `warn_once`, a global
  flag) names the key and the struct and the entry is skipped (JavaScript would add the property). The same warning replaces the
  old compile error for a source property the target lacks. `{ ...dict }` into an object of a declared type does the same by
  key; with no object type expected it is a new dictionary (`{ ...dict, z: 1 }`, also several spreads and object spreads into a
  `Record<string, V>`). `M9-TS-3319..3322`. Not built: `Object.hasOwn(dictionary, k)`.
- **`as const` arrays.** `["a", "b"] as const` of string literals is an array of union members (`Lit[]`), so its elements go
  where the union is expected (`(typeof NAMES)[number]`); the engine's own enums are strings, and a string reaches a union member
  through the tag table (`StrTag`), so the two meet either way: a generic parameter bound to both a union and a string is the
  string, and an array of members goes where an array of strings is expected as a converted copy (`next(MODES, engineString)`,
  `M9-TS-3196`). Other `as const` arrays are arrays or tuples as before.
- **`flat(depth)`.** A literal depth, `Infinity`, or a constant is exact (the declared nesting gives the result type). A depth
  known only at run time flattens exactly that many levels, as JavaScript (0, negative or `NaN` copy, a fraction truncates,
  `Infinity` flattens fully): the result is an array of the union of every level the declared nesting allows
  (`T | T[] | T[][]` for `T[][][]`, `flat_dynamic` in `arrays.rs`). Every element of the receiver has the same nesting, so after
  `i` flattenings the array holds one type; the run-time depth only picks which step's array is wrapped into the union. Narrow
  the elements with `typeof` / `Array.isArray`; `.length` of a union of arrays reads the member the tag names
  (`union_length`); `xs.flat(depth) as number[]` copies the elements of that member and throws a `TypeError` when the depth did
  not reach them (bun returns the nested arrays under the wrong type); a local `const d = 2` counts as a constant. Indexing such
  a union is not supported; `JSON.stringify` writes the member it holds (`M9-TS-3506`; `JSON.parse` into a union is an error). `M9-TS-3224`, `3313..3316`.
- **A tuple as a rest parameter.** `function f(...args: [string, number?])`: the arguments from there on make the tuple (a missing
  optional element is undefined); `f(...pair)` with a tuple of that type passes it as it is. Not for function values.
- **Facade chunks need the thing they are written for.** A `//# when a|b&c` header triggers on `a`, or on `b` and `c` both; the
  window helpers (`.on(`, `.emit(`, `.request(`, `broadcast`, ..) need the program to name `Window`, so a class with its own `on` or
  `emit` method does not pull them in (`M9-TS-3197`).
- **`register(Class)`** is an ordinary run-time call (`__registerClass`): it records the class when it runs, so it works inside `if`,
  loops and functions and after an `await`; the order of the registrations is the order of the calls. The argument must name a class
  (`register(Player)`, `register({ A, B })`): a class is not a value in a compiled program, so `register(cls)` of a variable is a compile
  error. `{ spawn: false }` is read at run time. The host starts the registered classes (and loads `platypus.startScene`) only after every
  module has finished evaluating.

## The original example sources ([lower/tagged.rs](../tools/platypus_ts/src/lower/tagged.rs), [lower/accessors.rs](../tools/platypus_ts/src/lower/accessors.rs))

What the QuickJS-era sources of the examples (`git show main:examples/<name>/src/..`) needed, tested by
`tools/platypus_ts_tests/tests/lang/originals.rs` (against bun, `M9-TS-3174..3183`) and `originals_engine.rs` (`M9-TS-3184..3189`):

- **Generic inference.** A closure that returns nothing fixes `T = void` (`under(root, () => this.build())`). A written return type
  on an inlined array callback is the callback's result type (`rows.map(([a, b]): Star => ({ .. }))` gives `Star[]`). A string
  literal argument is a `string`; a value that has a union type keeps it (`pick(star.poses)` is `PoseName`).
- **Utility types** (`utility.rs`): `ReturnType<F>` and `Parameters<F>` over `typeof f`, `typeof f<string>` (an instantiation
  expression: the generic function is instantiated and, when its result is inferred, lowered at once, with the classes it needs
  collected and laid out on the spot: `eager_classes`) or a function type; `Awaited<T>`, `NonNullable<T>`, `Exclude<T, U>` and
  `Extract<T, U>` over unions of string literals (or `null`). `typeof local` is the type of a local. Top-level functions are noted
  before anything is collected (`register_top_functions`), so a type met early may name them.
- **Unions of different types** (`tagged.rs`): `number | [number, number]`, `string | string[] | number`, .. A struct with a tag and a
  slot per member (`unions`). A member goes where the union is expected (`coerce` wraps it), a union goes where a member is
  expected (it is projected), `typeof u` and `Array.isArray(u)` read the tag. Narrowing is by path: a test of the form
  `typeof P === "number"`, `Array.isArray(P)`, `!`, `&&`, `||`, written in an `if`, a `?:` or the left of `&&` / `||`, narrows the
  path `P` (`o.scale`) to the members that pass in the branch it guards (and, after an `if` that always leaves, in the rest of the
  block); reading a path that is narrowed to one member gives that member (`narrowed`). Also narrowing: `"name" in P` (the
  members that have the property; an optional one may be missing), `P instanceof C` (the members that are `C` or a subclass), and a
  truthiness test of `P`: when it is false the value is one of the members that can be falsy (number, string, boolean, bigint),
  never an object, array or function. `"name" in u` and `u instanceof C` on a union read its tag; `if (u)` and `!u` look at the
  member it holds (0, `NaN`, `""` and null are false). A literal that goes where a union of several object types is expected is
  the member whose properties it has. **Narrowing inside a closure** (`lower_closure`): the narrowing of a plain variable that
  the closure names carries over when nothing assigns that variable anywhere in the enclosing function (`Scan::assigned`):
  a `const`, a parameter or a `let` that is never reassigned, as in TypeScript; a path through a property (`o.scale`) does not
  survive (TypeScript drops those too). Callbacks of `map` / `filter` / `forEach` / `some` / `every` / `find` are inlined and keep
  every narrowing. `M9-TS-3219..3223`.
- **Strings and literal unions.** A text from outside (an engine property, a map's keys) goes where a union member is expected through
  the tag table (`StrTag`, `ts_rt_str_tag`; text with no tag gives "no tag"). A value of a union type keeps it in an object or array
  literal (`{ mode: MODES[0]! }`); only a string written there widens to `string` (`is_fresh_literal`).
- **Objects.** A constant object whose methods name the constant (`const sfx = { play() { sfx.preload(); } }`) gets its type from
  the methods' written signatures first (`object_shape`). A getter in an object literal (`get scene() { return self.effects; }`)
  is a hidden closure field `_p_get_<name>` in every plain struct that has a field of that name; a read calls it when it is set
  (`accessors.rs`). Methods, getters and setters of an object literal may use `this`: it is the literal's object (a cell filled once
  the object exists, captured by those closures as a class method captures its own; `lower_this_prop` in `expr.rs`). A literal
  with such a method gets a struct of its own whose method fields are patched with the types the bodies give; one whose type is
  declared (an interface) keeps that struct. A method taken off its object (`const f = o.m; f()`) keeps its binding (JavaScript
  has `this` undefined there). `M9-TS-3317/3318`. `new (a.constructor as new (n: number) => T)(len)` makes another typed array (or an object of a class of the
  program) like `a`. A generic arrow declared in a function (`const more = <T extends Float32Array>(a: T): T => ..`) is a generic
  function of its own (it cannot use the locals around it). `Record<Union, V>` with more than 48 keys is a dictionary
  (`Map<string, V>`: a struct holds 64 fields), and `Partial<Record<..>>` of a dictionary is the dictionary.
- **Expressions.** `s && obj` (a string, number or boolean and an object) is the object or null; `collection.get(k) ?? []` over a
  set, map or typed array is an empty one; `cond ? a : b` with a condition known at compile time (`"uv" in data`) lowers only the
  arm taken; `new Color(...rgba)` reads the fields from an array or tuple (a missing alpha is 1); `{ ...base, opacity }` in a
  style takes an entry per field of a plain object.
- **Engine API.** `LineRenderer2D.points` and `LineRenderer.positions` read as a property (a copy; write a new array to change
  them). `playback.onEnded = f` is the call `playback.onEnded(f)` (any method whose one parameter is a closure). `Map` values may
  be handles (`map.get(k)` is the handle or null; handles are never 0). `pop` / `shift` / `at` over handle arrays. `Mesh.createFlat`
  takes an object with only some of its lists (the others are empty). `key={s}` with a string that may be undefined leaves the key out.

## Symbol, iterators and generators

- **Well-known symbols.** `[Symbol.iterator]()` in a class is a method with the internal name `@@iterator`
  (`symbol_key` maps the computed key, `well_known` recognises `Symbol.<name>`). `obj[Symbol.iterator]()` is
  an ordinary call of it.
- **Symbol values** (prelude class `Symbol_`, [lower/prelude.rs](../tools/platypus_ts/src/lower/prelude.rs)). `Symbol(desc)`
  makes an object with identity; `description` (null, not `undefined`, when none), `toString()`,
  `Symbol.for(key)` / `Symbol.keyFor(sym)` (null for an unregistered symbol), `typeof sym === "symbol"` and
  the `symbol` type work. Symbols are not usable as property or Map keys, and the other well-known symbols
  are not available as values.
- **Iterator protocol for classes.** `for...of`, array spread and `Array.from` over a value whose class has
  `@@iterator` call it, then `next()` until `done` (an `IteratorResult` object of the program's own types).
  Arrays, strings, `Map`, `Set` and typed arrays keep their loops.
- **Coroutines** ([platypus_ts_rt/src/coro.rs](../tools/platypus_ts_rt/src/coro.rs)). A coroutine runs a `() => void`
  closure on its own native stack (corosensei, 256 KB). The shadow stack (the linked list of frames living in
  each function's native frame) needs no change, because the GC roots are not on the native stack: the
  coroutine's first frame is relinked under the resumer's frame on every resume; when the coroutine object
  (header `HDR_CORO`) is marked, a suspended coroutine's frames are scanned by walking `saved_top` down to
  `bottom`; a coroutine swept while suspended gets `force_reset` (no unwinding: compiled frames have no
  destructors). Not available on WebAssembly (not a target in the next year).
- **Generators** ([lower/generators.rs](../tools/platypus_ts/src/lower/generators.rs)) are a source rewrite
  (`generators::desugar`, before parsing). `function*` and generator methods become normal functions whose body
  is `return __genNew$K(() => { ... })`; every `yield x` becomes `__yield$K(x)`, and `yield* e;` (statement)
  a `for...of` loop of yields. For each yield type `K` the program mentions, the classes `Generator$K`,
  `Iterator$K`, `IteratorResult$K` and helper functions are generated as TypeScript text and put in front of
  the program. The annotations `Generator<T,R,N>`, `IterableIterator<T>`, `Iterable<T>`, `Iterator<T>` and
  `IteratorResult<T>` map to them. The intrinsics `__coNew`, `__coResume`, `__coYield<N>`, `__coValue<T>`,
  `__coError` and `__coFinish` are lowered in `generators.rs`. Closures, captures, `this`, `try/finally` and
  checked-return exceptions are the ordinary machinery: an exception thrown in the body is left pending by
  the coroutine and rethrown by `next()`. Struct names with `$` are made Rust-safe in the generated bindings.
- **`throw`, `return`, return values.** `generator.throw(e)` raises `e` at the suspended `yield`.
  `generator.return(v)` raises a marker error (`__GenReturn$K`) there, so `finally` blocks run; `catch`
  clauses inside generator bodies are rewritten to rethrow the marker. `return value` works for
  `Generator<T, T>` (the return type must equal the yield type, or be void); the `value` of a finished
  generator without one is the zero value.
- **Limits:** the return type must be written (untyped generators are an error); `yield*` only as a
  statement (not as a value); a `yield` inside another `yield`'s operand is an error;
  `IteratorResult.value` is the zero value (0, null) instead of `undefined` when done; `Iterable<T>`
  annotations accept generators only. See [differences.md](differences.md).
- Tests: `M9-TS-376..393` (`bench/generators.ts` with `tests/generators.rs`, bun-compared; the Rust coroutine
  test is `M9-TS-381`); `M9-TS-394..398` symbols, `M9-TS-399..407` `throw` / `return` / return values
  (`bench/symbols.ts`, `bench/generators2.ts`).

## Promise, async/await and timers

- **`Promise<T>`** is a prelude class `Promise$K` per value type text (`K` is the mangled type), written as
  TypeScript text in front of the program together with helper functions (`__fulfill$K`, `__then$K$V`,
  ...) by [lower/asyncs.rs](../tools/platypus_ts/src/lower/asyncs.rs). `void` shares `number`'s class;
  `number`, `string` and `boolean` always exist; `T[]` too when `Promise.all` appears. A `Promise<T>` needs
  `T` written somewhere in the program, and awaiting needs the value's `Promise<T>` class to exist.
- **Lazy helpers.** The per-type helpers (`__then$K$V`, `__fulfill$K`, ...) are lowered only when something calls them
  (`is_lazy_helper` in `lower/mod.rs`), and the program's own types are not paired with each other for `then`. A
  generic promise function makes a class per program type; lowering a `then` for every pair (and counting the helpers as
  always reachable) took the editor from 1 MB of object code to 57 MB and from 0.5 s to 70 s (`M9-TS-3523`).
- **Generic promises.** `async function load<T>(x: T): Promise<T>`, `new Promise<T>(..)`, `Promise<T[]>` and
  a `class Box<T>` holding a `Promise<T>` work: the rewrite emits `__asyncG<T>(..)` / `__newPromiseG<T>(..)`
  (and annotates the coroutine's `__p: Promise<T>`) for a type that mentions a type parameter, and the
  lowerer resolves the class per specialisation (`generic_promise_key`: the promise class whose value type
  is the specialised `T`). `T` is inferred from `Promise<T>` parameters (`unify_ann`) and from closures
  that return promises. A promise class must exist for the specialised type: once a generic promise is
  written, every non-generic class and interface the program declares, `number[]`, `string[]` and
  `boolean[]` get one; other types (a type alias, `Map<..>`, an array of aliases) need a
  `Promise<that type>` written once, otherwise the error says `the program never writes Promise<..>`.
  Generic *methods* are still not built (a method of a generic class is fine).
- **`async` rewrite** (`asyncs::desugar`, after the generator rewrite and before parsing). An `async`
  function, method or arrow becomes `__async$K((__p) => { ... })`, a coroutine closure; `return x` becomes
  `__asyncRet$K(__p, x)`, `await e` becomes `__await(e)` (the coroutine suspends and a reaction on the
  promise resumes it), and `new Promise<T>(f)` becomes `__newPromise$K(f)`.
- **Routing** ([lower/promise.rs](../tools/platypus_ts/src/lower/promise.rs)). `__await`, `__asyncRet`,
  `.then(f)`, `.catch(f)`, `.finally(f)`, `Promise.resolve`, `Promise.reject`, `Promise.all` and
  `Promise.race` are typed calls into the generated helpers. The callback's return type picks the helper:
  `__then$K$V` for a value of type `V`, `__thenP` when it returns a promise (flattened), `__thenV` for void.
- **Microtasks and timers** ([platypus_ts_rt/src/event.rs](../tools/platypus_ts_rt/src/event.rs)). A FIFO microtask
  queue (`queueMicrotask`, promise reactions) and timers (`setTimeout`, `setInterval`, `clearTimeout`,
  `clearInterval`); no thread and no OS event loop. Timers run on virtual time and are at least 1 ms, like
  browsers. Queued closures and timers are GC roots. `ts_rt_after_microtasks(f)` runs `f` once the queue
  has drained (the rejection check). `setTimeout(callback, ms)` takes `(arg?: number) => void`, so
  `new Promise<void>((resolve) => setTimeout(resolve, 100))` works.
- **Unhandled rejections.** A promise that rejects with no reaction is tracked (`__Rej` in the
  prelude); a reaction attached before the microtask queue drains marks it handled; the rest are reported
  once, after the drain, with `console.error("Unhandled promise rejection: <name>: <message> (..)")`
  (level 4: `platypus_app` logs it as an error). The program goes on. A `then` chain reports its last promise
  only. There is no stack (Error has none yet). An exception escaping a `setTimeout` / `setInterval` /
  `queueMicrotask` callback is reported the same way (`Uncaught exception in a timer or microtask: ..`) and
  the other jobs still run.
- **Hot reload and destroyed objects.** A reload drops every pending microtask and timer (the old code's
  awaits never continue; `ts_rt_events_clear`). An await in a component whose object was destroyed
  continues normally (JavaScript semantics, there is no cancellation): the component and what it holds stay
  alive until the coroutine ends, and engine calls on the dead object are no-ops or defaults (checked under
  GC stress, M9-API-1514). After an await check `this.gameObject!.exists()` (the handle itself stays), or keep a flag that
  `onDestroy` clears.
- **Engine promises.** `SceneManager.loadSceneAsync(path, mode)` and `Prefab.load(path)` resolve in the tick
  that read the file and reject with an `Error` (missing file, not a scene, path outside the game folder);
  `Texture.whenLoaded()` / `Model.whenLoaded()` resolve when loaded and reject when loading failed;
  `Task.delay` / `delaySeconds` / `frame`; `Window.request` / `ask`; `Light.measureIlluminance`. The
  native forms (`loadSceneAsync$n`, `requestJson$n`, `askJson$n`, ..: `#[promise]` functions with resolve and reject callbacks) are what the
  compiler's promise functions are written over.
- **Window handlers.** `answer(name, async (..) => ..)` and an `async` raw `onRequest` handler are
  allowed: the compiler finds the handler's promise type (`async_variant` in `facade.rs`) and uses the
  `..P` function (`lower/messages.rs`), which replies when the promise resolves and fails the asker's promise with the error's
  message (`Window.replyError`) when it rejects.
- **Host contract.** After every call into script the host calls `ts_rt_microtasks_run()`; once per frame it
  calls `ts_rt_timers_advance(now_ms)` with the game clock (a paused game pauses its timers);
  `ts_rt_run_loop()` runs both until nothing is left (tests and tools). `ts_rt_events_pending()` reports
  what is queued. Wiring this into `platypus_app` is not done.
- **Tick order equals bun's:** returning a promise from `then` costs 3 ticks, `await` 1, and `finally`
  settles 2 jobs later; the tests compare the order of logged events with bun.
- **Also changed.** A function type `(x?: T) => void` makes the parameter nullable and a call may leave it
  out; a closure may take fewer parameters than its expected type. `PLATYPUS_TS_DUMP_SRC=1` prints the program
  after the rewrites; `PLATYPUS_TS_TRACE=1` prints function names during codegen; parse errors give the line
  (counting the prelude).
- **Limits:** `then(f, g)` needs both callbacks to return the same kind of result; `Promise.allSettled` results are `PromiseSettledResult<T>` objects with all of `status`/`value`/`reason`;
  `for await` works over arrays and other sync iterables (each element is awaited; it is rewritten to a
  `for..of` plus `await`); async generators, `Symbol.asyncIterator` and `AsyncGenerator` are compile errors;
  top-level `await` works (see its section); `Promise.reject(e)` is
  `Promise<number>` unless written `Promise.reject<T>(e)`; `resolve()` with no value only for
  `Promise<void>` / `Promise<number>`; `resolve(v)` takes a value, not a promise; `setTimeout` takes no
  extra arguments; `Promise<Promise<T>>` is a compile error (write `Promise<T>`); `Promise.all([a, b])`
  over promises of different value types (a tuple result) is a compile error (await each one);
  `Promise.withResolvers<T>()` needs its type written and is not available inside generic code;
  `AbortController` / `AbortSignal` do not exist. See [differences.md](differences.md).
- **Callback literals.** A callback that returns a string literal is typed `string` (inferred callback return types
  widen literals).
- Tests: `M9-TS-408..427` (`bench/promises.ts` with `tests/promises.rs`, bun-compared; the source is
  generated from `bench/promises_src.ts` by `gen_promises_program.py`); `M9-TS-2897..2919`
  (`bench/promises2_cases.ts` with `tests/promises2.rs`: each case is compiled in the process by the JIT, run
  in the three GC modes and compared with `promises2_expected.json`, made by `bun
  bench/gen_promises2_expected.ts`); engine promises `M9-API-1512..1517` (`crates/platypus_app/src/native.rs`).

## Runtime and GC

[platypus_ts_rt/src/lib.rs](../tools/platypus_ts_rt/src/lib.rs) is an ordinary Rust crate the generated code
calls into. Its header comment is the long version.

- **Heap.** One heap per thread. Blocks up to 512 bytes come from 1 MiB chunks with size-class free
  lists; larger ones use the system allocator. Nothing moves, so a pointer Rust holds stays valid while
  the object is reachable. Every object is tracked with its size.
- **Headers.** A header is `mark (top bit) | heap id | kind`. The kind is the struct id + 1 for
  structs, or one of `HDR_ARRAY`, `HDR_ARRAY_PTR`, `HDR_BYTES`, `HDR_STR`, `HDR_CLOSURE`,
  `HDR_CLOSURE_ENV`, `HDR_CELL`, `HDR_CELL_PTR`, `HDR_MAP`, `HDR_BIGINT`. The heap id keeps a collector away from other
  threads' objects reached through a shared global slot.
- **Tracing.** Which slots of a struct are pointers is a mask the generated `ts_init` registers
  (`ts_rt_register_struct`). Pointer arrays, cells and script closure environments are traced.
- **Maps** ([map.rs](../tools/platypus_ts_rt/src/map.rs)). `Map` and `Set` are one object kind, `HDR_MAP`
  (`MapObj`: key kind, whether values are pointers, counts, next sequence number), with two raw blocks
  outside the object: the entries in insertion order (`key`, `value`, `seq`, `hash`, `deleted`; deleted
  ones stay as tombstones) and an open-addressing index of entry numbers. A rebuild keeps the order and
  drops tombstones. A Set is a map with unused values. Equality is SameValueZero: `NaN` is a key, `-0` is
  `0`, strings by content (hash cached in the string's spare header word), objects, handles and booleans
  by identity. The collector traces the live entries' keys and values (by key kind and `val_ptr`) and
  frees the blocks with the object. **Iteration is live and exact:** every entry gets an increasing
  sequence number, and an iterator is the last number it gave out plus a position hint;
  `ts_rt_map_next(map, seq, hint)` finds the first entry with a larger number, so deleting, adding and
  rebuilding under a loop need no fix-up. Entry points: `ts_rt_map_new_gc`, `find`, `set`, `delete`,
  `clear`, `size`, `key_at`/`value_at`/`seq_at`, `next`, `keys_gc`/`values_gc` (snapshot arrays).
- **Roots.** (1) The shadow stack: a generated function with pointer locals links a `Frame` with one
  slot per pointer local plus spill slots (`ts_rt_frame_push` / `ts_rt_frame_pop`) and mirrors every
  pointer assignment into it. (2) Globals, registered by address (`ts_rt_add_global_root`). (3) Host
  roots, for Rust code that keeps script pointers across calls (`ts_rt_root_add` / `ts_rt_root_remove`).
  (4) Temp roots, for values a runtime routine holds while it collects (a new cell's value, a closure
  environment, the sort's merge buffer). Registers and Cranelift's own spill slots are never scanned.
- **The spill rule** ([frames.rs](../tools/platypus_ts/src/frames.rs)). A pointer evaluated early and held
  while a later operand can allocate is copied to a frame slot, unless it is a local no later operand
  assigns, a global, or null (`spill_flags`, `is_stable`, `may_alloc`). Field reads are never trusted.
`CallVirtual` and `InstanceOf` are counted like calls (`CallVirtual` also allocates, as a call). The
  slot count computed here is the number codegen's `eval_args` consumes, and codegen refuses a
  function whose spills would overrun its frame. A new IR node that evaluates operands must use
  `eval_args` and be counted in `count_expr`/`count_stmts`, `may_alloc` and `children`.
- **When it collects.** Only inside allocations by generated code (`ts_rt_obj_new_gc`,
  `ts_rt_arr_new_gc`, `ts_rt_bytes_new_gc`, cells, closures) once the larger of 512 KiB and the live size
  has been handed out, or when the host calls `ts_rt_gc_collect`. Host-side constructors
  (`ts_rt_obj_new`, `ts_rt_arr_new`, `ts_rt_arr_push`) never collect, so Rust can build a graph from
  unrooted pieces and root it afterwards. `ts_rt_gc_enable(false)` stops automatic collection.
- **Stress mode** (`ts_rt_gc_stress(true)`): collect at every allocation and fill freed memory with
  `0xDD`, so a missing root crashes loudly. The root analysis is a conservative approximation, not a proof.
- **Entry points** are `#[unsafe(no_mangle)] extern "C"` functions named `ts_rt_*`: allocation, arrays
  (`push`, `set_len`, `insert`, `drop`, `splice`, `slice`, `append`, `reverse`, `fill`, `sort`), strings
  (see below), cells and closures, math (`sin`, `cos`, `pow`, `random` ...), collection
  control (`gc_collect`, `gc_stress`, `gc_enable`, `allocated`) and roots. Panics inside `extern "C"`
  abort the process, so fallible logic is tested through a Rust-level function.
- **Strings** ([string.rs](../tools/platypus_ts_rt/src/string.rs); design in
  [platypus-ts-strings.md](research/platypus-ts-strings.md)). Stored as WTF-8 (UTF-8 plus the three-byte form
  of lone surrogates) in one GC object, `Str { header, byte_len, utf16_len, flags, cursor_unit,
  cursor_byte }` with the bytes inline (a leaf for the collector). Flags `ASCII` and `WELL_FORMED`: an
  ASCII string maps unit index to byte offset directly; for other text a one-entry cursor (last unit and
  byte position) makes a forward scan O(1) per step. What a script sees is JavaScript's exactly:
  `length`, indices, `slice` and `charCodeAt` count UTF-16 units, ordering is by code unit, lone
  surrogates are values, and a slice through a surrogate pair leaves lone surrogates that rejoin on
  concat. Host-side routines never collect: `ts_rt_str_new` (UTF-8/WTF-8 bytes), `ts_rt_str_from_utf16`,
  `ts_rt_str_concat`. Routines for generated code may collect first and are suffixed `_gc`
  (`concat_gc`, `char_at_gc`, `slice_gc`, `substring_gc`, `from_code_point_gc`). The rest return
  numbers or bools: `eq`, `compare`, `length`, `char_code_at`, `code_point_at`, `index_of`,
  `last_index_of`, `includes`, `starts_with`, `ends_with`. Rust code reads a string through `Str`:
  `as_bytes`, `as_str` (borrowed when well formed), `to_string_lossy` (lone surrogates become U+FFFD),
  `to_utf16`.
  Part two ([string_more.rs](../tools/platypus_ts_rt/src/string_more.rs), slice 1b), generated code calls
  the `_gc` ones: `trim_gc`/`trim_start_gc`/`trim_end_gc` (JavaScript's white space set), `repeat_gc`
  (a negative or infinite count is a fault, a RangeError), `pad_start_gc`/`pad_end_gc`,
  `to_upper_gc`/`to_lower_gc` (Rust's Unicode tables with special casing and final sigma; lone
  surrogates pass through), `at_gc` (null for undefined), `concat_n_gc` (template literals: one
  allocation from an array of parts), `split_gc` (a new array of strings) and `join_gc` (a null element
  is empty). Number conversion: `ts_rt_num_to_str_gc` (`String(x)`: shortest round-trip digits in
  JavaScript's layout, exponent form from 1e21 and below 1e-7), `ts_rt_num_to_fixed_gc` (`toFixed`;
  exact ties round up as in JavaScript, where Rust rounds to even), `ts_rt_num_to_radix_gc` (integers
  only; a fraction in another radix is a fault for now), and the parsers `ts_rt_str_to_number`
  (`Number(s)`), `ts_rt_str_parse_float`, `ts_rt_str_parse_int(s, radix)`. Not done yet: `toString(radix)`
  of fractions, `toPrecision`/`toExponential`, `normalize`, `localeCompare`, RegExp and the engine boundary
  (slice 3). Slice 2 changed `ts_rt_str_eq` to be null-safe, `ts_rt_str_compare` to return `f64`, and
  added `ts_rt_str_from_char_code_gc`.
  Tests: [tools/platypus_ts_rt/tests/strings.rs](../tools/platypus_ts_rt/tests/strings.rs) (M9-TS-162..172) and
  [strings_more.rs](../tools/platypus_ts_rt/tests/strings_more.rs) (M9-TS-173..177), both differential:
  `tests/gen_strings.ts` (run with bun) writes `strings_expected.json` and
  `strings_more_expected.json` from a 24-string corpus (part two adds number and parse cases) and the
  Rust tests repeat the same operations on the runtime and compare against bun's output.
- **Host side.** `link_anchor()` keeps the exports linked into an executable that only reaches them from
  generated code; `gc_stats()` reports collections and live bytes. Several modules in one process each
  need their own `struct_base`; registering a different table under a taken id is refused.

## Tests

[platypus_ts_tests](../tools/platypus_ts_tests) holds the TypeScript sources and the Rust that drives them.

| Path | What |
|---|---|
| `fixtures/sim.ts`, `sim.test.ts` | A game-rules simulation (678 lines) and its bun tests |
| `bench/*.ts` | One TypeScript module per language area (`closures`, `loops`, `switches`, `classes`, `inherit` ...); `bench.ts` and `sine_bench.ts` are the JavaScript sides of the benchmarks (run in bun, not compiled) |
| `bench/engine.ts` | Calls the `Demo` engine API; `build.rs` compiles it with the manifest of [tools/platypus_demo_api](../tools/platypus_demo_api/src/lib.rs) (a small world of entities written with `#[platypus_api]`), whose shims the test binaries link |
| `bench/scene.ts` | Drives the real scene: `build.rs` compiles it (prefix `scn_`, base 15000) with `platypus_scene`'s manifest (`platypus_scene::api::scene_api::MANIFEST`) and the tests run it against a real `Scene` made current with `platypus_scene::api::enter`; `platypus_ts_tests::scene_anchor()` keeps the `Scene` shims linked |
| `build.rs` | Compiles `fixtures/sim.ts` and every `bench/<name>.ts` in a list of `(name, prefix, struct_base)` at build time, writes the `.obj` and `<name>_bindings.rs` to `OUT_DIR` and passes the objects to the linker. Each program is cached there under a hash of its (bundled) source, prefix, base, manifests and the build script's own executable (relinked by cargo whenever `platypus_ts` changes), so a bench edit recompiles one program and a compiler edit all of them, in parallel. Build scripts run unoptimised (no `build-override`): `platypus_ts` built once more at `opt-level = 3` cost more than it saved |
| `src/lib.rs` | One `pub mod` per module that `include!`s its bindings, plus helpers (`root`, `push_f64s`, `gc_mode_from_env`, builders for the sim's state); `demo_api_anchor()` uses `platypus_demo_api` so the linker keeps its shims |
| `tests/<group>/*.rs` | Five test binaries by area, one file per module: `lang` (language, the bun-compared case files, the JIT), `stdlib` (Math, JSON, RegExp, typed arrays, Date, Intl, Temporal), `engine` (scenes, components, physics, animation, `sim_port` with 13 ported cases, `gc`), `engine_api` (engine services and maths, the facade) and `ui` (UI and windows); `tests/common/cases.rs` (the `//## name` case runner) is shared by `lang` and `engine_api` |
| `src/bin/{bench,sine_bench}.rs` | The benchmarks: the same work in bun and native code, one checksum for both |

Each language-area test calls the generated functions through the bindings (`ts_<prefix><name>`) inside
`in_every_gc_mode`, which runs the body normally, with `ts_rt_gc_stress(true)` and with
`ts_rt_gc_enable(false)`. **Expected numbers come from running the same TypeScript in bun**, so the
tests check that native code computes what JavaScript does. Test IDs are `M9-TS-*` in
[TESTS.md](../TESTS.md) (section "M9.b"); the next free number is in `tools/platypus_ts/TODO.md`.

Tests that use program globals (module variables such as a log array) take turns through a mutex, because the
globals are shared between test threads.

To add a test module:

1. Write `bench/<name>.ts` (exported functions only get symbols; run it in bun to get the expected numbers).
2. Add `("<name>", "<prefix>_", <struct_base>)` to the list in `build.rs`. Bases are 1000 apart (sim
   0, sine 1000, ... classes 11000, inherit 12000, props 13000, engine 14000, scene 15000, strings 16000, errors 17000); take the next free one.
3. Add a `pub mod` for it in `src/lib.rs` that includes `<name>_bindings.rs`.
4. Add `tests/<group>/<name>.rs` and a `mod <name>;` line in that group's `main.rs`: call `ts_<prefix>init()` first, use `in_every_gc_mode`, and tag each test with
   its `// M9-TS-nnn` ID.
5. Add the IDs to `TESTS.md`, then run fmt, clippy, `cargo test --workspace` and the stress run.

## Components for the host

`class Player extends Component { onUpdate(dt: number) { .. } }` plus `register(Player)` at the top level
([components.rs](../tools/platypus_ts/src/lower/components.rs)). `Component` is a prelude class with `id` and, when
the manifest has the `GameObject` handle, `gameObject: GameObject | null`. `register(X)` becomes a run-time call
(`__registerClass("X", spawn)`, wherever it is written) and exported functions are appended for every component class of the
program, registered or not: `platypus_new_X(id, gameObject)` and one `platypus_hook_X_<hook>(this, dt)`
for each hook the class or an ancestor defines (`onAwake`, `onStart`, `onUpdate`, `onLateUpdate`,
`onFixedUpdate`, `onPreRender`, `onDestroy`; all take `dt` so the host calls them alike).

The generated bindings add `classes() -> Vec<platypus_ts_rt::NativeClass>` (name, create thunk, hooks as
`Option<fn>`). `platypus_ts_rt::NativeHost` ([host.rs](../tools/platypus_ts_rt/src/host.rs)) is the engine side with no
script engine: `spawn(class, object)` (roots the instance, runs `onAwake`), `update(dt, active)` (`onStart` once,
all `onUpdate`, then all `onLateUpdate`), `fixed_update`, `pre_render`, `destroy`, `destroy_on_objects`. It runs
`ts_rt_microtasks_run()` after every hook and disables a component whose hook left an exception pending
(`Fault { id, class, hook }`). Same rules as the removed QuickJS `ComponentHost`. Not built: `@property` save/load
through `PROPS_<Class>` in the host, `getComponent`/`addComponent`, a message for the fault, `this.transform`.
Tests `M9-TS-556..561`.

**Startup (the host's side of `register` and top-level `await`).** Which classes are registered is not in the class table: it is
what the `register(..)` calls did when they ran, in call order, with their `spawn` option (`platypus_ts_rt::registered_classes()`,
`NativeHost::registered_class_names()`; `Game::init` clears it before the top level runs, so a reload registers again). The
host waits for `platypus_ts_rt::startup_state()` to be `Done` before it spawns the registered classes, loads `platypus.startScene` or
runs a component hook (`Native::advance_startup` in `crates/platypus_app/src/native.rs`); frames, timers, microtasks and the UI the
top level mounted run while it waits, so `createRoot().render(<Loading />)` before the first `await` shows and updates. A module
still pending after ten seconds is named in the log (`warn`); a `Failed` state is logged once ("the game did not start: module `x`
failed while evaluating: ..") and the game never starts. A hot reload into a program that is still starting puts the saved world
back when its startup finishes. A scene or prefab that names a component class no `register` call named gets a warning that names
it (`go.addComponent(Class)` needs no registration). Tests `M9-TS-3211..3215`, `M9-API-1615..1618`.

Contact hooks (`onCollisionEnter/Exit/Stay`, `onTriggerEnter/Exit/Stay`, and the `..2D` forms) take Unity's `Collision`
(`Collision2D` for the `..2D` names), and the trigger hooks the other `Collider` (`Collider2D`); a parameter typed as the
other kind gets that one. They are handle classes of the physics manifest (`platypus_physics_api::contacts`, see
`docs/platypus-api.md`). The host passes each thunk `(this, collision, collider)`, two handles (`ContactHook`), and the thunk hands
the hook the one it asks for; `NativeClass.contact_hooks` holds the thunks and `NativeHost::dispatch_physics` /
`dispatch_contacts` call them, one `Collision` per event shared by the object's components (design:
`docs/research/platypus-native-physics.md` section 8). Tests `M9-TS-1000..1005`.

The `GameObject` properties that are arrays or not in the engine API are done by the compiler: `go.children` and `GameObject.roots`
(TypeScript helpers over `childAt` / `rootAt`, appended when the source names them), `go.components` (every component of the program on
the object, `ts_rt_component_all_gc` with `Component`'s class id) and `go.scene` (`SceneManager.sceneOf`). `x === null` on an engine
handle tests the raw 0 (`go.parent`, `GameObject.find`). Tests `M9-TS-1010..1013`.

## Generics

Generic functions, classes, interfaces and type aliases are supported by **monomorphisation**
([generics.rs](../tools/platypus_ts/src/lower/generics.rs)): nothing is generic at run time. Each distinct list of type
arguments gets its own copy (`identity$$number`, `Box$$string`, `Box$$Item_5`), lowered with a type environment
(`Lowerer::tenv`) that maps the type parameters to IR types, so `T` in any annotation of the copy means that type.

- **Functions** `function f<T, U>(..)`, `const f = <T>(..) => ..`: type arguments come from the call
  (`f<number>(x)`), else from the argument types (arguments are lowered once only to learn their type, closures
  after the others so their parameter types are known and their result type is read from the body), else from a
  parameter default, else from the expected type of the result. A conflict (`two(1, "a")`) or a parameter that
  nothing fixes is an error. Constraints (`T extends X`) are accepted and not checked.
- **Classes** `class Box<T>`: `Box<number>` in a type, `new Box<number>(1)` or `new Box(1)` (inferred from the
  constructor) make the class `Box$$number` with its own struct, fields and methods; `extends Base<T>` and
  recursive uses (`next: Node<T> | null`) work. `x instanceof Box` is true for any specialisation (the table is
  filled in at the end, so specialisations made later count).
- **Generic methods** `map<U>(f: (x: T) => U)`: one copy per list of type arguments, called directly (never
  virtual, so an override of a generic method is not dispatched). Only plain instance methods; a generic static
  method, accessor or constructor is an error.
- **Interfaces and aliases** `Pair<A, B>`, `Maybe<T>` are specialised the same way.
- **Not built**: a generic function used as a value (`xs.map(identity)`), exporting a generic function (export a
  specialisation), default type arguments depending on later ones.

Tests `M9-TS-562..574` (all bun-compared except the error cases).

## UI: JSX and hooks (native reconciler)

TSX components compile against the `Ui` namespace of [platypus_ts_ui](../tools/platypus_ts_ui/src/lib.rs) and are rendered by
[platypus_ui_native](ui-native.md); no JavaScript engine. Source rewrite: [jsx.rs](../tools/platypus_ts/src/lower/jsx.rs) (runs
first, whenever the source uses JSX or hooks); the generic hooks and the event class are the prelude
[ui.ts](../tools/platypus_ts/src/lower/ui.ts).

JSX is part of the compiler: when the source uses JSX or hooks and the caller's manifests lack the `Ui` namespace,
`platypus_ts` adds its embedded copy (`ui_manifest.json`; the test `M9-TS-1511` in `platypus_ts_ui` checks that it equals
`platypus_ts_ui::manifest_json()`, regenerate with `cargo run -p platypus_ts_ui --example manifest`). The compiled code calls
`ts_ui_*` symbols, so a host that compiles JSX must link `platypus_ts_ui`; without it the link fails. A JSX syntax error
is reported by the TSX parser with its line (`parse error (line 4): ...`). `{cond && <x/>}` renders an empty fragment
when `cond` is false.

- **Elements.** `<box style={{ width: 10 }} onClick={f} className="x" key="k">` becomes builder calls
  (`Ui.begin`, `Ui.style`, `Ui.attr`, `Ui.on`, `Ui.key`, `Ui.child`). A `Ui` function with several versions
  (`style`, `attr`, `child`, `dep`, `stateNew`, `store`) is picked by the argument types (`Manifest::overloads`); a child
  may be an element, a string, a number or an array of elements. Elements are `UiElement` handles. `style={{ width: 10 }}` is a literal; `style={obj}` (or a literal with spread) uses the typed object's fields (`Ui.styleObj`: numbers
  and strings, optional fields left out when null); `{...obj}` on a host element does the same for attributes
  (`Ui.attrObj`: numbers, strings, booleans; handlers are written as `onX`), and `{...props}` on a component spreads into its props
  (not with `memo`). A spread of a conditional, `{...(c ? handlers : {})}`, sets the taken arm's props and `{}` sets
  none (`M9-TS-3532`). `useState(name)` takes `name` as a lazy initialiser only when it resolves to a function: the JSX
  pass keeps the bindings (parameters and declarations, and whether each is a function) of the functions and blocks it is
  in (`Finder::locals`) and resolves the name through them before the top-level functions, as TypeScript scoping does, so
  a local `const shown = ..` is a value even when a top-level function is called `shown` (`M9-TS-3533`). `Text`, `Span`, `Input`, `TextArea`, `Image`, `List`, `Dialog` (from `@platypus/react`) are the intrinsic elements unless
  the program declares a component of that name. A key may be a number; `{flag}` renders nothing; a style value may be undefined.
- **Components** `function Counter(props: P): UiElement`: `<Counter a={1} />` becomes `Ui.component(ID, () => Counter({ a: 1 }))`,
  `ID` a hash of the name (it says which state belongs to which component). With no attributes the call is `Counter()`.
  Children of a component arrive as `props.children: ReactNode` (= `UiElement[]`): elements, text, numbers and lists; text and
  numbers are wrapped into elements (`Ui.child(Ui.fragment(), x)`). `<Btn />` for a component whose props are all optional passes an
  empty object. A component may `return null` (its type is then `UiElement | null`; a null handle renders nothing).
- **Events.** `onClick`, `onPointerDown`, ... become handlers of the lowercase kind (`"click"`, `"pointerdown"`;
  `onXCapture` is `"x:capture"`, `onChange` is `"input"`). The handler receives a `UiEvent` (`type`, `target`, `clientX`,
  `clientY`, `button`, `buttons`, `detail`, `deltaX`, `deltaY`, `shiftKey`, `ctrlKey`, `altKey`, `metaKey`, `repeat`, `scrollLeft`/`Top`/`Width`/`Height`, `clientWidth`/`Height`, `key`, `code`,
  `value`, `target.id`, `target.value`, `stopPropagation()`, `preventDefault()`). A handler may take no parameter, or be undefined.
- **Hooks.** `const [n, setN] = useState(0)` (generic `useState<T>`; numbers, booleans, and script pointers such as strings,
  arrays and objects), `useRef`, `useMemo`, `useCallback`, `useEffect(f, [deps])` (`f` may return a cleanup function; no
  list means every render; a list is rewritten into `Ui.dep(..)` calls, so it may mix types). Setting state from a handler
  schedules a render; setting it during a render only keeps the value. The updater form `setN(n => n + 1)` works for
  setters bound by `const [x, setX] = useState(..)` when the argument is an arrow or function (it becomes `__up_setX`, which
  applies the function to the latest state); a setter passed around under another name takes values only. A setter,
  updater or reducer dispatch called after its component unmounted does nothing, as in React: stores to a hook slot that
  is gone are ignored, and updaters and `dispatch` check `Ui.stateLive(slot)` (`ts_ui_state_live`) before reading it.
- **Closures to Rust.** The manifest type `fn:T` is a zero-parameter closure the engine roots until the description holding
  it is dropped; render functions, handlers and effects are such closures. `ptr` is any other script pointer, `handles:T`
  an array of handles.
- **Refs and context.** `ref={r}` (with `const r = useRef<UiNode | null>(null)`) writes the node into `r.current` when the
  element attaches and clears it on unmount (`Ui.nodeId(r.current)` is the node id); `createContext(default)`,
  `<Ctx.Provider value={v}>` and `useContext(Ctx)` for numbers, booleans and script pointers.
- **More hooks.** `useReducer` (`const [s, dispatch] = useReducer(reducer, init)`; dispatch applies the reducer to the latest
  state), `useImperativeHandle(ref, create, deps)` (filled and cleared as an effect), `useSyncExternalStore(subscribe, getSnapshot)`
  (resubscribes only when `subscribe` changes).
- **`memo` and `forwardRef`.** `const Fast = memo(Slow)` is erased and `<Fast a={x} />` passes its props as the component's
  dependencies, so equal props skip the render (a prop expression is evaluated twice: keep them free of side effects).
  `const Box = forwardRef((props, ref) => ...)` becomes `function Box(props, ref)`; `<Box ref={r} />` passes `r` (or `null`) as
  the second argument. A component that is not a `forwardRef` gets `ref` as an ordinary prop.
- **Lazy state, node methods.** `useState(() => init)` and `useState(read)` (an arrow, or a top-level function) compute the first
  value once (`useStateLazy`); state may be an array of numbers. `ref.current?.focus()`, `.blur()`, `.scrollTo(x, y)` queue a
  `NodeCommand` that the host applies to its tree (`platypus_ts_ui::take_node_commands`); `useRef<T>(null)` is `useRef<T | null>(null)`.
  `scrollTo` / `scrollBy` also take the DOM's `{ left, top, behavior }`, and `scrollIntoView({ block, inline, behavior })` uses the DOM
  defaults (`NodeCommand::ScrollTo` / `ScrollBy` / `ScrollIntoView`, applied by `App::apply_node_commands`); their option types
  (`ScrollToOptions`, `ScrollIntoViewOptions`, `ScrollBehavior`, `ScrollLogicalPosition`) are generated by `ui_dts.rs` from the Ui manifest.
  Callback refs `ref={(el) => ..}` work too (`ts_ui_ref_fn`, lowered by `lower_ui_ref`), and `Ref<T>` is `{ current: T }`.
- **No imports needed.** Engine and UI names are globals; `tsc` sees them through `types/platypus.d.ts` (`platypus-build types`, generated from the manifests, the UI tables (`platypus_ui::tables`, written by `ui_dts.rs`) and the compiler built-ins (`builtins.rs`); see `docs/platypus-build.md`). `import { .. } from "@platypus/engine" | "@platypus/react" | "react"` is still accepted and erased by the module graph (`modules.rs`); a program that has none resolves the same names.
- **Ambient types** (the prelude and the `.d.ts` are both written by [ui_dts.rs](../tools/platypus_ts/src/ui_dts.rs) from the tables of
  `crates/platypus_ui/src/tables`; the hook helpers stay in [ui.ts](../tools/platypus_ts/src/lower/ui.ts)):
  `ReactNode`, `React.ReactNode`, `PlatypusElement`, `PlatypusKeyboardEvent` and the other event names, `FullScreenMode`, `Gamepad`,
  `Collision2D` (= `Collision`) and `PlatypusStyle`. The prelude also has the named unions, `Length` and
  `interface __PlatypusStyleProps` (from `STYLE_PROPS`), because the compiled `PlatypusStyle` is a list of entries: `PlatypusStyle["prop"]`
  and `keyof PlatypusStyle` read the declared types there (`M9-TS-3525`). `number | string` is the class `StyleValue` (a number or a string converts where it
  is expected). `PlatypusStyle` is a list of entries: `const card: PlatypusStyle = { width: 100, padding: "4px" }` builds it,
  `{ ...card, ...style, flexGrow: 1 }` concatenates (a later entry wins, `undefined` values apply nothing) and `style={card}` applies
  it. The `.d.ts` (`platypus_ts::ui_dts::declarations`) declares the same names, the `react` and `@platypus/react` modules and `JSX` for `tsc`.
- **Tuples and records.** A tuple whose elements share one type (`[number, number]`) is an array of it; `Record<Weather, V>` over a numeric enum is an array indexed by the enum value (`{ [Weather.Rain]: v }` builds it); `Record<string, unknown>` is a `PlatypusStyle` when the ambient types are there. `createRoot(t).render(el)` is `UiRoot.mount(t, el)` (a `WorldPanel` target uses `ts_ui_mount_panel`); top-level expression statements run in `init` in source order. A `const` arrow with annotated parameters and no value return may call itself; a top-level `let f = () => ..` is a variable.
- **Language.** Parameter types come from their defaults (`phase = 0`); a program class or interface shadows an engine name
  (`class Scene`); `static get x()` reads as `Class.x`; a function with fewer parameters passes for one with more.
- **Limits.** The updater form `setN(n => n + 1)`; an inline object literal returned from a callback needs a type annotation
  when another argument names the type (`(): Api => ({ ... })`); `string | null` state works as a pointer, `number | null`
  as a number.
- **Hot reload data.** A program with components exports `__uiSignatures(): string` (`Name:ID:hook,hook;...`, ids are name hashes,
  hooks in call order). The host keeps the old table, and after loading new code calls `Runtime::refresh` with the ids whose
  list changed ([ui-native.md](ui-native.md)).
- **Not built yet**: `platypus_app` wiring, the dev loop itself.

Tests `M9-TS-575..588`.

## Engine objects and options

Compiled gameplay code reads like the engine's TypeScript:

- `new GameObject("Main Camera", { position: new Vector3(0, 0, 10) })` is `GameObject.create()` plus the setter of `name` and of each
  option (an option the object has no property for goes to its `transform`); `go.addComponent(Camera, { orthographic: true })` is
  `Camera.attach(go)` plus a setter per option (`engine_objs.rs`). A key with no setter is an error that names it.
- A trailing `{ key: value }` argument of an engine function fills the parameters after the positional ones by name; a parameter
  left out is `false`, `0` or `""` (`audio.load(path, { stream: true })`; Rust parameter `r#loop` is the script's `loop`).
- `Time.deltaTime`, `Gizmos.color = c`, `Color.white`, `Color.hex("#ff0000")`: namespace properties and statics of
  [platypus-api.md](platypus-api.md).
- `register(Player)` or `register({ Player, Enemy })`.
- Class fields without an annotation get their type from the initialiser's syntax: literals, arithmetic, a top-level `const`,
  `C.x` of a constant object, object literals, `Color.white`, `new Vector3(..)`, `Color.hex(..)`, `new Tween<number>(..)`.
- `Record<"a" | "b", V>` (a struct with a field per member) and `rec[kind]` with a union-member `kind`.
- `boolean | undefined` works for fields, parameters and members (`??`, truthiness, `=== undefined`, assignment).
Tests `M9-TS-593..600`.

## Component properties and lookups

The host edits components without a script running. `platypus_ts_rt::NativeHost` has `properties(id)`, `get_property` / `set_property`
(`PropValue`: null, number, boolean, string, component id), `save_properties` and `load_properties`, all through the generated
`PROPS_<Class>` table that each `NativeClass` carries (offsets are checked against the type name: `number`, `number?`, `boolean`,
`string`, `object:Class?`; other kinds are not saved yet).

`go.getComponent(Player)` and `go.addComponent(Player, { speed: 3 })` (also on `this`, through its `gameObject`) are compiler
intrinsics for classes of the program: the class becomes its struct id, and `ts_rt_component_get` / `ts_rt_component_add_gc` search and
extend a per-thread registry the host keeps in step. `addComponent` builds the object at once and applies the options; its `onAwake`
runs when the calling hook returns (the removed QuickJS host ran it immediately). `getComponent(Base)` finds exact classes only, not subclass
instances. Tests `M9-TS-601..604`.

`Task.delay(ms)`, `Task.delaySeconds(s)` and `Task.frame()` are a class of TypeScript appended to the program (`prelude::TASK`) over `setTimeout`; the host advances the timers with engine time (`ts_rt_timers_advance`). Test `M9-TS-651`.

## Stack values

Value-class objects (`Vector3`, ...) are made by the lowering as heap objects. `platypus_ts::sroa` runs on the module before code
generation and turns every local of such a type whose object never escapes its function into one number local per field: field
reads and writes become variable reads and writes, `new Vector3(..)` becomes assignments, and an engine call with a value result
(`CallExternInto`) becomes `CallExternOut`, which reads the result from the call's stack buffer straight into the locals. A local
stays an object when it is passed to a script function, returned, stored in a field, array or closure, or when two names for it
exist and either is changed. `JSON.stringify` of an engine handle gives `{}`.

## Several files

`platypus_ts::modules::bundle(entry)` (`compile_program` bundles and compiles) turns a program of several files into one source text: relative
imports and re-exports are resolved, `@platypus/*` and `react` imports erased, colliding top-level names renamed, `import "./x.css"`
listed for the build tool. `compile_with_manifests(src, ..)` still takes one source. See `docs/platypus-build.md`. Tests `M9-TS-1800..1806`.
## Running a compiled game in the process (JIT)

[jit.rs](../tools/platypus_ts/src/jit.rs): `compile_and_load(source, &JitOptions { prefix, struct_base, manifests, symbols, .. })`
lowers and generates code on a 64 MiB thread (the game's code then runs on the host's main thread, which
`crates/platypus_app/build.rs` asks to be 16 MB on Windows), running the same `codegen::generate` (generic over the Cranelift `Module`; the object writer is
unchanged, byte for byte, `M9-TS-1900`) into a `cranelift-jit` module with the CPU's own features. Imports are resolved by name
from `platypus_ts_rt::symbols()` (generated by `platypus_ts_rt/build.rs` from its exports; `tools/symbols_build.rs`, also used by
`platypus_ts_ui`), `platypus_api::symbol` (filled when a manifest is read) and `JitOptions::symbols`; a missing one is an error before any
code runs. `Game::function("name")`, `Game::init()`, `Game::class_meta()` (the data form of the bindings' `classes()`,
`classmeta.rs`) and `Game::classes()` (`NativeClass` values for `NativeHost`). Memory of a `Game` is never freed.

`codegen::generate(module, m, far_refs)` takes a `far_refs` flag: the JIT passes `true`, the object writer `false`. With it,
`Cg::data_addr` and `Cg::func_addr` declare the module's own data and functions non-colocated, so they compile to 64-bit
absolute immediates instead of 32-bit RIP-relative references. `cranelift-jit`'s default memory provider puts code and data
in separate allocations that can lie more than 2 GB apart (likelier after a hot reload in a grown process); applying an
`X86PCRel4` relocation then panicked (`TryFromIntError` in `compiled_blob.rs`), which crashed about 3 hot reloads in 10.
Calls are unchanged: `cranelift-jit` routes an out-of-range call through a veneer. Object files keep the RIP-relative form.

`platypus_app` reads `PLATYPUS_TS_GC` when the native backend starts (`gc_mode_from_env` in `native.rs`), with the same values as the
tests: `stress` collects at every allocation and poisons freed memory, `off` never collects. Use it to chase memory bugs in a
running game. After each compile, `native.rs` also gives the game's exported functions to the crash reporter
([platform.md](platform.md#crash-report-windows)).

**Debug info** (design and plan: [research/platypus-ts-debug-info.md](research/platypus-ts-debug-info.md)). The lowering puts
`Stmt::Line(n)` (a line of the bundled source) before each statement; codegen turns it into a Cranelift source location, and
`Game::line_table()` gives the JIT's `(start, end, line)` ranges by address, which the crash reporter maps to a line. Every
compile pays for it; the cost is a TODO item. `platypus-build build` writes `build/game.pdebug` (a hash of the bundled source
and where each file starts) to turn the line into `file:line`. The JIT also registers Windows x64 unwind tables
(`unwind.rs`, `RtlAddFunctionTable`), so debuggers, profilers and exceptions can walk through compiled frames. The crash
report's game call stack does not use the system unwinder: a handler-time `RtlVirtualUnwind` over the host's frames faulted in a
unit test on some builds, so `unwind::stack_words` scans the stack for return addresses into compiled code instead. Host use:
[platypus-api.md](platypus-api.md#native-games-in-platypus_app-compiled-in-process).
