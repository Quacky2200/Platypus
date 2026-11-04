# Debugging a game: breakpoints, pause and stepping

`platypus-app <game> --debug-port <port>` runs the game as a **debug build** and listens on `127.0.0.1:<port>` for a debugger
(the text protocol below). `--dap-port <port>` does the same speaking the **Debug Adapter Protocol**, for VS Code and other editors
([DAP](#editors-the-debug-adapter-protocol)).
Design and plan: [research/platypus-ts-debug-info.md](research/platypus-ts-debug-info.md).

## What a debug build is

The compiler puts a line marker before every statement (`Stmt::Line`, also in normal builds, for crash reports). A debug build
(`JitOptions::debug`) also turns each marker into a check: it loads one byte (`platypus_ts::debug`'s armed flag) and, when
that byte is set, calls the debugger's hook with the line. Nobody debugging means the byte is zero: the cost is a load and a branch
per statement, so debug builds are slower and never the default. Code the compiler writes itself (hook wrappers, facade members) has
no markers, so stepping never stops in it.

The hook (`debug.rs`) decides whether to stop: a breakpoint on the line, a pause request, or the step in progress. A stopped game
parks its thread until the controller resumes it. The game runs on the window's thread, so **a stopped game does not repaint or answer
the window**; resume it to use the window again.

Steps compare the stack pointer, so no call depth is tracked: a statement of the same function has the same stack pointer, a
callee a lower one, a caller a higher one. *Step in* stops at the next statement anywhere, *step over* at the next statement
of this function or a caller's, *step out* at the next statement of a caller.

## The controller protocol

Lines of text over TCP (`telnet 127.0.0.1 <port>` works); one reply line per command, and `stopped` lines whenever the game stops.

| Command | |
|---|---|
| `break <file>:<line>` | adds a breakpoint (file as in `game.pdebug`, e.g. `src/main.ts:20`) |
| `clear` | removes every breakpoint |
| `pause` | stops at the next statement |
| `continue`, `in`, `over`, `out` | resumes a stopped game |
| `where` | `ok stopped <file>:<line>` or `ok running` |
| `locals` | the stopped function's named locals as `name:type=value; ...` |
| `stack` | the stop and its callers as `function file:line; ...` |
| `detach` | clears everything and lets the game go |

Events: `stopped <file>:<line> <breakpoint|pause|step>`.

## Editors: the Debug Adapter Protocol

`platypus-app <game> --dap-port 4711` serves DAP on `127.0.0.1:4711` (`dap.rs`) and **waits for the editor's `configurationDone`**
before the game starts, so breakpoints sent while the editor connects are in place before the first statement. In VS Code, install the
folder `tools/vscode-platypus` (it only registers the `platypus` debugger type, see its README), add a `launch.json` entry
`{ "type": "platypus", "request": "attach", "debugServer": 4711 }` and press F5.

Supported requests: `initialize`, `attach`/`launch`, `setBreakpoints`, `setExceptionBreakpoints` (accepted, nothing to break on),
`configurationDone`, `threads` (one: the game's), `stackTrace` (the stop and its callers), `scopes`/`variables` (the innermost frame's locals),
`continue`, `next`, `stepIn`, `stepOut`, `pause`, `disconnect`. A `stopped` event follows every stop. Editor paths are matched to the
program's files by their ends (`game.pdebug`); a game run as one `.ts` file needs no sidecar (its lines are the bundle's).
Tests: `M9-TS-3398`, `M9-TS-3399`.

## Files, lines and hot reload

A breakpoint is a file and a line of that file. The bundled source joins all files into one text, so the host maps with
`build/game.pdebug` (written by `platypus-build build`, see [platypus-build.md](platypus-build.md)); without it, breakpoints
cannot be placed (the log says so), except for a game run as one `.ts` file. After **every** compile, including a hot reload, the breakpoints are resolved again
(`debugger::resolve_breakpoints`), so they follow a file's lines when other files change size, and the reloaded game is a debug
build again. A reload cannot happen while the game is stopped: the reload is handled by the thread that is parked.

## Locals and the call stack

A debug build gives every function a stack slot for its locals. On the hook path only (so nothing is paid while nobody is stopped),
the generated code stores each local there as raw 64-bit words and passes the slot and the function's number to the hook; the hook
reads them with the function's table (names from the lowering, types from the IR) and keeps the text of each in the `Stop`:
numbers (`3`, `0.5`), booleans, strings (quoted, cut at 200 characters), string-literal unions (the member), class instances and
arrays (the class name and address: they are not expanded), and a variable that a closure captured (its value, read through the
cell). A function with more than 256 locals (a big top-level `init`) is not spilled, so a stop in it has no locals.

The call stack is found by scanning the stack above the function's slot for return addresses into compiled code (Windows; elsewhere
a stop shows only its own frame), so it crosses the host's frames between two game frames; a stale word can show as an extra frame.
**Only the innermost frame has locals**: a caller's registers were not saved at its call.

## Not yet
- Callers' locals, and expanding objects and arrays.
- The editor's own debugger UI (`editor/`): it can speak DAP to a game the same way.
- Conditional breakpoints, watchpoints, breaking on a thrown exception.
- Stopping while the window stays responsive (pumping the event loop while parked).
