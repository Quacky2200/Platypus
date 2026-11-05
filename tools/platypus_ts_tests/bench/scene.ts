// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Compiled scripts driving the real engine scene (`platypus_scene`'s `Scene` API, written with
// `#[ts_api]`): GameObject handles and Vector3 / Quaternion values, no JavaScript engine.

/** A parent and a child: the child's world position includes the parent's. */
export function moveAround(): number {
  const before = Scene.count();
  const parent = Scene.create();
  const child = Scene.create();
  Scene.setLocalPosition(parent, new Vector3(10, 0, 0));
  Scene.setParent(child, parent, false);
  Scene.setLocalPosition(child, new Vector3(1, 2, 3));
  const p = Scene.position(child);
  const hasParent = Scene.parent(child) ? 1 : 0;
  const rootHasParent = Scene.parent(parent) ? 1 : 0;
  Scene.destroy(parent);
  const left = Scene.count() - before;
  return p.x * 10000000 + p.y * 100000 + p.z * 1000 + hasParent * 100 + rootHasParent * 10 + left;
}

/** A per-frame update loop: move an object round a circle and read it back. */
export function orbit(frames: number): number {
  const o = Scene.create();
  let sum = 0;
  for (let i = 0; i < frames; i++) {
    const a = i * 0.1;
    Scene.setPosition(o, new Vector3(Math.cos(a) * 5, 0, Math.sin(a) * 5));
    const p = Scene.position(o);
    sum += p.x + p.z;
  }
  Scene.destroy(o);
  return sum;
}

/** Destroyed objects stop answering; a new object reusing the slot does not revive the old handle. */
export function staleHandles(): number {
  const a = Scene.create();
  Scene.destroy(a);
  const gone = Scene.exists(a) ? 1 : 0;
  const b = Scene.create(); // takes a's slot with a new generation
  const reborn = Scene.exists(a) ? 1 : 0;
  const alive = Scene.exists(b) ? 1 : 0;
  const origin = Scene.position(a);
  Scene.destroy(b);
  return gone * 1000 + reborn * 100 + alive * 10 + origin.x + origin.y + origin.z;
}

/** A rotation round y, set from a not-yet-unit Quaternion: read back normalised. */
export function rotation(): number {
  const o = Scene.create();
  Scene.setLocalRotation(o, new Quaternion(0, 0.5, 0, 0.5));
  const q = Scene.localRotation(o);
  Scene.destroy(o);
  return q.y * 1000 + q.w;
}

/** Objects in an array and in a list of handles, between collections. */
export function crowd(n: number): number {
  const before = Scene.count();
  const all: GameObject[] = [];
  for (let i = 0; i < n; i++) {
    const o = Scene.create();
    Scene.setLocalPosition(o, new Vector3(i, i * 2, 0));
    all.push(o);
  }
  let sum = 0;
  for (const o of all) sum += Scene.localPosition(o).y;
  const made = Scene.count() - before;
  for (const o of all) Scene.destroy(o);
  return sum * 1000 + made * 1 + (Scene.count() - before) * 100;
}

/** The same hierarchy through handle classes: `go.transform.position = v` and methods on handles. */
export function classHierarchy(): number {
  const before = Scene.count();
  const parent = GameObject.create();
  const child = GameObject.create();
  parent.transform.localPosition = new Vector3(10, 0, 0);
  child.transform.setParent(parent.transform, false);
  child.transform.localPosition = new Vector3(1, 2, 3);
  const p = child.transform.position;
  const hasParent = child.transform.parent ? 1 : 0;
  const rootHasParent = parent.transform.parent ? 1 : 0;
  parent.destroy();
  const left = Scene.count() - before;
  return p.x * 10000000 + p.y * 100000 + p.z * 1000 + hasParent * 100 + rootHasParent * 10 + left;
}

/** Scalar, boolean and handle properties, and a getter that goes back to the object. */
export function classFlags(): number {
  const o = GameObject.create();
  o.layer = 5;
  o.enabled = false;
  const off = o.activeInHierarchy ? 1 : 0;
  const was = o.enabled ? 1 : 0;
  o.enabled = true;
  const on = o.activeInHierarchy ? 1 : 0;
  const same = o.transform.gameObject === o ? 1 : 0;
  const alive = o.exists() ? 1 : 0;
  const layer = o.layer;
  o.destroy();
  const gone = o.exists() ? 1 : 0;
  return layer * 10000 + off * 1000 + was * 100 + on * 10 + same + alive * 100000 + gone * 1000000;
}

/** A per-frame update loop through `transform.position`. */
export function classOrbit(frames: number): number {
  const o = GameObject.create();
  let sum = 0;
  for (let i = 0; i < frames; i++) {
    const a = i * 0.1;
    o.transform.position = new Vector3(Math.cos(a) * 5, 0, Math.sin(a) * 5);
    const p = o.transform.position;
    sum += p.x + p.z;
  }
  o.destroy();
  return sum;
}

/** Methods and getters on Vector3, compiled as direct calls (`a.add(b)`, `v.magnitude`). */
export function vectorMath(): number {
  const a = new Vector3(1, 2, 3);
  const b = new Vector3(4, 5, 6);
  const s = a.add(b).scale(2); // (10, 14, 18)
  const d = a.dot(b); // 32
  const c = a.cross(b); // (-3, 6, -3)
  const m = new Vector3(3, 4, 0).magnitude; // 5
  const n = new Vector3(0, 0, 9).normalized; // (0, 0, 1)
  const l = a.lerp(b, 0.5); // (2.5, 3.5, 4.5)
  const near = Math.abs(a.distance(b) - Math.sqrt(27)) < 0.00001 ? 1 : 0;
  return s.x + s.y + s.z + d * 1000 + m * 100000 + n.z * 10000000 + c.y * 100000000 + l.y * 1000000000 + near * 10000000000;
}

/** Quaternion methods: rotating a vector, combining and inverting. */
export function quatMath(): number {
  const q = new Quaternion(0, 0.7071068, 0, 0.7071068); // 90 degrees round y
  const v = q.rotate(new Vector3(1, 0, 0)); // (0, 0, -1)
  const r = q.mul(q.inverse()); // identity
  return v.z * 1000 + r.w * 100000;
}

/** A vector kept across allocations in a loop: each step makes a Vector3 argument and results. */
export function vectorLoop(n: number): number {
  let acc = new Vector3(0, 0, 0);
  for (let i = 0; i < n; i++) {
    acc = acc.add(new Vector3(i, 1, 0)).scale(0.5);
  }
  return acc.x * 1000 + acc.y;
}

/** Strings across the boundary: names in, names out, and `GameObject.find`. */
export function names(): string {
  const a = GameObject.create();
  a.name = "player-" + 1;
  const b = GameObject.create();
  b.name = "enemy";
  const found = GameObject.find("player-1");
  const missing = GameObject.find("nobody") ? "yes" : "no";
  const r = found.name + "/" + (found === a ? "same" : "other") + "/" + missing + "/" + b.name.length;
  a.destroy();
  b.destroy();
  return r;
}

/** A name with accents and an emoji comes back unchanged (UTF-16 `length` of the script string). */
export function unicodeName(): number {
  const o = GameObject.create();
  o.name = "héllo 😀";
  const n = o.name;
  const same = n === "héllo 😀" ? 1 : 0;
  const len = n.length;
  o.destroy();
  return same * 1000 + len;
}

/** A lone surrogate in a name reaches the engine as U+FFFD (and the rest is untouched). */
export function lossyName(): number {
  const o = GameObject.create();
  o.name = "x" + String.fromCharCode(0xd83d) + "y";
  const n = o.name;
  const r = n.length * 100000 + n.charCodeAt(1);
  o.destroy();
  return r;
}

/** Many names made and read back between collections. */
export function manyNames(count: number): number {
  const all: GameObject[] = [];
  for (let i = 0; i < count; i++) {
    const o = GameObject.create();
    o.name = "n" + i;
    all.push(o);
  }
  let total = 0;
  for (const o of all) total += o.name.length;
  const second = GameObject.find("n" + 2).name;
  for (const o of all) o.destroy();
  return total * 1000 + second.length;
}
