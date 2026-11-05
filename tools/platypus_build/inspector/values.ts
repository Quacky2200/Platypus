// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The UI inspector's value logic, as plain functions: stepping a number in a style value, reading and
// writing colours, cubic-bezier curves and gradients, and filtering autocomplete words. The widgets in
// `shared.tsx` draw them; the tests are `editor/test/ui-values.test.ts`. Nothing here touches the engine.
// Names start with `uiVal` / `UiVal` so an appended copy never meets a game's. When the build appends
// this file it drops the `export`s and the `import`s.
// Design: docs/research/platypus-ui-inspector.md

// --- Numbers -------------------------------------------------------------------------------------

/** A number inside a value, with where it sits (`10px` in `margin: 10px 4px`: start 0, end 4). */
export interface UiValNumber {
  start: number;
  end: number;
  num: number;
  unit: string;
}

const isDigit = (c: number): boolean => c >= 48 && c <= 57;
const isAlpha = (c: number): boolean => (c >= 65 && c <= 90) || (c >= 97 && c <= 122);

/** Every number of a value (a leading `-` or `.` belongs to it, a unit is the letters or `%` after it). */
export function uiValNumbers(text: string): UiValNumber[] {
  const out: UiValNumber[] = [];
  let i = 0;
  while (i < text.length) {
    const c = text.charCodeAt(i);
    // A word (a name, a `#abc123` colour, `--x1`) holds no numbers: skip all of it.
    if (isAlpha(c) || c === 35 || c === 95) {
      i++;
      while (i < text.length && (isAlpha(text.charCodeAt(i)) || isDigit(text.charCodeAt(i)) || text.charCodeAt(i) === 45 || text.charCodeAt(i) === 95)) i++;
      continue;
    }
    // A number starts at a digit, or at `-` / `.` before a digit.
    const next = i + 1 < text.length ? text.charCodeAt(i + 1) : 32;
    const startsHere = isDigit(c) || ((c === 45 || c === 46) && (isDigit(next) || (c === 45 && next === 46)));
    if (!startsHere) {
      i++;
      continue;
    }
    let j = i;
    if (text.charCodeAt(j) === 45) j++;
    let dot = false;
    while (j < text.length) {
      const d = text.charCodeAt(j);
      if (isDigit(d)) j++;
      else if (d === 46 && !dot) {
        dot = true;
        j++;
      } else break;
    }
    const numEnd = j;
    while (j < text.length && (isAlpha(text.charCodeAt(j)) || text.charCodeAt(j) === 37)) j++;
    const n = parseFloat(text.substring(i, numEnd));
    if (!Number.isNaN(n)) out.push({ start: i, end: j, num: n, unit: text.substring(numEnd, j) });
    i = j;
  }
  return out;
}

/** A number as short text: no float noise (`0.1 + 0.2` is `0.3`), no trailing zeros. */
export function uiValFormatNumber(n: number): string {
  const r = Math.round(n * 10000) / 10000;
  return String(r === 0 ? 0 : r);
}

/** The step of one arrow-key press for a property and unit: 1, but 0.1 for plain numbers that live in 0..1. */
export function uiValStepSize(prop: string, unit: string): number {
  if (unit === "") {
    if (prop === "opacity" || prop === "flexGrow" || prop === "flexShrink" || prop === "zIndex" || prop === "order") {
      return prop === "opacity" ? 0.1 : 1;
    }
    if (prop === "lineHeight") return 0.1;
  }
  if (unit === "s" || unit === "em" || unit === "rem") return unit === "s" ? 0.1 : 0.1;
  return 1;
}

/**
 * The value with the number at (or just before) `caret` moved by `delta` steps of `step` (`caret` -1:
 * the first number). `null` when there is no number to move.
 */
export function uiValStep(text: string, delta: number, step: number, caret: number): string | null {
  const nums = uiValNumbers(text);
  if (nums.length === 0) return null;
  let pick = nums[0]!;
  if (caret >= 0) {
    for (let i = 0; i < nums.length; i++) {
      const n = nums[i]!;
      if (caret >= n.start && caret <= n.end) {
        pick = n;
        break;
      }
      if (n.end <= caret) pick = n;
    }
  }
  return text.substring(0, pick.start) + uiValFormatNumber(pick.num + delta * step) + pick.unit + text.substring(pick.end);
}

// --- Colours -------------------------------------------------------------------------------------

/** A colour: channels 0..255 and alpha 0..1. */
export interface UiValColor {
  r: number;
  g: number;
  b: number;
  a: number;
}

export type UiValColorFormat = "hex" | "rgb" | "hsl";

const NAMED: string[] = [
  "black", "#000000", "white", "#ffffff", "red", "#ff0000", "lime", "#00ff00", "blue", "#0000ff", "yellow", "#ffff00",
  "cyan", "#00ffff", "aqua", "#00ffff", "magenta", "#ff00ff", "fuchsia", "#ff00ff", "gray", "#808080", "grey", "#808080",
  "silver", "#c0c0c0", "maroon", "#800000", "olive", "#808000", "green", "#008000", "purple", "#800080", "teal", "#008080",
  "navy", "#000080", "orange", "#ffa500", "pink", "#ffc0cb", "brown", "#a52a2a", "gold", "#ffd700", "transparent", "#00000000",
];

function hexDigit(c: number): number {
  if (c >= 48 && c <= 57) return c - 48;
  if (c >= 97 && c <= 102) return c - 87;
  if (c >= 65 && c <= 70) return c - 55;
  return -1;
}

function clamp(n: number, lo: number, hi: number): number {
  return n < lo ? lo : n > hi ? hi : n;
}

/** The arguments of `name(...)` split on commas, spaces and `/`, or `null` when `text` is not that call. */
function callArgs(text: string, names: string[]): string[] | null {
  const t = text.trim();
  const open = t.indexOf("(");
  if (open < 0 || !t.endsWith(")")) return null;
  const name = t.substring(0, open).trim().toLowerCase();
  if (names.indexOf(name) < 0) return null;
  const body = t.substring(open + 1, t.length - 1);
  const parts: string[] = [];
  let cur = "";
  for (let i = 0; i < body.length; i++) {
    const ch = body.charAt(i);
    if (ch === "," || ch === " " || ch === "/") {
      if (cur !== "") parts.push(cur);
      cur = "";
    } else cur += ch;
  }
  if (cur !== "") parts.push(cur);
  return parts;
}

function channel(s: string): number {
  if (s.endsWith("%")) return clamp((parseFloat(s) / 100) * 255, 0, 255);
  return clamp(parseFloat(s), 0, 255);
}

function alphaOf(s: string | undefined): number {
  if (s === undefined) return 1;
  if (s.endsWith("%")) return clamp(parseFloat(s) / 100, 0, 1);
  return clamp(parseFloat(s), 0, 1);
}

/** Hue/saturation/lightness (degrees, 0..1, 0..1) to a colour. */
export function uiValFromHsl(h: number, s: number, l: number, a: number): UiValColor {
  const hh = (((h % 360) + 360) % 360) / 60;
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs((hh % 2) - 1));
  let r = 0;
  let g = 0;
  let b = 0;
  if (hh < 1) [r, g, b] = [c, x, 0];
  else if (hh < 2) [r, g, b] = [x, c, 0];
  else if (hh < 3) [r, g, b] = [0, c, x];
  else if (hh < 4) [r, g, b] = [0, x, c];
  else if (hh < 5) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  const m = l - c / 2;
  return { r: Math.round((r + m) * 255), g: Math.round((g + m) * 255), b: Math.round((b + m) * 255), a };
}

/** A colour as hue (0..360), saturation (0..1), lightness (0..1). */
export function uiValToHsl(c: UiValColor): number[] {
  const r = c.r / 255;
  const g = c.g / 255;
  const b = c.b / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  const l = (max + min) / 2;
  let h = 0;
  if (d !== 0) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  const s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  return [h, s, l];
}

/** A colour as hue (0..360), saturation (0..1), value (0..1): what a picker's square shows. */
export function uiValToHsv(c: UiValColor): number[] {
  const r = c.r / 255;
  const g = c.g / 255;
  const b = c.b / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  const hsl = uiValToHsl(c);
  return [hsl[0]!, max === 0 ? 0 : d / max, max];
}

export function uiValFromHsv(h: number, s: number, v: number, a: number): UiValColor {
  const l = v * (1 - s / 2);
  const sl = l === 0 || l === 1 ? 0 : (v - l) / Math.min(l, 1 - l);
  return uiValFromHsl(h, sl, l, a);
}

/** A CSS colour (`#rgb`, `#rrggbbaa`, `rgb()`, `rgba()`, `hsl()`, `hsla()`, a common name), or `null`. */
export function uiValParseColor(text: string): UiValColor | null {
  const t = text.trim().toLowerCase();
  if (t.startsWith("#")) {
    const ds: number[] = [];
    for (let i = 1; i < t.length; i++) {
      const d = hexDigit(t.charCodeAt(i));
      if (d < 0) return null;
      ds.push(d);
    }
    if (ds.length === 3 || ds.length === 4) {
      return { r: ds[0]! * 17, g: ds[1]! * 17, b: ds[2]! * 17, a: ds.length === 4 ? (ds[3]! * 17) / 255 : 1 };
    }
    if (ds.length === 6 || ds.length === 8) {
      return {
        r: ds[0]! * 16 + ds[1]!,
        g: ds[2]! * 16 + ds[3]!,
        b: ds[4]! * 16 + ds[5]!,
        a: ds.length === 8 ? (ds[6]! * 16 + ds[7]!) / 255 : 1,
      };
    }
    return null;
  }
  const rgb = callArgs(t, ["rgb", "rgba"]);
  if (rgb !== null) {
    if (rgb.length < 3) return null;
    const c = { r: channel(rgb[0]!), g: channel(rgb[1]!), b: channel(rgb[2]!), a: alphaOf(rgb[3]) };
    return Number.isNaN(c.r + c.g + c.b + c.a) ? null : { r: Math.round(c.r), g: Math.round(c.g), b: Math.round(c.b), a: c.a };
  }
  const hsl = callArgs(t, ["hsl", "hsla"]);
  if (hsl !== null) {
    if (hsl.length < 3) return null;
    const h = parseFloat(hsl[0]!);
    const s = clamp(parseFloat(hsl[1]!) / 100, 0, 1);
    const l = clamp(parseFloat(hsl[2]!) / 100, 0, 1);
    const a = alphaOf(hsl[3]);
    return Number.isNaN(h + s + l + a) ? null : uiValFromHsl(h, s, l, a);
  }
  const at = NAMED.indexOf(t);
  if (at >= 0 && at % 2 === 0) return uiValParseColor(NAMED[at + 1]!);
  return null;
}

/** The format a colour was written in (the picker writes it back the same way unless told otherwise). */
export function uiValColorFormat(text: string): UiValColorFormat {
  const t = text.trim().toLowerCase();
  if (t.startsWith("hsl")) return "hsl";
  if (t.startsWith("rgb")) return "rgb";
  return "hex";
}

function hex2(n: number): string {
  const v = clamp(Math.round(n), 0, 255);
  return (v < 16 ? "0" : "") + v.toString(16);
}

/** A colour as CSS text in the format asked for (alpha only when it is not 1). */
export function uiValFormatColor(c: UiValColor, format: UiValColorFormat): string {
  const a = Math.round(c.a * 1000) / 1000;
  if (format === "rgb") {
    return a < 1 ? `rgba(${Math.round(c.r)}, ${Math.round(c.g)}, ${Math.round(c.b)}, ${a})` : `rgb(${Math.round(c.r)}, ${Math.round(c.g)}, ${Math.round(c.b)})`;
  }
  if (format === "hsl") {
    const h = uiValToHsl(c);
    const hs = `${Math.round(h[0]!)}, ${Math.round(h[1]! * 100)}%, ${Math.round(h[2]! * 100)}%`;
    return a < 1 ? `hsla(${hs}, ${a})` : `hsl(${hs})`;
  }
  return "#" + hex2(c.r) + hex2(c.g) + hex2(c.b) + (a < 1 ? hex2(a * 255) : "");
}

// --- Cubic béziers -------------------------------------------------------------------------------

/** The easing keywords and their curves `[x1, y1, x2, y2]`. */
export const UI_VAL_EASINGS: { name: string; curve: number[] }[] = [
  { name: "linear", curve: [0, 0, 1, 1] },
  { name: "ease", curve: [0.25, 0.1, 0.25, 1] },
  { name: "ease-in", curve: [0.42, 0, 1, 1] },
  { name: "ease-out", curve: [0, 0, 0.58, 1] },
  { name: "ease-in-out", curve: [0.42, 0, 0.58, 1] },
];

/** A timing function inside a value (`ease-in` in `opacity 0.3s ease-in`) and where it sits. */
export interface UiValEasing {
  start: number;
  end: number;
  curve: number[];
}

function isWordChar(c: number): boolean {
  return isAlpha(c) || isDigit(c) || c === 45 || c === 95;
}

/** The first `cubic-bezier(..)` or easing keyword of a value, or `null`. */
export function uiValFindEasing(text: string): UiValEasing | null {
  const cb = text.indexOf("cubic-bezier(");
  if (cb >= 0) {
    const close = text.indexOf(")", cb);
    if (close > cb) {
      const args = text.substring(cb + 13, close).split(",");
      if (args.length === 4) {
        const curve = args.map((s) => parseFloat(s));
        if (curve.every((n) => !Number.isNaN(n))) return { start: cb, end: close + 1, curve };
      }
    }
  }
  // Keywords: longest first so `ease-in-out` is not read as `ease`.
  const names = UI_VAL_EASINGS.map((e) => e.name).sort((a, b) => b.length - a.length);
  for (let i = 0; i < text.length; i++) {
    if (i > 0 && isWordChar(text.charCodeAt(i - 1))) continue;
    for (const name of names) {
      if (text.startsWith(name, i) && (i + name.length >= text.length || !isWordChar(text.charCodeAt(i + name.length)))) {
        const e = UI_VAL_EASINGS.find((x) => x.name === name)!;
        return { start: i, end: i + name.length, curve: e.curve };
      }
    }
  }
  return null;
}

/** A curve as `cubic-bezier(..)` (x clamped to 0..1, as CSS requires), or its keyword when it is one. */
export function uiValFormatEasing(curve: number[]): string {
  const c = [clamp(curve[0]!, 0, 1), curve[1]!, clamp(curve[2]!, 0, 1), curve[3]!].map((n) => Math.round(n * 100) / 100);
  for (const e of UI_VAL_EASINGS) {
    if (e.curve[0] === c[0] && e.curve[1] === c[1] && e.curve[2] === c[2] && e.curve[3] === c[3]) return e.name;
  }
  return `cubic-bezier(${c.join(", ")})`;
}

/** The point of a curve at parameter `t` (0..1): `[x, y]` of the easing graph. */
export function uiValEasingPoint(curve: number[], t: number): number[] {
  const u = 1 - t;
  const a = 3 * u * u * t;
  const b = 3 * u * t * t;
  const d = t * t * t;
  return [a * curve[0]! + b * curve[2]! + d, a * curve[1]! + b * curve[3]! + d];
}

// --- Gradients -----------------------------------------------------------------------------------

/** One colour stop: the colour text and the position text (`""` when it has none). */
export interface UiValStop {
  color: string;
  pos: string;
}

/** A gradient: `linear` or `radial`, the text before the stops (`135deg`, `to right`, `circle at center`), the stops. */
export interface UiValGradient {
  kind: string;
  head: string;
  stops: UiValStop[];
  repeating: boolean;
}

/** `text` split at the commas outside brackets. */
export function uiValSplitTop(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let cur = "";
  for (let i = 0; i < text.length; i++) {
    const ch = text.charAt(i);
    if (ch === "(") depth++;
    else if (ch === ")") depth--;
    if (ch === "," && depth === 0) {
      parts.push(cur.trim());
      cur = "";
    } else cur += ch;
  }
  if (cur.trim() !== "") parts.push(cur.trim());
  return parts;
}

/** The index where a stop's colour ends and its position starts (the first space outside brackets), or -1. */
function stopSplit(s: string): number {
  let depth = 0;
  for (let i = 0; i < s.length; i++) {
    const ch = s.charAt(i);
    if (ch === "(") depth++;
    else if (ch === ")") depth--;
    else if (ch === " " && depth === 0) return i;
  }
  return -1;
}

/** `linear-gradient(..)` / `radial-gradient(..)` / `conic-gradient(..)` (also `repeating-`), or `null`. */
export function uiValParseGradient(text: string): UiValGradient | null {
  const t = text.trim();
  const open = t.indexOf("(");
  if (open < 0 || !t.endsWith(")")) return null;
  let name = t.substring(0, open).trim().toLowerCase();
  let repeating = false;
  if (name.startsWith("repeating-")) {
    repeating = true;
    name = name.substring(10);
  }
  if (name !== "linear-gradient" && name !== "radial-gradient" && name !== "conic-gradient") return null;
  const parts = uiValSplitTop(t.substring(open + 1, t.length - 1));
  if (parts.length < 2) return null;
  let head = "";
  // The first part is the head unless it is a stop (it starts with a colour).
  const first = parts[0]!;
  const firstColour = first.substring(0, stopSplit(first) < 0 ? first.length : stopSplit(first));
  if (uiValParseColor(firstColour) === null) head = parts.shift()!;
  const stops: UiValStop[] = parts.map((p) => {
    const at = stopSplit(p);
    return at < 0 ? { color: p, pos: "" } : { color: p.substring(0, at), pos: p.substring(at + 1).trim() };
  });
  if (stops.length < 2) return null;
  return { kind: name.substring(0, name.indexOf("-")), head, stops, repeating };
}

/** The head a gradient of `kind` starts with when it is switched to that kind (`linear`, `radial`, `conic`). */
export function uiValGradientHead(kind: string): string {
  return kind === "radial" ? "circle" : kind === "conic" ? "from 0deg" : "";
}

export function uiValFormatGradient(g: UiValGradient): string {
  const name = (g.repeating ? "repeating-" : "") + g.kind + "-gradient";
  const parts: string[] = [];
  if (g.head !== "") parts.push(g.head);
  for (const s of g.stops) parts.push(s.pos === "" ? s.color : `${s.color} ${s.pos}`);
  return `${name}(${parts.join(", ")})`;
}

/** The angle of a linear gradient's head in degrees (`to right` is 90, none is 180), or `null` for a free-form head. */
export function uiValGradientAngle(g: UiValGradient): number | null {
  const h = g.head.trim().toLowerCase();
  if (h === "") return 180;
  if (h.endsWith("deg")) {
    const n = parseFloat(h);
    return Number.isNaN(n) ? null : n;
  }
  const sides = ["to top", "to right", "to bottom", "to left"];
  const at = sides.indexOf(h);
  return at >= 0 ? at * 90 : null;
}

// --- Property names ------------------------------------------------------------------------------

/** `backgroundColor` as CSS writes it, `background-color` (custom properties `--x` stay as they are). */
export function uiValKebab(prop: string): string {
  if (prop.startsWith("--")) return prop;
  let out = "";
  for (let i = 0; i < prop.length; i++) {
    const c = prop.charAt(i);
    const upper = c >= "A" && c <= "Z";
    out += upper ? "-" + c.toLowerCase() : c;
  }
  return out;
}

/** `background-color` as the engine's scripts name it, `backgroundColor` (custom properties `--x` stay as they are). */
export function uiValCamel(prop: string): string {
  if (prop.startsWith("--")) return prop;
  let out = "";
  let up = false;
  for (let i = 0; i < prop.length; i++) {
    const c = prop.charAt(i);
    if (c === "-" && i > 0) {
      up = true;
    } else {
      out += up ? c.toUpperCase() : c;
      up = false;
    }
  }
  return out;
}

// --- Autocomplete --------------------------------------------------------------------------------

/** The words that start with `prefix`, then those that contain it (case-insensitive), at most `max`. */
export function uiValSuggest(prefix: string, words: string[], max: number): string[] {
  const p = prefix.trim().toLowerCase();
  if (p === "") return [];
  const starts: string[] = [];
  const has: string[] = [];
  for (const w of words) {
    const lw = w.toLowerCase();
    if (lw === p) continue;
    if (lw.startsWith(p)) starts.push(w);
    else if (lw.indexOf(p) >= 0) has.push(w);
  }
  // Shorter words first among those that start with it: `color` before `columnGap` for `co`.
  starts.sort((a, b) => a.length - b.length);
  return starts.concat(has).slice(0, max);
}
