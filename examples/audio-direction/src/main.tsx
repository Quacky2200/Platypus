// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Which way a sound faces, and how far it has come. A loudspeaker on a turntable plays a tune with
// the polar pattern of a microphone, turned round: omni (the same everywhere), cardioid,
// supercardioid, hypercardioid, figure-eight (front and back, nothing at the sides) and
// ultra-directional (a narrow beam with side lobes). Walk round it, or let it sweep past you, and hear
// the sound swell in front, thin out at the sides and go dull and quiet behind. The plot shows the
// pattern and where you stand on it. Walk (or run) away across the field: air takes the highs off with
// distance, the farther the duller (`Acoustics.airAbsorption`).
//
// WASD move (Shift runs) · mouse look (Esc frees the cursor, click locks it) · Y invert look
// 1-6 pattern · M speaker: spins / aims at you / stays fixed (Z X turn it) · A air: none, light, normal, heavy
// K distance decay (mixer default, inverse like air, linear, inverse square) · H head cue (dulls what is behind you)

const SOUND = (name: string) => `assets/sounds/${name}.wav`;
const PATTERNS: { name: Directivity; label: string; key: string }[] = [
  { name: "omni", label: "Omni-directional", key: "1" },
  { name: "cardioid", label: "Cardioid", key: "2" },
  { name: "supercardioid", label: "Supercardioid", key: "3" },
  { name: "hypercardioid", label: "Hypercardioid", key: "4" },
  { name: "figureEight", label: "Figure-eight", key: "5" },
  { name: "ultraDirectional", label: "Ultra-directional", key: "6" },
];
const MODES = ["spins", "aims at you", "stays fixed"];
const AIR = [0, 0.5, 1, 2];
const AIR_LABEL = ["none", "light", "normal", "heavy"];
/** How the speaker's volume falls with distance beyond its 3 m minimum (K cycles them). */
const ROLLOFF: { name: Rolloff; label: string; factor: number }[] = [
  { name: "default", label: "Mixer default (gentle)", factor: 1 },
  { name: "inverse", label: "Inverse, as in air (-6 dB per doubling)", factor: 1 },
  { name: "linear", label: "Linear to silence at 420 m", factor: 1 },
  { name: "exponential", label: "Inverse square (-12 dB per doubling)", factor: 2 },
];
const MIN_DISTANCE = 3;
const MAX_DISTANCE = 420;

let patternIndex = 1;
let mode = 0;
let airIndex = 1;
let rolloffIndex = 1;
/** The head cue dulls sounds behind you, so an omni speaker seems to brighten when you face it: off here. */
let headCue = false;
let speaker: Speaker | null = null;
let player: Player | null = null;

class Speaker extends Component {
  yaw = 0;
  source!: AudioSource;
  override onUpdate(dt: number) {
    if (input.getKeyDown("KeyM")) mode = (mode + 1) % MODES.length;
    for (let i = 0; i < PATTERNS.length; i++) {
      if (input.getKeyDown(`Digit${i + 1}`)) patternIndex = i;
    }
    if (input.getKeyDown("KeyA")) {
      airIndex = (airIndex + 1) % AIR.length;
      Acoustics.airAbsorption = AIR[airIndex]!;
    }
    if (input.getKeyDown("KeyH")) {
      headCue = !headCue;
      Acoustics.headShadow = headCue ? 0.5 : 0;
    }
    if (input.getKeyDown("KeyK")) {
      rolloffIndex = (rolloffIndex + 1) % ROLLOFF.length;
      this.source.rolloff = ROLLOFF[rolloffIndex]!.name;
      this.source.rolloffFactor = ROLLOFF[rolloffIndex]!.factor;
      // The mixer's own falloff is set when the source starts playing: start it again.
      this.source.play();
    }
    this.source.directivity = PATTERNS[patternIndex]!.name;
    if (mode === 0) this.yaw += 40 * dt;
    if (mode === 2) {
      if (input.getKey("KeyZ")) this.yaw += 80 * dt;
      if (input.getKey("KeyX")) this.yaw -= 80 * dt;
    }
    if (mode === 1 && player) {
      // Turn to face the listener (smoothly: it follows when you walk).
      const to = player.position.sub(this.transform.position);
      const want = (Math.atan2(-to.x, -to.z) * 180) / Math.PI;
      const diff = ((want - this.yaw + 540) % 360) - 180;
      this.yaw += Math.sign(diff) * Math.min(Math.abs(diff), 120 * dt);
    }
    this.transform.rotation = Quaternion.euler(0, this.yaw, 0);
  }

  /** The unit vector the speaker faces (its -z). */
  get facing(): Vector3 {
    return this.transform.rotation.rotate(Vector3.forward);
  }
}

class Player extends Component {
  private controller!: CharacterController;
  cam!: GameObject;
  private yaw = 0;
  private pitch = 0;
  private vy = 0;
  private locked = true;
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
    if (this.locked || input.getMouseButton(0) || input.getMouseButton(1)) {
      const d = input.mouseDelta;
      this.yaw += d.x * 0.15;
      const up = this.invertLook ? -d.y : d.y;
      this.pitch = Math.min(70, Math.max(-60, this.pitch - up * 0.15));
    }
    if (input.getKey("KeyQ")) this.yaw -= 100 * dt;
    if (input.getKey("KeyR")) this.yaw += 100 * dt;
    const x = (input.getKey("KeyD") || input.getKey("ArrowRight") ? 1 : 0) - (input.getKey("KeyA") || input.getKey("ArrowLeft") ? 1 : 0);
    const z = (input.getKey("KeyW") || input.getKey("ArrowUp") ? 1 : 0) - (input.getKey("KeyS") || input.getKey("ArrowDown") ? 1 : 0);
    const yaw = (this.yaw * Math.PI) / 180;
    const pitch = (this.pitch * Math.PI) / 180;
    const fwd = new Vector3(Math.sin(yaw), 0, -Math.cos(yaw));
    const right = new Vector3(Math.cos(yaw), 0, Math.sin(yaw));
    let dir = fwd.scale(z).add(right.scale(x));
    if (dir.magnitude > 0.01) dir = dir.normalized;
    const speed = input.getKey("ShiftLeft") ? 14 : 3.2;
    this.vy = this.controller.isGrounded ? -0.3 : this.vy - 16 * dt;
    this.controller.move(dir.scale(speed * dt).add(new Vector3(0, this.vy * dt, 0)));
    this.cam.transform.rotation = Quaternion.lookRotation(new Vector3(Math.sin(yaw) * Math.cos(pitch), -Math.sin(pitch), -Math.cos(yaw) * Math.cos(pitch)));
    this.transform.rotation = Quaternion.lookRotation(fwd);
  }

  get position(): Vector3 {
    return this.cam.transform.position;
  }
}

function box(name: string, position: Vector3, scale: Vector3, color: string, collide = false, parent?: GameObject): GameObject {
  const go = parent
    ? GameObject.createPrimitive("cube", { parent, position, scale })
    : GameObject.createPrimitive("cube", { position, scale });
  go.name = name;
  go.getComponent(MeshRenderer)!.material = new Material(Color.hex(color), Color.black, 0.2);
  if (collide) go.addComponent(BoxCollider);
  return go;
}

class DirectionWorld extends Component {
  override onStart() {
    RenderSettings.ambientMode = "color";
    RenderSettings.ambientLight = new Color(0.5, 0.52, 0.58, 1);
    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-50, 25, 0) });
    sun.addComponent(Light, { type: "directional", intensity: 0.7, shadows: "soft", shadowStrength: 0.6 });
    Acoustics.airAbsorption = AIR[airIndex]!;
    Acoustics.headShadow = headCue ? 0.5 : 0;

    // A wide plain, with a post every 25 m along the way away from the speaker, to see how far you are.
    box("Ground", new Vector3(0, -0.5, 150), new Vector3(900, 1, 900), "#6f8f5a", true);
    for (let i = 1; i <= 16; i++) {
      box(`Post ${i * 25}`, new Vector3(-3, 1.5, i * 25), new Vector3(0.3, 3, 0.3), i % 4 === 0 ? "#ffd93d" : "#e8eefc");
    }

    // The speaker: a cabinet with a bright front face, its front the object's -z.
    const turntable = new GameObject("Speaker", { position: new Vector3(0, 0.9, 0) });
    box("Cabinet", new Vector3(0, 0.2, 0), new Vector3(0.8, 1.4, 0.55), "#2c2f36", false, turntable);
    box("Front", new Vector3(0, 0.2, -0.3), new Vector3(0.7, 1.3, 0.06), "#ff8c42", false, turntable);
    for (const [y, r] of [[0.55, 0.18], [0.0, 0.28]] as const) {
      const cone = GameObject.createPrimitive("sphere", { parent: turntable, position: new Vector3(0, y, -0.34), scale: new Vector3(r * 2, r * 2, 0.08) });
      cone.getComponent(MeshRenderer)!.material = new Material(Color.hex("#1c1d22"), Color.black, 0.3);
    }
    box("Stand", new Vector3(0, -0.75, 0), new Vector3(0.25, 0.3, 0.25), "#40444d", false, turntable);
    const spk = turntable.addComponent(Speaker);
    spk.source = turntable.addComponent(AudioSource, {
      clip: audio.load(SOUND("radio_music")),
      loop: true,
      spatialBlend: 1,
      minDistance: MIN_DISTANCE,
      maxDistance: MAX_DISTANCE,
      directivity: PATTERNS[patternIndex]!.name,
      rolloff: ROLLOFF[rolloffIndex]!.name,
      rolloffFactor: ROLLOFF[rolloffIndex]!.factor,
    });
    spk.source.play();
    speaker = spk;

    // The listener.
    const p = new GameObject("Player", { position: new Vector3(0, 0.95, 12) });
    p.addComponent(CharacterController, { radius: 0.3, height: 1.8, stepOffset: 0.4 });
    const eyes = new GameObject("Eyes", { parent: p, position: new Vector3(0, 0.7, 0) });
    eyes.addComponent(Camera, { fieldOfView: 70, backgroundColor: Color.hex("#a9d0f0"), nearClipPlane: 0.05, farClipPlane: 900 });
    eyes.addComponent(AudioListener);
    const pl = p.addComponent(Player);
    pl.cam = eyes;
    player = pl;
  }
}

register({ DirectionWorld });
register({ Speaker, Player }, { spawn: false });

// ---- the polar plot ----

const PLOT = 150;
const R = 60;

/** The pattern as a closed path, front (0 degrees) up, drawn from the mixer's own curve. */
function polarPath(pattern: Directivity): string {
  const pts: string[] = [];
  for (let a = 0; a <= 360; a += 4) {
    const rad = (a * Math.PI) / 180;
    const r = R * Acoustics.directivityGain(pattern, a > 180 ? a - 360 : a);
    pts.push(`${(r * Math.sin(rad)).toFixed(1)} ${(-r * Math.cos(rad)).toFixed(1)}`);
  }
  return `M ${pts.join(" L ")} Z`;
}

function Hud() {
  const [, tick] = useState(0);
  useEffect(() => {
    let id = 0;
    const loop = () => {
      tick((n) => n + 1);
      id = setTimeout(loop, 60);
    };
    loop();
    return () => clearTimeout(id);
  }, []);
  if (!speaker || !player) return <box />;
  const pattern = PATTERNS[patternIndex]!;
  const ear = player.position;
  const at = speaker.transform.position;
  const to = ear.sub(at);
  const distance = to.magnitude;
  const facing = speaker.facing;
  const unit = to.normalized;
  const cos = Math.max(-1, Math.min(1, facing.dot(unit)));
  const side = facing.x * unit.z - facing.z * unit.x;
  const angle = ((Math.acos(cos) * 180) / Math.PI) * (side < 0 ? -1 : 1);
  const gain = Acoustics.directivityGain(pattern.name, angle);
  const rad = (angle * Math.PI) / 180;
  const dotX = R * gain * Math.sin(rad);
  const dotY = -R * gain * Math.cos(rad);
  const air = Acoustics.airCutoff(distance);
  const decay = ROLLOFF[rolloffIndex]!;
  const decayGain = Acoustics.rolloffGain(decay.name, distance, MIN_DISTANCE, MAX_DISTANCE, decay.factor);
  const decayDb = decayGain > 0.00001 ? 20 * Math.log10(decayGain) : -100;
  return (
    <box style={{ width: "100%", height: "100%", padding: 18, pointerEvents: "none" }}>
      <box style={{ alignSelf: "flex-start", padding: 14, gap: 8, borderRadius: 10, backgroundColor: "rgba(10,14,22,0.74)", flexDirection: "column" }}>
        <text style={{ color: "#ffd93d", fontSize: 18, fontWeight: 700 }}>Audio: direction</text>
        <box style={{ flexDirection: "row", gap: 14 }}>
          <svg viewBox={`${-PLOT / 2} ${-PLOT / 2} ${PLOT} ${PLOT}`} style={{ width: PLOT, height: PLOT }}>
            <circle cx={0} cy={0} r={R} fill="none" stroke="rgba(255,255,255,0.18)" strokeWidth={1} />
            <circle cx={0} cy={0} r={R / 2} fill="none" stroke="rgba(255,255,255,0.12)" strokeWidth={1} />
            <path d={`M 0 ${-R - 8} L 0 ${R + 8} M ${-R - 8} 0 L ${R + 8} 0`} fill="none" stroke="rgba(255,255,255,0.12)" strokeWidth={1} />
            <path d={polarPath(pattern.name)} fill="rgba(255,140,66,0.28)" stroke="#ff8c42" strokeWidth={2} />
            <circle cx={dotX} cy={dotY} r={5} fill="#7bd88f" />
          </svg>
          <box style={{ flexDirection: "column", gap: 4, width: 230 }}>
            <text style={{ color: "#e8eefc", fontSize: 15, fontWeight: 600 }}>{pattern.label}</text>
            <text style={{ color: "#c8d0e0", fontSize: 12 }}>You are {Math.abs(Math.round(angle))}° off its front</text>
            <text style={{ color: "#c8d0e0", fontSize: 12 }}>Pattern: {Math.round(gain * 100)}% of full</text>
            <text style={{ color: "#c8d0e0", fontSize: 12 }}>Distance: {distance.toFixed(0)} m</text>
            <text style={{ color: "#c8d0e0", fontSize: 12 }}>Decay: {decay.label}</text>
            <text style={{ color: "#c8d0e0", fontSize: 12 }}>Volume at this distance: {decayDb.toFixed(0)} dB</text>
            <text style={{ color: "#c8d0e0", fontSize: 12 }}>Air leaves {Math.round(air / 100) / 10} kHz ({AIR_LABEL[airIndex]})</text>
            <text style={{ color: "#8f9bb3", fontSize: 12 }}>Speaker {MODES[mode]}</text>
          </box>
        </box>
        <text style={{ color: "#8f9bb3", fontSize: 12 }}>WASD move · Shift runs · mouse look (Esc frees) · Y invert</text>
        <text style={{ color: "#8f9bb3", fontSize: 12 }}>1-6 pattern · M speaker mode · Z X turn it · A air · K decay · H head cue {headCue ? "on" : "off"}</text>
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
