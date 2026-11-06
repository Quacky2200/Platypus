// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Dictionaries and small language leftovers the original example sources use, for
// tests/lang/leftovers.rs. Each case (split at `//## name`) defines `function t(): string`; the test
// compiles it with web_ui_ts (normal, GC-stress, GC-off) and compares the result with bun's
// (`bun bench/gen_cases_expected.ts leftovers_cases.ts`).
//## record_over_a_union_is_iterable
type Tag = "a" | "b" | "c";
const R: Record<Tag, number> = { a: 1, b: 2, c: 3 };
function t(): string {
  const keys = Object.keys(R);
  const vals = Object.values(R);
  const ents = Object.entries(R)
    .map(([k, v]) => k + v)
    .join(",");
  let loop = "";
  for (const [k, v] of Object.entries(R)) loop += k + v * 2;
  let forIn = "";
  for (const k in R) forIn += k;
  const doubled = Object.fromEntries(Object.entries(R).map(([k, v]) => [k, v * 2]));
  const has = ("b" in R) && !("z" in R);
  return `${keys.join("")} ${vals.reduce((a, b) => a + b, 0)} ${ents} ${loop} ${forIn} ${doubled["c"]} ${has} ${Object.keys(doubled).length}`;
}
//## partial_record_presence_and_delete
type Tag = "a" | "b" | "c";
const P: Partial<Record<Tag, number>> = { a: 1 };
function t(): string {
  const before = Object.keys(P).join("");
  P.b = 5;
  const mid = `${"a" in P}${"b" in P}${"c" in P}`;
  delete P.a;
  const after = `${"a" in P}${Object.keys(P).join("")}${Object.values(P).join("+")}`;
  const e = Object.entries(P)
    .map(([k, v]) => `${k}=${v}`)
    .join(";");
  return `${before} ${mid} ${after} ${e} ${P.a === undefined}`;
}
//## string_dictionary_operations
function t(): string {
  const d: Record<string, number> = {};
  d["x"] = 1;
  d["y"] = 2;
  d["z"] = 3;
  delete d["y"];
  const key = "x";
  const present = key in d;
  const gone = "y" in d;
  const keys = Object.keys(d).join("");
  const vals = Object.values(d).join("+");
  const ents = Object.entries(d)
    .map(([k, v]) => k + v)
    .join(",");
  const rebuilt = Object.fromEntries([
    ["p", 1],
    ["q", 2],
  ] as [string, number][]);
  let n = 0;
  for (const k in d) n += d[k]!;
  return `${present} ${gone} ${keys} ${vals} ${ents} ${rebuilt["q"]} ${Object.keys(rebuilt).length} ${n}`;
}
//## const_arrays_are_literal_unions
const NAMES = ["tick", "ding", "tada"] as const;
type Name = (typeof NAMES)[number];
function upper(n: Name): string {
  return n.toUpperCase();
}
const PAIR = [1, "two"] as const;
function t(): string {
  let s = "";
  for (const n of NAMES) s += upper(n);
  const mapped = NAMES.map(upper).join("-");
  const idx = NAMES.indexOf("ding");
  const has = NAMES.includes("tada");
  const first: Name = NAMES[0];
  const lens = NAMES.map((n) => n.length).join("");
  return `${s} ${mapped} ${idx} ${has} ${first} ${NAMES.length} ${lens} ${PAIR[0]}${PAIR[1]}`;
}
//## flat_with_a_depth_that_is_not_a_literal
function flatten(xs: number[][][], depth: number): number[] {
  return xs.flat(depth) as number[];
}
function t(): string {
  const xs = [[[1, 2], [3]], [[4]]];
  const a = xs.flat(2);
  const d = 2;
  const b = xs.flat(d);
  return `${a.join("")} ${b.join("")} ${flatten(xs, 2).join("")} ${[[1], [2, 3]].flat().length}`;
}
//## a_tuple_as_a_rest_parameter
function describe(...args: [string, number]): string {
  return args[0] + args[1];
}
function withOptional(...args: [number, string?]): string {
  return `${args[0]}${args[1] ?? "-"}`;
}
function t(): string {
  const pair: [string, number] = ["k", 4];
  return `${describe("a", 1)} ${describe(...pair)} ${withOptional(5)} ${withOptional(6, "x")}`;
}
//## const_array_with_a_string_argument
const MODES = ["disable", "hardOnly", "all"] as const;
function next<T>(list: readonly T[], cur: T): T {
  return list[(list.indexOf(cur) + 1) % list.length]!;
}
let current: string = "hardOnly";
function t(): string {
  const a = next(MODES, current);
  current = a;
  const b = next(MODES, current);
  const c = next(MODES, "all");
  const lit: "disable" | "hardOnly" | "all" = next(MODES, "disable");
  return `${a} ${b} ${c} ${lit}`;
}
//## reduce_without_an_initial_value
interface Glow {
  age: number;
  life: number;
}
function t(): string {
  const max = [3, 9, 4].reduce((a, b) => (a > b ? a : b));
  const right = ["a", "b", "c"].reduceRight((a, b) => a + b);
  const glows: Glow[] = [{ age: 1, life: 4 }, { age: 3, life: 4 }, { age: 1, life: 2 }];
  const oldest = glows.reduce((a, b) => (a.age / a.life > b.age / b.life ? a : b));
  let caught = "";
  try {
    ([] as number[]).reduce((a, b) => a + b);
  } catch (e) {
    caught = e instanceof TypeError ? "TypeError" : "other";
  }
  const one = [7].reduce((a, b) => a * b);
  return `${max} ${right} ${oldest.age}/${oldest.life} ${caught} ${one}`;
}
