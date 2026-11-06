// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Partial / Required / Pick / Omit / Readonly for tests/lang/utility.rs. Each case (split at
// `//## name`) defines `function t(): string`; the test compiles it with web_ui_ts (normal,
// GC-stress, GC-off) and compares the result with bun's (`bun bench/gen_cases_expected.ts
// utility_cases.ts`).
//## partial_default_param
interface Opts {
  a: number;
  b: string;
  c?: boolean;
}
function f(o: Partial<Opts> = {}): string {
  return (o.a ?? 1) + "/" + (o.b ?? "x") + "/" + (o.c ?? false);
}
function t(): string {
  return [f(), f({ a: 5 }), f({ b: "q", c: true }), f({ a: 0, b: "" })].join(" ");
}
//## partial_from_full_value
interface P {
  x: number;
  y: number;
  name: string;
}
function describe(p: Partial<P>): string {
  return (p.x === undefined ? "-" : "" + p.x) + (p.y === undefined ? "-" : "" + p.y) + (p.name ?? "?");
}
function t(): string {
  const full: P = { x: 1, y: 2, name: "n" };
  const half: Partial<P> = { y: 7 };
  return describe(full) + " " + describe(half) + " " + describe({});
}
//## pick_omit
interface Ctx {
  sfx: (n: string) => string;
  shake: number;
  smoke: boolean;
  other: string;
}
type Host = Pick<Ctx, "sfx" | "shake">;
type NoOther = Omit<Ctx, "other" | "sfx">;
function use(h: Host): string {
  return h.sfx("s") + h.shake;
}
function rest(n: NoOther): string {
  return "" + n.shake + n.smoke;
}
function t(): string {
  const c: Ctx = { sfx: (n: string) => n + "!", shake: 3, smoke: true, other: "o" };
  return use(c) + " " + rest(c) + " " + use({ sfx: (n: string) => n + "?", shake: 1 });
}
//## required_and_readonly
interface Cfg {
  size?: number;
  label?: string;
}
function sized(c: Required<Cfg>): string {
  return c.label + c.size;
}
function ro(c: Readonly<Cfg>): number {
  return c.size ?? -1;
}
function t(): string {
  const c: Cfg = { size: 4, label: "L" };
  return sized({ size: 2, label: "two" }) + " " + ro(c) + ro({}) + " " + sized({ size: c.size ?? 0, label: c.label ?? "" });
}
//## partial_of_class
class Box {
  w = 1;
  h = 2;
  title = "box";
  area(): number {
    return this.w * this.h;
  }
}
function patch(b: Box, p: Partial<Box>): void {
  if (p.w !== undefined) b.w = p.w;
  if (p.h !== undefined) b.h = p.h;
  if (p.title !== undefined) b.title = p.title;
}
function t(): string {
  const b = new Box();
  patch(b, { w: 5 });
  const a = b.area();
  patch(b, { h: 3, title: "t" });
  return a + " " + b.area() + " " + b.title;
}
//## spread_partial_in_literals
interface Style {
  width: number;
  color: string;
  loop?: boolean;
}
const BASE = { width: 0.05, color: "ink" };
function make(look: Partial<Style>): Style {
  return { ...BASE, ...look };
}
function twice(look: Partial<Style>): string {
  const s = make({ loop: true, width: 1, ...look });
  return s.width + s.color + (s.loop ?? false);
}
function t(): string {
  return twice({}) + " " + twice({ color: "red" }) + " " + twice({ width: 9, loop: false });
}
//## partial_records_and_arrays
type Sfx = "boom" | "pop" | "ding";
const FILE: Partial<Record<Sfx, string>> = { boom: "mp3" };
const CUT: Partial<Record<Sfx, number>> = { pop: 6 };
function ext(s: Sfx): string {
  return FILE[s] ?? "wav";
}
function cut(s: Sfx): number {
  return CUT[s] ?? 0;
}
function t(): string {
  const all: Partial<{ a: number; b: number }>[] = [{ a: 1 }, { b: 2 }, { a: 3, b: 4 }, {}];
  let sum = 0;
  for (const p of all) sum += (p.a ?? 10) + (p.b ?? 100);
  return ext("boom") + ext("pop") + cut("pop") + cut("ding") + " " + sum;
}
//## pick_of_an_interface_reached_through_its_own_members
interface Ctx {
  sfx: (n: string) => string;
  shake: number;
  scene: Effects;
}
interface Effects {
  host: Host;
  name: string;
}
type Host = Pick<Ctx, "sfx" | "shake">;
function useHost(h: Host): string {
  return h.sfx("s") + h.shake;
}
function t(): string {
  const host: Host = { sfx: (n: string) => n + "!", shake: 4 };
  const fx: Effects = { host, name: "fx" };
  const ctx: Ctx = { sfx: (n: string) => n + "?", shake: 7, scene: fx };
  return useHost(ctx) + " " + useHost(fx.host) + " " + ctx.scene.name;
}
