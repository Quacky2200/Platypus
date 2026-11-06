# A dynamic tier for untyped JavaScript (design and measured spike)

> The spike's code (`tools/platypus_js`) is not in the tree: the `iron-js` branch was removed and only
> these notes were kept. The spike was commit `424e0a7b`, the notes' last version `b2fe28f1`.

Status: research, 2026-10-20. Question from the user: can untyped JavaScript be compiled to the same
IR as typed TypeScript, through a real dynamic value type, JIT-compiled with Cranelift at load time,
faster than QuickJS, reusing the Rust standard library in `platypus_ts_rt`? Answer: **yes, measured on a
small subset: 1.2x to 12.8x QuickJS; recommendation: no-go for a complete dynamic tier now, go for
typed TypeScript compiled at load time for mods** (section 6). Code: `tools/platypus_js` (the spike, as a
crate), tests `M9-TS-1420..1440`. Parent note: [platypus-native-without-quickjs.md](platypus-native-without-quickjs.md).

## 1. Value representation

One 64-bit word, `Ty::JsValue` in the IR (it was reserved and already counted as a pointer type by
`Ty::is_ptr`, so frame planning roots it). Pointer-favouring NaN-boxing, as JavaScriptCore does:

| Kind | Encoding | Test |
|---|---|---|
| GC object (string, array, object, function, cell, any `platypus_ts_rt` object) | the raw pointer: top 16 bits clear, value >= 0x10000 | `v - 0x10000 < 2^48 - 0x10000` |
| double | its bits + 2^49 (`DOUBLE_OFFSET`); NaNs canonical when boxed from Rust | `v >= 2^48` |
| int32 (reserved, not used by the spike) | `0xFFFF_0000_0000_0000 \| i` | top 16 bits all set |
| `undefined` / `null` / `false` / `true` | 0x0A / 0x02 / 0x06 / 0x07 | equality |
| empty (array hole, zeroed slot) | 0 | never a script value |

Why pointer-favouring rather than double-favouring: a pointer is the raw pointer, so **every
`platypus_ts_rt` routine takes a dynamic string or array unchanged** (`ts_rt_str_concat_gc`,
`ts_rt_str_split_gc`, `ts_rt_str_join_gc`, `ts_rt_arr_push`, ...), an array of strings made by the
runtime is already an array of `JsValue`s, and the typed tier can pass objects to dynamic code with
no conversion.

**Tracing.** The collector already treats every frame slot, global root, host root and masked
struct slot as "pointer or 0". The one change: `mark_ptr` skips words that cannot be objects
(`platypus_ts_rt::is_object_word(w) = w >= 0x10000 && w >> 48 == 0`). For typed code nothing changes
(its slots hold pointers or 0); a slot holding a boxed double or an immediate is now skipped instead
of dereferenced. With that, `JsValue` fields in structs, `HDR_ARRAY_PTR` arrays of values, cells
(`HDR_CELL_PTR`), map entries and the pending exception are all traced by the existing code. Test
`M9-TS-1403`.

## 2. Object model (the spike's subset)

- **Objects** are a registered struct kind (`OBJ_HDR`, struct ids from 90,001 so they never meet a
  compiled module's): `[header][shape][proto][6 inline slots][overflow array]`; the pointer mask
  covers proto, slots and overflow; the shape is a number.
- **Shapes** (hidden classes) are a Rust-side table: keys in insertion order, key-to-slot index,
  transitions (`shape_add(shape, key)`). Object literals get their final shape at compile time, so
  `{x, y, vx, vy}` is one allocation plus inline stores.
- **Atoms**: property names interned to `u32` at compile time; builtins (`length`, `push`, ...) have
  fixed numbers.
- **Inline caches**: one `{shape, slot offset}` cell per property site in a table whose address is
  baked into the code. The fast path is: is a pointer, header kind is `OBJ_HDR`, shape equals the
  cached shape, then one load at the cached offset. A miss calls `js_get_slow`, which looks up the
  shape, walks the prototype chain, refills the cache, and throws the `TypeError` for
  `undefined.x`. `length` of arrays and strings is inline.
- **Prototype chain**: the `proto` slot (`Object.create(p)`); misses walk it in the slow path.
- **Arrays**: the runtime's `Array` with `HDR_ARRAY_PTR` (elements are `JsValue`s); `a[i]` inline for
  an integral in-range index, holes read as `undefined`, `push` inline when the receiver is an
  array, writes past the end fill holes.
- **Functions**: a struct kind `FN_HDR` `[header][code][env][arity]`. A call checks the kind and the
  arity and calls the code directly (`call_indirect`, signature `(env, this, a0..an) -> value`);
  otherwise `js_call_slow_gc` pads or drops arguments. Top-level function declarations called with a
  fitting argument count are direct `Call`s.
- **Closures**: captured variables (found by a scan of inner functions) live in the runtime's cells;
  a closure's environment is an array of cells, as in the typed tier.
- **Exceptions**: a pending-exception word at a fixed address (a GC root); checked after calls and
  slow paths; `throw` stores and jumps to the innermost handler; `try`/`catch` as `Stmt::Try`.
- Not in the subset: `new`, classes, getters/setters, `delete`, `for-in`/`for-of`, destructuring,
  spread, `finally`, labels, `arguments`, `eval`, `with`, `Proxy`, symbols as keys.

## 3. Lowering: the same IR, unchanged

`tools/platypus_js/src/lower.rs` (oxc to IR) produces the structures of `tools/platypus_ts/src/ir.rs` with
**no change to the IR**: every value is `Ty::JsValue`; every dynamic operation is
`ExprKind::Rt(RtFn::Str("js_add_gc"), args)` and friends (`RtFn::Str` is the existing "runtime
function by symbol" node); constants for atoms, cache numbers and shapes are `Num` operands;
statements (`Assign`, `If`, `Loop`, `Block`, `Break`, `Return`, `Try`, `Throw`, `CellSet`) are the
typed tier's. Names ending in `_gc` may collect, so **`platypus_ts::frames` (the GC root analysis) runs
on the dynamic IR unmodified**: locals get frame slots, operands are spilled where a later operand
can allocate. A local number inference (`numinf.rs`) then turns locals that only ever hold numbers
into `f64` locals and box/unbox pairs into plain `Bin`/`Cmp` nodes (the typed tier's), without
speculation or deoptimisation.

The code generator (`jit.rs`, Cranelift JIT in the process) is separate from `platypus_ts`'s
`codegen.rs` for the spike; it inlines the fast paths and calls `rt.rs` helpers by symbol.

## 4. Measurements

Machine: AMD Ryzen 9 5900X, Windows 11, release builds, best of 3 runs of `run(n)` after loading;
QuickJS is rquickjs 0.14 (the engine's runtime); bun is a JIT reference. Results are checked equal
in all engines. Run: `cargo run --release -p platypus_js --bin platypus_js_bench -- bench`. Switches used
for the attributions below: `IRON_JS_INFER=0` (no number inference), `IRON_JS_OPT=none`,
`IRON_JS_NOFRAMES=1` with `IRON_TS_GC=off` (no shadow-stack frames; measurement only, unsafe with
the collector on), `benchfile file.js n`, `rtbench` (runtime microbenchmarks).

| Benchmark | n | QuickJS | platypus_js | vs QuickJS | platypus_js, no number inference | bun |
|---|---|---|---|---|---|---|
| numeric loop (`s = (s + i*i) % 1000003`, a branch on `i & 1`) | 10^7 | 860 ms | 70.3 ms | **12.2x** | 126.7 ms (6.9x) | 88.1 ms |
| object updates (1,000 `{x, y, vx, vy, alive}`, 4 reads, 2-4 writes, 2 compares each) | 2,000 | 354 ms | 27.7 ms | **12.8x** | 34.7 ms (10.6x) | 3.7 ms |
| strings and arrays (20 `"item" + i` pushed, `join`, `split`, index) | 20,000 | 110 ms | 87-91 ms | **1.2x** | 93 ms | 16.3 ms |
| closures and methods (a counter object's method, an arrow call) | 2*10^6 | 224 ms | 40.1 ms | **5.6x** | 40.9 ms | 3.1 ms |

Load (compile) time: the four benchmark scripts lower in 0.1-0.6 ms and compile in 1-4 ms. A
6,000-line file (the benchmarks repeated 100 times): lowering 11 ms, Cranelift **347 ms**
(`opt_level=none`: 271 ms); QuickJS loads it in 7 ms. Load time is the dynamic tier's weak point.

### Where the time goes

- **Numeric**: boxed `i++` is a chain of int/float moves (4.4 ns per empty iteration); with number
  inference the empty loop is 0.9 ns and `s + i*i` 1.0 ns per iteration. The rest is the `%`: a
  64-bit `idiv` in the integer fast path (a 32-bit one measured slower, because `s + i*i` leaves the
  int32 range and falls back to `fmod`).
- **Objects**: 14 ns per particle update (about 10 property operations): each inline-cache hit
  loads the cache cell (shape and offset) and checks pointer, kind and shape. bun's optimizing JIT
  removes repeated shape checks and keeps doubles unboxed in registers.
- **Strings**: the runtime's string routines are QuickJS's speed: `ts_rt_str_concat_gc` is 52 ns,
  `split` ~100 ns per piece. A first version was 0.5x QuickJS: the slow `+` copied through three
  `Vec`s (186 ns) and `number_to_string` formats every integer through `format!("{:e}")`. Fixed in
  the spike with a string+string path (52 ns) and an integer-to-digits path for `"item" + i`
  (230 ns to ~120 ns per concat); the integer path belongs in `platypus_ts_rt::number_to_string` for both
  tiers (plan step B6). The collector is not the cost (off: same time).
- **Closures**: 60% is the shadow-stack frame: `ts_rt_frame_push` + `ts_rt_frame_pop` is 4.9 ns per
  call (two runtime calls, a thread-local, a `memset`); 40.5 ms with frames, 15.8 ms without (the
  collector off for that measurement only). The same cost hits typed code
  ([platypus-ts-compiler-improvements.md](platypus-ts-compiler-improvements.md)).

## 5. What a complete dynamic tier would take

The spike is about 3,600 lines of Rust and covers numbers, strings, plain objects, arrays, calls, closures,
exceptions. A tier that runs arbitrary mod JavaScript also needs: `new`, prototypes of builtins,
classes, getters/setters and property attributes, `delete`, property enumeration order (integer
keys first), `for-in`/`for-of`/iterators, destructuring, spread, rest, default parameters,
`arguments`, labels, `finally`, generators and `async` (the typed tier's coroutines can be reused),
ToPrimitive with `valueOf`/`toString`, sparse arrays, every `Array`/`String`/`Object`/`Math`/`JSON`/
`Map`/`Set`/`Date`/`RegExp`/`Intl` builtin taking dynamic arguments (the runtime has the algorithms;
each needs a dynamic entry point), `Symbol` keys, error objects with stacks, the engine API from
dynamic code (argument conversion per manifest), deoptimisation-free type feedback, int32 values,
and lazy (per-function) compilation to fix the load time. That is the size of the typed tier's
library work again (months), plus the conformance testing (test262 subsets).

## 6. Recommendation: no-go for now; mods are typed TypeScript compiled at load time

Reasons:

1. **It is possible and faster** (1.2-12.8x QuickJS here), so QuickJS is not needed for speed; but
   the remaining work (section 5) is large, and the typed tier already beats it on the code games
   are written in.
2. **Nothing needs untyped JavaScript today.** There is no mod loader; mods are a plan. Requiring
   mods to be TypeScript that `tsc --strict` accepts (the same rule as games) gives mods the typed
   tier's speed (bun-level on loops; 2-5x behind bun on method-heavy code, closing with inlining),
   the same API, and compile errors instead of runtime surprises.
3. **The cheaper option reuses everything**: the compiler, runtime, manifests and host exist; what
   is missing is compiling into memory instead of an object file (Cranelift JIT, the same code
   generator made generic over the module type) and resolving runtime and engine symbols in the
   process. Plan phase C.
4. **The spike keeps its value**: `Ty::JsValue` with this representation is the right shape for
   `any`/`unknown` inside typed code (untyped `JSON.parse`, plugin data, dynamic configs), built on
   demand from the spike's helpers, and for a full dynamic tier later if the user wants untyped
   mods (plan phase E, gated on that decision).

The decision that needs the user: **may mods be untyped JavaScript?** If yes, either QuickJS stays
for mods only, or phase E (the dynamic tier) is built. If no, QuickJS can go after phase D.

## 7. Sharing the JavaScript library between the tiers

The dynamic tier needs `platypus_ts_rt`'s heap, collector, strings, arrays, cells, closures, `Map`/`Set`,
`RegExp`, `Date`, `Intl`, `BigInt`, typed arrays, coroutines and timers. Evaluated split:

- **Needed now: nothing but `is_object_word`.** `tools/platypus_js` depends on `platypus_ts_rt` (and on
  `platypus_ts` for `ir` and `frames`) like any host; no module moves, no change to `platypus_ts`.
- **Possible later, mechanical:** move everything but `host.rs` into a new crate (`platypus_js_std`) with
  `git mv`, and make `platypus_ts_rt` `pub use platypus_js_std::*;` plus `host.rs` (the only module that
  needs `platypus_api`, `platypus_kinds`, `platypus_physics_api`; it uses only the public runtime API). Symbol
  names (`#[no_mangle] ts_rt_*`) and every path the compiler emits stay the same, so `platypus_ts` sees no
  change. Worth it when the dynamic tier must build without the engine crates (a standalone JS
  runtime) or a third user appears. Typed-only pieces (`json.rs` descriptor blobs) stay in the shared
  crate; they are data-driven and harmless to the dynamic tier.
- Must stay typed-tier-only: nothing in the runtime; the typed tier's code generator (`codegen.rs`)
  until phase C makes it generic.
