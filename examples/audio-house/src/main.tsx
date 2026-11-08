// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// A two-storey house that sounds like one. Walls, floors and doors are AudioOccluders: a radio
// behind a shut door is dull and quiet, with the door open it is clear, and one upstairs is heard
// through the floor, but it comes down the stairwell when you stand in the east room: AudioPortals
// mark the doorways and the stairwell, and a source behind a wall is heard from the opening it
// bends through, duller, from the right direction. Grab the portable radio
// (E) and carry it about, or shut it in the cupboard (F at the door) and listen to it muffle.
//
// Walk: WASD or arrows. Look: the mouse (the cursor is locked: Esc frees it, a click locks it again; Q / R also turn, Y inverts). E: grab or drop the radio.
// F: open or shut the nearest door. Outside the walls a wind bed fades in (an inverted AudioZone).

const HOUSE = { x0: -7, x1: 7, z0: -5, z1: 5, storey: 3 };
const SOUND = (name: string) => `assets/sounds/${name}.wav`;

const wallMaterial = new Material(Color.hex("#d9d2c3"), Color.black, 0.15);
const floorMaterial = new Material(Color.hex("#8a6b4a"), Color.black, 0.3);
const woodMaterial = new Material(Color.hex("#6b4a2d"), Color.black, 0.35);

/** A solid box with a collider, optionally something sound cannot pass. */
function slab(name: string, center: Vector3, size: Vector3, material: Material, occludes = true, solid = true): GameObject {
  const go = GameObject.createPrimitive("cube", { position: center, scale: size });
  if (solid) go.addComponent(BoxCollider);
  go.name = name;
  go.getComponent(MeshRenderer)!.material = material;
  if (occludes) go.addComponent(AudioOccluder, { openness: 0, absorption: 0.6, lowPass: 800 });
  return go;
}

/** A door on a hinge: its occluder opens as it swings, so sound follows the picture. */
class Door extends Component {
  opened = false;
  private amount = 0;
  /** The occluder on the panel: how open it is is how much sound gets through. */
  panelOccluder!: AudioOccluder;
  /** The yaw (degrees) it swings by when open, and the yaw it rests at when shut. */
  swing = 95;
  baseYaw = 0;
  toggle() {
    this.opened = !this.opened;
    const sound = this.opened ? "door" : "knock";
    this.gameObject!.getComponent(AudioSource)?.playOneShot(SOUND(sound), 0.8);
  }
  get open() {
    return this.amount;
  }
  override onUpdate(dt: number) {
    const target = this.opened ? 1 : 0;
    this.amount += Math.sign(target - this.amount) * Math.min(Math.abs(target - this.amount), dt * 1.6);
    this.transform.localRotation = Quaternion.euler(0, this.baseYaw + this.swing * this.amount, 0);
    this.panelOccluder.openness = this.amount;
  }
}

type DoorEntry = { go: GameObject; door: Door; name: string };
const doors: DoorEntry[] = [];

/** A door in a wall: a hinge at `hinge`, a panel 2 m wide hanging off it along +z. */
function makeDoor(name: string, hinge: Vector3): GameObject {
  const pivot = new GameObject(`${name} hinge`, { position: hinge });
  const panel = GameObject.createPrimitive("cube", { parent: pivot, position: new Vector3(0, 1.1, 1.0), scale: new Vector3(0.12, 2.2, 2.0) });
  panel.name = name;
  panel.addComponent(BoxCollider);
  panel.getComponent(MeshRenderer)!.material = woodMaterial;
  panel.addComponent(AudioOccluder, { openness: 0, absorption: 0.7, lowPass: 650 });
  // The hinge turns, the panel (with its collider and occluder) goes with it.
  const door = pivot.addComponent(Door);
  // The Door reads the occluder on its panel.
  door.panelOccluder = panel.getComponent(AudioOccluder)!;
  pivot.addComponent(AudioSource, { spatialBlend: 1, maxDistance: 20 });
  doors.push({ go: pivot, door, name });
  return pivot;
}

class Radio extends Component {
  held = false;
  body!: Rigidbody;
  override onStart() {
    this.body = this.getComponent(Rigidbody)!;
  }
}

class Player extends Component {
  private controller!: CharacterController;
  cam!: GameObject;
  private yaw = 90;
  private pitch = 0;
  private vy = 0;
  radio!: Radio;
  hold: Radio | null = null;

  /** Mouse look with the cursor held (Esc lets go, a click takes it back). */
  private locked = true;
  /** Moving the mouse up looks up; Y flips it. */
  invertLook = false;

  override onStart() {
    this.controller = this.getComponent(CharacterController)!;
    Cursor.lockState = CursorLockMode.Locked;
  }

  override onUpdate(dt: number) {
    if (input.getKeyDown("Escape")) {
      this.locked = false;
      Cursor.lockState = CursorLockMode.None;
    } else if (!this.locked && input.getMouseButtonDown(0)) {
      this.locked = true;
      Cursor.lockState = CursorLockMode.Locked;
    }
    if (input.getKeyDown("KeyY")) this.invertLook = !this.invertLook;
    // Locked: every movement turns the view. Free: drag with a button held, as before.
    if (this.locked || input.getMouseButton(0) || input.getMouseButton(1)) {
      const d = input.mouseDelta;
      this.yaw += d.x * 0.15;
      // The mouse reports up as positive: up looks up (pitch is positive downwards).
      const up = this.invertLook ? -d.y : d.y;
      this.pitch = Math.min(70, Math.max(-60, this.pitch - up * 0.15));
    }
    if (input.getKey("KeyQ")) this.yaw -= 100 * dt;
    if (input.getKey("KeyR")) this.yaw += 100 * dt;
    const x = (input.getKey("KeyD") || input.getKey("ArrowRight") ? 1 : 0) - (input.getKey("KeyA") || input.getKey("ArrowLeft") ? 1 : 0);
    const z = (input.getKey("KeyW") || input.getKey("ArrowUp") ? 1 : 0) - (input.getKey("KeyS") || input.getKey("ArrowDown") ? 1 : 0);
    const yaw = (this.yaw * Math.PI) / 180;
    const fwd = new Vector3(Math.sin(yaw), 0, -Math.cos(yaw));
    const right = new Vector3(Math.cos(yaw), 0, Math.sin(yaw));
    let dir = fwd.scale(z).add(right.scale(x));
    const moving = dir.magnitude > 0.01;
    if (moving) dir = dir.normalized;
    const speed = input.getKey("ShiftLeft") ? 5 : 2.6;
    this.vy = this.controller.isGrounded ? -0.3 : this.vy - 16 * dt;
    this.controller.move(dir.scale(speed * dt).add(new Vector3(0, this.vy * dt, 0)));
    const pitch = (this.pitch * Math.PI) / 180;
    const look = new Vector3(Math.sin(yaw) * Math.cos(pitch), -Math.sin(pitch), -Math.cos(yaw) * Math.cos(pitch));
    this.cam.transform.rotation = Quaternion.lookRotation(look);
    this.transform.rotation = Quaternion.lookRotation(fwd);

    if (input.getKeyDown("KeyE")) this.grab();
    if (input.getKeyDown("KeyF")) this.useDoor();
    if (this.hold) this.carry(dt);
  }

  /** Picks up the radio in front of the player, or drops the one held. */
  private grab() {
    if (this.hold) {
      this.drop();
      return;
    }
    const eye = this.cam.transform.position;
    const look = this.cam.transform.rotation.rotate(Vector3.forward);
    const toRadio = this.radio.transform.position.sub(eye);
    if (toRadio.magnitude < 2.6 && toRadio.normalized.dot(look) > 0.7) {
      this.hold = this.radio;
      this.radio.held = true;
      this.radio.body.isKinematic = true;
    }
  }

  private drop() {
    const r = this.hold!;
    r.held = false;
    r.body.isKinematic = false;
    r.body.velocity = this.cam.transform.rotation.rotate(Vector3.forward).scale(1.5);
    this.hold = null;
  }

  /** The held radio hangs in front of the camera, easing into place. */
  private carry(dt: number) {
    const r = this.hold!;
    const eye = this.cam.transform.position;
    const look = this.cam.transform.rotation.rotate(Vector3.forward);
    const target = eye.add(look.scale(0.9)).add(new Vector3(0, -0.25, 0));
    r.transform.position = r.transform.position.lerp(target, Math.min(1, dt * 14));
    r.transform.rotation = Quaternion.lookRotation(new Vector3(Math.sin((this.yaw * Math.PI) / 180), 0, -Math.cos((this.yaw * Math.PI) / 180)));
  }

  private useDoor() {
    const p = this.transform.position;
    let best: DoorEntry | null = null;
    let bestDistance = 3.2;
    for (const d of doors) {
      const distance = d.go.transform.position.add(new Vector3(0, 0, 1)).sub(p).magnitude;
      const nearest = Math.min(distance, d.go.transform.position.sub(p).magnitude);
      if (nearest < bestDistance) {
        bestDistance = nearest;
        best = d;
      }
    }
    best?.door.toggle();
  }

  get position() {
    return this.cam.transform.position;
  }
}

/** Which room the point is in (for the HUD). */
function room(p: Vector3): string {
  const inside = p.x > HOUSE.x0 && p.x < HOUSE.x1 && p.z > HOUSE.z0 && p.z < HOUSE.z1;
  if (!inside) return "Outside";
  const floor = p.y > HOUSE.storey - 0.2 ? "Upstairs" : "Downstairs";
  if (p.x > 1.2 && p.x < 7 && p.z > -1 && p.z < 2) return `${floor}: stairwell`;
  return `${floor}: ${p.x < 0 ? "west room" : "east room"}`;
}

let player: Player | null = null;
let radioA: AudioSource | null = null; // upstairs, always playing
let radioB: AudioSource | null = null; // the portable one

class HouseWorld extends Component {
  override onStart() {
    RenderSettings.ambientMode = "color";
    RenderSettings.ambientLight = new Color(0.32, 0.33, 0.38, 1);
    // Ground and sun.
    // Its top is 2 cm under the house's ground-floor slab (y 0), so the two never share a plane
    // (they flickered against each other: z-fighting).
    const ground = GameObject.createPrimitive("cube", { position: new Vector3(0, -0.27, 0), scale: new Vector3(60, 0.5, 60) });
    ground.getComponent(MeshRenderer)!.material = new Material(Color.hex("#4d6b3c"), Color.black, 0.1);
    ground.addComponent(BoxCollider);
    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-48, 30, 0) });
    sun.addComponent(Light, { type: "directional", intensity: 0.7, shadows: "soft", shadowStrength: 0.7 });

    const { x0, x1, z0, z1, storey } = HOUSE;
    const t = 0.3;
    const H = storey;
    // Floors: the ground slab, and the first floor with a stairwell hole at x 1.2..7, z -1..2.
    slab("Ground floor", new Vector3(0, -0.1, 0), new Vector3(x1 - x0, 0.2, z1 - z0), floorMaterial, false);
    slab("Floor 1 main", new Vector3((x0 + 1.2) / 2, H, 0), new Vector3(1.2 - x0, t, z1 - z0), floorMaterial);
    slab("Floor 1 north", new Vector3((1.2 + x1) / 2, H, (z0 + -1) / 2), new Vector3(x1 - 1.2, t, -1 - z0), floorMaterial);
    slab("Floor 1 south", new Vector3((1.2 + x1) / 2, H, (2 + z1) / 2), new Vector3(x1 - 1.2, t, z1 - 2), floorMaterial);
    // Outer walls, both storeys (with a window-sized gap left out of nothing: shut house).
    for (const lvl of [0, 1]) {
      const y = lvl * H + H / 2 + (lvl === 0 ? 0 : t / 2);
      const h = H - (lvl === 0 ? 0 : t);
      slab(`Wall N${lvl}`, new Vector3(0, y, z0), new Vector3(x1 - x0, h, t), wallMaterial);
      slab(`Wall S${lvl}`, new Vector3(0, y, z1), new Vector3(x1 - x0, h, t), wallMaterial);
      slab(`Wall W${lvl}`, new Vector3(x0, y, 0), new Vector3(t, h, z1 - z0), wallMaterial);
      slab(`Wall E${lvl}`, new Vector3(x1, y, 0), new Vector3(t, h, z1 - z0), wallMaterial);
      // The partition at x = 0 with a doorway at z -1..1 (a door fills it).
      const py = y;
      slab(`Partition a${lvl}`, new Vector3(0, py, (z0 - 1) / 2), new Vector3(t, h, -1 - z0), wallMaterial);
      slab(`Partition b${lvl}`, new Vector3(0, py, (z1 + 1) / 2), new Vector3(t, h, z1 - 1), wallMaterial);
      slab(`Lintel${lvl}`, new Vector3(0, lvl * H + (lvl === 0 ? 2.6 : 2.9 + t / 2), 0), new Vector3(t, 0.8, 2.0), wallMaterial);
      makeDoor(lvl === 0 ? "Door downstairs" : "Door upstairs", new Vector3(0, lvl * H + (lvl === 0 ? 0 : t / 2), -1));
    }
    // Portals: the openings sound can bend through. A radio in the other room is heard from
    // the doorway (muffled by the door if it is shut), and one upstairs comes down the stairwell.
    for (const [name, p] of [
      ["Doorway downstairs", new Vector3(0, 1.1, 0)],
      ["Doorway upstairs", new Vector3(0, H + t / 2 + 1.1, 0)],
      ["Stairwell", new Vector3(4.1, H, 0.5)],
    ] as const) {
      new GameObject(name, { position: p }).addComponent(AudioPortal);
    }
    // A staircase filling the stairwell, climbing along +x from x 1.2 to 7: 17 gentle steps (0.19 m
    // each, well under the controller's step height) up to the first floor's top at the east end,
    // where you step off sideways onto the floor beside it.
    const steps = 17;
    const top = H + t / 2;
    const run = (x1 - 1.2) / steps;
    for (let i = 0; i < steps; i++) {
      const rise = (top / steps) * (i + 1);
      slab(`Step ${i}`, new Vector3(1.2 + run * (i + 0.5), rise / 2, 0.5), new Vector3(run, rise, 3.0), floorMaterial, false, false);
    }
    // The steps only look like steps: you walk up an invisible ramp along their nosings (about
    // 28 degrees, under the controller's slope limit), which a character controller climbs
    // reliably where it can stall on real risers.
    const angle = Math.atan2(top, x1 - 1.2);
    const ramp = new GameObject("Stair ramp", {
      position: new Vector3((1.2 + x1) / 2, top / 2, 0.5),
      rotation: Quaternion.euler(0, 0, (angle * 180) / Math.PI),
      scale: new Vector3(Math.hypot(x1 - 1.2, top), 0.2, 3.0),
    });
    ramp.addComponent(BoxCollider);

    // Lights.
    for (const [x, y, z] of [
      [-3.5, 2.5, 0],
      [3.5, 2.5, -3],
      [-3.5, 5.5, 0],
      [3.5, 5.5, -3],
    ] as const) {
      const lamp = new GameObject("Lamp", { position: new Vector3(x, y, z) });
      lamp.addComponent(Light, { type: "point", color: Color.hex("#ffd9a0"), intensity: 3, range: 8 });
    }

    // The upstairs radio: always on, a table's worth of box.
    const table = slab("Table up", new Vector3(-5, H + 0.45, -3), new Vector3(1.2, 0.9, 0.8), woodMaterial, false);
    const upstairs = GameObject.createPrimitive("cube", { parent: table, position: new Vector3(0, 0.62, 0), scale: new Vector3(0.45, 0.28, 0.2) });
    upstairs.name = "Radio upstairs";
    upstairs.getComponent(MeshRenderer)!.material = new Material(Color.hex("#c0392b"), Color.black, 0.4);
    radioA = upstairs.addComponent(AudioSource, {
      clip: audio.load(SOUND("radio_music")),
      loop: true,
      spatialBlend: 1,
      minDistance: 1.5,
      maxDistance: 22,
      effects: true,
      loudness: 22,
    });
    radioA.play();

    // The portable radio, downstairs in the west room, grabbable.
    const portable = GameObject.createPrimitive("cube", { position: new Vector3(-4.5, 1.2, 2.5), scale: new Vector3(0.42, 0.26, 0.18) });
    portable.name = "Portable radio";
    portable.addComponent(BoxCollider);
    portable.getComponent(MeshRenderer)!.material = new Material(Color.hex("#2e86c1"), Color.black, 0.4);
    const body = portable.addComponent(Rigidbody, { mass: 1.2 });
    const radio = portable.addComponent(Radio);
    radio.body = body;
    radioB = portable.addComponent(AudioSource, {
      clip: audio.load(SOUND("radio_music")),
      loop: true,
      spatialBlend: 1,
      minDistance: 1,
      maxDistance: 20,
      pitch: 1.12,
      effects: true,
      loudness: 18,
    });
    radioB.play();
    slab("Table down", new Vector3(-4.5, 0.55, 2.5), new Vector3(1.4, 1.0, 1.0), woodMaterial, false);

    // A cupboard in the east room: five panels and a door. Put the radio in and shut it.
    const cx = 5.5;
    const cz = -4.1;
    slab("Cupboard back", new Vector3(cx, 0.6, cz - 0.4), new Vector3(1.0, 1.2, 0.08), woodMaterial);
    slab("Cupboard left", new Vector3(cx - 0.5, 0.6, cz), new Vector3(0.08, 1.2, 0.8), woodMaterial);
    slab("Cupboard right", new Vector3(cx + 0.5, 0.6, cz), new Vector3(0.08, 1.2, 0.8), woodMaterial);
    slab("Cupboard top", new Vector3(cx, 1.24, cz), new Vector3(1.08, 0.08, 0.88), woodMaterial);
    slab("Cupboard base", new Vector3(cx, 0.0, cz), new Vector3(1.08, 0.08, 0.88), woodMaterial);
    const cdoor = makeDoor("Cupboard door", new Vector3(cx - 0.5, 0, cz + 0.4));
    cdoor.transform.localScale = new Vector3(1, 0.54, 0.4);
    const cupboardDoor = cdoor.getComponent(Door)!;
    cupboardDoor.baseYaw = 90;
    cupboardDoor.swing = -105;

    // Outside the walls, wind: an AudioZone around the house, inverted, so it is loudest away from
    // the walls and fades in across them (the gradient is the box's outer edge to its core).
    const outside = new GameObject("Outside", { position: new Vector3(0, 3, 0) });
    outside.addComponent(AudioZone, {
      shape: "box",
      size: new Vector3(18, 12, 14),
      innerFraction: 0.75,
      curve: "smooth",
      invert: true,
      ambience: audio.load(SOUND("wind"), { stream: true }),
      ambienceVolume: 0.5,
    });

    // The player.
    const p = new GameObject("Player", { position: new Vector3(-4, 0.95, 0) });
    p.addComponent(CharacterController, { radius: 0.3, height: 1.8, stepOffset: 0.4 });
    const eyes = new GameObject("Eyes", { parent: p, position: new Vector3(0, 0.7, 0) });
    eyes.addComponent(Camera, { fieldOfView: 70, backgroundColor: Color.hex("#9bc4e8"), nearClipPlane: 0.05 });
    eyes.addComponent(AudioListener);
    player = p.addComponent(Player);
    player.radio = radio;
    player.cam = eyes;
  }
}

register({ HouseWorld });
register({ Door, Radio, Player }, { spawn: false });

function bar(v: number, color: string) {
  return (
    <box style={{ width: 120, height: 8, borderRadius: 4, backgroundColor: "rgba(255,255,255,0.15)" }}>
      <box style={{ width: `${Math.round(Math.max(0, Math.min(1, v)) * 100)}%`, height: 8, borderRadius: 4, backgroundColor: color }} />
    </box>
  );
}

function Hud() {
  const [, tick] = useState(0);
  useEffect(() => {
    let id = 0;
    const loop = () => {
      tick((n) => n + 1);
      id = setTimeout(loop, 100);
    };
    loop();
    return () => clearTimeout(id);
  }, []);
  if (!player || !radioA || !radioB) return <box />;
  const ear = player.position;
  const a = Acoustics.transmission(ear, radioA.transform.position);
  const b = Acoustics.transmission(ear, radioB.transform.position);
  return (
    <box style={{ width: "100%", height: "100%", padding: 18, pointerEvents: "none" }}>
      <box style={{ alignSelf: "flex-start", padding: 14, gap: 8, borderRadius: 10, backgroundColor: "rgba(10,14,22,0.72)", flexDirection: "column" }}>
        <text style={{ color: "#ffd93d", fontSize: 18, fontWeight: 700 }}>Audio: house</text>
        <text style={{ color: "#e8eefc", fontSize: 14 }}>{room(ear)}</text>
        <box style={{ flexDirection: "row", gap: 10, alignItems: "center" }}>
          <text style={{ color: "#c8d0e0", fontSize: 13, width: 120 }}>Upstairs radio</text>
          {bar(a, "#e74c3c")}
        </box>
        <box style={{ flexDirection: "row", gap: 10, alignItems: "center" }}>
          <text style={{ color: "#c8d0e0", fontSize: 13, width: 120 }}>{player.hold ? "Portable (held)" : "Portable radio"}</text>
          {bar(b, "#3498db")}
        </box>
        <text style={{ color: "#8f9bb3", fontSize: 12 }}>The bars show how much of each radio gets through to you.</text>
        <text style={{ color: "#8f9bb3", fontSize: 12 }}>WASD move · mouse look (Esc frees the cursor, click to lock) · Y invert · E grab / drop radio · F door</text>
        {doors.map((d) => (
          <text key={d.name} style={{ color: d.door.opened ? "#7be07b" : "#e0907b", fontSize: 12 }}>
            {d.name}: {d.door.opened ? "open" : "shut"}
          </text>
        ))}
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
