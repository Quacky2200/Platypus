// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Language leftovers for tests/lang/lang2.rs: `for...in` over arrays, `Object.assign` into
// dictionaries and objects, setters in object literals, narrowing across closures, `in` and
// `instanceof` narrowing of unions, truthiness narrowing, `flat(depth)`. Each case (split at
// `//## name`) defines `function t(): string`; the test compares what web_ui_ts gives with bun's
// (`bun bench/gen_cases_expected.ts lang2_cases.ts`).
//## for_in_over_arrays
function t(): string {
  const xs = [10, 20, 30];
  let a = "";
  for (const i in xs) a += i + "=" + xs[Number(i)] + ";";
  const keys: string[] = [];
  for (const k in ["x", "y"]) keys.push(k);
  let n = 0;
  for (const i in xs) {
    if (i === "1") continue;
    n += Number(i);
  }
  const empty: number[] = [];
  let e = 0;
  for (const _ in empty) e++;
  let concat = "";
  for (const i in xs) concat += i + 1;
  return a + keys.join("|") + n + e + typeof keys[0] + concat;
}
//## object_assign_into_dictionaries_and_objects
function t(): string {
  const d: Record<string, number> = { a: 1 };
  Object.assign(d, { b: 2, c: 3 });
  const e: { [k: string]: number } = {};
  Object.assign(e, d, { z: 26 });
  const o = { x: 1, y: 2 };
  Object.assign(o, { y: 20 }, { x: 10 });
  const r = Object.assign(d, { a: 100 });
  const f: Record<string, number> = {};
  Object.assign(f, e);
  Object.assign(f, { z: 0 });
  return (
    Object.keys(d).join(",") +
    "/" +
    Object.values(d).join(",") +
    "/" +
    Object.keys(e).join(",") +
    "/" +
    o.x +
    o.y +
    (r === d) +
    "/" +
    Object.values(f).join(",")
  );
}
//## setters_in_object_literals
function t(): string {
  let store = 1;
  const o = {
    get v(): number {
      return store * 2;
    },
    set v(n: number) {
      store = n + 1;
    },
    plain: 5,
  };
  o.v = 10;
  const a = o.v;
  o.v += 1;
  return a + "," + o.v + "," + store + "," + o.plain;
}
//## narrowing_across_closures
type U = number | string;
function make(u: U): () => string {
  if (typeof u === "number") {
    return () => "n" + (u + 1);
  }
  return () => "s" + u.toUpperCase();
}
function t(): string {
  const v: U = Math.random() < 2 ? 5 : "x";
  const w: U = Math.random() < 2 ? "hello" : 1;
  let out = "";
  if (typeof v === "number") {
    const f = () => v * 2;
    out += f();
    out += [1, 2, 3].reduce((acc, k) => acc + k * v, 0);
  }
  if (typeof w === "string") {
    out += [1].map(() => w.length).join("");
    const g = function () {
      return w.toUpperCase();
    };
    out += g();
  }
  return out + make(3)() + make("ab")();
}
//## narrowing_across_closures_stops_at_a_reassignment
type U = number | string;
function t(): string {
  let v: U = Math.random() < 2 ? 5 : "x";
  let out = "";
  if (typeof v === "number") {
    out += v + 1;
    const f = () => typeof v;
    out += f();
    v = "now a string";
    out += f();
  }
  return out;
}
//## in_narrowing_of_a_union
interface Circle {
  radius: number;
}
interface Rect {
  width: number;
  height: number;
}
type Shape = Circle | Rect;
function area(s: Shape): number {
  if ("radius" in s) return Math.round(3 * s.radius * s.radius);
  return s.width * s.height;
}
function label(s: Shape): string {
  return "width" in s ? "rect" + s.width : "circle" + s.radius;
}
function t(): string {
  const shapes: Shape[] = [{ radius: 2 }, { width: 3, height: 4 }];
  return shapes.map(area).join(",") + shapes.map(label).join(",");
}
//## instanceof_narrowing_of_a_union
class Cat {
  lives = 9;
  meow(): string {
    return "meow" + this.lives;
  }
}
class Dog {
  name = "rex";
  bark(): string {
    return "woof" + this.name;
  }
}
type Pet = Cat | Dog;
function speak(p: Pet): string {
  if (p instanceof Cat) return p.meow();
  return p.bark();
}
function t(): string {
  const pets: Pet[] = [new Cat(), new Dog()];
  const isDog = (p: Pet): boolean => p instanceof Dog;
  return pets.map(speak).join(",") + pets.map(isDog).join(",");
}
//## truthiness_narrowing_of_a_union
type V = number | string | number[] | null;
function show(v: V): string {
  if (!v) return "falsy";
  if (typeof v === "number") return "n" + (v + 1);
  if (typeof v === "string") return "s" + v.length;
  return "a" + v.length;
}
function pick(v: number | string | null): string {
  if (v) {
    return typeof v === "number" ? "num" + v : "str" + v;
  }
  return "none";
}
interface Pt {
  x: number;
}
function empty(v: string | Pt): string {
  if (!v) return "empty" + v.length;
  return typeof v === "string" ? "s" + v : "p" + v.x;
}
function zero(v: number | number[]): string {
  if (v) {
    return typeof v === "number" ? "n" + (v + 1) : "a" + v.length;
  }
  return "zero" + (v + 1);
}
function t(): string {
  const all: V[] = [0, 5, "", "abc", [1, 2], null];
  const more: (number | string | null)[] = [0, 3, "", "q", null];
  const pts: (string | Pt)[] = ["", "ab", { x: 7 }];
  const nums: (number | number[])[] = [0, 4, [1, 2, 3]];
  return (
    all.map(show).join(",") +
    ";" +
    more.map(pick).join(",") +
    ";" +
    pts.map(empty).join(",") +
    ";" +
    nums.map(zero).join(",")
  );
}
//## flat_with_a_run_time_depth
function flatten(xs: number[][][], depth: number): number[] {
  return xs.flat(depth) as number[];
}
function t(): string {
  const xs = [[[1, 2], [3]], [[4]]];
  return flatten(xs, 2).join("") + " " + flatten(xs, 10).join("") + " " + flatten(xs, Infinity).join("");
}
