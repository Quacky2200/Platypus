// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// What you hit decides how it sounds. You hold a tool (1 crowbar, 2 spanner, 3 wooden mallet, 4 a
// bunch of flowers) and swing it (click or Space) at things made of different stuff: a steel plate, a
// wooden crate, stone, the earth, glass. Each thing carries an `AudioSurface` (a name: "metal",
// "wood", ...). A ray from your eye finds what you hit; the sound is the surface's own body (a
// steel ring, a wooden knock, a glassy chime) with the tool's strike on top (a hard click, a light clink, a dull tok,
// the rustle of petals), played at the place you hit, softer for a gentler tool, with a different variant each time.
//
// WASD move (Shift runs) · mouse look (Esc frees the cursor, click locks it, then click to swing) · Y invert
// 1-4 tool · click / Space swing

const SOUND = (name: string) => `assets/sounds/${name}.wav`;

type ToolName = "crowbar" | "spanner" | "mallet" | "flowers";
const TOOLS: { name: ToolName; label: string; key: string; body: number; strike: number; pitch: number }[] = [
  // How much of the surface's own sound it brings out, how loud its own strike is, its pitch (mass).
  { name: "crowbar", label: "Crowbar", key: "1", body: 1.0, strike: 0.9, pitch: 0.92 },
  { name: "spanner", label: "Spanner", key: "2", body: 0.7, strike: 0.7, pitch: 1.18 },
  { name: "mallet", label: "Wooden mallet", key: "3", body: 0.85, strike: 0.8, pitch: 1.0 },
  { name: "flowers", label: "Flowers", key: "4", body: 0.07, strike: 0.6, pitch: 1.0 },
];
const VARIANTS = 3;
/** The hand at rest: the tool tilted a little forward and in. */
const REST = Quaternion.euler(-20, 0, -8);

const SURFACES: { name: string; label: string; color: string }[] = [
  { name: "metal", label: "Steel", color: "#9aa7b8" },
  { name: "wood", label: "Wood", color: "#9a6a3c" },
  { name: "stone", label: "Stone", color: "#8b8c90" },
  { name: "earth", label: "Earth", color: "#5b7f45" },
  { name: "glass", label: "Glass", color: "#aee4f2" },
];

let player: Player | null = null;
let toolIndex = 0;
let lookingAt = "";
let last = "";
let lastAt = -10;

/** What the ray from the eye hits: the point, and the surface name of the thing there. */
function strikeAt(origin: Vector3, direction: Vector3): { point: Vector3; surface: string } | null {
  const hit = Physics.raycast(origin, direction, 3.2);
  if (!hit) return null;
  const go = hit.collider.gameObject;
  const tag = go ? go.getComponent(AudioSurface) : null;
  return { point: hit.point, surface: tag ? tag.surface : "stone" };
}

class Player extends Component {
  private controller!: CharacterController;
  cam!: GameObject;
  hand!: GameObject;
  private yaw = 0;
  private pitch = 0;
  private vy = 0;
  private locked = true;
  invertLook = false;
  /** 0 when idle, then 0..1 through a swing. */
  private swing = -1;
  private struck = false;
  private groups: GameObject[] = [];

  override onStart() {
    this.controller = this.getComponent(CharacterController)!;
    Cursor.lockState = CursorLockMode.Locked;
  }

  setGroups(groups: GameObject[]) {
    this.groups = groups;
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
    if (this.locked) {
      const d = input.mouseDelta;
      this.yaw += d.x * 0.15;
      const up = this.invertLook ? -d.y : d.y;
      this.pitch = Math.min(70, Math.max(-60, this.pitch - up * 0.15));
    }
    for (let i = 0; i < TOOLS.length; i++) {
      if (input.getKeyDown(`Digit${i + 1}`) && this.swing < 0) toolIndex = i;
    }
    this.groups.forEach((g, i) => (g.enabled = i === toolIndex));

    const x = (input.getKey("KeyD") || input.getKey("ArrowRight") ? 1 : 0) - (input.getKey("KeyA") || input.getKey("ArrowLeft") ? 1 : 0);
    const z = (input.getKey("KeyW") || input.getKey("ArrowUp") ? 1 : 0) - (input.getKey("KeyS") || input.getKey("ArrowDown") ? 1 : 0);
    const yaw = (this.yaw * Math.PI) / 180;
    const pitch = (this.pitch * Math.PI) / 180;
    const fwd = new Vector3(Math.sin(yaw), 0, -Math.cos(yaw));
    const right = new Vector3(Math.cos(yaw), 0, Math.sin(yaw));
    let dir = fwd.scale(z).add(right.scale(x));
    if (dir.magnitude > 0.01) dir = dir.normalized;
    const speed = input.getKey("ShiftLeft") ? 6 : 3;
    this.vy = this.controller.isGrounded ? -0.3 : this.vy - 16 * dt;
    this.controller.move(dir.scale(speed * dt).add(new Vector3(0, this.vy * dt, 0)));
    const look = new Vector3(Math.sin(yaw) * Math.cos(pitch), -Math.sin(pitch), -Math.cos(yaw) * Math.cos(pitch));
    this.cam.transform.rotation = Quaternion.lookRotation(look);
    this.transform.rotation = Quaternion.lookRotation(fwd);

    // What you are looking at (for the HUD).
    const eye = this.cam.transform.position;
    const seen = strikeAt(eye, look);
    lookingAt = seen ? seen.surface : "";

    // The swing: raise, strike (at the middle of it), settle.
    if ((input.getKeyDown("Space") || (this.locked && input.getMouseButtonDown(0))) && this.swing < 0) {
      this.swing = 0;
      this.struck = false;
    }
    if (this.swing >= 0) {
      this.swing += dt / 0.3;
      const s = this.swing;
      // Up to -35 degrees, then down through 0 to +30 as it strikes, back to rest.
      const angle = s < 0.35 ? -35 * (s / 0.35) : s < 0.55 ? -35 + 65 * ((s - 0.35) / 0.2) : 30 * (1 - Math.min(1, (s - 0.55) / 0.45));
      this.hand.transform.localRotation = Quaternion.euler(angle - 20, 0, -8);
      if (s >= 0.55 && !this.struck) {
        this.struck = true;
        this.strike(eye, look);
      }
      if (s >= 1) {
        this.swing = -1;
        this.hand.transform.localRotation = REST;
      }
    }
  }

  private strike(eye: Vector3, look: Vector3) {
    const tool = TOOLS[toolIndex]!;
    const hit = strikeAt(eye, look);
    if (!hit) {
      // Nothing there: a swish.
      audio.play(SOUND(`tool_flowers_${Math.floor(Math.random() * VARIANTS)}`), { volume: 0.1, rate: 0.7 });
      return;
    }
    const variant = Math.floor(Math.random() * VARIANTS);
    const jitter = 0.96 + Math.random() * 0.08;
    // The surface's body, brought out by the tool, and the tool's own strike on top.
    const body = AudioSource.playClipAtPoint(audio.load(SOUND(`hit_${hit.surface}_${variant}`)), hit.point, tool.body);
    body.setRate(tool.pitch * jitter);
    AudioSource.playClipAtPoint(audio.load(SOUND(`tool_${tool.name}_${variant}`)), hit.point, tool.strike);
    last = `${tool.label} on ${hit.surface}`;
    lastAt = Time.time;
  }

  get position(): Vector3 {
    return this.cam.transform.position;
  }
}

function block(name: string, position: Vector3, scale: Vector3, color: string, surface: string): GameObject {
  const go = GameObject.createPrimitive("cube", { position, scale });
  go.name = name;
  go.getComponent(MeshRenderer)!.material = new Material(Color.hex(color), Color.black, surface === "metal" ? 0.8 : surface === "glass" ? 0.9 : 0.2);
  go.addComponent(BoxCollider);
  go.addComponent(AudioSurface, { surface });
  return go;
}

/** A held tool made of primitives, in the hand's space (it holds the near end). */
function makeTool(hand: GameObject, name: ToolName): GameObject {
  const g = new GameObject(`${name} model`, { parent: hand });
  g.transform.localScale = Vector3.one.scale(0.55);
  const part = (kind: "cube" | "sphere" | "cylinder", p: Vector3, s: Vector3, color: string, rot?: Quaternion) => {
    const o = GameObject.createPrimitive(kind, { parent: g, position: p, scale: s });
    o.getComponent(MeshRenderer)!.material = new Material(Color.hex(color), Color.black, 0.5);
    if (rot) o.transform.localRotation = rot;
  };
  if (name === "crowbar") {
    part("cube", new Vector3(0, 0.2, -0.05), new Vector3(0.05, 0.7, 0.05), "#b02a2a");
    part("cube", new Vector3(0.05, 0.57, -0.05), new Vector3(0.12, 0.05, 0.05), "#b02a2a", Quaternion.euler(0, 0, -25));
  } else if (name === "spanner") {
    part("cube", new Vector3(0, 0.15, -0.05), new Vector3(0.06, 0.5, 0.025), "#c9ced6");
    part("cube", new Vector3(0, 0.45, -0.05), new Vector3(0.14, 0.12, 0.025), "#c9ced6");
    part("cube", new Vector3(0, 0.48, -0.05), new Vector3(0.05, 0.08, 0.03), "#2a2d33");
  } else if (name === "mallet") {
    part("cube", new Vector3(0, 0.12, -0.05), new Vector3(0.05, 0.55, 0.05), "#8a5a2b");
    part("cube", new Vector3(0, 0.45, -0.05), new Vector3(0.14, 0.14, 0.22), "#6e4520");
  } else {
    part("cube", new Vector3(0, 0.15, -0.05), new Vector3(0.02, 0.5, 0.02), "#3f8f3f");
    for (const [x, y, c] of [[0, 0.48, "#ff6b9d"], [0.07, 0.42, "#ffd93d"], [-0.07, 0.4, "#b46bff"]] as const) {
      part("sphere", new Vector3(x, y, -0.05), new Vector3(0.1, 0.1, 0.1), c);
    }
  }
  return g;
}

class SurfacesWorld extends Component {
  override onStart() {
    RenderSettings.ambientMode = "color";
    RenderSettings.ambientLight = new Color(0.5, 0.52, 0.58, 1);
    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-50, 25, 0) });
    sun.addComponent(Light, { type: "directional", intensity: 0.75, shadows: "soft", shadowStrength: 0.6 });
    audio.play(SOUND("wind"), { loop: true, volume: 0.12 });

    // The ground is earth; the surfaces stand in a row to walk along, each a few things big.
    block("Ground", new Vector3(0, -0.5, 0), new Vector3(60, 1, 60), "#5b7f45", "earth");
    const row = [
      { surface: "metal", color: "#9aa7b8", size: new Vector3(2.2, 1.6, 0.25) },
      { surface: "wood", color: "#9a6a3c", size: new Vector3(1.6, 1.6, 1.0) },
      { surface: "stone", color: "#8b8c90", size: new Vector3(2.2, 1.4, 0.7) },
      { surface: "glass", color: "#aee4f2", size: new Vector3(2.0, 1.8, 0.1) },
    ];
    row.forEach((r, i) => {
      const x = (i - 1.5) * 3.2;
      block(`${r.surface} station`, new Vector3(x, r.size.y / 2, -4), r.size, r.color, r.surface);
    });
    // A few more of each, to try different places: a drum, a bench, a low wall, a pane.
    block("Metal drum", new Vector3(-6.5, 0.5, 1), new Vector3(0.9, 1.0, 0.9), "#6f7c8f", "metal");
    block("Wooden bench", new Vector3(6.5, 0.3, 1), new Vector3(2.0, 0.6, 0.6), "#7a4f2a", "wood");
    block("Stone wall", new Vector3(0, 0.4, 7), new Vector3(5.0, 0.8, 0.5), "#7d7e82", "stone");

    const p = new GameObject("Player", { position: new Vector3(0, 0.95, -1.4) });
    p.addComponent(CharacterController, { radius: 0.3, height: 1.8, stepOffset: 0.4 });
    const eyes = new GameObject("Eyes", { parent: p, position: new Vector3(0, 0.7, 0) });
    eyes.addComponent(Camera, { fieldOfView: 70, backgroundColor: Color.hex("#a9d0f0"), nearClipPlane: 0.05 });
    eyes.addComponent(AudioListener);
    // The hand: the tool rises from the lower right, tip up, and swings down at what you look at.
    const hand = new GameObject("Hand", { parent: eyes, position: new Vector3(0.32, -0.3, -0.6) });
    hand.transform.localRotation = REST;
    const pl = p.addComponent(Player);
    pl.cam = eyes;
    pl.hand = hand;
    pl.setGroups(TOOLS.map((t) => makeTool(hand, t.name)));
    player = pl;
  }
}

register({ SurfacesWorld });
register({ Player }, { spawn: false });

function Hud() {
  const [, tick] = useState(0);
  useEffect(() => {
    let id = 0;
    const loop = () => {
      tick((n) => n + 1);
      id = setTimeout(loop, 70);
    };
    loop();
    return () => clearTimeout(id);
  }, []);
  if (!player) return <box />;
  const fresh = Time.time - lastAt < 1.6;
  const surface = SURFACES.find((s) => s.name === lookingAt);
  return (
    <box style={{ width: "100%", height: "100%", padding: 18, pointerEvents: "none" }}>
      <box style={{ alignSelf: "flex-start", padding: 14, gap: 6, borderRadius: 10, backgroundColor: "rgba(10,14,22,0.74)", flexDirection: "column" }}>
        <text style={{ color: "#ffd93d", fontSize: 18, fontWeight: 700 }}>Audio: surfaces</text>
        <box style={{ flexDirection: "row", gap: 10 }}>
          {TOOLS.map((t, i) => (
            <text key={t.name} style={{ color: i === toolIndex ? "#ffd93d" : "#8f9bb3", fontSize: 13, fontWeight: i === toolIndex ? 700 : 400 }}>
              {t.key} {t.label}
            </text>
          ))}
        </box>
        <text style={{ color: "#c8d0e0", fontSize: 13 }}>Looking at: {surface ? surface.label : lookingAt ? lookingAt : "nothing in reach"}</text>
        <text style={{ color: fresh ? "#7bd88f" : "#8f9bb3", fontSize: 13 }}>{fresh ? `Struck: ${last}` : "Click or Space to swing"}</text>
        <text style={{ color: "#8f9bb3", fontSize: 12 }}>WASD move · mouse look (Esc frees) · Y invert · 1-4 tool</text>
      </box>
      <box style={{ position: "absolute", left: "50%", top: "50%", width: 6, height: 6, marginLeft: -3, marginTop: -3, borderRadius: 3, backgroundColor: "rgba(255,255,255,0.7)" }} />
    </box>
  );
}

createRoot().render(<Hud />);
