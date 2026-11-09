// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// A small ecosystem. Rabbits graze, drink, sleep, flee foxes, fight rivals and mate; foxes hunt
// rabbits, drink, sleep, fight rivals and mate. Every animal has needs that rise over time. Each
// tick it turns the world around it into GOAP facts, picks its most urgent need as the goal,
// asks its species' `Goap` brain for the cheapest plan and does the first step. Walking goes
// through a `NavGrid` (A*) around the pond and the rocks, steered with `Steering`.
// The meadow is a 3D scene: rabbits hop, foxes lope, there is a pond and rocks on the grass.
// Tab opens the stats of the selected animal (its needs, goal and plan); N picks the next one.

type Species = "rabbit" | "fox";

interface Animal {
  id: number;
  species: Species;
  male: boolean;
  x: number;
  y: number;
  vx: number;
  vy: number;
  age: number;
  health: number;
  hunger: number;
  thirst: number;
  tired: number;
  libido: number;
  cooldown: number;
  goal: string;
  plan: string[];
  path: Vector3[];
  pathIdx: number;
  pathGoalX: number;
  pathGoalY: number;
  heading: number;
}

interface Patch {
  x: number;
  y: number;
  amount: number;
}

const W = 600;
const H = 400;
const CELL = 10;
const POND = { x: 300, y: 200, r: 48 };
const ROCKS = [
  { x: 110, y: 90, w: 70, h: 40 },
  { x: 440, y: 280, w: 40, h: 80 },
  { x: 150, y: 290, w: 90, h: 24 },
];

const SPEED = { rabbit: 62, fox: 78 };
const ADULT_AGE = 18;
const MAX_AGE = { rabbit: 230, fox: 300 };
const CAP = 90;

// --- The world -----------------------------------------------------------------------------

const rng = new Rng(11);
const grid = new NavGrid(new Vector3(W / CELL, 1, H / CELL), new Vector3(0, 0, 0), CELL);
for (let cx = 0; cx < W / CELL; cx++) {
  for (let cz = 0; cz < H / CELL; cz++) {
    const x = cx * CELL + CELL / 2;
    const y = cz * CELL + CELL / 2;
    const inPond = Math.sqrt((x - POND.x) * (x - POND.x) + (y - POND.y) * (y - POND.y)) < POND.r;
    const inRock = ROCKS.some((r) => x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h);
    if (inPond || inRock) grid.setBlocked(new Vector3(cx, 0, cz), true);
  }
}

const pt = (x: number, y: number) => new Vector3(x, CELL / 2, y);

function blockedAt(x: number, y: number): boolean {
  return grid.isBlocked(new Vector3(Math.floor(x / CELL), 0, Math.floor(y / CELL)));
}

let animals: Animal[] = [];
let patches: Patch[] = [];
let log: string[] = [];
let nextId = 1;
let births = 0;
let deaths = 0;
let clock = 0;

function say(msg: string) {
  console.log(msg);
  log.push(msg);
  if (log.length > 8) log.shift();
}

function name(a: Animal): string {
  return (a.species === "rabbit" ? "Rabbit " : "Fox ") + a.id;
}

function freeSpot(): { x: number; y: number } {
  for (let i = 0; i < 100; i++) {
    const x = rng.float(20, W - 20);
    const y = rng.float(20, H - 20);
    if (!blockedAt(x, y)) return { x, y };
  }
  return { x: 20, y: 20 };
}

function spawn(species: Species, x: number, y: number, age: number): Animal {
  const a: Animal = {
    id: nextId++,
    species,
    male: rng.bool(0.5),
    x,
    y,
    vx: 0,
    vy: 0,
    age,
    health: 1,
    hunger: rng.float(0.1, 0.4),
    thirst: rng.float(0.1, 0.4),
    tired: rng.float(0, 0.3),
    libido: rng.float(0, 0.4),
    cooldown: 0,
    goal: "wander",
    plan: [],
    path: [],
    pathIdx: 0,
    pathGoalX: -999,
    pathGoalY: -999,
    heading: rng.float(0, 6.28),
  };
  animals.push(a);
  return a;
}

function reset() {
  animals = [];
  patches = [];
  log = [];
  births = 0;
  deaths = 0;
  clock = 0;
  nextId = 1;
  for (let i = 0; i < 16; i++) {
    const p = freeSpot();
    patches.push({ x: p.x, y: p.y, amount: rng.float(0.5, 1) });
  }
  for (let i = 0; i < 24; i++) {
    const p = freeSpot();
    spawn("rabbit", p.x, p.y, rng.float(ADULT_AGE, 120));
  }
  for (let i = 0; i < 3; i++) {
    const p = freeSpot();
    spawn("fox", p.x, p.y, rng.float(ADULT_AGE, 150));
  }
  say("Dawn on the meadow.");
}

// --- Brains --------------------------------------------------------------------------------

const AT = ["atFood", "atWater", "atMate", "atRival"];

/** The same action set for every species; only what "food" is differs (grass or rabbits). */
function makeBrain(eatName: string): Goap {
  const g = new Goap();
  const go = (action: string, pre: string[], at: string) =>
    g.addAction(
      action,
      2,
      [...pre, "!" + at],
      [at, ...AT.filter((t) => t !== at).map((t) => "!" + t)],
    );
  go("goFood", ["foodKnown"], "atFood");
  g.addAction(eatName, 1, ["atFood"], ["!hungry"]);
  go("goWater", [], "atWater");
  g.addAction("drink", 1, ["atWater"], ["!thirsty"]);
  go("goMate", ["mateKnown"], "atMate");
  g.addAction("mate", 1, ["atMate", "!rivalNear"], ["!amorous"]);
  go("goRival", ["rivalNear"], "atRival");
  g.addAction("fight", 2, ["atRival"], ["!rivalNear"]);
  g.addAction("flee", 1, ["threatened"], ["!threatened"]);
  g.addAction("sleep", 3, [], ["!tired"]);
  return g;
}

const brains: Record<Species, Goap> = {
  rabbit: makeBrain("graze"),
  fox: makeBrain("hunt"),
};

// --- Perception ----------------------------------------------------------------------------

function dist(ax: number, ay: number, bx: number, by: number): number {
  const dx = ax - bx;
  const dy = ay - by;
  return Math.sqrt(dx * dx + dy * dy);
}

function adult(a: Animal): boolean {
  return a.age >= ADULT_AGE;
}

function amorous(a: Animal): boolean {
  // Only well-fed animals breed, so a species can't outgrow its food.
  return adult(a) && a.libido > 0.6 && a.cooldown <= 0 && a.health > 0.5 && a.hunger < 0.5;
}

interface Senses {
  patch: Patch | null;
  prey: Animal | null;
  mate: Animal | null;
  rival: Animal | null;
  threat: Animal | null;
  facts: string[];
}

/** Where an animal's food is: a grass patch for rabbits, a rabbit for foxes. */
function foodSpot(a: Animal, patch: Patch | null, prey: Animal | null): { x: number; y: number } | null {
  if (a.species === "rabbit") return patch === null ? null : { x: patch.x, y: patch.y };
  return prey === null ? null : { x: prey.x, y: prey.y };
}

function sense(a: Animal): Senses {
  let patch: Patch | null = null;
  let prey: Animal | null = null;
  let mate: Animal | null = null;
  let rival: Animal | null = null;
  let threat: Animal | null = null;
  let best = 1e9;
  if (a.species === "rabbit") {
    for (const p of patches) {
      const d = dist(a.x, a.y, p.x, p.y);
      if (p.amount > 0.25 && d < best) {
        best = d;
        patch = p;
      }
    }
  }
  let bestPrey = 220;
  let bestMate = 200;
  let bestRival = 70;
  // Foxes scare rabbits at 75 px; a desperate rabbit only bolts when one is nearly on top of it.
  let bestThreat = a.hunger > 0.7 || a.thirst > 0.7 ? 30 : 75;
  for (const o of animals) {
    if (o === a) continue;
    const d = dist(a.x, a.y, o.x, o.y);
    if (a.species === "fox" && o.species === "rabbit" && d < bestPrey) {
      bestPrey = d;
      prey = o;
    }
    if (a.species === "rabbit" && o.species === "fox" && d < bestThreat) {
      bestThreat = d;
      threat = o;
    }
    if (o.species === a.species && amorous(a) && amorous(o)) {
      if (o.male !== a.male && d < bestMate) {
        bestMate = d;
        mate = o;
      }
      if (a.male && o.male && d < bestRival) {
        bestRival = d;
        rival = o;
      }
    }
  }
  const facts: string[] = [];
  if (a.hunger > 0.5) facts.push("hungry");
  if (a.thirst > 0.5) facts.push("thirsty");
  if (a.tired > 0.7) facts.push("tired");
  if (amorous(a)) facts.push("amorous");
  if (patch !== null || prey !== null) facts.push("foodKnown");
  if (mate !== null) facts.push("mateKnown");
  if (rival !== null) facts.push("rivalNear");
  if (threat !== null) facts.push("threatened");
  const food = foodSpot(a, patch, prey);
  if (food !== null && dist(a.x, a.y, food.x, food.y) < 16) facts.push("atFood");
  if (dist(a.x, a.y, POND.x, POND.y) < POND.r + 22) facts.push("atWater");
  if (mate !== null && dist(a.x, a.y, mate.x, mate.y) < 16) facts.push("atMate");
  if (rival !== null && dist(a.x, a.y, rival.x, rival.y) < 16) facts.push("atRival");
  return { patch, prey, mate, rival, threat, facts };
}

/** The most urgent need as a goal literal. */
function pickGoal(a: Animal, facts: string[]): { name: string; literal: string } {
  if (facts.includes("threatened")) return { name: "stay safe", literal: "!threatened" };
  const needs = [
    { v: a.hunger, fact: "hungry", name: "eat", literal: "!hungry" },
    { v: a.thirst, fact: "thirsty", name: "drink", literal: "!thirsty" },
    { v: a.tired, fact: "tired", name: "rest", literal: "!tired" },
    { v: a.libido * 0.9, fact: "amorous", name: "mate", literal: "!amorous" },
  ];
  let pick = { name: "wander", literal: "" };
  let best = 0;
  for (const n of needs) {
    if (facts.includes(n.fact) && n.v > best) {
      best = n.v;
      pick = { name: n.name, literal: n.literal };
    }
  }
  return pick;
}

// --- Acting --------------------------------------------------------------------------------

function moveToward(a: Animal, tx: number, ty: number, speed: number, dt: number) {
  let gx = tx;
  let gy = ty;
  if (!grid.lineOfSight(pt(a.x, a.y), pt(tx, ty))) {
    const stale = dist(tx, ty, a.pathGoalX, a.pathGoalY) > 25;
    if (a.path.length === 0 || stale || a.pathIdx >= a.path.length) {
      a.path = grid.findPath(pt(a.x, a.y), pt(tx, ty), true);
      a.pathIdx = 1;
      a.pathGoalX = tx;
      a.pathGoalY = ty;
    }
    while (a.pathIdx < a.path.length - 1 && dist(a.x, a.y, a.path[a.pathIdx]!.x, a.path[a.pathIdx]!.z) < 8) {
      a.pathIdx++;
    }
    const wp = a.path[a.pathIdx];
    if (wp !== undefined) {
      gx = wp.x;
      gy = wp.z;
    }
  } else {
    a.path = [];
  }
  const pos = new Vector3(a.x, 0, a.y);
  const vel = new Vector3(a.vx, 0, a.vy);
  const near: Vector3[] = [];
  for (const o of animals) {
    if (o !== a && o.species === a.species && dist(a.x, a.y, o.x, o.y) < 22) near.push(new Vector3(o.x, 0, o.y));
  }
  const accel = Steering.seek(pos, vel, new Vector3(gx, 0, gy), speed, 400).add(
    Steering.separation(pos, vel, near, 22, speed, 250),
  );
  let nvx = a.vx + accel.x * dt;
  let nvy = a.vy + accel.z * dt;
  const s = Math.sqrt(nvx * nvx + nvy * nvy);
  if (s > speed) {
    nvx = (nvx / s) * speed;
    nvy = (nvy / s) * speed;
  }
  // Slide along walls, the pond and rocks: try each axis on its own when the full step is blocked.
  const open = (x: number, y: number) => x >= 4 && x <= W - 4 && y >= 4 && y <= H - 4 && !blockedAt(x, y);
  const px = a.x + nvx * dt;
  const py = a.y + nvy * dt;
  if (open(px, py)) {
    a.x = px;
    a.y = py;
    a.vx = nvx;
    a.vy = nvy;
  } else if (open(px, a.y)) {
    a.x = px;
    a.vx = nvx;
    a.vy = 0;
  } else if (open(a.x, py)) {
    a.y = py;
    a.vx = 0;
    a.vy = nvy;
  } else {
    a.vx = 0;
    a.vy = 0;
  }
}

function die(a: Animal, why: string) {
  animals = animals.filter((o) => o !== a);
  deaths++;
  say(name(a) + " " + why + ".");
}

function actOn(a: Animal, s: Senses, action: string, dt: number) {
  const speed = SPEED[a.species];
  const food = foodSpot(a, s.patch, s.prey);
  switch (action) {
    case "goFood":
      if (food !== null) moveToward(a, food.x, food.y, a.species === "fox" ? speed * 1.05 : speed, dt);
      break;
    case "goWater": {
      const dx = a.x - POND.x;
      const dy = a.y - POND.y;
      const d = Math.max(1, Math.sqrt(dx * dx + dy * dy));
      moveToward(a, POND.x + (dx / d) * (POND.r + 14), POND.y + (dy / d) * (POND.r + 14), speed, dt);
      break;
    }
    case "goMate":
      if (s.mate !== null) moveToward(a, s.mate.x, s.mate.y, speed, dt);
      break;
    case "goRival":
      if (s.rival !== null) moveToward(a, s.rival.x, s.rival.y, speed, dt);
      break;
    case "flee":
      if (s.threat !== null) {
        const dx = a.x - s.threat.x;
        const dy = a.y - s.threat.y;
        const d = Math.max(1, Math.sqrt(dx * dx + dy * dy));
        // Run away, but toward a spot inside the meadow (not into a wall).
        const fx = Math.min(W - 12, Math.max(12, a.x + (dx / d) * 90));
        const fy = Math.min(H - 12, Math.max(12, a.y + (dy / d) * 90));
        moveToward(a, fx, fy, speed * 1.35, dt);
      }
      break;
    case "graze":
      if (s.patch !== null) {
        s.patch.amount = Math.max(0, s.patch.amount - 0.5 * dt);
        a.hunger = Math.max(0, a.hunger - 0.6 * dt);
      }
      break;
    case "hunt":
      if (s.prey !== null) {
        a.hunger = Math.max(0, a.hunger - 0.85);
        say(name(a) + " caught " + name(s.prey) + ".");
        animals = animals.filter((o) => o !== s.prey);
        deaths++;
      }
      break;
    case "drink":
      a.thirst = Math.max(0, a.thirst - 0.6 * dt);
      break;
    case "sleep":
      a.tired = Math.max(0, a.tired - 0.3 * dt);
      break;
    case "fight":
      if (s.rival !== null) {
        s.rival.health -= 0.22 * dt;
        a.health -= 0.14 * dt;
        for (const f of [s.rival, a]) {
          if (f.health < 0.4 && f.libido > 0) {
            f.libido = 0;
            f.cooldown = 10;
            say(name(f) + " backs down.");
          }
        }
      }
      break;
    case "mate":
      if (s.mate !== null && amorous(s.mate)) {
        // Rabbits breed fast, foxes slowly (one cub, then a long rest), so the foxes can't outgrow their prey.
        const litter = a.species === "rabbit" ? rng.int(1, 4) : 1;
        const rest = a.species === "rabbit" ? 14 : 50;
        for (let i = 0; i < litter && animals.length < CAP; i++) {
          spawn(a.species, (a.x + s.mate.x) / 2 + rng.float(-6, 6), (a.y + s.mate.y) / 2 + rng.float(-6, 6), 0);
          births++;
        }
        say(name(a) + " and " + name(s.mate) + " had " + litter + ".");
        for (const p of [a, s.mate]) {
          p.libido = 0;
          p.cooldown = rest;
          p.hunger = Math.min(1, p.hunger + 0.15);
        }
      }
      break;
  }
}

function wander(a: Animal, dt: number) {
  if (rng.bool(0.04)) a.heading += rng.float(-1.2, 1.2);
  const tx = a.x + Math.cos(a.heading) * 30;
  const ty = a.y + Math.sin(a.heading) * 30;
  if (tx < 10 || tx > W - 10 || ty < 10 || ty > H - 10 || blockedAt(tx, ty)) a.heading += 2.4;
  moveToward(a, tx, ty, SPEED[a.species] * 0.45, dt);
}

function step(dt: number) {
  const before = Math.floor(clock / 10);
  clock += dt;
  if (Math.floor(clock / 10) !== before) {
    const rabbits = animals.filter((a) => a.species === "rabbit").length;
    console.log("t=" + Math.floor(clock) + "s rabbits=" + rabbits + " foxes=" + (animals.length - rabbits) + " born=" + births + " died=" + deaths);
  }
  for (const p of patches) p.amount = Math.min(1, p.amount + 0.07 * dt);
  // Migration: a species that has all but died out is topped up by newcomers from the edge.
  if (Math.floor(clock / 30) !== Math.floor((clock - dt) / 30)) {
    const rabbitCount = animals.filter((a) => a.species === "rabbit").length;
    const foxCount = animals.length - rabbitCount;
    if (rabbitCount < 6) {
      for (let i = 0; i < 4; i++) spawn("rabbit", 12, rng.float(30, H - 30), rng.float(ADULT_AGE, 60));
      say("Rabbits wander in from the west.");
    }
    if (foxCount < 2) {
      spawn("fox", W - 12, rng.float(30, H - 30), rng.float(ADULT_AGE, 60));
      say("A fox wanders in from the east.");
    }
  }
  for (const a of [...animals]) {
    if (!animals.includes(a)) continue;
    a.age += dt;
    a.cooldown = Math.max(0, a.cooldown - dt);
    a.hunger = Math.min(1, a.hunger + (a.species === "fox" ? 0.011 : 0.02) * dt);
    a.thirst = Math.min(1, a.thirst + 0.012 * dt);
    a.tired = Math.min(1, a.tired + 0.01 * dt);
    if (adult(a)) a.libido = Math.min(1, a.libido + 0.02 * dt);
    if (a.hunger >= 1 || a.thirst >= 1) a.health -= 0.08 * dt;
    else a.health = Math.min(1, a.health + 0.015 * dt);
    if (a.health <= 0) {
      die(a, a.hunger >= 1 ? "starved" : a.thirst >= 1 ? "died of thirst" : "died of its wounds");
      continue;
    }
    if (a.age > MAX_AGE[a.species]) {
      die(a, "died of old age");
      continue;
    }
    const s = sense(a);
    const goal = pickGoal(a, s.facts);
    a.goal = goal.name;
    a.plan = [];
    if (goal.literal !== "" && brains[a.species].canPlan(s.facts, [goal.literal])) {
      a.plan = brains[a.species].plan(s.facts, [goal.literal]);
    }
    const first = a.plan[0];
    if (first === undefined) wander(a, dt);
    else actOn(a, s, first, dt);
  }
}

// --- 3D view -------------------------------------------------------------------------------

/** Metres per simulation pixel: the 600 x 400 meadow is 30 x 20 m. */
const M = 0.05;
const toX = (x: number) => (x - W / 2) * M;
const toZ = (y: number) => (y - H / 2) * M;
const v3 = (x: number, y: number, z: number) => new Vector3(x, y, z);

const mat = (color: string, smoothness = 0.05, metallic = 0) =>
  Material.create({ color: Color.hex(color), smoothness, metallic });

const GRASS = mat("#5f9444");
const FAR_GRASS = mat("#436b34");
const TUFT = mat("#86c25a");
const WATER = mat("#2f78b5", 0.95, 0.2);
const KERB = mat("#a8a69a", 0.1);
const ROCK = mat("#7b7f88", 0.1);
const TRUNK = mat("#6b4a2f");
const LEAVES = mat("#2f6b34");
const RABBIT_FUR = [mat("#d9ccb0"), mat("#b9a07e"), mat("#8e7b62")];
const WHITE = mat("#f3f0e6");
const PINK = mat("#e9a3a8");
const FOX_FUR = mat("#d9722f");
const FOX_DARK = mat("#3a2a22");
const FOX_PALE = mat("#f1e3cf");
const RING = mat("#ffffff", 0.3);

function shape(
  name: string,
  mesh: "cube" | "sphere" | "cylinder" | "capsule" | "plane",
  at: Vector3,
  scale: Vector3,
  material: Material,
  parent?: GameObject,
  rotation: Quaternion = Quaternion.identity,
): GameObject {
  const go = new GameObject(name, { position: at, rotation, scale, parent });
  go.addComponent(MeshRenderer, { mesh, material });
  return go;
}

interface View {
  id: number;
  species: Species;
  root: GameObject;
  yaw: number;
  phase: number;
  /** 0 standing, 1 at full stride: eases so legs settle instead of snapping. */
  stride: number;
  /** Hip pivots (foxes): swung to walk. */
  legs: GameObject[];
}

let views: View[] = [];
let tufts: { go: GameObject; patch: Patch; shown: number }[] = [];
let ring: GameObject | null = null;
let viewsStale = false;

// UI state shared with the HUD.
let showStats = false;
let speedUp = 1;
let selectedId = 0;
let follow = false;
let freeRoam = false;

function buildRabbit(a: Animal): View {
  const root = new GameObject("Rabbit " + a.id, { position: v3(toX(a.x), 0, toZ(a.y)) });
  const fur = RABBIT_FUR[a.id % 3]!;
  shape("Body", "sphere", v3(0, 0.2, 0), v3(0.32, 0.3, 0.46), fur, root);
  shape("Head", "sphere", v3(0, 0.3, 0.25), v3(0.22, 0.2, 0.24), fur, root);
  shape("Ear L", "capsule", v3(-0.06, 0.5, 0.2), v3(0.05, 0.13, 0.035), fur, root, Quaternion.euler(0, 0, 10));
  shape("Ear R", "capsule", v3(0.06, 0.5, 0.2), v3(0.05, 0.13, 0.035), fur, root, Quaternion.euler(0, 0, -10));
  shape("Nose", "sphere", v3(0, 0.29, 0.36), v3(0.05, 0.04, 0.04), PINK, root);
  shape("Tail", "sphere", v3(0, 0.22, -0.25), v3(0.12, 0.12, 0.12), WHITE, root);
  return { id: a.id, species: "rabbit", root, yaw: rng.float(0, 360), phase: rng.float(0, 6.28), stride: 0, legs: [] };
}

function buildFox(a: Animal): View {
  const root = new GameObject("Fox " + a.id, { position: v3(toX(a.x), 0, toZ(a.y)) });
  shape("Body", "capsule", v3(0, 0.34, 0), v3(0.26, 0.34, 0.26), FOX_FUR, root, Quaternion.euler(90, 0, 0));
  shape("Chest", "sphere", v3(0, 0.3, 0.2), v3(0.22, 0.2, 0.2), FOX_PALE, root);
  shape("Head", "sphere", v3(0, 0.46, 0.45), v3(0.24, 0.22, 0.28), FOX_FUR, root);
  shape("Snout", "cube", v3(0, 0.42, 0.62), v3(0.1, 0.09, 0.14), FOX_PALE, root);
  shape("Nose", "sphere", v3(0, 0.44, 0.7), v3(0.04, 0.04, 0.04), FOX_DARK, root);
  shape("Ear L", "cube", v3(-0.08, 0.62, 0.4), v3(0.06, 0.12, 0.03), FOX_DARK, root, Quaternion.euler(0, 0, 12));
  shape("Ear R", "cube", v3(0.08, 0.62, 0.4), v3(0.06, 0.12, 0.03), FOX_DARK, root, Quaternion.euler(0, 0, -12));
  shape("Tail", "capsule", v3(0, 0.38, -0.55), v3(0.13, 0.3, 0.13), FOX_FUR, root, Quaternion.euler(-70, 0, 0));
  shape("Tail tip", "sphere", v3(0, 0.46, -0.84), v3(0.13, 0.13, 0.15), WHITE, root);
  // Each leg hangs from a hip pivot, so rotating the pivot swings the whole leg. Order: rear left,
  // front left, rear right, front right.
  const legs: GameObject[] = [];
  for (const sx of [-0.1, 0.1]) {
    for (const sz of [-0.2, 0.22]) {
      const hip = new GameObject("Hip", { position: v3(sx, 0.28, sz), parent: root });
      shape("Leg", "cylinder", v3(0, -0.14, 0), v3(0.07, 0.14, 0.07), FOX_DARK, hip);
      legs.push(hip);
    }
  }
  return { id: a.id, species: "fox", root, yaw: rng.float(0, 360), phase: 0, stride: 0, legs };
}

function buildScene() {
  const sky = Color.hex("#a9d0ee");
  const cam = new GameObject("Main Camera", { position: v3(0, 14, -18) });
  cam.addComponent(Camera, { fieldOfView: 50, backgroundColor: sky, farClipPlane: 300 });
  const sun = new GameObject("Sun", { rotation: Quaternion.euler(52, -30, 0) });
  sun.addComponent(Light, { type: "directional", color: new Color(1, 0.96, 0.88, 1), intensity: 1.35, shadows: "soft", shadowStrength: 0.8 });

  // Land: a wide dark surround and the lighter meadow; a low stone wall marks the edge.
  shape("Surround", "plane", v3(0, -0.02, 0), v3(12, 1, 9), FAR_GRASS);
  shape("Meadow", "plane", v3(0, 0, 0), v3((W * M) / 10, 1, (H * M) / 10), GRASS);
  const hw = (W * M) / 2;
  const hh = (H * M) / 2;
  shape("Wall N", "cube", v3(0, 0.15, -hh - 0.15), v3(W * M + 0.6, 0.3, 0.3), KERB);
  shape("Wall S", "cube", v3(0, 0.15, hh + 0.15), v3(W * M + 0.6, 0.3, 0.3), KERB);
  shape("Wall W", "cube", v3(-hw - 0.15, 0.15, 0), v3(0.3, 0.3, H * M), KERB);
  shape("Wall E", "cube", v3(hw + 0.15, 0.15, 0), v3(0.3, 0.3, H * M), KERB);

  // The pond: a stone kerb and the water, a little above the grass.
  const pr = POND.r * M;
  shape("Kerb", "cylinder", v3(toX(POND.x), 0.02, toZ(POND.y)), v3((pr + 0.3) * 2, 0.02, (pr + 0.3) * 2), KERB);
  shape("Water", "cylinder", v3(toX(POND.x), 0.035, toZ(POND.y)), v3(pr * 2, 0.02, pr * 2), WATER);

  for (const r of ROCKS) {
    const cx = toX(r.x + r.w / 2);
    const cz = toZ(r.y + r.h / 2);
    shape("Rock", "cube", v3(cx, 0.35, cz), v3(r.w * M, 0.7, r.h * M), ROCK);
    shape("Rock top", "sphere", v3(cx, 0.65, cz), v3(r.w * M * 0.8, 0.4, r.h * M * 0.8), ROCK);
  }

  // Trees beyond the wall, from a generator of their own so the animals' dice stay theirs.
  const trees = new Rng(5);
  for (let i = 0; i < 26; i++) {
    const side = i % 4;
    const along = trees.float(-1, 1);
    const out = trees.float(1.5, 6);
    const x = side === 0 ? -hw - out : side === 1 ? hw + out : along * (hw + 4);
    const z = side === 2 ? -hh - out : side === 3 ? hh + out : along * (hh + 3);
    const h = trees.float(2, 3.6);
    shape("Trunk", "cylinder", v3(x, h / 2, z), v3(0.35, h / 2, 0.35), TRUNK);
    shape("Leaves", "sphere", v3(x, h + 0.8, z), v3(2.4, 2.2, 2.4), LEAVES);
  }
}

function buildPatches() {
  for (const t of tufts) t.go.destroy();
  // Each tuft sits a hair higher than the last, so two overlapping full-grown tufts never have
  // their flat tops at the same height (that flickers: z-fighting).
  tufts = patches.map((p, i) => ({
    go: shape("Grass", "cylinder", v3(toX(p.x), 0.03 + i * 0.004, toZ(p.y)), v3(1.4, 0.05, 1.4), TUFT),
    patch: p,
    shown: p.amount,
  }));
}

function angleDiff(to: number, from: number): number {
  let d = (to - from) % 360;
  if (d > 180) d -= 360;
  if (d < -180) d += 360;
  return d;
}

function syncViews(dt: number) {
  if (viewsStale) {
    for (const v of views) v.root.destroy();
    views = [];
    buildPatches();
    viewsStale = false;
  }
  for (const a of animals) {
    if (!views.some((v) => v.id === a.id)) views.push(a.species === "rabbit" ? buildRabbit(a) : buildFox(a));
  }
  views = views.filter((v) => {
    const alive = animals.some((a) => a.id === v.id);
    if (!alive) v.root.destroy();
    return alive;
  });
  for (const v of views) {
    const a = animals.find((o) => o.id === v.id);
    if (a === undefined) continue;
    const speed = Math.sqrt(a.vx * a.vx + a.vy * a.vy);
    const moving = speed > 6;
    if (moving) v.yaw += angleDiff((Math.atan2(a.vx, a.vy) * 180) / Math.PI, v.yaw) * Math.min(1, dt * 10);
    // Rabbits hop (a bounce per hop, higher when running); foxes lope with a small bob.
    let lift = 0;
    v.stride += ((moving ? 1 : 0) - v.stride) * Math.min(1, dt * 8);
    if (moving) {
      v.phase += dt * (a.species === "rabbit" ? 6 + speed * 0.1 : 8 + speed * 0.12);
      lift = a.species === "rabbit" ? Math.abs(Math.sin(v.phase)) * (0.12 + speed * 0.004) : Math.abs(Math.sin(v.phase)) * 0.035;
    }
    // Fox legs swing in diagonal pairs (front left with rear right), and ease back to standing.
    for (let i = 0; i < v.legs.length; i++) {
      const swing = Math.sin(v.phase) * 38 * v.stride * (i === 1 || i === 2 ? 1 : -1);
      v.legs[i]!.transform.localRotation = Quaternion.euler(swing, 0, 0);
    }
    const act = a.plan[0];
    const dipping = act === "graze" || act === "drink";
    const sleeping = act === "sleep";
    const fighting = act === "fight";
    const size = adult(a) ? 1 : 0.65;
    // Heads down to graze or drink, a squat to sleep, a lunge to fight.
    const pitch = dipping ? 22 : fighting ? Math.sin(clock * 18) * 12 : 0;
    v.root.transform.position = v3(toX(a.x), lift - (sleeping ? 0.04 : 0), toZ(a.y));
    v.root.transform.rotation = Quaternion.euler(pitch, v.yaw, 0);
    v.root.transform.localScale = v3(size, size * (sleeping ? 0.8 : 1), size);
  }
  // Tufts shrink as they are grazed and swell back as they regrow: the height is the amount,
  // eased so many small bites don't make it twitch.
  for (const t of tufts) {
    t.shown += (t.patch.amount - t.shown) * Math.min(1, dt * 4);
    const k = t.shown;
    t.go.transform.localScale = v3(0.9 + 0.7 * k, 0.04 + 0.3 * k, 0.9 + 0.7 * k);
  }
  // A ring under the selected animal.
  const sel = animals.find((a) => a.id === selectedId);
  if (sel !== undefined) {
    if (ring === null) ring = shape("Selection", "cylinder", v3(0, 0.04, 0), v3(1, 0.01, 1), RING);
    const r = sel.species === "fox" ? 1.3 : 0.9;
    ring.transform.position = v3(toX(sel.x), 0.045, toZ(sel.y));
    ring.transform.localScale = v3(r, 0.01, r);
  } else if (ring !== null) {
    ring.transform.localScale = v3(0, 0, 0);
  }
}

function nextAnimal() {
  if (animals.length === 0) return;
  const at = animals.findIndex((a) => a.id === selectedId);
  selectedId = animals[(at + 1) % animals.length]!.id;
}

class Meadow extends Component {
  private acc = 0;
  private yaw = 0;
  /** Degrees: the world is right-handed with forward = -z, so a negative pitch looks down. */
  private pitch = -42;
  private distance = 26;
  private center: Vector3 = new Vector3(0, 0, 0);
  private free = false;
  private flyPos: Vector3 = new Vector3(0, 12, 14);

  /** Free-roam: the cursor is locked and the mouse looks (no dragging); WASD flies. */
  private setFree(on: boolean) {
    this.free = on;
    freeRoam = on;
    Cursor.lockState = on ? "locked" : "none";
    if (on) {
      follow = false;
      const cam = Camera.main;
      if (cam) this.flyPos = cam.transform.position;
    }
  }

  /** The animal nearest the pointer (or the screen centre when the cursor is locked), if one is close enough. */
  private pick(cam: Camera) {
    const m = input.mousePosition;
    const px = this.free ? Screen.width / 2 : m.x;
    const py = this.free ? Screen.height / 2 : m.y;
    let best = 1;
    let hit = 0;
    for (const a of animals) {
      const p = cam.worldToScreenPoint(v3(toX(a.x), a.species === "fox" ? 0.35 : 0.25, toZ(a.y)));
      if (p.z <= 0) continue;
      const dx = p.x - px;
      const dy = p.y - py;
      // Bigger on screen when nearer: a generous click radius that grows as the camera closes in.
      const radius = (a.species === "fox" ? 1.4 : 1) * (14 + 500 / p.z);
      const score = Math.sqrt(dx * dx + dy * dy) / radius;
      if (score < best) {
        best = score;
        hit = a.id;
      }
    }
    if (hit !== 0) {
      selectedId = hit;
      showStats = true;
    }
  }

  override onStart() {
    buildScene();
    reset();
    selectedId = (animals.find((a) => a.species === "fox") ?? animals[0]!).id;
    viewsStale = true;
  }

  override onUpdate(dt: number) {
    if (input.getKeyDown("Tab") || input.getKeyDown("KeyI")) showStats = !showStats;
    if (input.getKeyDown("KeyT")) speedUp = speedUp >= 4 ? 1 : speedUp * 2;
    if (input.getKeyDown("KeyN")) nextAnimal();
    if (input.getKeyDown("KeyF") && !this.free) follow = !follow;
    if (input.getKeyDown("KeyC")) this.setFree(!this.free);
    if (this.free && input.getKeyDown("Escape")) this.setFree(false);
    if (input.getKeyDown("KeyR")) {
      reset();
      selectedId = (animals.find((a) => a.species === "fox") ?? animals[0]!).id;
      viewsStale = true;
    }

    // Real time, a little brisker than life; faster with T. Capped so a slow frame doesn't spiral.
    this.acc += Math.min(dt, 0.1) * speedUp * 1.5;
    let steps = 0;
    while (this.acc >= 0.025 && steps < 12) {
      step(0.025);
      this.acc -= 0.025;
      steps++;
    }
    syncViews(dt);
    if (animals.find((a) => a.id === selectedId) === undefined) nextAnimal();

    const cam = Camera.main;
    if (!cam) return;
    if (input.getMouseButtonDown(0)) this.pick(cam);

    if (this.free) {
      // Free-roam. Mouse right turns right (yaw falls), mouse up looks up (pitch rises); W flies along
      // the view (-z at yaw 0), A / D strafe, E / Space up, Q down, Shift faster.
      const look = input.mouseDelta;
      this.yaw -= look.x * 0.12;
      this.pitch = Math.min(85, Math.max(-85, this.pitch - look.y * 0.12));
      const view = Quaternion.euler(this.pitch, this.yaw, 0);
      const right = Quaternion.euler(0, this.yaw, 0).rotate(Vector3.right);
      let move = view.rotate(Vector3.forward).scale(input.getAxisRaw("Vertical")).add(right.scale(input.getAxisRaw("Horizontal")));
      if (input.getKey("KeyE") || input.getKey("Space")) move = move.add(Vector3.up);
      if (input.getKey("KeyQ")) move = move.sub(Vector3.up);
      const fast = input.getKey("ShiftLeft") ? 3 : 1;
      const next = this.flyPos.add(move.scale(9 * fast * dt));
      this.flyPos = v3(next.x, Math.max(0.4, next.y), next.z);
      cam.transform.position = this.flyPos;
      cam.transform.rotation = view;
      return;
    }

    // Orbit with A / D (or the arrows), zoom with W / S or the wheel.
    this.yaw += input.getAxisRaw("Horizontal") * 70 * dt;
    this.distance = Math.min(42, Math.max(6, this.distance - input.getAxisRaw("Vertical") * 14 * dt - input.mouseScrollDelta.y * 1.5));
    const sel = animals.find((a) => a.id === selectedId);
    const want = follow && sel !== undefined ? v3(toX(sel.x), 0.3, toZ(sel.y)) : v3(0, 0, 0);
    this.center = this.center.add(want.sub(this.center).scale(Math.min(1, dt * 3)));
    const dir = Quaternion.euler(follow ? this.pitch + 10 : this.pitch, this.yaw, 0).rotate(Vector3.forward);
    cam.transform.position = this.center.sub(dir.scale(follow ? this.distance * 0.45 : this.distance));
    cam.transform.lookAt(this.center);
  }
}

register({ Meadow });

// --- Stats overlay ---------------------------------------------------------------------------

function Bar(props: { label: string; value: number; color: string }) {
  return (
    <box style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
      <text style={{ width: 54, color: "#9fb0cc", fontSize: 12 }}>{props.label}</text>
      <box style={{ width: 150, height: 8, borderRadius: 4, backgroundColor: "#243047" }}>
        <box style={{ width: 150 * Math.min(1, Math.max(0, props.value)), height: 8, borderRadius: 4, backgroundColor: props.color }} />
      </box>
    </box>
  );
}

function Hud() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 100);
    return () => clearInterval(id);
  }, []);

  const rabbits = animals.filter((a) => a.species === "rabbit").length;
  const foxes = animals.length - rabbits;
  const focus = animals.find((a) => a.id === selectedId);
  const chip: PlatypusStyle = {
    padding: "8px 14px",
    borderRadius: 10,
    backgroundColor: "rgba(10, 14, 22, 0.72)",
    border: "1px solid rgba(255,255,255,0.15)",
    flexDirection: "column",
    gap: 4,
  };
  return (
    // Full screen, but the pointer passes through to the world.
    <box style={{ width: "100%", height: "100%", padding: 16, gap: 10, pointerEvents: "none" }}>
      <box style={{ ...chip, alignSelf: "flex-start" }}>
        <text style={{ color: "#e8eefc", fontSize: 16, fontWeight: 700 }}>
          {rabbits} rabbits · {foxes} foxes · {Math.floor(clock)}s{speedUp > 1 ? " · x" + speedUp : ""}
        </text>
        <text style={{ color: "#9fb0cc", fontSize: 12 }}>
          {freeRoam
            ? "Free-roam: mouse look · WASD fly · Q/E down/up · Shift fast · click selects · C or Esc to leave"
            : "Click an animal · Tab stats · C free-roam · N next · F follow · T speed · R restart · A/D orbit · W/S wheel zoom"}
        </text>
      </box>
      {freeRoam && (
        <box style={{ position: "absolute", left: "50%", top: "50%", width: 14, height: 14, marginLeft: -7, marginTop: -7 }}>
          <box style={{ position: "absolute", left: 6, top: 0, width: 2, height: 14, backgroundColor: "rgba(255,255,255,0.8)" }} />
          <box style={{ position: "absolute", left: 0, top: 6, width: 14, height: 2, backgroundColor: "rgba(255,255,255,0.8)" }} />
        </box>
      )}
      {showStats && (
        <box style={{ ...chip, alignSelf: "flex-start", width: 320, gap: 6 }}>
          <text style={{ color: "#e8eefc", fontSize: 18, fontWeight: 800 }}>Wildlife</text>
          <text style={{ color: "#9fb0cc", fontSize: 12 }}>
            {births} born, {deaths} died
          </text>
          {focus !== undefined && (
            <box style={{ flexDirection: "column", gap: 4, padding: 10, borderRadius: 10, backgroundColor: "rgba(27, 36, 51, 0.9)" }}>
              <text style={{ color: "#e8eefc", fontSize: 15, fontWeight: 700 }}>
                {name(focus)} ({focus.male ? "male" : "female"}, {adult(focus) ? "adult" : "young"})
              </text>
              <Bar label="health" value={focus.health} color="#6bdc7b" />
              <Bar label="hunger" value={focus.hunger} color="#e8a23b" />
              <Bar label="thirst" value={focus.thirst} color="#4aa3e8" />
              <Bar label="tired" value={focus.tired} color="#9b8cf0" />
              <Bar label="libido" value={focus.libido} color="#ff8fc4" />
              <text style={{ color: "#9fb0cc", fontSize: 12 }}>Goal: {focus.goal}</text>
              {focus.plan.length === 0 ? (
                <text style={{ color: "#6b7a99", fontSize: 12 }}>No plan: wandering</text>
              ) : (
                focus.plan.map((p, i) => (
                  <text key={p + i} style={{ color: i === 0 ? "#5ec8ff" : "#e8eefc", fontSize: 13 }}>
                    {i + 1}. {p}
                  </text>
                ))
              )}
            </box>
          )}
          {log.map((l, i) => (
            <text key={i} style={{ color: "#9fb0cc", fontSize: 12 }}>
              {l}
            </text>
          ))}
        </box>
      )}
    </box>
  );
}

createRoot().render(<Hud />);
