// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Player movement as a pure function, the same code on the server and every client. A client runs it
// at once on what the player pressed (prediction); the server runs it on the inputs it receives and
// is the one that counts; the client compares what the server says with what it predicted and, if it
// is off, takes the server's state and runs its not yet confirmed inputs again (reconciliation).
//
// It must give the same answer from the same state and input on both sides, so it uses nothing but
// its arguments, the static solids of the map and plain arithmetic: no physics, no time, no random.

import { BOUND, SOLIDS } from "./map";
import { SPRINT, WALK } from "./schema";

/** Seconds a step covers: movement is simulated at this fixed rate on both sides. */
export const STEP = 1 / 30;
/** The radius of a player's circle on the ground. */
const RADIUS = 0.3;
const GRAVITY = 16;
const JUMP_SPEED = 5.5;
/** How far from the middle a player can stand (the fog is solid beyond). */
const LIMIT = BOUND - 1;

export interface MoveState {
  x: number;
  y: number;
  z: number;
  /** Vertical speed. */
  vy: number;
}

export interface MoveInput {
  /** Counts up by one per step on the client; the server answers with the last one it used. */
  seq: number;
  /** The way to walk in the world (length at most one), in hundredths. */
  dx: number;
  dz: number;
  sprint: boolean;
  jump: boolean;
}

/** An input as the wire will carry it: the walk direction in hundredths, so both sides step the same numbers. */
export function makeInput(seq: number, dx: number, dz: number, sprint: boolean, jump: boolean): MoveInput {
  return { seq, dx: Math.round(dx * 100) / 100, dz: Math.round(dz * 100) / 100, sprint, jump };
}

export function encodeInput(i: MoveInput): string {
  return `${i.seq},${Math.round(i.dx * 100)},${Math.round(i.dz * 100)},${(i.sprint ? 1 : 0) + (i.jump ? 2 : 0)}`;
}

/** Inputs from a text of `encodeInput`s joined by `|`; anything malformed is skipped. */
export function decodeInputs(text: string): MoveInput[] {
  const out: MoveInput[] = [];
  for (const part of text.split("|")) {
    const f = part.split(",");
    if (f.length !== 4) continue;
    const seq = parseFloat(f[0]!);
    const dx = parseFloat(f[1]!);
    const dz = parseFloat(f[2]!);
    const flags = parseFloat(f[3]!);
    // (NaN is the only number that differs from itself.)
    if (seq !== seq || dx !== dx || dz !== dz || flags !== flags) continue;
    out.push({ seq, dx: dx / 100, dz: dz / 100, sprint: (flags & 1) !== 0, jump: (flags & 2) !== 0 });
  }
  return out;
}

export function copyState(s: MoveState): MoveState {
  return { x: s.x, y: s.y, z: s.z, vy: s.vy };
}

/** One fixed step from `s` under `inp`: walk or sprint, slide along the houses and cars, jump, fall. */
export function step(s: MoveState, inp: MoveInput): MoveState {
  let dx = inp.dx;
  let dz = inp.dz;
  const len = Math.hypot(dx, dz);
  // Nobody walks faster than the walk speed, whatever an input claims.
  if (len > 1) {
    dx /= len;
    dz /= len;
  }
  const speed = inp.sprint ? SPRINT : WALK;
  let x = s.x + dx * speed * STEP;
  let z = s.z + dz * speed * STEP;
  // A solid is a box grown by the player's radius; inside it, leave by the nearer side.
  for (const o of SOLIDS) {
    const ex = o.hx + RADIUS;
    const ez = o.hz + RADIUS;
    const ox = x - o.x;
    const oz = z - o.z;
    if (Math.abs(ox) < ex && Math.abs(oz) < ez) {
      const px = ex - Math.abs(ox);
      const pz = ez - Math.abs(oz);
      if (px < pz) x += ox >= 0 ? px : -px;
      else z += oz >= 0 ? pz : -pz;
    }
  }
  x = Math.max(-LIMIT, Math.min(LIMIT, x));
  z = Math.max(-LIMIT, Math.min(LIMIT, z));
  let vy = s.vy;
  let y = s.y;
  if (y <= 0.001 && vy <= 0 && inp.jump) vy = JUMP_SPEED;
  vy -= GRAVITY * STEP;
  y += vy * STEP;
  if (y <= 0) {
    y = 0;
    if (vy < 0) vy = 0;
  }
  return { x, y, z, vy };
}

/**
 * How far along a shot a player is hit, or -1: the shot leaves `origin` along the unit heading
 * `flat` (x and z) and climbs `rise` for every unit it travels; the player is a vertical cylinder
 * of radius 0.45 and height 1.85 standing at `feet`. The same test on the server (to hurt) and on
 * the shooter's screen (to draw the beam at once).
 */
export function shotHitsPlayer(origin: Vector3, flat: Vector3, rise: number, feet: Vector3): number {
  // The shot's length is one unit along its path; its flat part is shorter when it climbs.
  const level = Math.sqrt(Math.max(0, 1 - rise * rise));
  if (level < 0.02) return -1;
  const ox = origin.x - feet.x;
  const oz = origin.z - feet.z;
  const b = ox * flat.x + oz * flat.z;
  const disc = b * b - (ox * ox + oz * oz - 0.45 * 0.45);
  if (disc < 0) return -1;
  const along = -b - Math.sqrt(disc);
  if (along <= 0) return -1;
  const d = along / level;
  const y = origin.y + rise * d;
  return y < feet.y - 0.05 || y > feet.y + 1.85 ? -1 : d;
}

/**
 * How far along the segment from `a` towards `b` (in the ground plane) the first house or parked
 * car is entered, or -1 when the segment crosses none (a slab test against each footprint). The
 * map's own check for what the physics rays should have found: walls stop shots and sight, and a
 * ray that slipped through a collider's edge must not let a bot see or shoot through a house.
 */
export function solidReach(a: Vector3, b: Vector3): number {
  const dx = b.x - a.x;
  const dz = b.z - a.z;
  const len = Math.hypot(dx, dz);
  if (len < 1e-6) return -1;
  let best = -1;
  for (const o of SOLIDS) {
    let t0 = 0;
    let t1 = 1;
    let hit = true;
    // x slab, then z slab: the part of the segment inside both.
    for (let axis = 0; axis < 2; axis++) {
      const start = axis === 0 ? a.x : a.z;
      const delta = axis === 0 ? dx : dz;
      const lo = (axis === 0 ? o.x - o.hx : o.z - o.hz) - start;
      const hi = (axis === 0 ? o.x + o.hx : o.z + o.hz) - start;
      if (Math.abs(delta) < 1e-9) {
        if (lo > 0 || hi < 0) hit = false;
      } else {
        let u0 = lo / delta;
        let u1 = hi / delta;
        if (u0 > u1) {
          const swap = u0;
          u0 = u1;
          u1 = swap;
        }
        t0 = Math.max(t0, u0);
        t1 = Math.min(t1, u1);
      }
    }
    if (hit && t0 <= t1) {
      const d = t0 * len;
      if (best < 0 || d < best) best = d;
    }
  }
  return best;
}

/** Whether the state is standing on the ground. */
export function grounded(s: MoveState): boolean {
  return s.y <= 0.001 && s.vy <= 0;
}
