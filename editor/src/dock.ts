// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The docking layout as data: splits (rows or columns of children with size fractions)
// and tab groups at the leaves. Every change returns a new layout; the layout saves to
// JSON and loads back. Rects are computed here too, the same way the flex layout in
// Dock.tsx places things, so drops and splitter drags need no measurements from the engine.

export type DockZone = "left" | "right" | "top" | "bottom" | "center";

/** Tabs shown together; one is active. */
export interface DockGroup {
  type: "group";
  id: string;
  tabs: string[];
  active: string | null;
}

/** Children side by side (`row`) or stacked (`column`); `sizes` are fractions that add up to 1. */
export interface DockSplit {
  type: "split";
  id: string;
  direction: "row" | "column";
  children: DockNode[];
  sizes: number[];
}

export type DockNode = DockGroup | DockSplit;

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Splitter thickness between split children, in UI pixels. */
export const SPLITTER = 4;
/** Height of a group's tab strip. */
export const TAB_BAR = 26;
/** The smallest a split child may be made by dragging, as a fraction. */
export const MIN_FRACTION = 0.05;

export const group = (id: string, tabs: string[], active: string | null = tabs[0] ?? null): DockGroup => ({
  type: "group",
  id,
  tabs,
  active,
});

export const split = (id: string, direction: "row" | "column", children: DockNode[], sizes?: number[]): DockSplit => ({
  type: "split",
  id,
  direction,
  children,
  sizes: normalize(sizes ?? children.map(() => 1), children.length),
});

function normalize(sizes: readonly number[], n: number): number[] {
  const s = Array.from({ length: n }, (_, i) => {
    const v = sizes[i];
    return typeof v === "number" && Number.isFinite(v) && v > 0 ? v : 1 / Math.max(1, n);
  });
  const total = s.reduce((a, b) => a + b, 0);
  return s.map((v) => v / total);
}

/** Every node, depth first. */
export function* walk(node: DockNode): Generator<DockNode> {
  yield node;
  if (node.type === "split") for (const c of node.children) yield* walk(c);
}

export const groups = (layout: DockNode): DockGroup[] => [...walk(layout)].filter((n): n is DockGroup => n.type === "group");

/** The group holding a tab. */
export const groupOf = (layout: DockNode, tab: string): DockGroup | null => groups(layout).find((g) => g.tabs.includes(tab)) ?? null;

export const findNode = (layout: DockNode, id: string): DockNode | null => [...walk(layout)].find((n) => n.id === id) ?? null;

/** An id not used in the layout yet: `prefix1`, `prefix2`, ... */
function freshId(layout: DockNode, prefix: string): string {
  const used = new Set([...walk(layout)].map((n) => n.id));
  for (let i = 1; ; i++) if (!used.has(`${prefix}${i}`)) return `${prefix}${i}`;
}

/** Copies the tree, replacing nodes `fn` changes (`fn` returns the node, a replacement or null to drop it). */
function rebuild(node: DockNode, fn: (n: DockNode) => DockNode | null): DockNode | null {
  const mapped = fn(node);
  if (!mapped || mapped.type === "group") return mapped;
  const kept: DockNode[] = [];
  const sizes: number[] = [];
  mapped.children.forEach((c, i) => {
    const r = rebuild(c, fn);
    if (r) {
      kept.push(r);
      sizes.push(mapped.sizes[i] ?? 0);
    }
  });
  if (kept.length === 0) return null;
  // A split of one is just that child.
  if (kept.length === 1) return kept[0]!;
  // A child split in the same direction merges into its parent.
  const children: DockNode[] = [];
  const merged: number[] = [];
  kept.forEach((c, i) => {
    if (c.type === "split" && c.direction === mapped.direction) {
      c.children.forEach((cc, j) => {
        children.push(cc);
        merged.push(sizes[i]! * c.sizes[j]!);
      });
    } else {
      children.push(c);
      merged.push(sizes[i]!);
    }
  });
  return { ...mapped, children, sizes: normalize(merged, children.length) };
}

/** Makes a tab the active one in its group. */
export function activate(layout: DockNode, tab: string): DockNode {
  return (
    rebuild(layout, (n) => (n.type === "group" && n.tabs.includes(tab) ? { ...n, active: tab } : n)) ?? layout
  );
}

/** Whether a tab is in the layout. */
export const hasTab = (layout: DockNode, tab: string): boolean => groupOf(layout, tab) !== null;

/** Shows a tab: made active where it is, else added to the first group (View > Panels). */
export function openTab(layout: DockNode, tab: string): DockNode {
  if (hasTab(layout, tab)) return activate(layout, tab);
  const first = groups(layout)[0];
  if (!first) return group("main", [tab]);
  return dockTab(layout, tab, first.id, "center");
}

/** Takes a tab out; empty groups go and their space goes to their neighbours. The last group stays, empty. */
export function removeTab(layout: DockNode, tab: string): DockNode {
  const out = rebuild(layout, (n) => {
    if (n.type !== "group" || !n.tabs.includes(tab)) return n;
    const tabs = n.tabs.filter((t) => t !== tab);
    if (tabs.length === 0) return null;
    const at = n.tabs.indexOf(tab);
    const active = n.active === tab ? (tabs[Math.min(at, tabs.length - 1)] ?? null) : n.active;
    return { ...n, tabs, active };
  });
  return out ?? group(layout.type === "group" ? layout.id : "main", []);
}

/**
 * Moves a tab onto a group: `center` adds it to the group's tabs (at `index`, else the
 * end), an edge splits the group and puts the tab in a new group on that side.
 */
export function dockTab(layout: DockNode, tab: string, target: string, zone: DockZone, index?: number): DockNode {
  const source = groupOf(layout, tab);
  const targetNode = findNode(layout, target);
  if (!targetNode || targetNode.type !== "group") return layout;
  if (zone === "center") {
    if (source?.id === target) {
      // Reordering within a group.
      const tabs = targetNode.tabs.filter((t) => t !== tab);
      tabs.splice(Math.max(0, Math.min(index ?? tabs.length, tabs.length)), 0, tab);
      return rebuild(layout, (n) => (n.id === target ? { ...targetNode, tabs, active: tab } : n)) ?? layout;
    }
    const without = source ? removeTab(layout, tab) : layout;
    return (
      rebuild(without, (n) => {
        if (n.id !== target || n.type !== "group") return n;
        const tabs = [...n.tabs];
        tabs.splice(Math.max(0, Math.min(index ?? tabs.length, tabs.length)), 0, tab);
        return { ...n, tabs, active: tab };
      }) ?? without
    );
  }
  // Splitting a group by its only tab would leave nothing on the other side.
  if (source?.id === target && targetNode.tabs.length === 1) return layout;
  const without = source ? removeTab(layout, tab) : layout;
  const fresh = group(freshId(without, "g"), [tab]);
  const direction = zone === "left" || zone === "right" ? "row" : "column";
  const before = zone === "left" || zone === "top";
  const splitId = freshId(without, "s");
  // The new split holds the target again: replace it once only.
  let done = false;
  return (
    rebuild(without, (n) => {
      if (done || n.id !== target) return n;
      done = true;
      return split(splitId, direction, before ? [fresh, n] : [n, fresh], [0.5, 0.5]);
    }) ?? without
  );
}

/** Moves the splitter after child `index` of a split by `delta` (a fraction of the split's size). */
export function resize(layout: DockNode, splitId: string, index: number, delta: number, min = MIN_FRACTION): DockNode {
  return (
    rebuild(layout, (n) => {
      if (n.id !== splitId || n.type !== "split") return n;
      const a = n.sizes[index];
      const b = n.sizes[index + 1];
      if (a === undefined || b === undefined) return n;
      const d = Math.max(min - a, Math.min(b - min, delta));
      const sizes = [...n.sizes];
      sizes[index] = a + d;
      sizes[index + 1] = b - d;
      return { ...n, sizes };
    }) ?? layout
  );
}

/** The layout as JSON (what `parseLayout` reads back). */
export const serializeLayout = (layout: DockNode): string => JSON.stringify(layout);

/**
 * A layout node as saved: any key may be missing (JSON.parse checks the types of those there).
 * Read as this record rather than `unknown` with checks: web_ui_ts has no dynamic values yet
 * (tools/web_ui_ts/TODO.md D1, D2).
 */
interface SavedNode {
  type?: string;
  id?: string;
  tabs?: string[];
  active?: string | null;
  direction?: string;
  children?: SavedNode[];
  sizes?: number[];
}

/**
 * Reads a saved layout. Malformed parts make it null; with `knownTabs`, tabs that no
 * longer exist are dropped (and groups left empty with them).
 */
export function parseLayout(json: string, knownTabs?: readonly string[]): DockNode | null {
  let raw: SavedNode;
  try {
    raw = JSON.parse(json) as SavedNode;
  } catch {
    return null;
  }
  const known = knownTabs ? new Set(knownTabs) : null;
  const seen = new Set<string>();
  // A malformed part spoils the whole layout; null is a part left empty (its tabs are gone).
  // (A flag rather than `undefined`: compiled code has one value for null and undefined.)
  let broken = false;
  const bad = (): null => {
    broken = true;
    return null;
  };
  const read = (o: SavedNode): DockNode | null => {
    const id = o.id;
    if (id === undefined || seen.has(id)) return bad();
    seen.add(id);
    if (o.type === "group") {
      if (!o.tabs) return bad();
      const tabs = o.tabs.filter((t) => !known || known.has(t));
      if (tabs.length === 0) return null;
      const active = typeof o.active === "string" && tabs.includes(o.active) ? o.active : tabs[0]!;
      return group(id, tabs, active);
    }
    if (o.type === "split") {
      const direction = o.direction;
      if ((direction !== "row" && direction !== "column") || !o.children) return bad();
      const children: DockNode[] = [];
      const sizes: number[] = [];
      const given = o.sizes ?? [];
      for (const [i, c] of o.children.entries()) {
        const node = read(c);
        if (broken) return null;
        if (node) {
          children.push(node);
          sizes.push(given[i] ?? 0);
        }
      }
      if (children.length === 0) return null;
      if (children.length === 1) return children[0]!;
      return split(id, direction, children, sizes);
    }
    return bad();
  };
  const node = read(raw);
  return broken ? null : (node ?? group("main", []));
}

/** Where every node sits inside `rect` (splitters take `SPLITTER` pixels between children). */
export function computeRects(layout: DockNode, rect: Rect, splitter = SPLITTER): Map<string, Rect> {
  const out = new Map<string, Rect>();
  const place = (node: DockNode, r: Rect) => {
    out.set(node.id, r);
    if (node.type !== "split") return;
    const row = node.direction === "row";
    const total = row ? r.width : r.height;
    const free = Math.max(0, total - splitter * (node.children.length - 1));
    let at = row ? r.x : r.y;
    node.children.forEach((c, i) => {
      const size = free * node.sizes[i]!;
      place(c, row ? { x: at, y: r.y, width: size, height: r.height } : { x: r.x, y: at, width: r.width, height: size });
      at += size + splitter;
    });
  };
  place(layout, rect);
  return out;
}

const inside = (r: Rect, x: number, y: number) => x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height;

/**
 * Where a dragged tab would land: the group under the point and the zone in it. The tab
 * strip and the middle are `center`; the outer quarter of each side is that side.
 */
export function dropTarget(layout: DockNode, rects: ReadonlyMap<string, Rect>, x: number, y: number): { group: string; zone: DockZone } | null {
  for (const g of groups(layout)) {
    const r = rects.get(g.id);
    if (!r || !inside(r, x, y)) continue;
    if (y < r.y + TAB_BAR) return { group: g.id, zone: "center" };
    const body = { x: r.x, y: r.y + TAB_BAR, width: r.width, height: r.height - TAB_BAR };
    const fx = (x - body.x) / Math.max(1, body.width);
    const fy = (y - body.y) / Math.max(1, body.height);
    // The nearest edge, if it's within a quarter.
    const edges: [DockZone, number][] = [
      ["left", fx],
      ["right", 1 - fx],
      ["top", fy],
      ["bottom", 1 - fy],
    ];
    edges.sort((a, b) => a[1] - b[1]);
    const [zone, dist] = edges[0]!;
    return { group: g.id, zone: dist < 0.25 ? zone : "center" };
  }
  return null;
}

/** The part of a group a drop on `zone` would take, for the drop preview. */
export function zoneRect(r: Rect, zone: DockZone): Rect {
  switch (zone) {
    case "left":
      return { ...r, width: r.width / 2 };
    case "right":
      return { ...r, x: r.x + r.width / 2, width: r.width / 2 };
    case "top":
      return { ...r, height: r.height / 2 };
    case "bottom":
      return { ...r, y: r.y + r.height / 2, height: r.height / 2 };
    case "center":
      return r;
  }
}
