// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// String-literal unions for tests/lang/unions.rs. Each case (split at `//## name`) defines
// `function t(): string`; the test compiles it with web_ui_ts (normal, GC-stress, GC-off) and
// compares the result with bun's (`bun bench/gen_cases_expected.ts unions_cases.ts`).
//## from_optional_field
type M = "a" | "b";
interface O {
  mode?: M;
}
function pick(o: O): string {
  let mode: M = "a";
  if (o.mode !== undefined) mode = o.mode;
  return mode;
}
function t(): string {
  return pick({}) + pick({ mode: "b" }) + pick({ mode: "a" });
}
//## from_optional_field_with_as
type M = "a" | "b";
interface O {
  mode?: M;
}
function pick(o: O): string {
  let mode: M = "a";
  if (o.mode !== undefined) mode = o.mode as M;
  const copy: M = mode;
  const c2 = o.mode as M;
  return mode + copy + (o.mode === undefined ? "-" : c2);
}
function t(): string {
  return pick({}) + "|" + pick({ mode: "b" });
}
//## from_nullish_coalescing
type Dir = "up" | "down" | "left";
interface Opt {
  dir?: Dir;
  other?: Dir | null;
}
function t(): string {
  const o: Opt = { other: "left" };
  const a: Dir = o.dir ?? "up";
  const b: Dir = o.other ?? "down";
  const c = o.dir ?? o.other ?? "up";
  return a + b + c;
}
//## argument_and_return
type Dir = "up" | "down" | "left";
function flip(d: Dir): Dir {
  return d === "up" ? "down" : d === "down" ? "up" : "left";
}
function name(d: Dir): string {
  return "<" + d + ">";
}
function opt(d?: Dir): Dir {
  return d ?? "left";
}
function maybe(b: boolean): Dir | undefined {
  return b ? "down" : undefined;
}
function t(): string {
  const d: Dir = "up";
  const m = maybe(true);
  return name(flip(d)) + name(flip("left")) + name(opt()) + name(opt("up")) + (maybe(false) ?? "none") + (m ?? "none");
}
//## fields_arrays_maps
type Kind = "x" | "y" | "z";
class Item {
  kind: Kind;
  alt: Kind | undefined;
  constructor(k: Kind) {
    this.kind = k;
    this.alt = undefined;
  }
}
function t(): string {
  const items: Kind[] = ["x", "y", "z"];
  items.push("x");
  const objs: Item[] = [new Item("z"), new Item("y")];
  objs[0]!.alt = "x";
  const counts = new Map<Kind, number>();
  for (const k of items) counts.set(k, (counts.get(k) ?? 0) + 1);
  const byKind = new Map<string, Kind>();
  byKind.set("first", "z");
  let s = items.join("") + objs.map((o) => o.kind + (o.alt ?? "_")).join(",");
  s += counts.get("x")! + "" + counts.get("y")! + "" + counts.get("z")!;
  s += byKind.get("first") + (byKind.get("none") ?? "dflt");
  const rec = { k: "y" as Kind, list: ["x", "z"] as Kind[] };
  rec.k = "z";
  rec.list.push(rec.k);
  return s + rec.k + rec.list.join("");
}
//## narrowing_switch
type Shape = "circle" | "square" | "tri";
function sides(s: Shape): number {
  switch (s) {
    case "circle":
      return 0;
    case "square":
      return 4;
    case "tri":
      return 3;
  }
}
function label(s: Shape | undefined): string {
  if (s === undefined) return "none";
  if (s === "circle") return "round";
  if (s !== "square") return "pointy" + s;
  return "boxy";
}
function t(): string {
  const all: Shape[] = ["circle", "square", "tri"];
  let n = 0;
  for (const s of all) n += sides(s);
  return n + label("circle") + label("square") + label("tri") + label(undefined);
}
//## compare_and_indexof
type C = "r" | "g" | "b";
function t(): string {
  const a: C = "r";
  let b: C = "g";
  const same = a === b;
  b = "r";
  const same2 = a === b;
  const o: { c?: C } = {};
  const x = o.c === undefined;
  o.c = "b";
  const y = o.c === "b";
  const z = o.c !== a;
  const list: C[] = ["r", "g", "b", "g"];
  const flags: boolean[] = [same, same2, x, y, z, list.includes("b"), list.includes(o.c)];
  const idx: number[] = [list.indexOf("g"), list.lastIndexOf("g"), list.indexOf(b)];
  return flags.join(",") + "|" + idx.join(",");
}
//## string_interop
type C = "r" | "g" | "b";
function t(): string {
  const c: C = "g";
  const s: string = c;
  const up = c.toUpperCase();
  const len = c.length;
  const parts: string[] = [c, "x"];
  let tpl = `${c}-${s}`;
  const cc: C = (s === "g" ? "b" : "r") as C;
  return s + up + len + parts.join("") + tpl + cc + (c + "!") + c.charAt(0);
}
//## keyof_literals
const TABLE = { a: 1, b: 2, c: 3 };
type K = keyof typeof TABLE;
function get(k: K): number {
  return TABLE[k];
}
function t(): string {
  const keys: K[] = ["a", "b", "c"];
  let sum = 0;
  for (const k of keys) sum += get(k);
  const k: K = "b";
  return sum + ":" + get(k);
}
//## loop_with_optional_state
type Mode = "off" | "on" | "auto";
interface Cfg {
  mode?: Mode;
}
function t(): string {
  const cfgs: Cfg[] = [{}, { mode: "on" }, { mode: "auto" }, { mode: "off" }];
  let out = "";
  let mode: Mode = "auto";
  for (const c of cfgs) {
    if (c.mode !== undefined) mode = c.mode;
    out += mode + ";";
  }
  const nested = { inner: { m: "on" as Mode | undefined } };
  let m2: Mode = "off";
  if (nested.inner.m) m2 = nested.inner.m;
  return out + m2;
}
//## json_and_gc_churn
type M = "a" | "b" | "c";
interface O {
  name: string;
  mode?: M;
}
function t(): string {
  const a: O = { name: "x" };
  const b: O = { name: "y", mode: "b" };
  let out = JSON.stringify([a, b]);
  const list: O[] = [];
  for (let i = 0; i < 200; i++) {
    const o: O = { name: "n" + i };
    if (i % 3 === 0) o.mode = "c";
    else if (i % 3 === 1) o.mode = "a";
    list.push(o);
  }
  let c = 0;
  let u = 0;
  for (const o of list) {
    let m: M = "b";
    if (o.mode !== undefined) m = o.mode;
    if (m === "c") c++;
    if (o.mode === undefined) u++;
  }
  const back = JSON.parse('[{"name":"p","mode":"c"},{"name":"q"}]') as O[];
  return out + c + "," + u + (back[0]!.mode ?? "-") + (back[1]!.mode ?? "-");
}
//## switch_on_optional_and_returns
type S = "idle" | "run" | "stop";
class Machine {
  state?: S;
  history: S[] = [];
  step(): S {
    switch (this.state) {
      case "idle":
        this.state = "run";
        break;
      case "run":
        this.state = "stop";
        break;
      case undefined:
        this.state = "idle";
        break;
      default:
        break;
    }
    const s: S = this.state ?? "idle";
    this.history.push(s);
    return s;
  }
}
function t(): string {
  const m = new Machine();
  const seen: string[] = [];
  for (let i = 0; i < 5; i++) seen.push(m.step());
  return seen.join(">") + m.history.length + m.history[2];
}
//## nested_union_takes_an_array_literal
type Key = number | string | (number | string)[];
interface Range {
  lower: Key | undefined;
  upper: Key | undefined;
}
function describe(q: Key | Range): string {
  if (typeof q === "number") return "n" + q;
  if (typeof q === "string") return "s" + q;
  if (Array.isArray(q)) return "a" + q.length;
  return "r" + JSON.stringify(q.lower);
}
function maybe(q?: Key | Range): string {
  return q === undefined ? "none" : describe(q);
}
function t(): string {
  const k: Key = "x";
  const parts: string[] = [];
  parts.push(describe([1, "b", 3]));
  parts.push(describe(k));
  parts.push(describe(7));
  parts.push(describe({ lower: [2, "c"], upper: undefined }));
  parts.push(maybe(["z"]));
  parts.push(maybe());
  return parts.join(" ");
}
//## json_parse_into_a_union
type Key = number | string | (number | string)[];
interface Row {
  key: Key;
  note: string | number[];
}
function kind(k: Key): string {
  if (typeof k === "number") return "n" + k;
  if (typeof k === "string") return "s" + k;
  return "a" + k.length;
}
function t(): string {
  const keys: Key[] = JSON.parse("[1, \"two\", [3, \"four\"], []]");
  const parts = keys.map(kind);
  const row: Row = JSON.parse("{\"key\": [5], \"note\": [1, 2]}");
  parts.push(kind(row.key) + (typeof row.note === "string" ? "s" : "a"));
  const maybe: Key | null = JSON.parse("null");
  parts.push(maybe === null ? "null" : "set");
  parts.push(JSON.stringify(keys));
  return parts.join(" ");
}
