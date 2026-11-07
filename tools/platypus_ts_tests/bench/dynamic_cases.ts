// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Dynamic `any` / `unknown` values for tests/lang/dynamic.rs (phases P0 and P1 of
// docs/research/platypus-dynamic-values.md). Each case (split at `//## name`) defines
// `function t(): string`; the test compiles it with web_ui_ts (normal, GC-stress, GC-off) and
// compares the result with bun's (`bun bench/gen_cases_expected.ts dynamic_cases.ts`). What bun
// does not do (checked conversions) is tested with literal expectations in dynamic.rs.
//## box_and_unbox_round_trips
class P {
  x: number = 3;
}
function id(v: any): any {
  return v;
}
function t(): string {
  const n: number = id(42);
  const s: string = id("hi");
  const b: boolean = id(true);
  const p: P = id(new P());
  const xs: number[] = id([1, 2, 3]);
  const nn: number | null = id(null);
  const ss: string[] = id(["a", "b"]);
  return `${n} ${s} ${b} ${p.x} ${xs.length} ${nn === null} ${ss[1]}`;
}
//## typeof_of_each_kind
function t(): string {
  const items: unknown[] = [1, "a", true, null, undefined, [1], 5n, -0, NaN];
  const out: string[] = [];
  for (let i = 0; i < items.length; i++) out.push(typeof items[i]);
  return out.join(",");
}
//## truthiness
function bit(v: unknown): string {
  return v ? "1" : "0";
}
function t(): string {
  const items: unknown[] = [0, 1, "", "a", NaN, null, undefined, [], false, true, -0, "0", 0.5, -1];
  let out = "";
  for (let i = 0; i < items.length; i++) out += bit(items[i]);
  const e: any = "";
  return out + (!e ? "n" : "y") + (e || "dflt") + ((1 as any) && "both");
}
//## null_and_undefined
function describe(v: unknown): string {
  if (v === null) return "null";
  if (v === undefined) return "undefined";
  if (v == null) return "never";
  return "value";
}
function t(): string {
  const z: any = undefined;
  const nul: any = null;
  const zero: any = 0;
  return (
    describe(null) + "," + describe(undefined) + "," + describe(0) + "," + describe("") + "," +
    String(z ?? 7) + String(nul ?? "n") + String(zero ?? 9) + "," +
    (z == null) + (nul == null) + (zero == null) + (z != null) + (nul === undefined) + (z !== null)
  );
}
//## strict_equality
class K {}
function t(): string {
  const a: any = 1;
  const b: any = 1;
  const s1: any = "x" + "y";
  const s2: any = "xy";
  const nan: any = NaN;
  const z1: any = 0;
  const z2: any = -0;
  const k1: any = new K();
  const k2: any = new K();
  const t1: any = true;
  return [
    a === b, a === 2, a !== 1, s1 === s2, s1 === "xy", nan === nan, nan !== nan, z1 === z2,
    k1 === k1, k1 === k2, t1 === true, t1 === false, a === "1", 5 === a, a === (1 as any),
  ].join(",");
}
//## typeof_narrowing
function f(v: unknown): string {
  if (typeof v === "number") return "n" + (v + 1);
  if (typeof v === "string") return "s" + v.length;
  if (typeof v === "boolean") return v ? "yes" : "no";
  if (typeof v === "bigint") return "big" + v;
  return "other";
}
function g(v: any): string {
  if (typeof v !== "string") return "-1";
  return String(v.length * 2);
}
function t(): string {
  return [f(1), f("abc"), f(true), f(false), f(null), f(undefined), f([1]), f(10n), g("four"), g(4)].join(",");
}
//## instanceof_narrowing
class A {
  a: number = 1;
}
class B extends A {
  b: number = 2;
}
class C {}
function f(v: unknown): string {
  if (v instanceof B) return "B" + v.b;
  if (v instanceof A) return "A" + v.a;
  return "none";
}
function t(): string {
  return [f(new B()), f(new A()), f(new C()), f(5), f(null), f("a")].join(",");
}
//## string_conversions
function t(): string {
  const items: unknown[] = [1, 1.5, "a", true, null, undefined, [1, 2], 5n, NaN, -0, 1e21, 1e-7, [[1], [2, 3]]];
  const out: string[] = [];
  for (let i = 0; i < items.length; i++) {
    const v = items[i];
    out.push(`${v}` + String(v) + ("" + v) + (v + "!"));
  }
  return out.join("|");
}
//## as_any_and_back
class P {
  n: number = 7;
}
function t(): string {
  const a = (5 as any) as number;
  const b = ("x" as unknown) as string;
  const p = (new P() as any) as P;
  const c = [1, 2, 3] as any;
  const d = c as number[];
  const e: any = true;
  return `${a + 1}${b}${p.n}${d.length}${e as boolean}`;
}
//## any_params_returns_fields
class Box {
  v: any = 1;
  tag: any;
}
function pass(a: any, b: any): any {
  return a ? a : b;
}
function t(): string {
  const bx = new Box();
  const out: string[] = [];
  out.push(String(bx.v), String(bx.tag));
  bx.v = "str";
  bx.tag = 2.5;
  out.push(String(bx.v), String(bx.tag));
  bx.v = [1, 2];
  out.push(String(bx.v));
  bx.v = null;
  out.push(String(bx.v));
  out.push(String(pass(0, "zero")), String(pass("one", "never")), String(pass(null, null)));
  return out.join("|");
}
//## any_arrays
function t(): string {
  const xs: any[] = [];
  xs.push(1);
  xs.push("two");
  xs.push(true);
  xs.push(null);
  xs.push(undefined);
  xs.push([4, 5]);
  let out = xs.length + ":";
  for (let i = 0; i < xs.length; i++) out += typeof xs[i] + (i < xs.length - 1 ? "," : "");
  const n: number = xs[0];
  const s: string = xs[1];
  return out + "|" + (n + 1) + s + String(xs[5]) + String(xs[9]);
}
//## mixed_array_of_unknown
function t(): string {
  const xs: unknown[] = [1, "a", [2, 3], true];
  let nums = 0;
  let strs = 0;
  let arrs = 0;
  let other = 0;
  for (let i = 0; i < xs.length; i++) {
    const v = xs[i];
    if (typeof v === "number") nums++;
    else if (typeof v === "string") strs++;
    else if (Array.isArray(v)) arrs += v.length;
    else other++;
  }
  return `${nums}${strs}${arrs}${other}`;
}
//## catch_non_errors
function thrower(k: number): void {
  if (k === 0) throw "text";
  if (k === 1) throw 42;
  if (k === 2) throw new RangeError("r");
  if (k === 3) throw null;
  if (k === 4) throw undefined;
  if (k === 5) throw true;
  throw [1, 2];
}
function describe(e: unknown): string {
  if (typeof e === "string") return "s:" + e;
  if (typeof e === "number") return "n:" + e;
  if (e instanceof RangeError) return "R:" + e.message;
  if (e === null) return "null";
  if (e === undefined) return "undef";
  if (typeof e === "boolean") return "b:" + e;
  return "obj:" + String(e);
}
function t(): string {
  const out: string[] = [];
  for (let k = 0; k < 7; k++) {
    try {
      thrower(k);
    } catch (e) {
      out.push(describe(e));
    }
  }
  return out.join("|");
}
//## catch_error_through_unknown
function t(): string {
  const out: string[] = [];
  try {
    throw new TypeError("bad");
  } catch (e: unknown) {
    out.push(String(e));
    out.push(e instanceof Error ? e.message : "?");
    out.push((e as Error).name);
  }
  try {
    throw "plain";
  } catch (e) {
    out.push(String(e));
  }
  return out.join("|");
}
//## finally_passes_values_through
function t(): string {
  const log: string[] = [];
  try {
    try {
      throw "x";
    } finally {
      log.push("f");
    }
  } catch (e) {
    log.push(String(e));
  }
  try {
    try {
      throw undefined;
    } finally {
      log.push("g");
    }
  } catch (e) {
    log.push(String(e));
  }
  return log.join();
}
//## json_parse_dynamic
function t(): string {
  const v = JSON.parse('{"a":1,"b":[1,2,{"c":null}],"s":"hi","t":true,"3":"three","2":"two","u":"\\u00e9\\ud83d\\ude00"}');
  const out: string[] = [];
  out.push(String(v.a), String(v.b[1]), String(v.s.length), String(v.b[2].c), String(v["3"]));
  out.push(String(v.missing), typeof v.missing, typeof v.b[2].c, typeof v.t, typeof v.b, String(v.u.length));
  out.push(JSON.stringify(v));
  const arr = JSON.parse("[1, 2.5, -3e2, true, null, \"x\", [], {}]");
  out.push(String(arr.length), JSON.stringify(arr), String(arr[1]), typeof arr[6]);
  return out.join("|");
}
//## json_round_trips_and_pretty_print
function t(): string {
  const texts = [
    "null", "true", "0", "-0", "1e3", "\"s\"", "[]", "{}", "[[],{}]", "{\"a\":null}", "{\"a\":[1,{\"b\":[]}]}",
    "{\"b\":1,\"a\":2,\"1\":3}", "\"line\\nbreak \\\"q\\\" \\\\ tab\\t\"", "1.5", "123456789012345680000", "1e-7",
  ];
  const out: string[] = [];
  for (let i = 0; i < texts.length; i++) out.push(JSON.stringify(JSON.parse(texts[i])));
  const v = JSON.parse('{"a":[1,{"b":[]},{}],"c":"d"}');
  out.push(JSON.stringify(v, null, 2));
  out.push(JSON.stringify(v, null, "--"));
  out.push(JSON.stringify(JSON.parse("[]"), null, 2));
  return out.join("\n");
}
//## json_parse_errors
function t(): string {
  const bad = ["", "{", "[1,]", "{\"a\" 1}", "tru", "'a'", "[1 2]", "{\"a\":1,}", "01", "1 2", "\"abc"];
  const out: string[] = [];
  for (let i = 0; i < bad.length; i++) {
    try {
      JSON.parse(bad[i]);
      out.push("ok");
    } catch (e) {
      out.push((e as Error).name + ":" + (e as Error).message);
    }
  }
  return out.join("\n");
}
//## get_of_undefined_throws
function t(): string {
  const out: string[] = [];
  const a: any = undefined;
  const n: any = null;
  const o = JSON.parse('{"x":{"y":1}}');
  try {
    out.push(String(a.b));
  } catch (e) {
    out.push((e as Error).name + ":" + (e as Error).message);
  }
  try {
    out.push(String(n.b));
  } catch (e) {
    out.push((e as Error).name + ":" + (e as Error).message);
  }
  try {
    out.push(String(o.q.r));
  } catch (e) {
    out.push((e as Error).name + ":" + (e as Error).message);
  }
  out.push(String(a?.b), String(n?.b.c), String(o?.x?.y), String(o.x?.["y"]), String(o.zz?.[0]));
  return out.join("\n");
}
//## optional_chaining_and_index_on_any
function t(): string {
  const v: any = JSON.parse('{"list":[10,20,30],"name":"abc","nested":{"deep":{"k":"v"}}}');
  const n: any = null;
  return [
    String(v.list[2]), String(v.list.length), String(v.name.length), String(v.name[1]), String(v.name[10]),
    String(v.nested.deep.k), String(v.nested?.deep?.k), String(n?.x), String(n?.[0]), String(v.list?.[0]),
    String(v.list[5]), String(v["name"]), String(v.nested["deep"]["k"]),
  ].join(",");
}
//## record_of_any
function t(): string {
  const r: Record<string, any> = {};
  r["a"] = 1;
  r["b"] = "x";
  r["c"] = null;
  r["d"] = [1, 2];
  r["e"] = JSON.parse('{"n":[1,2]}');
  const out: string[] = [];
  out.push(String(r["a"]), String(r["b"]), String(r["c"]), String(r["d"]), String(r["zzz"]), typeof r["zzz"]);
  out.push(JSON.stringify(r));
  out.push(String("a" in r), String("zzz" in r), Object.keys(r).join("+"));
  let keys = "";
  for (const k in r) keys += k;
  out.push(keys);
  const anyR: any = r;
  out.push(String(anyR.b), String(anyR["e"].n[1]));
  const back: Record<string, any> = anyR;
  out.push(String(back["a"]));
  return out.join("|");
}
//## record_of_unknown_from_json
function t(): string {
  const r: Record<string, unknown> = JSON.parse('{"x":1,"y":"two","z":[3]}');
  const out: string[] = [];
  for (const k in r) {
    const v = r[k];
    if (typeof v === "number") out.push(k + "=n" + v);
    else if (typeof v === "string") out.push(k + "=s" + v);
    else if (Array.isArray(v)) out.push(k + "=a" + v.length);
  }
  return out.join(",");
}
//## any_in_objects_and_classes_through_generics
function first<T>(xs: T[]): T {
  return xs[0];
}
function wrap<T>(v: T): T[] {
  return [v];
}
function t(): string {
  const a: any[] = [1, "b"];
  const x = first(a);
  const y = first<any>(wrap<any>("w"));
  const z = first(wrap(5));
  return `${x}${y}${z}`;
}
//## json_stringify_of_other_dynamic_values
function t(): string {
  // (`JSON.stringify(undefined)` is `undefined`; here it is `null`, see docs/differences.md.)
  const items: any[] = [1, "two", true, null, [1, "x", null], NaN, Infinity];
  const out: string[] = [];
  for (let i = 0; i < items.length; i++) out.push(String(JSON.stringify(items[i])));
  out.push(JSON.stringify(items));
  const holes: any[] = [1, undefined, "z"];
  out.push(JSON.stringify(holes));
  return out.join("|");
}
//## any_survives_collections
let g: any = "global-string";
class H {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
}
function churn(n: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < n; i++) out.push("s" + i);
  return out;
}
function t(): string {
  const hs: H[] = [];
  const dyn: any[] = [];
  const rec: Record<string, any> = {};
  let captured: any = "cap" + 1;
  const f = () => captured;
  for (let i = 0; i < 20; i++) {
    hs.push(new H("v" + i));
    dyn.push([i, "e" + i]);
    rec["k" + i] = "r" + i;
    churn(30);
  }
  captured = "cap" + 2;
  g = "global-" + g;
  churn(50);
  let sum = "";
  for (let i = 0; i < 20; i += 7) sum += String(hs[i].v) + String(dyn[i]) + String(rec["k" + i]);
  return sum + String(f()) + String(g);
}
//## boxing_bool_and_nullable_arrays
function t(): string {
  const flags: boolean[] = [true, false];
  const nums: (number | null)[] = [1, null, 3];
  const a: any = flags;
  const b: any = nums;
  const c: number[] = [4, 5];
  const d: any = c;
  return [String(a), typeof a[0], String(b), String(b[1]), String(d), String(d[1])].join("|");
}
//## new_array_slots_are_undefined
function t(): string {
  const xs = new Array<any>(3);
  xs[1] = "one";
  const out: string[] = [];
  for (let i = 0; i < xs.length; i++) out.push(typeof xs[i]);
  return out.join(",") + JSON.stringify(xs);
}
//## any_in_class_fields_and_json
class Msg {
  kind: string = "m";
  payload: any = null;
  tags: any[] = [];
}
function t(): string {
  const m = new Msg();
  m.payload = "p";
  m.tags.push(1, "two", null);
  const a = JSON.stringify(m);
  m.payload = JSON.parse('{"deep":[1,{"x":true}]}');
  const b = JSON.stringify(m, null, 1);
  const c = JSON.parse('{"kind":"k","payload":{"n":[1,2]},"tags":[1,"a",null,{"z":0}]}') as Msg;
  return a + "|" + b + "|" + c.kind + JSON.stringify(c.payload) + JSON.stringify(c.tags) + String(c.tags[3] === undefined);
}
