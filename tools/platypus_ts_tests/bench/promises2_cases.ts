// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Promise programs for tests/lang/promises2.rs. Each case (split at `//## name`) defines
// `async function t(): Promise<string>`; the test compiles it with web_ui_ts (in the normal, GC-stress
// and GC-off modes) and compares the result with bun's (`gen_promises2_expected.ts`).
//## generic_identity
async function load<T>(x: T): Promise<T> {
  return x;
}
async function t(): Promise<string> {
  const a = await load<number>(3);
  const b = await load<string>("s");
  const c = await load(true);
  return a + b + c;
}
//## generic_closure_result
async function wrap<T>(f: () => T): Promise<T> {
  return f();
}
async function t(): Promise<string> {
  const a = await wrap(() => 5);
  const b = await wrap(() => "five");
  return a + b;
}
//## generic_array_result
async function twice<T>(x: T): Promise<T[]> {
  return [x, x];
}
async function t(): Promise<string> {
  const a = await twice<number>(3);
  const b = await twice("ab");
  return a.join(",") + "|" + b.join(",");
}
//## generic_map_result
let seed: Promise<Map<string, number>> | null = null;
async function table<T>(key: string, v: T): Promise<Map<string, T>> {
  const m = new Map<string, T>();
  m.set(key, v);
  return m;
}
async function t(): Promise<string> {
  const m = await table("k", 7);
  seed = null;
  return "" + m.get("k") + m.size;
}
//## generic_delay
function delay<T>(ms: number, v: T): Promise<T> {
  return new Promise<T>((resolve) => {
    setTimeout(() => {
      resolve(v);
    }, ms);
  });
}
async function t(): Promise<string> {
  const a = await delay(5, 42);
  const b = await delay(1, "x");
  return a + b;
}
//## generic_retry
async function retry<T>(f: () => Promise<T>, n: number): Promise<T> {
  for (let i = 0; i < n; i++) {
    try {
      return await f();
    } catch (e) {
      if (i === n - 1) throw e;
    }
  }
  throw new Error("none");
}
let calls = 0;
async function flaky(): Promise<number> {
  calls++;
  if (calls < 3) throw new Error("no " + calls);
  return calls;
}
async function alwaysFails(): Promise<string> {
  throw new Error("never");
}
async function t(): Promise<string> {
  const a = await retry(flaky, 5);
  let msg = "";
  try {
    await retry(alwaysFails, 2);
  } catch (e) {
    msg = (e as Error).message;
  }
  return a + ":" + msg;
}
//## generic_then_chain
function twice<T>(p: Promise<T>): Promise<T[]> {
  return p.then((v) => [v, v]);
}
async function t(): Promise<string> {
  const a = await twice(Promise.resolve(2));
  const b = await twice(Promise.resolve("z"));
  return a.join("-") + "/" + b.join("-");
}
//## generic_all_map
async function each<T, U>(xs: T[], f: (x: T) => Promise<U>): Promise<U[]> {
  return Promise.all(xs.map(f));
}
async function dbl(x: number): Promise<number> {
  return x * 2;
}
async function name(x: number): Promise<string> {
  return "n" + x;
}
async function t(): Promise<string> {
  const a = await each([1, 2, 3], dbl);
  const b = await each([4, 5], name);
  return a.join(",") + "|" + b.join(",");
}
//## generic_class_with_promise
class Box<T> {
  p: Promise<T>;
  constructor(v: T) {
    this.p = Promise.resolve(v);
  }
  async get(): Promise<T> {
    return await this.p;
  }
}
async function t(): Promise<string> {
  const b = new Box<number>(7);
  const s = new Box<string>("s");
  return (await b.get()) + (await s.get());
}
//## generic_with_class_value
class Foo {
  n = 1;
}
async function make<T>(f: () => T): Promise<T> {
  return f();
}
async function t(): Promise<string> {
  const a = await make(() => new Foo());
  const b = await make<Foo>(() => new Foo());
  return "" + (a.n + b.n);
}
//## generic_recursive
async function sumAll<T>(xs: T[], f: (x: T) => Promise<number>): Promise<number> {
  if (xs.length === 0) return 0;
  const head = await f(xs[0]!);
  return head + (await sumAll(xs.slice(1), f));
}
async function sq(x: number): Promise<number> {
  await Promise.resolve();
  return x * x;
}
async function t(): Promise<string> {
  return "" + (await sumAll([1, 2, 3, 4], sq));
}
//## class_async_methods
class Worker {
  base: number;
  log = "";
  constructor(base: number) {
    this.base = base;
  }
  async run(n: number): Promise<number> {
    await Promise.resolve();
    this.log += "r" + n;
    return this.base + n;
  }
  async both(): Promise<number> {
    const a = await this.run(1);
    const b = await this.run(2);
    return a * b;
  }
}
async function t(): Promise<string> {
  const w = new Worker(10);
  const v = await w.both();
  return v + ":" + w.log;
}
//## interface_and_union_results
interface P {
  x: number;
  tag: string;
}
async function mk(): Promise<P> {
  return { x: 4, tag: "p" };
}
async function maybe(f: boolean): Promise<number | null> {
  return f ? 1 : null;
}
async function t(): Promise<string> {
  const p = await mk();
  const a = await maybe(true);
  const b = await maybe(false);
  return p.x + p.tag + a + b;
}
//## try_catch_finally_with_awaits
let log = "";
async function t(): Promise<string> {
  try {
    await Promise.resolve(1);
    throw new Error("x");
  } catch (e) {
    log += "c" + (e as Error).message;
    await Promise.resolve(2);
  } finally {
    await Promise.resolve(3);
    log += "f";
  }
  try {
    log += "t";
  } finally {
    await Promise.resolve(4);
    log += "g";
  }
  return log;
}
//## for_await_array_of_promises
async function num(n: number): Promise<number> {
  await Promise.resolve();
  return n;
}
async function t(): Promise<string> {
  let s = "";
  for await (const x of [num(1), num(2), num(3)]) {
    if (x === 2) continue;
    s += x;
  }
  for await (const y of [10, 20, 30]) {
    s += "," + y;
    if (y === 20) break;
  }
  return s;
}
//## with_resolvers
async function t(): Promise<string> {
  const r = Promise.withResolvers<number>();
  setTimeout(() => {
    r.resolve(5);
  }, 2);
  const v = await r.promise;
  const q = Promise.withResolvers<string>();
  q.reject(new Error("no"));
  let msg = "";
  try {
    await q.promise;
  } catch (e) {
    msg = (e as Error).message;
  }
  const w = Promise.withResolvers<void>();
  w.resolve();
  await w.promise;
  return v + msg;
}
//## awaiting_values_and_iifes
async function t(): Promise<string> {
  const a = await 5;
  const b = await (async () => 6)();
  const c = await Promise.resolve("c");
  await Promise.resolve();
  return a + ":" + b + ":" + c;
}
//## timers_order_inside_async
let log = "";
function sleep(ms: number): Promise<void> {
  return new Promise<void>((resolve) => {
    setTimeout(() => {
      resolve();
    }, ms);
  });
}
async function slow(name: string, ms: number): Promise<string> {
  await sleep(ms);
  log += name;
  return name;
}
async function t(): Promise<string> {
  const a = slow("a", 30);
  const b = slow("b", 10);
  const c = slow("c", 20);
  const all = await Promise.all([a, b, c]);
  const first = await Promise.race([slow("x", 9), slow("y", 3)]);
  return all.join("") + log + first;
}
//## rejection_paths
async function bad(n: number): Promise<number> {
  await Promise.resolve();
  throw new Error("bad" + n);
}
async function t(): Promise<string> {
  let s = "";
  try {
    await bad(1);
  } catch (e) {
    s += (e as Error).message;
  }
  const r = await bad(2).catch((e) => -1);
  const f = await bad(3)
    .finally(() => {
      s += "F";
    })
    .catch((e) => -2);
  const settled = await Promise.allSettled([bad(4), Promise.resolve(5)]);
  s += settled[0]!.status + settled[1]!.status;
  return s + r + f;
}
//## all_tuple_of_different_types
async function num(n: number): Promise<number> {
  await Promise.resolve();
  return n;
}
async function str(s: string): Promise<string> {
  await Promise.resolve();
  return s;
}
function delay<T>(ms: number, v: T): Promise<T> {
  return new Promise<T>((resolve) => {
    setTimeout(() => {
      resolve(v);
    }, ms);
  });
}
async function t(): Promise<string> {
  const [a, b] = await Promise.all([num(3), str("x")]);
  const all = await Promise.all([num(1), str("y"), Promise.resolve(true), 7]);
  const [slow, fast] = await Promise.all([delay(20, "slow"), delay(5, 42)]);
  return `${a}${b} ${all[0]}${all[1]}${all[2]}${all[3]} ${all.length} ${slow}${fast}`;
}
//## all_tuple_rejects_with_the_first_failure
async function bad(): Promise<number> {
  throw new Error("nope");
}
async function str(s: string): Promise<string> {
  await Promise.resolve();
  return s;
}
async function t(): Promise<string> {
  try {
    const [a, b] = await Promise.all([bad(), str("q")]);
    return "no" + a + b;
  } catch (e) {
    return "caught " + (e as Error).message;
  }
}
