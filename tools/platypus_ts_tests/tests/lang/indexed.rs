// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Indexed access types as TypeScript types them: properties of interfaces and aliases, tuple
//! positions, unions of keys (the union of the properties' types), `typeof x["k"]`, and `T[K]` in
//! generic code. Each case runs in the normal, stress GC and collection-off modes; the errors
//! name the line. IDs are in TESTS.md (M9-TS-3526..).
use crate::cases::run;

fn check(src: &str, prefix: &'static str, base: u32, want: &str) {
    let got = run(src, prefix, base).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(got, want);
}

fn error_of(src: &str) -> String {
    match web_ui_ts::lower::lower_source(src) {
        Ok(_) => panic!("expected an error from:\n{src}"),
        Err(e) => e,
    }
}

// M9-TS-3526
//
// `Iface["k"]` and `Alias["k"]` (a type literal, also a literal-union property), nested
// `A["b"]["c"]`, `typeof x["k"]` (`(typeof x)["k"]`) and `typeof a.b.c`, an indexed type as a
// `Record`'s keys.
#[test]
fn properties_of_interfaces_aliases_and_typeof() {
    check(
        r#"
interface Shape { kind: "circle" | "square"; size: number; pos: { x: number; y: number } }
type Pair = { name: string; mode: "on" | "off" };
type Kind = Shape["kind"];
type X = Shape["pos"]["x"];
const base = { label: "b", count: 3 };
function t(): string {
  const k: Kind = "square";
  const m: Pair["mode"] = "off";
  const x: X = 4;
  const n: Pair["name"] = "p";
  const c: typeof base["count"] = base.count + 1;
  const local = { inner: { deep: "d" } };
  const d: typeof local["inner"]["deep"] = local.inner.deep;
  const e: typeof local.inner.deep = "e";
  const kinds: Record<Shape["kind"], number> = { circle: 1, square: 2 };
  return `${k} ${m === "off"} ${x} ${n} ${c} ${d} ${e} ${kinds[k]}`;
}
"#,
        "ix0_",
        970_000,
        "square true 4 p 4 d e 2",
    );
}

// M9-TS-3527
//
// Tuples and arrays: `T[0]` is that element's type, `T[number]` the union of the elements' types
// (one type for an array), `T["length"]` a number.
#[test]
fn tuple_positions_and_element_types() {
    check(
        r#"
type Rec = [string, number, boolean];
type Pt = [number, number];
function t(): string {
  const a: Rec[0] = "a";
  const b: Rec[1] = 2;
  const c: Rec[2] = true;
  const p: Pt[0] = 5;
  const v: Rec[number] = 7;
  const w: Rec[number] = "w";
  const names = ["x", "y"];
  const el: (typeof names)[number] = names[1];
  const len: Rec["length"] = 3;
  return `${a}${b}${c}${p}${el}${len} ${typeof v === "number"} ${typeof w === "string"}`;
}
"#,
        "ix1_",
        971_000,
        "a2true5y3 true true",
    );
}

// M9-TS-3528
//
// A union of keys or `keyof X`: one type when the properties agree, else the union of their types
// (T8); reading `o[k]` with `k` such a union gives that union too.
#[test]
fn unions_of_keys_give_the_union_of_the_property_types() {
    check(
        r#"
interface Row { id: number; name: string; tag: "a" | "b"; score: number }
type Num = Row["id" | "score"];
type Mixed = Row["id" | "name"];
type All = Row[keyof Row];
function pick(r: Row, k: "id" | "name"): Row["id" | "name"] { return r[k]; }
const show = (v: Mixed): string => (typeof v === "number" ? `#${v}` : `'${v}'`);
function t(): string {
  const r: Row = { id: 1, name: "n", tag: "b", score: 9 };
  const n: Num = r.score;
  const m: Mixed = "str";
  const m2: Mixed = 5;
  const a: All = "a";
  return `${n} ${show(m)} ${show(m2)} ${show(pick(r, "id"))} ${show(pick(r, "name"))} ${typeof a}`;
}
"#,
        "ix2_",
        972_000,
        "9 'str' #5 #1 'n' string",
    );
}

// M9-TS-3529
//
// `T[K]` in generic code: `get(o, "x")` is a copy for the key `"x"` (so its result is `T["x"]`),
// also with written type arguments; a generic alias `Get<T, K>`; `T["id"]` of a type parameter.
#[test]
fn type_parameters_index_and_are_indexed() {
    check(
        r#"
interface P { x: number; s: string; on: boolean }
function get<T, K extends keyof T>(o: T, k: K): T[K] { return o[k]; }
type Get<T, K extends keyof T> = T[K];
function idOf<T extends { id: number }>(o: T): T["id"] { return o.id; }
class Store<T> {
  v: T;
  constructor(v: T) { this.v = v; }
  read<K extends keyof T>(k: K): T[K] { return this.v[k]; }
}
function either(p: P, k: "x" | "s"): string {
  const v = get(p, k);
  return typeof v === "number" ? `n${v}` : `s${v}`;
}
function t(): string {
  const p: P = { x: 3, s: "hi", on: true };
  const x: number = get(p, "x");
  const s: string = get(p, "s");
  const on: boolean = get(p, "on");
  const w = get<P, "s">(p, "s");
  const g: Get<P, "x"> = 10;
  const i = idOf({ id: 7, name: "q" });
  const st = new Store<P>(p);
  const r: string = st.read("s");
  return `${x + 1} ${s.length} ${on} ${w} ${g} ${i} ${r} ${either(p, "x")} ${either(p, "s")}`;
}
"#,
        "ix3_",
        973_000,
        "4 2 true hi 10 7 hi n3 shi",
    );
}

// M9-TS-3530
//
// What cannot be typed is an error with the line: a property the type lacks, a tuple position past
// its end, a key parameter whose argument is a `string`, `typeof` of a missing property, and writing
// through a key that names fields of different types.
#[test]
fn indexed_access_errors_name_the_line_and_the_cause() {
    let e = error_of("interface A { a: number }\nexport function f(): A[\"b\"] { return 1; }");
    assert!(
        e.contains("line 2: property \"b\" does not exist on type A"),
        "{e}"
    );
    let e = error_of("export function f(): [number, string][5] { return 1; }");
    assert!(
        e.contains("line 1: tuple type [number, string] of length 2 has no element at index 5"),
        "{e}"
    );
    let e = error_of(
        "interface P { x: number; s: string }\nfunction get<T, K extends keyof T>(o: T, k: K): T[K] { return o[k]; }\nexport function f(key: string): number { const p: P = { x: 1, s: \"\" }; get(p, key); return 0; }",
    );
    assert!(
        e.contains("line 2: indexed access by `K`: which keys `K` is is not known in this call"),
        "{e}"
    );
    let e = error_of(
        "export function f(): number { const o = { a: 1 }; const v: typeof o.b = 1; return v; }",
    );
    assert!(
        e.contains("line 1: property \"b\" does not exist on typeof o"),
        "{e}"
    );
    let e = error_of(
        "interface P { x: number; s: string }\nexport function f(p: P, k: \"x\" | \"s\"): void {\n  p[k] = 1;\n}",
    );
    assert!(
        e.contains("line 3: assigning through a key that names fields of different types"),
        "{e}"
    );
}
