# Virtual file system and `data/assets.dat`: assets the build cannot see (plan)

Status: **research and plan, 2026-10-22.** Nothing here is built. This note builds on, and amends, the asset-pack design
(`docs/research/platypus-asset-packs.md`, "the pack design") and its step list (`docs/research/platypus-asset-packs-plan.md`, "the pack
plan"); read sections 2 to 6 of the pack design first. The container (`crates/platypus_pack`: `.ppk`, `PackWriter`, `Pack`, `Vfs` over
`DirSource` / `PackSource`) exists and is tested (`M9-API-1100..1112`); nothing in `platypus_app` or `platypus-build` uses it yet.

The user's request: asset paths are not always fixed. `examples/emoji` builds them from templates (`assets/sfx/${n}.wav`), which
`platypus-build` cannot see, so a packaged build can miss sounds and art. Put asset extras into `data/assets.dat`, a flat table of entries
with no directory tree (like a flattened device tree, "FDT"). Give games a way to mark files as assets so they load from a virtual
file system. Possibly list assets in a scene's JSON, and move assets that several scenes share into `assets.dat`. When a scene loads,
look in the scene's own data first, then in `assets.dat`. The reasoning: the build cannot know every path the TypeScript needs, but
the engine already loads assets by path at run time, so it can look them up in a virtual file system the same way.

## 1. The problem, measured

### 1.1 How paths reach the engine today

Every load takes a **game path** (`assets/x.png`, relative, checked by `platypus_core::resolve_asset`), and the app reads
`<asset root>/<path>` with `std::fs`. There are about 15 read sites (pack design 4.1). The ones that matter here are:
`load_sound` (`crates/platypus_app/src/main.rs:2854`; compiled games reach it through `native_pump.rs:22`), UI images and SVG
(`crates/platypus_render/src/ui/images.rs:172,232`), textures (`world_view.rs:742`), models and glTF URIs (`models.rs:84,113`), fonts and
runtime stylesheets (`main.rs:2838,2971`), `asset.readText` (`main.rs:1325`), cursors (`cursor.rs:16`), and scenes and prefabs
(`native.rs:268-297`).

| How a path is written | Can `platypus-build` see it? | What happens today |
|---|---|---|
| A string literal that names a whole existing file: `audio.load("assets/hit.wav")`, `<Image src="assets/a.svg">`, `"scenes/l1.scene.json"` | yes. `referenced_assets` (`tools/platypus_build/src/lib.rs:338`) scans every `"`, `'` and backtick literal of the bundled source. It keeps a literal whose extension is an asset kind (`assets.rs:9`) or `.scene.json`, and that is an existing file | copied with its `.meta` into `dist/`, and listed in `game.pbundle` `assets` |
| A template or concatenation: `` `assets/sfx/${n}.wav` ``, `"assets/" + name + ".png"` | **no**. Literals containing `{` or `$` are skipped (`lib.rs:359`), and `"assets/"` alone is not a file | not in `dist/`, not in the bundle's `assets` list |
| A literal held in data: `const P = [{ file: "assets/bunny.glb" }]`, then `Model.load(p.file)` (classics, materials) | yes, because the literal itself is in the source | as row 1 |
| `url()` in an imported `.css` file | yes, through `css.rs`: up to 32 KiB becomes a `data:` URL, larger files are copied to `dist/name-<hash>.ext` | inlined or copied |
| `url()` inside `addStyleSheet(...)` text at run time | no | none of the examples does this (checked: clock, fishtank, world and emoji's runtime sheets have no `url(`) |
| Assets named inside scene JSON (`assets` table `src`/`guid`, `{"$type":"Sound","src":..}`, the `"model"` extra) | not by the build today (the pack plan's step 2 adds `scene_refs`) | not copied to `dist/`, read from the game folder |
| Textures and buffers a `.gltf` names by URI | no (the pack design packs them inside the cooked model) | read next to the model (`models.rs:74`). Every example ships `.glb` |

**The app does not read `dist/` for assets.** `resolve_bundle` (`main.rs:457`) takes the bundle from `<game>/dist/` and uses the
**game folder** as both the code root and the asset root (`main.rs:204`). The `dist/assets/...` copies feed one thing only: the bundle's
`assets` list, which `platypus-build package` uses for the "essentials" it embeds. Emoji works in the dev loop because the app reads
`examples/emoji/assets/` directly. The "assets are found by looking in `dist/`" model in the request is how the build's *view* works,
not the runtime's. That view is the part that is blind to templates.

### 1.2 What a packaged exe does with them

`platypus-build package` (`tools/platypus_build/src/package.rs:271 collect_files`) embeds only the code (`game.pbundle`, `game.css`,
`project.json`, the icon). It also embeds the bundle-named assets if together they come to at most 256 KiB. It copies the **data
files** to `data/` beside the exe:

- every asset the bundle names, plus `startScene`;
- **everything under `include`**: a list of files or folders, **default `["assets"]`, the whole folder**. It was added because of
  exactly this blindness;
- the other files of `dist/` (CSS copies) as `data/dist/...`.

At run time `asset_root::locate_data` makes `data/` the asset root (`--data`, then `$PLATYPUS_DATA`, then `<exe dir>/<dataDir>`). There
are no packs and no virtual file system: just a second real folder.

### 1.3 Which examples break

Five sites in emoji build asset paths at run time. No other example does (checked with a grep over `examples/*/src` for template
literals and concatenations ending in an asset extension, and for `Texture.load`, `Model.load`, `audio.load`, `loadFont`,
`loadScene*`, `<Image src>` with non-literal arguments):

| Site | Path | Files it can reach |
|---|---|---|
| `examples/emoji/src/sfx.ts:60` | `` `assets/sfx/${n}.${extension(n)}` ``, where `n` ranges over `NAMES` (79 names) | 78 `.wav` + 1 `.mp3`, 5.9 MB |
| `src/hud.tsx:178`, `:245`, `:351`, `src/sprites.tsx:63` | `` `assets/emoji/${code}.svg` `` (codes from the `ROWS` table in `cast.ts`) | up to 77 SVGs, 0.87 MB |

What each layout gets for emoji:

| Layout | Sounds | SVGs |
|---|---|---|
| Dev loop (game folder) | 79 / 79 | 77 / 77 |
| `dist/` (what the build "knows") | **0 / 79** | **7 / 77** (the seven whole literals: `1f319`, `1f345`, `1f388`, `1f680-body/-flame/-stars`, `2764`) |
| `platypus-build package` today, default `include` | 79 / 79 (whole `assets/` copied) | 77 / 77 |
| `platypus-build package` with a narrowed `include` (e.g. `["assets/emoji"]`) | 0 / 79 | 77 / 77 |
| `include: ["assets/sfx/*.wav"]` (a glob) today | **0**. `include` takes no globs: a non-folder entry is treated as one file, `is_file()` fails, and it is **dropped silently** (`package.rs:298-319`) | as narrowed |
| The pack plan as written (step 2: "assets nothing references are left out") | **0 / 79** | **7 / 77** |

So by reading the code (no build or run was done for this note), today's default `package` ships emoji complete, as a loose
`data/assets/` folder. Emoji loses its sounds and art in three cases: something treats `dist/` as the asset set; `include` is narrowed
or written as a glob; or the planned pack build drops unreferenced files. The last one is the real danger, because the pack plan
replaces the loose `data/` folder. **The first deliverable below is a packaged emoji screenshot run** (test `M9-TS-2553`), so the
current state is measured rather than inferred.

## 2. Design

### 2.1 One path namespace

- A key is a **game path**: relative, `/`-separated, no `..`, no scheme or drive. `platypus_pack::normalize_path` turns `./assets\a//b.png`
  into `assets/a/b.png`. The same key works in the dev folder, in packs and in scripts; `guid:<32 hex>` and `data:` URLs stay as they
  are (`Vfs::read`).
- Most keys are `assets/...`, but any path under the game folder is allowed (`scenes/`, `levels/`). Packs keep the full path: no prefix
  is stripped, so a key never changes between dev and release.
- **Case-sensitive**, as in packs, Linux and Android. Windows folders are not, so `audio.load("assets/SFX/Boom.wav")` works in the dev
  loop and fails in a pack. Fix: in the dev loop `DirSource` compares the key with the real file names (one `read_dir` per folder,
  cached) and warns once per path: `asset "assets/SFX/Boom.wav": the file on disk is "assets/sfx/boom.wav" (packs are case-sensitive)`.
  The build refuses two included paths that differ only in case.
- `.meta` files are never keys. Their `guid` and `importer` go into the entry (`Entry.guid`, `meta` text), as the pack design says.

### 2.2 `assets.dat` is a `.ppk`; the "FDT" is its table of contents

The flat table the request asks for already exists. The `.ppk` table of contents is a flat array of entries with **no directory tree**,
plus sorted index arrays:

| An FDT-style entry wants | Where `.ppk` v1 has it |
|---|---|
| path hash | `PathSlot.path_hash` (FNV-1a 64, 16-byte slots sorted by `(hash, cooked, path)`, binary search) |
| path string offset | `Entry.path_offset u32` + `path_len u16` into one string table (each distinct text once) |
| offset, stored size, raw size, codec | per **chunk**: `Chunk { offset u64, stored_size u32, raw_size u32, codec u8, crc u32 }`; the entry has `first_chunk`, `chunk_count`, raw `size u64` |
| GUID | `Entry.guid [16]` (entries sorted by GUID for `guid:` lookups) |
| kind, flags | `kind u8` (texture, audio, svg, ...), flags (tombstone, contiguous, cooked) |
| (extra) | `content_hash [16]`, importer `meta` text, CRC-32 per chunk, TOC and header CRCs, `pack_id` |

**Recommendation: reuse `platypus_pack`. Do not add a second format.** Call the file `assets.dat` if that name is preferred: the extension
is cosmetic, the magic is `IRPK`. Honest differences from a minimal FDT:

- *One level of indirection* (entry, then chunks), 24 bytes per 256 KiB chunk. It buys range reads inside compressed entries (streamed
  audio, texture mips later), per-chunk codecs (the 1/16 rule keeps mp3, ogg and png raw), parallel decompression, and the GPU-codec
  path (`platypus-directstorage-and-codecs.md`: 256 KiB is four GDeflate tiles). A stored entry is flagged *contiguous* and is one byte
  range, which is exactly the FDT case, and is served zero-copy from the map.
- *Two sorted indexes* (GUID and path hash) instead of one: 16 more bytes per entry, so lookups by GUID survive renames.
- Measured cost (pack design 7): 142 bytes per entry with paths; 100k entries is a 14 MB TOC that opens in 3.7 ms mapped; `find_path`
  takes 0.8 µs. A minimal FDT would save about 40 of the 142 bytes and none of the time that matters.

### 2.3 Files of a packaged game

```text
MyGame.exe                 code only (payload: game.pbundle, game.css, project.json, icon)
data/
  index.json               manifest (pack design 3.1): packs in mount order, ids, scene -> pack
  assets.dat               role "global": what code, CSS, include globs and dynamic patterns name, plus what 2+ scenes share
  scene-level1.dat         role "scene": scenes/level1.scene.json (first entry) + assets only that scene uses
  scene-level2.dat
  patch-0001.dat           (later) changed entries and tombstones
dlc/<id>/index.json        (later)        mods/<name>/   (later, if the game allows mods)
```

Changes from the pack design: **one** `assets.dat` replaces `global.ppk` and `shared.ppk`. Both were always mounted together, so
splitting them bought nothing at run time (this is decision 7, "one shared pack", taken). Packs are named `.dat`; `index.json` is
kept. A scene's "own data" is its pack, and the scene file is that pack's first entry.

**Do not embed asset bytes in the scene's JSON.** Base64 adds 33%, cannot be mapped or streamed, puts megabytes through
`serde_json`, and would copy shared assets into every scene. The scene JSON keeps *references* (its `assets` table, with GUIDs added
when cooked, pack design 5). The pack holds the bytes.

### 2.4 Lookup order

The `Vfs` is one stack, searched from the top; the first answer wins and a tombstone answers "deleted". Release, from bottom to top:

```text
mods/*        (top, only when the game allows mods)
dlc/*
patch-*.dat
scene-*.dat   (every scene pack, by scene path)
assets.dat    (bottom of the base)
[game folder or loose data/: dev loop or --loose only, below everything, warning on each hit]
```

This gives the requested order: scene data, then `assets.dat`, then loose files. Two honest notes:

1. **The order of the base layers never changes an answer.** The build puts each path in exactly one base pack (2.5), so "the current
   scene's pack first" is the same lookup as "any pack". What matters is that **patches, DLC and mods sit above all base packs**. A
   "current scene first, then `assets.dat`" rule that skipped the stack would let a scene pack shadow a patch. So the scene order is
   only an ordering inside the base, not a special path.
2. **Every pack is mounted at start**, not when its scene loads (opening is about 0.1 ms + 37 ns per entry; a map costs address space,
   not memory). A script can load any asset at any time, including a dynamic path that only scene B's pack holds while scene A runs.
   Loading a scene means *prefetching* its pack (pack plan step 9), not making its assets visible. Mounting on scene load would bring
   back the bug this note is about, as an ordering bug.

A merged index (`Vfs::freeze`, pack plan step 10) makes a lookup one hash probe however many packs there are.

Dev loop: `Vfs::dir(game folder)` only. No packing; the `AssetWatcher` and `.platypus/cooked` work as today. `platypus <game> --packs` mounts
`<game>/dist/data/` instead, with the folder **below** it only under `--packs --fallback`, logging
`asset "x" came from the game folder: it is not packed`. Shipping builds never have the folder layer.

### 2.5 What goes into which pack (reference counting)

Inputs (all sorted sets, deterministic):

- `code`: whole literals (today's `referenced_assets`) and CSS copies and fonts;
- `patterns`: dynamic-path globs inferred from the code (2.6 c);
- `included`: files matching `include` globs minus `exclude` (2.6 a and d);
- `scenes[s]`: for each scene root `s`, the assets its JSON names, with prefabs expanded (`scene_refs`, pack plan step 2), plus an
  optional `"preload"` list of globs in the scene file (2.6 e).

```text
for each asset a in code ∪ match(patterns) ∪ included ∪ ⋃ scenes[s]:
    users(a) = { s : a ∈ scenes[s] }
    if a ∈ code ∪ match(patterns) ∪ included   -> assets.dat   (code may load it at any time)
    else if |users(a)| == 1                     -> scene-<slug(s)>.dat
    else                                         -> assets.dat   (shared by 2+ scenes)
```

A scene file goes into its own pack, even when code also names it (code naming a scene makes it a scene root, not an asset). Forms
(cooked or source), compression flags, GUIDs and entry order follow pack plan step 3 unchanged. `--report` prints
`assets.dat: N entries (code n1, patterns n2, include n3, shared n4)` per pack, so every byte's reason is visible.

### 2.6 Marking files as assets

| Option | What it is | Catches emoji? | Cost | Verdict |
|---|---|---|---|---|
| (a) `project.json` `include` / `exclude` globs | `"include": ["assets/**", "levels/*.json"]`, `"exclude": ["**/*.psd", "assets/wip/**"]`. Extends the existing `include` field: a plain folder still means `folder/**`. `*` stays inside one path part, `**` crosses parts. About 20 lines, no crate | yes | over-includes when written loosely | **escape hatch** |
| (b) `.assets` manifest, `// @asset assets/sfx/*.wav` comments, or a compile-time `Assets.include("...")` call | a second syntax to learn and keep in sync; a call that does nothing at run time misleads readers | yes, if written | low | not recommended: (a) says the same in the file that already lists packaging rules |
| (c) Pattern inference from the code | In `platypus_ts` (it has an oxc AST and `oxc_ast_visit`), visit template literals and `+` chains whose parts include string literals. Each hole becomes `*` within one path part: `` `assets/sfx/${n}.${e}` `` gives `assets/sfx/*.*`, and `` `assets/emoji/${c}.svg` `` gives `assets/emoji/*.svg`. Keep a pattern only if it has a literal folder prefix and matches at least one file of an asset kind; otherwise warn `src/x.ts:12: dynamic asset path with no folder prefix; add an "include" glob`. **Full constant evaluation** (proving `n` ranges over `NAMES`) is feasible only in narrow cases: a top-level `const` array of literals iterated directly by `for..of` into the hole. Emoji's `extension(n)` (a call) and `s.code` (fields of objects pushed in a loop over `ROWS`) defeat it, so it would cover about 1 of the 5 emoji sites, against 5 of 5 for globbing | yes, all 5 sites | S to M; over-includes within one folder (fine for emoji: every match is used) | **on by default as a safety net**: adds its matches and reports them |
| (d) Pack everything under `assets/` | the `include` default becomes `["assets/**"]` (what `package` already does with `["assets"]`) | yes | ships unused files (reported) | **default** |
| (e) A scene's `"preload": ["assets/sfx/boss_*.wav"]` | files a scene's scripts load dynamically, placed by 2.5 (one scene: its pack) | n/a (emoji has no scenes) | S | optional, for locality only; never needed for correctness |

**Recommendation.** Default **(d)**: everything under `include` (default `["assets/**"]`) minus `exclude` is packed. When a game
author has narrowed `include`, **(c)** pattern inference still adds the families the code builds, and lists them. The escape hatch is
**(a)**: explicit `include` and `exclude` globs, for files outside `assets/`, for trimming, or when inference is too broad. A
`.meta` `"importer": { "pack": false }` excludes one file. This reverses pack design decision 5 ("leave unreferenced out"): the
request says the build cannot be trusted to know, and a missing sound in a shipped game costs more than a few spare megabytes. Open
question 1 asks for confirmation.

A dev-loop check catches the remaining gap before shipping: `platypus-build build` writes `dist/assets.plan` (the sorted keys the packs
would hold). While it exists, the dev app logs once per path:
`asset "assets/x.wav" loaded, but a packaged build would not include it (add it to "include" in project.json)`.

### 2.7 The emoji fix, precisely

- **Today (no code change):** keep `include` unset, or write `"include": ["assets"]` (folders only). **Do not write globs** in
  `include` until phase 0 lands: they are silently dropped (1.3). The current `package` then ships all 79 sounds and 77 SVGs in
  `data/assets/`. Confirm it with the `M9-TS-2553` run.
- **After phase 0:** `examples/emoji/project.json` becomes

  ```json
  {
    "name": "emoji",
    "title": "Emoji parade",
    "include": ["assets/emoji/*.svg", "assets/sfx/*.wav", "assets/sfx/*.mp3", "assets/CREDITS.md"]
  }
  ```

  The globs are explicit and checked (each must match at least one file, or the build warns). `CREDITS.md` ships the attribution for
  Noto Emoji and the sounds. Even with no `include`, inference reports `src/sfx.ts:60: assets/sfx/*.* (79 files)` and
  `src/hud.tsx:178 ... assets/emoji/*.svg (77 files)`.
- **After phase 2:** the same globs place all 156 files in `data/assets.dat`, because emoji has no scenes. SVG, WAV and MP3 are
  source-form kinds, so **no cooker is involved**, and emoji is the ideal first pack customer.
- Optional code change, for clarity: replace `NAMES` with `Assets.list("assets/sfx/*")` (phase 3), so the folder is the single source
  of truth.

## 3. Script API

Unchanged, and resolved through the `Vfs` with no script change: `Texture.load`, `Model.load`, `audio.load(path, stream)`, `loadFont`,
`<Image src>`, CSS `url()`, `Cursor.setCursor`, `SceneManager.loadScene` / `loadSceneAsync`, prefabs, `guid:` sources,
`Assets.guidToPath` / `pathToGuid` / `importer` / `count` / `pathAt` (the release asset database comes from `Vfs::entries()`, pack
plan step 7).

New, in `platypus_engine_api::assets` (namespace `Assets`, already `#[ts_api(namespace = "Assets", quickjs)]`, so QuickJS games get
the same functions with no extra work). They answer from the asset database the host fills (`set_assets`), so `platypus_engine_api`
gains no dependency on `platypus_pack`:

```ts
Assets.exists(path: string): boolean          // in a pack or the dev folder (a data: URL is true)
Assets.list(glob: string): string[]           // sorted keys matching a glob (same matcher as project.json)
Assets.packOf(path: string): string           // "assets", "scene-level1", "" in the dev loop
```

`Assets.preload` / `preloadScene` / `isDone` / `progress` / `release` stay in pack plan step 10 (they need the async loader).
`Assets.list` needs every asset to be in the database. Today `asset_db::scan` lists only files with a `.meta`, so phase 3 lists every
`DirSource` path, with GUID `""` when there is no `.meta`.

**Errors.** A failed load names the key and each place that was searched, and gives the fix. It is logged once per path:

- release: `asset "assets/sfx/boom.wav" not found in data/scene-level1.dat, data/assets.dat (build 6f1c..): it was not packed. Add it
  to "include" in project.json.`
- dev: `asset "assets/sfx/boom.wav" not found in the game folder C:\...\emoji`, plus the case hint of 2.1 when a file differs only in
  case.
- `audio.load` keeps returning a handle; the failure stays the existing logged error with the same text.

**Hot reload.** The dev loop is unchanged (`DirSource`, `AssetWatcher`, `Assets.onChanged`). Packs are immutable: under `--packs`,
watching `data/index.json` and remounting is optional. `platypus-build dev` never packs. A rebuild refreshes `dist/assets.plan` (2.6).

## 4. Implementation plan

Test IDs: `M9-API-1140..1149` (Rust, app), `M9-TS-2550..2559` (`platypus_build`, `platypus_ts`). Both ranges sit inside the pack plan's
blocks and are unused. One commit per phase step, all `CLAUDE.md` checks green. No new crates.

### Phase 0: make dynamic paths visible, emoji safe now (S)

- `platypus_core::project`: `include` accepts globs (`*`, `**`); new `exclude` (globs). The default include is `["assets/**"]`, and a plain
  folder entry means `folder/**`. `glob.rs` in `platypus_core` holds the 20-line matcher, used by `platypus_build` now and by `Assets.list`
  later.
- `package.rs collect_files`: expand globs. An entry that matches nothing is a **warning** (`include "assets/sfx/*.wv" matches no
  file`), never a silent drop.
- `platypus_ts::asset_patterns(program) -> Vec<(glob, file:line)>` (oxc visitor, 2.6 c). `platypus_build::build` records the patterns in
  `game.pbundle` as `"patterns"`, with their matches; prints `note:` lines; and `package` copies the matches too.
- Tests: `M9-TS-2550` the glob matcher (`*` in one part, `**` across parts, case-sensitive, `folder` = `folder/**`); `M9-TS-2551`
  pattern inference: a template, a `+` chain, nested templates in holes, refusing prefix-less patterns, emoji's five sites giving
  `assets/sfx/*.*` and `assets/emoji/*.svg`; `M9-TS-2552` `package` with a narrowed `include` still copies the pattern matches; an
  unmatched include warns; `exclude` removes files; `M9-TS-2553` (manual or `#[ignore]`, needs a built host and a GPU)
  `platypus-build package examples/emoji`, run from another folder with `--screenshot --frames 120` and `PLATYPUS_LOG_FILE` set: the log has no
  `not found`, and the screenshot shows the SVG emoji.
- This fixes emoji for narrowed or glob `include`s and proves the default case. Size: about 300 lines plus tests.

### Phase 1: the pack build with `assets.dat` (M; amends pack plan steps 1 to 4)

- Pack plan step 1 (manifest) as written, with pack names `assets` and `scene-<slug>` and files `*.dat`.
- Step 2 (graph): add `patterns` and `included` (phase 0) and the scene `"preload"` key; drop `platypus.pack.include` and `resources/**`
  (they are now `include` globs: `resources/**` can be listed there).
- Step 3 (plan): the placement rule of 2.5 (one `assets` pack replaces global and shared). Add `--source-only`: every entry in source
  form. This needs no cooker and is enough for emoji (and for SVG, audio and font-only games) until pack plan step 15 moves the cooker.
- Step 4 (write): writes `dist/data/assets.dat`, `scene-*.dat` and `index.json`, plus `dist/assets.plan`. `--report` shows the per-reason
  counts.
- Tests: `M9-TS-2554` placement: an asset in one scene goes to its pack; in two scenes, in code, or only `include`d goes to
  `assets.dat`; a scene file always goes to its own pack; `M9-TS-2555` golden pack contents: packing `examples/emoji` with
  `--source-only` gives one `assets.dat` whose sorted keys equal the matched files (156 assets + `CREDITS.md`, no `.meta`), with GUIDs
  from the `.meta` files; `M9-TS-2556` two runs are byte-identical; two keys that differ only in case are an error.

### Phase 2: the runtime reads through the `Vfs` (M; pack plan steps 6, 7 and 14, source forms first)

- Step 6 as written: about 15 sites change from `resolve_asset + fs::read` to `vfs.read` (`load_sound`, `images.rs` via
  `set_assets(Vfs)`, `texture_op`, `models::load` / `read_uri`, fonts, stylesheets, `asset.readText`, cursors, scene and prefab reads in
  `native.rs`). `AssetRoot` becomes the `Vfs` (that file already calls itself "the seam where asset packs can mount").
- Step 7, subset: `data.rs` mounts `data/index.json` (mapped on 64-bit) when the exe carries a payload and `data/index.json` exists,
  and otherwise falls back to today's loose `data/` folder as a `DirSource` (old packages keep working). Cooked forms from packs wait
  for the rest of step 7. Add the errors of section 3, the `DirSource` case check, and the dev `assets.plan` check.
- Step 14, subset: `platypus-build package --packs` = phase 1 pack, then copy `index.json` and the listed `.dat` files to `<out>/data/`, with
  no loose assets. The default stays loose until the user decides (open question 8).
- Tests: `M9-API-1140` lookup order: a key in a scene pack and in `assets.dat` comes from the scene pack; a patch above both wins; a
  tombstone hides both; the folder layer answers only under `--fallback`; `M9-API-1141` the not-found message names every pack that was
  searched and the `include` hint; `M9-API-1142` the dev case check warns once for `assets/SFX/a.wav` against `assets/sfx/a.wav`;
  `M9-API-1143` the dev `assets.plan` check warns for a loaded key that is not in the plan, and not for one that is; `M9-API-1144`
  `load_sound` and a UI SVG load from a pack-only `Vfs` (`OfflineBackend`, no GPU); `M9-API-1145` (manual) packaged emoji with
  `--packs`, same run as `M9-TS-2553`: no `not found`, and the screenshot equals the loose run.

### Phase 3: `Assets.exists` / `list` / `packOf` (S)

- `platypus_engine_api::assets` adds the three functions. The host fills the database from `Vfs::entries()` (release) or a full
  `DirSource` walk (dev; entries without a `.meta` get guid `""`). Update `tools/platypus_ts/src/lower/engine_ts.d.ts` only if the shapes
  need it (the manifests give `Assets.*` already).
- Tests: `M9-API-1146` `exists` and `list` against a fixed database (sorted output, glob rules shared with `M9-TS-2550`, `guid:` and
  `data:` cases); `M9-TS-2557` a compiled script calls `Assets.list("assets/sfx/*.wav")` and gets the sorted keys (host simulated as in
  the `M9-API-806`-style tests).

### Then: the rest of the pack plan, unchanged

Cooked forms from packs (step 7 rest), streamed audio from the map (8), scene prefetch (9), async loads and the merged index (10),
refcounts (11), mip streaming (12), patches, DLC and mods (13), `platypus_cook` (15), mobile (16), codecs (17).

### Risks

- **Over-inclusion.** `assets/**` ships work files. Mitigations: `exclude`, `.meta` `pack: false`, and `--report` listing unreferenced
  files and their total. A large game should narrow `include`, and inference and the `assets.plan` check keep that safe.
- **Inference false positives and negatives.** A pattern like `assets/*/x.png` cannot be told apart from a dynamic folder. Holes match
  inside one path part only, prefix-less patterns only warn, and the dev check catches what is left.
- **Hash collisions.** FNV-1a 64 path hashes are compared by string on a match, so a collision costs a string compare, never a wrong
  file. GUIDs come from `.meta` files (random 128-bit) or a hash of the path. The build already refuses duplicate GUIDs within a pack.
- **Case on Windows** (2.1): dev-only success, release failure. Mitigated by the warning and the build check, not by a case-insensitive
  pack index (that would hide the bug on Linux and Android).
- **Memory.** Maps cost address space. 32-bit targets use `OpenMode::Read` (positional reads). A single `assets.dat` of many GB is fine
  mapped on 64-bit. A truncated mapped file faults the process; packs are read-only release files.
- **Overlay precedence.** Patches, DLC and mods must stay above every base pack (2.4); a "scene first" shortcut must not bypass them.
  Tested in `M9-API-1140`.
- **Old packages.** An exe with a loose `data/` and no `index.json` keeps working through the `DirSource` fallback (phase 2).

### The first small slice

Phase 0 is about 300 lines, needs no format, runtime or cooker work, and can land before anything else:

1. glob `include` / `exclude` with warnings;
2. pattern inference;
3. the packaged emoji screenshot run (`M9-TS-2553`).

After it, emoji ships complete under any `include`, and the build tells the author which dynamic families it found. Phase 1 with
`--source-only`, plus phase 2, then moves emoji into `data/assets.dat` with no cooker.

## 5. Open questions for the user

1. **Pack everything under `include` (default `assets/**`), unreferenced or not?** Recommended yes. It reverses pack design decision 5
   ("leave unreferenced out"), and it is what `package` does today.
2. **Naming and split:** one `data/assets.dat` (global and shared merged) plus `scene-<slug>.dat`, with `IRPK` inside and `.dat` names.
   Recommended. Or keep `.ppk` names, or separate global and shared packs (finer downloads, more files)?
3. **Codec** (pack decision 1): LZ4 default. Is the `zstd` crate OK as a second codec, and is the `CODEC-1` prototype OK (DirectStorage,
   `gdeflate` and `zstd` downloads, a headless D3D12 probe on this machine)?
4. **DLC and mods** (pack decisions 3 and 4): may DLC override base assets (recommended yes, like patches)? Mods from `mods/`, mounted
   last, opt-in per game, or never?
5. **Textures named in code, including ones found by inference** (pack decision 6): store both cooked and source forms (safe, larger), or
   cooked only? It matters more now that pattern matches add whole folders.
6. **Textures whose sizes are not multiples of 4** (pack decision 8): pad them to BC (smaller packs, a UV change), or keep the RGBA8
   fallback (measured at 39% of cooked bytes in the test corpus)?
7. **Case:** warn in the dev loop on a case mismatch (recommended), or make dev lookups case-sensitive too (stricter, and it may break
   existing games on Windows)?
8. **Packaged default:** when packs land, should `platypus-build package` write packs by default, with loose `data/` behind `--loose` for
   debugging and modding, or stay loose until `--packs` is asked for?

## Unsafe paths

The VFS lookup is the one choke point: every asset, font, sound, scene, CSS `url()` and future file-IO read goes through `platypus_core::resolve_asset` (today it returns `None` for anything outside the game folder). The unsafe-path policy lives there, in the core engine, so it applies to QuickJS and compiled games alike (no per-tier code).

### Modes
- **Safe (default).** A path outside the game / data / virtual-FS roots (absolute, `..` escape, other drive, UNC, URL scheme, `file://`) is refused. **Decided:** a console / `Player.log` warning only (`asset not found: <requested> (resolved: <canonical path>, outside the game folders)`, once per distinct path); no popup and no in-app notice.
- **Unsafe** (`--unsafe-paths`). Each unsafe access asks the user first (below). Without an answer the result is a denial.

### Where the flag lives, who may set it
- `platypus-build --unsafe-paths` / `platypus-build package --unsafe-paths` writes `"unsafePaths": true` into the payload's project metadata; `platypus --unsafe-paths` sets it at run time. Either enables the mode, nothing in game code can (no op, no `project.json` read at run time from a writable folder for packaged games: the value comes from the signed/packed payload or the command line).
- A downloaded game must not enable it silently: the flag only enables the prompt, it never grants. The popup appears even when the flag is baked in.

### The popup (decided: three choices, default Cancel)
- Engine-drawn modal is not possible from a loader thread and not in headless runs, so use a **native OS dialog** (rfd-style message box, always on top, title names the game) for the file prompt. Buttons: **Cancel** (default, Enter / Esc / close = deny), **Allow once**, **Allow for this session**. Shows the canonical path, the requesting kind (image, font, scene, ...) and the game name.
- Asset loads on worker threads send a request to the main thread over a channel and block on the answer with a timeout (30 s = deny); the main thread shows at most one dialog at a time and the rest queue. Screenshot / headless / `--frames` runs auto-deny and log `denied (no UI)`.
- Compiled-code file IO (M6) calls the same function and gets a `Denied` result (QuickJS: a rejected promise / thrown error) with the same message.

### Caching and flood protection
- Per exact canonical path: *once* (not cached), *session* (cached in memory only, never persisted), deny (cached for the session, so a denied path does not re-prompt every frame).
- **Decided:** grants are per exact canonical path. Folder-level grants are not part of this; recommendation: do not add them by default. If ever added as an optional later extra, they must be an explicit extra button limited to the request's parent folder, never a drive root or the profile root.
- Flood: after N (default 5) prompts in 10 s, further distinct paths are auto-denied with one summary dialog ("the game requested many files; denied until you allow a folder"). Without folder grants, a game that touches thousands of files is simply denied after the flood limit (it should use the game folder or the VFS).

### Canonicalisation hazards
Decide on the **canonical, final** path, after everything is resolved, never on the text the game gave:
- Symlinks / junctions / reparse points: `canonicalize` (resolves them) and compare to the canonical roots; a link inside the game folder pointing out is unsafe.
- Windows: strip `\\?\` and `\\?\UNC\` after canonicalising, treat UNC / network paths and device paths (`\\.\`, `CON`, `NUL`, alternate data streams `file:stream`) as unsafe always (prompt, with a stronger wording; never folder-granted).
- Case-insensitivity: compare with case folding on Windows / macOS; 8.3 short names are expanded by canonicalisation first (`PROGRA~1`).
- `..` after normalisation: normalise lexically, then canonicalise the real path; check containment by path components, not by string prefix (`/game2` is not under `/game`).
- URL schemes and `file://`: only `file:` with a local path is converted to a path and then goes through the same check; `http(s):` etc. are not paths (fetch is a separate policy, M6).
- TOCTOU: open the canonical path once and read from that handle.

### Logging
Every refusal and every decision (path, kind, decision, mode, source of the flag) goes to `Player.log`; granted session paths are listed in the log at exit.

### Tests (proposed IDs `M6-VFS-UNSAFE-01..`)
01 safe mode refuses absolute, `..`, drive, UNC, `file://`; 02 warning text contains the resolved path; 03 symlink / junction out of the folder is unsafe; 04 `\\?\` and 8.3 and case variants canonicalise to the same decision; 05 prefix sibling (`game2`) is outside; 06 unsafe mode with a stub prompter: Cancel/Esc/close deny, Once is not cached, Session is; 07 flood limit; 08 headless auto-deny; 09 baked flag still prompts; 10 QuickJS and compiled tiers get the same result for one path list; 11 `platypus-build` writes the metadata, runtime flag overrides nothing from game code.

### Phases and sizes
1. Policy in `resolve_asset` (safe mode, warning, canonical checks, tests 01-05): S.
2. Flag plumbing in `platypus-build` / `platypus-build package` / `platypus_app`, project metadata (09, 11): S.
3. Prompter trait + native dialog + worker-thread channel + caching/flood (06-08): M.
4. In-app notice and docs: S.

### Open decisions for the user
- Is the native OS dialog acceptable, or must it be an engine-drawn modal (then loads must wait for the frame loop)?
- The flood limit values (N prompts in T seconds) and the dialog timeout.
- Should "Allow for this session" ever be persisted (per game, in prefs)? Proposed: no.
- Should UNC / network paths be refusable-only (never promptable)?

## 6. Decisions (user, 2026-10-22)

1. Pack everything under `include` (default `assets/**`), referenced or not. This reverses pack design decision 5 ("leave unreferenced out").
2. One `data/assets.dat` (global and shared merged) plus `scene-<slug>.dat`, with `IRPK` inside and `.dat` names.
3. Codec: LZ4 default; the `zstd` crate is approved as the second codec. DirectStorage and the `CODEC-1` prototype come later.
4. DLC may override base assets (like patches). Mods are mounted last, opt-in per game.
5. Textures named in code: store both cooked and source forms for now. **Note to revisit:** consider cooked only to reduce bloat.
6. Textures whose sizes are not multiples of 4: keep the RGBA8 fallback for now.
7. Lookups are case sensitive following the OS / platform defaults (the host file system's own rules for loose files). Pack keys are exact-case. Keeping a dev-loop warning for a case mismatch is still recommended, so a game that works on Windows does not fail on a case-sensitive platform.
8. `package` stays loose (`data/`) until packs are tried and tested, then the default flips. **TODO:** track "flip `platypus-build package` to write packs by default" (and a `--loose` switch for debugging and modding) in the TODO once packs have been exercised on real games.
9. Unsafe paths (section added by the plan agent if present): safe mode warns on the console only; unsafe mode asks per path with Allow once / Allow for this session / Cancel, default Cancel.
