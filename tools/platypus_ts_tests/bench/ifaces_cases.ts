// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Interfaces implemented by classes, for tests/lang/ifaces.rs. Each case (split at `//## name`)
// defines `function t(): string`; the test compiles it with web_ui_ts (normal, GC-stress, GC-off)
// and compares the result with bun's (`bun bench/gen_cases_expected.ts ifaces_cases.ts`).
//## basic_dispatch
interface Shape {
  readonly name: string;
  area(): number;
  scaled(k: number): string;
}
class Circle implements Shape {
  readonly name = "circle";
  constructor(private r: number) {}
  area(): number {
    return 3 * this.r * this.r;
  }
  scaled(k: number): string {
    return "c" + this.r * k;
  }
}
class Square implements Shape {
  name: string;
  constructor(public s: number) {
    this.name = "square" + s;
  }
  area(): number {
    return this.s * this.s;
  }
  scaled(k: number): string {
    return "s" + this.s * k;
  }
}
function total(list: Shape[]): number {
  let t = 0;
  for (const s of list) t += s.area();
  return t;
}
function describe(s: Shape): string {
  return s.name + "=" + s.area() + s.scaled(2);
}
function t(): string {
  const shapes: Shape[] = [new Circle(1), new Square(2), new Circle(2)];
  let cur: Shape = shapes[0]!;
  const first = describe(cur);
  cur = new Square(3);
  const out = shapes.map((s) => describe(s)).join(",");
  return total(shapes) + " " + first + " " + describe(cur) + " " + out + " " + shapes.length;
}
//## object_literals_and_mixed
interface Counter {
  step: number;
  next(): number;
}
class Plus implements Counter {
  step = 2;
  private n = 0;
  next(): number {
    this.n += this.step;
    return this.n;
  }
}
function drive(c: Counter, times: number): string {
  let s = "";
  for (let i = 0; i < times; i++) s += c.next() + ";";
  return s;
}
function make(k: number): Counter {
  let n = 100;
  return {
    step: k,
    next() {
      n += k;
      return n;
    },
  };
}
function t(): string {
  const a = new Plus();
  const b = make(5);
  const all: Counter[] = [a, b, { step: 1, next: () => 7 }];
  let out = "";
  for (const c of all) out += drive(c, 2) + c.step + "|";
  return out;
}
//## extends_and_subclasses
interface Named {
  name: string;
}
interface Animal extends Named {
  legs: number;
  speak(): string;
}
class Base implements Animal {
  name = "base";
  legs = 4;
  speak(): string {
    return "...";
  }
}
class Dog extends Base {
  constructor() {
    super();
    this.name = "dog";
  }
  speak(): string {
    return "woof";
  }
}
class Bird implements Animal {
  name = "bird";
  legs = 2;
  speak(): string {
    return "tweet";
  }
}
function label(n: Named): string {
  return "<" + n.name + ">";
}
function t(): string {
  const zoo: Animal[] = [new Base(), new Dog(), new Bird()];
  const asBase: Base = new Dog();
  const viaBase: Animal = asBase;
  let legs = 0;
  let out = "";
  for (const a of zoo) {
    legs += a.legs;
    out += label(a) + a.speak();
  }
  return out + legs + viaBase.speak() + label(viaBase);
}
//## optional_members_and_instanceof
interface Face {
  readonly name: string;
  update(t: number): string;
  show?(n: number): string;
}
class Plain implements Face {
  name = "plain";
  update(t: number): string {
    return "p" + t;
  }
}
class Fancy implements Face {
  name = "fancy";
  flips = 0;
  update(t: number): string {
    this.flips++;
    return "f" + t;
  }
  show(n: number): string {
    return "show" + n + this.flips;
  }
}
function t(): string {
  const faces: Face[] = [new Plain(), new Fancy(), new Fancy()];
  let out = "";
  for (const f of faces) {
    out += f.update(1);
    out += f.show?.(2) ?? "-";
    if (f instanceof Fancy) out += "(" + f.flips + ")";
  }
  return out;
}
//## writes_through_the_interface
interface Gauge {
  level: number;
  label?: string;
  bump(): void;
}
class Tank implements Gauge {
  level = 1;
  label = "tank";
  bump(): void {
    this.level += 10;
  }
}
class Dial implements Gauge {
  private v = 0;
  get level(): number {
    return this.v;
  }
  set level(x: number) {
    this.v = x * 2;
  }
  bump(): void {
    this.v += 1;
  }
}
function t(): string {
  const gs: Gauge[] = [new Tank(), new Dial(), { level: 5, bump() {} }];
  let out = "";
  for (const g of gs) {
    g.level += 3;
    g.bump();
    out += g.level + (g.label ?? "_") + ",";
  }
  return out;
}
//## closures_and_containers
interface Handler {
  id: number;
  run(x: number): number;
}
class Doubler implements Handler {
  constructor(public id: number) {}
  run(x: number): number {
    return x * 2;
  }
}
class Adder implements Handler {
  constructor(
    public id: number,
    private k: number,
  ) {}
  run(x: number): number {
    return x + this.k;
  }
}
function t(): string {
  const byId = new Map<number, Handler>();
  const list: Handler[] = [];
  for (let i = 0; i < 40; i++) {
    const h: Handler = i % 2 === 0 ? new Doubler(i) : new Adder(i, i);
    byId.set(h.id, h);
    list.push(h);
  }
  const fns = list.map((h) => (x: number) => h.run(x));
  let sum = 0;
  for (const f of fns) sum += f(3);
  const pick = byId.get(7);
  return sum + " " + (pick ? pick.run(1) : -1) + " " + byId.size + " " + list.filter((h) => h.run(10) > 20).length;
}
//## composite_abstract_and_this
interface Node {
  readonly parts: Node[];
  eval(): number;
  describe(depth?: number): string;
}
interface Named {
  label: string;
}
abstract class Base implements Node, Named {
  parts: Node[] = [];
  label = "base";
  abstract eval(): number;
  describe(depth: number = 0): string {
    return "  ".repeat(depth) + this.label + "=" + this.eval();
  }
  register(into: Node[]): Node {
    into.push(this);
    return this;
  }
}
class Lit extends Base {
  constructor(private v: number) {
    super();
    this.label = "lit";
  }
  eval(): number {
    return this.v;
  }
}
class Sum extends Base {
  constructor(parts: Node[]) {
    super();
    this.parts = parts;
    this.label = "sum";
  }
  eval(): number {
    let t = 0;
    for (const p of this.parts) t += p.eval();
    return t;
  }
  describe(depth: number = 0): string {
    return super.describe(depth) + "[" + this.parts.map((p) => p.describe(depth + 1)).join(",") + "]";
  }
}
function names(list: Named[]): string {
  return list.map((n) => n.label).join("+");
}
function t(): string {
  const seen: Node[] = [];
  const a = new Lit(2).register(seen);
  const b = new Lit(5).register(seen);
  const tree = new Sum([a, b, new Sum([new Lit(1), new Lit(1)])]).register(seen);
  return tree.eval() + " " + tree.describe() + " " + seen.length + " " + names([new Lit(1), new Sum([])]);
}
