// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The killcam: what the client keeps so a player who has just been killed can watch the last few
// seconds again, from behind their killer. The client records what it drew (every figure's pose,
// the props' poses, the beams) about thirty times a second into a short tape; when the local player
// dies it plays that tape forward over the live scene, the camera on the killer, then goes back to
// live. It records what the client already knows, so the replay costs the network nothing and
// shows exactly what the player saw (and could not see: the killer, seen from behind, was in the
// fog all along).

/** Seconds of tape kept. */
export const TAPE_SECONDS = 6;
/** Seconds of tape recorded a second. */
const RATE = 30;

export interface PlayerFrame {
  id: number;
  x: number;
  y: number;
  z: number;
  /** The body's turn (degrees, the model's), as the figure was drawn. */
  yaw: number;
  /** How fast it was running, for the run cycle. */
  speed: number;
  alive: boolean;
}

export interface PropFrame {
  id: number;
  x: number;
  y: number;
  z: number;
  qx: number;
  qy: number;
  qz: number;
  qw: number;
}

export interface Frame {
  t: number;
  players: PlayerFrame[];
  props: PropFrame[];
}

export interface Shot {
  t: number;
  from: Vector3;
  to: Vector3;
  hit: boolean;
}

/** Linear blend of two angles in degrees along the shorter way round. */
export function blendAngle(a: number, b: number, k: number): number {
  let d = (b - a) % 360;
  if (d > 180) d -= 360;
  if (d < -180) d += 360;
  return a + d * k;
}

export class Tape {
  frames: Frame[] = [];
  shots: Shot[] = [];
  private nextAt = 0;

  /** Records a frame (at most `RATE` a second) and forgets what is older than the tape. */
  record(now: number, players: PlayerFrame[], props: PropFrame[]) {
    if (now < this.nextAt) return;
    this.nextAt = now + 1 / RATE;
    this.frames.push({ t: now, players, props });
    while (this.frames.length > 0 && now - this.frames[0]!.t > TAPE_SECONDS) this.frames.shift();
    while (this.shots.length > 0 && now - this.shots[0]!.t > TAPE_SECONDS) this.shots.shift();
  }

  addShot(shot: Shot) {
    this.shots.push(shot);
  }

  /** When the tape starts and ends (the same time twice when it is empty). */
  get start(): number {
    return this.frames.length > 0 ? this.frames[0]!.t : 0;
  }

  get end(): number {
    return this.frames.length > 0 ? this.frames[this.frames.length - 1]!.t : 0;
  }

  /** The frame before time `t` and how far `t` is on to the next (0 to 1); null on an empty tape. */
  around(t: number): { a: Frame; b: Frame; k: number } | null {
    const f = this.frames;
    if (f.length === 0) return null;
    if (t <= f[0]!.t) return { a: f[0]!, b: f[0]!, k: 0 };
    for (let i = 1; i < f.length; i++) {
      if (t <= f[i]!.t) {
        const a = f[i - 1]!;
        const span = f[i]!.t - a.t;
        return { a, b: f[i]!, k: span > 1e-6 ? (t - a.t) / span : 0 };
      }
    }
    const last = f[f.length - 1]!;
    return { a: last, b: last, k: 0 };
  }

  clear() {
    this.frames = [];
    this.shots = [];
  }
}
