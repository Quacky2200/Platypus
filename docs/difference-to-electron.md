# Platypus and Electron

Electron packages a web app with Chromium and Node.js, so each app ships its own browser
engine. Platypus is aimed at the same kind of product, a desktop app with a React UI, but with
one native binary, a Rust core, and no bundled browser. This page covers the reasons and the
plan, and where Electron is still ahead.

## The problem with the current shape

- **Each app ships its own engine.** Chromium and its JavaScript engine (V8) come with every
  Electron app. The ICU data that Chromium and V8 use for locale and text handling is large
  on its own. We haven't measured the total per app here; check current Electron builds
  before quoting a figure.
- **Memory and disk use add up.** Several apps mean several copies of the same engine running.
- **A web runtime is a large framework for simple programs.** Many apps need a window, a
  form, a list and a few network calls, not a full document and layout engine.
- **Two processes and a bridge.** Electron apps split into a renderer and a main process, and talk
  over IPC. That's safe and flexible, but it's extra code and extra cost.

Electron's isolation is good, and it has a huge ecosystem. Our aim is not to remove what works,
but to keep the web developer's way of building UIs while cutting what each app has to carry.

## What Platypus does differently

- **React and CSS for the UI, running on our own renderer.** React components drive a UI tree (the native reconciler, `web_ui_react`) that
  Platypus lays out with Taffy and draws with wgpu. There is no DOM and no browser engine.
- **TypeScript compiled to native code.** There is no JavaScript runtime in the app at all (QuickJS was
  removed). Scripts call the Rust core directly, without an IPC bridge, and a typed `@platypus/api` surface is
  planned so an app can call the engine as a normal TypeScript module.
- **Rust for the platform.** Memory safety and a single native binary per platform. Windowing
  (winit), GPU (wgpu), audio (kira), physics (rapier) and gamepads (gilrs) are all Rust crates.
- **One binary, no bundled browser.** An app ships its own code plus the engine, not a copy of
  Chromium.

The claim that matters most for users is that Platypus uses far less disk and memory per app. We
should measure it when the app shell exists, against a comparable Electron app, before we say
it publicly.

## The plan

These are planned, not built. The order may change.

1. **An Electron-style app shell.** A way to write an app with React and TypeScript, with no
   game world required: window, menus, dialogs, file access, and the UI tree on its own. The
   TODO "App shell for UI-first apps" under "Later" covers the first step.
2. **`@platypus/api`.** A typed TypeScript surface for the engine's services (files, network,
   audio, windows), so apps don't need a separate process or IPC layer.
3. **Web platform APIs,** in the order that unlocks the most apps:
   - **Files and fetch** (M6 Script IO): read and write files, HTTP requests.
   - **WebSocket** (M6): realtime connections, also for multiplayer (M8).
   - **Canvas.** A 2D drawing API close to the browser's canvas, built on the existing
     `LineRenderer2D`, `Shapes` and `TextMesh` work, plus the "2D canvas mode" in the TODO.
   - **Audio.** A Web Audio-like graph on top of kira (buses, gain, filters, analysers).
   - **WebAssembly.** Load wasm modules from scripts (M9 backend options, including the
     AssemblyScript and wasm-to-Rust routes in TODO M9.b).
   - **Storage** (IndexedDB-style key-value stores) for app data.
4. **Embedding the 3D engine in React UI** (the "canvas UI element" in TODO "Later"), so an app
   can show a live 3D view in a box.

## Where Electron is ahead

- **The web platform is huge.** Electron gives every standard browser API and a large set of
  npm packages. Platypus will have a fraction of that at first, and each missing API is a limit.
- **Accessibility and text input.** Chromium's accessibility tree, IME support and text
  shaping are years of work. Platypus's text fields and IME exist, but they need more testing across
  platforms.
- **Tooling.** Chrome DevTools, mature debuggers and profilers, and well-known packaging and
  updater tools.
- **Predictability.** Every Electron app sees the same browser. Platypus's renderer means each
  platform's GPU driver matters, so we'll have more driver-specific testing to do.

## Open questions and risks

- **Compatibility.** Existing web apps won't run unchanged. We should say so up front, and
  offer a migration path from React DOM code.
- **Adoption.** Developers won't switch because an approach is cleaner. We need a few real apps
  first, and to show the size and speed gains with numbers.
- **Security.** Isolation has to be designed, not inherited from Chromium's sandbox. This is
  a real cost and needs its own design doc before the app shell ships.
- **Scale.** This is an ambitious project. The game engine comes first, and the app shell
  shares its foundations.
