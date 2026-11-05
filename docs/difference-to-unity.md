# Platypus and Unity

Platypus borrows Unity's workflow: GameObjects, components, scenes, prefabs, the inspector-driven
way of building a world. It differs in the language, the stack under it, and the business it
sits in. This page covers those differences and what we want to keep from Unity.

## What we keep from Unity

- **The component model.** `GameObject`, `Component`, `Transform`, `Rigidbody`, `Collider`,
  `CharacterController`, `AudioSource`, `Animator`, `SceneManager`, `Cursor` and the lifecycle
  hooks (`onAwake`, `onUpdate`, `onFixedUpdate`, `onLateUpdate`) work the way Unity developers
  expect. Names follow Unity where that's practical, with degrees for Euler angles.
- **Portability.** Unity runs on many platforms, and that's a strong reason to be here. Platypus
  aims for the same reach over time (M9.c: macOS, Linux, Android, iOS). Nothing ships on those
  platforms yet.
- **Popularity and learning material.** Unity's large body of tutorials and its community are
  valuable. Platypus's API is close enough to Unity's that much of the knowledge carries over.

## The main difference: TypeScript, not C#

Unity scripts are C#. Platypus scripts are TypeScript, compiled ahead of time to native code
(`platypus_ts`) in development and shipped builds.

Why:
- **TypeScript is the most common typed language among web developers,** and React is the
  dominant UI model. Many developers already know both.
- **The web's tooling is the most active in the industry:** the compiler, linters, package
  managers, test runners and hot reload all exist in TypeScript form.
- **It reduces conversion work.** Teams with web backends can share types, validation and
  models with the game client.

What we're not claiming: TypeScript is not faster than C# for hot code, and the web tooling
doesn't replace a mature game-profiling pipeline. The AOT compiler (`platypus_ts`) is
the answer to speed.

## Other differences

| | Unity | Platypus |
|---|---|---|
| Scripting | C#, with IL2CPP or Mono | TypeScript, compiled to native code |
| UI | uGUI and UI Toolkit (UXML/USS, a CSS-like system) | React components with CSS (`platypus_ui_native` and a CSS cascade) |
| Renderer | Built in, with SRP and HDRP/URP | Own wgpu renderer (`platypus_render`) |
| Engine code | Closed source (C++ runtime) | Planned open source, Rust core, TypeScript gameplay |
| Licence | Free under a revenue threshold, paid seats above it (see below) | Planned: free, see the licence section |
| Stability | Major versions change APIs; packages change often | Stable API boundaries are a goal (see below) |
| Editor | A large, mature editor | Early; built with Platypus's React UI |

## Licensing and business model

Unity's 2023 runtime-fee proposal (a per-install charge) was announced, drew heavy criticism,
and was reversed in 2024 in favour of a different pricing model. The episode is the reason we
care about this. A pricing model can change quickly, and a studio's costs can change with it,
even when the software is the same.

Platypus's goal is to stay mostly free or very low cost, so that developers keep making games.
Concretely, that means:
- Platypus is planned to be open source. **Its licence has not been chosen yet.** We will choose one before
  release and say what it allows and what it costs, and we won't charge per install.
- No per-install fee, no revenue cut, and no approval step to publish a game.
- Any future paid services (hosting, a store, support) are optional and separate from the engine.

We should write the licence down in `LICENSE` and this page before the first release. Until then,
no one should assume a licence.

## Where Unity is ahead

- A mature editor, profiler and asset pipeline.
- Years of platform-specific fixes, console and mobile support, and certified workflows.
- A very large store of packages, tutorials, and hired talent.
- Proven shipped games at every scale.

## Our stability goal

Unity has changed its APIs and packages often over the years. We want Platypus to behave more like a
long-lived Unreal release: a stable core that games can rely on for years, with changes that add
features rather than break code. The principle is the Linux kernel's "don't break userspace",
applied to game code.

Concretely:
- Keep the Unity and s&box API names where they make sense, so code written against them stays
  familiar.
- Treat exported API names, script ops and the scene file format as public: changes need a
  deprecation period and a migration note.
- Keep dev and shipped runtimes on the same behaviour, so a game that works in dev works shipped.

The script API (`#[ts_api]` declarations) has been stable so far in our own testing. That is the
start of a track record, not proof of one yet.

## Open questions and risks

- **Our editor is far behind Unity's.** This is the biggest practical gap for new users.
- **Community size.** Open source would help, if we go that way, but it won't match Unity's community quickly.
- **C# developers** may not want to move to TypeScript. We can serve them only if the AOT path and
  the API are good enough.
- **Platform reach** depends on work not yet done (M9.c).
