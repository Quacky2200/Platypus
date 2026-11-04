# React Pane, New React Component, and inspecting the game's UI in the editor

Status: design only (2026-11-06). Nothing here is built. It follows a request from the user while trying the
UI inspector inside the editor.

## The problem

The editor's "UI" tab (`editor/src/UiInspectorPanel.tsx`) inspects **the editor's own UI**: the host has one
screen tree and the editor is that tree, so the tab shows the editor's menus and docks, and Inspect picks them.
What a game author wants to inspect is **the game's UI as the Game tab shows it**. Today that does not exist
in the editor:

- The Game tab draws the scene through the game's cameras (`placeGameCameras`); it does not run the project's code
  (TODO.md E11: the project's code and play mode are not in the editor yet).
- A game's screen UI is drawn by the host's one screen root, which in the editor is the editor.
- A game's *world* UI is `WorldPanel` components, each with its own `UiTree` and `Interaction`
  (`docs/world-rendering.md`, "World panels"), but a script creates the React root of a panel
  (`createRoot(panel).render(<Menu />)`), so a scene saved by the editor has panels with nothing in them.

## Two separate things

1. **Which tree the inspector looks at** (the UI inspector's "tree ids", TODO.md).
2. **Authoring game UI in the editor**: `GameObject > React Pane` and `File > New React Component`.

(1) is useful once there is a game tree to look at; (2) needs the project's code in the editor (E11) for the
pane to render a real component.

## 1. Tree ids

Every `UiInspector` call names a tree: `0` the screen, a `WorldPanel` component id for a panel. The host's
`sync_ui_inspector` already inspects one tree per frame, so the change is to hold a *current tree* (set by
`UiInspector.target(id)`), publish the list of trees (`UiInspector.targets()`: id, name, kind), and read hover,
pick and probes from the current tree.

- **Editor tab:** a target menu at the top of the tab: *Editor UI* (today's behaviour) and one entry per world
  panel in the loaded scene, named after its GameObject. The default is the first panel when there is one.
- **Picking in the Game tab:** with Inspect on, the pointer over the Game view is turned into a camera ray
  (`panels.rs` already does this for pointer events on world panels) and the panel it hits becomes the target;
  the node under the pointer is picked inside that panel's tree. This is how the game's UI is told apart from
  the editor's: the editor's own roots are marked as tool UI (like the inspector's own `uii` roots, which the
  host already skips when picking), so the editor chrome is only inspected from the *Editor UI* target.
- **Overlay (games):** unchanged. A game built with `--add-ui-inspector` gets the same target menu in the card's
  header; the screen is the default, panels are listed after it.
- **Game screen UI inside the editor** needs a tree hosted by the Game view (the "canvas UI element" in
  TODO.md); until then the Game tab has no screen UI to inspect.

## 2. React Pane

`GameObject > React Pane` creates a GameObject with a `WorldPanel` and a **`component`** property: the module
and export that fill it (`"Hud"` for `src/modules/Hud.tsx`, export `Hud`, or `"Hud#Corner"`).

- **Saved in the scene** as an ordinary property of the `WorldPanel` kind (`platypus_kinds` schema), so the
  Inspector edits it with a dropdown of the project's components (the files of `src/modules/` that export a
  function component).
- **At run time** the loader mounts it: `createRoot(panel).render(<Component />)`. This needs the project's code
  (E11): a second bundle JIT-loaded into the editor with its hooks held back in edit mode. Until then the
  pane shows a placeholder (the component's name on a tinted plane) so panes can be placed, sized and saved.
- **Size and scale** are the `WorldPanel`'s own (`width`, `height`, `pixelsPerMeter`); the Scene view draws the
  plane and its frame like any quad.
- A pane is also a **target** of the inspector (part 1), so selecting it and choosing the UI tab shows its
  elements and styles; edits are live overrides, written back through *Copy CSS / Copy JSX* (no source editing).

## 3. New React Component

`File > New React Component` (also on the Project browser's context menu once it has one) asks for a name and
writes `src/modules/<Name>.tsx`, a function component with no engine imports (globals only, like any game):

```tsx
export function Name() {
  return <box className="name">Name</box>;
}
```

and `src/modules/<Name>.css` when the user ticks "with a stylesheet" (added to the game's CSS by the build as
other `.css` imports are). The file is created with `Editor.writeFile` (the project file calls the editor
already has) and given its `.meta` (`Editor.ensureMeta`). Bare imports resolve from `src/modules/`
(`docs/platypus-build.md`, "Modules and packages"), so a game's `main.tsx` imports it as `from "Name"`; the dialog
offers to add that import and a mount for it.

- Name rules: a TypeScript identifier starting with a capital; an existing file is never overwritten.
- The Project browser lists the new file and opens it with the system editor (`Editor.openFile`).

## Order of work

1. Tree ids in the API and the host (`target`, `targets`), the target menu in the UI tab, the editor's roots
   marked as tool UI so game picking never lands on them. Testable now with a scene whose script makes a
   `WorldPanel`, by running a game (not the editor) with the overlay.
2. `File > New React Component` (independent of the rest; only project file calls).
3. `GameObject > React Pane` with the placeholder and the `component` property.
4. E11: the project's code in the editor, which makes the pane render its component and the game's scripts run.
5. The canvas UI element (a screen UI inside the Game view), so a game's screen UI can be inspected there too.

## Open questions

- The name: **React Pane** (the user's word), or **UI Panel**, or **React Panel**? ("Pane" matches the user's
  wording; `WorldPanel` stays the component's name.)
- Should a pane be addable *without* E11 as a pure placeholder (step 3 before step 4), or wait for it?
- One pane per component, or several panes sharing one component with their own props (`props` as a JSON
  property)?
- Should New React Component also offer *New Stylesheet* and *New Script* in the same menu?
