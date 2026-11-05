// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Scripts calling the engine API directly: `Demo.fn(..)` is a call of the Rust shim
// `platypus_Demo_fn`, with the numbers converted to the engine's own widths and `Entity` handles
// passing through as plain 64-bit ids. (The API is `platypus_demo_api`, written with `#[ts_api]`.)

interface Slot {
  owner: Entity;
  label: number;
}

/** i32 and u32 parameters, bool results. */
export function arithmetic(): number {
  return (
    Demo.scale(3, 4) * 1000 +
    Demo.clampI(250, 0, 100) * 10 +
    Demo.clampI(-5.7, 0, 10) +
    (Demo.isEven(4) ? 1 : 0) * 100000 +
    (Demo.isEven(7) ? 1 : 0) * 10000
  );
}

/** A number passed as an i32 is truncated toward zero, as `as i32` does in Rust. */
export function truncates(): number {
  return Demo.clampI(3.9, 0, 10) * 10 + Demo.clampI(-5.7, -10, 10);
}

/** The engine computes in f32: 0.1 narrows on the way in and widens on the way out. */
export function halfOfTenth(): number {
  return Demo.half(0.1);
}

/** Handles are ids: equal ids compare equal, a destroyed entity's id stops working. */
export function entities(): number {
  Demo.reset();
  const a: Entity = Demo.spawn(7);
  const b = Demo.spawn(9);
  Demo.setHp(a, 55.5);
  const same = a === b ? 1 : 0;
  const kinds = Demo.kindOf(a) * 100 + Demo.kindOf(b);
  Demo.destroy(b);
  const gone = Demo.alive(b) ? 1 : 0;
  Demo.spawn(3); // takes b's slot, with a new generation
  const stale = Demo.alive(b) ? 1 : 0;
  return Demo.count() * 1000000 + kinds * 1000 + same * 100 + gone * 10 + stale;
}

export function liveHp(): number {
  Demo.reset();
  const a = Demo.spawn(1);
  Demo.setHp(a, 55.5);
  return Demo.hp(a);
}

export function staleHp(): number {
  Demo.reset();
  const a = Demo.spawn(1);
  Demo.destroy(a);
  return Demo.hp(a);
}

/** Handles in objects and arrays, between allocations. */
export function dataStructures(): number {
  Demo.reset();
  const slots: Slot[] = [];
  const ids: Entity[] = [];
  for (let i = 0; i < 20; i++) {
    const e = Demo.spawn(i);
    ids.push(e);
    slots.push({ owner: e, label: i * 2 });
  }
  let total = 0;
  for (const s of slots) total += Demo.kindOf(s.owner) + s.label;
  return total + Demo.kindOf(ids[3]!) * 1000;
}

/** `Entity | null`. */
export function nullableHandle(flag: boolean): number {
  Demo.reset();
  const e = Demo.spawn(5);
  let target: Entity | null = null;
  if (flag) target = e;
  return target === null ? -1 : Demo.kindOf(target);
}

/** A closure over a handle reads the engine's current state when it is called. */
export function closureOverHandle(): number {
  Demo.reset();
  const e = Demo.spawn(1);
  Demo.setHp(e, 40);
  const add = (n: number) => Demo.hp(e) + n;
  Demo.setHp(e, 41);
  return add(1);
}

/** A value class: `new Vector3(..)` is an object with x, y, z; the engine takes and returns it as 32-bit floats. */
export function vectors(): number {
  const a = new Vector3(3, 0, 4);
  const len = Demo.length(a);
  const b = Demo.scaled(a, 2);
  const c = Demo.cross(new Vector3(1, 0, 0), new Vector3(0, 1, 0));
  return len * 1000 + b.x * 100 + b.z + c.z * 10000;
}

/** Results are fresh objects and rounded to f32 on the way: 0.1 is not 0.1 afterwards. */
export function vectorRounding(): number {
  return Demo.scaled(new Vector3(0.1, 0, 0), 1).x;
}

/** A vector stored in the engine, next to a handle. */
export function positions(): number {
  Demo.reset();
  const e = Demo.spawn(1);
  Demo.setPosition(e, new Vector3(1.5, 2.5, 3.5));
  const p = Demo.position(e);
  const origin = Demo.position(Demo.spawn(2));
  return p.x * 100 + p.y * 10 + p.z + origin.x;
}

/** Many short-lived vectors between collections. */
export function manyVectors(n: number): number {
  let total = 0;
  let v = new Vector3(1, 0, 0);
  for (let i = 0; i < n; i++) {
    v = Demo.scaled(v, 1.5);
    const w = Demo.cross(v, new Vector3(0, 1, 0));
    total += Demo.length(w) + w.z * 0;
  }
  return total;
}
