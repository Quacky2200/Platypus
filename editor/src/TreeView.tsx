// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// A tree view on the virtual list: only the rows in view are mounted, so hierarchies
// with hundreds of thousands of nodes stay fast. Selection, expansion and focus live
// in a `TreeState` the parent owns (tree.ts has the logic).

import { cx } from "./classes.ts";
import { clickSelect, type DropZone, dropPlace, flatten, type TreeNode, type TreeRow, type TreeState, toggle, treeKey } from "./tree.ts";
import { VirtualList, type VirtualListHandle } from "./VirtualList.tsx";

export interface TreeViewProps<T> {
  roots: readonly TreeNode<T>[];
  state: TreeState;
  onStateChange: (state: TreeState) => void;
  /** Visible height in UI pixels. */
  height: number;
  rowHeight?: number;
  /** Pixels per level of depth. */
  indent?: number;
  /** The icon slot, drawn before the label. */
  renderIcon?: (node: TreeNode<T>, row: TreeRow<T>) => ReactNode;
  /** Replaces the label (the default is the node's `label`). */
  renderLabel?: (node: TreeNode<T>, row: TreeRow<T>) => ReactNode;
  /** Double-click or Enter on a row. */
  onActivate?: (node: TreeNode<T>) => void;
  /** F2 (or a double-click when `onActivate` isn't set) starts renaming; called with the new name. */
  onRename?: (node: TreeNode<T>, name: string) => void;
  /** The rename slot: what to draw while renaming (`done(null)` cancels). Defaults to a text field. */
  renderRename?: (node: TreeNode<T>, done: (name: string | null) => void) => ReactNode;
  className?: string;
  /**
   * Set: rows can be dragged (carrying their id as this data type) and dropped onto other rows
   * (under them) or between rows (the top or bottom quarter of a row: before or after it, shown by
   * an insertion line).
   */
  dragType?: string;
  /** A row's id dropped: to go under `parent` (null: a root) at `index` among its children. */
  onMoveNode?: (dragged: string, parent: string | null, index: number) => void;
  /** Other drags a row takes (files, say). */
  acceptsDrop?: (e: PlatypusDragEvent) => boolean;
  /** One of those dropped onto a row. */
  onDropOn?: (e: PlatypusDragEvent, onto: string) => void;
}

/** The default rename field: Enter or leaving it renames, Escape cancels. */
function RenameField({ initial, done }: { initial: string; done: (name: string | null) => void }) {
  const field = useRef<PlatypusElement>(null);
  const [value, setValue] = useState(initial);
  const finished = useRef(false);
  const finish = (name: string | null) => {
    if (finished.current) return;
    finished.current = true;
    done(name);
  };
  useEffect(() => field.current?.focus(), []);
  return (
    <Input
      ref={field}
      className="platypus-tree-rename"
      value={value}
      onChange={(e) => setValue(e.value)}
      onKeyDown={(e: PlatypusKeyboardEvent) => {
        e.stopPropagation();
        if (e.key === "Enter") finish(value);
        else if (e.key === "Escape") finish(null);
      }}
      onBlur={() => finish(value)}
    />
  );
}

export function TreeView<T>({
  roots,
  state,
  onStateChange,
  height,
  rowHeight = 22,
  indent = 14,
  renderIcon,
  renderLabel,
  onActivate,
  onRename,
  renderRename,
  className,
  dragType,
  onMoveNode,
  acceptsDrop,
  onDropOn,
}: TreeViewProps<T>) {
  const rows = useMemo(() => flatten(roots, state.expanded), [roots, state.expanded]);
  const list = useRef<VirtualListHandle>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  // The row a drag is over (highlighted, or with a line before or after it).
  const [over, setOver] = useState<string | null>(null);
  const [zone, setZone] = useState<DropZone>("on");
  // A row of this tree being dragged: the rows show their before / after zones.
  const [dragging, setDragging] = useState(false);
  const focusIndex = state.focus === null ? -1 : rows.findIndex((r) => r.node.id === state.focus);

  const set = (next: TreeState) => {
    onStateChange(next);
    const at = next.focus === null ? -1 : flatten(roots, next.expanded).findIndex((r) => r.node.id === next.focus);
    if (at >= 0) list.current?.scrollToIndex(at);
  };
  const finishRename = (node: TreeNode<T>) => (name: string | null) => {
    setRenaming(null);
    if (name !== null && name !== node.label) onRename?.(node, name);
    list.current?.focus();
  };

  // `: ReactNode` written: a function returning an element isn't adapted where one returning a
  // `ReactNode` goes (tools/web_ui_ts/TODO.md, "A self- or forward-referencing closure").
  const renderRow = (i: number): ReactNode => {
    const row = rows[i]!;
    const { node } = row;
    const selected = state.selected.has(node.id);
    const lit = over === node.id;
    const classes = cx("platypus-tree-row", selected ? "selected" : "", state.focus === node.id ? "focused" : "", lit && zone === "on" ? "over" : "");
    const draggedId = (e: PlatypusDragEvent): string => (dragType ? e.dataTransfer.getData(dragType) : "");
    const place = lit && zone !== "on" ? dropPlace(roots, rows, i, zone) : null;
    /** Drag handlers for a part of the row: the row itself ("on") or its top or bottom quarter. */
    const target = (z: DropZone) => ({
      onDragOver: (e: PlatypusDragEvent) => {
        const from = draggedId(e);
        const other = !from && z === "on" && acceptsDrop !== undefined && acceptsDrop(e);
        if ((!from || (from === node.id && z === "on")) && !other) return;
        e.preventDefault();
        e.stopPropagation();
        e.dataTransfer.dropEffect = other ? "copy" : "move";
        setOver(node.id);
        setZone(z);
      },
      onDrop: (e: PlatypusDragEvent) => {
        const from = draggedId(e);
        setOver(null);
        setDragging(false);
        if (!from && z === "on" && acceptsDrop && acceptsDrop(e)) {
          e.stopPropagation();
          onDropOn?.(e, node.id);
          return;
        }
        if (!from) return;
        // Onto itself: nothing happens (not a drop on the list behind it either).
        e.stopPropagation();
        if (from === node.id && z === "on") return;
        const to = dropPlace(roots, rows, i, z);
        if (to) onMoveNode?.(from, to.parent, to.index);
      },
    });
    return (
      <box
        className={classes}
        style={{ paddingLeft: 4 + row.depth * indent, height: rowHeight }}
        draggable={dragType !== undefined && renaming !== node.id}
        onDragStart={(e: PlatypusDragEvent) => {
          if (!dragType) return;
          e.dataTransfer.setData(dragType, node.id);
          e.dataTransfer.effectAllowed = "move";
          setDragging(true);
        }}
        {...target("on")}
        onDragLeave={() => setOver(null)}
        onDragEnd={() => {
          setOver(null);
          setDragging(false);
        }}
        onPointerDown={(e: PlatypusPointerEvent) => {
          if (e.button !== 0) return;
          onStateChange(clickSelect(rows, state, node.id, { shift: e.shiftKey, ctrl: e.ctrlKey || e.metaKey }));
        }}
        onDoubleClick={() => {
          if (onActivate) onActivate(node);
          else if (onRename) setRenaming(node.id);
          else if (row.hasChildren) onStateChange(toggle(state, node.id));
        }}
      >
        <box
          className={row.hasChildren ? "platypus-tree-twisty" : "platypus-tree-twisty leaf"}
          onPointerDown={(e: PlatypusPointerEvent) => {
            if (!row.hasChildren) return;
            e.stopPropagation();
            onStateChange(toggle(state, node.id));
          }}
        >
          {row.hasChildren ? <box className={row.expanded ? "platypus-chevron open" : "platypus-chevron"} /> : null}
        </box>
        {renderIcon ? <box className="platypus-tree-icon">{renderIcon(node, row)}</box> : null}
        {renaming === node.id ? (
          renderRename ? (
            renderRename(node, finishRename(node))
          ) : (
            <RenameField initial={node.label} done={finishRename(node)} />
          )
        ) : renderLabel ? (
          renderLabel(node, row)
        ) : (
          <Text className="platypus-tree-label">{node.label}</Text>
        )}
        {dragging ? <box className="platypus-tree-zone before" {...target("before")} /> : null}
        {dragging ? <box className="platypus-tree-zone after" {...target("after")} /> : null}
        {place ? (
          <box className="platypus-tree-insert" style={{ left: 4 + place.depth * indent, top: zone === "before" ? -1 : rowHeight - 1 }} />
        ) : null}
      </box>
    );
  };

  return (
    <VirtualList
      ref={list}
      className={className ? `platypus-tree ${className}` : "platypus-tree"}
      count={rows.length}
      rowHeight={rowHeight}
      height={height}
      keyboard={false}
      renderRow={renderRow}
      onKeyDown={(e: PlatypusKeyboardEvent) => {
        if (renaming) return;
        const focused = rows[focusIndex];
        if (e.key === "F2" && focused && onRename) {
          e.preventDefault();
          setRenaming(focused.node.id);
          return;
        }
        if (e.key === "Enter" && focused) {
          e.preventDefault();
          onActivate?.(focused.node);
          return;
        }
        const next = treeKey(rows, state, e.key, { shift: e.shiftKey, ctrl: e.ctrlKey || e.metaKey }, Math.floor(height / rowHeight));
        if (!next) return;
        e.preventDefault();
        set(next);
      }}
    />
  );
}
