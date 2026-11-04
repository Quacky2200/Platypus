# Crashes: reports, the crash dialog and `Application.crash`

A guide for game makers: what happens when a Platypus game crashes, where the report goes, what the player sees, how a game
can end itself with `Application.crash`, and how to read a report. How it is built is in
[platform.md](platform.md#crash-report-windows).

## What happens

1. The game hits an error it can't survive: an access violation in the game's program or in its compiled TypeScript, heap
   corruption, or a call to `Application.crash`.
2. Platypus writes what it knows to the log (`Player.log`) and saves a crash report, `crash-<time>.txt`, in the same folder.
3. It starts a second copy of the game's program that only shows the crash dialog, and the game's process ends. The dialog
   never runs inside the broken process: its memory may be damaged, and a dialog drawn there could crash too.
4. The player sees Platypus's crash dialog. If that one can't run (no working GPU, a broken install, or it crashes itself), the
   system's plain message box shows instead.

A Rust panic is not a crash in this sense: it ends the game with its message in the log, as before.

## The dialog

**Platypus's dialog** is a small window drawn by Platypus itself (a built-in TSX program, `crates/platypus_app/src/crash_dialog.tsx`):

- the title, "*Game* stopped unexpectedly", and a line saying a report was saved;
- the reason (for example `Access violation reading 0x28 at 0x7ff6… (in the game's program)` or
  `The game called Application.crash: save file is unreadable`);
- the whole report in a scrolling box;
- the report's path, **Open report folder** (shows the file in the file browser) and **Close**.

**The plain dialog** is the system's message box (Windows `MessageBoxW`) with the same summary and the report's path.
**Yes** opens the folder that has the report. On other systems the plain fallback is a message on stderr.

## Where reports go

| Run | Report folder |
|---|---|
| A normal run | Beside `Player.log`: the game's settings folder, e.g. `%APPDATA%\<company>\<title>\` on Windows |
| `--screenshot` / `--bench`, or no log file | The system's temp folder |

Each report is a plain text file named after the time it was written (Unix seconds):

```text
Platypus crash report
Game: Zombie Horders
Reason: Access violation reading 0x28 at 0x7ff698893242 (in the game's program)
Time: 1793151215 (Unix seconds)
Log: C:\Users\you\AppData\Roaming\Platypus examples\Zombie Horders\Player.log

crash: exception 0xc0000005: Access violation reading 0x28 at 0x7ff698893242 (in the game's program) on thread "main"
registers: rip 0x7ff698893242 rsp ... rbp ...
  rax ... rbx ... rcx ... rdx 0x10
  rsi ... rdi 0x10 ...
code [rip-96, rip+16): 48 89 cf ...
   0: ...
   8: winit::platform_impl::windows::event_loop::EventLoopThreadExecutor::in_event_loop_thread
   ...
```

Only the first report of a process is saved: a second fault while reporting adds nothing.

## Ending a game with `Application.crash`

```ts
Application.crash(message?: string, plain?: boolean): void
Application.crash({ message?: string, plain?: boolean }): void
```

It ends the game at once, the same way an unexpected error does: the log, a report, the dialog, and the process exits with
code **3**. `message` becomes the report's reason (`The game called Application.crash: <message>`). `plain` shows the plain
dialog instead of Platypus's own.

A game should normally end with `Application.quit(exitCode)`. `crash` is for errors the game can't recover from, where a
report and a dialog help the player more than a window that silently closes:

```ts
const save = PlayerPrefs.getString("save", "");
if (save !== "" && !isValidSave(save)) {
  Application.crash("the save file is damaged");
}
```

Nothing after the call runs: there is no exception to catch and no `onDestroy`.

### Trying the dialogs

A component that crashes a second in, with `-- plain` choosing the plain dialog (arguments after `--` reach
`Application.args`):

```ts
class Crasher extends Component {
  private frames = 0;

  override onUpdate(): void {
    this.frames += 1;
    if (this.frames === 60) {
      Application.crash("testing the crash dialog", Application.args.includes("plain"));
    }
  }
}

register({ Crasher });
```

```bash
cargo run --release -p platypus_app -- path/to/game
```

```bash
cargo run --release -p platypus_app -- path/to/game -- plain
```

The dialog can also be shown again for any saved report, without a game (`--plain` for the plain one):

```bash
build/release/platypus-app.exe --crash-report "C:\Users\you\AppData\Roaming\<company>\<title>\crash-1793151215.txt"
```

## When no dialog shows

- **Screenshot and bench runs** (`--screenshot`, `--bench`): nobody is watching, so the report is saved (in the temp
  folder) and the run ends without a dialog.
- **A crash inside the crash dialog** (`PLATYPUS_CRASH_DIALOG` is set in its process): it is logged, nothing more opens.
- **A fault outside the game's code** (an access violation in a graphics driver, say): it is logged, but no report or
  dialog follows, because the driver may handle it and carry on. If the process does die from it, the log has the details.

## A panic that aborts the process

A call from compiled TypeScript into the host cannot unwind a Rust panic: Rust prints "panic in a function that cannot
unwind" and aborts. No exception handler sees that, so the game used to vanish with only log lines (the panic's own line
and the abort's). The panic hook (`logging.rs`) now hands each panic to `crash::note_panic`; the abort's panic saves the
report and shows the dialog, with the first panic's message, location and backtrace as the reason and details. Example: a
game that adds a component of a class it never registered.

## Reading a report

- **Reason**: what went wrong and where. *in the game's program* means the exe (Platypus's Rust code). *in compiled
  TypeScript, near `g_Arena_onUpdate+0x1c4`* names the compiled TypeScript function the fault is in, with the offset into it
  (`crashHere+0xd`, `lambda80+0x105`). Every compiled function is in the table, local ones too; closures show as
  `lambdaN`. A fault in compiled code always gets a report and the dialog.
- **Where in the TypeScript** (`at src/main.ts:7`): the line of the game's source the fault's code came from. It needs the
  debug file `platypus-build build` writes as `build/game.pdebug`; the host reads it from `<exe>.pdebug` beside the exe, else
  `build/game.pdebug` in the game folder. A shipped game has it only if it was packaged with `--debug-info`
  ([platypus-build.md](platypus-build.md), "Debug info"); without it the report says `bundled source line N`, a line of the
  one text all the game's files are joined into. A file from another build (its source hash differs) is ignored, and the
  report says so. Lines in code the compiler generated (hook wrappers) can be past the end of a file.
- **Call stack**: the compiled functions the fault is inside, innermost first, each with its line, for example
  `deref at src/main.ts:7`, `middle at src/main.ts:11`, `outer at src/main.ts:15`, `Boom.onStart at src/main.ts:20`. It is
  found by scanning the faulting thread's stack for return addresses into compiled code, so it also crosses the host's
  frames between two game frames, and a stale address left on the stack can show as an extra entry. Windows x64 only.
- **Reading or writing a small address** (`reading 0x28`) is a null pointer plus a field offset.
- **Registers**: which register held the bad pointer. In the example above `rdi` is `0x10`, the `0x28` read is
  `rdi + 0x18`, so the code was handed a null `Arc`: the value had been zeroed.
- **Code bytes**: the 96 bytes before the faulting instruction and 16 from it, for a disassembler. Left out when the fault
  was jumping to a bad address.
- **Backtrace**: the Rust frames of the faulting thread. Its walk may stop at the first game frame
  (the call stack above is the dependable way through the game's frames). A release build only names frames well with line tables:

```bash
CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --release -p platypus_app
```

  That rebuilds every release crate; build again without it afterwards.

### Chasing a crash that only happens sometimes

- Run the same start many times with `--screenshot out.png --frames 5` and count the reports in the temp folder.
- `PLATYPUS_TS_GC=stress` collects garbage at every allocation, so a missing GC root crashes at once instead of now and then.
- Forced hot reloads during a screenshot run: `PLATYPUS_RELOAD_AT_FRAME=N`.
- [AUDIT-UNSAFE-CODE.md](AUDIT-UNSAFE-CODE.md) (local, not in git) lists the code that can corrupt memory, for a review.

## Command line and environment

| Name | What |
|---|---|
| `--crash-report <file>` | Shows the crash dialog for a saved report, then exits (what a crashing game starts). |
| `--plain` | With `--crash-report`: the system's plain dialog only. |
| `--crash-dialog <folder>` | Runs the dialog's own game folder (the supervisor writes it to the temp folder); works in a packaged exe and the editor too. Not for direct use. |
| `PLATYPUS_CRASH_DIALOG` | Set in the dialog's process: a crash there opens no further dialog. |
| `PLATYPUS_LOG_FILE` | The log file; the dialog's process logs into its temp folder. |
| `PLATYPUS_TS_GC` | `stress` or `off`: the garbage collector's mode, for chasing memory bugs. |

## Platforms

| | Windows | macOS, Linux |
|---|---|---|
| Faults (access violation, heap corruption) logged and reported | yes | no (the process ends with the system's message) |
| `Application.crash`: report and dialog | yes | yes |
| Platypus's dialog | yes | yes |
| Plain fallback | message box | stderr |
| Open report folder | Explorer with the file selected | `open -R` (macOS), `xdg-open` on the folder (Linux) |
