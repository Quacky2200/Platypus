# Window, display, frame pacing and saved settings

How the window's mode, vsync, the frame cap and `PlayerPrefs` work. Code: `platypus_platform`
([display.rs](../crates/platypus_platform/src/display.rs), [platform.rs](../crates/platypus_platform/src/platform.rs)),
`platypus_render` ([surface.rs](../crates/platypus_render/src/surface.rs)), `platypus_app`
([display.rs](../crates/platypus_app/src/display.rs), [prefs.rs](../crates/platypus_app/src/prefs.rs),
[logging.rs](../crates/platypus_app/src/logging.rs),
[main.rs](../crates/platypus_app/src/main.rs)) and the native script API (`Screen`, `PlayerPrefs`,
`Application`, `QualitySettings` in `platypus_engine_api`; the `@platypus/engine` TypeScript is deleted).

## Window modes

`FullScreenMode` has Unity's four modes (scripts use the names):

| Mode | What the window does |
|---|---|
| `ExclusiveFullScreen` | The display switches to the chosen size and refresh rate |
| `FullScreenWindow` | A borderless window covering the monitor at its own resolution |
| `MaximizedWindow` | A normal window, maximized |
| `Windowed` | A normal window of the chosen size |

`DisplaySettings` is the mode plus width, height (physical pixels) and refresh rate (Hz, 0 means
"the monitor's rate"). The default is windowed, 1280x720. A width and height of 0 mean "no size asked
for"; `window_width` / `window_height` remember the last windowed size.

**Platform rules.** `supported_modes` lists what the platform offers, most fullscreen first:
all four on desktop, three on Wayland (compositors own the display mode, so no exclusive
fullscreen), and only `FullScreenWindow` on iOS and Android. `effective_mode` maps an unsupported
request to the nearest mode: fullscreen requests try `FullScreenWindow` then `MaximizedWindow`,
window requests try `MaximizedWindow` then `FullScreenWindow`. Scripts can read the list from
`Screen.supportedFullScreenModes` and show only those.

**`display::apply`** puts a window in a mode and returns what took effect:
- Exclusive: `exclusive_target` gives the size and rate asked for, or with no size the monitor's current
  resolution and rate (not the smallest mode). `closest_mode` then picks the video mode nearest to it
  (size first, then refresh rate; with no rate asked for, the fastest). The result carries that mode's
  real size and rate. If the monitor reports no modes it falls back to borderless.
- Windowed: `windowed_target` gives the size asked for, or with none the remembered window size. It is
  clamped between 320x240 and the monitor, then requested with `request_inner_size`. So leaving
  fullscreen goes back to the previous window size (`M5-DSP-04`).
- Borderless and maximized ignore the size.

`resolutions` lists the monitor's video modes, largest and fastest first, without repeats.
`current_resolution` is the monitor's size and rate now.

## How settings flow

1. At startup `main` reads the manifest's `platypus.display` (`{ mode, width, height }`) as the defaults,
   then lets the saved `platypus.display` pref override them field by field (`display::from_json`;
   unknown mode names and sizes below 1 are ignored). The window is created at the resulting size.
   With `resizable: false` in the manifest's `platypus.display`, the window opens at the manifest's
   own size (saved sizes are ignored) and can't be resized (`WindowConfig::resizable`).
2. The display state is built before the game starts (`display::state`). It holds the settings
   plus `supportedModes`, `resolutions` and `currentResolution`; before the window exists the last
   two are empty/null and all four modes are listed. It is rebuilt once the window
   is ready and after every `screen.set`. (The QuickJS `__platypus_info` script that carried it is gone.)
3. `window_ready` applies a non-windowed mode **before the first frame**, so the window never
   flashes at the wrong size (it is created hidden and shown after its first frame).
4. At run time `Screen.setResolution(w, h, mode, refreshRate)` asks the host (`screen.set mode width height
   refreshRate`). The `fullScreenMode` and `fullScreen` setters send a size of 0, so the app picks the
   monitor's resolution or the last window size (see above). The app applies it, writes what was asked for to the `platypus.display` pref and
   rebuilds the display state with what took effect.
5. **Screenshot runs** (`--screenshot`) stay windowed and never apply a mode, and their prefs are
   kept in memory, so renders are the same every time.

**Centred main window.** The main window opens centred on the primary monitor (else the one it is on) when it is
windowed. `display::centered_position` uses the window's outer size and keeps the title bar on screen when the window is
bigger than the monitor. Fullscreen and maximized windows and screenshot runs are left alone (`display::center`,
`WindowConfig::center`; `M5-DSP-05`). Secondary windows are unchanged.

`Screen.width`/`height` (UI pixels, i.e. logical pixels) and `Screen.windowSize` (physical pixels) come from the host's screen state.

## Third-party notices

`platypus-app --licenses` prints `THIRD-PARTY-NOTICES.md`, built into the program (`licenses.rs`, `include_str!` of the
repository root's file), and exits 0. It is checked first thing in `main`, before the log, a window or a packaged game's
payload, so it works on any exe made from the host; on Windows it joins the parent's console for stdout. The file
itself is generated by `tools/platypus_notices` ([platypus-build.md](platypus-build.md), "Third-party notices").

## Log file

`platypus-app` is a Windows GUI-subsystem program, so it has no console of its own. `logging::init` sends the log
(`tracing`, default filter `info,wgpu_core=warn,wgpu_hal=error`, `RUST_LOG` overrides it) to `Player.log`
in the game's settings folder, next to `prefs.json`, with the same ANSI colours a terminal shows. The
previous run's file is kept as `Player-prev.log`. `PLATYPUS_LOG_FILE` names another file. Started from a
terminal, it also logs there: on Windows `attach_console` attaches to the parent's console when stderr
is not already redirected, and piped output (scripts, tests) works as before. A panic hook writes the
panic to the log (`M5-APP-04`).

### Crash report (Windows)

A crash that is not a Rust panic (an access violation in compiled game code, the runtime or a driver) used to end the
process with only an exit code. `crash::install` (`crates/platypus_app/src/crash.rs`, called first in `main`) adds a vectored
exception handler that logs an access violation (`0xC0000005`) or heap corruption (`0xC0000374`) before the process dies.
Other exceptions are ignored. The report goes to stderr and `Player.log` and has:

- the exception code, the address and what the access was (reading, writing or executing which address);
- the compiled game function (`name+offset`, e.g. `crashHere+0xd`, `lambda80+0x105`) when the address is in JIT code.
  `native.rs` registers every compiled function after each compile (`note_jit_functions`, from `Game::code_symbols()`),
  local ones too; closures show as `lambdaN`, so a fault in compiled code always gets a report and the dialog;
- the TypeScript file and line of the fault (`at src/main.ts:7`) and the game call stack, from the JIT's line table
  (`note_jit_lines`) and the optional `game.pdebug` file; see [crashes.md](crashes.md#reading-a-report);
- the general registers;
- 112 bytes of machine code around the instruction (`rip-96` to `rip+16`), for a disassembler. Left out when the fault was
  executing a bad address, since the code is not readable then;
- a Rust backtrace.

A release build names frames well only with line tables: build with `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only`. Pair it
with `PLATYPUS_TS_GC=stress` ([platypus-ts.md](platypus-ts.md#running-a-compiled-game-in-the-process-jit)) to make a missing GC root
crash at once. On other platforms the exception handler is not installed (`Application.crash` still works).

#### Crash report file and dialog

The game maker's guide (the dialog, reports, `Application.crash`, reading a report) is [crashes.md](crashes.md).

When the fault is the game's own (the address is in the exe or in compiled game code, or it is heap corruption), the
handler also saves a report and starts a dialog. A fault elsewhere (a driver) is only logged, since its code may handle it.
`Application.crash(message?, plain?)` (listed with `Application` in [scene-and-objects.md](scene-and-objects.md)) takes the same
way and then exits with code 3.

1. `crash::set_reports` (called in `main` once logging has started) gives the report folder, the game's name, the log path
   and whether a dialog shows. The folder is the one with `Player.log` (the temp folder when there is no log).
2. `crash-<unix seconds>.txt` is written there: the game, the reason, the time, the log path and the details (the
   registers, code bytes and backtrace above, or the `Application.crash` message and a backtrace). Only the first report of
   a process is saved. Screenshot and bench runs save reports but show no dialog.
3. The process starts `<exe> --crash-report <file> [--plain]` with no stdio and dies. The dialog never runs in the broken
   process (`crash_dialog.rs`).
4. That supervisor process writes a small game folder to the temp folder: `project.json` and `build/game.pbundle`, whose
   source is the constants `GAME`, `REASON`, `DETAILS` and `REPORT_PATH` followed by `crash_dialog.tsx`. It runs the folder
   with `<exe> --crash-dialog <folder>`, with `PLATYPUS_CRASH_DIALOG=1` (so a crash inside it opens no dialog) and
   `PLATYPUS_LOG_FILE` in that folder. `--crash-dialog` runs the folder even from a packaged exe or the editor.
5. The dialog's exit code decides: 0 closes, 10 opens the report's folder (`explorer /select`). Any other ending (it
   crashed, there is no GPU) or `--plain` shows the system's message box instead: `MessageBoxW` on Windows (Yes opens the
   folder), stderr elsewhere. The folder is removed afterwards.

Tests: `M9-API-917` (`--crash-dialog <folder>` parses), `M9-API-918` (the dialog reads a report's game and reason), `M9-API-919` (`crates/platypus_app/tests/crash_report.rs`: a fault in compiled code writes a report naming the function).

## Vsync and the frame cap

- `QualitySettings.vSyncCount` (0 or 1) goes out as the seventh argument of `render.quality`. It
  fills `Quality.vsync` (default on: the quality levels and `Quality::default()` have `vSyncCount` 1; a `render.quality` op
  without the argument means on).
  A saved `platypus.quality` keeps its vsync only when it has `playerVSync: true`; every save writes that now (the startup
  prompt too). Older saves, made while vsync was off by default, take the level's vsync.
- The app calls `WindowSurface::set_vsync` before each frame with `vsync && !offline`, so screenshot runs never wait for
  vsync (`--bench` draws to an offscreen target and is unaffected). It switches the present mode between `AutoVsync` and
  `AutoNoVsync` and reconfigures the surface only when the mode changes. In a window, frame rates are now capped by the
  display unless the player or game turns vsync off. wgpu picks what the platform has (relaxed or plain FIFO;
  mailbox or immediate).
- `Application.targetFrameRate` sends `app.targetFrameRate` (-1 means no cap). Like Unity's, it only
  counts with vsync off, since vsync already paces frames. Screenshot runs ignore it.
- The cap is `PlatformApp::frame_interval`. After handling events, the loop in `about_to_wait`
  checks the time since the last frame started; if the next frame is not due it sets
  `ControlFlow::WaitUntil(due)` and does not request a redraw. Input events still wake the loop.
  Otherwise it sets `ControlFlow::Wait` and requests a redraw.
- The target frame rate is not saved; games keep it in `PlayerPrefs` if players choose it.
- `lifecycle::frame_cap(target_fps, vsync, screenshot)` makes the interval: `None` unless vsync is off,
  the rate is above 0 and it is not a screenshot run (`M5-APP-08`).
## Startup video-settings prompt

A game can ask the player once, before its window opens, for the window mode, resolution, vsync and quality level. Code:
[video_prompt.rs](../crates/platypus_app/src/video_prompt.rs), [video_prompt.tsx](../crates/platypus_app/src/video_prompt.tsx),
[video_prompt_sizes.ts](../crates/platypus_app/src/video_prompt_sizes.ts).

**When it shows.** It is opt-in. The sources, strongest first:
1. The command line: `platypus-app --video-settings-prompt` forces it (also after a choice, and in a screenshot run, where the
   PNG is the dialog's and the game does not start); `--no-video-settings-prompt` turns it off.
2. The build argument `platypus-build package --video-settings-prompt` / `--no-video-settings-prompt`, which writes
   `videoSettingsPrompt` into the exe's embedded `project.json` over the game's own.
3. `videoSettingsPrompt` in `project.json`.
4. Off.

It shows once: the game's prefs hold `platypus.videoSettings.chosen` after Play. It never shows in screenshot, `--bench` or
`--cook` runs unless forced, and not in the editor.

**How it is hosted.** `main` runs the prompt before anything of the game's window exists, as a child process of the same exe:
`<exe> --video-settings-dialog <folder>`, with a small game folder written to the temp folder (like the crash dialog above).
A child is needed because winit allows one event loop per process, and the dialog's window and GPU context are gone before the
game's are made. The child is UI only: its bundle says `"renderer": "ui"`, so the host takes its light path (GLES through
the linked-in ANGLE, else wgpu with small allocations; [rendering.md](rendering.md#choosing-the-renderer)); the world renderer
is never built. The crash dialog's bundle says the same. The folder's source is constants
prepended by `video_prompt.rs` followed by `video_prompt.tsx`.

**The dialog.** A fixed 600 x 500 window (300 tall without the mode and resolution rows; `resizable: false`, so no resize
edges or maximize button), centred on the screen like the game's, its content a centred column:
- A header with the game's title, and its `subtitle` in smaller, dimmer text under it when it has one (centred text; no
  image). The window's title and its title-bar / taskbar icon are the game's: `Manifest::window_title` (title, else name,
  else "Platypus") and `Manifest::window_icon` (the project's icon, else Platypus's), the same values the game's own window
  uses. The host writes the icon as `icon.png` into the dialog's folder (its `project.json` icon); the title and subtitle
  come as constants.
- Window mode: Windowed, Borderless or Fullscreen.
- A resolution list box. The modes are the monitor's, from winit through `Screen.resolutions`, de-duplicated and largest first
  by area. Refresh rates show only for exclusive fullscreen. Windowed lists only sizes smaller than the monitor. The saved size
  is selected, else the nearest. The list takes the wheel, Up/Down/PageUp/PageDown/Home/End, the d-pad and clicks, and scrolls
  the selection into view. Borderless shows the monitor's own size. The pure helpers are in `video_prompt_sizes.ts`
  (its Bun test was deleted with the JS test setup).
- Vsync On/Off (default on).
- Quality level (default: the saved one, else the manifest's default).
- Play and Quit. Esc, gamepad B or closing the window is Quit.

A fixed-size game (`display.resizable: false`) gets no mode or resolution rows, and its display prefs are not written.

**Result.** Play exits with code 10 and the answer travels in the dialog's own `PlayerPrefs` (a temp `PLATYPUS_PREFS_DIR`).
`Choice::apply` writes `platypus.display`, `platypus.quality` (`{level, current: {vSyncCount}, playerVSync: true}`; saved
per-setting changes are kept only when the level is the same) and the chosen key, and then the game starts through its normal
path. Quit exits 0: the game does not start and nothing is recorded. A failure (no GPU, a compile error) logs a warning and
starts the game without recording anything.

**From a running game.** `Application.showVideoSettings()` opens the same dialog over the game (a game's own "Video
settings..." button; the arcade's settings menu has one). It returns at once (a `ShowVideoSettings` request,
`native_pump.rs`); the host starts the same UI-only child (`video_prompt::Running`, with `STARTUP` false: the buttons say
Apply and Cancel, and Cancel never quits the game) and polls it each frame and while idle. The game pauses meanwhile
(`Lifecycle::set_modal`, even with `runInBackground`): `Application.isPaused` is true and the pause hooks run as for the
background pause. Apply puts the choice into effect the way scripts would: the window's mode and size through
`apply_display` (saved as `platypus.display`; not for fixed-size games), then `QualitySettings.setQualityLevel` and
`vSyncCount` (saved as `platypus.quality` with `playerVSync`), marks the prompt answered and runs the
`Application.onVideoSettingsChanged` listeners; the game window gets focus back. Cancel, Esc or closing the dialog changes
nothing. Screenshot and bench runs, a dialog already open and a dialog that fails to start only log a warning.
Tests: `M9-VSP-01`..`16` (`TESTS.md`).

## Focus and pausing

[lifecycle.rs](../crates/platypus_app/src/lifecycle.rs) holds `Lifecycle`, which decides whether the game is
in the background and whether that pauses it.

- **Background** means unfocused (`InputEvent::Focused(false)`), occluded (`InputEvent::Occluded`, sent on
  platforms that tell, such as macOS and Wayland) or sized to 0 x 0 (how Windows reports a minimised
  window). `App::input` feeds those three into `Lifecycle`.
- **Pausing** happens in the background unless `runInBackground` is on (`Application.runInBackground`,
  which sends `app.runInBackground`, or `runInBackground` in `project.json`; default off). Screenshot
  runs never pause. Switching `runInBackground` off while in the background pauses at once.- `Lifecycle::update` is called from `App::sync_lifecycle` (after each input event and op) and returns a
  `Change`: `paused`, `resumed` (with how long the pause lasted) and `report`, the new
  `(focused, paused)` for scripts when either changed.
- **While paused**, `App::tick` returns early after polling the hot reload watchers: no fixed ticks,
  updates, timers or animation. `Audio::set_muted(true)` fades the main track to silence over 0.1 s
  without losing the master volume scripts set (a mixer opened during a pause starts muted).
  `PlatformApp::paused()` is true, so the event loop requests no frames and waits; it calls
  `PlatformApp::idle()` every `IDLE_POLL` (250 ms), which polls script hot reload so rebuilds show on resume.
- **On resume**, `started` moves forward by the pause, so script time (timers, caret, UI time) skips it,
  and `FixedClock::restart` makes the first frame count as one tick instead of catching up. `FixedClock`
  turns frame times into fixed ticks (at most `MAX_TICKS_PER_FRAME`, dropping the rest after a hitch).
  The mixer unmutes.
- **Scripts.** The host (`native_pump.rs`) updates `Application.isFocused` and `isPaused`, then calls
  `onApplicationPause(paused)` on active components when pausing changed, then the `onFocusChanged`
  listeners and `onApplicationFocus(focused)` when focus changed. A throwing listener doesn't stop the
  rest. New scripts after a reload start believing they are focused and running
  (`scripts_restarted`), so the next update tells them the real state.

Tests: `M5-APP-05` (window events), `-06` (muting, in `platypus_audio/tests/audio.rs`), `-07` (`Lifecycle` and
`FixedClock`), `-08` (the frame cap) and `-09` (manifest) in the app crate (`-10`, the JS side, was removed with QuickJS).

## System theme

`prefers-color-scheme` follows the system's light/dark preference, live, in every window
([theme.rs](../crates/platypus_platform/src/theme.rs)). Each frame the app asks `Manifest::dark`
(`info.rs`), which the main frame (`main.rs`), the light GLES/CPU frames (`light_media_env`) and the
secondary windows (`draw_windows`) put in the UI's `MediaEnv`.

- **`project.json` `theme`**: `system` (the default), `light` or `dark`. A forced theme applies on every
  platform and window, also to the offscreen windows of screenshot runs (light otherwise); the system is
  then never asked.
- **Windows and macOS**: winit's `Window::theme()`.
- **Linux**: winit reports no system theme (Wayland gives back only the theme the app set, X11 nothing),
  so `Platform::run` calls `theme::watch` before the event loop starts. Its thread (`platypus-theme`,
  `zbus` blocking, a Linux-only dependency) connects to the session bus and asks the XDG Settings portal
  (`org.freedesktop.portal.Desktop`, `/org/freedesktop/portal/desktop`,
  `org.freedesktop.portal.Settings`) for `org.freedesktop.appearance` `color-scheme` with `ReadOne`, else
  the older `Read` (whose reply is wrapped in one more variant): 1 dark, 2 light, 0 (or anything else) no
  preference. Without a bus or a portal answer it runs
  `gsettings get org.gnome.desktop.interface color-scheme` once (`'prefer-dark'`, `'prefer-light'`,
  `'default'`); with neither, winit's answer is used as before. Start-up waits up to 300 ms for the
  first answer, so the first frame has it; a later one counts as a change.
- **Changes**: the thread subscribes to `SettingChanged` (filtered on the namespace and key) before it
  reads, stores each new scheme in a process-wide atomic and wakes the loop through an
  `EventLoopProxy<()>`. `Runner::user_event` requests a redraw (as winit's `ThemeChanged` would need),
  and the next frame restyles the main window and then the secondary ones. Only the portal gives live
  changes; a `gsettings` answer is read once.
- The mapping (portal number, nested variants, `gsettings` text) and the stored scheme are plain code,
  tested on every platform; the portal calls themselves only run on Linux.

To see what the portal answers: `gdbus call --session --dest org.freedesktop.portal.Desktop
--object-path /org/freedesktop/portal/desktop --method org.freedesktop.portal.Settings.ReadOne
org.freedesktop.appearance color-scheme`.

Tests: `M2-THEME-01` (mapping), `-02` (the stored scheme), `-03` (`project.json` `theme`).

## PlayerPrefs

`PlayerPrefs` has `getInt`/`getFloat`/`getString` (with a fallback; a key of the wrong type gives the
fallback, ints are truncated), `setInt`/`setFloat`/`setString`, `hasKey`, `deleteKey`, `deleteAll`
and `save`. Values are numbers and strings.

- **File.** One JSON object per game, `prefs.json`, in the OS settings folder:
  - Windows: `%APPDATA%\<company>\<title>\`
  - macOS: `~/Library/Application Support/<identifier, or company.title>/`
  - Linux and others: `$XDG_CONFIG_HOME/<company>/<title>/` (or `~/.config`)

  Names come from the manifest and are made file-name safe. `PLATYPUS_PREFS_DIR` replaces the folder
  (tests, portable installs). With no settings folder, or on screenshot runs, prefs live in memory
  only.
- **Read.** The app loads the file at startup and keeps it in a map, so reads do not touch the disk. A file that is not a JSON
  object is ignored with a warning.
- **Write.** Sets and deletes go to the app (`prefs.set key value`, `prefs.delete key`,
  `prefs.deleteAll` and `prefs.save`). The file is written on `save()` and when the `App` is dropped
  (so on exit). `Prefs::save` skips the write when nothing changed and writes through a temporary
  file and a rename, so a crash cannot leave half a file.
- **Engine keys** start with `platypus.`: `platypus.display` (the window mode and size, a JSON object) and
  `platypus.quality` (the quality level and its current values, a JSON string). `deleteAll` removes
  them too.
- **Quality.** Every quality or shadow change saves `platypus.quality`. The host
  restores it when the game starts (level by name, then the saved values over that level), without
  writing the starting state back as if the player had chosen it.

## Multi-window

`platypus_platform` can run more than one window (design: `docs/research/platypus-windows.md`). One-window apps are unchanged: every new
`PlatformApp` method has a default, and `WindowConfig::virtual_windows` is false by default.

**Keys and types** (`platypus_platform::windows`): `WindowKey(u32)` (`WindowKey::MAIN` = 1; the app chooses the others), `WindowKind`,
`WindowSpec` (logical sizes and positions), `plan(&spec, Os) -> NativePlan` (what a style means on an OS, decided without a display),
`place(..)`, `WindowTree` (parents; `close_order` deepest first; `contains`), `WindowCommand`, `WindowHost::{Os(Arc<Window>), Virtual
{ width, height, scale }}` (physical size), `attributes(spec, plan, owner, embed) -> WindowAttributes`.

**`PlatformApp` methods** (all default):

```rust
fn take_window_commands(&mut self) -> Vec<WindowCommand>;       // polled after input, after frame(), after events_done()
fn window_opened(&mut self, key: WindowKey, host: WindowHost);  // the window exists, hidden
fn window_open_failed(&mut self, key: WindowKey, error: String);
fn window_input(&mut self, key: WindowKey, event: InputEvent);  // default: MAIN -> input(), others dropped
fn close_requested(&mut self, key: WindowKey) -> bool;          // default true; MAIN true ends run()
fn window_closed(&mut self, key: WindowKey);                    // drop the surface + OS window clones NOW
fn events_done(&mut self);                                      // once per loop iteration, after all queued events
fn take_windows_to_show(&mut self) -> Vec<WindowKey>;           // first frame drawn: the platform shows them
```

Call order per loop iteration (winit runner and `FakePlatform::step` alike): events (`window_input(key, ..)`, then the commands
they caused) -> `frame()` on the main window's redraw (then commands, then `take_windows_to_show`) -> `about_to_wait`:
`events_done()`, commands, `take_windows_to_show`. The main window is made in `resumed` as before (`window_ready`, no `window_opened`);
it is `WindowKey::MAIN` in the tree from then on. The runner re-polls commands for a few rounds when applying them produced events
(virtual windows echo `Resized` / `Moved`).

**Commands** (`WindowCommand`): `Open(key, spec)` plans the spec for `Os::current()` (a plan warning is logged), makes the window
hidden and answers `window_opened`/`window_open_failed`; the tree parent is `spec.parent`, else the plan's owner / embed target
(a parent that is not open is dropped with a warning). `Close(key)` closes the window and its children, deepest first:
`window_closed(k)` and then the OS window is destroyed (the platform drops its `Arc`), so the app **must** drop its surface and every
`Arc<Window>` clone in `window_closed`. `Close(MAIN)` and a key that is not open do nothing. `SetKind` that would need another
owner than the window has is ignored with a warning (OS owners are fixed at creation). `SetFlags` reads `resizable`, `minimizable`,
`maximizable`, `closable`, `always_on_top`, `visible`.

**Events.** `InputEvent::Moved { x, y }` (physical outer position) and `ScaleChanged(f64)` are new. `CloseRequested` is not an event
for secondary windows: the platform asks `close_requested(key)` and closes. For `MAIN` it also still sends `CloseRequested` to
`window_input` first (so `input` sees it, as before); a refused close keeps the window. `Destroyed` for a window the platform did not
close (an owned window the OS destroyed with its owner) runs the same close path.

**Per OS** (`attributes`): Windows `with_owner_window(hwnd)` + `with_skip_taskbar`, `WS_CHILD` for `child`, and for tool windows and
popups a subclass (`SetWindowSubclass`, `Win32_UI_Shell`) that ORs `WS_EX_TOOLWINDOW` into `WM_STYLECHANGING` because winit rewrites
the extended style on every flag change; macOS `with_parent_window` (child window); X11 `with_x11_window_type` and reparenting for
`child`; Wayland plain toplevels.

**Screenshot runs** (`WindowConfig::virtual_windows = true`): the main window is still real; `Open` makes no OS window and answers
`WindowHost::Virtual` (physical size at the main window's scale). Setters update the record and queue `Resized` / `Moved` for
`window_input`. Secondary windows must be drawn offscreen by the app.

**Tests without a display:** `FakePlatform::new(app, WindowSpec::main(..), scale)` (`.os(Os::MacOs)` picks the plan rules) drives the
same `run_commands` / close code as the runner over a recording backend: `step()`, `send(key, event)`, `user_close(key)`, `focus(key)`,
`window(key) -> Option<&FakeWindow>` (`spec`, `plan`, `physical`, `position`, `scale`, `focused`, `minimized`, `maximized`, `shown`),
`keys()`, `tree()`, `exited()`, `app()`. The main window is opened with `window_opened(MAIN, Virtual)` (not `window_ready`).

**Surfaces for more windows** (`platypus_render`): `Gpu::create_window_surface(target, width, height) -> Result<WindowSurface,
RenderError>` makes a surface on the same instance, adapter and device (`RenderError::Unsupported` when the adapter cannot present to
that window). One `UiRenderer` and one `WorldRenderer` serve every window; draw with `WindowSurface::present_with`. Secondary
windows keep vsync off (never call `set_vsync(true)` on them: several FIFO swapchains in one thread can cost a vblank each), and
the surface must be dropped before its OS window is destroyed, i.e. in `window_closed`. A surface cannot be made without a real window,
so this function has no unit test.

## Limits

- One monitor: the window's current monitor is used, and there is no way to pick another.
- Exclusive fullscreen is untested across platforms.
- No HDR output modes.
- Phones and tablets: no safe area or orientation handling yet; only `FullScreenWindow`.
