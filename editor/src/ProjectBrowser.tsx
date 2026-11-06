// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The project browser: the project folder's files, navigable by folder. Files dragged
// in from the system are copied into the folder they're dropped on (or the one shown);
// items dragged inside it move into a folder (a row, a breadcrumb, or `..`). New Folder,
// Rename (F2) and Delete (Del, into the project's trash) work on the selected item.

import { cx } from "./classes.ts";
import { confirmDialog, textDialog } from "./Modal.tsx";
import {
  baseName,
  canMoveInto,
  copyIn,
  crumbs,
  formatSize,
  joinPath,
  listDir,
  moveInto,
  OBJECT_TYPE,
  PROJECT_PATH_TYPE,
  parentPath,
  projectFolder,
} from "./project.ts";
import { VirtualList } from "./VirtualList.tsx";

export interface ProjectBrowserProps {
  width: number;
  height: number;
  rowHeight?: number;
  /** A file double-clicked or opened with Enter (folders open in the browser). */
  onOpenFile?: (path: string) => void;
  /** An item went to the trash (`id` for `Editor.restore`), so the editor can undo it. */
  onTrashed?: (path: string, id: string) => void;
  /** A scene object (its id) dropped on a folder: save it there as a prefab; the new file's path ("" when it failed). */
  onDropObject?: (objectId: string, dir: string) => string;
}

const TOOLBAR = 28;

/** What a drag over a folder would do there, if anything (`prefab`: a scene object saved as one). */
function dropAction(e: PlatypusDragEvent, dir: string): "copy" | "move" | "prefab" | null {
  if (e.dataTransfer.files.length) return "copy";
  if (e.dataTransfer.getData(OBJECT_TYPE)) return "prefab";
  const from = e.dataTransfer.getData(PROJECT_PATH_TYPE);
  return from && canMoveInto(from, dir) ? "move" : null;
}

export function ProjectBrowser({ width, height, rowHeight = 22, onOpenFile, onTrashed, onDropObject }: ProjectBrowserProps) {
  const [path, setPath] = useState("");
  const [entries, setEntries] = useState<ProjectEntry[]>([]);
  const [selected, setSelected] = useState(-1);
  const [status, setStatus] = useState("");
  // The folder a drop would go into, highlighted while dragging over it.
  const [over, setOver] = useState<string | null>(null);
  const root = projectFolder();
  const rootName = root ? baseName(root) : "Project";
  // `..` first when inside a folder, told apart by its place: a `(ProjectEntry & { up?: true })[]`
  // isn't converted element by element yet (tools/web_ui_ts/TODO.md, "An array of `A`").
  const rows: ProjectEntry[] = path ? [{ name: "..", dir: true, size: 0 }, ...entries] : entries;
  const isUp = (i: number) => path !== "" && i === 0;

  const refresh = useCallback(
    (dir = path) =>
      listDir(dir).then(
        (l) => {
          setEntries(l.entries);
          setStatus(`${l.entries.length} items`);
        },
        (e: Error) => setStatus(e.message),
      ),
    [path],
  );
  useEffect(() => {
    if (root) void refresh(path);
    else setStatus("No project folder (run platypus-app editor --project <folder>)");
  }, [path, root]);

  const open = (dir: string) => {
    setSelected(-1);
    setPath(dir);
  };
  const report = (message: string, ok: boolean): void => {
    setStatus(message);
    if (ok) console.log(message);
    else console.warn(`Project: ${message}`);
    void refresh();
  };
  const newFolder = (): void => {
    void textDialog({ title: "New Folder", label: "Folder name", initial: "New Folder", ok: "Create" }).then((name) => {
      if (name === "") return;
      const made = Editor.createDir(joinPath(path, name));
      report(made ? `Made ${made}` : Editor.lastError, made !== "");
    });
  };
  /** The selected item's project path ("" for none or `..`). */
  const selectedPath = (): string => {
    const row = rows[selected];
    return row && !isUp(selected) ? joinPath(path, row.name) : "";
  };
  const rename = (): void => {
    const from = selectedPath();
    if (!from) return;
    const old = baseName(from);
    void textDialog({ title: "Rename", label: `New name for ${old}`, initial: old, ok: "Rename" }).then((name) => {
      if (name === "" || name === old) return;
      const to = Editor.rename(from, name);
      report(to ? `Renamed to ${to}` : Editor.lastError, to !== "");
    });
  };
  const remove = (): void => {
    const target = selectedPath();
    if (!target) return;
    void confirmDialog({
      title: "Delete",
      message: `Delete ${target}? It goes to the project's trash (.platypus/trash); Undo puts it back.`,
      buttons: ["Delete", "Cancel"],
      cancel: "Cancel",
    }).then((answer) => {
      if (answer !== "Delete") return;
      const id = Editor.trash(target);
      if (id) {
        setSelected(-1);
        onTrashed?.(target, id);
      }
      report(id ? `Deleted ${target}` : Editor.lastError, id !== "");
    });
  };

  /** Drag handlers that make `dir` a drop target. */
  const target = (dir: string) => ({
    onDragOver: (e: PlatypusDragEvent) => {
      const action = dropAction(e, dir);
      if (!action || (action === "prefab" && !onDropObject)) return;
      e.preventDefault();
      // The innermost folder wins over the list behind it.
      e.stopPropagation();
      e.dataTransfer.dropEffect = action === "prefab" ? "copy" : action;
      setOver(dir);
    },
    onDrop: (e: PlatypusDragEvent) => {
      const action = dropAction(e, dir);
      if (!action) return;
      e.stopPropagation();
      setOver(null);
      if (action === "prefab") {
        const made = onDropObject ? onDropObject(e.dataTransfer.getData(OBJECT_TYPE), dir) : "";
        report(made ? `Saved prefab ${made}` : `Prefab not saved: ${Editor.lastError}`, made !== "");
        return;
      }
      const done =
        action === "copy"
          ? copyIn(
              e.dataTransfer.files.map((f) => f.path),
              dir,
            ).then((r) => `Copied ${r.created.length} item${r.created.length === 1 ? "" : "s"} into ${dir || rootName}`)
          : moveInto(e.dataTransfer.getData(PROJECT_PATH_TYPE), dir).then((r) => `Moved to ${r.path}`);
      done.then(
        (message) => {
          setStatus(message);
          console.log(message);
          void refresh();
        },
        (err: Error) => {
          setStatus(err.message);
          console.warn(`Project: ${err.message}`);
        },
      );
    },
  });

  const listHeight = Math.max(0, height - TOOLBAR);

  return (
    <box
      className="platypus-project"
      style={{ width, height }}
      onDragLeave={() => setOver(null)}
      onDragEnd={() => setOver(null)}
      {...target(path)}
    >
      <box className="platypus-project-toolbar" style={{ height: TOOLBAR }}>
        {crumbs(path, rootName).map((c, i) => (
          <box key={c.path} className={over === c.path && c.path !== path ? "platypus-project-crumb over" : "platypus-project-crumb"} onClick={() => open(c.path)} {...target(c.path)}>
            <Text>{i === 0 ? c.name : `/ ${c.name}`}</Text>
          </box>
        ))}
        <box style={{ flexGrow: 1 }} />
        <Text className="platypus-project-status">{status}</Text>
        <box className="platypus-project-button" onClick={newFolder}>
          <Text>New Folder</Text>
        </box>
        <box className="platypus-project-button" onClick={rename}>
          <Text>Rename</Text>
        </box>
        <box className="platypus-project-button" onClick={remove}>
          <Text>Delete</Text>
        </box>
        <box className="platypus-project-button" onClick={() => void refresh()}>
          <Text>Refresh</Text>
        </box>
      </box>
      <VirtualList
        className={over === path ? "platypus-project-list over" : "platypus-project-list"}
        count={rows.length}
        rowHeight={rowHeight}
        height={listHeight}
        selected={selected}
        onSelect={setSelected}
        empty={root ? "Empty folder: drop files here" : ""}
        onKeyDown={(e) => {
          const row = rows[selected];
          if (e.key === "Enter" && row?.dir) open(isUp(selected) ? parentPath(path) : joinPath(path, row.name));
          else if (e.key === "Enter" && row) onOpenFile?.(joinPath(path, row.name));
          else if (e.key === "Backspace" && path) open(parentPath(path));
          else if (e.key === "F2") rename();
          else if (e.key === "Delete") remove();
          else return;
          e.preventDefault();
        }}
        renderRow={(i) => {
          const row = rows[i]!;
          const full = isUp(i) ? parentPath(path) : joinPath(path, row.name);
          const classes = cx("platypus-project-row", row.dir ? "dir" : "", row.dir && over === full ? "over" : "");
          return (
            <box
              className={classes}
              draggable={!isUp(i)}
              onDragStart={(e: PlatypusDragEvent) => {
                e.dataTransfer.setData(PROJECT_PATH_TYPE, full);
                e.dataTransfer.setData("text/plain", full);
                e.dataTransfer.effectAllowed = "move";
              }}
              onDoubleClick={() => (row.dir ? open(full) : onOpenFile?.(full))}
              // Folders take drops; files don't.
              {...(row.dir ? target(full) : {})}
            >
              <box className={row.dir ? "platypus-project-icon folder" : "platypus-project-icon file"} />
              <Text className="platypus-project-name">{row.name}</Text>
              <Text className="platypus-project-size">{row.dir ? "" : formatSize(row.size)}</Text>
            </box>
          );
        }}
      />
    </box>
  );
}
