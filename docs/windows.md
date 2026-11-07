# Windows: more than one OS window

User guide to the `Window` API: opening tool windows, palettes and dialogs next to the game's own window, giving each its
own UI, and talking between windows. The example is `examples/windows`. Design and decisions:
`docs/research/platypus-windows.md` (plan: `platypus-windows-plan.md`). Code: the API and its state now live in web-ui's
`web_api_window` crate (`../web-ui/crates/web_api_window`, feature `window` of `web_api`; web-ui `docs/web-api.md` and `docs/app.md`
"Secondary windows"); `crates/platypus_engine_api/src/window.rs` is only `pub use web_api_window::*`. Differences for Platypus: its
`log::set_sink` forwards to `web_api_window::set_log_sink`; `app::take_requests` turns `take_quit()` (`Window.main.close()`) into
`Request::Quit(0)`; the main title is `""` until `set_main`. The API's tests moved to web-ui (`WUI-WIN-01..07`, written, not run);
`M9-API-1426` stays here as `crates/platypus_engine_api/tests/window_quit.rs`. Platypus's own host code:
`crates/platypus_platform/src/windows.rs` and the runner (`docs/platform.md`, "Multi-window"),
`crates/platypus_app/src/windows.rs` (trees, surfaces, routing, drawing) and `native_render.rs` (`sync_window_roots`),
`tools/web_ui_ts/src/builtins.rs` (the typed members for compiled games) and `tools/web_ui_ts/src/lower/messages.rs` (their compiled form).

**Honest status.** The move into web-ui compiled; its tests were written but not run, and no window was opened. The winit runner has not been run on a real display. Everything was checked with the fake platform
(`FakePlatform`), unit tests of the pure parts (`plan`, `place`, `WindowTree`, the message queues) and `--screenshot` runs, where
secondary windows are virtual (offscreen, no OS window, no swapchain). Windows is the platform the code was written for; the
tool-window subclass, `WS_EX_TOOLWINDOW` handling, owner behaviour and DPI changes still need a manual check, and the macOS, X11
and Wayland paths are untested.

## Making windows

```tsx
Window.main.title = "My Game";                        // the main window always exists

const palette = Window.create({
  title: "Palette", style: "tool", parent: Window.main,
  width: 200, height: 190, x: 80, y: 120,
  key: "palette",                                      // survives hot reload, found again with Window.find
});
createRoot(palette).render(<Palette />);              // each window has its own UI root
palette.onCloseRequested(() => { palette.cancelClose(); palette.visible = false; });  // hide instead of closing
```

`Window.create` never throws and returns a handle at once; the OS window appears at the end of the frame
(`onOpened`, below). Sizes and positions are UI pixels (the client size; `x`/`y` are the outer frame's top-left on the desktop).
Left out, a window is 400 x 300 and is placed centred on its parent, cascading.

### Options (`WindowOptions`)

| Option | Meaning |
|---|---|
| `title` | Title bar text (default `""`). |
| `width`, `height` | Client size (default 400 x 300). `minWidth`, `minHeight` set the lower bound. |
| `x`, `y` | Outer top-left; both left out lets the host place the window. |
| `style` | `"normal"` (default), `"tool"`, `"dialog"`, `"popup"`, `"borderless"`, `"child"`. |
| `parent` | The owner. Tool, dialog, popup and child default to `Window.main`; `normal` and `borderless` have none. |
| `resizable`, `minimizable`, `maximizable`, `closable` | Frame buttons and sizing. |
| `alwaysOnTop` | Above other windows (any style). |
| `visible`, `focus` | Created hidden; takes focus when shown (popups never do). |
| `key` | A name that keeps the window across a hot reload and finds it again with `Window.find(key)`. |
| `runInBackground` | Root windows of isolated families: keep that domain's timers running while unfocused (default true). |

Positional arguments (`Window.create("Title", 320, 200)`) also work. Most options can be changed later through
properties: `title`, `style` (not to or from `"child"`), `x`, `y`, `width`, `height`, `setPosition`, `setSize`, `center()`,
`minimized`, `maximized`, `visible`, `resizable`, ..., `focus()` and `close()`.

### Styles

- `normal`: an ordinary top-level window with its own taskbar button.
- `tool`: a palette or inspector; small caption, no taskbar button, stays above its owner, hidden while the owner is minimised and
  closed with it.
- `dialog`: a question or settings box over its owner; no taskbar button, not resizable by default. It does **not** block the
  owner (modal dialogs are not built).
- `popup`: undecorated, owned, takes no focus. For menus, tooltips and splash screens.
- `borderless`: undecorated top-level window with a taskbar button.
- `child`: meant to be embedded in the parent's client area. **Not built**: it is accepted by the API and opens as a `tool` window.

On Windows a tool window uses `WS_EX_TOOLWINDOW` and an owner; the other platforms map as the table at the end shows.

### Parents, children and the main window

- `Window.main` is the original window (a property, not a call). It is the root of everything; `Window.main.close()` quits the
  application (`Application.quit()`), and a script cannot otherwise close it.
- `win.parent` is the owner (undefined for the main window and for windows without one); `win.isChild` is true when it has a
  parent; `win.isMain`, `win.isOpen`, `win.root`, `win.childCount` and `win.childAt(i)` describe the tree. Closing a window
  closes its children first.
- `Window.current` is the window whose event (UI handler, window callback, message) is running. `Window.focused` is the window
  with keyboard focus, if it is one of the game's. `Window.count` / `Window.at(i)` list the open windows (the main one first,
  then in creation order); `Window.find(key)` and `Window.byHandle(n)` look one up. `win.handle` is a small number that is safe
  to put in a message (handles in messages cannot be objects).
- `Screen` stays the main window's: `Screen.width`, `height` and display modes do not apply to tool windows; use `win.width`.

### Events

Each listener returns a token for `win.off(token)`.

| Listener | Runs when |
|---|---|
| `onOpened((width, height, scale) => ..)` | The OS window exists (the end of the frame in which it was created). Register it right after `create`. |
| `onOpenFailed((error) => ..)` | The window could not open; it is then closed. |
| `onCloseRequested(() => ..)` | The user asked to close it (title bar, Alt+F4). Call `win.cancelClose()` to keep it. |
| `onClosed(() => ..)` | It closed, by `close()`, the user or its parent. Once. |
| `onResized((w, h) => ..)`, `onMoved((x, y) => ..)` | New client size, new outer position. |
| `onFocusChanged((focused) => ..)` | Keyboard focus moved to or from it. |
| `onScaleChanged((scale) => ..)` | It moved to a monitor of another DPI. |

`win.openError` is `null`, or the text of why it could not open; `win.isOpen` is false then.

### Keys and hot reload

A window made with a `key` survives a hot reload with its position and size (the player may have moved it). The new code's
`Window.create({ key })` returns that same window; the new title, style and flags apply, a different `parent` is ignored with a
warning, and `createRoot(win)` mounts the new UI. Windows **without** a key close at the start of a reload (without
`onCloseRequested` or `onClosed`, since their listeners belong to the old code). Keyed windows nobody claimed by the end of the new
code's first frame close. Listeners are dropped at the reload and must be registered again by the new code, which is what the
example's `openPalette()` does on every start. The main window always stays. Queued messages and pending requests are dropped. If the new code fails to load, the old code keeps running and gets its listeners, queued messages and pending requests back (`window::reload_abort`); whatever the failed code registered or created is dropped, and windows already told to close stay closed.

### Running code in a window

`createRoot(win).render(<Panel />)` mounts React (or the native reconciler) into the window; its handlers run with that window
current. For plain code, `win.run(() => ..)` queues a function to run once in the window's own event domain: the "main function" of
an isolated window. The creator of an isolated root may call it, and register listeners, until the end of the frame it created
the window in.

## Messages

Windows exchange data with `postMessage`, `onMessage`, `request` / `onRequest` and `Window.broadcast`, or with named events (`emit` / `on` / `ask` / `answer`, below).

```ts
// Sender
inspector.postMessage({ kind: "select", name: "Player", position: [1, 2, 3] });
const reply = await inspector.request<number>({ kind: "count" }, 2000);   // timeout in ms, default 10000
Window.broadcast({ kind: "theme", dark: true });                          // the root of every other family

// Receiver, in that window's code
Window.current.onMessage<{ kind: string }>((from, msg) => { /* from is the sending Window */ });
Window.onMessage<{ kind: string }>((from, msg) => { /* any window of this domain, after the per-window listeners */ });
Window.current.onRequest<{ kind: string }, number>((from, req, reply) => reply(3));  // or Window.onRequest
```

- **What can travel:** values that are plain JSON: numbers, strings, booleans, `null`, arrays, plain objects. Value classes
  (`Vector3`, `Color`, ...) travel in their compact JSON form (`Vector3` is `[x, y, z]`); type the field as a tuple. **No handles**
  (`GameObject`, `Component`, `Texture`, `Window`: send `win.handle` and read it with `Window.byHandle(n)`), no functions, no `Map` or
  `Set`, no cycles. Games check this at build time from the static type; a value that still breaks it throws a `TypeError` at the
  call.
- **Size:** at most 1 MiB of UTF-8 JSON per message. Over that, or to a closed window, `postMessage` returns `false` and logs.
- **Delivery:** always asynchronous, also within one family, queued for the target's domain and run at the start of that domain's
  next dispatch. Order is kept per sender. A message to a window that closes before delivery is dropped.
- **Requests:** `request` resolves with the reply, and rejects with an `Error` when the window closes, when its domain has no
  `onRequest` listener at delivery, or after the timeout. A listener may reply later; a second `reply` is ignored.
- Compiled games: the type argument decodes the payload: `request<R>`, `postMessage` / `onMessage<T>` and `onRequest<T, R>` accept
  a program's own interface or type alias, an object literal type, arrays and nested objects (no handles, functions, `Map` or
  `Set`); the compiler refuses anything else with the path to the field.

### Named events

For "something happened" and "tell me X", name the message instead of switching on a `kind` field. Named events are sugar over the
raw messages above: `emit` is a `postMessage`, `on` an `onMessage` that only runs for its name, `ask` a `request`, `answer` an
`onRequest` for its name.

```ts
interface Thing { name: string; position: [number, number, number] }

// Main window
Window.main.answer("selected", (): Thing => thingAt(selected));               // the return value is the reply
Window.main.on("edit", (e: { name: string; y: number }, from: Window) => { /* from is the sender */ });
inspector.emit("select", thingAt(2));                                       // payload is optional: emit("close-all")
Window.emitAll("theme", { dark: true });                                     // broadcast of a named event

// Inspector (another domain)
const t = Window.current.on("select", (thing: Thing) => setThing(thing));   // a token, like every listener
Window.current.off(t);
const thing = await Window.main.ask<Thing>("selected");                      // ask<R>(name, payload?, timeoutMs?)
Window.main.emit("edit", { name: thing.name, y: 2 });
```

- **Async handlers:** an `answer` handler may return a promise (`async (n: number): Promise<number> => ..`): the reply is sent when
  it resolves, and a rejection fails the asker's `ask` with the error's message. A raw `onRequest` handler may be `async` too (it
  still calls `reply`; a rejection before the reply fails the `request`).
- **Typing:** the handler's parameter type (or the explicit `on<T>(..)` / `answer<T, R>(..)`) tells the compiler how to decode the
  payload; a handler without parameters (`on("ping", () => ..)`) takes none, and an `answer` handler's return type is `R`.
  `ask<R>` needs its reply type, as `request<R>` does. A later idea: one shared
  event-type map (`interface Events { select: Thing; ... }`) so a name fixes both ends.
- **Names:** any non-empty string (an empty one throws `Window event names must not be empty`). Events never collide with the
  built-in events (`onClosed`, `onResized`, ...): those stay `onX` methods, a custom name is only ever matched against other
  custom events.
- **Same transport, separate namespace:** the same JSON limits (1 MiB, no handles; `emit` returns `false`, `ask` rejects with
  `request refused`), asynchronous delivery per domain, `off(token)`, and the rejection reasons `window closed` and `timed out`
  (default 10000 ms) for `ask`. But events are a layer of their own: the host routes an envelope only to the `on` / `answer`
  listeners of its name on the target window. Raw `onMessage` / `onRequest` / `Window.onMessage` / `Window.onRequest` listeners
  never see events, and `on` / `answer` never see raw messages or `request`s (a `request` to a window that only has `answer`
  listeners fails with `no request listener`).
- **Fast fail:** an `ask` for a name the target window has no `answer` for rejects at once, at delivery, with
  `no answer for "<name>"` (the window has no listener at all, only raw ones, or answers other names). The timeout stays for an
  `answer` that never replies; a closed window gives `window closed`. An `emit` nobody listens for is dropped silently.
- **Wire form:** `{"$e":"name","d":payload}` (no `d` without a payload; the `on` / `answer` callbacks get the payload text, `""`
  without one, and compiled code decodes that as `null` for a handler that wants a value). Any text of exactly this shape is an
  event, also one posted by hand with `postMessage`; anything else (including a malformed envelope) is a raw message
  (`window::parse_event`).
- **When to use the raw layer:** one message type for a whole window (a protocol with its own `kind` field), or a handler that
  must reply later or from a callback: `onRequest` gives you `reply`. `answer` replies once, with the handler's return value
  (`undefined` becomes `null`).

## Event domains and isolation

A **family** is a root window (one without a parent) and every window whose `parent` chain leads to it. `Window.main` is the
root of the main family. A family has one **event domain**: its own keyboard focus and input, its pause state, its timers, and its
UI roots. The game counts as focused while any window of the main family is focused; an isolated family (say, an Inspector made
with no parent) is paused or running independently of it, according to its `runInBackground` option.

What isolation means in phase A, which is what exists: all domains run in the **one script runtime, one thread**, taking turns.
The engine API enforces the separation:

- Windows of another family can be read (`title`, size, `isOpen`) but not changed: setters, `close`, `focus` and registering
  listeners on them are refused with a warning naming both windows. (The one exception is the setup window described above.)
- Handles cannot go through messages, so a message is the only way to ask another family to do something.
- Input, focus and timers are per domain (see "Input belongs to the window's event domain" above).
- Module variables are still shared memory. Code can reach across families through them (and a slow handler in one domain delays
  the others); do not rely on it. Moving a domain to its own thread (phase B) or process (phase C) is designed
  (`platypus-windows.md` section 3a) and not built; sticking to messages is what keeps a game working when it is.

### Input belongs to the window's event domain

> **Read this before using `input` with more than one window.** `input` is not one global keyboard.
>
> - `input` reads the keys, mouse and gamepads of the **window family whose code is running**. Inside an isolated window's turn
>   (`win.run(..)`, its timers, message handlers, UI events) it is that window's; everywhere else it is the main window's.
> - **Main-game components (`onUpdate`, ...) always read the main window's input**, even while another window is focused. They do
>   not see the Inspector's keys, and they keep running or pause according to `Application.runInBackground` (**default off**: the main game pauses when the app loses focus, i.e. when no window of the main
>   family is focused, unless you turn it on), not according to which of the main family's windows has focus. An isolated window's
>   own `runInBackground` option (default true) decides whether *its* timers keep running while it is unfocused.
> - An isolated window's keys **never reach the main `input`**. To tell the game about a key, send a message.
> - **Focus loss clears the held keys** of that domain, so a key released in another window does not stay stuck.
> - Callbacks with no domain tag (such as an `await` continuation) may read the **main** domain's input, even if
>   the `await` began in an isolated window. Read the keys before the first `await`.
> - `input.on(..)` listeners belong to the domain they were registered in.
> - Tested in `crates/platypus_engine_api/tests/input_domains.rs`; `crates/platypus_app/src/game_input.rs` sends each domain's
>   events with that domain entered.

```ts
class Player extends Component {
  onUpdate() {
    if (input.isKeyDown("KeyW")) this.transform.translate(0, 0, 0.1); // main's keys, whichever window is focused
  }
}

const inspector = Window.create({ title: "Inspector", key: "insp" }); // no parent: its own domain
inspector.run(() => {
  setInterval(() => {
    if (input.isKeyDown("KeyW")) log("W held in the Inspector"); // the Inspector's keys, not Player's
  }, 100);
});
```

`examples/windows` shows the pattern: the main window owns the data, the Inspector (no parent) asks for it with `ask<Thing>`, hears
changes through `on("select", ..)`, and emits its edits back to `Window.main`.

## Screenshots

`platypus-app <game> --screenshot out.png --frames N` writes the main window to `out.png` and every open secondary window to
`out-<slug>.png`, drawn offscreen at its own size. The slug is the title lowercased with each run of characters other than `a-z0-9`
turned into `-` (an empty slug is `window<handle>`; a clash appends `-<handle>`). `examples/windows` produces `windows-palette.png`,
`windows-counter-1.png` and `windows-inspector.png`.

## Window, Screen and Application

| | What it is | Use it for |
|---|---|---|
| `Application` | The process: name, version, quit, focus and pause (`onFocusChanged`, `runInBackground`), platform and system information | Anything about the program as a whole |
| `Screen` | The display and the **main window's** mode: resolution, full screen / windowed, refresh rate, supported modes | Settings menus, the main window's size |
| `Window` | One OS window: its own title, size, position, style, UI, events and messages | Tool windows, palettes, dialogs, inspectors, more than one view |

`Window.main.title` is the main window's caption and `Window.main.width` its client size, but its mode and resolution stay with
`Screen` (secondary windows have no full screen or per-window display modes). `Application.quit()` ends everything; closing a
secondary window ends only that window and its children.

**Unity.** The player has one window. `Display` is about monitors and cameras, not windows; the Editor has `EditorWindow`, but a
built game cannot open extra OS windows without native plugins. Platypus's tool windows are closest to what Unity leaves to plugins.

**Godot 4.** The `Window` node is the closest relative: subwindows with a title, size and position, a `close_requested` signal, a
`transient`/parent relation and its own viewport. Differences: Godot embeds subwindows inside the main window by default
(`embed_subwindows`) and has modal `exclusive` windows, which Platypus lacks; Platypus windows are always real OS windows (except where
unsupported), each with its own UI tree, and isolated families add an event-domain model and messages that Godot does not have
(its windows share one scene tree).

## Platform support

Messaging, families, requests and everything else in code is platform independent: it runs the same everywhere, and a game that
never opens a second window loses nothing. What differs is whether the OS window opens.

| Platform | Extra windows | Tool windows | Notes |
|---|---|---|---|
| Windows | Yes | Yes (owner, no taskbar button, `WS_EX_TOOLWINDOW`) | The reference platform; still unchecked on a real display |
| macOS | Yes | Yes (as child window of the owner; normal caption) | Untested |
| Linux X11 | Yes | Yes (utility window type; not tied to the owner) | Untested |
| Linux Wayland | Yes, as plain top-level windows | No: `tool`, `dialog`, `popup` open without an owner and log `Cannot open Window due to platform restrictions: tool windows are not supported on Wayland` as a warning; `child` becomes `tool` | Untested; winit has no Wayland parents |
| iOS | No | No | `Window.create` returns a handle that never opens |
| Android | No | No | same |
| Web | No | No | same |

On iOS, Android and the web, `Window.create` still returns a handle at once and never throws. The host then reports a failure:
the window is closed, `win.openError` is `Cannot open Window due to platform restrictions (platform: <os>)` (`ios`, `android`, `web`),
`win.onOpenFailed((error) => ..)` runs once, and an error is logged once. `Window.isSupported` is false on those platforms, so a game
can check first and keep its UI on the main window (the example shows a notice). The main window is never affected.

Not built, listed as optional later steps in `platypus-windows-plan.md`: embedded `child` windows (D1), modal dialogs (D2),
`Camera.targetWindow` (D3), transparent windows (D4), a hidden main window in screenshot runs (D5), and `Application.wantsToQuit`.

## Limits

- Fullscreen secondary windows, per-window display modes and choosing a monitor.
- File drops and drag and drop between windows.
- Component hooks (`onUpdate`, ...) belong to the scene, which is the main domain's; other domains run timers, UI handlers and
  messages.
- Secondary windows present with vsync off and draw every main frame (hidden and minimised ones are skipped).
- A `tool` style that the OS cannot attach to an owner (Wayland) opens as a plain window.
