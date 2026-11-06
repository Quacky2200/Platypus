// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! TypeScript the native editor needed (2026-10-29): type guards, unions of interfaces, generic
//! inference from the context, computed keys, maps from pairs and more. Each case runs in the
//! normal, stress GC and collection-off modes. IDs are in TESTS.md (M9-TS-3500..).
use crate::cases::run;

fn check(src: &str, prefix: &'static str, base: u32, want: &str) {
    let got = run(src, prefix, base).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(got, want);
}

// M9-TS-3500
//
// `(x): x is T => ..` returns a boolean; `filter` with one keeps `T`s; `asserts` returns nothing.
#[test]
fn type_guards_return_booleans_and_filter_keeps_the_guarded_type() {
    check(
        r#"
interface Box { n: number }
const isBig = (b: Box | undefined): b is Box => !!b && b.n > 1;
function sure(v: number): asserts v { if (v === 0) throw new Error("zero"); }
function t(): string {
  const m = new Map<string, Box>([["a", { n: 1 }], ["b", { n: 5 }]]);
  const got: Box[] = ["a", "b", "c"].map((k) => m.get(k)).filter((b): b is Box => !!b);
  sure(got.length);
  return `${got.length} ${got[1]!.n} ${isBig(got[0])} ${isBig(got[1])}`;
}
"#,
        "eg0_",
        940_000,
        "2 5 false true",
    );
}

// M9-TS-3501
//
// A union of object types written as literals: `in` tells the members apart, a member is
// picked by the keys an object literal has.
#[test]
fn object_literals_take_the_union_member_their_keys_name() {
    check(
        r#"
type Segment = { text: string } | { text: string; file: string; line: number };
const isLink = (s: Segment): s is Extract<Segment, { file: string }> => "file" in s;
function t(): string {
  const out: Segment[] = [];
  out.push({ text: "see " });
  out.push({ text: "a.ts:3", file: "a.ts", line: 3 });
  return out.map((s) => (isLink(s) ? `[${s.file}:${s.line}]` : s.text)).join("");
}
"#,
        "eg1_",
        941_000,
        "see [a.ts:3]",
    );
}

// M9-TS-3502
//
// Type arguments from the context: a generic call's expected result gives a closure's body its
// type (so closures inside get theirs), an object argument's plain properties fix `T` before its
// closures are typed, and a bare generic interface takes its defaults.
#[test]
fn generic_calls_infer_from_the_expected_result_and_object_arguments() {
    check(
        r#"
interface Panel { title: string; render: (h: number) => string }
function make<T>(f: () => T): T { return f(); }
interface Node<T = number> { id: string; data?: T }
interface ListProps<T> { roots: readonly Node<T>[]; label: (n: Node<T>) => string }
function list<T>(p: ListProps<T>): string { return p.roots.map((r) => p.label(r)).join(","); }
function count(rows: readonly Node[]): number { return rows.length; }
interface Thing { kind: string }
function t(): string {
  const p: Record<string, Panel> = make(() => ({ a: { title: "A", render: (h) => `${h * 2}` } }));
  const roots: Node<Thing>[] = [{ id: "a", data: { kind: "x" } }, { id: "b" }];
  const plain: Node[] = [{ id: "c", data: 3 }];
  return `${p["a"]!.render(21)} ${list({ roots, label: (n) => n.data?.kind ?? "-" })} ${count(plain)}`;
}
"#,
        "eg2_",
        942_000,
        "42 x,- 1",
    );
}

// M9-TS-3503
//
// `Record<K, V>` keyed by `Exclude<U, X>` and by an interface property's type (`Thing["kind"]`).
#[test]
fn records_keyed_by_exclude_and_indexed_access_types() {
    check(
        r#"
type Part = "x" | "y" | "center";
interface Thing { kind: "a" | "b" }
const AXES: Record<Exclude<Part, "center">, number> = { x: 1, y: 2 };
const ICONS: Record<Thing["kind"], string> = { a: "A", b: "B" };
function t(): string {
  const k: Exclude<Part, "center"> = "y";
  const th: Thing = { kind: "b" };
  return `${AXES[k]}${ICONS[th.kind]}`;
}
"#,
        "eg3_",
        943_000,
        "2B",
    );
}

// M9-TS-3504
//
// `{ ...f, [key]: !f[key] }` with `key` a union of field names, and `obj[k]` where `k` is a
// `const` of literals (a conditional of a literal and an interface field).
#[test]
fn computed_keys_and_constants_of_literal_unions_index_objects() {
    check(
        r#"
interface Filter { info: boolean; warn: boolean; search: string }
type Level = "debug" | "info" | "warn";
interface Entry { level: Level }
function t(): string {
  let f: Filter = { info: true, warn: true, search: "" };
  const flip = (key: "info" | "warn") => {
    f = { ...f, [key]: !f[key] };
  };
  flip("warn");
  const e: Entry = { level: "debug" };
  const level = e.level === "debug" ? "info" : e.level;
  return `${f.info} ${f.warn} ${f[level]} ${f.search === ""}`;
}
"#,
        "eg4_",
        944_000,
        "true false true true",
    );
}

// M9-TS-3505
//
// `new Map(pairs)` from an array of `[key, value]` pairs that is not a literal, and template
// parts that are conditionals of different types.
#[test]
fn maps_from_pair_arrays_and_conditionals_in_templates() {
    check(
        r#"
function size(v: number): string { return `${v < 10 ? v.toFixed(1) : Math.round(v)} KB`; }
function t(): string {
  const m = new Map(["a", "b"].map((s, i) => [s, i]));
  const n = 1000;
  return `${[...m.keys()].join(",")} ${m.get("b")} ${n > 999 ? "999+" : n} ${size(2.5)} ${size(12.4)}`;
}
"#,
        "eg5_",
        945_000,
        "a,b 1 999+ 2.5 KB 12 KB",
    );
}

// M9-TS-3506
//
// A union of interfaces with a tag (`DockGroup | DockSplit`): a field every member has, the tag
// narrowing after an early `return`, an object literal or spread taking its member, and
// `JSON.stringify` writing the member the value is.
#[test]
fn unions_of_interfaces_read_common_fields_narrow_and_stringify() {
    check(
        r#"
interface G { type: "group"; id: string; tabs: string[] }
interface S { type: "split"; id: string; children: Item[] }
type Item = G | S;
function ids(n: Item): string {
  if (n.type !== "split") return n.id + "[" + n.tabs.join("") + "]";
  return n.id + "(" + n.children.map((c) => ids(c)).join(",") + ")";
}
function rename(n: Item): Item {
  if (n.type === "group") return n;
  return { ...n, id: n.id + "!" };
}
function t(): string {
  const root: Item = { type: "split", id: "r", children: [{ type: "group", id: "a", tabs: ["x", "y"] }] };
  return ids(rename(root)) + " " + JSON.stringify(root);
}
"#,
        "eg6_",
        946_000,
        r#"r!(a[xy]) {"type":"split","id":"r","children":[{"type":"group","id":"a","tabs":["x","y"]}]}"#,
    );
}

// M9-TS-3507
//
// Closures: a defaulted literal-union parameter may be left out through an interface's optional
// one, `const` arrows of a block may call each other (written types), `Error.stack` is the error's
// first line, and `Promise.resolve` / `reject` take the promise type the context expects.
#[test]
fn defaulted_closures_mutual_arrows_error_stack_and_contextual_promises() {
    check(
        r#"
type Align = "auto" | "end";
interface H { go(i: number, align?: Align): string }
interface L { path: string; n: number }
function list(path: string): Promise<L> {
  return path === "" ? Promise.reject(new Error("no")) : Promise.resolve({ path, n: path.length });
}
function make(): H {
  const go = (i: number, align: Align = "auto"): string => `${i}:${align}`;
  return { go };
}
function t(): string {
  const h = make();
  const even = (n: number): boolean => (n === 0 ? true : odd(n - 1));
  const odd = (n: number): boolean => (n === 0 ? false : even(n - 1));
  let stack = "";
  try {
    throw new RangeError("bad");
  } catch (e) {
    stack = e instanceof Error ? e.stack : "";
  }
  const p = list("abc");
  return `${h.go(1)} ${h.go(2, "end")} ${even(10)} ${odd(7)} ${stack} ${p !== null}`;
}
"#,
        "eg7_",
        947_000,
        "1:auto 2:end true true RangeError: bad true",
    );
}

// M9-TS-3508
//
// `unknown` (and `any`, `never`) are not built yet: the error says so and where, also when a bare
// generic interface uses its `T = unknown` default; with a type argument it compiles.
#[test]
fn unknown_is_a_clear_error_and_a_default_of_unknown_needs_a_type_argument() {
    let bare = "interface Row<T = unknown> { id: string; data?: T }\nexport function n(rows: Row[]): number { return rows.length; }";
    let e = web_ui_ts::lower::lower_source(bare).err().unwrap();
    assert!(e.contains("line 1: `unknown` is not supported yet"), "{e}");
    let typed = "interface Row<T = unknown> { id: string; data?: T }\nexport function n(rows: Row<number>[]): number { return rows.length; }";
    assert!(web_ui_ts::lower::lower_source(typed).is_ok());
}

// M9-TS-3509
//
// A function nothing calls is not lowered by a build (so a construct the compiler lacks in it
// does not fail the build); `check_unused` (`platypus-build build --check-unused`) lowers it too.
#[test]
fn check_unused_lowers_functions_nothing_calls() {
    let src = "function unused(): number { return +new Proxy({}, {}); }\nexport function used(): number { return 1; }";
    assert!(web_ui_ts::lower::lower_source(src).is_ok());
    let e = web_ui_ts::check_unused(src, &[]).err().unwrap();
    assert!(e.contains("line 1"), "{e}");
}

// M9-TS-3510
//
// A `string | undefined` compared with string literals (`d !== "row" && d !== "column"`), also
// after `JSON.parse` into an interface with optional keys.
#[test]
fn optional_strings_compare_with_literals() {
    check(
        r#"
interface Saved { direction?: string; children?: Saved[] }
function bad(o: Saved): boolean {
  const d = o.direction;
  return (d !== "row" && d !== "column") || !o.children;
}
function t(): string {
  const a = JSON.parse('{"direction":"diagonal","children":[]}') as Saved;
  const b = JSON.parse('{"direction":"row","children":[]}') as Saved;
  const c: Saved = { direction: "column" };
  return `${bad(a)} ${bad(b)} ${bad(c)} ${bad({})}`;
}
"#,
        "eg10_",
        950_000,
        "true false true true",
    );
}

// M9-TS-3511
//
// `return bad()` where `bad` returns `null` (typed `null`) and sets a flag: the call still runs
// (it used to be replaced by the null it gives).
#[test]
fn a_call_that_returns_null_still_runs() {
    check(
        r#"
function parse(x: number): string {
  let broken = false;
  const bad = (): null => {
    broken = true;
    return null;
  };
  const read = (n: number): number | null => {
    if (n < 0) return bad();
    return n;
  };
  const v = read(x);
  return broken ? "broken" : `${v}`;
}
function t(): string {
  return `${parse(-1)} ${parse(2)}`;
}
"#,
        "eg11_",
        951_000,
        "broken 2",
    );
}

// M9-TS-3512
//
// Top-level statements: a closure inside a callback passed at the top level shares the
// callback's variables (`test("x", () => { let s; const key = () => { s = .. } })`); a generic
// call's object literal argument takes the type an earlier argument fixed (`eq(range, { .. })`);
// `filter` with a guard picks members out of a union of interfaces.
#[test]
fn top_level_callbacks_contextual_literals_and_union_filters() {
    check(
        r#"
interface Range { start: number; end: number }
interface G { type: "group"; id: string }
interface S { type: "split"; id: string; children: Item[] }
type Item = G | S;
let out = "";
function test(name: string, f: () => void): void { f(); }
function eq<T>(got: T, want: T): string { return JSON.stringify(got) === JSON.stringify(want) ? "ok" : "no"; }
test("x", () => {
  let s = 1;
  const key = (k: number): number => {
    s = s + k;
    return s;
  };
  key(2);
  const r: Range = { start: 1, end: 3 };
  const items: Item[] = [{ type: "group", id: "a" }, { type: "split", id: "b", children: [] }, { type: "group", id: "c" }];
  const groups = items.filter((n): n is G => n.type === "group");
  out = `${s} ${eq(r, { start: 1, end: 3 })} ${eq(r, { start: 0, end: 3 })} ${groups.map((g) => g.id).join("")}`;
});
function t(): string {
  return out;
}
"#,
        "eg12_",
        952_000,
        "3 ok no ac",
    );
}

// M9-TS-3513
//
// Settles docs/platypus-ts.md, which listed them as unsupported: generic classes and functions,
// and destructuring assignment to existing variables, compile and run.
#[test]
fn generics_and_destructuring_assignment_work() {
    check(
        r#"
class Box<T> {
  constructor(public value: T) {}
  map<U>(f: (v: T) => U): Box<U> { return new Box(f(this.value)); }
}
function first<T>(xs: T[]): T | undefined { return xs[0]; }
function t(): string {
  const b = new Box(20).map((n) => `${n + 1}`);
  let a = 1;
  let c = 2;
  [a, c] = [c, a];
  let x = 0;
  let y = 0;
  ({ x, y } = { x: 5, y: 6 });
  return `${b.value} ${first(["p", "q"])} ${a}${c} ${x}${y}`;
}
"#,
        "eg13_",
        953_000,
        "21 p 21 56",
    );
}

// M9-TS-3514
//
// `f?.(x)` on a callback that is left out does nothing, also when a closure captured `f` (a
// destructured optional prop, as the editor's VirtualList row handler).
#[test]
fn optional_call_of_a_captured_missing_callback_does_nothing() {
    check(
        r#"
interface Props { n: number; onPick?: (i: number) => void; other?: number }
function make({ n, onPick, other = 2 }: Props): (() => void)[] {
  const out: (() => void)[] = [];
  for (let i = 0; i < n; i++) out.push(() => onPick?.(i + other));
  return out;
}
function plain(f?: (i: number) => void): () => void {
  return () => f?.(1);
}
function t(): string {
  let got = "";
  for (const h of make({ n: 2 })) h();
  plain()();
  for (const h of make({ n: 2, onPick: (i) => { got += `${i};`; } })) h();
  plain((i) => { got += `p${i}`; })();
  const twice = (g?: (n: number) => number): number | undefined => g?.(4);
  return `${got} ${twice() === undefined} ${twice((n) => n * 2)}`;
}
"#,
        "eg14_",
        954_000,
        "2;3;p1 true 8",
    );
}

// M9-TS-3515
//
// Reversing an empty array (one with no storage yet, as `[...noRoots].reverse()` in the editor's
// Scene view) leaves it empty; a debug build checked the slice it made over no storage.
#[test]
fn an_empty_array_reverses_to_an_empty_array() {
    check(
        r#"
function t(): string {
  const none: number[] = [];
  const spread = [...new Set<string>()].reverse();
  none.reverse();
  const one = [7].reverse();
  return `${none.length} ${spread.length} ${one[0]}`;
}
"#,
        "eg15_",
        955_000,
        "0 0 7",
    );
}

// M9-TS-3524
//
// `cond && f()` with a void `f`, as an arrow's body or a statement: `f` runs when `cond` holds.
#[test]
fn and_with_a_void_call_runs_the_call() {
    check(
        r#"
let n = 0;
function bump(): void { n++; }
function t(): string {
  const on = true;
  const off = false;
  const click = () => !off && bump();
  click();
  on && bump();
  off && bump();
  const f: () => void = () => on && bump();
  f();
  const g = (): void => { !on || bump(); };
  g();
  return `${n}`;
}
"#,
        "eg16_",
        956_000,
        "4",
    );
}

// M9-TS-3531
//
// `new Set(cond ? set : [])` and `new Map(cond ? [] : map)`: the argument is any iterable, so
// the arms may be a collection and an empty array; the test runs once, then only the arm taken.
#[test]
fn a_collection_from_a_conditional_of_a_collection_and_an_array() {
    check(
        r#"
let tests = 0;
function on(v: boolean): boolean { tests++; return v; }
function t(): string {
  const s = new Set<string>(["a"]);
  const n = new Set(on(true) ? s : []);
  const e = new Set(on(true) ? [] : s);
  const m = new Map<string, number>([["k", 1]]);
  const mm = new Map(on(false) ? [] : m);
  const typed = new Set<string>(on(false) ? s : ["x", "y"]);
  n.add("b");
  return `${n.size} ${s.size} ${e.size} ${mm.get("k")} ${typed.size} ${tests}`;
}
"#,
        "eg17_",
        957_000,
        "2 1 0 1 2 4",
    );
}
