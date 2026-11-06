# platypus_ts: compiler improvements for both tiers

Status: research with two measured changes, 2026-10-20. Base `a0dcd3a`. Part of
[platypus-native-without-quickjs.md](platypus-native-without-quickjs.md); steps are in
[platypus-native-without-quickjs-plan.md](platypus-native-without-quickjs-plan.md) (phase B).

## 1. Why: typed code is slow at calls, not at arithmetic

The typed tier is level with bun on the sine and sim benchmarks, which are loops over fields and
numbers. Gameplay code also calls many **small methods** (`isWall(x, y)`, `outOfBounds()`, getters).
Two benchmarks of that shape (`particles`: 1,000 objects, `move()` calling `outX()`/`outY()`, a
`speed` getter, 2,000 steps; `grid`: `Grid.isWall(x, y)` with `Math.floor`, 3.3 million calls) show
it, measured on the developer machine (AMD Ryzen 9 5900X, Windows 11, release build, best of 5):

| Benchmark | platypus_ts at `a0dcd3a` | bun 1.x (JIT) |
|---|---|---|
| particles (n = 2000) | 44.7 ms | 6.4 ms |
| grid (n = 200) | 52.8 ms | 5.9 ms |
| busy sim (sim.ts, 10,000 steps) | 9.3 ms | 70.9 ms |

Where the time goes (measured by switching each cause off):

- **Shadow-stack frames.** Every function with a pointer local (every method: `this`) calls
  `ts_rt_frame_push` and `ts_rt_frame_pop`: two calls into the runtime, a thread-local access each,
  and a `memset` of the slots. Measured from Rust: **4.9 ns per push/pop pair**. A leaf method's
  body is often 1-2 ns of work.
- **`Math.floor` is a C-library call.** `codegen.rs` builds the ISA for the baseline x86-64 target;
  without SSE4.1 Cranelift lowers `floor`/`ceil`/`trunc`/`nearest` to calls of `floor`/`ceil`...
  in the C library (about 5 ns each) instead of one `roundsd`.

## 2. The two changes made (patches with this note)

### 2.1 No frame for a function that cannot collect (`frames.rs`)

The collector only runs inside allocations (`*_gc` runtime calls, `NewObj`, calls that may
allocate). While a function that never allocates and never calls is running, nothing can collect,
so its pointer locals need no roots; its callers root their own values. `frames::plan` now returns
an empty plan for such a function (no frame slot, no push/pop, no mirroring). The test is the
existing root analysis (`stmt_may_alloc`, the GC's correctness argument) plus engine calls
(`CallExtern*`), which count as "may collect" because an engine function could call back into
script. Tests: `M9-TS-1400` (a getter and a two-object predicate have no frame), `M9-TS-1401` (a
method that allocates or calls keeps it). All 699 integration tests pass, normally and with
`IRON_TS_GC=stress`.

### 2.2 x86-64-v2 code (`codegen.rs`)

`has_sse3`, `has_ssse3`, `has_sse41`, `has_sse42`, `has_popcnt` are enabled when targeting x86-64:
the x86-64-v2 level, which every x86-64 CPU since 2009 has and Windows 11 24H2 requires. Test
`M9-TS-1402` (an object using `Math.floor`/`ceil`/`trunc` imports no `floor`/`ceil`). Decision
recorded: compiled games require x86-64-v2 (noted in the plan; reversible in one line).

### 2.3 Measured effect

Same machine and harness (`tsb`: the TypeScript compiled by `platypus_ts` into an object, linked with
`platypus_ts_rt`; checksums equal bun's in every configuration):

| Configuration | particles | grid | busy sim |
|---|---|---|---|
| Before (baseline ISA, frames everywhere) | 44.6 ms | 52.8 ms | 9.3 ms |
| x86-64-v2 only | 46.0 ms | 22.7 ms | 9.3 ms |
| Frame elision only | 22.4 ms | 40.8 ms | 9.4 ms |
| **Both** | **22.4 ms (2.0x)** | **10.7 ms (4.9x)** | 9.3 ms (unchanged) |
| bun | 6.4 ms | 5.9 ms | 70.9 ms |

The busy sim's hot path is loops over arrays of objects with few calls, so it does not move; it is
the check that nothing got slower. The gap to bun on call-heavy code is now 1.8-3.5x; inlining
(3.1) is the step that closes it.

## 3. Proposals, ranked (gain per effort)

| Rank | Item | Effort | Expected gain | Evidence |
|---|---|---|---|---|
| done | Frame elision for non-collecting functions | 0.5 day | 2x on method-heavy loops | 2.3 |
| done | x86-64-v2 (SSE4.1 rounding) | 1 hour | 2.3x on `Math.floor`-heavy code | 2.3 |
| 1 | **Inlining of small direct calls at the IR level** (callee: `Call(FuncId)` target, at most ~40 IR nodes, no loops or `try`, not recursive; parameters become locals, `return` becomes `Break` out of a `Block` with a result local) | 2-3 days | particles 22 → ~8 ms (estimate: the remaining ~2.5 ns per call times 8 million calls), unlocks SROA and constant folding across calls | Remaining cost after elision is call overhead plus loads of `this` fields |
| 2 | **Cheaper frame push/pop** for functions that keep a frame: (a) the frame top in its own const thread-local `Cell` instead of inside `Heap` (measured: 4.9 → 4.0 ns per pair; the dynamic tier's closure benchmark 40.5 → 36.1 ms); (b) zero the slots inline instead of `memset`; (c) later, pass the frame top as a hidden parameter so push/pop are two stores | (a) 1 hour, (b) 1 hour, (c) 3 days | (a)+(b) ~1.5 ns per non-leaf call; (c) ~4 ns | Measured in the spike runtime |
| 3 | **Cold slow paths**: `set_cold_block` on exception exits, collector and fallback calls, `ts_rt_fmod` paths | 1 hour | a few % on branchy loops | Cranelift lays cold blocks out of line |
| 4 | **Integer fast path in `number_to_string`** (`"item" + i`; today `format!("{:e}")` plus four `String`s) | 1 hour | 2-3x on number-to-string concatenation (dynamic spike: 230 → 120 ns per concat) | 2.4 of the dynamic-tier note |
| 5 | **Bounds-check elimination** for `for (let i = 0; i < a.length; i++) ... a[i]` when the body cannot change `a`'s length (no calls, no `push`/`pop`/`splice`/`length =` on `a`) | 2 days | 5-15% on array loops | Each `a[i]` loads `len` and compares |
| 6 | **SROA of small script objects** that never escape (object literals used as records, `{x, y}` temporaries) | 1-2 days | no allocation for such temporaries (GC pressure) | `sroa.rs` already does it for value classes |
| 7 | **Value classes by scalars across script calls** (`Vector3` parameters/results of script functions) | 3-5 days (calling convention) | no allocation per call in vector-math helpers | sroa.rs "Left:" note |
| 8 | **Constant propagation and branch pruning** over literals and literal unions (after inlining: `kind === "a"` on a constant tag, defaulted parameters) | 1-2 days | small alone, useful after 1 | Cranelift folds within a function only |
| 9 | Devirtualisation of `CallVirtual` for classes with no subclasses | — | none | Already done: direct calls by whole-program analysis, `CallVirtual` only for real overrides (platypus-ts-classes.md) |
| 10 | Profile-guided block ordering | needs profiling infrastructure | low; 3 gives most of it | — |

### Applies to the dynamic tier too

The dynamic tier ([platypus-js-dynamic-tier.md](platypus-js-dynamic-tier.md)) reuses `frames.rs`, so 2.1
applies to it unchanged (its non-allocating leaf functions lose their frames), and 1, 2, 3, 4 help it
as much: its closure benchmark is 60% frame push/pop (40.5 ms with frames, 15.8 ms without, with
the collector off for the measurement).
