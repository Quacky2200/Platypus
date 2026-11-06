# Platypus and Bevy

Platypus is not trying to surpass Bevy. It's a **Unity / s&box alternative**: a game engine where the
workflow feels like those tools (GameObjects, components, a Unity-style API, scenes, a visual
editor), built on the newest practical stack: **Rust** for the engine core and **TypeScript** for
gameplay and UI. Bevy is a Rust game engine built around an ECS. Both are written in Rust and
both sit on the same wgpu, winit and Taffy libraries, so some overlap is expected. This page
explains where the two differ and why, so readers don't have to guess.

## What each project is for

| | Platypus | Bevy |
|---|---|---|
| Goal | An s&box / Unity-style engine with React and TypeScript for gameplay and UI | A modular Rust engine built around an ECS |
| Who writes gameplay | TypeScript, compiled ahead of time to native code (`web_ui_ts`) | Rust, with systems and plugins |
| World model | GameObjects with components, scenes as JSON, `@property` fields | Entities, components as data, systems |
| UI | React reconciler → our own UI tree, CSS cascade, Taffy layout, Parley text, drawn by our renderer | `bevy_ui` (ECS entities, Taffy layout) |
| Editor | Built with the same React and Platypus UI (`editor/`; the binary is gone until it is re-made natively in TypeScript) | Experimental and in progress; check Bevy's current status |
| Physics | Built in: rapier (3D and 2D), joints, character controller, cloth | Provided by community crates such as Avian or bevy_rapier |
| Audio | Built in: kira mixer, buses, spatial emitters | Built in via rodio (`bevy_audio`), or community crates |
| Renderer | Own wgpu renderer (`platypus_render`), decided 2026-10-17 | Own wgpu-based renderer and render graph |

## Where the similarities come from

- **Shared Rust libraries.** winit (windows), wgpu (GPU), taffy (flexbox and grid), gilrs
  (gamepads), and rapier (physics) are the normal choices for Rust engines. Two projects using
  the same library is not a sign of copying.
- **Public references.** `DESIGN.md` names Bevy as a reference for a few specific things:
  taffy's use, the OpenXR integration (`bevy_mod_openxr`), and networking (`lightyear`). Those are
  points to read about, not code we took.
- **Shared public knowledge.** Both projects are discussed in the same articles, talks and
  training data. Similar names for common concepts (transforms, cameras, lights, materials,
  shadows, PBR) follow the wider graphics literature.

## Where Platypus differs

- **No ECS.** `DESIGN.md` §7.4 looked at reusing Bevy's rendering and asset crates and rejected it,
  because they bring Bevy's ECS and app model. Platypus keeps Rust owning the world (`platypus_scene`)
  and JavaScript owning behaviour (components with `onUpdate`, `onFixedUpdate`, and so on). The
  scripts hold only numeric IDs and call the engine API (`#[ts_api]`) directly.
- **TypeScript gameplay.** Gameplay is TypeScript compiled to native code by `web_ui_ts` (oxc →
  Cranelift). No JavaScript engine runs a game.
- **React UI.** The UI is a React reconciler over our own tree. Components, state and events work
  as in React, and CSS does the styling. Bevy's UI is built from ECS entities and components.
- **s&box and Unity conventions.** Names, units and flows follow Unity and s&box where that's
  practical (for example, degrees for Euler angles, `GameObject`, `Component`,
  `SceneManager`, `Rigidbody`, `CharacterController`, `Cursor`).
- **Built-in batteries.** Physics, audio, animation, particles, splines, clocks and
  a React HUD are in the engine core, not added by plugins.
- **One UI stack for the editor.** The editor reuses the engine's React and UI pieces, so a
  change to the UI helps both.

## What Bevy has that Platypus doesn't (yet)

- A much larger ecosystem of plugins and examples, and many people who have used it in
  production.
- Years of renderer and asset-pipeline fixes from a wide user base.
- A mature ECS with scheduling, parallel systems and change detection.
- Broader platform coverage. Platypus's own platform plan is in M9.c (macOS, Linux, Android, iOS).

Platypus is in its early stages. Parts of it are well tested; others are not. The gaps are listed in
`TODO.md`. This page does not claim Platypus is better than Bevy at anything beyond the workflow and
stack it was built for.

## How we work

- Platypus is written from scratch. We don't copy Bevy code. When we look at how another engine
  solves a problem, the design goes in `DESIGN.md` with the reasoning.
- When an approach looks like a known one, we note it in `DESIGN.md` or here, instead of hiding
  it.
- We watch known pitfalls from mature engines (see the "Watch list" in `TODO.md`), so we can check
  our code for them early. Those notes come from public discussion of such engines and need to be
  verified in Platypus before we rely on them.
