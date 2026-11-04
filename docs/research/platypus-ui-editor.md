# A UI editor: React (TSX) as the source, edited visually

Status: research (2026-11-05). Builds on `platypus-ui-inspector.md` (the inspector, done in part) and on the debug-info /
VS Code work of another session (`platypus-ts-debug-info.md`: line tables, a DAP server, VS Code attach).

## Question

Can the UI of a game be edited visually, with the TSX source as the thing that changes, in an editor made for UI
(not the engine editor), with light and dark modes? If not, what tools would we have to make?

## Answer: yes, without Node tooling for the heavy part

The pieces already exist:

| Need | What we have |
|---|---|
| Find JSX in source | `platypus_ts` parses TSX with oxc and lowers JSX as a **text edit over byte spans** (`lower/jsx.rs`: `element_text` knows each element's `open.span.start`); `platypus_ts::modules` bundles files the same way ("the text is edited, not re-printed", so formatting and comments survive). A UI editor can rewrite source with the same kind of edits. |
| Map a running node to its JSX | Not yet. The bundle maps lines back to files (`Bundle::locate`); the debug-info work keeps a line table for the JIT (columns are "later"). A JSX element becomes `Ui.begin("box")...`: a build with `--add-ui-inspector` can add the element's source position there and the host keeps `node -> (file, line, col)`. Cost only in the inspector build. |
| Inspect what a node is | `UiTree::inspect` (matched rules, winning declarations), `UiInspector` API, the overlay and the editor's UI tab. |
| Apply an edit live | `inspect_set_style` (an inline override now); `platypus-build dev` rebuilds and signals the app; reload keeps state (`Runtime::refresh`, Fast Refresh semantics). A source edit is therefore: patch the file, the existing watcher rebuilds, the window updates. |
| Light and dark | `prefers-color-scheme` is a media feature (`platypus_ui/src/media.rs`), the OS theme is read (`platypus_platform/src/theme.rs`), the editor's CSS already colours through 16 `--platypus-*` custom properties, and `UiTree::set_media_env` can force a scheme, so the UI under edit can be previewed in both. |

So "React as source and UI edited" is a source-to-source tool: **pick a node, find its JSX site, patch the TSX text**.

## What can be edited in source (and what cannot)

A JSX site is editable when its value is a literal:

- `style={{ width: 120, color: "red" }}`: change, add or remove a property in the object literal.
- `className="card hot"`: edit the string.
- text children, literal attributes (`placeholder`, `src`), reordering and deleting elements, wrapping in a `box`.

Not editable as source, shown read-only with the expression (and what it evaluated to now):
`style={styles.card}`, `{...props}`, conditionals, `.map` output (one site, many nodes: edits apply to the site and say so),
and component instances (`<Card title="x"/>`: edit the *definition's* JSX, or the instance's literal props).
CSS edits go to the stylesheet text (`.card { ... }`): that needs rule source ranges (the parser keeps none today; see the
inspector note, "Gaps").

## The tools

1. **Source tags in the inspector build** (compiler): `Ui.begin(..)` gets the element's position when the flag is on; the host
   publishes `UiInspector.source(id) -> "file:line:col"` (also gives "reveal in VS Code"). Reuses the debug-info line/column work.
2. **A Rust source editor**, a crate (say `platypus_ui_edit`) plus a `platypus-build edit-ui` subcommand: given a file and a
   position it applies one operation (`set-style`, `set-class`, `set-text`, `remove`, `move`, `wrap`) with oxc spans and returns
   the new text or a refusal ("this value is an expression"). No JavaScript engine, no Node; testable without a window.
3. **The UI editor**, a UI-only game like the engine editor (`ui-editor/`, GLES tier, light): Elements tree with source,
   Styles panel (the `UiInspStyles` parts already shared with the overlay), a canvas showing the UI under edit, light/dark switch
   for the editor and, separately, for the preview. Two ways to show the UI under edit: **(a)** attach to the running game
   built with the inspector (a local socket carrying the `UiInspector` calls: first, simplest, the game stays a separate
   process) or **(b)** run the UI program inside the editor in a canvas element (needs the canvas UI element from TODO.md "Later").
   Start with (a).
4. **VS Code**: a thin extension. Its host is Node by necessity, so a little JavaScript is unavoidable there, but it should
   only start the Rust tools and speak to them over the socket/DAP channel the other session is already building: "Open in UI
   Editor", reveal-in-source, CodeLens "edit visually". The edit logic stays in Rust. A Node-side reimplementation would
   duplicate the compiler's TSX handling and drift from it.

## Order

1. Source tags and `UiInspector.source(id)`.
2. `platypus_ui_edit` with `set-style` first, round-trip tests (text in, text out; comments and formatting kept).
3. The inspector's Edit mode gains "write to source": a literal edit patches the file, the dev watcher reloads.
4. The `ui-editor` app over a socket, then light/dark, undo as a text history.
5. VS Code commands.

## Risks and open points

- A node from `.map` or a shared component has several instances for one site; the editor must say which it edits.
- Reload keeps state only where hook signatures match; a structural edit remounts that component.
- Concurrent edits: the file may change under the editor (VS Code, the other session): patch against the text the position was
  taken from and refuse when the span no longer matches.
- "Similar format": see the open question in TODO.md ("UI editor"): the element view could read as devtools (tree + styles) or
  as JSX text lines; both come from the same source tags.
