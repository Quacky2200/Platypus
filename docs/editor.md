# The editor

The editor is a game like any other: `editor/` is built with `platypus-build build editor` and run by the one host,
`platypus-app editor --project <folder>`. Its UI is Platypus's own React UI, compiled natively by web_ui_ts. There is
no second binary, no QuickJS bundle and no separate component package: the components (menubar, docking, virtual
list, tree view, console, project browser, Scene view) are modules of `editor/src`, so a game can copy them in.

| Path | Role |
|---|---|
| [editor/src/main.tsx](../editor/src/main.tsx) | The editor's UI: menubar, dock, status bar, demo scene, Hierarchy, Game, Project |
| [menu.ts](../editor/src/menu.ts), [MenuBar.tsx](../editor/src/MenuBar.tsx) | Menubar model and component |
| [dock.ts](../editor/src/dock.ts), [Dock.tsx](../editor/src/Dock.tsx) | Dock layout model and component |
| [virtual.ts](../editor/src/virtual.ts), [VirtualList.tsx](../editor/src/VirtualList.tsx) | Virtual list |
| [tree.ts](../editor/src/tree.ts), [TreeView.tsx](../editor/src/TreeView.tsx) | Tree view |
| [console.ts](../editor/src/console.ts), [log.ts](../editor/src/log.ts), [Console.tsx](../editor/src/Console.tsx) | Console model, the log feed, the panel |
| [project.ts](../editor/src/project.ts), [ProjectBrowser.tsx](../editor/src/ProjectBrowser.tsx) | Project file model (promises over `Editor`) and the browser |
| [modal.ts](../editor/src/modal.ts), [Modal.tsx](../editor/src/Modal.tsx) | The stack of open dialogs, `ModalHost`, `Modal`, `confirmDialog`, `textDialog` |
| [file-dialog.ts](../editor/src/file-dialog.ts), [FileDialog.tsx](../editor/src/FileDialog.tsx) | The open / save / folder dialog: its logic (filters, names, typed paths) and the component |
| [SceneView.tsx](../editor/src/SceneView.tsx), [scene-camera.ts](../editor/src/scene-camera.ts), [handles.ts](../editor/src/handles.ts), [frame.ts](../editor/src/frame.ts) | The Scene view, its camera model, transform handles, `onEditorFrame` |
| [history.ts](../editor/src/history.ts), [scene-edit.ts](../editor/src/scene-edit.ts) | Undo and redo: the command list, and scene edits as commands |
| [editor_api.rs](../crates/platypus_scenes/src/editor_api.rs), [inspect.rs](../crates/platypus_scenes/src/inspect.rs) | The `EditorScene` namespace; the component and field logic under the Inspector |
| [placement.ts](../editor/src/placement.ts) | Placing files: `groundPoint`, `assetKind`, `placeable`, `importFolder` |
| [Inspector.tsx](../editor/src/Inspector.tsx), [inspector.ts](../editor/src/inspector.ts) | The Inspector panel and its logic (number parsing, field JSON, undo commands) |
| [InspectorInputs.tsx](../editor/src/InspectorInputs.tsx), [Pickers.tsx](../editor/src/Pickers.tsx) | The Inspector's reference and colour inputs; the picker dialogs (`pickDialog`, `colorDialog`) |
| [refs.ts](../editor/src/refs.ts) | References into deleted objects, noted before a delete and set again on undo |
| [screen.ts](../editor/src/screen.ts), [classes.ts](../editor/src/classes.ts), [demo-scene.ts](../editor/src/demo-scene.ts) | `useScreenSize`, `cx()` class lists, the demo scene |
| [editor.css](../editor/src/editor.css), [editor-ui.css](../editor/src/editor-ui.css) | Styles (coloured through `--platypus-*` custom properties) |
| [editor/test](../editor/test/check.ts) | Model tests, run natively (see Tests) |
| [editor.rs](../crates/platypus_engine_api/src/editor.rs), [editor/open.rs](../crates/platypus_engine_api/src/editor/open.rs), [editor/project.rs](../crates/platypus_engine_api/src/editor/project.rs), [editor/files.rs](../crates/platypus_engine_api/src/editor/files.rs) | The `Editor` namespace |
| [scenes.ts](../editor/src/scenes.ts) | The open scene: recent files per project (PlayerPrefs), window title, game cameras, the start scene (`startScene(fromProject, reopenLast)`) |
| [Preferences.tsx](../editor/src/Preferences.tsx) | Edit > Preferences: true/false editor settings in PlayerPrefs (`getPref`, `setPref`, the `PREFS` list) |
| [ProjectSettings.tsx](../editor/src/ProjectSettings.tsx), [project_edit.rs](../crates/platypus_core/src/project_edit.rs) | Edit > Project Settings, and the text edits of one `project.json` key (`get_key`, `set_key`, `remove_key`, the `FIELDS` table) |
| [meta.rs](../crates/platypus_core/src/meta.rs) | `.meta` helpers (GUIDs, importer settings, `ensure`), shared with `platypus-build` |
| [logging.rs](../crates/platypus_app/src/logging.rs) | The capture layer that feeds the log |

## Running it

```bash
cd tools && cargo run -p platypus_build -- build ../editor
cargo run --release -p platypus_app -- editor --project examples/arcade
```

`editor` is the game folder (the editor's own UI); `--project <folder>` is the project it works on. The editor folder
builds like any game (`project.json`, `tsconfig.json`, `src/`; `editor/build` is ignored by git), so `--screenshot`,
`--frames` and `platypus-build dev editor` work as for games.

With `--project`, the host (`main.rs`):

- turns on `Editor` for that folder (`platypus_engine_api::editor::enable`, the path made absolute; the editor's own
  folder is used for relative paths in `openFile`) and keeps the log from the start, before anything logs;
- sets `Application.isEditor`;
- makes the project folder (made absolute) the asset root, unless `--data` is given: scenes (`SceneManager.loadSceneAsync`),
  models, textures, sounds, the `.meta` asset scan and the asset watcher all resolve against the project, so the editor's
  world loads the project's files by their project paths. The editor's own UI stylesheet (`game.css`) still comes from the
  editor's folder, and the editor's `build/assets.plan` is not used;
- passes the paths dropped on the window from the system to `Editor`'s allow list (`file_drop.rs`).

Without `--project` the `Editor` calls do nothing and fail, so games can't use them.

## The `Editor` namespace

Declared with `#[ts_api]` in [editor.rs](../crates/platypus_engine_api/src/editor.rs), so its TypeScript
declarations are generated. All calls answer at once; there are no ops and no promises on the Rust side.

| Member | Does |
|---|---|
| `Editor.isAvailable` | True when the host runs the editor on a project |
| `Editor.project` | The project folder's full path ("" without one) |
| `Editor.logSince(seq)` | The kept `LogRecord`s (`seq`, `level` 0 trace to 4 error, `target`, `message`, `timeMs`) with `seq` above the argument, oldest first |
| `Editor.openFile(path, line, col)` | Opens the file at that place in the user's code editor |
| `Editor.listDir(path)` | `ProjectEntry[]` (`name`, `dir`, `size`) of a project folder ("" is the root): folders first, by name, without dot files and `.meta` files |
| `Editor.copyIn(sources, dir)` | Copies files or whole folders from outside into a project folder, renaming (`a (2).png`) instead of overwriting; returns the new project paths |
| `Editor.move(path, dir)` | Moves a project file or folder into a project folder; refuses overwriting and moving a folder into itself; returns the new path |
| `Editor.readText(path)` | A project file's text ("" when it can't be read: see `lastError`) |
| `Editor.writeText(path, text)` | Writes a project file, making its folders; the text goes to a temporary file beside it, which then replaces the old file in one step. False when it failed |
| `Editor.exists(path)` | Whether a project path exists |
| `Editor.stat(path)` | A `FileStat` (`exists`, `dir`, `size`, `modified_ms` since 1970, 0 when unknown) |
| `Editor.freeName(dir, name)` | A path in `dir` that isn't taken: `dir/name`, else `dir/name (2).ext`, ... |
| `Editor.createDir(path)` | Makes a folder (and those above it); returns its path ("" if it exists or failed) |
| `Editor.rename(path, name)` | Renames a file or folder in its folder, `.meta` included; refuses a taken or unusable name; returns the new path |
| `Editor.trash(path)` | Moves a file or folder (and its `.meta`) into the project's trash; returns the id `restore` takes |
| `Editor.restore(id)` | Puts a trashed item back where it was; returns its path. Refuses when something is there now |
| `Editor.ensureMeta(path)` | Gives an asset a `.meta` file if it has none (`platypus_core::meta::ensure`); returns its GUID ("" for a file that isn't an asset) |
| `Editor.startScene` | The project's `startScene` from `project.json` ("" for none) |
| `Editor.projectFields()` | `ProjectField[]` (`key`, `ty` `string` / `bool` / `number` / `enum` / `path`; `key` may be dotted, like `display.width`, `group`, `options` with `\|` between ("" is "left out"), `doc`, `value` as written with `${templates}` unexpanded, `set`: whether the file has the key), one per `project_edit::FIELDS` entry |
| `Editor.setProjectField(key, json)` | Sets one setting (a dotted key edits inside its section) from JSON text by changing only that key's text in `project.json`; an empty enum choice removes the key. Refuses an unknown setting, a wrong type, a bad enum value, an empty `name`, or an edit that would break the JSON; false with the reason in `lastError` |
| `Editor.removeProjectField(key)` | Removes a setting so its default applies; refuses `name` |
| `Editor.layerNames()` | The project's 32 layer names (`project.json` `layers` over the built-ins, "" for an unnamed layer), for the Inspector's layer choice |
| `Editor.lastError` | Why the last file call failed ("" when it worked) |

**The log.** `logging::init` adds a capture layer (filtered like the other layers). While the editor is enabled, each
event becomes a `LogRecord` in a ring of 5,000 (`LOG_LIMIT`); the message is the `message` field followed by the other
fields as ` name=value`. Scripts' `console.*` logs with target `script`, so the ring has engine and script lines in
order. `seq` counts up from 1, so a reader asks for what it hasn't seen.

**Files.** Paths are relative to the project and go through `editor/project.rs`: `platypus_core::resolve_asset`'s
rules (no absolute paths, drives, schemes or `..`), and through symlinks the real path must still be inside. `copyIn`'s
sources are the exception, being outside: only paths dropped on the window this session (`allow_sources`) are
allowed. A copy creates at most 10,000 files and skips symlinks. `files.rs` holds the calls above `copyIn` and `move`;
`readText` reads files up to a size limit.

An asset's `.meta` file (its GUID) goes wherever the asset goes: `move`, `rename` and `trash` take it along, and listings
hide `.meta` files. `copyIn` gives each copied asset a `.meta`, and the editor calls `ensureMeta` on a saved scene. The
helpers live in `platypus_core::meta` (moved from `platypus-build`'s `assets.rs`); `.fbx` counts as a model. `trash` moves the item into `.platypus/trash/<id>/` in the project, next to a `.origin` note with
its old path; `restore` reads the note and moves it back once. The trash folder itself can't be trashed.

**Opening files.** A relative path is looked up in the current folder, the editor's folder, then the source tree the
program was built from, taking the first where it exists. The command comes from `PLATYPUS_EDITOR_OPEN` if set: split
on spaces (double quotes group), with `{file}`, `{line}` and `{col}` filled in, e.g. `subl "{file}:{line}:{col}"`.
Otherwise it is `code -g file:line:col` when `code` is on PATH (`which` tries PATHEXT's extensions on Windows, where it
is `code.cmd`, and the process gets `CREATE_NO_WINDOW`). With neither, it logs how to set one up.

## The `EditorScene` namespace

Declared in [editor_api.rs](../crates/platypus_scenes/src/editor_api.rs) with `#[ts_api]`. Like `Editor` it works
only with `--project`: each call goes through `Editor`'s `project_op`, so a failure warns and leaves its reason in
`Editor.lastError`. Paths are project paths.

| Member | Does |
|---|---|
| `EditorScene.snapshot(object)` | An object and everything under it as JSON text (components, children, references between them inside it; references outside and assets stay live). "" for an object that can't be copied alone (a part of a model) |
| `EditorScene.restore(data, parent, index)` | Builds a snapshot again under `parent` (no object: a root of the active scene) at sibling `index`, with its local transform; returns the new object (none for bad text) |
| `EditorScene.newScene(name)` | Replaces the loaded scenes with an empty one; it has no file until saved |
| `EditorScene.openScene(path)` | Loads a project scene file in place of the loaded scenes, keeping its path |
| `EditorScene.saveScene(path)` | Writes the active scene as pretty JSON (folders made, old file replaced in one step); the scene takes the file's path and name (`scene_name_of`: without `.scene.json`) |
| `EditorScene.scenePath` / `sceneName` | The active scene's file ("" if never saved) and name |
| `EditorScene.components(object)` | `ComponentInfo[]` (`kind`, `handle`, `opaque`): the registered kinds' components in registration order, then those kept from a file whose kind isn't registered (`opaque`, a game's scripts) |
| `EditorScene.fields(object, kind, handle)` | `FieldInfo[]` of one component: `name`, `ty` (`number`, `bool`, `string`, `enum`, `color`, `vector2/3/4`, `object`, `component`, `asset`, `json`), `json` (the saved value), the typed value (`num`, `flag`, `text`, `vec`), `options` (an enum's choices, `\|` between), `doc`, `read_only` (only nested data and references kept as file data), `refKind` (what a reference points at: `GameObject`, a component kind or an asset kind such as `Texture`; known for empty ones too, from the schema) |
| `EditorScene.assetFromFile(kind, path)` | An asset of `kind` (`Texture`) made from a project file, as the placeholder JSON `setField` takes; "" when it can't be made (see `Editor.lastError`). Sounds are kept by path |
| `EditorScene.setField(object, kind, handle, name, json)` | Sets one field from JSON text (a field's `json`, or a plain array for a vector or colour), through the kind's `load`; an opaque component's data is replaced instead (`set_opaque`) |
| `EditorScene.addComponent(object, kind)` / `removeComponent(object, kind, handle)` | Adds a component of a registered kind (returns its handle, 0 on failure); removes one, opaque ones included |
| `EditorScene.componentKinds()` | The kinds a component can be added of, by name |
| `EditorScene.savePrefab(object, path)` | Writes an object and everything under it as a `.prefab.json` (`manager::prefab_file_json`: assets in the file's `assets` table, references outside it dropped, no placement of its own); false for a part of a model. `Editor.ensureMeta` gives the file its GUID |
| `EditorScene.placePrefab(path, parent)` | Places a copy of a prefab file under `parent` (no object: the root of the active scene), last, remembering its file; none when the file isn't a prefab |

It uses `SceneManager` helpers in `manager.rs`: `scene_value` (a scene as JSON), `rename_scene` and `load_scene_file`.
The component calls live in `inspect.rs`. Field types and enum options come from the kind's schema
(`platypus_kinds::register_fields`, written by `platypus_render_api`'s build script for the generated components); a
hand-written kind without one is typed from its saved values. The hand-written kinds register theirs too:
`platypus_physics_api` `kind_fields.rs` for the physics components, and `platypus_scenes` `kind_fields.rs` for `Camera`,
`Animation`, `Animator`, `AudioSource`, `AudioListener` and `SplineAnimate` (types, enum choices, reference kinds, docs).
Component placeholders name their object.

## Undo and redo ([history.ts](../editor/src/history.ts), [scene-edit.ts](../editor/src/scene-edit.ts))

`history.ts` is plain functions over a `History` record: `done` and `undone` lists of `Command`s (`label`, `run`,
`undo`, optional `merge`), a `saved` position, a `limit` (200 entries, the oldest drop off) and a `version` that listeners
watch. `perform` runs a command and records it; `record` keeps one that already happened (a finished handle drag).
Recording drops what was undone. A command with the same `merge` key as the last one folds into it (the first's undo,
the new one's run), so a drag or scrub is one entry; it never folds into the saved entry. `undo` / `redo` return the
label, or "" when empty. `isDirty` compares `saved` with the length of `done`; `markSaved` sets it, `clearHistory`
resets (a new or opened scene), and when the cap drops the saved entry the history stays dirty. `group` makes several
commands one entry, run in order and undone in reverse.

`scene-edit.ts` makes scene edits commands: pose (handle drags), rename, enable, create, delete, duplicate, paste and
reparent. Undoing a delete builds the objects again from a snapshot, with new ids, so a command can't keep a
`GameObject`: it keeps where the object is, a `Loc` (the index among the shown siblings at each level, from the root;
`locate`, `find`). Run and undo then find the right object by its place whatever its id is now. Only objects shown in
the Hierarchy count (`shown`: not destroyed, not `HideInHierarchy`), and `restoreAt` turns a shown index into the
sibling index that `EditorScene.restore` takes. `topmost` drops objects whose ancestor is also selected, and deletes go
last place first so earlier places stay valid. `copyObjects` / `pasteCommand` use the same snapshots. Copies are named like
Unity's, "Cube (1)", "Cube (2)" (`duplicateName`).

Undoing a delete also gives the rebuilt objects their references back ([refs.ts](../editor/src/refs.ts)). Before the
delete, `refsInto` notes each reference field of an object outside the deleted ones that points into them, by place (the
deleted object's `Loc`, the child path under it, and for a component its kind and place among that kind); after the undo
`repoint` sets those fields to the rebuilt objects and components. The delete command does this as part of its undo.

## The editor's components

Each component has a model module of plain functions (tested natively, without React) and a React component that
draws it. Styles are in `editor-ui.css` (bundled into the app's `game.css`), coloured through `--platypus-*` custom
properties.

### MenuBar ([menu.ts](../editor/src/menu.ts), [MenuBar.tsx](../editor/src/MenuBar.tsx))

`menus: Menu[]`, each with `MenuItem`s: `label`, `accelerator`, `action`, `submenu`, `checked`,
`disabled`, `separator`. The state is a `MenuState { top, open, path }`: the open top menu and the
highlighted index at each level; a submenu shows while its item is highlighted.

- **Mouse:** pressing a title opens or closes it; while one is open, hovering another title opens
  that one; hovering an item highlights it; a click runs it. When open, a transparent backdrop
  (an absolutely positioned child of the bar, far larger than the window, below the titles and
  menus) catches presses anywhere else and closes the menus.
- **Keys:** the bar takes focus (`tabIndex={-1}`) when a menu opens. `menuKey` handles
  arrows (Left/Right also open and close submenus and move between menus), Home/End, Enter/Space
  and Escape (one level at a time). Separators and disabled items are skipped.
- **From anywhere:** `input.on("keydown")` (gameplay keys, which arrive whatever has focus, except
  text fields) runs accelerators (`findAccelerator`, matched by `code` and modifiers), Alt+letter
  opens the menu starting with that letter, and F10 highlights the bar. None of this runs while a modal dialog is
  open (`modalOpen()`).
- **In text fields:** their keys don't reach `input`, so a `keydown` handler on the editor's root
  (`fieldAccelerator`) sees them as they bubble from the field. `fieldKeepsKey` (in `menu.ts`, with `isTextField`)
  says which the field keeps: typing, Delete, Backspace, Escape, Enter, arrows and Ctrl+A / C / V / X. Other Ctrl / Alt
  keys and the function keys run the menus (Undo, Redo, Save, New, Open, Duplicate, ...) and the field does not also handle them.

Accelerators are written like `Ctrl+Shift+S`, `F5`, `Delete`, `Ctrl++`; `acceleratorLabel` shows
them normalized.

### Dock ([dock.ts](../editor/src/dock.ts), [Dock.tsx](../editor/src/Dock.tsx))

A layout is a tree of `DockSplit { direction: "row" | "column", children, sizes }` (sizes are
fractions adding to 1) with `DockGroup { tabs, active }` leaves. Every operation returns a new
layout: `activate`, `removeTab`, `dockTab(layout, tab, group, zone, index?)`, `openTab` and `resize`. `hasTab` says
whether a tab is in the layout; `openTab` makes a tab active where it is, or adds a missing one to the first group.
After each change, empty groups are dropped, single-child splits are replaced by the child, and a
child split in the same direction as its parent merges into it, so docking left of a group in a
row adds a column instead of nesting a new row. `serializeLayout` / `parseLayout` save and load
JSON; `parseLayout` drops unknown tabs and returns null for malformed input.

`Dock` draws splits as flex rows or columns whose children have `flexGrow` = their fraction and
`flexBasis: 0`, with `SPLITTER`-pixel splitters between them, and groups as a `TAB_BAR`-high tab
strip over the active panel. Platypus has no layout read-back for scripts, so `computeRects` works out
the same rects in JS from the `rect` prop (the dock's place on screen); panels get their content
box from it (`render({ x, y, width, height })`: the size below the tab strip, and where it is on screen in UI
pixels, for a camera drawing into it), and drags use it:

- **Seeing through.** A panel with `seeThrough: true` gets a transparent group background (the
  `see-through` class), and `.platypus-dock` has no background of its own, so what is drawn under the UI (a
  camera's view of the world) shows in the panel's box. Other groups keep the panel colour.
- **Splitters** take pointer capture on press and resize from the layout as it was when the drag
  started (pixels moved divided by the split's free space).
- **Close box.** The active tab shows a `×` that calls `removeTab` (it stops the press, so it starts no drag). View >
  Panels opens the tab again.
- **Tabs** take pointer capture on press. Past 5 pixels the press becomes a drag: `dropTarget`
  finds the group under the pointer and the zone (tab strip or middle: `center`; within a quarter
  of an edge: that side), a translucent box previews the zone (`zoneRect`) and a ghost of the tab
  follows the pointer. Releasing calls `dockTab`. The drag state is kept in a ref too, since
  pointer moves render a tick later.

### VirtualList ([virtual.ts](../editor/src/virtual.ts), [VirtualList.tsx](../editor/src/VirtualList.tsx))

Fixed row height. The list is a scrolling box (`overflowY: auto`, `height` from props) with a
spacer as tall as all rows; only rows in `visibleRange(scrollTop, height, rowHeight, count,
overscan)` are mounted, absolutely placed at `index * rowHeight`. The engine scrolls the box
(wheel, scrollbar, keys) and the `scroll` event updates `scrollTop`, so the overscan rows cover the
frame before React catches up. The handle (`ref`) has `scrollToIndex(index, align)`
(`scrollForIndex`: only as far as needed, or start / center / end), `scrollToEnd` and `focus`.
With `keyboard` on (the default), arrows, Page Up/Down and Home/End move `selected` (`keyIndex`)
and scroll it into view.

### TreeView ([tree.ts](../editor/src/tree.ts), [TreeView.tsx](../editor/src/TreeView.tsx))

`roots: TreeNode[]` (`id`, `label`, `children`, `data`) and a `TreeState { expanded, selected,
focus, anchor }` the parent keeps. `flatten` turns the expanded part into rows (iteratively, so
deep trees are fine), drawn by a `VirtualList`. Clicks go through `clickSelect` (Ctrl toggles,
Shift ranges from the anchor, both add a range); keys through `treeKey` (Left collapses or goes to
the parent, Right expands or goes to the first child, Shift extends, Ctrl moves focus only,
Ctrl+Space toggles). Slots: `renderIcon`, `renderLabel`, and renaming with F2 (or a double-click
without `onActivate`): `renderRename(node, done)` or a default text field that commits on Enter or
blur and cancels on Escape, then `onRename(node, name)`. Expanded nodes show a CSS chevron (a
rotated square with two borders), since system fonts may lack the triangle glyphs. With `dragType` set, rows are
draggable (they carry their id under that data type, not while renamed) and a drop calls
`onMoveNode(dragged, parent, index)` (`parent` null: a root). While a row of the tree is dragged, each row has a top and a
bottom quarter as drop zones (`platypus-tree-zone`); the middle drops under the row. `dropPlace(roots, rows, at, zone)`
([tree.ts](../editor/src/tree.ts)) turns the zone into a place: before or after the row among its siblings, last under it
for the middle, and first under it for the bottom of an expanded row with children (as Unity does). The middle lights the
row (the `over` class); a quarter draws an accent insertion line (`platypus-tree-insert`) at the place's depth. A drop on
itself does nothing, and the drop doesn't reach the list behind. `acceptsDrop(e)` lets rows take other drags (files, say): such a drag gets `copy`
and a drop calls `onDropOn(e, onto)`.

### Console ([console.ts](../editor/src/console.ts), [log.ts](../editor/src/log.ts), [Console.tsx](../editor/src/Console.tsx))

`log.ts` keeps up to 10,000 `LogEntry`s and is read through `useSyncExternalStore`. Subscribing
starts the feed (`startLog`): `onEditorFrame` calls `Editor.logSince(lastSeq)` every frame, so the whole
history comes first and then each new record, once (`addRecords` skips seqs already seen). The panel has Clear,
Collapse (identical level and message shown once with a count, `collapse`), a search box over
message and target, and Info / Warn / Error toggles with counts (`countLevels`, debug counts as
info). Rows show a level dot, the time and the first line of the message, coloured by level. It
follows new lines while scrolled to the bottom; scrolling up pauses that until the end is in view
again.

`parseLinks` finds `path/file.ext:line[:col]` (relative, absolute, Windows drives, backslashes),
skipping hosts right after a URL scheme. Each link is its own `Text` element (spans aren't hit
targets of their own), and clicking it calls `Editor.openFile(path, line, col)`.

### Modal, dialogs and FileDialog ([modal.ts](../editor/src/modal.ts), [Modal.tsx](../editor/src/Modal.tsx), [file-dialog.ts](../editor/src/file-dialog.ts), [FileDialog.tsx](../editor/src/FileDialog.tsx))

Dialogs are drawn by the editor's own UI, not the OS. `modal.ts` keeps the stack of open dialogs (`pushModal(render)`,
`closeModal(id)`, `modalOpen()`, `subscribeModals`); `ModalHost`, rendered once at the end of the editor UI, draws them over
everything. `Modal` is the shell: an open modal `dialog`, so the engine's top layer does the work. Its `::backdrop`
dims everything under it (black at 0.35: blending is in sRGB, as in browsers, so a light tint shows; it was 0.6 when blending was in linear light); Tab, d-pad
and the pointer stay inside; Escape sends `cancel`, which `Modal` prevents and answers with `onCancel`; focus goes back to
where it was when the dialog closes. It has a centred box with a title and Enter for the default action (`onEnter`).
`Button` is its button and takes `tabIndex`, Enter and Space. Two dialogs return promises:

- `confirmDialog({ title, message, buttons, cancel })` answers with the label of the button pressed (the first is
  the default; Escape answers `cancel`, "" by default).
- `textDialog({ title, label, initial, ok, check })` asks for a line of text; "" when cancelled. `check` gives the reason
  a value can't be used ("" when it can).

`fileDialog({ mode, title, filters, dir, name, ok })` shows the open, save or folder dialog and answers with the chosen
project path, or "" when cancelled. It has breadcrumbs, the folder's entries (folders first; double-click or Enter opens,
Backspace goes up), a name field that also takes a typed path, the type list, New Folder, and an overwrite check
when saving. It starts with focus in the name field (the list in folder mode). `file-dialog.ts` holds the logic as plain functions: `matches` and `shownEntries` (folders always, files
that pass the filter, only folders in folder mode), `withExtension` (a saved name gets the filter's first extension once),
`resolveTyped` (a typed path relative to the folder, or to the root with `/`, `.` and `..` worked out; null when it
leaves the project or names a drive) and `stemOf`. Ready filters: `SCENE_FILTER`, `PREFAB_FILTER`, `MODEL_FILTER`, `ALL_FILES`.

### ProjectBrowser ([project.ts](../editor/src/project.ts), [ProjectBrowser.tsx](../editor/src/ProjectBrowser.tsx))

The project folder's files on a `VirtualList`: breadcrumbs (the project's name, then each
folder), a status and Refresh; rows with a folder or file icon, the name and the size, and `..`
inside a folder. Double-click or Enter opens a folder, Backspace goes up. It uses drag and drop
(web-ui `docs/input-and-events.md`):

- Rows are `draggable`; `onDragStart` puts the item's path in the transfer
  (`PROJECT_PATH_TYPE`).
- Folder rows, breadcrumbs, `..` and the list itself (the folder shown) are drop targets: their
  `onDragOver` accepts system files (copy) and project items that `canMoveInto` that folder
  (move), highlights the folder, and stops propagation so the innermost one wins.
- `onDrop` calls `copyIn` with the dropped paths or `moveInto` with the item's path, then logs
  the result and lists the folder again.
- A Hierarchy object (`OBJECT_TYPE`, in `project.ts`) dropped on a folder gets `copy`; `onDrop` calls
  `onDropObject(objectId, dir)`, which saves it there as a prefab (see below), and logs "Saved prefab <path>" or the
  failure.

The toolbar and keys work on the selected item (not `..`), each through a dialog: New Folder (a `textDialog`, then
`Editor.createDir`), Rename (F2; `Editor.rename`) and Delete (Del; a `confirmDialog`, then `Editor.trash`). Each logs
its result (a failure as a warning with `Editor.lastError`) and lists the folder again. Props: `onOpenFile(path)`
is called for a file opened by double-click or Enter, and `onTrashed(path, id)` after a delete, so the editor can undo
it with `Editor.restore(id)`.

`project.ts` turns the `Editor` file calls into promises (`listDir`, `copyIn`, `moveInto`), reading
`Editor.lastError` when one fails. `useScreenSize()` ([screen.ts](../editor/src/screen.ts)) gives the
window size in UI pixels, re-rendering on `Screen.onChange`.

## The Scene view ([SceneView.tsx](../editor/src/SceneView.tsx), [scene-camera.ts](../editor/src/scene-camera.ts), [frame.ts](../editor/src/frame.ts))

The editor's world is the engine's: the Scene view is a camera in it, drawing into the panel's box.

**The camera.** `SceneView` makes a `Scene Camera` object with `hideFlags = HideAndDontSave` (not listed,
never saved, never `Camera.main`) and `dontDestroyOnLoad`, with a skybox clear, a dark background and
`depth: -100` so game cameras draw over it. The panel is `seeThrough`, so the view shows in its box. Each
frame `place` sets the camera's position, rotation, projection, clip planes and `rect` (the panel's box
over the screen size, written only when it changed), and `sceneGrid` follows the `grid` prop. The
camera is destroyed when the panel unmounts.

**The model** (scene-camera.ts) is plain functions from one `SceneCameraState { pivot, yaw, pitch,
distance, orthographic }` to the next, tested without React (`M5-EDT-22`). The eye sits `distance` behind the
pivot along the view direction (an orthographic view sits farther back and takes its size from
`distance`, so nearby objects aren't cut by the near plane). Yaw 0 looks along -z; positive pitch looks down.

| Control | Function |
|---|---|
| Right-drag | `look`: turns about the eye (the pivot moves to keep the distance) |
| Right-drag + WASD / QE | `fly`: right, forward, down and up for the frame's `unscaledDeltaTime`; speed grows with `distance`, x4 with Shift |
| Middle-drag | `pan`: slides the pivot so the point under the cursor at its depth follows |
| Wheel | `zoom`: `distance` times 1.1 per 40 units, clamped |
| Alt + left-drag | `orbit`: turns about the pivot |
| F | `frame`: the pivot at the selection's centre, far enough back to see its padded bounds |
| Axis gizmo | `lookAlong`: looks along an axis about the same pivot |

Pointer capture keeps a drag going outside the panel. A left click that doesn't move selects nothing
(`onSelect(null)`), and the context menu is suppressed.

**Frame callbacks.** `onEditorFrame(fn)` ([frame.ts](../editor/src/frame.ts)) calls `fn` every frame
after scripts and animation have moved things. It makes one hidden object (`HideAndDontSave`,
`dontDestroyOnLoad`) with a component whose `onLateUpdate` calls the listeners, and returns the function
that unsubscribes. The Scene view and the live Hierarchy use it.

**Orientation gizmo.** An SVG in the top-right corner draws the six axes from `gizmoAxes` (farthest
first, so near ones paint over): positive axes are coloured discs with a line and a label, negative ones
small grey rings, and axes pointing away are dimmer. SVG shapes are not hit targets, so transparent boxes
sit over each axis end: clicking one looks along the opposite direction (towards the centre), and the
centre box toggles perspective and orthographic, shown as `Persp` / `Iso` under it.

**SceneIcons.** Cameras, directional, point and spot lights, audio sources and reflection probes (`iconKind`;
`visibleObjects()` lists the objects, depth first, leaving out `HideInHierarchy` ones) get a round 22 px
button at their projected position (`worldToScreenPoint`, dropped behind the near plane or outside the
panel), so icons keep one size at any distance. The art is a 16 px SVG per kind with fixed markup, so each
is rasterized once. A click selects the object (Ctrl or Shift adds); selected icons have an accent
border.

The icons move every frame but React doesn't: each frame's result is reduced to a key of the camera
state and each icon's id, kind and position rounded to whole pixels, and `setView` is called only when
the key changes. A still view costs a few projections and a string compare per frame.

**Wire gizmos.** Each frame, for the selected objects (when Show Gizmos is on), `drawGizmos` calls
`Gizmos` for what Unity draws: a camera's frustum (or a box for orthographic), a point light's sphere, a
spot light's cone, a directional light's rays, a probe's box, and an audio source's min and max
distance spheres. `Gizmos.present(camera)` then sends them to the Scene camera, so they draw over the
view (faint where hidden) and are replaced next frame. The lines belong to that camera, so the Game view
never shows them. See [world-rendering.md](world-rendering.md#editor-grid-and-gizmo-lines).

**Transform handles** ([handles.ts](../editor/src/handles.ts)). Like Unity: Q is the view tool, W move,
E rotate and R scale (keys, or the toolbar in the panel). The handles sit on the last selected object and
keep a constant size on screen (`HANDLE_PIXELS`). `handles.ts` is plain math over an abstract `HandleView`
(world to panel pixels, rays, units per pixel), so it is tested without a camera (`M5-EDT-24`). It lays out
the parts, finds the one under the pointer (the centre, an axis, or a ring's front arc, within
`GRAB_PIXELS`) and turns a drag into the object's new pose: move along a local axis or in the view plane,
rotate about an axis through the ring's plane, scale by the ratio along an axis or uniformly from the
centre. Ctrl snaps (1 m, 15 degrees, 0.1). `SceneView` draws the parts as SVG in a box just around them,
lights up the hovered part, and reports each finished drag with `onTransformed(object, before, after)`
so the editor can undo it.

`SceneView` also takes drops: `acceptsDrop(e)` says whether a drag can land (it gets `copy`), and `onDropAt(e, point)`
is called with the ground point under the pointer. `placement.ts` `groundPoint` casts the camera ray through the
pointer (`screenPointToRay`) and returns where it meets the plane y = 0 in front of the camera (within 10,000 m); a
ray that misses it gives the point `DROP_DISTANCE` (10 m) along the ray.

While a Project model or prefab is dragged over the view, `makePreview` makes the real object at once, hidden from the
Hierarchy and never saved (so it has no undo entry), and keeps it at the ground point under the pointer. Drag events
come with pointer moves only, so each editor frame checks that the pointer is still over the view with the button held.
When it is not, or on drop, the preview is destroyed; a drop then places the real object as before. Files dragged in
from the system get no preview (they are copied into the project on drop).

## The editor UI ([editor/src/main.tsx](../editor/src/main.tsx))

A menubar (File, Edit, View, Window, Help with accelerators and an Open Recent submenu), a dock
and a status bar. The default layout: Hierarchy on the left, Scene and Game in the middle,
Inspector on the right, Console and Project along the bottom. `.editor` has no background, so the
see-through panels show the world.

- **Demo scene.** `buildDemoScene()` ([demo-scene.ts](../editor/src/demo-scene.ts)) runs at startup: a
  floor, four shapes, the three kinds of light, a `Main Camera` (its `rect` is empty, so it draws
  nowhere until the Game panel gives it one), an audio source and a reflection probe, so every icon
  shows. It stands in only when the project has no last or start scene to open.
- **Hierarchy.** A `TreeView` over `visibleObjects()`, with `GameObject.id` as node ids and an icon by
  component (`kindOf`). `useSceneTree` computes a key of `id:parentId:name` per object in `onEditorFrame`
  and rebuilds the tree only when it changes, so adding, moving or renaming objects shows at once.
  Renaming sets `GameObject.name`.
  Dragging a row onto the middle of another reparents it (last child), and onto the top or bottom quarter of a row puts
  it before or after that row among its siblings, with an insertion line (`onMoveNode`; undoable, through
  `reparentCommand`; not into itself or its own children). Model and
  prefab files dropped on a row, or below the rows, are placed there (see Prefabs and placing models).
- **Edit menu.** Undo / Redo (Ctrl+Z, Ctrl+Y; the labels name the command, "Undo Move"), Cut, Copy, Paste, Duplicate
  (Ctrl+D), Delete and Select All. A focus area (`scene` or `project`, set by pressing in the Hierarchy / Scene view or the
  Project browser) says what they act on: in `project` Delete trashes the selected file (`Editor.trash`, undone with
  `Editor.restore`). Handle drags, renames, deletes and Hierarchy drags are commands; the status bar says what was undone
  or redone, and the title shows `*` while the history is dirty. The menu ends with Project Settings (see Project
  Settings) and Preferences (see below).
- **Selection** is the tree's selected ids, shared with the Scene view: selecting in either updates the
  other (`select`), and the Inspector shows the selected object (see Inspector below).
- **Game.** `GameView` sets `Camera.main`'s `rect` to the panel's box while it is shown and to an empty
  rect when it is hidden or unmounted, so the game's camera draws only inside the Game tab (see Game cameras below).
- **Project** is the project browser.
- **File menu.** New Scene, Open Scene (Ctrl+O; the file dialog over `scenes` with the scene filter), Open Recent (the
  project's last 8 scenes, kept in PlayerPrefs as `editor.recent:<project>`; a file that fails to open is dropped from
  the list), Clear Recent, Save (Ctrl+S; Save As when the scene has no file), Save As and Exit. They work through
  `EditorScene`. New, Open and Exit first ask "Save changes?" (Save, Don't Save, Cancel) when the history is dirty. A
  save marks the history saved, adds the scene to the recent list and calls `Editor.ensureMeta` on the file. The save
  dialog keeps a start folder that doesn't exist yet (`FileDialog.tsx`), so a new project's `scenes` folder is made on save.
- **GameObject menu.** Create Empty, 3D Object (Cube, Sphere, ...), Light and Camera; each is an undoable command and
  selects the new object.
- **Window title.** `windowTitle`: the scene's name (or "Untitled") with `*` while unsaved, then the project and
  "Platypus Editor".
- **Startup scene.** The editor opens the project's most recent scene if it still exists, else `Editor.startScene`;
  the demo scene is built only when neither is there.
- **Game cameras.** `placeGameCameras` (`scenes.ts`, every frame) gives `Camera.main` the Game panel's rect while it is
  shown and an empty rect for every other game camera, so they draw nowhere else. Each camera's own `rect` is kept
  (`authored`; a zero rect means the whole view) and `withAuthoredCameras` puts it back while saving, so a saved scene
  holds the cameras' real rects, not the editor's.
- **View menu.** Panels lists the panels, checked when in the layout; choosing one opens it, or closes it when it is the
  active tab of its group (`hasTab`, `openTab`, `removeTab`). Show Grid / Show Gizmos toggle the Scene view's `grid` and
  `gizmos` props and are kept as preferences.
- **Preferences.** Edit > Preferences ([Preferences.tsx](../editor/src/Preferences.tsx)) is a dialog of checkboxes kept in
  PlayerPrefs as `editor.pref.<name>` (apart from any project; a change applies at once): the grid, the gizmos, and
  whether the last scene opens at start (`reopenLastScene`; off: only the project's `startScene`, else the demo scene).
- **Window menu.** Save / Load / Reset Layout keeps the layout in PlayerPrefs (`editor.layout`), and the saved
  layout loads at start.
- **Help menu.** Documentation (F1) calls `Editor.openFile("docs/editor.md", 1, 1)`, which looks in the source tree. About
  shows a dialog with the product name, version and project folder.
- **Closing the window.** `Window.main.onCloseRequested` cancels the close when the history is dirty and asks the same
  "Save changes?" question as Exit (`closeGuard`); the window closes after Save or Don't Save.

### Prefabs and placing models

`assetKind(path)` ([placement.ts](../editor/src/placement.ts)) sorts a project file by its name: `.prefab.json` is a
prefab, `.scene.json` a scene, `.glb` / `.gltf` / `.fbx` a model. `placeable` is a model or a prefab.

- **Saving a prefab.** Drag a Hierarchy object onto a folder of the Project browser. `savePrefabOf` takes a free name
  `<dir>/<object name>.prefab.json` (`Editor.freeName`), writes it with `EditorScene.savePrefab` and gives it a
  `.meta` (`Editor.ensureMeta`). The file holds the object and its children with an `assets` table like a scene's.
- **Placing.** A model or prefab dragged from the Project browser or from the system lands:
  - on the Scene view: at the ground point under the pointer, at the root (a preview follows the pointer while
    dragging, except for system files; see the Scene view);
  - on a Hierarchy row: as that row's last child, with no point (the model's own position);
  - below the rows: at the root.

  `placeFile` runs it as a `placeCommand` (`Place <name>`): a model is `Model.load(path).instantiate`, a prefab
  `EditorScene.placePrefab`. A failure warns with `Editor.lastError`.
- **Double-click.** In the Project browser (`openFile`) a scene opens (after the unsaved-changes question), a model or
  prefab is placed at the root, and any other file opens in the code editor.
- **System files.** Files dropped from the system are first copied into the project (`Editor.copyIn`, with their
  `.meta`): models into `assets/models`, prefabs into `assets` (`importFolder`; the folder is made if missing). The log
  says "Imported <path>", then the copy is placed like a project file.
- **Undo and selection.** `placeCommand` makes the object again on redo and destroys it on undo, finding the parent by
  its place in the tree. The new object is the last child of its parent, and is selected.

## Inspector ([Inspector.tsx](../editor/src/Inspector.tsx), [inspector.ts](../editor/src/inspector.ts))

The Inspector shows the selected object's name, enabled flag, tag and layer, its Transform (local position, Euler
angles in degrees, scale), then a foldout per component from `EditorScene.components`, each with an editor per field
from `EditorScene.fields`.

- **Field editors by type.** `number`: a text field parsed by `parseNumber` and shown by `formatNumber` (at most 4
  decimals); `bool`: a checkbox; `string`: a text field; `enum`: a choice over `options`; `vector2/3/4`: one number
  field per component; `color`: a swatch (see below). Labels come from `labelOf` (`fieldOfView` is `Field Of View`).
  `json` fields (nested data) are shown, not edited.
- **References.** `object`, `component` and `asset` fields use `RefInput` ([InspectorInputs.tsx](../editor/src/InspectorInputs.tsx)).
  A button opens a picker dialog ([Pickers.tsx](../editor/src/Pickers.tsx), `pickDialog`, like Unity's object picker): a
  filtered list of the shown objects by path, of the objects that have the component kind, or of the project's files of
  the asset kind (`assetExtensions`, `fitsAsset`). A Hierarchy row can be dropped on an object or component field and a
  Project file on an asset field. A texture is made from its file with `EditorScene.assetFromFile`; a sound is kept by
  path. `FieldInfo.refKind` says what a field may point at.
- **Colours.** A colour's swatch opens the colour picker (`colorDialog`: a palette and a hex field).
- **Tag and layer.** Tag is a text field. Layer is a choice of the project's layer names (`Editor.layerNames`).
- **Choices.** A `Choice` opens a picker dialog, not a dropped-down list: z-index orders siblings only here
  (web-ui `docs/differences.md`), so a list inside a row would paint under the rows after it.
- **Several objects.** With several selected, the Inspector shows the components they share. A value that differs shows
  "—". An edit (for a vector, only the parts edited) goes to all the objects and is one undo step.
- **Live refresh.** The panel builds a key of what it shows each editor frame and redraws only when the key changes, so
  values follow the world (a handle drag, a script, an undo) without a redraw every frame.
- **Undo.** Every edit is a command (`fieldCommand`, `localPoseCommand`, `addComponentCommand`,
  `removeComponentCommand`) that keeps old and new JSON. Undo can rebuild an object with new handles, so a command finds
  its component again by kind and place (`placeOf`, `componentAt`: the n-th component of that kind), not by handle.
  Removing a component takes a snapshot first and undoes by restoring it.
- **Opaque components** (a game's scripts, whose kind the editor doesn't have) are listed with their saved data and
  edited as data: the type of each field comes from the value's shape.
- **Schemas.** The hand-written kinds (physics, Camera, Animation, Animator, AudioSource, AudioListener, SplineAnimate)
  have field schemas, so they get typed editors, enum choices, reference kinds and docs like the generated ones.
- **Add / Remove Component.** An Add Component button lists `EditorScene.componentKinds`; each component's header has
  a Remove button. Both are undoable.

## Project Settings ([ProjectSettings.tsx](../editor/src/ProjectSettings.tsx))

Edit > Project Settings opens a dialog with `project.json`'s settings in groups (Identity, Game, Build, Display, Quality, Layers), taken from
`Editor.projectFields`. Display (`mode`, `width`, `height`, `refreshRate`, `resizable`), Quality (`default`) and the
user Layers (3, 6 to 31) are keys inside the `display`, `quality` and `layers` objects, named with dotted keys
(`display.width`, `quality.default`, `layers.8`); the form labels them by their last part ("Layer 8"). The inputs are the Inspector's (`TextInput`, `Checkbox`, `Choice`, exported by
[Inspector.tsx](../editor/src/Inspector.tsx)). Each change calls `Editor.setProjectField`, which rewrites only that key's
text in the file (`platypus_core::project_edit`): `${templates}`, key order, spacing, the platform sections and unknown
keys stay as written, a new key goes last with the file's indentation (inside a dotted key's section, in that
section's own style; a missing section is added last), and an empty enum choice removes the key. An emptied layer
name or size (a `number` field) removes its key too, and a section left empty by a removal goes. A
refusal shows `Editor.lastError` in the dialog.

The dialog has its own undo history (a `history.ts` `History`, apart from the scene's) with Undo and Redo buttons. A
command's undo writes the old value back, or removes the key when the file had none.

## Tests and checks

- `editor/test/*.test.ts` (`models`, `log`, `project`, `scene-camera`, `handles`, `file-dialog`) are plain TypeScript with a
  tiny helper, [check.ts](../editor/test/check.ts). Each file's `run()` returns what failed, one line each.
  [editor_models.rs](../tools/platypus_ts_tests/tests/ui/editor_models.rs) bundles each file like a game and compiles
  it natively with the whole engine API, in the normal, GC-stress and collection-off modes; every mode must give "".
  Run it with `cargo test -p platypus_ts_tests --test ui editor_models`.
- IDs: the models (`M5-EDT-06` to `-14`, `-18`, `-19`), the log feed (`M5-EDT-20`), the Scene camera's controls and
  axis gizmo (`M5-EDT-22`), the transform handles' math (`M5-EDT-24`), project paths and `Editor` without a project
  (`M5-DND-08`), the file dialog's filters, saved names and typed paths (`M5-EDT-31` to `-33`).
- Rust units: the capture layer in `logging.rs` (`M5-EDT-25`), `Editor.openFile` (`M5-EDT-26`), the `Editor`
  namespace needing a project (`M5-EDT-27`), the project file ops (`M5-DND-12`) and those of `files.rs`: write and
  read, stat and new folders, rename, trash and restore (`M5-EFS-02` to `-05`).
- Smoke test `M5-EDT-28` (`tools/platypus_build/tests/editor_smoke.rs`): builds `editor/` and runs
  `platypus-app editor --project examples/arcade --bench` with `clickText` input. It needs a built host and a GPU.
  Shorter runs in the same file compare `--bench-shots` pictures of the Scene view: after the Game tab and back the view
  is unchanged (its own camera, grid and icons; `M5-EDT-29`), and selecting Cube in the Hierarchy names it in the
  Inspector and draws its handles (`M5-EDT-30`), and File > Open Scene shows the file dialog, Escape closes it, and New Folder in the
  Project browser asks for a name (`M5-EDT-34`, compared by pixels in the middle of the window).
- Scenes: `platypus_core::meta::ensure` (`M5-SCN-12`, in `meta.rs`) and the smoke test `M5-SCN-13` (New Scene, a Cube,
  Ctrl+S, Enter writes `scenes/Untitled.scene.json` and its `.meta`; a second run with `startScene` opens it).
- Undo: the history (`M5-UNDO-02` to `-05`, `editor/test/history.test.ts`), `EditorScene` snapshots and restore in
  `editor_tests.rs` (`M5-UNDO-01`), scene save / new / open (`M5-SCN-11`), and the smoke test `M5-UNDO-06` (Delete on
  Cube, Ctrl+Z, Ctrl+Y, Ctrl+D, a Hierarchy drag and its undo, each in the status bar). References restored on undo of a
  delete and the "Cube (1)" copy names: `M5-UNDO-07`, `-08`.
- Hierarchy drops: `dropPlace` (`M5-EDT-46`, `models.test.ts`) and the smoke test `M5-EDT-47` (the insertion line, a drop
  between rows, its undo).
- Inspector references and more: `Editor.layerNames` (`M5-EDT-48`, in `editor.rs`), and `M5-EDT-49`, `-50` (the reference
  picker, colour picker, tag and layer, several objects; `editor_tests.rs`, `inspector.test.ts`, `editor_smoke.rs`).
- Inspector: the `EditorScene` component calls in `editor_tests.rs` (`M5-EDT-35`: components, fields, setField, add
  and remove, opaque data), the smoke test `M5-EDT-36` in `editor_smoke.rs`, and the logic in
  `editor/test/inspector.test.ts` (`M5-EDT-37`, run by `editor_models`).
- Project Settings: `project_edit` units (`M5-PRJ-01` to `-03`: replace in place, add last, remove with its comma),
  the `Editor` project calls in `editor.rs` (`M5-PRJ-04`) and the smoke test `M5-PRJ-05` (company typed, the file gains
  one key, Undo restores it exactly), `M5-PRJ-06` (dotted keys in `project_edit`: replace in place, add last in the
  section's style, add the section, drop an emptied one) and `M5-PRJ-07` (the dialog's Display, Quality and Layers
  fields). `cond && f()` with a void `f` (the dialog buttons use it) is `M9-TS-3524`.
- Dialogs: the smoke test `M5-EDT-43` (the Hierarchy is darker behind a dialog, F2 works on the Project list after
  Escape, Tab wraps inside New Folder). Keys in text fields: the key rule `M5-EDT-44` (`fieldKeepsKey`, in
  `models.test.ts`) and the smoke test `M5-EDT-45` (Delete and Ctrl+A stay in the Inspector's name field, Ctrl+Z
  undoes, Ctrl+S opens Save Scene As).
- Panels and menus: the smoke test `M5-EDT-41` (the Console tab's close box, View > Panels > Console, Edit >
  Preferences and Help > About open and close) and the dock model `M5-EDT-42` (`hasTab`, `openTab`, in `models.test.ts`).
- Prefabs and placing: `savePrefab` / `placePrefab` and a prefab's `assets` table reaching its copies in
  `editor_tests.rs` (`M5-PRF-10`); the smoke test `M5-PRF-11` (Cube dragged onto the Project list is saved as
  `assets/Cube.prefab.json` with its `.meta`, the file dragged onto the Hierarchy is placed, Ctrl+Z removes it) and
  `M5-DND-20` (`assets/house.glb` dragged onto the Scene view is placed) and `M5-DND-22` (the drag preview exists while
  the pointer is over the view and is gone after the drop or when the pointer leaves); the placement model in
  `editor/test/placement.test.ts` (`M5-DND-21`, run by `editor_models`: the ground point, file kinds, import folders).
- Not ported: the React component tests (the menubar through the reconciler, the project browser; `M5-DND-10` is struck). The Scene view in the window is a manual check (`M5-EDT-23`); the grid and line passes
  have goldens (`M5-RND-22`, `-23`). `Gizmos` and `HideFlags` are covered in the engine's own tests.
- `cargo run --release -p platypus_app -- editor --project examples/arcade --screenshot out.png --frames 30` renders the editor
  (`M5-EDT-21`, checked by eye). There is no golden: the editor uses system fonts.

## Limits and known gaps

- The editor source works around gaps in web_ui_ts (listed in `tools/web_ui_ts/TODO.md`, "TypeScript gaps"),
  each cited where it is: class lists are built by `cx()` ([classes.ts](../editor/src/classes.ts); mixed-type arrays),
  the `..` row is told apart by index (struct conversion of array elements), `renderRow` carries a `: ReactNode`
  return annotation (no adapter for a function returning an element), and `parseLayout` reads its JSON into a typed
  `SavedNode` (no dynamic values). The other `: ReactNode` annotations are on closures that call themselves or each
  other, which TypeScript also asks for (TS7023). Remove each when the gap closes.

- Fixed row heights only; no tab reordering by position in the strip (a drop in a strip appends).
- The Scene view has no picking in the 3D view (only icons select) and snaps only with Ctrl;
  gizmos and handles are drawn for the selection only.
- A placed prefab is an unlinked copy: it keeps its file's path but later edits to the file don't reach it, and there
  is no "apply to prefab". Files dragged in from the system get no drag preview. Files dropped on the
  Scene view are placed at the root, never under a selected object.
- The Inspector shows nested data read-only. Assets other than textures and sounds can't be set from files (meshes,
  materials and clips come from models and code). There is no scrubbing a number by dragging its label, and field docs
  are not shown (no tooltips yet). Script `@property` fields need the project's code in the editor (TODO.md E11); a
  kind without a schema is typed from its saved values (no enum options or docs).
- Project Settings does not edit the platform sections or the per-level `quality` overrides (edit them in the file;
  they are kept). Path settings (`licenseFile`, `icon`, ...) are text fields,
  with no file picker.
- Only one scene is edited at a time (Open replaces the loaded scenes); there is no prefab editing and no scene tabs.
- Panels can't float in windows of their own.
- The project's own code and play mode are not in the editor yet (TODO.md E11): scripts of a scene stay opaque
  components, and the Game panel shows the scene without running it.
- The Project browser has no context menu; New Folder, Rename and Delete are on its toolbar and keys.
- No native menus yet (TODO.md M5.e).
