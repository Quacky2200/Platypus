// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// A swarm of rats in a walled maze, all heading for a piece of cheese. One search from the cheese
// outward (a flow field over a NavGrid) tells every cell which way to go; each rat just reads the
// field where it stands, so hundreds of rats cost one search. A `Crowd` moves them with a little
// separation so they spread through the gaps instead of piling up.
// Click the floor to move the cheese; B cycles the gate (open, half height, shut); G shows the
// flow field; Space adds rats. The field always routes through the gate, so the rats head for it
// and press against it when it is closed. A half-height gate can be climbed: with P on, the rats
// pile up against it and the one on top scrambles over, a few a second.

const COLS = 40;
const ROWS = 24;
const CELL = 0.5;
const HALF_X = (COLS * CELL) / 2;
const HALF_Z = (ROWS * CELL) / 2;
const WALL_HEIGHT = 0.9;
const MAX_RATS = 800;

/** Wall blocks as inclusive cell ranges [x0, z0, x1, z1]. */
const WALLS: number[][] = [
  [0, 0, 39, 0],
  [0, 23, 39, 23],
  [0, 0, 0, 23],
  [39, 0, 39, 23],
  [10, 1, 10, 8],
  [10, 13, 10, 22],
  [20, 1, 20, 5],
  [20, 10, 20, 22],
  [30, 1, 30, 14],
  [30, 19, 30, 22],
  [11, 15, 17, 15],
  [22, 8, 28, 8],
  [32, 8, 38, 8],
  [3, 10, 8, 10],
  [14, 5, 15, 6],
  [25, 16, 26, 17],
  [35, 12, 36, 13],
];
/** The gate across the gap in the middle wall. */
const GATE = [20, 6, 20, 9];

const cellX = (c: number) => (c + 0.5) * CELL - HALF_X;
const cellZ = (c: number) => (c + 0.5) * CELL - HALF_Z;
const v3 = (x: number, y: number, z: number) => new Vector3(x, y, z);

// --- The world and its flow field ------------------------------------------------------------

// What the rats walk on: a half-height or shut gate is solid here.
const grid = new NavGrid(v3(COLS, 1, ROWS), v3(-HALF_X, 0, -HALF_Z), CELL);
// What the flow field is built on: the same maze, but the gate never blocks it, so the rats always
// head for the gate by the shortest way (and pile up at it when it is closed).
const flowGrid = new NavGrid(v3(COLS, 1, ROWS), v3(-HALF_X, 0, -HALF_Z), CELL);

function setRect(g: NavGrid, r: number[], blocked: boolean) {
  for (let z = r[1]!; z <= r[3]!; z++) {
    for (let x = r[0]!; x <= r[2]!; x++) g.setBlocked(v3(x, 0, z), blocked);
  }
}
for (const w of WALLS) {
  setRect(grid, w, true);
  setRect(flowGrid, w, true);
}

let cheese: Vector3 = v3(cellX(36), 0.25, cellZ(20));
const field = new FlowField(flowGrid, [cheese]);
const crowd = new Crowd(grid, field, 0.13);
crowd.setGoalRadius(0.9);
/** 0 open, 1 half height (low enough to climb), 2 shut (a full wall). */
let gateMode = 0;
const GATE_NAMES = ["open", "half height", "shut"];
const GATE_HEIGHTS = [0, WALL_HEIGHT / 2, WALL_HEIGHT];
/** With a half-height gate: rats pile up against it and the top ones climb over. */
let pileUp = true;
let showField = false;

// The gate's faces and extent in metres (x across the wall, z along the gap).
const GATE_FACE_L = cellX(GATE[0]!) - CELL / 2;
const GATE_FACE_R = cellX(GATE[2]!) + CELL / 2;
const GATE_Z0 = cellZ(GATE[1]!) - CELL / 2;
const GATE_Z1 = cellZ(GATE[3]!) + CELL / 2;
/** Rats that have climbed over the gate (shown in the overlay). */
let climbed = 0;
/** How many rats must be pressing on the gate before the top one climbs. */
const PILE_NEEDED = 5;

function addRats(n: number) {
  for (let i = 0; i < n && crowd.count < MAX_RATS; i++) {
    // Out of an inlet in the top-left room.
    const x = cellX(2) + Random.range(0, 6) * CELL;
    const z = cellZ(2) + Random.range(0, 6) * CELL;
    crowd.add(v3(x, 0.25, z), Random.range(1.6, 2.6));
  }
}

function moveCheese(p: Vector3) {
  cheese = v3(p.x, 0.25, p.z);
  field.rebuild([cheese]);
}

function cycleGate() {
  gateMode = (gateMode + 1) % 3;
  setRect(grid, GATE, gateMode > 0);
}

// --- Looks -----------------------------------------------------------------------------------

const mat = (color: string, smoothness = 0.05, metallic = 0) =>
  Material.create({ color: Color.hex(color), smoothness, metallic });

const FLOOR = mat("#4a4f58", 0.15);
const BRICK = mat("#8a5240", 0.05);
const GATE_MAT = mat("#b8862e", 0.3, 0.4);
const CHEESE = mat("#f2c53d", 0.4);
const HOLE = mat("#c79a22", 0.3);
const RAT_FUR = [mat("#6b5b50"), mat("#4f4640"), mat("#857467"), mat("#3a3430")];
const PINK = mat("#d99a9a");
const EYE = mat("#0c0a0a", 0.9);

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

interface RatView {
  root: GameObject;
  yaw: number;
  phase: number;
}

/** A rat on its way over the gate (drawn by the script; the crowd moves it when it lands). */
interface Climb {
  i: number;
  t: number;
  from: Vector3;
  to: Vector3;
  /** How high the pile had lifted it when it set off. */
  y0: number;
}

function buildRat(i: number): RatView {
  const root = new GameObject("Rat " + i);
  const fur = RAT_FUR[i % RAT_FUR.length]!;
  shape("Body", "sphere", v3(0, 0.07, 0), v3(0.12, 0.1, 0.26), fur, root);
  shape("Head", "sphere", v3(0, 0.08, 0.15), v3(0.085, 0.08, 0.12), fur, root);
  shape("Nose", "sphere", v3(0, 0.075, 0.22), v3(0.03, 0.03, 0.03), PINK, root);
  shape("Ear L", "sphere", v3(-0.04, 0.14, 0.12), v3(0.04, 0.045, 0.012), PINK, root);
  shape("Ear R", "sphere", v3(0.04, 0.14, 0.12), v3(0.04, 0.045, 0.012), PINK, root);
  for (const sx of [-1, 1]) shape("Eye", "sphere", v3(sx * 0.035, 0.1, 0.19), v3(0.018, 0.018, 0.018), EYE, root);
  shape("Tail", "cylinder", v3(0, 0.05, -0.27), v3(0.012, 0.15, 0.012), PINK, root, Quaternion.euler(80, 0, 0));
  return { root, yaw: Random.range(0, 360), phase: Random.range(0, 6.28) };
}

function angleDiff(to: number, from: number): number {
  let d = (to - from) % 360;
  if (d > 180) d -= 360;
  if (d < -180) d += 360;
  return d;
}

// --- The scene -------------------------------------------------------------------------------

class Maze extends Component {
  private rats: RatView[] = [];
  private climbs: Climb[] = [];
  private sinceClimb = 0;
  private cheeseObj: GameObject | null = null;
  private gateObj: GameObject | null = null;
  private yaw = 0;
  private pitch = -62;
  private distance = 22;
  private t = 0;

  override onStart() {
    Random.initState(7);
    const cam = new GameObject("Main Camera", { position: v3(0, 14, 10) });
    cam.addComponent(Camera, { fieldOfView: 50, backgroundColor: Color.hex("#14161b"), farClipPlane: 200 });
    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-55, 25, 0) });
    sun.addComponent(Light, { type: "directional", color: new Color(1, 0.95, 0.88, 1), intensity: 1.3, shadows: "soft", shadowStrength: 0.85 });

    shape("Floor", "plane", v3(0, 0, 0), v3((COLS * CELL) / 10, 1, (ROWS * CELL) / 10), FLOOR);
    for (const w of WALLS) {
      const wx = ((w[0]! + w[2]! + 1) / 2) * CELL - HALF_X;
      const wz = ((w[1]! + w[3]! + 1) / 2) * CELL - HALF_Z;
      shape("Wall", "cube", v3(wx, WALL_HEIGHT / 2, wz), v3((w[2]! - w[0]! + 1) * CELL, WALL_HEIGHT, (w[3]! - w[1]! + 1) * CELL), BRICK);
    }
    const gx = ((GATE[0]! + GATE[2]! + 1) / 2) * CELL - HALF_X;
    const gz = ((GATE[1]! + GATE[3]! + 1) / 2) * CELL - HALF_Z;
    this.gateObj = shape("Gate", "cube", v3(gx, WALL_HEIGHT / 2, gz), v3(0, 0, 0), GATE_MAT);

    // The cheese: a wedge with a few holes.
    this.cheeseObj = new GameObject("Cheese", { position: v3(cheese.x, 0.14, cheese.z) });
    shape("Wedge", "cube", v3(0, 0, 0), v3(0.5, 0.22, 0.36), CHEESE, this.cheeseObj, Quaternion.euler(0, 20, 0));
    shape("Hole", "sphere", v3(0.12, 0.1, 0.1), v3(0.08, 0.04, 0.08), HOLE, this.cheeseObj);
    shape("Hole", "sphere", v3(-0.1, 0.1, -0.05), v3(0.06, 0.04, 0.06), HOLE, this.cheeseObj);

    addRats(300);
  }

  override onUpdate(dt: number) {
    this.t += dt;
    const cam = Camera.main;
    if (!cam) return;

    // Controls.
    if (input.getKeyDown("Space")) addRats(50);
    if (input.getKeyDown("KeyB")) cycleGate();
    if (input.getKeyDown("KeyP")) pileUp = !pileUp;
    if (input.getKeyDown("KeyG")) showField = !showField;
    if (input.getMouseButtonDown(0)) {
      const ray = cam.screenPointToRay(input.mousePosition);
      if (ray.direction.y < -0.001) {
        const hit = ray.origin.add(ray.direction.scale(-ray.origin.y / ray.direction.y));
        if (!grid.isBlocked(grid.worldToCell(hit))) moveCheese(hit);
      }
    }

    crowd.step(Math.min(dt, 0.05));

    // Rats: a view for each new agent; each faces where it is going and scurries.
    for (let i = this.rats.length; i < crowd.count; i++) this.rats.push(buildRat(i));
    const count = this.rats.length;
    const flat = crowd.positions();

    // Rats on their way over the gate land, and are put down on the far side.
    const still: Climb[] = [];
    const climbOf: (Climb | null)[] = [];
    for (let i = 0; i < count; i++) climbOf.push(null);
    for (const c of this.climbs) {
      c.t += dt / 0.9;
      if (c.t >= 1) {
        crowd.setPosition(c.i, c.to);
        climbed++;
      } else {
        still.push(c);
        climbOf[c.i] = c;
      }
    }
    this.climbs = still;

    // The pile: with a half-height gate, rats pressed against it that would get ahead by crossing
    // heap up (each lifted by how many are in front of it); the one on top climbs over.
    const lift: number[] = [];
    for (let i = 0; i < count; i++) lift.push(0);
    this.sinceClimb += dt;
    if (gateMode === 1 && pileUp) {
      const zone: number[] = [];
      const zoneD: number[] = [];
      const zoneLand: Vector3[] = [];
      for (let i = 0; i < count; i++) {
        if (climbOf[i] !== null) continue;
        const x = flat[i * 3]!;
        const z = flat[i * 3 + 2]!;
        if (z < GATE_Z0 - 0.3 || z > GATE_Z1 + 0.3) continue;
        let d = -1;
        let landX = 0;
        if (x < GATE_FACE_L && x > GATE_FACE_L - 1.4) {
          d = GATE_FACE_L - x;
          landX = GATE_FACE_R + 0.45;
        } else if (x > GATE_FACE_R && x < GATE_FACE_R + 1.4) {
          d = x - GATE_FACE_R;
          landX = GATE_FACE_L - 0.45;
        }
        if (d < 0) continue;
        const land = v3(landX, 0.25, Math.min(GATE_Z1 - 0.15, Math.max(GATE_Z0 + 0.15, z)));
        // Only rats that cross to a spot nearer the cheese climb (rats past the gate stay put).
        if (field.distance(land) < field.distance(v3(x, 0.25, z)) - 0.8) {
          zone.push(i);
          zoneD.push(d);
          zoneLand.push(land);
        }
      }
      const heap = Math.min(1, (zone.length - 2) / 4);
      let top = -1;
      for (let k = 0; k < zone.length; k++) {
        if (top < 0 || zoneD[k]! < zoneD[top]!) top = k;
        if (heap <= 0) continue;
        // A ramp: the nearer the gate, the higher up the pile.
        let behind = 0;
        for (let m = 0; m < zone.length; m++) if (zoneD[m]! > zoneD[k]!) behind++;
        lift[zone[k]!] = Math.min(0.42, 0.05 * (behind + 1)) * heap;
      }
      if (zone.length >= PILE_NEEDED && this.sinceClimb > 0.22 && top >= 0) {
        const i = zone[top]!;
        this.climbs.push({ i, t: 0, from: v3(flat[i * 3]!, 0, flat[i * 3 + 2]!), to: zoneLand[top]!, y0: lift[i]! });
        climbOf[i] = this.climbs[this.climbs.length - 1]!;
        this.sinceClimb = 0;
      }
    }

    for (let i = 0; i < count; i++) {
      const rat = this.rats[i]!;
      const climb = climbOf[i];
      if (climb !== null && climb !== undefined) {
        // Scrambling over the gate: along an arc, nose up.
        const e = climb.t * climb.t * (3 - 2 * climb.t);
        const x = climb.from.x + (climb.to.x - climb.from.x) * e;
        const z = climb.from.z + (climb.to.z - climb.from.z) * e;
        const y = climb.y0 * (1 - climb.t) + (GATE_HEIGHTS[1]! + 0.1) * Math.sin(Math.PI * climb.t);
        rat.yaw += angleDiff((Math.atan2(climb.to.x - climb.from.x, climb.to.z - climb.from.z) * 180) / Math.PI, rat.yaw) * Math.min(1, dt * 12);
        rat.phase += dt * 22;
        rat.root.transform.position = v3(x, y + Math.abs(Math.sin(rat.phase)) * 0.02, z);
        rat.root.transform.rotation = Quaternion.euler(-28, rat.yaw, 0);
        rat.root.transform.localScale = v3(1.5, 1.5, 1.5);
        continue;
      }
      const px = flat[i * 3]!;
      const pz = flat[i * 3 + 2]!;
      const vel = crowd.velocity(i);
      const speed = Math.sqrt(vel.x * vel.x + vel.z * vel.z);
      const moving = speed > 0.15;
      if (moving) rat.yaw += angleDiff((Math.atan2(vel.x, vel.z) * 180) / Math.PI, rat.yaw) * Math.min(1, dt * 12);
      let bob = 0;
      let pitch = 0;
      if (moving) {
        rat.phase += dt * (10 + speed * 6);
        bob = Math.abs(Math.sin(rat.phase)) * 0.025;
      } else if (lift[i]! > 0) {
        // Pressed in the pile, paws up on the rat in front.
        rat.phase += dt * 9;
        pitch = -14 + Math.sin(rat.phase + i) * 5;
      } else if (field.distance(v3(px, 0.25, pz)) < 1.5) {
        // At the cheese: nibbling.
        rat.phase += dt * 14;
        pitch = 18 + Math.sin(rat.phase) * 8;
      }
      rat.root.transform.position = v3(px, bob + lift[i]!, pz);
      rat.root.transform.rotation = Quaternion.euler(pitch, rat.yaw, 0);
      rat.root.transform.localScale = v3(1.5, 1.5, 1.5);
    }

    if (this.cheeseObj !== null) {
      this.cheeseObj.transform.position = v3(cheese.x, 0.14 + Math.sin(this.t * 2) * 0.02, cheese.z);
      this.cheeseObj.transform.rotation = Quaternion.euler(0, this.t * 25, 0);
    }
    if (this.gateObj !== null) {
      // Open: gone. Half height: a low wall the rats can pile up at. Shut: a full wall.
      const gh = GATE_HEIGHTS[gateMode]!;
      this.gateObj.transform.position = v3(this.gateObj.transform.position.x, gh / 2, this.gateObj.transform.position.z);
      this.gateObj.transform.localScale = gateMode > 0 ? v3((GATE[2]! - GATE[0]! + 1) * CELL, gh, (GATE[3]! - GATE[1]! + 1) * CELL) : v3(0, 0, 0);
    }

    // The flow field as short arrows, one per cell (gizmo lines are drawn between clear and present).
    Gizmos.clear();
    if (showField) {
      Gizmos.color = Color.hex("#5ec8ff");
      for (let z = 0; z < ROWS; z++) {
        for (let x = 0; x < COLS; x++) {
          const c = v3(cellX(x), 0.04, cellZ(z));
          const d = field.direction(c);
          if (d.x === 0 && d.z === 0) continue;
          Gizmos.drawLine(c.sub(d.scale(0.12)), c.add(d.scale(0.2)));
        }
      }
    }

    Gizmos.present(cam);

    // Orbit with A / D (or the arrows), zoom with W / S or the wheel.
    this.yaw += input.getAxisRaw("Horizontal") * 60 * dt;
    this.distance = Math.min(34, Math.max(5, this.distance - input.getAxisRaw("Vertical") * 10 * dt - input.mouseScrollDelta.y * 1.2));
    const dir = Quaternion.euler(this.pitch, this.yaw, 0).rotate(Vector3.forward);
    cam.transform.position = dir.scale(-this.distance);
    cam.transform.lookAt(v3(0, 0, 0));
    // Every five seconds, how many rats have reached the cheese (to Player.log).
    if (Math.floor(this.t / 5) !== Math.floor((this.t - dt) / 5)) {
      let near = 0;
      for (let i = 0; i < crowd.count; i++) if (field.distance(crowd.position(i)) < 2) near++;
      console.log("t=" + Math.floor(this.t) + "s rats=" + crowd.count + " near the cheese=" + near);
    }
  }
}

register({ Maze });

// --- The overlay -----------------------------------------------------------------------------

function Hud() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 150);
    return () => clearInterval(id);
  }, []);
  const hint: PlatypusStyle = { color: "#9fb0cc", fontSize: 12 };
  return (
    <box style={{ width: "100%", height: "100%", padding: 16, pointerEvents: "none", alignItems: "flex-start" }}>
      <box
        style={{
          flexDirection: "column",
          gap: 4,
          padding: "8px 14px",
          borderRadius: 10,
          backgroundColor: "rgba(10, 14, 22, 0.72)",
          border: "1px solid rgba(255,255,255,0.15)",
        }}
      >
        <text style={{ color: "#e8eefc", fontSize: 16, fontWeight: 700 }}>
          {crowd.count} rats · gate {GATE_NAMES[gateMode]}
          {gateMode === 1 ? (pileUp ? " · piling up and climbing (" + climbed + " over)" : " · queuing") : ""}
        </text>
        <text style={hint}>Click: move the cheese · B: gate (open, half height, shut) · P: pile up and climb</text>
        <text style={hint}>G: flow field · Space: +50 rats</text>
        <text style={hint}>A/D orbit · W/S or wheel zoom</text>
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
