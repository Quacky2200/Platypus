// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Sound as something you can see. Every noise (a footstep, a gunshot, a grenade's beeps and bang,
// a radio muttering in the back room) is an `Acoustics.emit` event: guards carry SoundSensors that
// hear it through the walls (quieter behind them, not at all out of reach) and turn to look. The
// same events are drawn two ways: rings that ripple across the picture through the walls (a custom
// post-process shader), and a radar in the corner that shows where each noise came from.
//
// WASD move · Shift run (loud) · C sneak (nearly silent) · Space fire · G throw a grenade · V toggle the ripples.

const SOUND = (name: string) => `assets/sounds/${name}.wav`;
const KIND_COLOR: Record<string, string> = {
  footstep: "#7bd88f",
  gunshot: "#ff6b5e",
  explosion: "#ff9f45",
  beep: "#ffd93d",
  source: "#7ab8ff",
  noise: "#c8d0e0",
};

const wallMaterial = new Material(Color.hex("#59616f"), Color.black, 0.1);
const floorMaterial = new Material(Color.hex("#2b3038"), Color.black, 0.2);

function wall(center: Vector3, size: Vector3): GameObject {
  const go = GameObject.createPrimitive("cube", { position: center, scale: size });
  go.getComponent(MeshRenderer)!.material = wallMaterial;
  go.addComponent(AudioOccluder, { openness: 0, absorption: 0.55, lowPass: 900 });
  return go;
}

let player: GameObject;
let camera: Camera;
const guards: Guard[] = [];

/** A guard: ears, and a head that turns to what it hears. */
class Guard extends Component {
  alert = 0;
  heading = 0;
  turnTo: Vector3 | null = null;
  lastLevel = 0;
  lastKind = "";
  private material!: Material;
  private sensor!: SoundSensor;

  override onStart() {
    this.sensor = this.getComponent(SoundSensor)!;
    this.material = this.gameObject!.getComponent(MeshRenderer)!.material!;
    this.sensor.onHear((event: SoundEvent, level: number) => {
      // Ignore the guard's own footsteps and what it cannot place.
      this.turnTo = event.position;
      this.alert = 3 + level * 4;
      this.lastLevel = level;
      this.lastKind = event.kind;
    });
    guards.push(this);
  }

  override onUpdate(dt: number) {
    if (this.turnTo) {
      const to = this.turnTo.sub(this.transform.position);
      const want = (Math.atan2(to.x, to.z) * 180) / Math.PI;
      let diff = ((want - this.heading + 540) % 360) - 180;
      this.heading += Math.sign(diff) * Math.min(Math.abs(diff), 240 * dt);
    } else {
      this.heading += 20 * dt;
    }
    this.transform.rotation = Quaternion.euler(0, this.heading, 0);
    this.alert = Math.max(0, this.alert - dt);
    if (this.alert === 0) this.turnTo = null;
    const a = Math.min(1, this.alert / 2);
    this.material.color = Color.hex("#3a6ea5").lerp(Color.hex("#ff8c42"), a);
  }

  get level() {
    return this.sensor.level;
  }
}

class Grenade extends Component {
  private time = 0;
  private nextBeep = 0.4;
  override onUpdate(dt: number) {
    this.time += dt;
    const fuse = 2.8;
    if (this.time >= this.nextBeep && this.time < fuse) {
      this.nextBeep += Math.max(0.12, 0.5 * (1 - this.time / fuse));
      this.getComponent(AudioSource)!.playOneShot(SOUND("beep"), 0.7);
      Acoustics.emit(this.transform.position, 14, "beep", this.gameObject!);
    }
    if (this.time >= fuse) {
      const at = this.transform.position;
      Acoustics.emit(at, 70, "explosion", this.gameObject!);
      AudioSource.playClipAtPoint(audio.load(SOUND("explosion")), at, 1);
      this.gameObject!.destroy();
    }
  }
}

class Player extends Component {
  private controller!: CharacterController;
  private stepTimer = 0;
  private facing = new Vector3(0, 0, 1);
  override onStart() {
    this.controller = this.getComponent(CharacterController)!;
  }
  override onUpdate(dt: number) {
    const x = (input.getKey("KeyD") || input.getKey("ArrowRight") ? 1 : 0) - (input.getKey("KeyA") || input.getKey("ArrowLeft") ? 1 : 0);
    const z = (input.getKey("KeyW") || input.getKey("ArrowUp") ? 1 : 0) - (input.getKey("KeyS") || input.getKey("ArrowDown") ? 1 : 0);
    let dir = new Vector3(x, 0, z);
    const moving = dir.magnitude > 0.01;
    if (moving) {
      dir = dir.normalized;
      this.facing = dir;
    }
    const run = input.getKey("ShiftLeft") || input.getKey("ShiftRight");
    const sneak = input.getKey("KeyC");
    const speed = run ? 6 : sneak ? 1.2 : 3;
    this.controller.move(dir.scale(speed * dt).add(new Vector3(0, -2 * dt, 0)));
    if (moving) {
      this.stepTimer -= dt;
      if (this.stepTimer <= 0) {
        this.stepTimer = run ? 0.28 : sneak ? 0.7 : 0.45;
        const loudness = run ? 16 : sneak ? 1.5 : 7;
        this.getComponent(AudioSource)!.playOneShot(SOUND("footstep"), run ? 0.8 : sneak ? 0.15 : 0.5);
        Acoustics.emit(this.transform.position, loudness, "footstep", this.gameObject!);
      }
    }
    if (input.getKeyDown("Space")) {
      this.getComponent(AudioSource)!.playOneShot(SOUND("gunshot"), 1);
      Acoustics.emit(this.transform.position, 45, "gunshot", this.gameObject!);
    }
    if (input.getKeyDown("KeyG")) this.throwGrenade();
  }
  private throwGrenade() {
    const g = GameObject.createPrimitive("sphere", {
      position: this.transform.position.add(this.facing.scale(0.8)).add(new Vector3(0, 0.6, 0)),
      scale: Vector3.one.scale(0.25),
    });
    g.getComponent(MeshRenderer)!.material = new Material(Color.hex("#3d5a3d"), Color.black, 0.3);
    const body = g.addComponent(Rigidbody, { mass: 0.5 });
    body.velocity = this.facing.scale(7).add(new Vector3(0, 3, 0));
    g.addComponent(AudioSource, { spatialBlend: 1, maxDistance: 40 });
    g.addComponent(Grenade);
  }
}

// ---- the sound pings, drawn over the picture ----

const PING_SHADER = `
fn ring(uv: vec2<f32>, p: vec4<f32>) -> vec3<f32> {
  let d2 = (uv - p.xy) * vec2<f32>(aspect(), 1.0);
  let d = length(d2);
  let r = exp(-pow((d - p.z) / 0.018, 2.0)) * p.w;
  let dir = select(vec2<f32>(0.0), d2 / d, d > 0.0001);
  return vec3<f32>(r, dir);
}
fn effect(uv: vec2<f32>, color: vec4<f32>) -> vec4<f32> {
  var glow = 0.0;
  var push = vec2<f32>(0.0);
  var r = ring(uv, p0); glow += r.x; push += r.yz * r.x;
  r = ring(uv, p1); glow += r.x; push += r.yz * r.x;
  r = ring(uv, p2); glow += r.x; push += r.yz * r.x;
  r = ring(uv, p3); glow += r.x; push += r.yz * r.x;
  r = ring(uv, p4); glow += r.x; push += r.yz * r.x;
  r = ring(uv, p5); glow += r.x; push += r.yz * r.x;
  r = ring(uv, p6); glow += r.x; push += r.yz * r.x;
  r = ring(uv, p7); glow += r.x; push += r.yz * r.x;
  let c = scene(uv - push * vec2<f32>(0.006 / aspect(), 0.006));
  return vec4<f32>(c.rgb + vec3<f32>(1.0, 0.72, 0.28) * glow * 0.8, c.a);
}`;

const PINGS = 8;
const ping = PostProcess.createCustom(PING_SHADER, Array.from({ length: PINGS }, (_, i) => `p${i}:vec4`));
let pingsOn = true;

/** Writes the newest noises as expanding rings: where on screen, how far they have spread, how strong. */
function drawPings() {
  ping.enabled = pingsOn;
  const n = Acoustics.eventCount;
  const slots: number[][] = [];
  for (let i = n - 1; i >= 0 && slots.length < PINGS; i--) {
    const e = Acoustics.eventAt(i);
    if (!e || e.pulse) continue;
    const age = e.age;
    const life = Acoustics.eventLifetime;
    if (age > life) continue;
    const s = camera.worldToScreenPoint(e.position);
    if (s.z <= 0) continue;
    // Spread at the speed of sound would take no time on screen: show it at a readable pace,
    // growing to about the noise's reach in screen terms.
    const reach = Math.min(0.9, e.loudness / 60);
    const radius = reach * Math.min(1, age / 1.2);
    slots.push([s.x / Screen.width, 1 - s.y / Screen.height, radius, (1 - age / life) * 0.9]);
  }
  for (let i = 0; i < PINGS; i++) {
    const v = slots[i] ?? [0, 0, 0, 0];
    ping.setVector(`p${i}`, v[0]!, v[1]!, v[2]!, v[3]!);
  }
}

class RadarWorld extends Component {
  override onStart() {
    RenderSettings.ambientMode = "color";
    RenderSettings.ambientLight = new Color(0.55, 0.58, 0.66, 1);
    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-60, 20, 0) });
    sun.addComponent(Light, { type: "directional", intensity: 0.6 });

    // The level: a floor, an outer wall and rooms off a corridor.
    const floor = GameObject.createPrimitive("cube", { position: new Vector3(0, -0.25, 0), scale: new Vector3(44, 0.5, 34) });
    floor.getComponent(MeshRenderer)!.material = floorMaterial;
    const H = 2.2;
    wall(new Vector3(0, H / 2, -17), new Vector3(44, H, 0.4));
    wall(new Vector3(0, H / 2, 17), new Vector3(44, H, 0.4));
    wall(new Vector3(-22, H / 2, 0), new Vector3(0.4, H, 34));
    wall(new Vector3(22, H / 2, 0), new Vector3(0.4, H, 34));
    // Corridor walls along z = -4 and z = 4, with doorways.
    for (const z of [-4, 4]) {
      wall(new Vector3(-16, H / 2, z), new Vector3(11, H, 0.3));
      wall(new Vector3(-2, H / 2, z), new Vector3(8, H, 0.3));
      wall(new Vector3(12, H / 2, z), new Vector3(14, H, 0.3));
    }
    wall(new Vector3(-8, H / 2, -10.5), new Vector3(0.3, H, 13));
    wall(new Vector3(8, H / 2, 10.5), new Vector3(0.3, H, 13));

    // A radio in the back room: always muttering, heard by sensors in range (loudness) and, through
    // the walls, as a dull voice by the player.
    const radio = GameObject.createPrimitive("cube", { position: new Vector3(15, 0.5, 11), scale: new Vector3(0.5, 0.3, 0.2) });
    radio.getComponent(MeshRenderer)!.material = new Material(Color.hex("#c0392b"), Color.black, 0.3);
    radio.addComponent(AudioSource, {
      clip: audio.load(SOUND("voice")),
      loop: true,
      spatialBlend: 1,
      minDistance: 1.5,
      maxDistance: 30,
      effects: true,
      loudness: 18,
    }).play();

    // Guards: capsules with a nose, each a SoundSensor.
    for (const [x, z, sens] of [
      [-12, -10, 1],
      [-15, 10, 1],
      [14, -9, 1.4],
      [18, 11, 1],
    ] as const) {
      const g = GameObject.createPrimitive("capsule", { position: new Vector3(x, 1, z), scale: new Vector3(0.7, 1, 0.7) });
      g.name = "Guard";
      g.getComponent(MeshRenderer)!.material = new Material(Color.hex("#3a6ea5"), Color.black, 0.3);
      const nose = GameObject.createPrimitive("cube", { parent: g, position: new Vector3(0, 0.45, 0.45), scale: new Vector3(0.2, 0.2, 0.5) });
      nose.getComponent(MeshRenderer)!.material = new Material(Color.hex("#e8eefc"), Color.black, 0.3);
      g.addComponent(SoundSensor, { sensitivity: sens, threshold: 0.04 });
      g.addComponent(Guard);
    }

    // The player and the camera above.
    player = new GameObject("Player", { position: new Vector3(0, 1, 0) });
    player.getComponent(MeshRenderer);
    const body = GameObject.createPrimitive("capsule", { parent: player, scale: new Vector3(0.7, 1, 0.7) });
    body.getComponent(MeshRenderer)!.material = new Material(Color.hex("#7bd88f"), Color.black, 0.3);
    player.addComponent(CharacterController, { radius: 0.3, height: 1.8 });
    player.addComponent(AudioSource, { spatialBlend: 0 });
    player.addComponent(AudioListener);
    player.addComponent(Player);

    const cam = new GameObject("Main Camera", { position: new Vector3(0, 24, 8), rotation: Quaternion.euler(70, 0, 0) });
    camera = cam.addComponent(Camera, { fieldOfView: 50, backgroundColor: Color.hex("#10131a") });
  }

  override onLateUpdate() {
    const p = player.transform.position;
    camera.transform.position = new Vector3(p.x, 24, p.z + 8);
    if (input.getKeyDown("KeyV")) pingsOn = !pingsOn;
    drawPings();
  }
}

register({ RadarWorld, Guard, Grenade, Player });

function Radar() {
  const [, tick] = useState(0);
  useEffect(() => {
    let id = 0;
    const loop = () => {
      tick((n) => n + 1);
      id = setTimeout(loop, 50);
    };
    loop();
    return () => clearTimeout(id);
  }, []);
  if (!player) return <box />;
  const size = 200;
  const range = 40;
  const me = player.transform.position;
  const rings: { x: number; y: number; radius: number; fade: number; color: string }[] = [];
  for (let i = 0; i < Acoustics.eventCount; i++) {
    const e = Acoustics.eventAt(i);
    if (!e || e.pulse) continue;
    rings.push({
      x: size / 2 + ((e.position.x - me.x) / range) * (size / 2),
      y: size / 2 + ((e.position.z - me.z) / range) * (size / 2),
      radius: Math.min(1, (e.loudness / range) * Math.min(1, e.age / 1.2)) * (size / 2),
      fade: 1 - e.age / Acoustics.eventLifetime,
      color: KIND_COLOR[e.kind] ?? "#c8d0e0",
    });
  }
  const dots = rings.map((r, i) => (
    <box
      key={i}
      style={{ position: "absolute", left: r.x - r.radius, top: r.y - r.radius, width: r.radius * 2, height: r.radius * 2, borderRadius: r.radius, border: `2px solid ${r.color}`, opacity: r.fade }}
    />
  ));
  return (
    <box style={{ width: "100%", height: "100%", padding: 18, pointerEvents: "none", justifyContent: "space-between" }}>
      <box style={{ alignSelf: "flex-start", padding: 14, gap: 6, borderRadius: 10, backgroundColor: "rgba(10,14,22,0.72)", flexDirection: "column" }}>
        <text style={{ color: "#ffd93d", fontSize: 18, fontWeight: 700 }}>Audio: radar</text>
        <text style={{ color: "#8f9bb3", fontSize: 12 }}>WASD move · Shift run · C sneak · Space fire · G grenade · V ripples {pingsOn ? "on" : "off"}</text>
        {guards.map((g, i) => (
          <box key={i} style={{ flexDirection: "row", gap: 8, alignItems: "center" }}>
            <text style={{ color: "#c8d0e0", fontSize: 12, width: 60 }}>Guard {i + 1}</text>
            <box style={{ width: 90, height: 6, borderRadius: 3, backgroundColor: "rgba(255,255,255,0.15)" }}>
              <box style={{ width: `${Math.round(Math.min(1, g.level) * 100)}%`, height: 6, borderRadius: 3, backgroundColor: g.alert > 0 ? "#ff8c42" : "#3a6ea5" }} />
            </box>
            <text style={{ color: "#8f9bb3", fontSize: 11 }}>{g.alert > 0 ? `heard ${g.lastKind}` : "calm"}</text>
          </box>
        ))}
      </box>
      <box style={{ alignSelf: "flex-end", width: size, height: size, borderRadius: size / 2, backgroundColor: "rgba(10,20,14,0.78)", border: "2px solid rgba(123,216,143,0.5)", overflow: "hidden" }}>
        {dots}
        <box style={{ position: "absolute", left: size / 2 - 3, top: size / 2 - 3, width: 6, height: 6, borderRadius: 3, backgroundColor: "#7bd88f" }} />
      </box>
    </box>
  );
}

createRoot().render(<Radar />);
