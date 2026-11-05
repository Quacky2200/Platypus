# platypus-build: building a game with platypus_ts

`tools/platypus_build` (binary `platypus-build`) builds a game folder written in TypeScript for `platypus_app` with **no bun and no
`node_modules`**. Only `tsc` is optional, for type checking while developing. It lives in the `tools` workspace:

```
cd tools
cargo run -p platypus_build -- build ../examples/hello
```

| Command | What |
|---|---|
| `build <game> [--no-tsc] [--aot] [--cook] [--no-compile] [--no-prebuild] [--check-unused] [--manifest api.json]... [--target triple] [--pic]` | Builds `<game>/build`. `--check-unused` also lowers the program's own functions nothing calls (a build skips them), so their errors show. `--manifest` adds a host's own `#[platypus_api]` namespace (the JSON its `MANIFEST.to_json()` writes) to the engine's, for a host that is not `platypus-app` (Fluid's `Fluid`). `--target` writes the `--aot` object for another triple (Cranelift needs no toolchain for it: `x86_64-unknown-linux-musl` from a glibc machine), `--pic` as position-independent code, which a Linux executable (PIE, Rust's default) or a shared library needs (see "Linking an `--aot` object"). The editor (`editor/`) builds like any game. |
| `types [<game>] [--out file]` | Writes `<game>/types/platypus.d.ts` (type declarations for `tsc`). |
| `assets <folder>... [--prune]` | Writes `.meta` GUID files (it replaced the Bun `scripts/assets.ts`, which is deleted). |
| `dev <game> [--no-tsc]` | Rebuilds on every change and signals the running app. |
| `check <examples folder> [--tsc] [--full] [--check-unused]` | Builds every game in the folder and prints a table (`--check-unused` as for `build`); `--tsc` also type-checks each with the generated `types/platypus.d.ts` (`bun run typecheck` runs it). |
| `init [<dir>] [flags]` | Writes a default `project.json`, a self-contained `tsconfig.json` and `types/platypus.d.ts` (see "`project.json`"). |
| `init --editor-files [<dir>...]` | Rewrites only `tsconfig.json` and `types/platypus.d.ts` of a game, or of every game in a folder (see "Editor files"). |
| `package <game> [flags]` | One executable for the game: `build/<exe>.exe` (see "Packaging a single executable"). |

**Building the host.** `platypus-build` itself needs only Rust. The engine host it packages (`cargo build --release -p
platypus_app`) links ANGLE on Windows, whose build runs bindgen and so needs libclang: set `LIBCLANG_PATH` to the `bin` folder of
an LLVM 20 or newer install for that command (for example `LIBCLANG_PATH='C:\llvm-23.1.4\bin'`), or build with
`--no-default-features` to leave ANGLE out ([rendering.md](rendering.md), "Static ANGLE").

## The output folder: `build/`

A game builds into `<game>/build/`. Games used to build into `dist/`; that folder is not
read any more (no fallback): `platypus-app` on a folder with only `dist/game.pbundle`, `dist/game.irbundle` or `dist/game.js` exits with an error that
says to rebuild with `platypus-build build` (`M9-APP-06`). A stale `dist/` can be deleted; `.gitignore` still ignores it.

## QuickJS is removed

`platypus_app` runs only games compiled by `platypus-build build` (`build/game.pbundle`, or a `.ts` / `.tsx` file). A folder or file that
holds only a QuickJS bundle (`build/game.js`, or a `.js` path) is refused with "... is a QuickJS bundle: QuickJS is no longer part of
Platypus; rebuild the game with `platypus-build build`" and a non-zero exit (`M9-APP-02`). `--force` no longer exists; passing it is an
argument error (`M9-APP-01`). The `platypus_editor` binary was removed with it; the editor is a game (`editor/`) run as `platypus-app editor --project <folder>` ([editor.md](editor.md)).

## `project.json`: the project file

A game's product metadata lives in `project.json` in the game folder (`platypus_core::project`, shared by `platypus-build` and `platypus_app`).
`project.json` is the **only** game config: a game needs no `package.json` and no `node_modules` (the examples have none; `platypus-build`
builds them, and plain `tsc` cannot check them since the engine names are globals from the generated `types/platypus.d.ts`: use
`platypus-build check --tsc`). A folder with a `project.json` is a game: `platypus-build check <dir>` and the examples test find games that way.
A leading `//` comment line is allowed; otherwise plain JSON.

- **`name` is the only required field.** Without it (or with `"name": ""`) the load fails with an error naming the file.
  `{ "name": "hello" }` is a complete project.
- **An omitted field takes its default silently**: the title is `${firstName}'s Game`, the company `${firstName}'s Company`, the license
  `private`, the version `0.1.0`, and so on (table below).
- **An explicitly empty string stays empty** (`"company": ""`: no default is substituted). `platypus-build build` and `platypus-build package` print
  one warning per empty metadata field (`title`, `company`, `identifier`, `version`, `description`, `copyright`, `license`), e.g.
  `warning: C:\Games\fox\project.json: "company" is empty (the exe's version info will have no company)`. `platypus-build check` never does.
  `package` also warns (it never blocks) when `company`, `copyright` or `license` were left out and so still hold their defaults.
- **`renderer`: left out (the default), `"vulkan"`, `"gles"` or `"cpu"`.** Which renderer `platypus-app`, the one host of
  every game, draws with (`platypus_core::project::Renderer`). Left out, it follows what the program uses, found at build
  time (the bundle's `renderer` tier, below): a program that is only UI starts on GLES (ANGLE linked into the Windows host,
  no world renderer, the lightest), one that uses the 2D or 3D world on wgpu with the world renderer. `"vulkan"` forces wgpu
  with the world renderer (Vulkan by default on Windows and Linux, DX12 with `WGPU_BACKEND=dx12`, Metal on macOS); `"gles"`
  forces the light UI-only path; `"cpu"` the CPU renderer (no GPU API; UI only, opt-in). For `gles` or `cpu` with a program
  that uses the world, `build` and `package` warn (`renderer is gles but this game uses the world (Camera, Light): it will
  show 3D Not Supported`), and the window shows "3D Not Supported" when the game asks for the world. Any other value warns
  and counts as left out. The command line's `platypus-app --renderer` and `package --renderer` come first. The old
  **`type`** (`game` / `app`) gets a one-line warning (`` `type` is replaced by `renderer`; "app" = "gles", "game" =
  "vulkan" ``) and is ignored. `init` leaves `renderer` out. Details: [rendering.md](rendering.md#choosing-the-renderer).
- **Deprecated fallback:** a folder with no `project.json` but a `package.json` with a `platypus` section still works: the loader reads that
  section (without templates) plus the file's `name`, `version`, `author` and `description` (and the old defaults: title `${name}`, empty
  company, version `0.0.0`). It is found as a game by `check` too. With a `project.json`, `package.json` is not read at all.
- **Names from before the rename to Platypus** (the engine was called Iron) are still read: a `package.json` section called `iron`
  (`platypus` wins when both exist), `build/game.irbundle` when there is no `game.pbundle` (the host logs a deprecation note),
  scene and prefab files with `"format": "iron.scene"` / `"iron.prefab"`, `import .. from "@iron/engine"` (erased like
  `@platypus/*`; `tsc` only knows the new names), the saved quality choice under PlayerPrefs `iron.quality`, and the settings
  folder `Iron/<title>` of a game without a company when no `Platypus/<title>` exists yet. The binary magics of packaged
  games (`IRONPAYL`), packs (`IRPK`) and cooked files (`IRTX`, `IRMD`) are unchanged, so old packs and caches still open.
  Environment variables, crate names and the cooked folder (`.platypus/`, was `.iron/`) have no fallback. Tests `M9-REN-01..04`.
- **Edited by the editor:** Edit > Project Settings changes the raw file key by key (`platypus_core::project_edit`: `get_key`, `set_key`,
  `remove_key`). Only the text of the one top-level key is replaced, added last or removed, so `${templates}`, key order, spacing, the
  platform sections and unknown keys stay as written. The result must still parse, or nothing is written. The keys it offers are the
  `FIELDS` table there. See [editor.md](editor.md#project-settings).

```json
{
  "name": "fox-run",
  "title": "${firstName}'s Game",
  "company": "${firstName}'s Company",
  "identifier": "com.${username}.${name}",
  "version": "0.1.0",
  "description": "${title}",
  "license": "private",
  "copyright": "Copyright (c) ${year} ${company}",
  "startScene": "scenes/level1.scene.json",
  "windows": { "icon": "icon.ico" }
}
```

| Field | Default | Meaning |
|---|---|---|
| `name` | **required** (in the deprecated `package.json` fallback: its `name`, else the folder name) | project name (`${name}`); not shown to players |
| `title` | `${firstName}'s Game` | window title (the initial value; the game can change it at run time), `ProductName`, settings folder |
| `subtitle` | none | an optional second line under the title (a series' episode): the video-settings prompt's header and `Application.subtitle`; not in the window title or version info; an empty string is none |
| `company` | `${firstName}'s Company` | `CompanyName`, settings folder |
| `identifier` | `dev.platypus.${name}` | reverse-DNS id (`Application.identifier`); sanitised: each dot part keeps letters, digits, `-`, `_`, empty parts go, a part starting with a digit gets `_` |
| `version` | `0.1.0` | `FileVersion`, `ProductVersion` |
| `description` | `${title}` | `FileDescription` |
| `license` | `private` | `private` (all rights reserved), `custom`, or an SPDX id: MIT, Apache-2.0, GPL-2.0-only/-or-later, GPL-3.0-only/-or-later, LGPL-2.1-only, LGPL-3.0-only, AGPL-3.0-only, BSD-2-Clause, BSD-3-Clause, ISC, MPL-2.0, Unlicense, CC0-1.0, CC-BY-4.0, CC-BY-SA-4.0, Zlib, BSL-1.0, 0BSD (case-insensitive; `UNLICENSED` means `private`). Anything else is an error naming the file and line |
| `licenseFile` | none | path of the license text (needed for `custom`) |
| `copyright` | `Copyright (c) ${company}` (empty when the company is empty) | `LegalCopyright` |
| `icon` | none | `.png` (any size) or `.ico`, relative to the project |
| `exe` | `${title}` made file-name safe (quotes dropped) | file name, `InternalName`, `OriginalFilename` |
| `main` | first of `src/main.tsx`, `src/main.ts`, `src/index.tsx`, ... | entry file |
| `startScene` | none | scene loaded at start |
| `prebuild` | none | a command run in the game folder before compiling (`cmd /C` on Windows, `sh -c` elsewhere), e.g. to write generated sources (`md-reader`: `bun gen-docs.ts`); a failing command stops the build; `--no-prebuild` skips it |
| `layers`, `quality`, `display` | none | layer names, quality levels and default, window mode and size (as before in `platypus`) |
| `fps`, `runInBackground` | on / false | FPS counter, `Application.runInBackground` |
| `blending` | `srgb` | how translucent UI colours combine: `srgb` (as browsers) or `linear` (light-correct, the engine's earlier look); an unknown value warns and means `srgb`; not written back when `srgb` ([rendering.md](rendering.md#blending-srgb-default-or-linear-light)) |
| `theme` | `system` | which theme `prefers-color-scheme` matches: `system` (the system's light/dark preference, followed live) or `light` / `dark` (forced, on every platform and every window); an unknown value warns and means `system`; not written back when `system` ([platform.md](platform.md#system-theme)) |
| `videoSettingsPrompt` | off | ask the player once for window mode, resolution, vsync and quality before the window opens ([platform.md](platform.md#startup-video-settings-prompt)) |
| `dataDir`, `include`, `exclude` | `data`, `["assets/**"]`, none | data folder name beside the exe; what `package` copies to it: paths, folders or globs (`*` inside one path part, `**` across parts, `?`; case-sensitive; a plain folder means `folder/**`), minus the `exclude` globs. An `include` entry that matches no file is a warning |
| `windows`, `macos`, `linux`, `android`, `ios` | none | an object whose keys replace the top-level ones when building for that platform |

Unknown keys are kept as they are. `package` picks the platform from the host executable (PE: windows, ELF: linux, Mach-O: macos);
the app uses the platform it runs on.

**Template variables** (in any string of the file; the platform sections too):

| Variable | Value |
|---|---|
| `${username}` | account name (`%USERNAME%` / `$USER`) |
| `${fullName}` | Windows: `GetUserNameExW(NameDisplay)`, else the account name; macOS: `id -F` (else `dscl`); Linux: first comma part of the GECOS field of `getent passwd`; falls back to the account name when empty |
| `${firstName}` / `${lastName}` | first / last word of the full name (`lastName` is empty for one word) |
| `${year}`, `${date}` | `2026`, `2026-10-21` |
| `${name}`, `${title}`, `${company}`, `${version}`, `${identifier}`, `${description}`, `${copyright}`, `${exe}` | the resolved field (defaults included); a cycle is an error |
| `${env:NAME}` / `${env:NAME:-fallback}` | an environment variable; unset without a fallback is an error |
| `$${` | a literal `${` |

An unknown variable, an unterminated `${` or a cycle is an error printed as `project.json:3:13: `title`: unknown variable
`${firstNam}``, never a silent `undefined`. Command-line flags of `package` (`--title`, ...) replace fields **before** resolution
and are literal text (no templates), so `${title}` elsewhere sees them.

**Resolved at build time.** `package` resolves everything (user, date, platform) and writes the result into the exe's payload as
`project.json` with `"resolved": true` (read as it is: no templates, no platform sections, `dataDir`, `dataRequired` and the
payload icon name added) plus a small `package.json` (`name`, `version`, `description`, `license`). The npm `license` mapping:
`private` -> `UNLICENSED`, `custom` -> `SEE LICENSE IN <licenseFile>`, an SPDX id stays. So a packaged exe needs neither the
templates nor the person who built it. Non-private licenses: the license text (`licenseFile`, else `LICENSE`, `LICENSE.md`,
`LICENSE.txt`, `COPYING`, `UNLICENSE` in the game folder) is copied next to the exe (and into `data/` when `licenseFile` names it),
and the PE version info gets `Comments` = `License: <id>`. `custom` without a file is an error; an SPDX license without a file only
sets the metadata.

`platypus-build init [<dir>] [--name n] [--title t] [--company c] [--description d] [--icon file.png|ico] [--version v]
[--identifier id] [--license private|MIT|...] [--force]` writes the default `project.json` of the example above (the values you give
replace only their own line), refuses to overwrite one without `--force`, writes the default `tsconfig.json` (when there is
none) and the engine's declarations `types/platypus.d.ts` (what `platypus-build types` writes), so an editor opened on the
folder completes `Vector3`, `GameObject` or `useState` at once, creates a minimal `src/main.ts` (a `Hello` component
logging `hello`) when the folder has no `src/`, adds `build/`, `types/` and `*.meta` to a `.gitignore` that already exists
(the declarations are generated, as the repository ignores the examples' `types/`), copies `--icon`
next to `project.json` as `icon.png` / `icon.ico`, and prints what the templates evaluate to now:

```
wrote C:\Games\fox\project.json
wrote tsconfig.json
wrote types/platypus.d.ts (the engine's declarations, for editors)
wrote src/main.ts
as the templates resolve now:
  name         fox
  title        Matthew's Game
  company      Matthew's Company
  identifier   com.matthew.fox
  ...
```

### Editor files: `tsconfig.json` and `types/platypus.d.ts`

The compiler reads neither file; editors and the optional `tsc` check do. The default `tsconfig.json`
(`tools/platypus_build/src/tsconfig.rs`) is self-contained, with no `extends` outside the game:

```json
{
  "compilerOptions": {
    "target": "ES2022", "lib": ["ES2022"], "types": [], "module": "ESNext", "moduleResolution": "bundler",
    "moduleDetection": "force", "allowImportingTsExtensions": true, "noEmit": true, "strict": true,
    "noUncheckedIndexedAccess": true, "noImplicitOverride": true, "isolatedModules": true,
    "verbatimModuleSyntax": true, "skipLibCheck": true, "jsx": "react-jsx",
    "paths": { "*": ["./src/modules/*"] }
  },
  "include": ["src"],
  "files": ["types/platypus.d.ts"]
}
```

Both go stale (an edited file, an engine API that changed), so **`platypus-build init --editor-files [<dir>...]`** rewrites just
those two files of a game, or of every game directly inside a folder (`platypus-build init --editor-files ../examples`), and
touches nothing else (`project.json`, `src/`, `.gitignore`, the icon). Every example's `tsconfig.json` is this default (none had
anything else worth keeping: some lacked `jsx` or `paths`, which are harmless where unused). A game without a `tsconfig.json`
builds and is type-checked with the same default (the build writes it into `build/.tsc/tsconfig.json`).

## What a build does

1. **Config.** `project.json` (deprecated fallback: `package.json` `platypus`, see "`project.json`") -> `title`, `company`, `identifier`, `main`; `tsconfig.json` (comments and trailing commas allowed)
   -> `include` (default `src`) and `compilerOptions.jsx`. The entry is `main`, else the first of `src/main.tsx`, `src/main.ts`,
   `src/index.tsx`, `src/index.ts`, `main.tsx`, `main.ts`.
2. **Module graph** (`platypus_ts::modules`, below): the entry and every file it imports become one source text.
3. **`tsc --noEmit`** with the generated config of "Type declarations" below (over the game's `tsconfig.json`, or the default
   one for a game without it), when `tsc` is found
   (`node_modules/.bin/tsc` in the game folder or any parent, else on the `PATH`). A missing `tsc` is only a note; a failing one
   stops the build. `--no-tsc` skips it (`check` skips it unless `--tsc`; `check --full` prints every error).
4. **Compile.** The bundle is compiled with all engine manifests (the lists of `platypus_native::manifests()` plus `Ui`), so lowering
   and code generation errors show up now. Errors name `file:line` of the original file. `--aot` also writes `build/game.o` and
   `build/game_bindings.rs` (the object file for a release build that links the game into the executable). What the program
   needs from the renderer (its **tier**) comes from the compile: the engine symbols the code calls (unused top-level functions
   left out) and its component classes, plus `GameObject` and the component types of the scene and prefab files the build would
   pack (`platypus_ts::renderer`; [rendering.md](rendering.md#choosing-the-renderer)). A `GameObject`, a component or a scene
   file makes a game at least `world2d`; only a program without any (React, CSS, windows, timers, input, audio) is `ui`. `build` prints it (`renderer: world3d (Camera, Light, ...)`) and `check`
   shows it per game. `--no-compile` still lowers the program to find it (no code generation); only a program that fails to lower leaves it unknown (no `renderer` in the bundle, which the host takes as the 3D world).
5. **Stylesheets.** `import "./x.css"` lines are collected in import order into `build/game.css` (`css.rs`): `@import` is
   inlined; every `url()` is rewritten: a file up to 32 KiB becomes a `data:` URL, a larger one is copied to `build/` as
   `name-<hash>.ext` and the URL is relative to `game.css`. A `url()` that does not exist stays as written, with a warning (the
   Charm font is not in the repository).
6. **Assets.** `.meta` files are made for new assets (`assets.rs`, over the helpers in `platypus_core::meta`, which the editor uses too; existing ones and their GUIDs are kept; a moved file keeps its
   GUID when the move is unambiguous). Every string literal of the program that names an existing asset file under the game folder
   (`Texture.load("assets/x.png")`, `Model.load`, `loadFont`, `scenes/*.scene.json`, sounds) is copied with its `.meta` to the same
   relative path in `build/`. Paths follow `platypus_core::resolve_asset` (relative, no `..`, no drive letters or schemes). `--cook` then
   runs `platypus-app <game> --cook` (the engine's cooker) when `build/{release,debug}/platypus-app` (cargo's output) is found.
7. **`build/game.pbundle`**, written atomically (temporary file and rename).

## Dynamic asset paths and `build/assets.plan`

The build also reads the code's template literals and `+` chains with a literal folder first
(`platypus_ts::assetpaths`, an oxc visitor). Each hole becomes `*` (one path part): `` `assets/sfx/${n}.${ext}` `` is `assets/sfx/*.*`,
`"assets/" + name + ".png"` is `assets/*.png`. A pattern needs a literal folder before the first hole and a file name with a dot
that ends in `*` or an asset extension. `platypus-build build` prints one `note:` per pattern (`src/sfx.ts:60: dynamic path
assets/sfx/*.* reaches 79 file(s): ...`), warns about a pattern that reaches no asset file and about a whole asset-looking
literal (`"assets/a.png"`) that names no file, and records the patterns and their matches in `game.pbundle` as `patterns`
(`glob`, `at`, `matches`). `package` copies the matches too, so a narrowed `include` still ships what the code builds.

`build/assets.plan` lists, one per line, the files a packaged build would hold (include minus exclude, named assets, `startScene`,
pattern matches, `build/` copies from CSS). In the dev loop `platypus_app` reads it and `platypus_core::resolve_asset` logs once per path
`asset "assets/x.wav" loaded, but a packaged build would not include it (add it to "include" in project.json)` for a file that
exists but is not in the plan.

## `game.pbundle`

JSON, `format` 1:

```json
{ "format": 1, "title": "...", "company": "...", "identifier": "...", "entry": "src/main.tsx",
  "source": "<one TypeScript/TSX text>", "files": ["src/tween.ts", "src/main.tsx"],
  "css": "game.css", "assets": ["assets/coin.png"],
  "patterns": [{ "glob": "assets/sfx/*.*", "at": "src/sfx.ts:60", "matches": ["assets/sfx/a.wav"] }],
  "renderer": "world3d", "uses": ["Camera", "Light", "MeshRenderer"] }
```

`source` has no `@platypus/*` or `react` imports: the host compiles it with the engine manifests and the `Ui` manifest. A host reads
`format`, `entry`, `source`, `title`, `company`, `identifier`, `css` (relative to `build/`, or `null`), `assets` and `renderer`
(`ui`, `world2d` or `world3d`: what the program needs from the renderer; missing means `world3d`) with `uses` (the engine names
that set it, for the log).

## Module graph (`tools/platypus_ts/src/modules.rs`)

`platypus_ts::modules::bundle(entry)` (and `compile_program`) writes the files, dependencies first, into one text, by *editing byte
ranges* of the originals (comments and JSX stay as written; erased statements keep their newlines):

- `import ... from "@platypus/..."` and `"react"` are erased. Names come from the manifests and the ambient prelude; an aliased import
  (`import { Color as C }`) is renamed to the real name (`Color`). Default and namespace imports of libraries are erased and the name
  is left as it is.
- Relative imports (`./x`, `../y.ts`, `./dir` -> `dir/index.ts`; `.js` specifiers try `.ts`/`.tsx`) are followed, including
  `export { a as b } from`, `export * from` and chains of re-exports; an import is renamed to the final name of the declaration it
  reaches. `import type` is handled like `import`.
- `export` is dropped from every file except the entry (the entry's exports are the program's). `export default <expression>` becomes
  `const __default_<path> = ...`.
- Bare imports (`"foo"`, `"foo/sub"`) are files of the `modules/` folder next to the entry ("Modules and packages" below).
- Top-level names that two files declare are renamed in the dependency (`helper` -> `helper_2`); the entry's names win. Names the
  engine declares as globals (`Random`, `Vector2`, `Color`, ...) count as taken too (`platypus_build` passes the names of
  `types/platypus.d.ts`), so a module's own `class Random` becomes `Random_2` there and in what imports it, and the engine's `Random`
  elsewhere stays the engine's.
  Shorthand properties keep their key (`{ count }` -> `{ count: n_2 }`).
- Top-level statements run in source order in the program's `init`, together with the top-level `const`/`let` and expression
  statements: `if`/`else`, `for`, `for-in`, `for-of`, `while`, `do-while`, `switch`, `try`/`catch`/`finally`, labelled statements,
  `throw` and bare blocks (block-scoped `let`/`const` work; functions declared later in the file are hoisted). This holds for
  imported modules too. `return` at the top level is a parse error; any other statement kind the compiler cannot lower is an error
  `unsupported top-level statement: <kind>` with its line, never dropped silently.
- `import "./x.css"` is not read; it is listed in `Bundle::css` for the build tool.
- Not supported (an error naming the file): `import * as ns` of a project file, `export * as ns`, anonymous `export default function`
  or `class`, bare imports that are not in `src/modules`.
- Lowering is by reachability for plain top-level functions: one that nothing refers to (no call, no closure, not exported, not a
  prelude helper) is not lowered, so a construct the compiler lacks in dead code does not fail the build (`tsc` still checks it).
  Methods, constructors, exported functions, classes and top-level `const`/`let` initialisers are always lowered.
- `Bundle::locate_in(message)` rewrites `line N` of an error as `file:line`.

## Modules and packages

A game's own code can be split into modules and imported by a bare name. There is no `node_modules` resolution: a bare import is
looked up in the `modules/` folder next to the entry file (`src/modules/` for `src/main.tsx`):

| import | file tried (first found; `.ts`, then `.tsx`) |
|---|---|
| `from "foo"` | `src/modules/foo.ts`, `src/modules/foo/index.ts` |
| `from "foo/sub/file"` | `src/modules/foo/sub/file.ts`, `src/modules/foo/sub/file/index.ts` |

The same rules as relative imports apply (`.js` specifiers try `.ts`, `index` files, re-exports, `import type`), files inside a module
may import each other relatively or by name, and a file reached twice is in the bundle once. `@platypus/*` and `react` are always erased
and win over a module of that name. `import "./x.css"` is unaffected. `examples/hello` has `src/modules/greet.ts`, imported as
`import { greeting } from "greet"`.

```ts
// src/modules/mathx/index.ts
export function lerp(a: number, b: number, t: number): number { return a + (b - a) * t; }
// src/main.ts
import { lerp } from "mathx";
```

**Using a TypeScript package:** copy its `.ts` source into `src/modules/<name>/` (keep its licence file next to it) and import it by
`<name>`. It compiles only if its code is in the subset platypus_ts supports (typed function bodies, classes, generics, no dynamic
features). What will not compile:

- CommonJS / UMD packages (`require`, `module.exports`), and packages that ship only `.js` plus `.d.ts` (the compiler needs types
  inside function bodies, not only on exports);
- Node built-ins (`fs`, `path`, `Buffer`, `process`, ...) and anything that touches the DOM;
- `any`, `eval`, `with`, `arguments`, prototype manipulation and other dynamic code.

Errors:

- `cannot find module "foo": bare imports are looked up in src/modules/ (tried src/modules/foo.ts, src/modules/foo.tsx,
  src/modules/foo/index.ts, ...). To use a TypeScript package, copy its .ts source into src/modules/<name>/.`
- `<file>: has no export `x``, and `<file:line>: <construct> is unsupported` from lowering, which names the file and line inside the
  module.

**`tsc` and editors.** The generated `build/.tsc/tsconfig.json` sets `compilerOptions.paths` to `{ "*": ["../../src/modules/*"] }` (the
entry's folder), so `tsc` resolves the same files as the compiler. For an editor, add the same to the game's `tsconfig.json`:

```json
{ "compilerOptions": { "paths": { "*": ["./src/modules/*"] } } }
```

(all the examples have it). **Hot reload:** `platypus-build dev` watches the game folder recursively (`build`, `node_modules`, `types` and
dot folders aside), so a change in `src/modules` rebuilds and reloads like any other source.

What was decided *not* to build (`node_modules` resolution, a packages section in `project.json`, licence generation, fetching from
registries, `any`, native plugins) is in `docs/research/platypus-external-packages.md`.

## Type declarations (`platypus-build types`)

**A game needs no import.** Every engine and UI name is a TypeScript **global** in `types/platypus.d.ts`: `Vector3`, `GameObject`,
`Component`, `register`, `Physics`, `Time`, `input`, `audio`, `Screen`, `createRoot`, `Text`, `useState`, `PlatypusStyle`, ... The compiler
resolves them from the manifests and the ambient prelude whether or not the file imports anything. The old forms still work:
`import { Vector3 } from "@platypus/engine"`, `import { useState } from "react"` and `import { createRoot } from "@platypus/react"` type-check
(the file also declares those three modules, each as `export import X = globalThis.X` re-exports) and the compiler erases them.

The file (`platypus_ts::types_dts`, one deterministic file, about 3000 lines) is, in order:

1. **The manifests** as top-level `declare class` / `declare namespace` / `declare type`: handle and value classes with constructors
   and fields, statics, methods, properties, namespaces; callbacks as function types; no doc comments (the manifests have none).
   Two rules add the shapes the compiler accepts over the native positional API: every call whose trailing parameters can be given by
   name gets an **options-object overload** (`audio.load(path, { stream: true })`, `Sound.play({ loop: true })`), and a creatable
   handle class (one with `static create`) gets constructors `new Material()`, `new Material({ color })`, `new GameObject(name, { .. })`
   and `Class.create({ .. })` with its writable properties as keys. A component handle (it has `gameObject`) also gets `transform` and
   `getComponent(Class)`. A parameter list never has a required parameter after an optional one (the later ones become optional).
2. **The UI declarations** (`tools/platypus_ts/src/ui_dts.rs`, from the tables of `crates/platypus_ui/src/tables`): `PlatypusStyle` (one entry per style
   property: name, TypeScript type, doc; `platypus_ui` tests that `apply_style` applies each and that nothing it applies is missing), the
   event interfaces and `PlatypusProps` (attributes and `on*` props with their event types), `JSX.IntrinsicElements` and the
   `Text`/`Span`/`Input`/`TextArea`/`Image`/`List`/`Dialog` components (from the element table), the hooks and `createRoot` (raw
   TypeScript signatures kept as data next to the tables), `PlatypusElement` (the `UiNode` class of the `Ui` manifest) and `console`. The
   same tables write the compiled side (`StyleValue`, `StyleEntry`, the `UiEvent` class whose fields read the event slots of
   `UiEvent::script_args`) and decide which event type an `on*` prop listens to (`onChange` is `input`, `onFocus` is `focusin`).
3. **The compiler built-ins** (`tools/platypus_ts/src/builtins.rs`): one entry per declaration the compiler itself gives a program, with
   its signature and doc and the code that lowers it named next to it: the `Component` base class (written from the hooks the host
   calls, `components::HOOKS` / `hook_param`), `register`, `property`, `ComponentClass`, `ClassOf`, the timers (`setTimeout`,
   `setInterval`, `clearTimeout`, `clearInterval`, `queueMicrotask`), `GameObjectOptions` / `InstantiateObjectOptions` and the
   generic `GameObject` members (`addComponent`, `getComponent`, `findObjectOfType`, `instantiate`, `createPrimitive`, ...), the typed
   window messages (`postMessage<T>`, `request<R>`, `emit` / `on` / `ask<R>` / `answer`, `Window.broadcast` / `emitAll`),
   `Random.pick<T>`, `NumberList`, and the members whose script type is wider than the native one (`MeshRenderer.mesh` takes a
   primitive's name, `AudioSource.playOneShot` a path: `coerce_ext` calls the handle's `fromString`; `Color`'s optional alpha). A
   member goes into the generated class or namespace of its owner and replaces a native member of the same name (a static of a
   namespace is a `function` / `const`); a constructor replaces the generated ones. The `<Value>Like` types (`Vector3Like`, ...)
   are written for the value classes the declarations name.
   No `.d.ts`, no `include_str!` of one and no Python file under `tools/` is written by hand: the guard `M9-TS-3100`
   (`tools/platypus_build/tests/generated_dts.rs`) is strict, and `M9-TS-3101` checks that no built-in declares an engine name again and
   that every class a built-in member belongs to exists.
4. **The three modules** (`@platypus/engine`, `@platypus/react`, `react`) as re-exports.

`platypus-build build` passes the file to `tsc` itself: it writes `build/.tsc/platypus.d.ts` and `build/.tsc/tsconfig.json` (which extends the
game's `tsconfig.json`, or holds the default options when the game has none, keeps its `include`, lists the declarations in `files`, and sets `moduleDetection: force` so top-level names of
a file never merge with the engine globals, and `jsx: preserve` so no `react/jsx-runtime` is needed) and runs `tsc -p` on it. A game
folder therefore needs **no `node_modules`, no `@platypus/*` or `react` packages and no `types` entry in its `tsconfig.json`**; `tsc` is
found in `node_modules/.bin` of the game folder or a parent, in the checkout the tool was built from, or on the `PATH`. Editors
read the game's own `tsconfig.json`, whose `files` names `types/platypus.d.ts` (`init` writes both; `init --editor-files`
refreshes them). A top-level name of a game that is also an engine name (`class Scene`) shadows the
engine's inside that file; the compiler lowers one program, so such a name is renamed or avoided (the examples do).

## The dev loop (`platypus-build dev`)

The watcher polls modification times (200 ms; no dependency) of everything in the game folder except `build`, `node_modules`,
`target`, `types` and dot folders, waits 100 ms for a second save, rebuilds, and writes `build/.platypus-reload` **after**
`build/game.pbundle`:

```json
{ "seq": 7, "ok": true }
{ "seq": 8, "ok": false, "error": "text of the build error" }
```

The app watches that one file; a new `seq` means reload `game.pbundle` (on `ok: false` log the error and keep the old code; the old
bundle is untouched). Hot reload keeps state through scene save/restore and `onReload` (see [scripting.md](scripting.md#hot-reload)).

## Packaging a single executable (`platypus-build package`)

```
platypus-build package <game> [--host platypus-app.exe] [--out file] [--no-build] [--no-tsc]
    [--exe name] [--title t] [--company c] [--version v] [--copyright c] [--description d]
    [--icon file.png|.ico] [--sign-cmd "signtool sign /fd sha256 ... {exe}"] [--data-dir name] [--no-data]
    [--packs [--report]] [--video-settings-prompt | --no-video-settings-prompt] [--renderer vulkan|gles|cpu|auto]
    [--no-notices] [--debug-info]
```

**Debug info.** `build` writes `build/game.pdebug` beside `game.pbundle`: a hash of the bundled source and where each source file
starts in it. It is not in the bundle or the exe's payload. `package --debug-info` copies it beside the exe as `<exe>.pdebug`; the
host looks for it there, then in the game folder's `build/`, when a crash happens in compiled code, and the crash report then
names `src/file.ts:line` instead of "bundled source line N". A file from another build (hash differs) is ignored with a note.
Design and plan: [research/platypus-ts-debug-info.md](research/platypus-ts-debug-info.md).

`--video-settings-prompt` / `--no-video-settings-prompt` set `videoSettingsPrompt` in the exe's own `project.json`, over the game's
([platform.md](platform.md#startup-video-settings-prompt)). `--renderer` sets the exe's `renderer` the same way (`auto` removes
it); a UI-only one for a game that uses the world is warned about with the build's tier.

**Third-party notices.** Every packaged exe gets `THIRD-PARTY-NOTICES.md` written beside it: the notices of everything the
host is built from (ANGLE and the code it bundles, the Inter font, every Rust crate of the host in `Cargo.lock`, `ufbx` for FBX models among them). The text is
built into `platypus-build` (`include_str!` of the repository root's file), so a game folder outside the repository gets it
too; ship it with the exe. `--no-notices` leaves the file out (the exe still prints the same text with
`platypus-app --licenses`, which is built into the host the same way).

The file is generated by `tools/platypus_notices` (`cd tools && cargo run -p platypus_notices`; `--check` only compares):
it reads `cargo metadata --offline` for `platypus_app` on `x86_64-pc-windows-msvc` (normal and build dependencies) and
each crate's `LICENSE*` / `COPYING*` / `NOTICE*` / `UNLICENSE` files from the local cargo registry, then writes
`hand-written.md` verbatim followed by one section per licence: the crates with their version, link and copyright lines,
the licence text once, other wordings of MIT/BSD-style texts de-duplicated, and a last section of NOTICE files and the
licences of code bundled inside crates (Zstandard, ...). A crate offered under a choice
(`MIT OR Apache-2.0`) is listed under the first of MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-3-Clause,
BSD-2-Clause, ISC, Zlib, Unicode-3.0, BSL-1.0, 0BSD, Unlicense, CC0-1.0, MPL-2.0 that it ships a file for; `AND` lists
it under each; MPL-2.0 crates say they are used unmodified, with their source link. It fails with the list on an
unknown licence, a GPL/LGPL/AGPL with no other choice, a crate without a licence file, or a licence file inside a crate
that `tables.rs` does not list, unless the reviewed tables there allow it with a reason. Regenerate it whenever
`Cargo.lock` changes: the `notices_are_current` test (`M9-LIC-02`) fails until the committed file matches (it skips when
`cargo metadata --offline` cannot run or a registry source is missing).

It builds the game (unless `--no-build`), then writes `<game>/build/<exe>.exe` (no extension on other systems): a copy of the
engine host (`platypus_app`) with the game's **icon and version info** written into its PE resources (Windows hosts only) and the game's
**code appended as a payload**, and copies the **assets into a `data/` folder beside it**. Assets are never embedded: games have
gigabytes of them. The result runs from any folder as long as `data/` is next to it; `platypus-app <game folder>` is unchanged.

```
build/                     (or the folder of --out)
  mygame.exe               engine host + icon + version info + payload (code only, a few hundred KiB over the host)
  data/                    --data-dir <name> renames it; --no-data does not write it
    assets/...             the same relative paths as in the game folder (textures, models, sounds, fonts, `.meta`)
    scenes/...             the start scene and everything else the game names
    build/<font>.ttf       the files the CSS references (hashed fonts and images)
```

The command prints the exe size, the number and size of the embedded files, and the number and total size of the copied asset
files. `--data-dir <name>` is one plain folder name (no separators, not hidden); the exe is told the name (`dataDir` in its
payload's `project.json`). `--no-data` is for a build that packs the assets itself: nothing is copied, the exe still looks for the
folder (or `--data` / `$PLATYPUS_DATA`). Nothing is deleted from an existing data folder; files with the same path are overwritten.

### `--packs`: assets in packs (opt-in, phase 1 of `docs/research/platypus-virtual-fs-plan.md`)

`platypus-build package <game> --packs` writes the data files as `.ppk` packs (`crates/platypus_pack`, LZ4 by default; `--codec zstd` uses the `zstd` crate, one frame per 256 KiB chunk so ranges still read per chunk; `platypus-build pack-info` shows the codec of every entry, `index.json` the pack set's) instead of loose files:

```
data/
  index.json               the packs in mount order (name, file, role, id, entries, bytes, scene)
  assets.dat               what the code may load at any time (named by the code, reached by a dynamic path, selected by
                           `include`, `build/` copies) and the assets two or more scenes share
  scene-<slug>.dat         one scene file (the first entry) and the assets only that scene uses
```

Placement (plan section 2.5): a file taken for any reason other than being named by a scene goes to `assets.dat`; the assets a scene
JSON names (any string that is an existing game file, a `guid:` source found through the `.meta` files, prefabs followed) go to that
scene's pack when it is the only user and to `assets.dat` when two or more scenes use them; a scene file always has its own pack, even
when `include` selects it. Scene references outside `include` are packed too. A `.meta` with `"importer": {"pack": false}` keeps a
file out (unless the code names it). The `.meta` files themselves are not entries: the GUID and the importer settings go into the
entry. Pack keys are exact-case; two keys that differ only in case are an error. mp3, ogg, png, jpg, flac and woff2 are stored raw.

The build prints, per pack, the entries by reason (`assets.dat: 157 entries (code 7, include 1, patterns 149), ...`) and the first
files of each reason; `--report` lists every file. The output is deterministic (the same files give the same bytes) and an old
`assets.dat`, `scene-*.dat` or `index.json` in the folder is removed first. Textures and models stay in **source form** for now (no
cooker in the build; a note says so); both cooked and source forms is the decision for later, with a note to revisit cooked only.
`platypus-build pack-info <file.dat>` lists a pack: version, id, entries, codec totals and one row per entry (kind, size, stored size,
codec, GUID, path).

**A running game reads them** (phase 2, see "Where the assets are read from"): `platypus_app` mounts `data/index.json` through `Vfs`. The
default stays loose; flipping it is a TODO (`tools/platypus_ts/TODO.md`, asset packs). `emoji` with `--packs`: one `assets.dat`, 157
entries, 6.41 MB raw, 5.93 MB on disk.

**The host** is `--host`, else `$PLATYPUS_HOST`, else the newest `platypus-app` next to `platypus-build` and in `build/{release,debug}/` (cargo's output, `$CARGO_TARGET_DIR` when set)
found going up from the game folder and the current folder, by modified time (a stale release build no longer wins over a fresh
debug one; it lacked payload support and died with a missing `game.js`). `package` prints `host: <path>` first. A host that already carries a payload is stripped first, so
a packaged exe can be re-packaged. A debug host is ~77 MB, a release one is much smaller: ship release.

**Metadata** (command line, then the `project.json` field, then the default; the table lists the flags, the fields are in "`project.json`"):

| Field | `platypus.` key / flag | Falls back to | Used for |
|---|---|---|---|
| title | `title` / `--title` | `${firstName}'s Game` | window title, `ProductName`, settings folder |
| exe | `exe` / `--exe` | the title (made file-name safe; `.exe` is dropped) | file name, `InternalName`, `OriginalFilename` |
| company | `company` / `--company` | `${firstName}'s Company` | `CompanyName`, settings folder |
| version | `version` / `--version` | `0.1.0` | `FileVersion`, `ProductVersion` (`1.2.3` -> 1.2.3.0; a `-pre` suffix is dropped from the numbers, kept in the text) |
| copyright | `copyright` / `--copyright` | `Copyright (c) <company>` (empty when the company is empty) | `LegalCopyright` |
| description | `description` / `--description` | the title | `FileDescription` (what Task Manager shows) |
| icon | `icon` / `--icon` | the Platypus icon (`resources/icon/icon.png`) | exe icon; window and taskbar icon at run time |
| identifier | `identifier` | `dev.platypus.<name>` | `Application.identifier` |

The icon is a `.png` (any size; centred on a transparent square when not square) or an `.ico`. A PNG is converted in-process
(`icon.rs`, `image` crate, Lanczos3 to 256 px and the smaller sizes from that) to an `.ico` of 16, 24, 32, 48, 64, 128 and 256 px
(PNG-compressed, the container from `platypus_core::icon`), and `pe.rs` writes it with the `editpe` crate (the whole `.rsrc`
section is parsed and rebuilt; no `rc.exe`, no other tool). The version info is a VS_VERSIONINFO block with the string table
`040904b0` (US English, Unicode) and the matching `Translation`; an empty title makes `ProductName` `Platypus`, and
`FileDescription` falls back to the title, then `Platypus`. The same icon file goes into the payload (`platypus-icon.png|ico`) and
`icon` of the payload's `project.json` points to it, so the window and taskbar icon come from the same file.

**The Platypus icon is the default.** `resources/icon/icon.png` (1254 x 1254, built into `platypus_core::icon::DEFAULT_PNG` with
`include_bytes!`, so no file is needed at run time) is used wherever a game has none: `package` writes it as the exe's icon
(replacing the host's), and the host's build script (`crates/platypus_app/build.rs`) turns it into a 256 x 256 window icon that
both hosts set for a game without `icon`, and, on Windows with the MSVC linker, into a `.res` file (`platypus_core::icon::windows_res`:
the seven sizes as the `MAINICON` group and version info with `ProductName` / `FileDescription` `Platypus`) linked into
`platypus-app.exe`, so plain `cargo build` binaries show it too. Tests `M9-ICO-01..06`.

**The payload** (`platypus_core::payload`, format version 1) is appended after the host (and after the resource edit):

```
[host exe][entry data...][table of contents][32-byte trailer]
trailer: "IRONPAYL" | version u32 | reserved u32 | payload_len u64 | toc_len u64   (little endian)
toc:     count u32; per entry: path_len u16, path, offset u64, stored_len u64, raw_len u64, method u8 (0 stored, 1 deflate), crc32 u32
```

Entries are deflate-compressed (`flate2`, stored when it does not help or under 64 bytes); every one has a CRC-32 that is checked on
read. Paths are relative, `/` separated and checked (`payload::safe_path`: no `..`, empty or `.` parts, drive letters, backslashes);
a table with an unsafe path is refused. **What goes in** (paths relative to the game folder):

- `build/game.pbundle` and `build/game.css` (CSS with its small `data:` URLs inlined by the build).
- `project.json`, the resolved project (so `Application`, the window, the icon and the settings folder agree with the exe's
  version info) plus `dataDir` (the folder name) and `dataRequired` (see below), a small `package.json` (name, version,
  description, npm `license`), and `platypus-icon.*`.
- **Essentials**: the assets the bundle itself names (`assets` in `game.pbundle`, no `.meta`), but **only if together they are at
  most 256 KiB** (`ESSENTIALS_LIMIT`, all or none: one byte over and none is embedded). A large file is never embedded.

Everything else is **not** in the exe: textures, models, sounds, fonts, scene files, `.meta` files. Those are the **data files**,
copied to `data/` with the same relative paths:

- every asset the bundle names, `startScene`, everything `include` selects (paths, folders, globs; **default
  `["assets/**"]`**, the whole folder, referenced or not, because the build finds assets only by string literals and misses paths
  built at run time or named inside CSS text) minus `exclude`, and the files the code's dynamic paths reach (below), each with its
  `.meta`. `exclude` trims `include` and the dynamic matches, never an asset the code names by a whole literal. The game
  folder's copy is used, not `build/`'s;
- the other files of `build/` (hashed fonts and images the CSS references), as `data/build/...`. Not `game.o`,
  `game_bindings.rs`, `*.exe`, `game.js`, `vendor.js`, source maps, dot files or the output's own data folder.

`dataRequired` is **false** when the game has no data files (hello) or when the embedded essentials are all of them (a `.meta`
file does not count): such a game also starts without a `data/` folder. Otherwise it is true.

**How the exe finds its payload.** At startup `platypus_app` (`payload.rs`, called from `parse_args` in `main.rs`)
opens its own executable and looks for the trailer at the end of the file (a plain `platypus_app` has none: `Ok(None)`, normal start). If
the file is a signed PE it also looks right before the Authenticode certificate table (data directory 4, 8-byte aligned), so a payload
survives signing. The payload's table of contents is hashed into an id; the files are extracted once into
`<per-user cache>/Platypus/games/<exe name>-<id>/` (`%LOCALAPPDATA%`, `~/Library/Caches`, `$XDG_CACHE_HOME` or `~/.cache`) beside a
`.platypus-extracted` marker (extracted to a temporary folder and renamed, so a crash or a second instance never sees half a game), and
the game runs with that folder as its **code folder** (bundle, CSS, `project.json`, icon): `--screenshot` and `--frames` still apply,
any folder argument is ignored. A new build has a new id, extracts again and removes the older folders of the same exe name. An
unchanged game starts at once. The extracted folder is small (code only), so a second copy costs almost nothing.

**Where the assets are read from.** The *asset root* (`crates/platypus_app/src/asset_root.rs`, `AssetRoot`, `locate_data`) is separate
from the code folder; a game folder run has the same folder for both, as before. For a packaged exe the first of these that exists:

1. `--data <folder>` on the command line (an error if it is not a folder),
2. `$PLATYPUS_DATA` (same),
3. `<folder of the exe>/<dataDir>` (default `data`),
4. the extracted code folder, but only if `dataRequired` is false (the essentials are there; hello needs nothing).

Otherwise the program stops with an error that names the folder it looked for and the two overrides, e.g. "the game's data folder
was not found (looked for `C:\Games\mygame\data`). Put the `data` folder that `platypus-build package` wrote beside the program, or
pass --data <folder> or set PLATYPUS_DATA." (to the log and stderr; the exe is a GUI program, so start it from a terminal to see it).
Textures, models, sounds, fonts, scenes, `url()`s and `@import`s in the CSS (rebased onto the CSS file's place in the code folder, so
`build/font.ttf` is found as `data/build/font.ttf`), GUID `.meta` files and the asset watcher all use the asset root. `Application`,
the window title and icon and the PlayerPrefs folder come from the code folder's `project.json` (the identifier), not from the data.

**The file system over that folder** (`asset_root::mount`, `platypus_pack::Vfs`; `docs/research/platypus-virtual-fs-plan.md` phase 2). At
start `platypus_app` mounts, bottom to top: the folder itself only when it holds no packs (a game folder in the dev loop, or a loose
`data/` from an older package), else `assets.dat` and then every scene pack, from `index.json` (any `assets.dat` / `scene-*.dat` are
found by name when there is no manifest). All packs are mounted at start, never per scene, so a script can load a path that only
another scene's pack holds. Lookups go from the top: scene data, then `assets.dat`, then the folder. `platypus_core::set_asset_fs`
installs it for every loader; `platypus_core::read_asset(root, path)` (which falls back to `root/path` when none is installed, as
tests do) is the one call textures, sprites, models (and the textures a glTF names), sounds, fonts, `url()` / `@import` in CSS, SVG
images, cursors, scenes, prefabs and `Assets`/`asset.readText` make. A streamed sound (`audio.load(path, true)`) is read in 256 KiB
windows through the pack's chunk table (`Vfs::open_range`, `platypus_core::RangeRead`), so a big compressed sound is never inflated whole.
The asset database scripts see (`Assets`, fed by `asset_db::scan`) lists the `.meta` assets of the folder and of the packs. Packs are read-only:
the asset watcher runs only over a folder.

Errors name where the lookup went, top to bottom: `asset "assets/sfx/boom.wav" not found in scene data, assets.dat or folder`
(`not found in assets.dat` for a packaged game, `not found in folder` in the dev loop), with ` (the file on disk is "assets/sfx/boom.wav";
asset paths are case-sensitive)` added when only the case differs. Pack keys are exact-case; loose lookups follow the OS. Where the OS
ignores case (Windows, macOS) a loose load of `assets/SFX/Boom.wav` still works in the dev loop and logs once
`asset "assets/SFX/Boom.wav": the file on disk is "assets/sfx/boom.wav" (packs are case-sensitive)`. The dev `assets.plan` warning
(`M9-API-1143`) fires on loads only, not on listing.

`--sign-cmd "<command>"` runs after the file is complete: `{exe}` is replaced by the quoted path and the command goes through
`cmd /S /C` (Windows) or `sh -c`. A failing command fails the package. Nothing is signed by Platypus itself; see
`docs/research/platypus-code-signing.md` (order: resources and payload first, sign last).

**Not done** (also in `tools/platypus_ts/TODO.md`): macOS `.app` bundle with `Info.plist` and `.icns`, Linux `.desktop` file and icon
theme entry, the X11/Wayland window class (winit's default is used), the Windows `AppUserModelID` for taskbar grouping, a Windows
application manifest (DPI awareness, long paths), cooked assets (`.platypus/cooked`) in the payload, an optional in-memory file system.

### Linking an `--aot` object

`build/game.o` is the whole program (the game, the parts of the prelude it uses, its string literals, type tables and
method tables) as an ELF / COFF / Mach-O object of the target. A host that links it needs no compiler: not `platypus_ts`,
oxc or Cranelift, only the runtime `platypus_ts_rt`, the UI bridge `platypus_ts_ui` when it has UI, and the crates whose
namespaces the manifests named. Fluid links its apps this way (Fluid's `docs/native-apps.md`, measured there: 34.9 MB
against 52.0 MB stripped for the same host with the JIT).

- **Symbols.** The program exports `ts_<prefix><name>` (`platypus-build` uses the prefix `game_`; `ts_game_init` runs the top
  level). Everything else is local. Its imports are the runtime's and the namespaces' `#[unsafe(no_mangle)]` shims, by the same
  names the JIT resolves, so the linker resolves them from the rlibs. Several programs link into one executable when each has its
  own prefix (`platypus_ts::compile_for_target`).
- **Starting it.** `platypus_ts_rt::startup_reset()`, then the program's `init`, as `jit::Game::init` does. A component game
  also takes `classes()` from `build/game_bindings.rs` (the table `Game::classes` builds at run time for the JIT).
- **Struct ids.** The object's struct headers are numbered from the `struct_base` it was compiled with (0 for `platypus-build`).
  The runtime's struct table is dense, so programs that run in the same process need disjoint ranges, and a program alone in
  its process can use 0. A process that also JIT-compiles needs the JIT's ranges (`JitOptions::struct_base`) apart from it.
- **PIC.** Without `--pic` the code uses absolute addresses (`R_X86_64_64` in `.text`), which a PIE link refuses; with it,
  calls go through the PLT and data through the GOT (relaxed by the linker), which every Linux link accepts.
- **What it doesn't have:** hot reload (`platypus-build dev` swaps JIT code) and anything compiled at run time.

### The same bundle on iOS and Android (plan only)

Nothing here is built yet. The inputs are the same: `game.pbundle` (or the `--aot` object `build/game.o` + `game_bindings.rs`),
`game.css`, assets and the metadata of the table above.

- **iOS** forbids writable-executable memory, so the Cranelift JIT that `platypus_app` uses to compile `game.pbundle` at start cannot run:
  the game must be built with `--aot` (an object file for `aarch64-apple-ios`, linked into the app binary with the engine host as a
  static library) and the bundle's source is not shipped at all. The app is an `.app` folder (host binary, `build/`, `assets/`,
  `Info.plist` from the metadata: `CFBundleName` = title, `CFBundleIdentifier` = identifier, `CFBundleShortVersionString` = version,
  an asset catalog from the icon), assets read from the app bundle (read-only, so no extraction), signed with a provisioning profile and
  uploaded as an `.ipa`. Needs a Mac for the linker, `codesign` and Xcode's tools, and an Apple developer account.
- **Android** takes the host as a `cdylib` per ABI (`arm64-v8a`, ...) in an APK or AAB (`NativeActivity` or GameActivity glue from
  `winit`'s android backend), with the `--aot` object linked in for release (JIT is allowed there, so a debug build can still compile
  the bundle at start). Assets go into the APK's `assets/` (read through the NDK `AAssetManager`: the asset reader needs a small
  seam, since there is no folder to extract to cheaply), the icon into `mipmap-*` densities, the identifier is the `applicationId`
  and the version fills `versionName` / `versionCode`. Packaging (`aapt2`, zip alignment, `apksigner` or bundletool) can be done
  from Rust or by calling the SDK tools; signing needs a keystore.
- Both need the `--aot` path to cover all of the engine API first (today the examples run through the JIT) and a mobile windowing and
  input layer in `platypus_platform`.

## Benchmarks (`platypus-app --bench`, `platypus-build bench`)

`platypus-app <game> --bench` runs a game with **no OS window**: the main window is an offscreen texture
(`platypus_app/src/frame_target.rs`), the platform is `platypus_platform::FakePlatform` (secondary windows virtual, as in screenshot
runs), and there is no vsync or frame cap. Each frame waits for the GPU work of the frame before it (like a swapchain queueing one
frame), so GPU time counts. It warms up, measures, and writes JSON (`--bench-out file`, else stdout): fps, mean / median / p95 /
p99 / worst frame ms, and the mean split into `script` (game code: hooks, timers, the UI it renders), `sync` (a compiled game's
`sync_native_render`: animation, splines, cameras, registries into `WorldViews`),
`render` (layout and draw list, world, GPU wait) and `other`; start-up (process start to the first frame), load time and peak
working set. Finer splits: `syncParts` (where `sync_native_render` goes: resources, line renderers, settings, each kind of
component, panels, texts, ...; `platypus_app/src/sync_profile.rs`), `startupParts` (log and asset mount, bundle read, asset index,
UI tree and fonts, the compiler's phases from `platypus_ts::timing`: engine manifests, source rewrites, oxc parse, type inference and
lowering, throw analysis, IR passes, Cranelift code generation, JIT linking; the program's top level and starting components; the
headless GPU device, renderer set-up and the first frame; plus the program's and prelude's line counts). `backend` is always `"native"`
(the QuickJS-only `opsApplyMs` and `opMarshalMs` are gone). The timers run only in bench runs. Flags: `--warmup N` (120), `--frames N` (600), `--fixed-dt s` (every frame advances the game's clock by exactly
`s`, so runs are comparable whatever their speed), `--size WxH`, `--bench-shots dir` (a PNG after each segment, to check the
input did what it should).

`--bench-input spec.json` scripts the run: `{"warmup": N, "segments": [{"name", "warmup", "frames", "input": [...]}]}`. Input
frames count from the segment's start (its warm-up included); kinds are `key` (`code`, optional `key`, `hold` frames),
`keyDown` / `keyUp` (after each Ctrl / Shift / Alt / Windows key press or release the modifier state is sent too, as a window does, so the UI sees Ctrl+Z; `M9-BEN-04`), `mouseMove` / `click` (`x`, `y` in window pixels, `button`), `mouseDown` / `mouseUp`, `wheel` (`dy` lines),
`clickText` / `moveToText` (the middle of the first screen text reading `text`, found when the frame comes, through transforms: `UiTree::to_window`, as a centred modal dialog is placed by a translate), and gamepad `axis` /
`button`; `count` and `every` repeat one. They are sent as the platform sends the OS's (`window_input`), so both backends see
the same input. The examples' scripts are in `tools/bench/inputs/<name>.json` (the example sources stay unchanged).

`platypus-build bench <examples> [--only a,b] [--no-host-build] [--frames N] [--timeout s]` builds the
release host (`cargo build --release -p platypus_app`) and every example natively, and times only the native backend (the
QuickJS originals, `--base` and `--no-quickjs` are gone). It
runs them with `--fixed-dt 1/60 --size 1280x720` and writes `docs/benchmarks/results.json` and the generated part of
`docs/benchmarks.md` (between its `bench:begin` / `bench:end` markers; text around them is kept; the markdown has fps,
median / p99 and the script / sync columns). To re-run:

```bash
cd tools && cargo run -q -p platypus_build -- bench ../examples
```

## Tests

`M9-BEN-01` bench files and frame stats, `M9-BEN-02` the page, `M9-BEN-03` (needs a GPU) a windowless bench run of a native
game with scripted keys.

`M9-API-1000..1005` the payload format (round trip, no trailer, unsafe paths, damage and duplicates, extraction and id, a payload before
a certificate table), `M9-API-1010..1011` the runtime side (`platypus_app/src/payload.rs`), `M9-TS-2400..2402` metadata, `2410..2412` file
collection and a full package with the sign hook, `2420..2421` icon conversion, `2430` the PE resource round trip, `2440` (ignored
unless asked: `cargo test -p platypus_build -- --ignored standalone`, needs a built host and a GPU) the standalone run,
`M9-API-1200..1205` the asset root and mounting (`platypus_app/src/asset_root.rs`), `M9-API-1145..1149` lookup order and the not-found message, the case check, ranged reads, sounds and SVGs from packs, `M9-API-1144` zstd chunks, `M9-TS-2550..2553` globs, dynamic path inference, include/exclude and the packaged emoji run, `2554..2559` the pack build (placement, determinism, byte-identical read back, case), `M9-API-1143` the dev `assets.plan` warning, `M9-TS-2600..2603` the payload/data split, the data-folder name and
`--data-dir` / `--no-data`, `2610` (ignored, `-- --ignored standalone_data`) a packaged game reading `data/` and the missing-folder error.

`M9-API-1300..1318` the project loader (`platypus_core/src/project.rs`: templates, escapes, errors with line, name splitting, platform sections, package.json fallback, license, required `name`, silent defaults, empty fields: `1321..1322`), `1320` the app reads it, `M9-TS-2700..2705` `init`, `2710..2711` templates and license in a package, `2720..2721` the build config and the examples (found by `project.json`, none has a `package.json`), `2722` and `2724` the empty-field warnings (build, package; not check), `2723` package default warnings, `2725` game discovery.

`M9-TS-1800..1806` module graph, `1810` declarations, `1830..1831` `.meta` files, `1840` CSS bundling, `1850..1852` config and a
full build, `1860..1861` the dev status file and the change fingerprint. `bun run typecheck` (`platypus-build check ../examples --tsc`, from `tools`)
builds all 45 examples and prints a table; failures there are missing language or engine features, listed in
`docs/research/platypus-examples-on-platypus-ts.md`.
