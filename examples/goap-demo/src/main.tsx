// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Two planners in a 3D village, one switch apart. A villager is given a goal and a planner works
// out the steps; the villager walks them out: to the shed for the axe, to the forest to chop, to
// the camp to build the fire.
//
// - GOAP (`Goap.plan`) searches for the cheapest list of actions from the facts that are true now.
// - HTN (`Htn.plan`) follows recipes the author wrote: a compound task such as GetWood has methods
//   tried in order (use what you have, spend a coin, or chop), each breaking it into subtasks. The
//   result is the first plan the author's order allows, and the overlay shows how it was broken down.
//
// The two differ when you start with a coin: GOAP finds chopping a little cheaper (12 against 13),
// while this HTN's author put "spend a coin" first, so it buys the wood.
//
// Walking is A* (`NavGrid`) over the village with the buildings and trees blocked and the dirt roads
// cheap, so the villager walks round buildings and keeps to the roads where that is not much longer.

type Place = "shed" | "forest" | "market" | "camp" | "home";

// `stand` is where the villager waits for an action there; `site` is the building.
const PLACES: Record<Place, { stand: [number, number]; site: [number, number]; label: string }> = {
  shed: { stand: [-8, -6], site: [-10.5, -8.5], label: "Shed" },
  forest: { stand: [8, -6], site: [10.5, -8.5], label: "Forest" },
  market: { stand: [-8, 6], site: [-10.5, 8.5], label: "Market" },
  camp: { stand: [8, 6], site: [10.5, 8.5], label: "Campsite" },
  home: { stand: [0, 3], site: [0, -1.5], label: "Home" },
};
const ORDER: Place[] = ["shed", "forest", "market", "camp", "home"];

// --- The brain ----------------------------------------------------------------------------

// `go:<place>` actions walk; the others happen where the villager stands.
const brain = new Goap();
for (const p of ORDER) {
  // Walking is possible from anywhere but where you are.
  brain.addAction(
    "go:" + p,
    2,
    ["!at:" + p],
    ["at:" + p, ...ORDER.filter((o) => o !== p).map((o) => "!at:" + o)],
  );
}
brain.addAction("takeAxe", 1, ["at:shed", "!hasAxe"], ["hasAxe"]);
brain.addAction("chopWood", 3, ["at:forest", "hasAxe"], ["hasWood"]);
brain.addAction("buyWood", 7, ["at:market", "hasCoins"], ["hasWood", "!hasCoins"]);
brain.addAction("sellWood", 1, ["at:market", "hasWood"], ["hasCoins", "!hasWood"]);
brain.addAction("buildFire", 2, ["at:camp", "hasWood"], ["fire", "!hasWood"]);
brain.addAction("sleep", 2, ["at:home", "fire"], ["rested"]);

// The same steps as an HTN: primitive tasks are the actions, compound tasks the author's recipes.
const htn = new Htn();
for (const p of ORDER) {
  htn.addPrimitive("go:" + p, 2, [], ["at:" + p, ...ORDER.filter((o) => o !== p).map((o) => "!at:" + o)]);
  // At:<place>: nothing to do if already there, else walk.
  htn.addCompound("At:" + p);
  htn.addMethod("At:" + p, "already there", ["at:" + p], []);
  htn.addMethod("At:" + p, "walk", [], ["go:" + p]);
}
htn.addPrimitive("takeAxe", 1, ["at:shed"], ["hasAxe"]);
htn.addPrimitive("chopWood", 3, ["at:forest", "hasAxe"], ["hasWood"]);
htn.addPrimitive("buyWood", 7, ["at:market", "hasCoins"], ["hasWood", "!hasCoins"]);
htn.addPrimitive("sellWood", 1, ["at:market", "hasWood"], ["hasCoins", "!hasWood"]);
htn.addPrimitive("buildFire", 2, ["at:camp", "hasWood"], ["fire", "!hasWood"]);
htn.addPrimitive("sleep", 2, ["at:home", "fire"], ["rested"]);

htn.addCompound("GetAxe");
htn.addMethod("GetAxe", "have it", ["hasAxe"], []);
htn.addMethod("GetAxe", "fetch it", [], ["At:shed", "takeAxe"]);

// Methods in the author's order of preference: wood in hand, else spend a coin, else chop.
htn.addCompound("GetWood");
htn.addMethod("GetWood", "have some", ["hasWood"], []);
htn.addMethod("GetWood", "spend a coin", ["hasCoins"], ["At:market", "buyWood"]);
htn.addMethod("GetWood", "chop it", [], ["GetAxe", "At:forest", "chopWood"]);

// Every top-level recipe starts with "already done": a task that is replanned each frame must know when it
// has nothing left to do (GOAP gets that from its goal being met).
htn.addCompound("LightFire");
htn.addMethod("LightFire", "already lit", ["fire"], []);
htn.addMethod("LightFire", "the usual", [], ["GetWood", "At:camp", "buildFire"]);

htn.addCompound("EarnCoins");
htn.addMethod("EarnCoins", "already has one", ["hasCoins"], []);
htn.addMethod("EarnCoins", "sell firewood", [], ["GetWood", "At:market", "sellWood"]);

htn.addCompound("Rest");
htn.addMethod("Rest", "already rested", ["rested"], []);
htn.addMethod("Rest", "fire is lit", ["fire"], ["At:home", "sleep"]);
htn.addMethod("Rest", "light one first", [], ["LightFire", "At:home", "sleep"]);

type Goal = { label: string; literals: string[]; root: string };
const GOALS: Goal[] = [
  { label: "Light a fire", literals: ["fire"], root: "LightFire" },
  { label: "Earn coins", literals: ["hasCoins"], root: "EarnCoins" },
  { label: "Get some rest", literals: ["rested"], root: "Rest" },
];

/** Which planner is in charge: GOAP (cheapest) or HTN (the author's recipes). */
let useHtn = false;
/** Draw the villager's route on the ground. */
let showRoute = true;

function currentPlanner(goal: Goal): string[] {
  return useHtn ? htn.plan(facts, goal.root) : brain.plan(facts, goal.literals);
}

function canPlanNow(goal: Goal): boolean {
  return useHtn ? htn.canPlan(facts, goal.root) : brain.canPlan(facts, goal.literals);
}

const START = ["at:home"];

const DESCRIBE: Record<string, string> = {
  takeAxe: "Take the axe",
  chopWood: "Chop wood",
  buyWood: "Buy wood",
  sellWood: "Sell wood",
  buildFire: "Build a fire",
  sleep: "Sleep",
};

function describe(action: string): string {
  if (action.startsWith("go:")) return "Walk to " + PLACES[action.slice(3) as Place].label;
  return DESCRIBE[action] ?? action;
}

const EFFECTS: Record<string, string[]> = {
  takeAxe: ["hasAxe"],
  chopWood: ["hasWood"],
  buyWood: ["hasWood", "!hasCoins"],
  sellWood: ["hasCoins", "!hasWood"],
  buildFire: ["fire", "!hasWood"],
  sleep: ["rested"],
};

/** The facts after an action (the same literals the planner was given). */
function applyAction(current: string[], action: string): string[] {
  const set = new Set(current);
  if (action.startsWith("go:")) {
    for (const p of ORDER) set.delete("at:" + p);
    set.add("at:" + action.slice(3));
    return [...set];
  }
  for (const e of EFFECTS[action] ?? []) {
    if (e.startsWith("!")) set.delete(e.slice(1));
    else set.add(e);
  }
  return [...set];
}

// --- The villager's state, shared with the HUD -----------------------------------------------

let facts: string[] = START;
let coins = false;
let goalIndex = 0;
let busy = 0; // seconds into the current action

function restart(withCoins: boolean, index: number) {
  coins = withCoins;
  goalIndex = index;
  facts = withCoins ? [...START, "hasCoins"] : START;
  busy = 0;
  teleport = true;
}
let teleport = false;

function currentGoal(): Goal {
  return GOALS[goalIndex] ?? GOALS[0]!;
}

// --- The 3D scene --------------------------------------------------------------------------

const v3 = (x: number, y: number, z: number) => new Vector3(x, y, z);
const mat = (color: string, smoothness = 0.05, metallic = 0) =>
  Material.create({ color: Color.hex(color), smoothness, metallic });

const GRASS = mat("#5f9444");
const FAR_GRASS = mat("#436b34");
const WOOD = mat("#8a5f36");
const DARK_WOOD = mat("#5c3d22");
const PLASTER = mat("#e8dcc0");
const ROOF_RED = mat("#a8442e");
const ROOF_BLUE = mat("#3f5f8f");
const LEAVES = mat("#2f6b34");
const STONE = mat("#8b8d94", 0.1);
const AWNING_A = mat("#c8402e");
const AWNING_B = mat("#f0e8d6");
const SKIN = mat("#e8b896");
const SHIRT = mat("#3f7fbf");
const TROUSERS = mat("#4a3f33");
const HAIR = mat("#3a2a1e");
const EYE = mat("#0c0a0a", 0.9);
const GOLD = mat("#e8b93b", 0.6, 0.6);
const IRON = mat("#9aa0a8", 0.5, 0.7);
const FLAME = Material.create({ color: Color.hex("#ff8a2e"), smoothness: 0.2, metallic: 0, emission: Color.hex("#ff6a10") });
const PATH = mat("#b9a77e");

// --- Where the villager can walk -------------------------------------------------------------

// Half-metre cells over the 28 x 22 m green. Buildings and trees block cells, grass costs more to
// cross than the dirt roads, so A* keeps to the roads unless the way over the grass is much shorter.
const CELL = 0.5;
const GRID_X0 = -14;
const GRID_Z0 = -11;
const COLS = 56;
const ROWS = 44;
const GRASS_COST = 2.5;
const grid = new NavGrid(v3(COLS, 1, ROWS), v3(GRID_X0, 0, GRID_Z0), CELL);
for (let cz = 0; cz < ROWS; cz++) {
  for (let cx = 0; cx < COLS; cx++) grid.setWeight(v3(cx, 0, cz), GRASS_COST);
}

/** Blocks the cells a footprint touches, grown by `margin` metres so the villager clears the corners. */
function blockRect(x0: number, z0: number, x1: number, z1: number, margin: number) {
  grid.setBoxBlocked(v3(x0 - margin, 0.1, z0 - margin), v3(x1 + margin, 0.1, z1 + margin), true);
}

/** Makes the cells within `half` metres of a road segment cheap to walk. */
function layRoad(ax: number, az: number, bx: number, bz: number, half: number) {
  const dx = bx - ax;
  const dz = bz - az;
  const len2 = dx * dx + dz * dz;
  for (let cz = 0; cz < ROWS; cz++) {
    for (let cx = 0; cx < COLS; cx++) {
      const px = GRID_X0 + (cx + 0.5) * CELL;
      const pz = GRID_Z0 + (cz + 0.5) * CELL;
      const t = Math.min(1, Math.max(0, ((px - ax) * dx + (pz - az) * dz) / len2));
      const ex = ax + dx * t - px;
      const ez = az + dz * t - pz;
      if (ex * ex + ez * ez <= half * half) grid.setWeight(v3(cx, 0, cz), 1);
    }
  }
}

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

/** A tree: trunk and a stack of leaf blobs. */
function tree(x: number, z: number, height: number) {
  blockRect(x - 0.3, z - 0.3, x + 0.3, z + 0.3, 0.35);
  shape("Trunk", "cylinder", v3(x, height / 2, z), v3(0.4, height / 2, 0.4), DARK_WOOD);
  shape("Leaves", "sphere", v3(x, height + 0.7, z), v3(2.4, 2.2, 2.4), LEAVES);
  shape("Leaves", "sphere", v3(x, height + 1.9, z), v3(1.7, 1.6, 1.7), LEAVES);
}

/** A little house: walls, a two-sided roof, a door. */
function house(x: number, z: number, w: number, d: number, roof: Material) {
  blockRect(x - w / 2, z - d / 2, x + w / 2, z + d / 2, 0.35);
  shape("Walls", "cube", v3(x, 1.2, z), v3(w, 2.4, d), PLASTER);
  shape("Roof L", "cube", v3(x - w * 0.27, 2.85, z), v3(w * 0.62, 0.2, d + 0.5), roof, undefined, Quaternion.euler(0, 0, 28));
  shape("Roof R", "cube", v3(x + w * 0.27, 2.85, z), v3(w * 0.62, 0.2, d + 0.5), roof, undefined, Quaternion.euler(0, 0, -28));
  shape("Door", "cube", v3(x, 0.8, z + d / 2 + 0.02), v3(0.9, 1.6, 0.06), DARK_WOOD);
}

const billboards: GameObject[] = [];

function placeLabel(text: string, x: number, y: number, z: number) {
  const go = new GameObject("Label " + text, { position: v3(x, y, z) });
  const t = go.addComponent(TextMesh);
  t.fontSize = 64;
  t.color = Color.white;
  t.text = text;
  t.alignment = "center";
  billboards.push(go);
}

function buildVillage() {
  const cam = new GameObject("Main Camera", { position: v3(0, 16, 22) });
  cam.addComponent(Camera, { fieldOfView: 50, backgroundColor: Color.hex("#a9d0ee"), farClipPlane: 300 });
  const sun = new GameObject("Sun", { rotation: Quaternion.euler(-52, 30, 0) });
  sun.addComponent(Light, { type: "directional", color: new Color(1, 0.96, 0.88, 1), intensity: 1.35, shadows: "soft", shadowStrength: 0.8 });

  shape("Surround", "plane", v3(0, -0.02, 0), v3(10, 1, 8), FAR_GRASS);
  shape("Green", "plane", v3(0, 0, 0), v3(2.8, 1, 2.2), GRASS);

  // Dirt paths from home to each place.
  for (const p of ORDER) {
    if (p === "home") continue;
    const [sx, sz] = PLACES[p].stand;
    const [hx, hz] = PLACES.home.stand;
    const len = Math.sqrt((sx - hx) * (sx - hx) + (sz - hz) * (sz - hz));
    const yaw = (Math.atan2(sx - hx, sz - hz) * 180) / Math.PI;
    shape("Path", "cube", v3((sx + hx) / 2, 0.012, (sz + hz) / 2), v3(1.1, 0.02, len), PATH, undefined, Quaternion.euler(0, yaw, 0));
    layRoad(hx, hz, sx, sz, 0.7);
  }

  // Shed.
  const [shx, shz] = PLACES.shed.site;
  house(shx, shz, 3.2, 2.6, ROOF_RED);
  shape("Axe on the wall", "cube", v3(shx + 1.2, 1.2, shz + 1.34), v3(0.5, 0.12, 0.05), IRON);
  // Forest.
  const [fx, fz] = PLACES.forest.site;
  tree(fx, fz, 3.2);
  tree(fx + 2.4, fz - 1.2, 2.6);
  tree(fx - 2.6, fz + 0.4, 3.6);
  tree(fx + 1.2, fz + 2.2, 2.9);
  tree(fx - 1.2, fz - 2.4, 3.1);
  // Market: a stall with a striped awning and crates.
  const [mx, mz] = PLACES.market.site;
  for (const sx of [-1.6, 1.6]) for (const sz of [-0.9, 0.9]) shape("Post", "cylinder", v3(mx + sx, 1.1, mz + sz), v3(0.1, 1.1, 0.1), WOOD);
  shape("Table", "cube", v3(mx, 0.8, mz + 0.2), v3(3.0, 0.15, 1.0), WOOD);
  for (let i = 0; i < 6; i++) shape("Awning", "cube", v3(mx - 1.4 + i * 0.56, 2.35, mz), v3(0.56, 0.1, 2.2), i % 2 === 0 ? AWNING_A : AWNING_B);
  shape("Crate", "cube", v3(mx - 2.4, 0.4, mz + 0.8), v3(0.8, 0.8, 0.8), WOOD);
  shape("Crate", "cube", v3(mx + 2.4, 0.3, mz + 1.0), v3(0.6, 0.6, 0.6), WOOD);
  shape("Coin pile", "cylinder", v3(mx - 0.4, 0.92, mz + 0.2), v3(0.3, 0.04, 0.3), GOLD);
  blockRect(mx - 2.9, mz - 1.0, mx + 2.9, mz + 1.5, 0.3);
  // Camp: a ring of stones; the fire is lit by the plan.
  const [cx, cz] = PLACES.camp.site;
  for (let i = 0; i < 9; i++) {
    const a = (i / 9) * Math.PI * 2;
    shape("Stone", "sphere", v3(cx + Math.cos(a) * 0.9, 0.18, cz + Math.sin(a) * 0.9), v3(0.4, 0.3, 0.4), STONE);
  }
  shape("Log seat", "cylinder", v3(cx - 2, 0.3, cz + 0.6), v3(0.3, 0.9, 0.3), DARK_WOOD, undefined, Quaternion.euler(0, 0, 90));
  blockRect(cx - 1.0, cz - 1.0, cx + 1.0, cz + 1.0, 0.2);
  blockRect(cx - 2.9, cz + 0.3, cx - 1.1, cz + 0.9, 0.1);
  // Home.
  const [hx, hz] = PLACES.home.site;
  house(hx, hz, 4.2, 3.4, ROOF_BLUE);
  shape("Chimney", "cube", v3(hx + 1.2, 3.4, hz), v3(0.5, 1.2, 0.5), STONE);

  for (const p of ORDER) {
    const [x, z] = PLACES[p].site;
    placeLabel(PLACES[p].label, x, p === "forest" ? 7.2 : 4.4, z);
  }
}

// --- The villager --------------------------------------------------------------------------

class Villager {
  root: GameObject;
  armL: GameObject;
  armR: GameObject;
  legL: GameObject;
  legR: GameObject;
  axe: GameObject;
  log: GameObject;
  purse: GameObject;
  fire: GameObject;
  fireLight: GameObject;
  fireGlow: Light;
  x: number = PLACES.home.stand[0];
  z: number = PLACES.home.stand[1];
  yaw = 180;
  stride = 0;
  phase = 0;
  /** The route of the walk in progress: A* waypoints, the next one's index, and the walk it is for. */
  path: Vector3[] = [];
  pathIdx = 0;
  pathTo = "";

  constructor() {
    this.root = new GameObject("Villager", { position: v3(this.x, 0, this.z) });
    const r = this.root;
    shape("Torso", "capsule", v3(0, 0.98, 0), v3(0.36, 0.38, 0.28), SHIRT, r);
    shape("Head", "sphere", v3(0, 1.58, 0), v3(0.32, 0.34, 0.32), SKIN, r);
    shape("Hair", "sphere", v3(0, 1.68, -0.03), v3(0.34, 0.26, 0.34), HAIR, r);
    for (const sx of [-1, 1]) {
      shape("Eye", "sphere", v3(sx * 0.085, 1.6, 0.14), v3(0.05, 0.05, 0.04), EYE, r);
      shape("Glint", "sphere", v3(sx * 0.093, 1.615, 0.165), v3(0.014, 0.014, 0.014), AWNING_B, r);
    }
    shape("Nose", "sphere", v3(0, 1.55, 0.17), v3(0.05, 0.05, 0.05), SKIN, r);
    // Arms and legs hang from pivots, so turning the pivot swings the limb.
    const limb = (name: string, sx: number, y: number, len: number, material: Material, w: number) => {
      const pivot = new GameObject(name, { position: v3(sx, y, 0), parent: r });
      shape(name + " mesh", "cylinder", v3(0, -len / 2, 0), v3(w, len / 2, w), material, pivot);
      return pivot;
    };
    this.armL = limb("Arm L", -0.27, 1.32, 0.56, SHIRT, 0.08);
    this.armR = limb("Arm R", 0.27, 1.32, 0.56, SHIRT, 0.08);
    this.legL = limb("Leg L", -0.1, 0.66, 0.66, TROUSERS, 0.1);
    this.legR = limb("Leg R", 0.1, 0.66, 0.66, TROUSERS, 0.1);
    shape("Hand L", "sphere", v3(0, -0.6, 0), v3(0.1, 0.1, 0.1), SKIN, this.armL);
    shape("Hand R", "sphere", v3(0, -0.6, 0), v3(0.1, 0.1, 0.1), SKIN, this.armR);
    shape("Shoe L", "cube", v3(0, -0.7, 0.05), v3(0.14, 0.08, 0.26), DARK_WOOD, this.legL);
    shape("Shoe R", "cube", v3(0, -0.7, 0.05), v3(0.14, 0.08, 0.26), DARK_WOOD, this.legR);

    // What the villager carries, shown while the fact is true.
    this.axe = new GameObject("Axe", { position: v3(0, -0.62, 0.08), parent: this.armR });
    shape("Handle", "cylinder", v3(0, 0.1, 0.0), v3(0.035, 0.3, 0.035), WOOD, this.axe, Quaternion.euler(90, 0, 0));
    shape("Blade", "cube", v3(0, 0.1, 0.27), v3(0.04, 0.2, 0.14), IRON, this.axe);
    this.log = shape("Log", "cylinder", v3(0, 1.0, -0.3), v3(0.12, 0.38, 0.12), DARK_WOOD, r, Quaternion.euler(0, 0, 90));
    this.purse = shape("Purse", "sphere", v3(0.24, 0.72, 0.12), v3(0.14, 0.16, 0.14), GOLD, r);

    // The fire at the camp: flames and a light, lit while the fact `fire` is true.
    const [cx, cz] = PLACES.camp.site;
    this.fire = shape("Flame", "sphere", v3(cx, 0.55, cz), v3(0.6, 0.9, 0.6), FLAME);
    this.fireLight = new GameObject("Fire light", { position: v3(cx, 1.2, cz) });
    this.fireGlow = this.fireLight.addComponent(Light, { type: "point", color: Color.hex("#ffa050"), intensity: 0, range: 9, haloSize: 0 });
  }

  private show(go: GameObject, on: boolean, scale: Vector3) {
    go.transform.localScale = on ? scale : v3(0, 0, 0);
  }

  /** The rest of the current route, as a line on the ground. */
  drawPath() {
    if (this.pathTo === "" || this.path.length === 0) return;
    Gizmos.color = Color.hex("#ffffff");
    let last = v3(this.x, 0.08, this.z);
    for (let i = this.pathIdx; i < this.path.length; i++) {
      const p = v3(this.path[i]!.x, 0.08, this.path[i]!.z);
      Gizmos.drawLine(last, p);
      last = p;
    }
  }

  update(dt: number, time: number) {
    if (teleport) {
      this.x = PLACES.home.stand[0];
      this.z = PLACES.home.stand[1];
      this.yaw = 180;
      this.pathTo = "";
      teleport = false;
    }
    const plan = currentPlanner(currentGoal());
    const next = plan[0];
    let walking = false;
    let doing = "";
    if (next !== undefined) {
      if (next.startsWith("go:")) {
        const [tx, tz] = PLACES[next.slice(3) as Place].stand;
        // A new walk: find the route over the village (round buildings, along the roads).
        if (this.pathTo !== next) {
          this.path = grid.findPath(v3(this.x, 0.25, this.z), v3(tx, 0.25, tz), true);
          if (this.path.length === 0) this.path = [v3(tx, 0.25, tz)];
          this.pathIdx = 0;
          this.pathTo = next;
        }
        const dEnd = Math.sqrt((tx - this.x) * (tx - this.x) + (tz - this.z) * (tz - this.z));
        if (dEnd > 0.15) {
          // Head for the next waypoint, moving on when close to it.
          let wp = this.path[this.pathIdx]!;
          while (this.pathIdx < this.path.length - 1 && Math.abs(wp.x - this.x) + Math.abs(wp.z - this.z) < 0.5) {
            this.pathIdx++;
            wp = this.path[this.pathIdx]!;
          }
          const dx = wp.x - this.x;
          const dz = wp.z - this.z;
          const d = Math.max(1e-6, Math.sqrt(dx * dx + dz * dz));
          const stepLen = Math.min(d, 4.2 * dt);
          this.x += (dx / d) * stepLen;
          this.z += (dz / d) * stepLen;
          this.yaw += angleDiff((Math.atan2(dx, dz) * 180) / Math.PI, this.yaw) * Math.min(1, dt * 9);
          walking = true;
        } else {
          facts = applyAction(facts, next);
          this.pathTo = "";
        }
      } else {
        doing = next;
        busy += dt;
        if (busy >= 1.1) {
          facts = applyAction(facts, next);
          busy = 0;
        }
      }
    }

    this.stride += ((walking ? 1 : 0) - this.stride) * Math.min(1, dt * 8);
    if (walking) this.phase += dt * 9;
    const swing = Math.sin(this.phase) * 40 * this.stride;
    this.legL.transform.localRotation = Quaternion.euler(swing, 0, 0);
    this.legR.transform.localRotation = Quaternion.euler(-swing, 0, 0);
    this.armL.transform.localRotation = Quaternion.euler(-swing * 0.8, 0, 0);
    // Right arm: swings while walking, chops with the axe, or reaches out to buy and sell.
    let armR = -swing * 0.8;
    if (doing === "chopWood") armR = -110 + Math.sin(time * 13) * 55;
    if (doing === "buyWood" || doing === "sellWood" || doing === "takeAxe") armR = -80 + Math.sin(time * 6) * 15;
    if (doing === "buildFire") armR = -60 + Math.sin(time * 9) * 25;
    this.armR.transform.localRotation = Quaternion.euler(armR, 0, 0);

    const sleeping = doing === "sleep";
    this.root.transform.position = v3(this.x, sleeping ? 0.18 : Math.abs(Math.sin(this.phase)) * 0.04 * this.stride, this.z);
    this.root.transform.rotation = Quaternion.euler(sleeping ? -82 : 0, this.yaw, 0);

    this.show(this.axe, facts.includes("hasAxe"), v3(1, 1, 1));
    this.show(this.log, facts.includes("hasWood"), v3(0.12, 0.38, 0.12));
    this.show(this.purse, facts.includes("hasCoins"), v3(0.14, 0.16, 0.14));
    const lit = facts.includes("fire");
    const flicker = 1 + Math.sin(time * 17) * 0.12 + Math.sin(time * 29) * 0.07;
    this.show(this.fire, lit, v3(0.6 * flicker, 0.9 * flicker, 0.6 * flicker));
    // A light ignores its object's scale: switch it by its own settings.
    this.fireGlow.intensity = lit ? 7 * flicker : 0;
    this.fireGlow.haloSize = lit ? 1.1 : 0;
  }
}

function angleDiff(to: number, from: number): number {
  let d = (to - from) % 360;
  if (d > 180) d -= 360;
  if (d < -180) d += 360;
  return d;
}

class Village extends Component {
  private villager: Villager | null = null;
  private yaw = 0;
  private distance = 31;
  private t = 0;

  override onStart() {
    buildVillage();
    this.villager = new Villager();
  }

  override onUpdate(dt: number) {
    this.t += dt;
    if (this.villager !== null) {
      this.villager.update(Math.min(dt, 0.1), this.t);
      // Gizmo lines are drawn between `clear` and `present` for a camera.
      Gizmos.clear();
      if (showRoute) this.villager.drawPath();
    }

    const cam = Camera.main;
    if (!cam) return;
    Gizmos.present(cam);
    for (const b of billboards) b.transform.rotation = cam.transform.rotation;
    // Orbit with A / D (or the arrows); W / S or the wheel zoom.
    this.yaw += input.getAxisRaw("Horizontal") * 60 * dt;
    this.distance = Math.min(45, Math.max(8, this.distance - input.getAxisRaw("Vertical") * 12 * dt - input.mouseScrollDelta.y * 1.5));
    const dir = Quaternion.euler(-40, this.yaw, 0).rotate(Vector3.forward);
    // Look a little left of the middle so the overlay on the left doesn't cover the village.
    const centre = v3(-2, 0, 0);
    cam.transform.position = centre.sub(dir.scale(this.distance));
    cam.transform.lookAt(centre);
  }
}

register({ Village });

// --- The overlay ---------------------------------------------------------------------------

function Button(props: { label: string; active?: boolean; onPress: () => void }) {
  return (
    <box
      tabIndex={0}
      onClick={props.onPress}
      style={{
        padding: "8px 14px",
        borderRadius: 8,
        backgroundColor: props.active ? "#5ec8ff" : "#2a3446",
        cursor: "pointer",
        pointerEvents: "auto",
      }}
    >
      <text style={{ color: props.active ? "#0b1220" : "#e8eefc", fontSize: 15 }}>{props.label}</text>
    </box>
  );
}

function Hud() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 100);
    return () => clearInterval(id);
  }, []);

  const goal = currentGoal();
  const plan = currentPlanner(goal);
  const reachable = canPlanNow(goal);
  const status = plan.length === 0 ? (reachable ? "Done." : "No plan reaches this goal.") : "Plan (" + plan.length + " steps left)";
  return (
    <box style={{ width: "100%", height: "100%", padding: 16, pointerEvents: "none", alignItems: "flex-start" }}>
      <box
        style={{
          flexDirection: "column",
          gap: 8,
          width: 290,
          padding: 14,
          borderRadius: 12,
          backgroundColor: "rgba(10, 14, 22, 0.78)",
          border: "1px solid rgba(255,255,255,0.15)",
        }}
      >
        <text style={{ color: "#e8eefc", fontSize: 22, fontWeight: 800 }}>{useHtn ? "HTN villager" : "GOAP villager"}</text>
        <box style={{ flexDirection: "row", gap: 6 }}>
          <Button label="GOAP: cheapest" active={!useHtn} onPress={() => (useHtn = false)} />
          <Button label="HTN: recipes" active={useHtn} onPress={() => (useHtn = true)} />
        </box>
        <text style={{ color: "#9fb0cc", fontSize: 13 }}>Goal</text>
        {GOALS.map((g, i) => (
          <Button key={g.label} label={g.label} active={i === goalIndex} onPress={() => restart(coins, i)} />
        ))}
        <Button label={coins ? "Purse: has a coin" : "Purse: empty"} active={coins} onPress={() => restart(!coins, goalIndex)} />
        <text style={{ color: "#9fb0cc", fontSize: 13 }}>{status}</text>
        {plan.map((a, i) => (
          <text key={a + i} style={{ color: i === 0 ? "#5ec8ff" : "#e8eefc", fontSize: 15 }}>
            {i + 1}. {describe(a)}
          </text>
        ))}
        {useHtn && plan.length > 0 && (
          <box style={{ flexDirection: "column", gap: 2 }}>
            <text style={{ color: "#9fb0cc", fontSize: 12 }}>How the recipes broke it down:</text>
            {htn.methods().map((m, i) => (
              <text key={m + i} style={{ color: "#6b7a99", fontSize: 11 }}>
                {m}
              </text>
            ))}
          </box>
        )}
        <Button label={showRoute ? "Route: shown" : "Route: hidden"} active={showRoute} onPress={() => (showRoute = !showRoute)} />
        <text style={{ color: "#6b7a99", fontSize: 12 }}>Facts: {facts.filter((f) => !f.startsWith("at:")).join(", ") || "none"}</text>
        <text style={{ color: "#6b7a99", fontSize: 12 }}>A/D orbit · W/S or wheel zoom</text>
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
