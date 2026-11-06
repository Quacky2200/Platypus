// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Tuples and destructuring assignment for tests/lang/tuples.rs. Each case (split at `//## name`)
// defines `function t(): string`; the test compiles it with web_ui_ts (normal, GC-stress, GC-off)
// and compares the result with bun's (`bun bench/gen_cases_expected.ts tuples_cases.ts`).
//## length_and_index_writes
function mk(): [number, string, boolean] {
  return [1, "ab", true];
}
function bump(t: [number, string, boolean]): void {
  t[0] += 5;
  t[1] = t[1] + "z";
  t[2] = !t[2];
}
function t(): string {
  const a = mk();
  bump(a);
  const list: [number, string][] = [
    [1, "x"],
    [2, "y"],
  ];
  list[1]![0] = 9;
  list[0]![1] = "w";
  return `${a.length} ${a[0]} ${a[1]} ${a[2]} ${list.length} ${list[0]!.length} ${list.map((p) => p[0] + p[1]).join(",")}`;
}
//## rest_and_spread
function show(n: number, s: string, b: boolean): string {
  return `${n}${s}${b}`;
}
function t(): string {
  const tup: [number, string, boolean] = [1, "q", false];
  const [first, ...rest] = tup;
  const [, ...tail] = tup;
  const copy = [...tup];
  const more: [number, string, boolean, number] = [...tup, 7];
  const nums: [number, number, number] = [3, 4, 5];
  const [h, ...others] = nums;
  return `${first} ${rest.length} ${rest[0]}${rest[1]} ${tail.length} ${copy.length} ${more.length} ${more[3]} ${h} ${others.join("")} ${show(...tup)}`;
}
//## tuple_params_and_loops
function label([n, s]: [number, string], i: number): string {
  return `${i}:${s}${n}`;
}
function total(pairs: [string, number][]): number {
  let t = 0;
  for (const [, n] of pairs) t += n;
  return t;
}
function t(): string {
  const pairs: [number, string][] = [
    [1, "a"],
    [2, "b"],
  ];
  const named: [string, number][] = [
    ["x", 10],
    ["y", 20],
  ];
  let out = "";
  for (const [n, s] of pairs) out += s + n;
  const m = pairs.map(([n, s], i) => label([n, s], i)).join(",");
  const fn = ([a, b]: [number, string]) => a + b.length;
  return `${out} ${m} ${total(named)} ${pairs.map(fn).join("+")} ${named.map(([k, v]) => k + v).join("")}`;
}
//## destructuring_assignment
function pair(): [number, string] {
  return [7, "seven"];
}
interface P {
  x: number;
  y: number;
  name?: string;
}
function t(): string {
  let a = 1;
  let b = 2;
  [a, b] = [b, a];
  const p = [10, 20, 30, 40];
  [p[0]!, p[3]!] = [p[3]!, p[0]!];
  let n = 0;
  let s = "";
  [n, s] = pair();
  const o: P = { x: 1, y: 2 };
  let x = 0;
  let y = 0;
  ({ x, y } = o);
  let name = "none";
  ({ name = "dflt" } = o);
  let first = 0;
  let rest: number[] = [];
  [first, ...rest] = [5, 6, 7];
  const grid = [
    [1, 2],
    [3, 4],
  ];
  [grid[0]![1]!, grid[1]![0]!] = [grid[1]![0]!, grid[0]![1]!];
  let u = 0;
  let v = 0;
  [u = 100, v = 200] = [1];
  return `${a}${b} ${p.join(",")} ${n}${s} ${x}${y}${name} ${first}${rest.join("")} ${grid.map((r) => r.join(",")).join("|")} ${u},${v}`;
}
//## swap_in_a_loop
function t(): string {
  const xs = [5, 3, 8, 1, 9, 2];
  for (let i = 0; i < xs.length; i++) {
    for (let j = 0; j + 1 < xs.length - i; j++) {
      if (xs[j]! > xs[j + 1]!) [xs[j], xs[j + 1]] = [xs[j + 1]!, xs[j]!];
    }
  }
  let a = 0;
  let b = 1;
  for (let i = 0; i < 10; i++) [a, b] = [b, a + b];
  return xs.join(",") + " " + a + " " + b;
}
//## defaulted_boolean_closure_params
function t(): string {
  const f = (flag: boolean = false, n = 1) => (flag ? n : -n);
  const g = (x?: boolean) => (x ? 1 : 0);
  const h = (loud = true, word = "hi") => (loud ? word.toUpperCase() : word);
  const cb: (on?: boolean) => string = (on = true) => (on ? "on" : "off");
  return `${f()} ${f(true)} ${f(false, 5)} ${f(true, 3)} ${g()} ${g(true)} ${g(false)} ${h()} ${h(false)} ${h(false, "yo")} ${cb()} ${cb(false)} ${cb(true)}`;
}
