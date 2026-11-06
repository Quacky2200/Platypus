# Plan: absorbing QuickJS into the native stack (for the executor)

Written 2026-10-20 for a Sonnet executor. Base commit `a0dcd3a`, branch `platypus-js` (worktree
`.claude/worktrees/platypus-js`; it is not merged until the user reviews it). Read first, in order:
[platypus-native-without-quickjs.md](platypus-native-without-quickjs.md) (inventory, removal order),
[platypus-js-dynamic-tier.md](platypus-js-dynamic-tier.md) (value representation, spike numbers),
[platypus-ts-compiler-improvements.md](platypus-ts-compiler-improvements.md) (the two measured compiler
changes), then `tools/platypus_ts/TODO.md` "How to pick this up".

## 0. Rules for every step

- One step = one commit. Commit trailer exactly: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
  (the user's CLAUDE.md rule; keep it even though you are Sonnet).
- Shell: `export PATH="$HOME/.cargo/bin:$HOME/.bun/bin:$PATH"`. Edit with the Edit/Write tools or a
  Python script written to the scratchpad (heredocs mangle apostrophes and backslashes). Files are
  CRLF: read with `newline=''`, keep the line ending.
- Checks before each commit (all must pass; `tools/` is its own workspace):

  ```bash
  cargo fmt --all && (cd tools && cargo fmt --all)
  cd tools && cargo clippy --workspace --all-targets -- -D warnings
  cd tools && cargo test --workspace
  cd tools && IRON_TS_GC=stress cargo test -p platypus_ts_tests --test all
  ```
  plus `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo test -p <crate>` for any
  crate under `crates/` you touched.
- **`tools/platypus_ts` must not change behaviour.** Steps that touch it are marked **[platypus_ts]** and
  say how they stay neutral; all 699 integration tests (and stress mode) must stay green, and no
  expected output (`bench/*_expected.json`) may change. If one would change, stop: that step is
  wrong.
- Never touch `crates/platypus_app` or `crates/platypus_render` (another session owns them). Steps that need
  them are marked **[owner]**: write the code as a proposal in `docs/research/` instead and stop.
- Steps marked **[user]** wait for the user's answer. Do the next unblocked step instead.
- Test IDs: `M9-TS-1400..1499`, `M9-API-400..499` (already used: `M9-TS-1400..1403`,
  `1420..1440`). Add each ID's row to `TESTS.md` (section "M9.b", table rows like the others: ID,
  kind, one-line behaviour) in the same commit, and a one-line entry to the "Log" of
  `tools/platypus_ts/TODO.md`. Differences from JavaScript go in `tools/platypus_ts/TODO.md` too.
- Benchmarks on this machine are noisy (other agents build in parallel): run each benchmark three
  times and take the best; an acceptance number is met if the best run meets it.
- Do not launch any window or GPU program.

## 1. Material handed over with this plan

The research session could not write into the repository worktrees (a hook blocked it), so its
work is delivered as files. Paths are under the research scratchpad
`C:\Users\quack\AppData\Local\Temp\claude\C--Users-quack-Desktop-Platypus--claude-worktrees-recursing-wright-856ffa\536299b7-5ac7-4d2d-8024-3c1f01c9b8f2\scratchpad\`
(below: `SP\`):

| Item | Path | Used by step |
|---|---|---|
| These four notes | `SP\deliver\docs\research\*.md` | A0 |
| `platypus_ts_rt/src/lib.rs` with `is_object_word` and test `M9-TS-1403` | `SP\deliver\tools\platypus_ts_rt\src\lib.rs` | A1 |
| `frames.rs` with frame elision and tests `M9-TS-1400..1401` | `SP\deliver\tools\platypus_ts\src\frames.rs` | B1 |
| `codegen.rs` (x86-64-v2) and `lib.rs` (test `M9-TS-1402`) | `SP\deliver\tools\platypus_ts\src\{codegen,lib}.rs` | B2 |
| The `platypus_js` crate (spike): `src/{lib,rt,lower,jit,numinf,cases}.rs`, `src/bin/platypus_js_bench.rs`, `tests/diff.rs`, `Cargo.toml` | `SP\deliver\tools\platypus_js\` | A2 |
| Typed call-heavy benchmark `particles.ts` and the harness | `SP\deliver\tools\platypus_ts_tests\bench\calls.ts` | B0 |
| Unified diffs of all of the above against `a0dcd3a` | `SP\deliver\patches\*.patch` | any |

Each delivered file is the whole file at `a0dcd3a` plus the change; copy it over the repository file
(or `git apply` the patch). All of them were checked together in a copy of `tools/`: fmt, clippy
`-D warnings`, 31 `platypus_ts` unit tests, 23 `platypus_ts_rt` unit tests, 699 integration tests normally
and in stress mode, the `platypus_js` tests normally and in stress mode.

## 2. Phase A: the dynamic tier as a new crate (no change to `platypus_ts`)

### A0. Commit the research notes

- Goal: the four notes in the repository.
- Files: copy `SP\deliver\docs\research\{platypus-native-without-quickjs,platypus-native-without-quickjs-plan,platypus-js-dynamic-tier,platypus-ts-compiler-improvements}.md`
  to `docs/research/`. Add one line for each to `docs/README.md` if it lists research notes.
- Tests: none. Checks: none needed (docs only), but run `git status` (no Charm font).

### A1. `platypus_ts_rt`: the collector skips words that cannot be objects

- Goal: slots may hold a dynamic value (`JsValue`); typed code is unaffected.
- File: `tools/platypus_ts_rt/src/lib.rs` (take the delivered file). The change: `pub fn
  is_object_word(w: u64) -> bool { w >= 0x1_0000 && w >> 48 == 0 }` before `mark_ptr`, and
  `mark_ptr` returns early when `!is_object_word(p as u64)` (replaces the null check). Test
  `the_collector_skips_boxed_numbers_and_immediates_in_pointer_slots` (`M9-TS-1403`) in the existing
  `#[cfg(test)] mod tests` of lib.rs.
- Why neutral: typed slots hold a pointer or 0; user-space pointers on x86-64 and AArch64 are in
  `[0x10000, 2^47)`; static strings already have no heap id and are skipped as before.
- Risk: none known. Fallback: none needed.

### A2. `tools/platypus_js`: the spike as a crate

- Goal: untyped JS subset → `platypus_ts::ir` → Cranelift JIT, differential-tested against QuickJS.
- Files: copy `SP\deliver\tools\platypus_js\` to `tools/platypus_js/`; add `"platypus_js"` to `members` in
  `tools/Cargo.toml`. Its `Cargo.toml` uses `platypus_ts = { path = "../platypus_ts" }` (only the public
  `ir` and `frames` modules), `platypus_ts_rt = { path = "../platypus_ts_rt" }`, `cranelift-jit` and
  `cranelift-native` `0.136.3` (new dependencies; network needed once), `oxc_*` `0.154.0`,
  `rquickjs = "0.14.0"` (for the differential tests and the benchmark).
- Layout of the crate (do not redesign; the notes explain each piece):
  - `rt.rs`: the value representation (`num`, `as_num`, `is_ptr`, `UNDEF` 0x0A, `NULL` 0x02,
    `FALSE` 0x06, `TRUE` 0x07, `DOUBLE_OFFSET` 2^49), object and function layouts (`OBJ_HDR` 90,001,
    `FN_HDR` 90,002), shapes and atoms, inline caches (`Ic { shape: u64, offset: u64 }`), every
    `extern "C" js_*` slow-path helper, `symbols()` (the JIT's import table).
  - `lower.rs`: oxc → IR (`lower(src) -> Program`).
  - `numinf.rs`: local number inference.
  - `jit.rs`: `compile(&Program, opt) -> Compiled` (fast paths, `frames::plan`, `eval_args`).
  - `cases.rs`: benchmark and test scripts. `bin/platypus_js_bench.rs`: `bench`, `test`, `run file`,
    `benchfile file n`, `ir file`, `rtbench`.
  - `tests/diff.rs`: `M9-TS-1420..1439` (one test running the 20 scripts in order, IDs in the order
    of `cases::TESTS`), `M9-TS-1440` (unsupported syntax is a load error).
- Tests: `cd tools && cargo test -p platypus_js` and `IRON_TS_GC=stress cargo test -p platypus_js`.
- Benchmark: `cd tools && cargo run --release -p platypus_js --bin platypus_js_bench -- bench`.
  Acceptance (best of 3, against QuickJS in the same run): numeric >= 10x, object updates >= 10x,
  strings and arrays >= 1.0x, closures and methods >= 4x. Measured at research time: 12.2x,
  12.8x, 1.2x, 5.6x.
- Risks: the tools workspace runs tests on several threads; the dynamic tier bakes the address of
  the pending-exception word and of its caches into code, so a program must run on the thread that
  compiled it (the test runs all scripts in one test function). Fallback if `cranelift-jit` cannot
  be fetched: stop and report (no other JIT is acceptable).
- TODO.md: add a short section "Dynamic tier (`tools/platypus_js`)" pointing at the note.

### A3. `platypus_js`: integer fast path of number-to-string in the runtime **[shared runtime]**

- Goal: `"item" + i` and `String(i)` cost a third, for both tiers.
- File: `tools/platypus_ts_rt/src/string_more.rs`, `number_to_string`: before `format!("{:e}")`, if
  `x.fract() == 0.0 && x.abs() < 1e21` return the integer's digits (`(x as i128).to_string()`:
  exact for every such double, sign kept; `-0` gives `"0"`, as `-0 as i128` is 0). Then in `tools/platypus_js/src/rt.rs` remove the
  spike's own copy of this fast path in `to_string_bytes` (keep the string+integer one-buffer path in
  `js_add_slow_gc`).
- Why neutral: the same digits for every integer below 1e21 (JavaScript prints those without an
  exponent). Test `M9-TS-1404`: for 10,000 integers (0..5000, powers of two up to 2^70 below 1e21,
  negatives, `-0`), the new function equals the old one (keep the old one as a private
  `number_to_string_slow` used by the test).
- Benchmark: `platypus_js_bench benchfile` on a script with `const x = 'item' + i` in a loop
  (`cases.rs` add `STR_CONCAT`), acceptance: not slower; and the typed side
  `M9-TS-178..182` stay green.

### A4. `platypus_js`: shared-runtime cost of frames (optional, small) **[shared runtime]**

- Goal: frame push/pop from 4.9 ns to ~4.0 ns per call (measured in the research copy).
- Files: `tools/platypus_ts_rt/src/lib.rs`: move `top: *mut Frame` out of `Heap` into
  `thread_local! { static TOP: Cell<*mut Frame> = const { Cell::new(null_mut()) } }` with
  `pub(crate) fn frame_top()` / `set_frame_top()`; `ts_rt_frame_push`/`ts_rt_frame_pop` and
  `collect` use them; `tools/platypus_ts_rt/src/coro.rs` `top()`/`set_top()` call them.
- Tests: the generator and async tests (`M9-TS-376..430`) in stress mode cover coroutine stacks.
- Benchmark: `platypus_js_bench rtbench` (prints "frame push+pop"), acceptance <= 4.3 ns; closures
  benchmark not slower.

## 3. Phase B: compiler improvements (both tiers)

### B0. A call-heavy benchmark for the typed tier

- Goal: a typed benchmark where calls dominate (the busy sim does not show them).
- Files: `tools/platypus_ts_tests/bench/calls.ts` (take `SP\deliver\tools\platypus_ts_tests\bench\calls.ts`:
  `export function particles(n)`, `export function grid(n)`), registered in `build.rs` with
  `bench/add_module.py` (prefix `calls_`, the next free struct base), a `pub mod calls` in
  `src/lib.rs`, a test `tests/all/calls.rs` (`M9-TS-1405`: `particles(20)` and `grid(2)` equal
  bun's results; produce them with `bun -e` importing the file; at research time `particles(2000)`
  was 3055575 and `grid(200)` was 686600), and a `--bin calls_bench` printing best-of-5 ms for
  `particles(2000)` and `grid(200)`.
- [platypus_ts] none (test crate only).

### B1. Frame elision for functions that cannot collect **[platypus_ts]**

- Goal: no `ts_rt_frame_push`/`pop` in functions that never allocate or call.
- File: `tools/platypus_ts/src/frames.rs` (take the delivered file): `plan()` returns an empty plan when
  `!f.body.iter().any(stmt_may_collect)`; `stmt_may_collect` = `stmt_may_alloc` or any
  `CallExtern`/`CallExternInto`/`CallExternOut`. Tests `M9-TS-1400`, `M9-TS-1401` in frames.rs.
- Why neutral: the collector only runs inside allocating calls; `codegen.rs` already skips the frame
  when `plan.total() == 0` and only mirrors locals with a slot. Output of every program is the same;
  stress mode is the proof.
- Benchmark: `calls_bench`: particles 44.7 → 22.4 ms measured; acceptance particles <= 26 ms;
  `cargo run --release -p platypus_ts_tests --bin bench` busy sim not slower than before (+5% allowed).
- Risk: an engine function that calls script synchronously would need a root; covered by counting
  every extern call as "may collect". Fallback: revert the commit.

### B2. x86-64-v2 code **[platypus_ts]**

- Goal: `Math.floor`/`ceil`/`trunc`/`round` as instructions.
- Files: `tools/platypus_ts/src/codegen.rs` (enable `has_sse3`, `has_ssse3`, `has_sse41`, `has_sse42`,
  `has_popcnt` on the ISA builder when `target_arch = "x86_64"`), test `M9-TS-1402` in `lib.rs`.
- Why neutral: the same IEEE results (`roundsd` is exact); only the instruction changes.
- Benchmark: `calls_bench` grid 52.8 → 22.7 ms (alone), 10.7 ms with B1; acceptance grid <= 13 ms
  with B1.
- Decision (made, the user may veto): compiled games require x86-64-v2 (2009+ CPUs; Windows 11 24H2
  requires it anyway). Record it in `docs/differences.md`? No: it is not a browser difference; add it
  to `docs/platypus-ts.md` "Pipeline" (codegen targets).

### B3. Cold slow paths **[platypus_ts]**

- Goal: exception exits, collector-reaching runtime calls and `fmod` fallbacks laid out of line.
- File: `tools/platypus_ts/src/codegen.rs`: `self.b.set_cold_block(block)` for the block made by
  `exc_target()`/`finish_exceptions()` and the slow block of `rem()`; same in
  `tools/platypus_js/src/jit.rs` for every `slow` block (`get`, `set`, `index`, `set_index`, `arith`,
  `compare`, `equal`, `truthy`, `call_value`, `call_method`, `tonum`).
- Test: none new (layout only); all tests green.
- Benchmark: `calls_bench`, `bench`, `sine_bench 2000`, `platypus_js_bench bench`; acceptance: none
  slower than 3%; keep the commit only if one is faster by >= 3%, else drop it and note the result
  in TODO.md.

### B4. Inlining of small direct calls **[platypus_ts]** (2-3 days; split into the commits below)

- Goal: remove call overhead for small methods and helpers; closes most of the gap to bun on
  `calls_bench`.
- New file `tools/platypus_ts/src/inline.rs`, run from `compile_inner` in `lib.rs` **before**
  `sroa::run` (so inlined value-class temporaries are scalarised), behind
  `pub fn run(m: &mut Module)`.
- B4a (commit 1): the candidate test `fn inlinable(m: &Module, f: FuncId) -> bool`: not exported,
  not `m.init`, not `m.may_throw[f]`, body has no `Loop`, `Try`, `Throw`, `Block` labels are fine,
  no `Call` of itself, at most 40 expression nodes (count with a walk like `frames::children`), no
  `MakeClosure`, no `Seq` containing `Return`. Unit tests `M9-TS-1406`: a getter, `isWall` and
  `outX` are candidates; a recursive function, one with a loop, an exported one are not.
- B4b (commit 2): the transform at each `ExprKind::Call(fid, args)` whose callee is a candidate:
  append the callee's locals to the caller's `locals` (offset `k`), rename the callee body's `VarId`s
  by `+k` and its `LabelId`s by a fresh range, then replace the call by
  `Seq([Assign(k + p_i, arg_i) for each parameter in order], Seq([Block(L, body')], Var(ret)))` where
  every `Return(Some(e))` becomes `[Assign(ret, e), Break(L)]` and `Return(None)` becomes
  `Break(L)`; `ret` is a new local of the callee's return type. Arguments are evaluated in order
  into locals, exactly as before the call. Callers repeat until no change or 3 rounds. Tests
  `M9-TS-1407`: IR dump of `particles` has no `call#` to `Particle.outX`; `M9-TS-1408`: results of
  `calls.ts`, `sim` and the classes/inherit modules unchanged (they are, through the existing
  tests); stress mode green.
- Why neutral: evaluation order and values are the same; GC roots are right because arguments now
  live in caller locals (rooted by the caller's frame if pointers) and `frames::plan` runs after.
- Benchmark: `calls_bench` particles acceptance <= 12 ms (estimate ~8 ms); busy sim not slower;
  `sine_bench 2000` not slower.
- Risk: code growth. Fallback: lower the node limit to 20; if results differ anywhere, revert.

### B5. Bounds-check elimination in counted loops **[platypus_ts]**

- Goal: `for (let i = 0; i < a.length; i++) { ... a[i] ... }` reads without the length compare.
- File: `tools/platypus_ts/src/codegen.rs` `Index` for `Ty::Arr` (and a pre-pass in `lower/stmt.rs`
  `lower_for` that marks the loop): when the loop test is `Lt(Var(i), Len(Var(a)))`, `i` starts at a
  non-negative integer constant, is only changed by `i++` at the end, and the body has no call, no
  `Rt` that mutates arrays (`ArrSetLen`, `ArrInsert`, `ArrDrop`, `ArrSplice`, push via `ArrPush` on
  `a`), and no assignment to `a`: emit an `ExprKind::IndexUnchecked` (new IR node: add it to
  `frames.rs` `children`/`count_expr` and `sroa.rs` `kids`/`kids_mut`) for `a[i]`.
- Tests `M9-TS-1409..1410`: the IR of such a loop uses `IndexUnchecked`; a loop that pushes to `a`
  does not; results unchanged.
- Benchmark: a sum over a 1e6-element `number[]` 100 times; acceptance >= 10% faster.

### B6. SROA of small script objects **[platypus_ts]**

- Goal: object literals of a declared shape that never escape a function become locals.
- File: `tools/platypus_ts/src/sroa.rs`: extend the candidate set from `value_structs` to any struct
  whose `NewObj` is assigned to a local that is only read by `Field` and written by `StoreField`
  (no other use), when the struct has only `Num`/`Bool` fields.
- Tests `M9-TS-1411..1412`: a `{x, y}` temporary in a loop allocates nothing (`ts_rt_allocated()`
  unchanged across the call); an escaping one still allocates.

### B7. Value classes by scalars across script calls **[platypus_ts]** (3-5 days, plan it first)

- Goal: `function add(a: Vector3, b: Vector3): Vector3` with no heap objects.
- Write a design note `docs/research/platypus-ts-value-abi.md` first (a STOP item per the tools TODO
  rules: the user reviews it). Do not code before the review.

### Done / not planned

- Devirtualisation of `CallVirtual`: already done by whole-program analysis (platypus-ts-classes.md).
- Profile-guided block ordering: no profiling infrastructure; B3 gives the cheap part.

## 4. Phase C: typed TypeScript compiled at load time (mods, dev loop)

### C1. Code generation over any Cranelift module **[platypus_ts]**

- Goal: the same code generator writes an object file or JIT memory.
- File: `tools/platypus_ts/src/codegen.rs`: change `Cg<'a, 'b>`'s `module: &'a mut ObjectModule` to a
  generic `M: cranelift_module::Module`; split `compile` into
  `pub fn compile_into<M: Module>(m: &Module, module: &mut M) -> R<Vec<CFuncId>>` (declares,
  defines, returns the function ids) and keep `pub fn compile(m: &Module, opt: &str) -> R<Compiled>`
  building the ISA and `ObjectModule` exactly as now and calling `compile_into`. Move ISA building
  into `pub fn host_isa(opt: &str) -> R<OwnedTargetIsa>`.
- Why neutral: `compile` does the same calls in the same order. Test `M9-TS-1413`: for every
  `bench/*.ts` module compiled by `build.rs`, the object bytes equal those of the parent commit
  (generate a list of SHA-256 sums with a small `--bin object_hashes` before the change, commit it
  as `tools/platypus_ts_tests/object_hashes.txt` in this step, and compare in the test).
- Risk: `declare_data_in_func`/`symbol_value` differ between modules? Both implement them.

### C2. Runtime symbol table **[shared runtime]**

- Goal: a JIT can resolve every `ts_rt_*` symbol the code generator imports.
- File: `tools/platypus_ts_rt/build.rs` (new): scan `src/*.rs` for `pub (unsafe )?extern "C" fn
  (ts_rt_[a-z0-9_]+)` and write `$OUT_DIR/symbols.rs` with
  `pub fn symbols() -> &'static [(&'static str, *const u8)]` listing `(name, name as *const u8)`;
  `include!` it in `lib.rs`. Test `M9-TS-1414`: every import name in the objects of `build.rs`'s
  modules that starts with `ts_rt_` is in `symbols()` (use the `object` crate, already a dependency
  of `cranelift-object`, to read the import table).

### C3. Engine shim symbol tables **[crates/platypus_api_macros]**

- Goal: a JIT can resolve the `#[platypus_api]` shims.
- File: `crates/platypus_api_macros/src/lib.rs`: for each namespace, also emit
  `pub fn __platypus_symbols() -> &'static [(&'static str, *const u8)]` with every generated
  `extern "C"` shim; `crates/platypus_api`: `pub struct SymbolTable` and a `symbols!` helper is not
  needed: a host collects the namespace functions it grants. Test `M9-API-400` in
  `crates/platypus_api/tests/`: a namespace's table lists exactly the manifest's symbols.

### C4. `platypus_ts::jit` **[platypus_ts, new module only]**

- Goal: `pub fn load(src: &str, manifests: &[String], symbols: &[(&str, *const u8)]) -> Result<Loaded, String>`
  with `pub struct Loaded { module: JITModule, funcs: HashMap<String, *const u8>, init: *const u8 }`;
  struct bases come from a global counter starting at 200,000, step 1,000.
- File: `tools/platypus_ts/src/jit.rs` (new), `cranelift-jit` dependency in `tools/platypus_ts/Cargo.toml`.
- Tests `M9-TS-1415..1418` in `tools/platypus_ts_tests/tests/all/jit.rs`: `sim.ts` loaded by `jit::load`
  gives the busy-sim checksum of the AOT build; a module using `Vector3` and `Time` through
  manifests runs; two modules loaded side by side keep separate struct ids; stress mode green.
- Benchmark: load time of `sim.ts` (lower + compile) printed; busy sim through the JIT within 5% of
  the AOT object.

### C5. Mods **[user]** (decision U1 first)

- If the user accepts "mods are TypeScript": `tools/platypus_mods` (new crate): `ModHost::load(dir,
  granted: &[&str]) -> Result<Mod, Vec<Diagnostic>>` concatenating the mod's `.ts` files in import
  order (`import` lines removed; a real module system is a later step), compiling with only the
  granted namespaces' manifests and symbols, running `init`, registering its component classes with
  a `NativeHost`. Tests `M9-TS-1419` and `M9-API-401..402` (a mod without the `Physics` grant fails
  to compile with a clear message; a mod component gets its hooks).

### C6. Reload of compiled code (dev loop) **[owner]** for the app part

- Library part (no owner needed): `platypus_ts_rt::host::NativeHost::swap_classes(new_table)` keeping
  instances whose class layout is unchanged (same `PROPS_<Class>` and field list), remaking others
  from their `@property` values. Tests `M9-TS-1441..1443`. The `platypus_app` watcher and call: a
  proposal note for the owner.

## 5. Phase D: the app runs compiled code **[owner]**

D1 a script-host seam over `QuickJsRuntime` + `ComponentHost` (trait or enum); D2 a native backend
(`NativeHost`, `ts_rt_timers_advance`/`ts_rt_microtasks_run`, scene/physics/audio already native);
D3 UI through `platypus_ui_native` for screen and world panels; D4 reload via C6. Write these as one
proposal note (`docs/research/platypus-app-native-host.md`) listing the exact `main.rs` sites from the
inventory (section 2.2) and stop; the owner implements.

## 6. Phase E: a complete dynamic tier **[user]** (only if untyped mods are wanted, decision U1)

In order, one commit each, every one with differential tests against QuickJS in `cases.rs`
(`M9-TS-1450..1499`): E1 `new` and constructor functions with `prototype`; E2 `class` (methods on
the prototype, `super`); E3 getters/setters on literals and classes; E4 `for-in`/`for-of` over
arrays, strings, objects (integer keys first); E5 destructuring, spread, rest, default parameters;
E6 template literals, labels, `finally`; E7 array methods with callbacks (`map`, `filter`,
`forEach`, `reduce`, `sort`) through `call_value`; E8 dynamic `JSON.parse`/`stringify`; E9 int32
values (the reserved tag) for counters and indices; E10 lazy per-function compilation (fixes the
load time: 347 ms for 6,000 lines today); E11 the engine API from dynamic code (argument
conversion per manifest); E12 `any`/`unknown` in typed code as `JsValue` (needs a design note
first: STOP item).

## 7. Phase R: removal (after D, and after C5 or E)

R1 can go now: `crates/platypus_api` `default = []`, tests that use `register_quickjs` enable
`features = ["quickjs"]` in their `[dev-dependencies]` (`platypus_engine_api`, `platypus_math`,
`platypus_scene`, `platypus_api` itself); `M9-API-403`: `cargo tree -p platypus_ts_rt` has no rquickjs. The rest
(delete `platypus_script`, the macro's QuickJS code, `js/*`, fixtures) needs the user **[user]** and
phase D done.

## 8. Decisions that need the user

| # | Question | Recommendation | Blocks |
|---|---|---|---|
| U1 | May mods be untyped JavaScript? | No: mods are TypeScript that passes `tsc --strict`, compiled at load (phase C) | C5, phase E, R11 |
| U2 | Compiled games require x86-64-v2 | Yes (made; veto possible) | B2 |
| U3 | Split the JS library into `platypus_js_std` with `platypus_ts_rt` re-exporting it | Not now; when the dynamic tier must build without engine crates | none |
| U4 | Value classes across script calls by scalars (design note) | Review the note when written | B7 |
| U5 | `platypus_app` work (D1-D4, C6 app part) | Hand the proposal to the owner | D, C6, R |
