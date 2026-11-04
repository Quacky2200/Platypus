// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The Platypus editor's UI (M5.e): a menubar, docked panels (Hierarchy, Scene, Game,
// Inspector, Console, Project) and a status bar. `platypus-app editor` runs it like a game
// (`platypus-build build editor` first). The Hierarchy, Scene view and Inspector show and edit
// the editor's own world: the project's scene (scenes.ts), every edit undoable (history.ts).
import "./editor-ui.css";
import "./editor.css";

import { Console } from "./Console.tsx";
import { Inspector } from "./Inspector.tsx";
import { projectSettings } from "./ProjectSettings.tsx";
import { Dock, type DockPanel } from "./Dock.tsx";
import {
  type DockNode,
  group,
  groupOf,
  hasTab,
  openTab,
  parseLayout,
  type Rect as DockRect,
  removeTab,
  serializeLayout,
  split,
} from "./dock.ts";
import { getPref, preferencesDialog, setPref } from "./Preferences.tsx";
import { ALL_FILES, SCENE_FILTER, stemOf } from "./file-dialog.ts";
import { fileDialog } from "./FileDialog.tsx";
import { onEditorFrame } from "./frame.ts";
import {
  canRedo,
  canUndo,
  clearHistory,
  type Command,
  createHistory,
  group as groupCommands,
  isDirty,
  markSaved,
  perform,
  record,
  redo,
  redoLabel,
  undo,
  undoLabel,
} from "./history.ts";
import { confirmDialog, ModalHost } from "./Modal.tsx";
import { assetKind, importFolder, placeable } from "./placement.ts";
import { baseName, joinPath, OBJECT_TYPE, parentPath, PROJECT_PATH_TYPE } from "./project.ts";
import {
  clearRecent,
  forgetScene,
  newScene,
  openScene,
  placeGameCameras,
  recentScenes,
  saveScene,
  startScene,
  windowTitle,
} from "./scenes.ts";
import {
  makeCommand,
  noObject,
  placeCommand,
  copyObjects,
  deleteCommand,
  duplicateCommand,
  pasteCommand,
  poseCommand,
  renameCommand,
  reparentCommand,
  shownChildren,
} from "./scene-edit.ts";
import { type Menu, type MenuItem } from "./menu.ts";
import { fieldAccelerator, MenuBar } from "./MenuBar.tsx";
import { refsInto, repointOnUndo } from "./refs.ts";
import { ProjectBrowser } from "./ProjectBrowser.tsx";
import { SceneView, visibleObjects } from "./SceneView.tsx";
import { useScreenSize } from "./screen.ts";
import { emptyTreeState, type TreeNode, type TreeState } from "./tree.ts";
import { TreeView } from "./TreeView.tsx";
import { UiInspectorPanel } from "./UiInspectorPanel.tsx";
import { buildDemoScene } from "./demo-scene.ts";

const MENUBAR = 26;
const STATUS = 22;
const PANELS = ["hierarchy", "scene", "game", "inspector", "ui", "console", "project"];
const LAYOUT_KEY = "editor.layout";

// The scene's undo history and the objects copied (snapshots).
const history = createHistory();
let clipboard: string[] = [];

/** The scene as the title and prompts name it: its file, else its name. */
const sceneLabel = (): string => EditorScene.scenePath || EditorScene.sceneName || "Untitled";

const PRIMITIVES: { label: string; type: PrimitiveType }[] = [
  { label: "Cube", type: "cube" },
  { label: "Sphere", type: "sphere" },
  { label: "Capsule", type: "capsule" },
  { label: "Cylinder", type: "cylinder" },
  { label: "Plane", type: "plane" },
];
const LIGHTS: { label: string; type: "directional" | "point" | "spot" }[] = [
  { label: "Directional Light", type: "directional" },
  { label: "Point Light", type: "point" },
  { label: "Spot Light", type: "spot" },
];

const defaultLayout = (): DockNode =>
  split("root", "column", [
    split("top", "row", [group("left", ["hierarchy"]), group("center", ["scene", "game"]), group("right", ["inspector", "ui"], "inspector")], [0.2, 0.58, 0.22]),
    group("bottom", ["console", "project"]),
  ], [0.7, 0.3]);

function savedLayout(): DockNode {
  const saved = PlayerPrefs.getString(LAYOUT_KEY, "");
  return (saved && parseLayout(saved, PANELS)) || defaultLayout();
}

interface Thing {
  kind: "object" | "light" | "camera" | "mesh" | "audio" | "probe";
  object: GameObject;
}

function kindOf(go: GameObject): Thing["kind"] {
  if (go.getComponent(Light)) return "light";
  if (go.getComponent(Camera)) return "camera";
  if (go.getComponent(MeshRenderer)) return "mesh";
  if (go.getComponent(AudioSource)) return "audio";
  if (go.getComponent(ReflectionProbe)) return "probe";
  return "object";
}

/** The world's objects as tree nodes (hidden ones left out), keyed by object id. */
function sceneTree(): TreeNode<Thing>[] {
  const shown = new Set(visibleObjects());
  const node = (go: GameObject): TreeNode<Thing> => {
    const children = go.children.filter((c) => shown.has(c));
    return {
      id: String(go.id),
      label: go.name,
      data: { kind: kindOf(go), object: go },
      children: children.length > 0 ? children.map(node) : undefined,
    };
  };
  return GameObject.roots.filter((g) => shown.has(g)).map(node);
}

/** The live hierarchy: rebuilt when objects, names or parents change. */
function useSceneTree(): TreeNode<Thing>[] {
  const [roots, setRoots] = useState(sceneTree);
  useEffect(() => {
    let last = "";
    return onEditorFrame(() => {
      const key = visibleObjects()
        .map((g) => `${g.id}:${g.parent?.id ?? ""}:${g.name}`)
        .join("|");
      if (key !== last) {
        last = key;
        setRoots(sceneTree());
      }
    });
  }, []);
  return roots;
}

const ICONS: Record<Thing["kind"], string> = { object: "■", light: "☀", camera: "◉", mesh: "▲", audio: "♪", probe: "◍" };

// The "Save changes?" check the window's close box goes through (the App's, set each render).
let closeGuard = (then: () => void): void => then();

// Where `Camera.main` draws (fractions of the window): the Game panel's box while it's shown.
let gameRect: Rect | null = null;

/** The Game panel: the main camera draws into it while it's shown (scenes.ts keeps the others off). */
function GameView({ rect }: { rect: DockRect }) {
  useEffect(() => {
    const sw = Math.max(1, Screen.width);
    const sh = Math.max(1, Screen.height);
    gameRect = new Rect(rect.x / sw, rect.y / sh, rect.width / sw, rect.height / sh);
    return () => {
      gameRect = null;
    };
  }, [rect.x, rect.y, rect.width, rect.height]);
  return <box style={{ width: rect.width, height: rect.height }} />;
}


function App() {
  const screen = useScreenSize();
  const [layout, setLayout] = useState<DockNode>(savedLayout);
  const roots = useSceneTree();
  const [tree, setTree] = useState<TreeState>(() => ({
    ...emptyTreeState(),
    expanded: new Set(roots.filter((r) => r.children).map((r) => r.id)),
  }));
  const [status, setStatus] = useState("Ready");
  const [showGrid, setShowGrid] = useState(() => getPref("grid", true));
  const [showGizmos, setShowGizmos] = useState(() => getPref("gizmos", true));
  // Where the pointer was last pressed: "scene" (Hierarchy, Scene view) or "project".
  const area = useRef<string>("scene");

  const say = (text: string) => {
    setStatus(text);
    console.log(text);
  };
  const warn = (text: string) => {
    setStatus(text);
    console.warn(text);
  };
  // The game's cameras draw only in the Game panel (not from the top level: components can be
  // added once the program has started).
  useEffect(() => onEditorFrame(() => placeGameCameras(gameRect)), []);
  // The window's title follows the scene and unsaved changes.
  useEffect(() => {
    const title = windowTitle(sceneLabel(), isDirty(history), baseName(Editor.project));
    Window.main.title = title;
  });
  // The window's close box asks about unsaved changes too.
  useEffect(() => {
    Window.main.onCloseRequested(() => {
      if (!isDirty(history)) return;
      Window.main.cancelClose();
      closeGuard(() => Window.main.close());
    });
  }, []);
  const saveLayout = () => {
    PlayerPrefs.setString(LAYOUT_KEY, serializeLayout(layout));
    PlayerPrefs.save();
    say("Layout saved");
  };
  // Re-renders when the history changes (menu labels, the title's "*").
  const [, setHistoryVersion] = useState(0);
  const historyChanged = (): void => setHistoryVersion(history.version);
  /** Runs an edit as an undoable command. */
  const exec = (c: Command | null): void => {
    if (!c) return;
    perform(history, c);
    historyChanged();
    say(c.label);
  };
  /** Records an edit that already happened (a finished handle drag). */
  const keep = (c: Command): void => {
    record(history, c);
    historyChanged();
  };
  const clearSelection = (): void => setTree((t) => ({ ...t, selected: new Set<string>(), focus: null, anchor: null }));

  // Scene files.
  const recent = recentScenes();
  /** A new document: no history, nothing selected. */
  const opened = (message: string): void => {
    clearHistory(history);
    clearSelection();
    historyChanged();
    say(message);
  };
  const startNewScene = (): void => {
    if (newScene()) opened("New scene");
    else warn(`New scene: ${Editor.lastError}`);
  };
  const openPath = (path: string): void => {
    if (openScene(path)) opened(`Opened ${path}`);
    else {
      forgetScene(path);
      warn(`Can't open ${path}: ${Editor.lastError}`);
    }
  };
  const nothing = (): void => {};
  const saveTo = (path: string, then: () => void): void => {
    if (saveScene(path)) {
      markSaved(history);
      historyChanged();
      say(`Saved ${path}`);
      then();
    } else warn(`Can't save ${path}: ${Editor.lastError}`);
  };
  const saveAs = (then: () => void): void => {
    const current = EditorScene.scenePath;
    void fileDialog({
      mode: "save",
      title: "Save Scene As",
      filters: [SCENE_FILTER],
      dir: current ? parentPath(current) : "scenes",
      name: current ? stemOf(baseName(current), SCENE_FILTER) : EditorScene.sceneName,
    }).then((path) => {
      if (path) saveTo(path, then);
    });
  };
  const save = (then: () => void): void => {
    const current = EditorScene.scenePath;
    if (current) saveTo(current, then);
    else saveAs(then);
  };
  /** Asks to save unsaved changes first, then runs `then` (not when cancelled). */
  const guardUnsaved = (then: () => void): void => {
    if (!isDirty(history)) {
      then();
      return;
    }
    void confirmDialog({
      title: "Unsaved changes",
      message: `Save the changes to ${sceneLabel()}?`,
      buttons: ["Save", "Don't Save", "Cancel"],
      cancel: "Cancel",
    }).then((answer) => {
      if (answer === "Save") save(then);
      else if (answer === "Don't Save") then();
    });
  };
  closeGuard = guardUnsaved;
  /** Places a model or prefab file under `parent` (none: the root), at `point` when given; undoable. */
  const placeFile = (path: string, parent: GameObject | null, point: Vector3 | null): void => {
    const kind = assetKind(path);
    if (kind !== "model" && kind !== "prefab") return;
    exec(
      placeCommand(`Place ${baseName(path)}`, parent, (p) => {
        const go =
          kind === "prefab"
            ? EditorScene.placePrefab(path, p ?? noObject())
            : p
              ? Model.load(path).instantiate({ parent: p })
              : Model.load(path).instantiate();
        if (point && go.exists()) go.transform.position = point;
        return go;
      }),
    );
    if (Editor.lastError !== "") warn(`Can't place ${path}: ${Editor.lastError}`);
    // The new object is the last under its parent: select it, as Unity does.
    const kids = shownChildren(parent);
    const made = kids[kids.length - 1];
    if (made) select(made, false);
  };
  /** Whether a drag carries something to place: a project model or prefab, or such files from the system. */
  const acceptsFiles = (e: PlatypusDragEvent): boolean => {
    const path = e.dataTransfer.getData(PROJECT_PATH_TYPE);
    if (path) return placeable(path);
    return e.dataTransfer.files.some((f) => placeable(f.path));
  };
  /** Places what a drag carries; files from the system are copied into the project first (with their `.meta`). */
  const dropFiles = (e: PlatypusDragEvent, parent: GameObject | null, point: Vector3 | null): void => {
    const path = e.dataTransfer.getData(PROJECT_PATH_TYPE);
    if (path) {
      placeFile(path, parent, point);
      return;
    }
    for (const f of e.dataTransfer.files) {
      if (!placeable(f.path)) continue;
      const dir = importFolder(f.path);
      if (!Editor.exists(dir)) Editor.createDir(dir);
      const made = Editor.copyIn([f.path], dir);
      if (made.length === 0) warn(`Can't import ${f.path}: ${Editor.lastError}`);
      for (const p of made) {
        say(`Imported ${p}`);
        placeFile(p, parent, point);
      }
    }
  };
  /** A preview of the model or prefab a Project drag carries, hidden and never saved (no undo: it goes when the drag ends). */
  const makePreview = (e: PlatypusDragEvent): GameObject | null => {
    const path = e.dataTransfer.getData(PROJECT_PATH_TYPE);
    const kind = assetKind(path);
    if (kind !== "model" && kind !== "prefab") return null;
    const go = kind === "prefab" ? EditorScene.placePrefab(path, noObject()) : Model.load(path).instantiate();
    if (!go.exists()) return null;
    const hide = (o: GameObject): void => {
      o.hideFlags = HideFlags.HideAndDontSave;
      for (const c of o.children) hide(c);
    };
    hide(go);
    go.name = `${go.name} (preview)`;
    return go;
  };
  /** Saves an object as `<dir>/<name>.prefab.json` (a free name); the file's path, "" when it failed. */
  const savePrefabOf = (go: GameObject, dir: string): string => {
    if (!go.exists()) return "";
    const path = Editor.freeName(dir, `${go.name}.prefab.json`);
    if (!path || !EditorScene.savePrefab(go, path)) return "";
    Editor.ensureMeta(path);
    return path;
  };
  /** A project file opened from the Project browser: a scene opens, a model or prefab is placed, others open in the code editor. */
  const openFile = (path: string): void => {
    const kind = assetKind(path);
    if (kind === "scene") guardUnsaved(() => openPath(path));
    else if (kind === "model" || kind === "prefab") placeFile(path, null, null);
    else Editor.openFile(joinPath(Editor.project, path), 1, 1);
  };
  /** A new object at the root, made by `make` (and again on redo), selected. */
  const create = (label: string, make: () => GameObject): void => {
    const c = makeCommand(`Create ${label}`, make);
    exec(c);
    const made = shownChildren(null);
    const last = made[made.length - 1];
    if (last) select(last, false);
  };

  const selectedIds = [...tree.selected];
  const byId = new Map(visibleObjects().map((g) => [String(g.id), g]));
  const selectedObjects = selectedIds.map((id) => byId.get(id)).filter((g): g is GameObject => !!g && !g.destroyed);
  // Edits of objects only while the Hierarchy or Scene view was used last (the Project browser has its own keys).
  const onScene = (): boolean => area.current === "scene";
  const deleteSelected = (): void => {
    if (!onScene()) return;
    const c = deleteCommand(selectedObjects);
    if (!c) return;
    clearSelection();
    // Undo builds the objects again with new ids: point other objects' references back at them.
    exec(groupCommands(c.label, [repointOnUndo(refsInto(selectedObjects)), c]));
  };
  const copySelected = (): void => {
    if (!onScene()) return;
    clipboard = copyObjects(selectedObjects);
    say(`Copied ${clipboard.length} object${clipboard.length === 1 ? "" : "s"}`);
  };
  const editMenu = (): MenuItem[] => [
    {
      label: canUndo(history) ? `Undo ${undoLabel(history)}` : "Undo",
      accelerator: "Ctrl+Z",
      disabled: !canUndo(history),
      action: () => {
        const label = undo(history);
        historyChanged();
        if (label) say(`Undid ${label}`);
      },
    },
    {
      label: canRedo(history) ? `Redo ${redoLabel(history)}` : "Redo",
      accelerator: "Ctrl+Y",
      disabled: !canRedo(history),
      action: () => {
        const label = redo(history);
        historyChanged();
        if (label) say(`Redid ${label}`);
      },
    },
    { separator: true },
    {
      label: "Cut",
      accelerator: "Ctrl+X",
      disabled: selectedIds.length === 0,
      action: () => {
        copySelected();
        deleteSelected();
      },
    },
    { label: "Copy", accelerator: "Ctrl+C", disabled: selectedIds.length === 0, action: copySelected },
    {
      label: "Paste",
      accelerator: "Ctrl+V",
      disabled: clipboard.length === 0,
      action: () => {
        // Beside the selected object (under its parent), else at the root.
        const at = selectedObjects[selectedObjects.length - 1];
        if (onScene()) exec(pasteCommand(clipboard, at ? at.parent : null));
      },
    },
    {
      label: "Duplicate",
      accelerator: "Ctrl+D",
      disabled: selectedIds.length === 0,
      action: () => {
        if (onScene()) exec(duplicateCommand(selectedObjects));
      },
    },
    { label: "Delete", accelerator: "Delete", disabled: selectedIds.length === 0, action: deleteSelected },
    { separator: true },
    {
      label: "Select All",
      accelerator: "Ctrl+A",
      action: () => {
        if (!onScene()) return;
        const all = visibleObjects().map((g) => String(g.id));
        setTree((t) => ({ ...t, selected: new Set(all) }));
      },
    },
    { separator: true },
    { label: "Project Settings...", action: projectSettings },
    {
      label: "Preferences...",
      action: () =>
        preferencesDialog((name, value) => {
          if (name === "grid") setShowGrid(value);
          else if (name === "gizmos") setShowGizmos(value);
        }),
    },
  ];
  const select = (go: GameObject | null, additive: boolean) => {
    if (!go) {
      if (!additive) setTree((t) => ({ ...t, selected: new Set() }));
      return;
    }
    const id = String(go.id);
    setTree((t) => {
      const selected = new Set(additive ? t.selected : []);
      if (additive && selected.has(id)) selected.delete(id);
      else selected.add(id);
      return { ...t, selected, focus: id, anchor: id };
    });
  };
  const menus: Menu[] = [
    {
      label: "File",
      items: [
        { label: "New Scene", accelerator: "Ctrl+N", action: () => guardUnsaved(startNewScene) },
        {
          label: "Open Scene...",
          accelerator: "Ctrl+O",
          action: () =>
            guardUnsaved(() => {
              void fileDialog({ mode: "open", title: "Open Scene", filters: [SCENE_FILTER, ALL_FILES], dir: "scenes" }).then((path) => {
                if (path) openPath(path);
              });
            }),
        },
        {
          label: "Open Recent",
          submenu: [
            ...recent.map(
              (path): MenuItem => ({
                label: path,
                action: () => guardUnsaved(() => openPath(path)),
              }),
            ),
            { separator: true },
            {
              label: "Clear Recent",
              disabled: recent.length === 0,
              action: () => {
                clearRecent();
                say("Recent scenes cleared");
              },
            },
          ],
        },
        { separator: true },
        { label: "Save", accelerator: "Ctrl+S", action: () => save(nothing) },
        { label: "Save As...", accelerator: "Ctrl+Shift+S", action: () => saveAs(nothing) },
        { separator: true },
        { label: "Exit", accelerator: "Alt+F4", action: () => guardUnsaved(() => Application.quit()) },
      ],
    },
    { label: "Edit", items: editMenu() },
    {
      label: "GameObject",
      items: [
        { label: "Create Empty", accelerator: "Ctrl+Shift+N", action: () => create("GameObject", () => new GameObject("GameObject")) },
        {
          label: "3D Object",
          submenu: PRIMITIVES.map(
            (p): MenuItem => ({
              label: p.label,
              action: () => create(p.label, () => GameObject.createPrimitive(p.type, { name: p.label })),
            }),
          ),
        },
        {
          label: "Light",
          submenu: LIGHTS.map(
            (l): MenuItem => ({
              label: l.label,
              action: () =>
                create(l.label, () => {
                  const go = new GameObject(l.label, { position: new Vector3(0, 3, 0), rotation: Quaternion.euler(50, -30, 0) });
                  go.addComponent(Light, { type: l.type });
                  return go;
                }),
            }),
          ),
        },
        {
          label: "Camera",
          action: () =>
            create("Camera", () => {
              const go = new GameObject("Camera", { position: new Vector3(0, 1, -10) });
              go.addComponent(Camera);
              return go;
            }),
        },
      ],
    },
    {
      label: "View",
      items: [
        {
          label: "Panels",
          // Checked: in the layout. Choosing one shows it, or closes it when it is the active tab.
          submenu: PANELS.map(
            (id): MenuItem => ({
              label: TITLES[id],
              checked: hasTab(layout, id),
              action: () => {
                const open = hasTab(layout, id);
                setLayout(open && groupOf(layout, id)?.active === id ? removeTab(layout, id) : openTab(layout, id));
              },
            }),
          ),
        },
        { separator: true },
        {
          label: "Show Grid",
          checked: showGrid,
          action: () => {
            setPref("grid", !showGrid);
            setShowGrid(!showGrid);
          },
        },
        {
          label: "Show Gizmos",
          checked: showGizmos,
          action: () => {
            setPref("gizmos", !showGizmos);
            setShowGizmos(!showGizmos);
          },
        },
      ],
    },
    {
      label: "Window",
      items: [
        { label: "Save Layout", action: saveLayout },
        {
          label: "Load Layout",
          action: () => {
            setLayout(savedLayout());
            say("Layout loaded");
          },
        },
        {
          label: "Reset Layout",
          action: () => {
            setLayout(defaultLayout());
            say("Layout reset");
          },
        },
        { separator: true },
        { label: "Log Layout JSON", action: () => console.log(serializeLayout(layout)) },
      ],
    },
    {
      label: "Help",
      items: [
        {
          label: "Documentation",
          accelerator: "F1",
          action: () => {
            // The editor's guide, in the code editor (`Editor.openFile` looks in the source tree).
            Editor.openFile("docs/editor.md", 1, 1);
            say("Opened docs/editor.md");
          },
        },
        {
          label: "About Platypus",
          action: () =>
            void confirmDialog({
              title: "About Platypus",
              message: `${Application.productName} ${Application.version}\nAn engine with a Rust core and TypeScript gameplay compiled to native code.\nProject: ${Editor.project}`,
              buttons: ["Close"],
              cancel: "Close",
            }),
        },
      ],
    },
  ];

  const panels: Record<string, DockPanel> = useMemo(
    () => ({
      hierarchy: {
        title: TITLES.hierarchy!,
        render: ({ width, height }) => (
          <box
            style={{ width, height, flexDirection: "column" }}
            onPointerDown={() => {
              area.current = "scene";
            }}
            onDragOver={(e: PlatypusDragEvent) => {
              // Below the rows: back to the root (an object), or placed at the root (a file).
              const files = acceptsFiles(e);
              if (!e.dataTransfer.getData(OBJECT_TYPE) && !files) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = files ? "copy" : "move";
            }}
            onDrop={(e: PlatypusDragEvent) => {
              if (acceptsFiles(e)) {
                dropFiles(e, null, null);
                return;
              }
              const go = GameObject.byId(Number(e.dataTransfer.getData(OBJECT_TYPE)));
              if (go.exists()) exec(reparentCommand(go, null, shownChildren(null).length));
            }}
          >
            <TreeView
              roots={roots}
              state={tree}
              onStateChange={setTree}
              height={height}
              dragType={OBJECT_TYPE}
              onMoveNode={(dragged, parentId, index) => {
                const go = GameObject.byId(Number(dragged));
                const parent = parentId === null ? null : GameObject.byId(Number(parentId));
                if (go.exists() && (!parent || parent.exists())) exec(reparentCommand(go, parent, index));
              }}
              acceptsDrop={acceptsFiles}
              onDropOn={(e, onto) => {
                const parent = GameObject.byId(Number(onto));
                dropFiles(e, parent.exists() ? parent : null, null);
              }}
              renderIcon={(node) => <Text className={`icon ${node.data?.kind ?? ""}`}>{ICONS[node.data?.kind ?? "object"]}</Text>}
              onRename={(node, name) => {
                if (node.data) exec(renameCommand(node.data.object, name));
              }}
            />
          </box>
        ),
      },
      project: {
        title: TITLES.project!,
        render: (s) => (
          <box
            style={{ width: s.width, height: s.height, flexDirection: "column" }}
            onPointerDown={() => {
              area.current = "project";
            }}
          >
            <ProjectBrowser
              {...s}
              onTrashed={(path, id) => {
                // Undo puts it back; redo deletes it again (a new trash id each time).
                let current = id;
                keep({
                  label: `Delete ${path}`,
                  run: () => {
                    current = Editor.trash(path);
                  },
                  undo: () => {
                    Editor.restore(current);
                  },
                });
              }}
              onDropObject={(id, dir) => savePrefabOf(GameObject.byId(Number(id)), dir)}
              onOpenFile={openFile}
            />
          </box>
        ),
      },
      scene: {
        title: TITLES.scene!,
        seeThrough: true,
        render: (r) => (
          <box
            style={{ width: r.width, height: r.height, flexDirection: "column" }}
            onPointerDown={() => {
              area.current = "scene";
            }}
          >
            <SceneView
              rect={r}
              selected={selectedObjects}
              onSelect={select}
              grid={showGrid}
              gizmos={showGizmos}
              onTransformed={(go, before, after) => keep(poseCommand(go, before, after))}
              acceptsDrop={acceptsFiles}
              onDropAt={(e, point) => dropFiles(e, null, point)}
              makePreview={makePreview}
            />
          </box>
        ),
      },
      game: { title: TITLES.game!, seeThrough: true, render: (r) => <GameView rect={r} /> },
      inspector: {
        title: TITLES.inspector!,
        render: (s) => <Inspector objects={selectedObjects} width={s.width} height={s.height} exec={exec} />,
      },
      ui: { title: TITLES.ui!, render: (s) => <UiInspectorPanel width={s.width} height={s.height} /> },
      console: { title: TITLES.console!, render: (s) => <Console {...s} /> },
    }),
    [roots, tree, showGrid, showGizmos],
  );

  const dockRect = { x: 0, y: MENUBAR, width: screen.width, height: Math.max(0, screen.height - MENUBAR - STATUS) };
  return (
    <box className="editor" onKeyDown={(e: PlatypusKeyboardEvent) => fieldAccelerator(menus, e)}>
      <MenuBar menus={menus} />
      <Dock layout={layout} onLayoutChange={setLayout} panels={panels} rect={dockRect} />
      <box className="statusbar">
        <Text>{status}</Text>
        <box style={{ flexGrow: 1 }} />
        <Text>{`${tree.selected.size} selected`}</Text>
      </box>
      <ModalHost />
    </box>
  );
}

const TITLES: Record<string, string> = {
  hierarchy: "Hierarchy",
  project: "Project",
  scene: "Scene",
  game: "Game",
  inspector: "Inspector",
  ui: "UI",
  console: "Console",
};

// The scene this project had open last, else its start scene, else a demo to look at.
const first = Editor.isAvailable ? startScene(Editor.startScene, getPref("reopenLastScene", true)) : "";
if (!first || !openScene(first)) buildDemoScene();
createRoot().render(<App />);
console.log("Platypus editor started (UI: editor/src/main.tsx:1)");
if (first) console.log(`Opened ${first}`);
