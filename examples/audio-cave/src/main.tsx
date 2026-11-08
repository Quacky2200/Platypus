// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Walk a talking character in and out of a cave. The voice plays on a bus with effects, and
// AudioZones colour it by how deep inside you are: the cave takes the sharpness off, thickens it
// with reverb and an echo, with a gradient from the mouth (weak) to the core (full). Birds and wind
// fade out as you go in (an inverted zone), water drips fade in; at the shore the sea's waves swell
// with a gradient of their own. The zones are components: their radius, inner fraction and curve
// are the gradient controls (open the editor on this folder to drag them).
//
// A / D walk (or let it walk by itself: T) · 1 cave · 2 meadow · 3 shore

const SOUND = (name: string) => `assets/sounds/${name}.wav`;
const rock = new Material(Color.hex("#4a4540"), Color.black, 0.1);
const grass = new Material(Color.hex("#4f7d3a"), Color.black, 0.1);
const sand = new Material(Color.hex("#d8c79a"), Color.black, 0.1);
const water = new Material(Color.hex("#2e86c1"), Color.black, 0.8);

function block(name: string, center: Vector3, size: Vector3, material: Material): GameObject {
  const go = GameObject.createPrimitive("cube", { position: center, scale: size });
  go.name = name;
  go.getComponent(MeshRenderer)!.material = material;
  return go;
}

/** The layout along x: meadow, then the cave mouth, the cave to the west, the shore and sea to the east. */
const CAVE = { mouth: -8, back: -34 };
const SHORE = 40;

let walker: Walker;
let voiceBus: Bus;
let recorded: AudioSource;
let micOn = false;

/** Swaps the recorded talker for your own microphone on the same bus (so the cave colours it too). */
function toggleMic() {
  micOn = !micOn;
  if (micOn) {
    recorded.mute = true;
    Microphone.start({ bus: voiceBus, volume: 1 });
  } else {
    Microphone.stop();
    recorded.mute = false;
  }
}
let zones: { name: string; zone: AudioZone; color: string }[] = [];

class Walker extends Component {
  auto = true;
  private dir = -1;
  private x = 18;
  private stepTimer = 0;
  steps!: AudioSource;
  override onStart() {
    this.steps = this.getComponent(AudioSource)!;
  }
  override onUpdate(dt: number) {
    let move = (input.getKey("KeyD") || input.getKey("ArrowRight") ? 1 : 0) - (input.getKey("KeyA") || input.getKey("ArrowLeft") ? 1 : 0);
    if (input.getKeyDown("KeyT")) this.auto = !this.auto;
    if (input.getKeyDown("KeyM")) toggleMic();
    if (input.getKeyDown("Digit1")) this.x = CAVE.mouth - 14;
    if (input.getKeyDown("Digit2")) this.x = 14;
    if (input.getKeyDown("Digit3")) this.x = SHORE - 6;
    if (move !== 0) this.auto = false;
    if (this.auto) {
      // Pace between the back of the cave and the water's edge.
      if (this.x < CAVE.back + 3) this.dir = 1;
      if (this.x > SHORE - 3) this.dir = -1;
      move = this.dir;
    }
    this.x += move * 2.2 * dt;
    this.transform.position = new Vector3(this.x, 1, 0);
    if (move !== 0) this.transform.rotation = Quaternion.lookRotation(new Vector3(move, 0, 0));
    if (move !== 0) {
      this.stepTimer -= dt;
      if (this.stepTimer <= 0) {
        this.stepTimer = 0.5;
        this.steps.playOneShot(SOUND("footstep"), 0.5);
      }
    }
  }
  get x_() {
    return this.x;
  }
}

class CaveWorld extends Component {
  override onStart() {
    RenderSettings.ambientMode = "color";
    RenderSettings.ambientLight = new Color(0.5, 0.52, 0.58, 1);
    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-40, 20, 0) });
    sun.addComponent(Light, { type: "directional", intensity: 0.7 });

    // Ground, the hill the cave goes into, the sea.
    block("Meadow", new Vector3(0, -0.5, 0), new Vector3(100, 1, 14), grass);
    block("Sand", new Vector3(SHORE - 2, -0.45, 0), new Vector3(10, 1, 14), sand);
    block("Sea", new Vector3(SHORE + 18, -0.55, 0), new Vector3(30, 1, 14), water);
    const length = CAVE.mouth - CAVE.back;
    const mid = (CAVE.mouth + CAVE.back) / 2;
    block("Hill", new Vector3(mid, 4.5, -4.5), new Vector3(length + 6, 9, 5), rock);
    block("Hill top", new Vector3(mid, 5.5, 0), new Vector3(length + 4, 2.2, 6), rock);
    block("Cave back", new Vector3(CAVE.back, 2, 0), new Vector3(1, 4, 6), rock);
    block("Mouth left", new Vector3(CAVE.mouth + 0.5, 2.5, -3.2), new Vector3(1.6, 5, 1), rock);

    // Buses: the voice and the world, with effects the zones can drive.
    voiceBus = audio.createBus({ volume: 1, effects: true });
    const worldBus = audio.createBus({ volume: 1, effects: true });

    // The cave: a long box around its interior, strongest in the middle of it.
    const cave = new GameObject("Cave zone", { position: new Vector3(mid - 2, 2, 0) });
    const caveZone = cave.addComponent(AudioZone, {
      shape: "box",
      size: new Vector3(length + 24, 8, 12),
      innerFraction: 0.55,
      curve: "smooth",
      affectsSources: true,
      lowPass: 3800,
      reverb: 0.8,
      reverbSize: 0.8,
      reverbDamping: 0.55,
      echo: 0.35,
      ambience: audio.load(SOUND("drips"), { stream: true }),
      ambienceVolume: 0.8,
    });
    caveZone.addBus(voiceBus);
    caveZone.addBus(worldBus);

    // A radio left in the cave: the zone also colours sources, so it echoes where it stands,
    // however far from it you are.
    const radio = GameObject.createPrimitive("cube", { position: new Vector3(CAVE.back + 5, 0.5, 1.2), scale: new Vector3(0.5, 0.3, 0.2) });
    radio.getComponent(MeshRenderer)!.material = new Material(Color.hex("#c0392b"), Color.black, 0.3);
    radio.addComponent(AudioSource, {
      clip: audio.load(SOUND("radio_music")),
      loop: true,
      spatialBlend: 1,
      minDistance: 2,
      maxDistance: 40,
      effects: true,
      volume: 0.5,
    }).play();

    // The outdoors: wind and birds, everywhere except in the cave.
    const outdoors = new GameObject("Outdoors zone", { position: new Vector3(mid - 2, 2, 0) });
    const outZone = outdoors.addComponent(AudioZone, {
      shape: "box",
      size: new Vector3(length + 34, 12, 20),
      innerFraction: 0.6,
      curve: "smooth",
      invert: true,
      ambience: audio.load(SOUND("nature"), { stream: true }),
      ambienceVolume: 0.7,
    });

    // The sea: waves that swell as you near the water, gentle at a distance.
    const sea = new GameObject("Sea zone", { position: new Vector3(SHORE + 4, 0, 0) });
    const seaZone = sea.addComponent(AudioZone, {
      shape: "sphere",
      radius: 38,
      innerFraction: 0.25,
      curve: "easeOut",
      ambience: audio.load(SOUND("waves"), { stream: true }),
      ambienceVolume: 1,
    });

    zones = [
      { name: "Cave", zone: caveZone, color: "#c39bd3" },
      { name: "Outdoors", zone: outZone, color: "#7bd88f" },
      { name: "Sea", zone: seaZone, color: "#5dade8" },
    ];

    // The character: a capsule that talks (a flat voice on the voice bus) and carries the ears.
    const person = new GameObject("Walker", { position: new Vector3(18, 1, 0) });
    const body = GameObject.createPrimitive("capsule", { parent: person, scale: new Vector3(0.8, 1, 0.8) });
    body.getComponent(MeshRenderer)!.material = new Material(Color.hex("#e8c25d"), Color.black, 0.3);
    const nose = GameObject.createPrimitive("cube", { parent: person, position: new Vector3(0, 0.55, 0.45), scale: new Vector3(0.2, 0.2, 0.5) });
    nose.getComponent(MeshRenderer)!.material = new Material(Color.white, Color.black, 0.3);
    person.addComponent(AudioListener);
    // The voice is its own source on the voice bus (flat: it is the speaker's own voice).
    const mouth = new GameObject("Voice", { parent: person });
    recorded = mouth.addComponent(AudioSource, {
      clip: audio.load(SOUND("voice")),
      loop: true,
      spatialBlend: 0,
      outputBus: voiceBus,
      volume: 0.9,
    });
    recorded.play();
    // The footsteps go through the world bus.
    person.addComponent(AudioSource, { spatialBlend: 0, outputBus: worldBus });
    walker = person.addComponent(Walker);

    const cam = new GameObject("Main Camera", { position: new Vector3(18, 3, 16), rotation: Quaternion.lookRotation(new Vector3(0, -2, -16)) });
    cam.addComponent(Camera, { fieldOfView: 45, backgroundColor: Color.hex("#9bc4e8") });
    cam.addComponent(CameraFollow);
  }
}

/** Keeps the camera beside the walker. */
class CameraFollow extends Component {
  override onLateUpdate() {
    if (!walker) return;
    const x = walker.transform.position.x;
    this.transform.position = new Vector3(x, 3, 16);
  }
}

register({ CaveWorld });
register({ Walker, CameraFollow }, { spawn: false });

function Hud() {
  const [, tick] = useState(0);
  useEffect(() => {
    let id = 0;
    const loop = () => {
      tick((n) => n + 1);
      id = setTimeout(loop, 80);
    };
    loop();
    return () => clearTimeout(id);
  }, []);
  return (
    <box style={{ width: "100%", height: "100%", padding: 18, pointerEvents: "none" }}>
      <box style={{ alignSelf: "flex-start", padding: 14, gap: 8, borderRadius: 10, backgroundColor: "rgba(10,14,22,0.72)", flexDirection: "column" }}>
        <text style={{ color: "#ffd93d", fontSize: 18, fontWeight: 700 }}>Audio: cave</text>
        {zones.map((z) => (
          <box key={z.name} style={{ flexDirection: "row", gap: 10, alignItems: "center" }}>
            <text style={{ color: "#c8d0e0", fontSize: 13, width: 70 }}>{z.name}</text>
            <box style={{ width: 160, height: 8, borderRadius: 4, backgroundColor: "rgba(255,255,255,0.15)" }}>
              <box style={{ width: `${Math.round(z.zone.weight * 100)}%`, height: 8, borderRadius: 4, backgroundColor: z.color }} />
            </box>
            <text style={{ color: "#8f9bb3", fontSize: 12 }}>{Math.round(z.zone.weight * 100)}%</text>
          </box>
        ))}
        <text style={{ color: "#8f9bb3", fontSize: 12 }}>A / D walk · T auto-walk {walker?.auto ? "on" : "off"} · 1 cave · 2 meadow · 3 shore</text>
        <text style={{ color: micOn ? "#7bd88f" : "#8f9bb3", fontSize: 12 }}>
          M: your microphone {micOn ? (Microphone.running ? `on, level ${Math.round(Microphone.level * 100)}% (use headphones)` : Microphone.error || "starting…") : "off (talk in the cave: use headphones)"}
        </text>
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
