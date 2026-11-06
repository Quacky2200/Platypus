// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Small TypeScript conveniences for tests/lang/misc.rs: functions as values, casts of empty
// literals, field types from constructor calls, `!` on assignment targets, closures over
// destructured constants, spreading strings. Each case (split at `//## name`) defines
// `function t(): string`; the test compiles it with web_ui_ts (normal, GC-stress, GC-off) and
// compares the result with bun's (`bun bench/gen_cases_expected.ts misc_cases.ts`).
//## static_function_values
class Util {
  static twice(n: number): number {
    return n * 2;
  }
  static label(s: string, n: number): string {
    return s + n;
  }
}
function apply(f: (n: number) => number, v: number): number {
  return f(v);
}
function t(): string {
  const tw = Util.twice;
  const lab = Util.label;
  const ab = Math.abs;
  const mx = Math.max;
  const pw = Math.pow;
  return `${tw(4)} ${lab("a", 2)} ${ab(-3)} ${mx(2, 9)} ${pw(2, 5)} ${apply(Util.twice, 7)} ${apply(Math.sqrt, 81)}`;
}
//## math_in_callbacks
function t(): string {
  const xs = [-1.5, 2, -3, 4];
  const a = xs.map(Math.abs);
  const f = xs.map(Math.floor);
  const r = xs.map(Math.round);
  const c = xs.filter(Math.abs).length;
  return a.join(",") + "|" + f.join(",") + "|" + r.join(",") + "|" + c;
}
//## casts_of_empty_literals
interface Item {
  id: number;
}
const state = {
  current: null as Item | null,
  items: [] as Item[],
  names: [] as string[],
  box: {} as { n?: number },
  best: null as number | null,
};
function t(): string {
  const before = state.current === null;
  state.current = { id: 4 };
  state.items.push({ id: 1 }, { id: 2 });
  state.names.push("x");
  state.box.n = 3;
  state.best = state.best === null ? 5 : state.best;
  const local = [] as number[];
  local.push(1);
  const nothing = null as string | null;
  return `${before} ${state.current.id} ${state.items.length} ${state.names[0]} ${state.box.n} ${state.best} ${local.length} ${nothing === null}`;
}
//## fields_from_constructor_calls
class Grid {
  private cells = new Float32Array(4);
  private seen = new Map<string, number>();
  private order = new Array<number>(2);
  private list = new Set<number>();
  total = 0;
  set(i: number, v: number): void {
    this.cells[i] = v;
    this.seen.set("k" + i, v);
    this.order[i % 2] = v;
    this.list.add(v);
    this.total += this.cells[i]!;
  }
  report(): string {
    return this.cells.length + ":" + this.seen.size + ":" + this.order.length + ":" + this.list.size + ":" + this.total;
  }
}
function t(): string {
  const g = new Grid();
  g.set(0, 1.5);
  g.set(1, 2);
  g.set(2, 2);
  return g.report();
}
//## non_null_assignment_targets
class Bag {
  vals: (number | undefined)[] = [1, 2, 3];
  opt: { n: number } | undefined = { n: 1 };
}
function t(): string {
  const xs: number[] = [1, 2, 3];
  xs[1]! += 4;
  xs[0]!++;
  xs[2]! *= 2;
  const m = new Map<string, number[]>();
  m.set("a", [5, 6]);
  m.get("a")![0]! *= 3;
  m.get("a")![1]! = 9;
  const b = new Bag();
  b.vals[0]! += 10;
  b.opt!.n = 7;
  (xs[1] as number) -= 1;
  return xs.join(",") + "|" + m.get("a")!.join(",") + "|" + b.vals[0] + "|" + b.opt!.n;
}
//## closures_over_destructured
function pair(): [number, string] {
  return [3, "ab"];
}
function t(): string {
  const [a, b] = pair();
  const { length } = b;
  const f = () => a + b.length + length;
  const o = { p: 1, q: 2 };
  const { p, q } = o;
  const g = () => p * q + a;
  const fs: (() => number)[] = [];
  for (const [i, s] of ["x", "yy"].entries()) fs.push(() => i + s.length);
  return `${f()} ${g()} ${fs.map((h) => h()).join("+")}`;
}
//## spread_strings
function t(): string {
  const s = "héllo";
  const emoji = "a😀b";
  const parts = [...s];
  const chars = Array.from(emoji);
  const upper = Array.from("abc", (c) => c.toUpperCase());
  let n = 0;
  for (const c of emoji) n += c.length;
  const xs = [3, 4];
  return `${parts.length} ${parts.join("-")} ${chars.length} ${chars[1]!.length} ${upper.join("")} ${n} ${Math.hypot(...xs)} ${Math.hypot(1, 2, 2)} ${Math.hypot(...[5, 12, 0])}`;
}
//## empty_array_in_conditional
interface V {
  x: number;
}
function pick(on: boolean, v: V | undefined): V[] {
  const list = on && v ? [v, { x: v.x + 1 }] : [];
  return list;
}
function t(): string {
  const a = pick(true, { x: 1 });
  const b = pick(false, { x: 1 });
  const c = pick(true, undefined);
  const names = a.length > 1 ? ["p", "q"] : [];
  const none = a.length > 5 ? [] : ["z"];
  return `${a.length}${b.length}${c.length} ${a[1]!.x} ${names.join("")} ${none.join("")}`;
}
//## field_types_from_expressions
class Gauge {
  shown = NaN;
  low = -Infinity;
  pi = Math.PI;
  big = Number.MAX_SAFE_INTEGER;
  root = Math.sqrt(16);
  lit = new Int8Array(3).fill(-1);
  seen = new Map<number, string[]>();
  kinds = [] as string[];
  state = { n: 0, items: [] as number[], best: null as number | null };
  report(): string {
    this.state.items.push(2);
    this.state.best = 4;
    this.seen.set(1, ["a"]);
    this.kinds.push("k");
    return `${isNaN(this.shown)} ${this.low} ${this.pi > 3} ${this.big > 1e15} ${this.root} ${this.lit.join(",")} ${this.seen.size} ${this.kinds[0]} ${this.state.items.length} ${this.state.best}`;
  }
}
function t(): string {
  return new Gauge().report();
}
//## top_level_destructuring
function pair(): [number, string] {
  return [7, "seven"];
}
const [ta, tb] = [1, 2];
const { p, q } = { p: "p", q: 4 };
let [m, s] = pair();
const [head, ...tail] = [10, 20, 30];
const fromClosure = () => ta + tb + q + m + head + tail.length;
function t(): string {
  m += 1;
  return `${ta}${tb}${p}${q} ${m}${s} ${head} ${tail.join("+")} ${fromClosure()}`;
}
//## spread_into_function_values_and_constructors
class P {
  constructor(
    public a: number,
    public b: number,
    public c: number = 100,
  ) {}
  sum(): number {
    return this.a + this.b + this.c;
  }
}
function apply(f: (a: number, b: number) => number, xs: number[]): number {
  return f(...xs);
}
function apply3(f: (a: number, b: number, c: number) => string, head: number, rest: number[]): string {
  return f(head, ...rest);
}
function rnd(min: number, max: number): number {
  return min * 10 + max;
}
const range: [number, number] = [2, 5];
function t(): string {
  const ys = [3, 4];
  const p = new P(...ys);
  const q = new P(...[1, 2, 3]);
  const mul = (a: number, b: number) => a * b;
  return `${apply(mul, ys)} ${apply((a, b) => a - b, [10, 4])} ${apply3((a, b, c) => `${a}${b}${c}`, 1, [2, 3])} ${p.sum()} ${q.sum()} ${rnd(...range)}`;
}
//## generic_function_with_void
function under<T>(f: () => T): T {
  return f();
}
let n = 0;
function t(): string {
  under<void>(() => {
    n++;
  });
  under<void>(() => {
    n += 2;
  });
  return `${n} ${under(() => 3)} ${under(() => "s")}`;
}
