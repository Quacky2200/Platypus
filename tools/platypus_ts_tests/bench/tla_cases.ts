// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Top-level `await` for tests/lang/tla.rs. Each case (split at `//## name`) is a module that
// exports `function result(): string`; the test compiles it with web_ui_ts, runs the module's code
// and the event loop in the normal, stress GC and collection-off modes, and compares the result
// with bun's (`bun bench/gen_tla_expected.ts`, which runs the module as an async function).
//## ordering_and_variables
let log = "a";
log += "b";
const first = await Promise.resolve(5);
log += "c" + first;
function twice(): number {
  return first * 2;
}
await new Promise<void>((r) => setTimeout(r, 2));
log += "d" + twice();
export function result(): string {
  return log;
}
//## awaits_in_blocks_and_loops
const xs: number[] = [];
for (let i = 0; i < 3; i++) {
  xs.push(await Promise.resolve(i * i));
}
if (xs.length > 2) {
  xs.push(await Promise.resolve(100));
}
let s = "";
for await (const v of [Promise.resolve(1), 2]) s += v;
try {
  await Promise.reject(new Error("boom"));
} catch (e) {
  s += (e as Error).message;
}
export function result(): string {
  return xs.join(",") + s;
}
//## classes_closures_and_a_later_function
class Counter {
  n = 0;
  inc(): void {
    this.n++;
  }
}
const c = new Counter();
const bump = () => c.inc();
bump();
await Promise.resolve();
bump();
const label: string = await new Promise<string>((r) => setTimeout(() => r("L"), 1));
function report(): string {
  return label + c.n;
}
export function result(): string {
  return report();
}
//## promise_all_tuple_at_the_top
async function num(): Promise<number> {
  return 4;
}
async function str(): Promise<string> {
  return "four";
}
const [a, b] = await Promise.all([num(), str()]);
export function result(): string {
  return a + b;
}
