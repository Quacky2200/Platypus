// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// A typed program for the dynamic-values guard (tests/lang/dynamic.rs): closures, calls of
// closures and methods, callbacks, `for..of` over every typed collection and string methods,
// none of it dynamic. It must compile to the same IR and object code with or without the code
// for calls through `any` (phase P3).

class Shape {
  constructor(public name: string) {}
  area(): number {
    return 0;
  }
  describe(prefix: string): string {
    return prefix + this.name + " " + this.area().toFixed(1);
  }
}

class Square extends Shape {
  constructor(public side: number) {
    super("square");
  }
  area(): number {
    return this.side * this.side;
  }
}

interface Handlers {
  onHit: (n: number) => string;
  label: string;
}

function counter(): (by: number) => number {
  let n = 0;
  return (by: number): number => {
    n += by;
    return n;
  };
}

function apply(f: (a: number, b: number) => number, a: number, b: number): number {
  return f(a, b);
}

export function run(): string {
  const out: string[] = [];
  const shapes: Shape[] = [new Shape("dot"), new Square(3)];
  for (const s of shapes) out.push(s.describe("> "));
  const c = counter();
  c(2);
  out.push(String(c(3)));
  out.push(String(apply((a: number, b: number): number => a * b, 6, 7)));
  const h: Handlers = { onHit: (n: number): string => "hit " + n, label: "L" };
  out.push(h.onHit(4) + h.label);
  const xs = [5, 3, 8, 1];
  let sum = 0;
  xs.forEach((x: number): void => {
    sum += x;
  });
  out.push(String(sum));
  out.push(xs.map((x: number): number => x * 2).filter((x: number): boolean => x > 5).join(","));
  out.push(String(xs.reduce((a: number, b: number): number => a + b, 0)));
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  m.set("c" + xs.length, 3);
  for (const [k, v] of m) out.push(k + "=" + v);
  const set = new Set<string>(["x", "y"]);
  for (const v of set) out.push(v);
  for (const ch of "héllo") out.push(ch.toUpperCase());
  const text = "  a,b,c  ";
  out.push(text.trim().split(",").join("+"), text.indexOf("b").toString(), text.slice(2, 5));
  const fns: ((s: string) => string)[] = [(s: string): string => s + "!", (s: string): string => s + "?"];
  for (const f of fns) out.push(f("x"));
  return out.join("|");
}
