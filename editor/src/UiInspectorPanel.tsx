// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The "UI" panel: the elements of the editor's own screen UI as a tree, and the styles of the
// node picked in it (the inline styles and the matching CSS rules, editable as inline overrides).
// It needs the editor built with `platypus-build build editor --add-ui-inspector`: that makes the
// host answer `UiInspector`. Picking a row pins the node, which the inspector overlay (F12) then
// outlines. Design: docs/research/platypus-ui-inspector.md

import { UiInspDocLine, UiInspName, UiInspRoot, UiInspStyles, UiInspThemeChip } from "../../tools/platypus_build/inspector/shared.tsx";
import { emptyTreeState, type TreeNode, type TreeState } from "./tree.ts";
import { TreeView } from "./TreeView.tsx";

export interface UiInspectorPanelProps {
  width: number;
  height: number;
}

/** How often the tree and styles are read again, in milliseconds. */
const REFRESH = 100;
/** The tree's share of the panel's height. */
const TREE_SHARE = 0.5;
const HEADER = 26;

/** A tree row for node `id` and, where it is open, the ones below it the host has published. */
function nodeOf(id: number, open: ReadonlySet<string>): TreeNode<number> {
  const node: TreeNode<number> = { id: String(id), label: id === 0 ? "root" : UiInspName(id), data: id };
  const count = UiInspector.childCount(id);
  if (count === 0) return node;
  if (!open.has(node.id)) {
    // Closed: a stand-in row so the row shows its arrow. The host publishes the children once it is open.
    node.children = [{ id: `${id}:more`, label: "…", data: 0 }];
    return node;
  }
  const kids: TreeNode<number>[] = [];
  for (let i = 0; i < count; i++) kids.push(nodeOf(UiInspector.childAt(id, i), open));
  node.children = kids;
  return node;
}

export function UiInspectorPanel({ width, height }: UiInspectorPanelProps) {
  const on = UiInspector.enabled();
  const [tree, setTree] = useState<TreeState>(() => ({ ...emptyTreeState(), expanded: new Set(["0"]) }));
  const [edit, setEdit] = useState(false);
  const [theme, setTheme] = useState("");
  const [tick, setTick] = useState(0);

  // The host publishes what is asked for: the root, then each open row.
  useEffect(() => {
    if (!on) return undefined;
    UiInspector.expand(0, true);
    const h = setInterval(() => setTick((t) => t + 1), REFRESH);
    return () => clearInterval(h);
  }, [on]);

  // A node pinned elsewhere (F9, a click in pick mode) is shown in the tree: its ancestors open
  // and its row is the focused one.
  const shown = useRef(0);
  useEffect(() => {
    const sel = UiInspector.selected();
    if (!on || sel === shown.current) return;
    shown.current = sel;
    if (sel === 0 || tree.focus === String(sel)) return;
    const open = new Set(tree.expanded);
    open.add("0");
    let at = UiInspector.parent(sel);
    // The chain is as deep as the UI; the cap keeps a bad answer from looping.
    for (let n = 0; at !== 0 && n < 256; n++) {
      open.add(String(at));
      at = UiInspector.parent(at);
    }
    for (const id of open) if (!tree.expanded.has(id)) UiInspector.expand(Number(id), true);
    setTree({ ...tree, expanded: open, selected: new Set([String(sel)]), focus: String(sel), anchor: String(sel) });
  }, [on, tick]);

  const roots = useMemo(() => (on ? [nodeOf(0, tree.expanded)] : []), [on, tick, tree.expanded]);

  const change = (next: TreeState): void => {
    for (const id of next.expanded) if (!tree.expanded.has(id)) UiInspector.expand(Number(id), true);
    for (const id of tree.expanded) if (!next.expanded.has(id)) UiInspector.expand(Number(id), false);
    const row = next.focus;
    if (row !== null && row !== tree.focus && !row.endsWith(":more")) UiInspector.select(Number(row));
    setTree(next);
  };

  if (!on) {
    return (
      <box style={{ width, height, padding: 12, flexDirection: "column", gap: 6 }}>
        <Text>The UI inspector is off.</Text>
        <Text>Build the editor with `platypus-build build editor --add-ui-inspector` to inspect its UI.</Text>
      </box>
    );
  }

  const selected = UiInspector.selected();
  const treeHeight = Math.max(60, Math.floor((height - HEADER) * TREE_SHARE));
  return (
    <UiInspRoot theme={theme} style={{ width, height, flexDirection: "column" }}>
      <box style={{ height: HEADER, flexDirection: "row", alignItems: "center", gap: 8, padding: "0 8px" }}>
        <Text>{selected === 0 ? "Pick a row, or press F12 then Inspect" : UiInspName(selected)}</Text>
        <box style={{ flexGrow: 1 }} />
        <UiInspThemeChip theme={theme} onTheme={setTheme} />
        <box
          style={{ cursor: "pointer", padding: "1px 8px", borderRadius: 3, backgroundColor: edit ? "rgba(80, 160, 255, 0.35)" : "rgba(255, 255, 255, 0.08)" }}
          onClick={() => setEdit(!edit)}
        >
          <Text>{edit ? "Editing" : "Edit"}</Text>
        </box>
      </box>
      <TreeView
        roots={roots}
        state={tree}
        onStateChange={change}
        height={treeHeight}
        // A row outlines its element (marching ants over the editor's own UI) while the pointer is on it.
        renderLabel={(node) => (
          <box
            onMouseEnter={() => {
              if (!node.id.endsWith(":more")) UiInspector.highlightNode(Number(node.id));
            }}
            onMouseLeave={() => UiInspector.clearHighlight()}
          >
            <Text className="platypus-tree-label">{node.label}</Text>
          </box>
        )}
      />
      <box className="uii-card" style={{ flexGrow: 1, margin: 6, padding: 8, overflow: "auto", flexDirection: "column" }}>
        {selected !== 0 ? <UiInspStyles id={selected} edit={edit} /> : null}
      </box>
      <UiInspDocLine />
    </UiInspRoot>
  );
}
