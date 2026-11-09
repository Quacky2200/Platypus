// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Rewind: a physics world you can run backwards. Knock a tower of crates down with the cannon,
// then hold R to rewind time: the crates (and the balls) fly back together, exactly as they fell,
// and let go to play on from there, perhaps with a better shot.
//
// How: every other physics step the world is remembered with `Physics.snapshot()` (a copy of every
// body's pose, speed and sleep state, the contacts, the joints) in a buffer of the last eight
// seconds. Rewinding takes the newest one off the buffer and `Physics.restore`s it each step; the
// objects follow their bodies. Stepping on from a restored world repeats exactly what it did
// before, so the tower falls the same way again if you fire the same shot.
//
// Keys: A / D turn the camera, W / S move it in and out, Space or the left mouse button fires,
// hold R to rewind (Shift for faster).

/** What the HUD draws. */
const hud = {
  rewinding: false,
  /** Seconds of history kept, and how far back the world is (0 at the newest). */
  kept: 0,
  back: 0,
  shots: 0,
  standing: 0,
  total: 0,
};
let refreshHud = () => {};

const SNAPSHOT_EVERY = 2; // steps between snapshots
const KEEP_SECONDS = 8;
const STEPS_PER_SECOND = 60;
const MAX_SNAPSHOTS = (KEEP_SECONDS * STEPS_PER_SECOND) / SNAPSHOT_EVERY;
const BALLS = 10;
const CRATE = 0.5;

class RewindGame extends Component {
  private yaw = 200;
  private distance = 9;
  private snaps: number[] = [];
  private stepCount = 0;
  private ticks = 0;
  private ballsAt: GameObject[] = [];
  private crates: GameObject[] = [];
  private next = 0;
  private home = new Vector3(0, 0, 0);

  override onStart() {
    const camObject = new GameObject("Main Camera");
    camObject.addComponent(Camera, { fieldOfView: 55, backgroundColor: Color.hex("#aab8c8") });
    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-48, 30, 0) });
    sun.addComponent(Light, { type: "directional", intensity: 1.1, shadows: "soft", shadowStrength: 0.8 });

    const floor = new GameObject("Floor");
    const mesh = new GameObject("Floor mesh", { parent: floor, scale: Vector3.one.scale(3) });
    mesh.addComponent(MeshRenderer, { mesh: "plane", material: new Material(Color.hex("#7e8b99"), Color.black, 0.3) });
    floor.addComponent(BoxCollider, { size: new Vector3(30, 0.2, 30), center: new Vector3(0, -0.1, 0) });

    // A wall of crates, six wide and seven high, in two shades so the fall is easy to follow.
    const light = new Material(Color.hex("#d9a066"), Color.black, 0.35);
    const dark = new Material(Color.hex("#a8703c"), Color.black, 0.35);
    for (let y = 0; y < 7; y++) {
      for (let x = 0; x < 6; x++) {
        const c = GameObject.createPrimitive("cube", {
          position: new Vector3((x - 2.5) * (CRATE + 0.02), CRATE / 2 + y * (CRATE + 0.001), -3),
          scale: Vector3.one.scale(CRATE),
        });
        c.addComponent(BoxCollider);
        c.addComponent(Rigidbody, { mass: 1 });
        c.getComponent(MeshRenderer)!.material = (x + y) % 2 === 0 ? light : dark;
        this.crates.push(c);
      }
    }
    // Cannonballs waiting in a row; a shot moves the next one to the cannon, so nothing is made
    // or destroyed in play and every ball is part of the world a snapshot keeps.
    const steel = new Material(Color.hex("#3b4252"), Color.black, 0.85);
    for (let i = 0; i < BALLS; i++) {
      const b = GameObject.createPrimitive("sphere", {
        position: new Vector3(-4.5 + i * 1.0, 0.3, 6),
        scale: Vector3.one.scale(0.55),
      });
      b.addComponent(SphereCollider);
      b.addComponent(Rigidbody, { mass: 6 });
      b.getComponent(MeshRenderer)!.material = steel;
      this.ballsAt.push(b);
    }
    hud.total = this.crates.length;
    this.placeCamera();
  }

  private placeCamera() {
    const cam = Camera.main;
    if (!cam) return;
    const a = (this.yaw * Math.PI) / 180;
    const target = new Vector3(0, 1.4, -3);
    const pos = new Vector3(target.x + Math.sin(a) * this.distance, 2.6, target.z + Math.cos(a) * this.distance);
    cam.transform.position = pos;
    cam.transform.lookAt(target);
  }

  /** Fires the next ball from the camera at what it looks at. */
  private fire() {
    const cam = Camera.main;
    if (!cam) return;
    const target = new Vector3(0, 1.4, -3);
    const from = cam.transform.position;
    const dir = target.sub(from).normalized;
    const ball = this.ballsAt[this.next % BALLS]!;
    this.next++;
    ball.transform.position = from.add(dir.scale(1.2));
    ball.getComponent(Rigidbody)!.velocity = dir.scale(26);
    hud.shots++;
  }

  override onUpdate(dt: number) {
    if (input.getKey("KeyA")) this.yaw -= 60 * dt;
    if (input.getKey("KeyD")) this.yaw += 60 * dt;
    if (input.getKey("KeyW")) this.distance = Math.max(4, this.distance - 6 * dt);
    if (input.getKey("KeyS")) this.distance = Math.min(16, this.distance + 6 * dt);
    this.placeCamera();
    // Firing while rewinding would be undone by the next restore: not allowed.
    if (!hud.rewinding && (input.getKeyDown("Space") || input.getMouseButtonDown(0))) this.fire();
    hud.standing = this.crates.filter((c) => c.transform.position.y < CRATE * 7 + 1 && c.transform.position.y > CRATE * 0.4 && Math.hypot(c.transform.position.x, c.transform.position.z + 3) < 3.2).length;
    hud.kept = (this.snaps.length * SNAPSHOT_EVERY) / STEPS_PER_SECOND;
    refreshHud();
  }

  override onFixedUpdate() {
    this.ticks++;
    if (this.ticks % 30 === 0 && Application.args.includes("--debug")) {
      console.log(`tick ${this.ticks}: crate0 y=${this.crates[0]!.transform.position.y.toFixed(3)} top x=${this.crates[41]!.transform.position.x.toFixed(3)} y=${this.crates[41]!.transform.position.y.toFixed(3)} standing=${hud.standing} rewinding=${hud.rewinding} back=${hud.back.toFixed(1)} snaps=${this.snaps.length}`);
    }
    // `--demo`: fires two shots by itself, then rewinds for a while, for screenshot runs.
    if (Application.args.includes("--demo")) {
      if (this.ticks === 60 || this.ticks === 100) this.fire();
    }
    const demoRewind = Application.args.includes("--demo") && this.ticks >= 260 && this.ticks < 400;
    const wantRewind = input.getKey("KeyR") || demoRewind;
    if (wantRewind && this.snaps.length > 1) {
      // Back by one snapshot (two steps) a step: twice real time, four times with Shift.
      const pops = input.getKey("ShiftLeft") || input.getKey("ShiftRight") ? 2 : 1;
      for (let i = 0; i < pops && this.snaps.length > 1; i++) Physics.dropSnapshot(this.snaps.pop()!);
      Physics.restore(this.snaps[this.snaps.length - 1]!);
      hud.rewinding = true;
      hud.back += (pops * SNAPSHOT_EVERY) / STEPS_PER_SECOND;
      return;
    }
    if (wantRewind) {
      // The start of the buffer: hold the world there.
      Physics.restore(this.snaps[0]!);
      hud.rewinding = true;
      return;
    }
    hud.rewinding = false;
    hud.back = 0;
    this.stepCount++;
    if (this.stepCount % SNAPSHOT_EVERY === 0 && !Application.args.includes("--nosnap")) {
      this.snaps.push(Physics.snapshot());
      if (this.snaps.length > MAX_SNAPSHOTS) Physics.dropSnapshot(this.snaps.shift()!);
    }
  }
}

register({ RewindGame });

addStyleSheet(`
  .tape { position: absolute; left: 0; top: 0; width: 100%; height: 100%; pointer-events: none;
    background-color: rgba(70, 120, 255, 0.14); }
`);

function Hud() {
  const [, setTick] = useState(0);
  useEffect(() => {
    refreshHud = () => setTick((t) => t + 1);
  }, []);
  const fill = Math.min(100, (hud.kept / KEEP_SECONDS) * 100);
  return (
    <box style={{ width: "100%", height: "100%", pointerEvents: "none" }}>
      {hud.rewinding && <box className="tape" />}
      <box style={{ position: "absolute", left: 16, top: 16, padding: "10px 14px", borderRadius: 10, backgroundColor: "rgba(20,28,40,0.72)", flexDirection: "column", gap: 4 }}>
        <text style={{ color: "#ffffff", fontSize: 18, fontWeight: 700 }}>{hud.rewinding ? "REWINDING" : "Rewind"}</text>
        <text style={{ color: "#c8d4e4", fontSize: 13 }}>{`shots ${hud.shots}  ·  crates still in the wall ${hud.standing} / ${hud.total}`}</text>
        <text style={{ color: "#c8d4e4", fontSize: 12 }}>A / D turn · W / S zoom · Space fire · hold R to rewind (Shift: faster)</text>
      </box>
      <box style={{ position: "absolute", left: "50%", bottom: 24, marginLeft: -220, width: 440, padding: 10, borderRadius: 10, backgroundColor: "rgba(20,28,40,0.72)", flexDirection: "column", gap: 6 }}>
        <text style={{ color: "#c8d4e4", fontSize: 12 }}>{`${hud.kept.toFixed(1)} s of history${hud.rewinding ? `  ·  ${hud.back.toFixed(1)} s back` : ""}`}</text>
        <box style={{ width: "100%", height: 8, borderRadius: 4, backgroundColor: "rgba(255,255,255,0.18)" }}>
          <box style={{ width: `${fill}%`, height: 8, borderRadius: 4, backgroundColor: hud.rewinding ? "#6ea0ff" : "#8fd19e" }} />
        </box>
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
