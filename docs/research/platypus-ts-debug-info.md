# Debug information and a debugger for platypus_ts (research, 2026-10-31)

Goal: when a compiled game crashes, the report names the TypeScript file, line and function, not
only an address. The mapping lives in an optional file beside the executable, so a shipped game
carries none of it. A debugger (pause, breakpoints) follows from the same tables.

## What exists today

- `crates/platypus_app/src/crash.rs`: a vectored exception handler (Windows) for access violations
  and heap corruption. It logs registers, code bytes and a Rust backtrace, and names the nearest
  *exported* compiled function (`jit_function_at`, from `Game::code_symbols`). It writes
  `crash-<secs>.txt` beside the log and starts the crash dialog.
- `tools/platypus_ts/src/jit.rs`: `compile_and_load(src)` lowers the bundled source **at run time**
  and compiles it with Cranelift. So the machine code is never in a build artefact, only the source.
- `build/game.pbundle` (JSON): `source` (all files joined by `modules.rs`), `files`. `Bundle::locate`
  already maps a line of the joined source to `file:line` (`Bundle::starts`), used for compile errors.
  The pbundle does not keep `starts`.
- The IR (`ir.rs`: `Stmt`, `Expr`, `Func`) has no source positions. The lowerer has
  `Lowerer::line(offset)` (bundled-source line, prelude lines removed).
- Cranelift supports this directly: `FunctionBuilder::set_srcloc(SourceLoc)` tags instructions, and
  after `define_function` the compiled code gives `(start, end, loc)` byte ranges relative to the
  function start (`ctx.compiled_code().buffer.get_srclocs_sorted()`).

## Design

Two layers, so the sidecar can be optional and the runtime cost is near zero.

1. **Line markers in the IR (always on).** A new `Stmt::Line(u32)` (line of the bundled source)
   goes before each lowered statement. Codegen turns it into `set_srcloc`. It emits no code.
   Cost: a few bytes of table per statement, kept in memory by the JIT.
2. **Address tables in the JIT (always on).** `generate` returns, per function symbol, the sorted
   `(code offset, bundle line)` ranges. `Game` keeps them next to `funcs`; `crash.rs` receives them
   with `note_jit_functions`. A fault address becomes `symbol+offset`, then a bundle line,
   by binary search. Local (non-exported) functions are in the table too, which fixes the "only a
   hint" caveat of `jit_function_at`.
3. **The sidecar (optional), `game.pdebug` beside the exe.** JSON: `format`, a hash of the bundled
   source, `starts` (bundle line to original file and line, the part of `Bundle` the pbundle drops),
   and optionally the original file texts for source excerpts in reports. `platypus-build build`
   writes it to `build/game.pdebug`; `package` writes `<name>.pdebug` beside `<name>.exe` only
   with `--debug-info`. The host looks beside the exe, then beside the pbundle. Without it, a report
   says `bundle line 1234` and the function name; with it, `src/enemy.ts:57`. A hash mismatch
   (sidecar from another build) is reported and ignored.
4. **Lines for the Rust frames stay with Rust.** Frames between the game and the fault are the
   existing backtrace; walking through JIT frames needs unwind info: Cranelift can emit it
   (`isa` flag `unwind_info`, registered with `RtlAddFunctionTable` on Windows), which lets
   `Backtrace` and the handler walk through compiled frames to get a *TypeScript call stack*.
   That is step 3 of the plan below.

Why `Stmt::Line` and not a field on every statement: statements are built in hundreds of places
in `lower/` (loops, iterators, regexp helpers); only `lower_stmts_in` knows the source statement.
Passes that pattern-match adjacent statements (`sroa.rs`, `frames.rs`, `throws.rs`) must skip `Line`;
the test suite (`PLATYPUS_TS_GC=stress` too) is the check that no optimisation regressed.

Columns: not in v1 (`SourceLoc` is 32 bits; line is enough). A later version can pack
`line << 12 | col` for expression-level positions of calls, which is what a debugger's step-into needs.

## Step 3 result (2026-11-04)

- Unwind tables are registered (`unwind.rs`): the system has entries for every compiled function. Cranelift's JIT
  frames are `push rbp; mov rbp, rsp` with correct codes.
- Walking the stack with `RtlVirtualUnwind` from the crash handler worked in some builds of a test exe and faulted
  inside ntdll (a read of -1, while unwinding a host frame after the game frames) in others: it depended on the build, not
  the run. Not understood (TODO 3b). The crash report therefore scans the stack for return addresses into compiled code
  (`unwind::stack_words`): it cannot fault, and it crosses host frames.
- Verified end to end with a small game that dereferences null three calls deep: the report lists `deref`, `middle`, `outer`
  and `Boom.onStart` with `src/main.ts` lines 7, 11, 15 and 20.
- The top-down game (`examples/zombie-horders`) did not crash at start-up, nor when started automatically and paused and
  resumed ~125 times in screenshot runs, also with `PLATYPUS_TS_GC=stress`. The old crashes were not reproduced, so this
  gave no debug information about them.

## Debugger (later)

Built on the same tables plus:

- **Breakpoints:** a `Line` marker becomes a check only in a debug build (`--debug`): load a
  global "breakpoint armed" byte and call `ts_rt_break(line)` when set. Debug builds are slower and
  never the default. The alternative, patching `int3` into live code, needs the table above to find
  instruction addresses and a vectored handler for `EXCEPTION_BREAKPOINT`; it works on release code
  but cannot show locals without the variable map below.
- **Pause:** `ts_rt_break` blocks the game thread on a channel to the debugger and keeps pumping
  nothing else. The host stays alive and serves the debugger.
- **Locals:** a table per function, local index to name, type and shadow-stack slot
  (`frames.rs` already mirrors pointer locals there; number locals live in registers, so debug builds
  must spill every local at each `Line`).
- **Protocol:** speak the Debug Adapter Protocol over a local socket so VS Code (and later the
  editor in `editor/`) connect without a custom client. Stepping is "break at the next `Line`".
- **Hot reload:** a reload recompiles; breakpoints are kept by file and line and re-armed.

## Plan

1. `Stmt::Line`, codegen srclocs, address tables in `Game`, crash report prints `bundle line`.
2. `game.pdebug` sidecar (build, package `--debug-info`), report prints `file:line`, source excerpt.
3. Unwind info for JIT frames, TypeScript call stack in reports.
4. Debug build mode with spills and the break hook; pause / resume / step.
5. DAP server; editor integration.

Open questions: whether packaged builds should keep tables in memory when no sidecar exists (cheap,
yes for now); column support; macOS/Linux fault handlers (`crash.rs` is Windows only).
