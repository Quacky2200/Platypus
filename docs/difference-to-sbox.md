# Platypus and s&box

s&box (Facepunch's successor to Garry's Mod, built on Valve's Source 2) is the closest
inspiration for Platypus's shape. We took its API style, its UI approach and its editor-first
workflow. The differences are the stack, the language, and how games are published.

## What we took from s&box

- **Unity-style APIs in a Source 2 mindset.** GameObjects, components, `@property` fields,
  scenes as files, and a clean lifecycle. Platypus's TypeScript API follows s&box's naming where
  it makes sense.
- **Razor-like panels with CSS.** s&box's UI approach was a fresh idea next to older engine UI
  systems. Writing panels with markup and CSS fits how interfaces are built on the web now.
  Platypus keeps that idea and uses React in place of Razor: components, state and events, styled
  with CSS.
- **Editor-first thinking.** The editor is built with the engine's own UI, so a fix to the
  UI improves the editor too.

## What we changed

| | s&box | Platypus |
|---|---|---|
| Engine base | Source 2 (C++), Valve's engine | Rust core, own renderer (wgpu) |
| Scripting | C# | TypeScript compiled ahead of time to native code (`web_ui_ts`) |
| UI | Razor panels with CSS | React reconciler with CSS (`web_ui_react`) |
| Licence and publishing | Free to use, but release goes through Valve and s&box | Planned open source, licence to be chosen; no approval step (see below) |
| Physics | Source 2 physics | rapier (3D and 2D), built in |
| Audio | Source 2 audio | kira mixer, built in |
| Stability | Engine heavily reworked during public development | Stable API boundaries as a goal (see below) |

## Publishing

As we understand it, a game made in s&box can't be released to players without Valve and s&box
approval, even though the engine itself is free. We don't agree with that arrangement for a
game engine, and Platypus's plan is the opposite: nothing between a developer and their players.
(Check the current s&box publishing terms before quoting them; the rules may have changed.)

Platypus will never require approval to publish a game, and we won't take a cut of sales.

## The stability question

s&box has been in public development for years, and its APIs and engine have been reworked a
lot. Each rework costs the people building on top of it, and that's a real problem for a new
engine that wants to be stable itself.

Platypus's script API (`#[ts_api]`) has stayed the same in our own testing. We want
the same stability level that a long-lived Unreal release has, where a version is used for
years because it doesn't need much change. We keep familiar Unity and s&box API names, which
helps us avoid the urge to rename things for taste.

## Where s&box is ahead

- A working editor and a live community building on it.
- Its Source 2 base has a long history, a large content library and shipped content.
- Razor and CSS panels are already built and used in production.

## Open questions and risks

- s&box's identity and direction are uncertain to us, and that could affect how much its
  developers and content are worth to us. We are not building on its code or its assets.
- Matching s&box's API closely is a goal, but its public API may change, so we'll adjust where
  it's sensible and document any difference.
- Platypus's editor still needs most of the work s&box has already done.
