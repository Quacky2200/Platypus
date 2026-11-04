# A UI inspector for games and the editor

Status: started (2026-10-31). The tree-side query API is done (`M3-INS-01..03`); the rest is planned below.

## Goal

Live UI design: hover an element to outline it, click to pin it, see its box, attributes, inline styles and the
stylesheet rules that match it (which declarations win the cascade and which are overridden), and change a style
and see it at once. It must be usable by any game and embedded in the editor (`editor/`).

## Two builds: fast path and inspection

The release build stays as it is: no recording, no extra code, no extra API. Inspection is opt-in:

```bash
platypus-build build <game> --add-ui-inspector
```

The flag is a build property, not a runtime switch, so a shipped game cannot be inspected by accident and the
release path carries no cost. It does three things:

1. writes `"uiInspector": true` into `build/game.pbundle` (the host reads it next to `renderer`);
2. adds the inspector component to the program (the compiled inspector TSX is only part of the bundle with the flag);
3. lets the host answer `UiInspector.*` calls; without the flag they are not declared for the game and the host
   answers nothing.

`platypus-build package` carries the flag's result like any other bundle content; packaging an inspected build is
allowed but warns.

## What is cheap, and why nothing is recorded

Rules are matched **when asked**: `UiTree::inspect(id)` runs the same `matched_declarations` the cascade uses for
one node, and says which declaration wins per property (last of: normal rules by specificity and order, inline
styles, `!important` rules). So the host keeps no per-node match records, and the debug cost is paid by the
inspector only, per inspected node. This replaces the earlier idea of recording applied rules while styling: that
would have slowed every restyle for data nobody reads.

Done (`crates/platypus_ui/src/tree/inspect.rs`, tests `crates/platypus_ui/tests/inspect.rs`):

| Call | Gives |
|---|---|
| `UiTree::inspect_pick(x, y)` | the topmost node under a point (`hit_test`; the overlay uses `pointer-events: none` to be skipped) |
| `UiTree::inspect(id)` | `Inspected`: kind, parent, children, `id`, classes, border box, padding box, inline styles (`applied` or shadowed), matching rules with selector text, sheet id, rule index and per-declaration `applied` |
| `UiTree::inspect_set_style(id, prop, value)` | sets an inline declaration like a `style` prop; an empty value removes it |

Gaps in this data (to do): the source file and line of a rule (the parser keeps neither; add a `source` to
`StyleRule` only when the flag is on, via `add_stylesheet_with_base`), `@media` conditions of a rule, pseudo-element
rules (`::before`), the computed value of every property, and `!important` inline.

## Script API

Done (`crates/platypus_engine_api/src/ui_inspector.rs`, host half `crates/platypus_app/src/ui_inspector.rs`; tests
`M3-INS-05..06`). The namespace `UiInspector` is always declared (the compiler's manifests are not per build), but it is
inert unless the host called `enable()`, which it does only for a bundle with `"uiInspector": true`: every getter then
answers 0, `""` or `false` and edits are dropped. The TSX inspector is what the build flag adds.

The host publishes a snapshot once per frame (`sync_native_render`), so calls from compiled code answer at once with no
ops. Scripts say what they want with `select(id)` and `expand(id, open)`; the host publishes the hovered, selected and
expanded nodes, their ancestors and the children of all of those (what an elements tree shows). Getters are scalar,
because the API types are numbers, strings and string lists:

- `enabled()`, `hovered()`, `selected()`, `select(id)`, `expand(id, open)`;
- per node: `kind`, `parent`, `childCount` / `childAt(id, i)`, `elementId`, `className`, `x` / `y` / `width` / `height`,
  `paddingBox(id, 0..3)`;
- inline styles: `inlineCount`, `inlineProp` / `inlineValue` / `inlineApplied(id, i)`;
- matching rules: `ruleCount`, `ruleSelector` / `ruleSheet(id, i)`, `declCount(id, rule)`, `declProp` / `declValue` /
  `declImportant` / `declApplied(id, rule, i)`;
- `setStyle(id, prop, value)`: queued, applied by the host before the frame is drawn; `""` removes.

Ids are node ids of the screen `UiTree`; scripts never hold tree references, as for everything else. World panels and
windows are not inspected yet (see Risks: tree ids).

## The component

`UiInspector` is written in TSX like the editor's panels, so it is drawn by the same renderer and tested the same
way. Three parts, usable apart:

- `InspectorOverlay`: a full-screen `position: fixed; pointer-events: none` layer drawing the hover outline (content,
  padding and border tinted like browser devtools) and a label (`box.card.hot 100×40`). It uses
  `UiInspector.hovered()` each frame and `info(id).rect`.
- `InspectorPanel`: elements tree (virtual list, like the editor's `TreeView`), the Styles list (rules in cascade
  order, overridden declarations struck through, each value editable in place, a toggle per declaration), and a box-model diagram.
- `useInspector()`: the selection and pick-mode state, for a host to arrange its own layout.

A game mounts the pair when the flag is on (`if (UiInspector.enabled) <UiInspectorRoot />`, a key such as F12
toggles it, off by default). The editor mounts the panel as a dock tab ("UI") and uses pick mode on its own UI and the Game panel's UI.

### Editor

Done (first version): a dock tab "UI" (`editor/src/UiInspectorPanel.tsx`, test `M3-INS-10`). The editor is a game
compiled by the same tool, so `platypus-build build editor --add-ui-inspector` makes it inspect its own screen UI: the
tab shows the elements as the editor's `TreeView` (open rows ask the host to publish their children), the picked node's
styles (`UiInspStyles`, from `tools/platypus_build/inspector/shared.tsx`, which the overlay appends and the editor
imports; a program that already has it does not get it twice) and an Edit toggle. A node picked in the tree is pinned, so
the overlay (F12) outlines it. Without the flag the tab says how to turn the inspector on.

Not done: inspecting the *project's* UI rather than the editor's own (the Game panel's tree) needs the `UiInspector`
calls to name a tree (`screen`, a panel, an editor-hosted game UI) and every call to take a tree id; and the panel
could open in its own window (`Window.create`, `createRoot(win)`: the panel is a plain component, and the pointer
would then not be over the UI being inspected; the window API has not run on a real display yet, see
`docs/windows.md`).

## Editing live

Edits are inline-style edits applied through `Op::SetStyle`, so they are undoable as editor commands
(`history.ts`) and show at once. Persisting them (writing the change back into the CSS file or the TSX) is a later
step: for CSS, rules would need source ranges (above); for TSX, the compiler would have to map a host element to
its JSX site (the same data component live-editing needs).

"Change components live" builds on existing hot reload (`Runtime::refresh`, state kept per component id): the
inspector would show the component tree (the reconciler's instance tree, not just host nodes) and its props and
hook state. That needs `Runtime` introspection (component names are already compiled in as stable ids, signatures in
`__uiSignatures()`), kept behind the same flag.

## Order of work

See TODO.md "UI inspector". In short: tree queries (done) → `UiInspector` API and host publishing → overlay →
panel (rules, editing) → build flag and pbundle field → editor tab and tree selection → source locations → component tree.

## Risks

- Hover while the pointer is over the inspector panel itself: the panel must be skipped by `inspect_pick`
  (give it a marker attribute the pick ignores, not only `pointer-events`).
- World panels have their own `UiTree` and `Runtime`; tree ids are needed from the start.
- Edits and the reconciler: the reconciler only diffs the props it renders, so an inline edit stays until the
  component changes that same property; re-renders that do change it overwrite the edit, which is the expected
  rule for live designing.
