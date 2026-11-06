// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Array and spread features for tests/lang/arrays2.rs. Each case (split at `//## name`) defines
// `function t(): string`; compiled with web_ui_ts (normal, GC-stress, GC-off) and compared with bun
// (`bun bench/gen_cases_expected.ts arrays2_cases.ts`).
//## from_length
function t(): string {
  const a = Array.from({ length: 5 }, (_, i) => i * 2);
  const b = Array.from({ length: 3 }, (_, i) => "s" + i);
  const n = 4;
  const c = Array.from({ length: n }, () => ({ v: 1 }));
  c[2]!.v = 9;
  const d = Array.from({ length: 0 }, (_, i) => i);
  const e: boolean[] = Array.from({ length: 3 }, (_, i) => i % 2 === 0);
  return a.join(",") + "|" + b.join(",") + "|" + c.map((o) => o.v).join(",") + "|" + d.length + "|" + e.join(",");
}
//## from_array_and_iterables
function t(): string {
  const src = [1, 2, 3];
  const a = Array.from(src);
  a.push(4);
  const b = Array.from(src, (x) => x * x);
  const c = Array.from(src, (x, i) => x + i);
  const s = new Set<number>([3, 1, 3, 2]);
  const d = Array.from(s);
  const e = Array.from(s, (x) => "n" + x);
  const f = ["h", "i"];
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  const g = Array.from(m.keys());
  const h = Array.from(m.values(), (v) => v * 10);
  const words = Array.from(["x", "y"], (w) => w + w);
  return [src.join(""), a.join(""), b.join(""), c.join(""), d.join(""), e.join(""), f.join("|"), g.join(""), h.join(","), words.join(",")].join(";");
}
//## new_array_n
function t(): string {
  const a = new Array<number>(3);
  const n = a.length;
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  const b = new Array(2).fill("x");
  const c = new Array<number>(4).fill(0);
  c[1] = 5;
  const d: string[] = new Array(3).fill("");
  d[2] = "z";
  const e = new Array(0);
  const f = new Array<number>(2).fill(7).map((x) => x + 1);
  const g = new Array(3).fill(null).map((_, i) => i);
  return n + a.join("") + b.join("") + c.join("") + d.join("-") + e.length + f.join("") + g.join("");
}
//## fill_range
function t(): string {
  const a = [1, 2, 3, 4, 5];
  a.fill(0, 1, 3);
  const b = [1, 2, 3, 4, 5];
  b.fill(9, 3);
  const c = [1, 2, 3, 4, 5];
  c.fill(8, -2);
  const d = [1, 2, 3, 4, 5];
  d.fill(7, 1, -1);
  const e = [1, 2, 3];
  e.fill(4, 5, 9);
  const f = ["a", "b", "c"];
  const r = f.fill("z", 1);
  const g = [1, 2, 3];
  g.fill(5);
  return [a, b, c, d, e, g].map((x) => x.join("")).join("|") + r.join("") + (r === f);
}
//## flat_and_flat_map
function t(): string {
  const a = [[1, 2], [3], [], [4, 5]];
  const b = a.flat();
  const c = [[[1], [2]], [[3]]];
  const d = c.flat();
  const e = c.flat(2);
  const s = [["a", "b"], ["c"]].flat();
  const fm = [1, 2, 3].flatMap((x) => [x, x * 10]);
  const fm2 = ["ab", "c"].flatMap((w) => w.split(""));
  const fm3 = [1, 2, 3].flatMap((x) => (x % 2 === 0 ? [] : [x]));
  const objs = [{ k: [1, 2] }, { k: [3] }].flatMap((o) => o.k);
  return [b.join(""), String(d.length), e.join(""), s.join(""), fm.join(","), fm2.join(""), fm3.join(""), objs.join("")].join(";");
}
//## index_of_all_types
type C = "r" | "g" | "b";
class P {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
}
function t(): string {
  const nums = [5, 6, 7, 6];
  const strs = ["a", "bb", "a" + "c", "bb"];
  const lits: C[] = ["r", "g", "b", "g"];
  const p1 = new P(1);
  const p2 = new P(2);
  const ps = [p1, p2, p1];
  const bools = [true, false, true];
  const out: string[] = [];
  out.push(String(nums.indexOf(6)), String(nums.lastIndexOf(6)), String(nums.indexOf(9)), String(nums.includes(7)), String(nums.includes(8)));
  const key = "b" + "b";
  out.push(String(strs.indexOf(key)), String(strs.lastIndexOf(key)), String(strs.includes("ac")), String(strs.indexOf("zz")));
  out.push(String(lits.indexOf("g")), String(lits.lastIndexOf("g")), String(lits.includes("b")));
  out.push(String(ps.indexOf(p2)), String(ps.lastIndexOf(p1)), String(ps.includes(new P(1))), String(ps.includes(p1)));
  out.push(String(bools.indexOf(false)), String(bools.includes(true)));
  out.push(String(nums.indexOf(6, 2)), String(nums.includes(5, 1)), String(nums.lastIndexOf(6, 2)));
  return out.join(",");
}
//## spread_in_calls
function many(first: string, ...rest: number[]): string {
  return first + rest.length + ":" + rest.join("");
}
function fixed(xs: number[]): number {
  return xs.length;
}
function t(): string {
  const list = [4, 2, 9];
  const xs = [3, 1, 2];
  const lo = Math.min(5, ...list);
  const hi = Math.max(...xs);
  const hi2 = Math.max(0, ...xs, 100, ...list);
  const lo2 = Math.min(...list, 1);
  const arr = [...list, ...xs];
  const joined = many("a", ...list);
  const joined2 = many("b", 7, ...list, 8);
  const none: number[] = [];
  const j3 = many("c", ...none);
  const p = Math.max(...none);
  const q = fixed([...list, 1]);
  return [lo, hi, hi2, lo2, arr.length, p, q].join(";") + joined + joined2 + j3;
}
//## spread_into_fixed_params
function add3(a: number, b: number, c: number): number {
  return a + b * 10 + c * 100;
}
function greet(name: string, greeting: string = "hi"): string {
  return greeting + " " + name;
}
function t(): string {
  const args: [number, number, number] = [1, 2, 3];
  const r1 = add3(...args);
  const two: [string, string] = ["bo", "yo"];
  const r2 = greet(...two);
  return r1 + ";" + r2;
}
//## spread_arrays_and_objects
function t(): string {
  const a = [1, 2];
  const b = [3];
  const c = [...a, ...b, 4];
  const d = [0, ...a];
  const e = [...c.slice(1), ...[]];
  const o1 = { x: 1, y: 2 };
  const o2 = { ...o1, y: 5, z: 6 };
  const o3 = { w: 0, ...o1 };
  return [c.join(""), d.join(""), e.join("")].join(",") + [o2.x, o2.y, o2.z, o3.w, o3.y].join(",");
}
//## write_at_length_grows
function t(): string {
  const a: number[] = [];
  a[a.length] = 5;
  a[a.length] = 6;
  a[a.length] = 7;
  const s: string[] = ["x"];
  s[s.length] = "y";
  const o: { n: number }[] = [];
  for (let i = 0; i < 40; i++) o[o.length] = { n: i };
  a[0] = 9;
  const l: ("p" | "q")[] = [];
  l[l.length] = "q";
  l[l.length] = "p";
  return a.join("") + a.length + s.join("") + s.length + o.length + o[39]!.n + l.join("");
}
//## read_out_of_bounds
function t(): string {
  const a = [1, 2, 3];
  const s = ["x"];
  const o = [{ n: 1 }];
  const i = 3;
  const v = a[i];
  const w = a[-1];
  const u = s[5];
  const p = o[2];
  const r: string[] = [];
  r.push(v === undefined ? "u" : "n");
  r.push(w === undefined ? "u" : "n");
  r.push(u === undefined ? "u" : "n");
  r.push(p === undefined ? "u" : "n");
  return r.join("") + (a[10] ?? -1) + (s[9] ?? "dflt");
}
//## spread_edge_cases
function f(a: number, b: number = 5, ...rest: number[]): string {
  return a + "," + b + "," + rest.join("/");
}
function g(a: number, ...rest: number[]): string {
  return a + ":" + rest.length;
}
class K {
  total: number = 0;
  add(x: number, y: number = 100): number {
    this.total += x + y;
    return this.total;
  }
}
function t(): string {
  const one = [1];
  const four = [1, 2, 3, 4];
  const ta = new Float64Array([4, 8, 2]);
  const nested = [[[1], [2]], [[3]]];
  const k = new K();
  const r1 = k.add(...[1, 2]);
  const r2 = k.add(...[10]);
  return [f(...four), f(...one), g(...four), g(7, ...four)].join(";") + "|" + Math.max(...ta) + Math.min(...ta, 1) + Math.max(...[]) + "|" + nested.flat(Infinity).join("") + nested.flat(0).length + "|" + r1 + "," + r2;
}
