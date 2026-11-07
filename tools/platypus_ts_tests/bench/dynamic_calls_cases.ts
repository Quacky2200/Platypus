// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Calls through dynamic `any` / `unknown` values for tests/lang/dynamic_calls.rs (phase P3 of
// docs/research/platypus-dynamic-values.md): boxed closures called through `any`, `any` cast back
// to function types (adapters), methods of objects, strings, arrays, maps and sets through `any`,
// and `for..of` over `any`. Each case (split at `//## name`) defines `function t(): string`; the
// test compiles it with web_ui_ts (normal, GC-stress, GC-off) and compares the result with bun's
// (`bun bench/gen_cases_expected.ts dynamic_calls_cases.ts`).
//## call_boxed_closures_with_each_argument_kind
class P {
  x: number = 4;
}
function show(v: unknown): string {
  return typeof v + ":" + String(v);
}
function t(): string {
  const out: string[] = [];
  const num: any = (a: number, b: number): number => a * 10 + b;
  out.push(String(num(4, 2)));
  const str: any = (s: string): string => s.toUpperCase() + "!";
  out.push(str("hi"));
  const bool: any = (b: boolean): string => (b ? "yes" : "no");
  out.push([bool(true), bool(false)].join(""));
  const nul: any = (v: number | null): string => (v === null ? "null" : "n" + v);
  // (`nul(undefined)` is null too here: typed code does not tell them apart.)
  out.push([nul(null), nul(3)].join(""));
  const obj: any = (p: P): number => p.x * 2;
  out.push(String(obj(new P())));
  const arr: any = (xs: number[]): number => xs.length + xs[0]!;
  out.push(String(arr([5, 6, 7])));
  const fn: any = (f: (n: number) => number): number => f(3) + f(4);
  out.push(String(fn((n: number): number => n * n)));
  const dyn: any = (v: any): string => show(v);
  out.push([dyn(1), dyn("s"), dyn(null), dyn(undefined), dyn(true)].join(","));
  const nested: any = (v: unknown): unknown => [v, v];
  out.push(JSON.stringify(nested(JSON.parse('{"a": 1}'))));
  out.push(JSON.stringify(nested([1, "mixed", null])));
  const none: any = (): void => {};
  out.push(String(none()));
  return out.join("|");
}
//## arity_missing_and_extra_arguments
function t(): string {
  const out: string[] = [];
  const opt: any = (a: number, b?: number): string => a + "/" + (b === undefined ? "u" : String(b));
  out.push(opt(1));
  out.push(opt(1, 2));
  out.push(opt(1, 2, 3, 4));
  const anyArgs: any = (a: any, b: any): string => typeof a + typeof b;
  out.push(anyArgs());
  out.push(anyArgs(1));
  const zero: any = (): number => 7;
  out.push(String(zero(1, "x", null)));
  return out.join(",");
}
//## adapters_both_ways
function apply2(f: (a: number, b: number) => number, a: number, b: number): number {
  return f(a, b);
}
function t(): string {
  const out: string[] = [];
  const add = (a: number, b: number): number => a + b;
  const boxed: unknown = add;
  // Back to its own signature: the same closure.
  const same = boxed as (a: number, b: number) => number;
  out.push(String(same(2, 3)));
  out.push(String(same === add));
  // To another signature: an adapter that boxes, calls and unboxes.
  const loose = boxed as (a: number, b: number, c: number) => number;
  out.push(String(loose(1, 2, 100)));
  const dynArgs: any = (a: any, b: any): any => (a as number) + (b as number);
  out.push(String(apply2(dynArgs, 20, 22)));
  // The adapter boxed again is still callable.
  const again: any = dynArgs as (a: number, b: number) => number;
  out.push(String(again(1, 1)));
  const viaRecord: Record<string, any> = { f: again };
  out.push(String(apply2(viaRecord["f"], 5, 6)));
  return out.join(",");
}
//## captured_variables_after_scope_returns
function counter(): any {
  let n = 0;
  const step = 10;
  return (by: number): number => {
    n += by * step;
    return n;
  };
}
function makePair(): Record<string, any> {
  let shared = "";
  return {
    add: (s: string): void => {
      shared += s;
    },
    get: (): string => shared,
  };
}
function t(): string {
  const c = counter();
  c(1);
  c(2);
  const r = makePair();
  r["add"]("a");
  r["add"]("bc");
  const parts: string[] = [];
  for (let i = 0; i < 3; i++) {
    const keep: any = (): number => i * 100;
    parts.push(String(keep()));
  }
  return `${c(3)} ${r["get"]()} ${parts.join(",")}`;
}
//## recursion_through_any
const fns: Record<string, any> = {};
fns["fib"] = (n: number): number =>
  n < 2 ? n : (fns["fib"](n - 1) as number) + (fns["fib"](n - 2) as number);
fns["even"] = (n: number): boolean => (n === 0 ? true : fns["odd"](n - 1));
fns["odd"] = (n: number): boolean => (n === 0 ? false : fns["even"](n - 1));
function t(): string {
  return String(fns["fib"](15)) + " " + String(fns["even"](10)) + " " + String(fns["odd"](7));
}
//## exceptions_through_dynamic_calls
function t(): string {
  const out: string[] = [];
  const thrower: any = (msg: string): number => {
    throw new RangeError(msg);
  };
  try {
    thrower("deep");
    out.push("no");
  } catch (e) {
    out.push((e as Error).name + ":" + (e as Error).message);
  }
  const wrap: any = (f: any): string => {
    try {
      return String(f("inner"));
    } catch (e) {
      return "caught " + (e as Error).message;
    }
  };
  out.push(wrap(thrower));
  const rethrow: any = (): void => {
    wrap((s: string): number => {
      throw new Error("x" + s);
    });
    throw new TypeError("after");
  };
  try {
    rethrow();
  } catch (e) {
    out.push(String(e));
  }
  const xs: any = [1, 2, 3];
  try {
    xs.map((v: number): number => {
      if (v === 2) throw new Error("at two");
      return v;
    });
  } catch (e) {
    out.push((e as Error).message);
  }
  return out.join("|");
}
//## record_of_unknown_functions
function t(): string {
  const exports: Record<string, unknown> = {
    add: (a: number, b: number): number => a + b,
    name: "mod",
  };
  const add = exports["add"] as (a: number, b: number) => number;
  const out: string[] = [String(add(2, 3))];
  const logged: string[] = [];
  const imports: Record<string, Record<string, unknown>> = {
    env: {
      log: (x: number): void => {
        logged.push("log " + x);
      },
    },
  };
  (imports["env"]!["log"] as (x: number) => void)(5);
  const direct: any = exports["add"];
  out.push(String(direct(10, 20)));
  out.push(typeof exports["add"] + " " + typeof exports["name"]);
  return out.join(",") + " " + logged.join(";");
}
//## methods_of_class_instances
class Animal {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
  speak(): string {
    return this.name + " makes a sound";
  }
  rename(n: string): Animal {
    this.name = n;
    return this;
  }
}
class Dog extends Animal {
  speak(): string {
    return this.name + " barks";
  }
  fetch(times: number): string {
    return this.name + " fetches " + times;
  }
}
function t(): string {
  const a: any = new Animal("cat");
  const d: any = new Dog("rex");
  const list: any[] = [a, d];
  const out: string[] = [];
  for (const x of list) out.push(x.speak());
  out.push(d.fetch(2));
  out.push(d.rename("max").speak());
  const m = d.speak;
  out.push(typeof m);
  return out.join("|");
}
//## function_fields_and_plain_objects
interface Handlers {
  onHit: (n: number) => string;
  label: string;
}
function t(): string {
  const h: Handlers = { onHit: (n: number): string => "hit " + n, label: "L" };
  const v: any = h;
  const parsed: any = JSON.parse('{"a": 1, "list": [3, 1, 2]}');
  const out: string[] = [v.onHit(3), String(parsed.list.indexOf(1)), parsed.list.join("-")];
  const rec: Record<string, any> = { twice: (x: number): number => x * 2 };
  const r: any = rec;
  out.push(String(r.twice(21)));
  return out.join(",");
}
//## string_methods
function t(): string {
  const s: any = "  Hello, World  ";
  const w: any = "abcabc";
  const out: any[] = [
    s.trim(),
    s.trimStart(),
    s.trimEnd() + "|",
    s.toUpperCase(),
    s.toLowerCase(),
    w.indexOf("c"),
    w.indexOf("c", 3),
    w.lastIndexOf("b"),
    w.includes("ca"),
    w.startsWith("abc"),
    w.endsWith("bc"),
    w.slice(1, 4),
    w.slice(-2),
    w.substring(4, 1),
    w.charAt(2),
    w.charCodeAt(0),
    w.at(-1),
    w.repeat(2),
    "5".padStart(3, "0"),
    "5".padEnd(3, "-"),
    w.split("b").join("+"),
    w.split("").length,
    w.replace("b", "B"),
    w.replaceAll("b", "B"),
    w.concat("!", 1),
    w.toString(),
  ];
  return out.map((v: any): string => String(v)).join(",");
}
//## string_methods_edge_cases
function t(): string {
  const e: any = "";
  const u: any = "héllo 👋 wörld";
  const out: any[] = [
    e.trim().length,
    e.split(",").length,
    e.split("").length,
    e.indexOf(""),
    e.at(0),
    e.charAt(5),
    e.slice(-3),
    u.length,
    u.toUpperCase(),
    u.indexOf("👋"),
    u.slice(6, 8),
    u.split(" ").length,
    u.at(-1),
    u.codePointAt(6),
    u.charCodeAt(100),
    "abc".slice(5),
    "abc".substring(-1, 2),
    "abc".indexOf("c", -5),
    "a,b,,c".split(",", 2).join("/"),
    "x".repeat(0) + "|",
    "abc".padStart(2, "z"),
    "aaa".replaceAll("a", "bb"),
  ];
  return out.map((v: any): string => String(v)).join(",");
}
//## array_methods_mutating
function t(): string {
  const xs: any = [1, 2, 3];
  const ss: any = ["a"];
  const ds: any = JSON.parse("[true, null]");
  const out: string[] = [];
  out.push(String(xs.push(4, 5)) + ":" + xs.join(","));
  out.push(String(xs.pop()) + ":" + xs.join(","));
  out.push(String(xs.shift()) + ":" + xs.join(","));
  out.push(String(xs.unshift(9)) + ":" + xs.join(","));
  out.push(String(ss.push("b")) + ":" + ss.join(""));
  out.push(String(ds.push("x", 1)) + ":" + JSON.stringify(ds));
  xs.reverse();
  out.push(xs.join(","));
  const empty: any = [];
  out.push(String(empty.pop()) + String(empty.shift()) + String(empty.length));
  return out.join("|");
}
//## array_methods_callbacks
function t(): string {
  const xs: any = [3, 1, 4, 1, 5, 9, 2, 6];
  const out: string[] = [];
  out.push(xs.map((v: number, i: number): number => v * i).join(","));
  out.push(xs.filter((v: number): boolean => v % 2 === 1).join(","));
  let sum = 0;
  xs.forEach((v: number): void => {
    sum += v;
  });
  out.push(String(sum));
  out.push(String(xs.reduce((a: number, b: number): number => a + b, 100)));
  out.push(String(xs.reduce((a: number, b: number): number => (a > b ? a : b))));
  out.push(String(xs.some((v: number): boolean => v > 8)) + String(xs.every((v: number): boolean => v > 0)));
  out.push(String(xs.find((v: number): boolean => v > 4)) + String(xs.findIndex((v: number): boolean => v > 4)));
  out.push(String(xs.find((v: number): boolean => v > 40)) + String(xs.findIndex((v: number): boolean => v > 40)));
  out.push(String(xs.findLast((v: number): boolean => v < 3)) + String(xs.findLastIndex((v: number): boolean => v < 3)));
  const words: any = ["b", "a"];
  out.push(words.map((w: string): string => w + w).join(""));
  const arrs: any = [[1, 2], [3]];
  out.push(String(arrs.map((a: number[]): number => a.length).join("")));
  return out.join("|");
}
//## array_methods_queries_and_edge_cases
function t(): string {
  const xs: any = [1, 2, NaN, 2, "2", null];
  const e: any = [];
  const out: any[] = [
    xs.indexOf(2),
    xs.lastIndexOf(2),
    xs.indexOf(NaN),
    xs.includes(NaN),
    xs.includes("2"),
    xs.includes(3),
    xs.indexOf(2, -3),
    xs.at(-1),
    xs.at(10),
    xs.slice(1, -2).length,
    xs.slice(-100).length,
    xs.join(),
    xs.join(""),
    xs.toString(),
    xs.concat([7, 8], 9).length,
    e.join("-") + "|",
    e.map((v: any): any => v).length,
    e.reduce((a: number, b: number): number => a + b, 5),
    e.some((v: any): boolean => true),
    e.every((v: any): boolean => false),
    e.find((v: any): boolean => true),
  ];
  const ks: any = ["x", "y"];
  const keys: string[] = [];
  for (const k of ks.keys()) keys.push(String(k));
  for (const pair of ks.entries()) keys.push(JSON.stringify(pair));
  for (const v of ks.values()) keys.push(v);
  return out.map((v: any): string => String(v)).join(",") + " " + keys.join(";");
}
//## array_reduce_of_empty_throws
function t(): string {
  const e: any = [];
  try {
    e.reduce((a: number, b: number): number => a + b);
    return "no";
  } catch (err) {
    return (err as Error).name + ": " + (err as Error).message;
  }
}
//## map_and_set_methods
function t(): string {
  const m = new Map<string, number>();
  m.set("a", 1);
  const dm: any = m;
  dm.set("b", 2).set("c", 3);
  const out: string[] = [];
  out.push(String(dm.get("b")) + String(dm.get("zz")) + String(dm.has("a")) + String(dm.has(1)));
  out.push(String(dm.delete("a")) + String(dm.delete("a")) + String(dm.size));
  const listed: string[] = [];
  for (const k of dm.keys()) listed.push(k);
  for (const v of dm.values()) listed.push(String(v));
  for (const e of dm.entries()) listed.push(JSON.stringify(e));
  out.push(listed.join(";"));
  const seen: string[] = [];
  dm.forEach((v: number, k: string): void => {
    seen.push(k + "=" + v);
  });
  out.push(seen.join(","));
  const s = new Set<number>([3, 1]);
  const ds: any = s;
  ds.add(2).add(3);
  out.push(String(ds.has(2)) + String(ds.has(9)) + String(ds.size) + String(ds.delete(1)));
  const vals: number[] = [];
  ds.forEach((v: number): void => {
    vals.push(v);
  });
  out.push(vals.join(","));
  ds.clear();
  dm.clear();
  out.push(String(ds.size) + String(dm.size));
  return out.join("|");
}
//## number_boolean_and_function_methods
function t(): string {
  const n: any = 3.14159;
  const i: any = 255;
  const b: any = true;
  const f: any = (a: number, c: number): number => a - c;
  const out: any[] = [
    n.toFixed(2),
    n.toFixed(0),
    i.toString(16),
    i.toString(2),
    i.toString(),
    (-0.5).toFixed(1),
    b.toString(),
    f.call(null, 10, 3),
    f.apply(null, [10, 4]),
  ];
  return out.map((v: any): string => String(v)).join(",");
}
//## for_of_each_iterable_kind
function t(): string {
  const out: string[] = [];
  const nums: any = [1, 2, 3];
  for (const x of nums) out.push(String((x as number) * 2));
  const strs: any = ["a", "b"];
  for (const x of strs) out.push(x);
  const text: any = "h👋i";
  for (const ch of text) out.push(`[${ch}]`);
  const m = new Map<string, number>([["k", 1], ["l", 2]]);
  const dm: any = m;
  for (const e of dm) out.push(JSON.stringify(e));
  const s = new Set<string>(["x", "y"]);
  const dset: any = s;
  for (const v of dset) out.push(v);
  const mixed: unknown = [1, "two", null, [3]];
  for (const v of mixed as any) out.push(typeof v);
  const empty: any = [];
  for (const v of empty) out.push(`never${v}`);
  const grow: any = [1];
  for (const v of grow) {
    if ((grow.length as number) < 4) grow.push((v as number) + 1);
    out.push(`g${v}`);
  }
  return out.join(",");
}
//## for_of_type_errors
function attempt(v: any): string {
  try {
    for (const x of v) {
      return "iterated " + String(x);
    }
    return "empty";
  } catch (e) {
    return (e as Error).name + ": " + (e as Error).message;
  }
}
function t(): string {
  return [attempt(null), attempt(undefined), attempt(5), attempt(true), attempt(JSON.parse('{"a":1}'))].join("|");
}
//## not_a_function_errors
function attempt(f: () => void): string {
  try {
    f();
    return "ok";
  } catch (e) {
    return String(e);
  }
}
function t(): string {
  const v: any = 5;
  const s: any = "str";
  const o: any = JSON.parse('{"a": 1}');
  const u: any = undefined;
  const n: any = null;
  const arr: any = [1];
  return [
    attempt((): void => v(1, 2)),
    attempt((): void => s()),
    attempt((): void => o.m(1)),
    attempt((): void => o.a(1)),
    attempt((): void => u.m()),
    attempt((): void => n.m()),
    attempt((): void => arr.nope()),
    attempt((): void => s.nope()),
    attempt((): void => v.nope()),
    attempt((): void => arr.map(3)),
  ].join("\n");
}
//## optional_calls_on_any
function t(): string {
  const f: any = undefined;
  const g: any = (x: number): number => x + 1;
  const o: any = null;
  const p: any = JSON.parse('{"list": [1, 2]}');
  return [String(f?.(1)), String(g?.(1)), String(o?.m(1)), String(p?.list.join("+"))].join(",");
}
//## many_dynamic_calls_checksum
function t(): string {
  const inc: any = (x: number, k: number): number => (x + k * 7) % 1000003;
  const store: any[] = [];
  let acc = 0;
  for (let i = 0; i < 20000; i++) {
    acc = inc(acc, i);
    if (i % 1000 === 0) store.push((y: number): number => y + i);
  }
  let extra = 0;
  for (const f of store) extra += f(1) as number;
  const words: any = "a b c d e f g h".split(" ");
  let joined = 0;
  for (let i = 0; i < 2000; i++) {
    const s: string = words.map((w: string): string => w + i).join("");
    joined += s.length;
  }
  return acc + " " + extra + " " + joined;
}
//## gc_closures_kept_only_through_any_slots
// Each closure (and the array it captures) is reachable only through a dynamic slot: a record,
// an `any[]`, an `any` field, a map value. Garbage made in between forces collections (every
// allocation collects in the stress mode).
class Holder {
  slot: any = null;
}
function make(tag: string, n: number): any {
  const data: number[] = [];
  for (let i = 0; i < n; i++) data.push(i);
  return (k: number): string => tag + ":" + (data[k]! + data.length);
}
function churn(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    const junk = [i, i + 1, i + 2];
    total += ("x" + i).length + junk.length;
  }
  return total;
}
function t(): string {
  const rec: Record<string, any> = {};
  const list: any[] = [];
  const holder = new Holder();
  const map = new Map<string, any>();
  for (let i = 0; i < 20; i++) {
    rec["r" + i] = make("rec" + i, i + 1);
    list.push(make("list" + i, i + 2));
    map.set("m" + i, make("map" + i, i + 3));
    churn(50);
  }
  holder.slot = make("field", 5);
  churn(500);
  const out: string[] = [];
  for (let i = 0; i < 20; i += 7) {
    out.push(rec["r" + i](0), list[i](1), map.get("m" + i)(2));
  }
  out.push(holder.slot(4));
  return out.join(",") + " " + churn(10);
}
//## gc_adapters_survive_collections
function t(): string {
  const fs: ((a: number) => number)[] = [];
  for (let i = 0; i < 30; i++) {
    const offset = [i * 10];
    const loose: any = (a: any): any => (a as number) + offset[0]!;
    // A function of another signature becomes an adapter around the dynamic value.
    fs.push(loose as (a: number) => number);
    const junk: string[] = [];
    for (let j = 0; j < 20; j++) junk.push("j" + j);
  }
  let sum = 0;
  for (let i = 0; i < fs.length; i++) {
    const junk = [i, i];
    sum += fs[i]!(junk.length);
  }
  return String(sum);
}
//## gc_arguments_survive_a_collection_in_the_callee
class Box3 {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
}
function t(): string {
  // Unboxing the first argument copies the `any[]` into a `number[]` (an allocation, so a
  // collection in the stress mode) while the others are only in the caller's argument slots.
  const f: any = (xs: number[], s: string, b: Box3, tail: string[]): string => {
    const junk: string[] = [];
    for (let i = 0; i < 30; i++) junk.push("k" + i);
    return xs.join("+") + " " + s + " " + b.v + " " + tail.join("") + " " + junk.length;
  };
  const out: string[] = [];
  for (let i = 0; i < 5; i++) {
    out.push(f([i, i + 1], "s" + i, new Box3(i * 3), ["t", String(i)]));
  }
  return out.join("|");
}
//## long_loop_boxes_calls_and_allocates
function t(): string {
  const pick: any = (xs: any, i: number): any => xs[i % 3];
  const keep: any[] = [];
  let total = 0;
  let text = 0;
  for (let i = 0; i < 100000; i++) {
    const v = pick([i, "s" + (i % 10), null], i);
    if (typeof v === "number") total += v;
    else if (typeof v === "string") text += v.length;
    if (i % 10000 === 0) keep.push(pick(["first", "second", "third"], i));
  }
  return total + " " + text + " " + keep.join(",");
}
//## million_dynamic_calls_checksum
function t(): string {
  const step: any = (x: number, i: number): number => (x * 31 + i) % 65521;
  let h = 7;
  for (let i = 0; i < 1000000; i++) h = step(h, i);
  const fold: any = (a: number, b: number): number => (a ^ b) >>> 0;
  let x = 0;
  for (let i = 0; i < 1000; i++) x = fold(x, i * 2654435761);
  return h + " " + x;
}
//## typed_map_set_keeps_its_key_across_a_collection
// Typed code (found by the GC tests above): `m.set(key, value)` where making the value collects
// once held the new key string in no root.
function mk(n: number): number[] {
  const a: number[] = [];
  for (let i = 0; i < n; i++) a.push(i);
  return a;
}
function t(): string {
  const m = new Map<string, number[]>();
  for (let i = 0; i < 10; i++) m.set("m" + i, mk(i + 1));
  const r: Record<string, number[]> = { ["k" + m.size]: mk(3) };
  const out: string[] = [];
  for (let i = 0; i < 10; i++) out.push(String(m.get("m" + i)?.length));
  return out.join(",") + " " + Object.keys(r).join("");
}
