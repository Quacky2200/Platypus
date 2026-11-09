// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The arena's bot brain, built on `platypus_ai`: a `NavGrid` of the town (houses and parked cars
// blocked) for A* routes and line-of-sight checks, and a `Goap` planner that picks what to do next.
//
// What a bot does:
// - patrol: walks between spots on the map until it sees or hears someone;
// - chase: follows an A* route to where it last saw or heard its target;
// - fight: strafes in a ring round a target it can see, aiming with an error that grows with
//   distance, and shooting only after a reaction time and only with a clear line (a ray the same
//   as the server's, so it never fires through a wall);
// - take cover: when hurt, runs to the nearest spot the target cannot see, then waits there to heal.
// It keeps away from other characters, and only knows what it has seen (inside BOT_SIGHT, which is
// well inside the fog) or heard (a shot within BOT_HEARING).

import { BOUND, SOLIDS } from "./map";
import { solidReach } from "./move";

/** Shoots and notices only this close: the fog hides a bot's target past 22 m, so a player sees a bot first. */
const SIGHT = 13;
/** A shot this close is heard. */
const HEARING = 26;
/** The cone a bot sees in, in degrees across (what is outside it is not seen, until the bot turns). */
const FOV = 120;
/** Anyone this close is noticed whichever way the bot faces (it hears the footsteps). */
const NOTICE = 2.5;
/** How fast a bot turns, degrees a second: it has to turn to what it heard or what is behind it. */
const TURN = 260;
/** A bot fires once its aim is this close to the target (degrees). */
const FIRE_CONE = 10;
const REACTION = 0.6;
const AIM_ERROR = 5;
/** Seconds a bot remembers where it last saw someone. */
const MEMORY = 6;
/** Fighting distance band: closer than NEAR it backs off, further than FAR it closes in. */
const NEAR = 6;
const FAR = 10;
/** Health below which it runs for cover, and the health it wants back before it fights again. */
const LOW = 40;
const HEALED = 75;
/** Space kept clear round a wall: a character is 0.6 m wide. */
const MARGIN = 0.8;
const CELLS = BOUND * 2;

export interface Contact {
  pos: Vector3;
}

export interface Think {
  /** The way to walk (flat; zero to stand). */
  dir: Vector3;
  /** Where the bot looks and aims, degrees (0 faces -z). */
  yaw: number;
  fire: boolean;
  run: boolean;
  /** Distance to what it is fighting (99 when it sees nothing). */
  dist: number;
}

function flat(v: Vector3): Vector3 {
  return new Vector3(v.x, 0, v.z);
}

function onGrid(p: Vector3): Vector3 {
  return new Vector3(p.x, 0.5, p.z);
}

/** The turn from heading `from` to heading `to`, in degrees, the short way round (-180 to 180). */
function angleDiff(to: number, from: number): number {
  return ((((to - from) % 360) + 540) % 360) - 180;
}

function headingOf(d: Vector3): number {
  return (Math.atan2(d.x, -d.z) * 180) / Math.PI;
}

export class BotBrain {
  private grid: NavGrid;
  private goap: Goap;
  private spots: Vector3[] = [];
  private path: Vector3[] = [];
  private pathIndex = 0;
  private pathAt = -9;
  private pathGoal = new Vector3(0, 0, 0);
  private known: Vector3 | null = null;
  private knownAt = -99;
  private seenFor = 0;
  private low = false;
  private mode = "patrol";
  private modeAt = -9;
  private patrolTo: Vector3 | null = null;
  private patrolAt = 0;
  private cover: Vector3 | null = null;
  private coverAt = -9;
  private strafe = 1;
  private strafeAt = 0;
  private jitter = 0;
  private jitterAt = 0;
  private blockedUntil = 0;
  private lastYaw = 0;
  /** Where the bot faces, degrees (0 faces -z): it sees in a cone round this and turns at `TURN`. */
  private heading = 0;

  constructor() {
    this.grid = new NavGrid(new Vector3(CELLS, 1, CELLS), new Vector3(-BOUND, 0, -BOUND), 1);
    for (let ix = 0; ix < CELLS; ix++) {
      for (let iz = 0; iz < CELLS; iz++) {
        const cx = -BOUND + ix + 0.5;
        const cz = -BOUND + iz + 0.5;
        // The outer metres are the fog's edge, left to the invisible wall.
        let blocked = Math.abs(cx) > BOUND - 2 || Math.abs(cz) > BOUND - 2;
        for (const s of SOLIDS) {
          if (Math.abs(cx - s.x) < s.hx + MARGIN && Math.abs(cz - s.z) < s.hz + MARGIN) blocked = true;
        }
        if (blocked) this.grid.setBlocked(new Vector3(ix, 0, iz), true);
        else if (ix % 4 === 1 && iz % 4 === 1) this.spots.push(new Vector3(cx, 0.1, cz));
      }
    }
    // What a bot can do, as GOAP actions over named facts. The goal is always to put its target
    // down; the plan's first step is what it does now.
    this.goap = new Goap();
    this.goap.addAction("patrol", 3, ["!hasTarget"], ["hasTarget"]);
    this.goap.addAction("chase", 2, ["hasTarget", "!canSee", "!lowHealth"], ["canSee"]);
    this.goap.addAction("fight", 1, ["canSee", "!lowHealth"], ["targetDown"]);
    this.goap.addAction("takeCover", 1, ["lowHealth", "!covered"], ["covered"]);
    this.goap.addAction("recover", 1, ["lowHealth", "covered"], ["!lowHealth"]);
  }

  /** A shot of this bot's that stopped short: whatever its ray said, something solid is in the way. */
  shotBlocked(now: number) {
    this.blockedUntil = now + 1.5;
  }

  /** A shot was fired at `from`: if it is close enough, go and look. */
  heard(from: Vector3, me: Vector3, now: number) {
    const d = flat(from.sub(me)).magnitude;
    if (d < 1.5 || d > HEARING) return;
    this.known = from;
    this.knownAt = now;
  }

  /** Whether a bot at `a` sees `b`: nothing solid between them (the server's ray, eye to chest). */
  private clear(a: Vector3, b: Vector3, now: number): boolean {
    if (now < this.blockedUntil) return false;
    // The map's houses and cars first (a physics ray can slip past a collider's edge).
    if (solidReach(a, b) >= 0) return false;
    const eye = a.add(new Vector3(0, 1.3, 0));
    const to = b.add(new Vector3(0, 1.1, 0)).sub(eye);
    const dist = to.magnitude;
    const dir = to.normalized;
    // Start past the bot's own collider.
    return !Physics.raycast(eye.add(dir.scale(0.6)), dir, Math.max(0.1, dist - 0.6));
  }

  /** The unit direction along an A* route to `goal` (routes are redone twice a second). */
  private walkTo(me: Vector3, goal: Vector3, now: number): Vector3 {
    if (now - this.pathAt > 0.5 || flat(goal.sub(this.pathGoal)).magnitude > 2) {
      this.path = this.grid.findPath(onGrid(me), onGrid(goal), true);
      this.pathIndex = 0;
      this.pathAt = now;
      this.pathGoal = goal;
    }
    while (this.pathIndex < this.path.length - 1 && flat(this.path[this.pathIndex].sub(me)).magnitude < 0.9) this.pathIndex++;
    // No route (the goal is inside a wall's margin): head straight for it.
    const next = this.pathIndex < this.path.length ? this.path[this.pathIndex] : goal;
    const d = flat(next.sub(me));
    return d.magnitude < 0.05 ? new Vector3(0, 0, 0) : d.normalized;
  }

  /** The nearest spot a bot at `me` can reach that `from` cannot see, and is not close to it. */
  private pickCover(me: Vector3, from: Vector3): Vector3 | null {
    let best: Vector3 | null = null;
    let bestD = 1e9;
    for (const s of this.spots) {
      const d = flat(s.sub(me)).magnitude;
      if (d >= bestD || flat(s.sub(from)).magnitude < 7) continue;
      if (this.grid.lineOfSight(onGrid(s), onGrid(from))) continue;
      best = s;
      bestD = d;
    }
    return best;
  }

  /** One frame of thinking for a bot standing at `me` with `hp` health. */
  think(me: Vector3, hp: number, others: Contact[], dt: number, now: number): Think {
    // Senses: the nearest character inside sight, inside the cone it faces (or close enough to
    // hear), with nothing between.
    let target: Vector3 | null = null;
    let dist = 99;
    const look = (this.heading * Math.PI) / 180;
    const cosHalf = Math.cos((FOV / 2) * (Math.PI / 180));
    for (const o of others) {
      const to = flat(o.pos.sub(me));
      const d = to.magnitude;
      const seen = d < NOTICE || (d > 0.01 && (to.x * Math.sin(look) - to.z * Math.cos(look)) / d > cosHalf);
      if (seen && d < SIGHT && d < dist && this.clear(me, o.pos, now)) {
        dist = d;
        target = o.pos;
      }
    }
    if (target) {
      this.known = target;
      this.knownAt = now;
      this.seenFor += dt;
    } else this.seenFor = 0;
    // Arrived where it last saw them and they are gone: that trail is cold.
    if (!target && this.known && flat(this.known.sub(me)).magnitude < 1.5) this.known = null;
    const hasTarget = target !== null || (this.known !== null && now - this.knownAt < MEMORY);

    if (hp < LOW) this.low = true;
    else if (hp >= HEALED) this.low = false;
    const covered = this.known !== null && !this.grid.lineOfSight(onGrid(me), onGrid(this.known));

    // Plan a few times a second: the first action of the cheapest plan is the mode.
    if (now - this.modeAt > 0.3) {
      this.modeAt = now;
      const facts: string[] = [];
      if (hasTarget) facts.push("hasTarget");
      if (target) facts.push("canSee");
      if (this.low) facts.push("lowHealth");
      if (covered) facts.push("covered");
      const plan = this.goap.plan(facts, ["targetDown"]);
      this.mode = plan.length > 0 ? plan[0] : "patrol";
    }

    let dir = new Vector3(0, 0, 0);
    let run = false;
    // Where it wants to look: ahead of where it goes, at its target, or towards where it last
    // saw or heard someone; the heading turns towards it at `TURN`, so a bot must turn before it
    // can see or shoot what is beside or behind it.
    let want = this.heading;
    let fire = false;
    const known = this.known;
    // Hunting, even without a clear view: look where they were.
    if (known && hasTarget) want = headingOf(flat(known.sub(me)));
    if (this.mode === "fight" && target) {
      const toT = flat(target.sub(me));
      const dirT = toT.normalized;
      if (now > this.strafeAt) {
        this.strafeAt = now + Random.range(1, 2.5);
        this.strafe = Random.range(0, 1) < 0.5 ? -1 : 1;
      }
      const side = new Vector3(-dirT.z, 0, dirT.x).scale(this.strafe * 0.8);
      const close = dist > FAR ? 0.8 : dist < NEAR ? -0.8 : 0;
      dir = side.add(dirT.scale(close));
      // A wall at its side: strafe the other way.
      if (dir.magnitude > 0.1 && Physics.raycast(me.add(new Vector3(0, 0.5, 0)), dir.normalized, 1.2)) this.strafe = -this.strafe;
      if (now > this.jitterAt) {
        this.jitterAt = now + 0.5;
        // Harder to hit from far away.
        const spread = AIM_ERROR * (0.5 + dist / SIGHT);
        this.jitter = Random.range(-spread, spread);
      }
      want = headingOf(toT);
      // It shoots once the turn has brought its aim to the target, and not before it has reacted.
      const off = Math.abs(angleDiff(want, this.heading));
      fire = this.seenFor > REACTION && off < FIRE_CONE && Math.sin(now * 2.3) > 0.3;
    } else if (this.mode === "chase" && known) {
      dir = this.walkTo(me, known, now);
      run = true;
    } else if (this.mode === "takeCover" && known) {
      if (!this.cover || now - this.coverAt > 1) {
        this.coverAt = now;
        this.cover = this.pickCover(me, known);
      }
      if (this.cover) {
        dir = this.walkTo(me, this.cover, now);
        run = true;
      }
      // Running for cover, it looks where it runs (not at the one it flees).
      if (dir.magnitude > 0.01) want = headingOf(dir);
    } else if (this.mode === "recover" || this.mode === "takeCover") {
      // Wait it out, turned towards where they will come from (as `want` already is).
    } else {
      // Patrol: spot to spot.
      const here = this.patrolTo ? flat(this.patrolTo.sub(me)).magnitude : 0;
      if (!this.patrolTo || here < 1.5 || now - this.patrolAt > 14) {
        this.patrolTo = this.spots[Math.floor(Random.range(0, this.spots.length))];
        this.patrolAt = now;
      }
      dir = this.walkTo(me, this.patrolTo, now);
      if (dir.magnitude > 0.01) want = headingOf(dir);
    }
    // Crowding: step away from anyone within two metres, so bots spread out rather than stack up.
    for (const o of others) {
      const away = flat(me.sub(o.pos));
      const d = away.magnitude;
      if (d < 2 && d > 0.01) dir = dir.add(away.normalized.scale((2 - d) * 0.75));
    }
    // Turn, at a limited speed.
    const turn = Math.max(-TURN * dt, Math.min(TURN * dt, angleDiff(want, this.heading)));
    this.heading += turn;
    // What it aims along is where it faces (with its aim error while it fights).
    const yaw = this.mode === "fight" && target ? this.heading + this.jitter : this.heading;
    this.lastYaw = yaw;
    return { dir, yaw, fire, run, dist };
  }
}
