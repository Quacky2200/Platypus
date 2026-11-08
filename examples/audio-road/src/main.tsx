// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Doppler. You stand by a road; cars go by at 15 to 40 m/s, engines droning, horns blaring, and
// a truck with a man on the back who shouts "get out of the way!" as it nears. Each car's AudioSource
// has `dopplerLevel`: its pitch rises as it comes at you, falls as it leaves, and it slides past you
// left to right on its way. Everything that plays on the source (engine, horn, shout) bends with it.
//
// A / D walk along the verge · H your own horn · L change the Doppler level (0, 1, 2)

const SOUND = (name: string) => `assets/sounds/${name}.wav`;
const LENGTH = 80; // the road runs from -LENGTH to LENGTH
const LEVELS = [0, 1, 2];
let levelIndex = 1;
let me: GameObject;

const cars: Car[] = [];

class Car extends Component {
  speed = 20;
  direction = 1;
  engine!: AudioSource;
  honk = 0;
  shoutsAt = Number.POSITIVE_INFINITY;
  private shouted = false;
  private hornTimer = 3 + Math.random() * 6;

  override onStart() {
    this.engine = this.getComponent(AudioSource)!;
    cars.push(this);
  }

  override onUpdate(dt: number) {
    const p = this.transform.position;
    let x = p.x + this.direction * this.speed * dt;
    if (Math.abs(x) > LENGTH) {
      // Round again, at another speed.
      x = -this.direction * LENGTH;
      this.speed = 15 + Math.random() * 25;
      this.engine.pitch = 0.8 + this.speed / 60;
      this.shouted = false;
    }
    this.transform.position = new Vector3(x, p.y, p.z);
    this.engine.dopplerLevel = LEVELS[levelIndex]!;
    this.hornTimer -= dt;
    if (this.hornTimer <= 0) {
      this.hornTimer = 5 + Math.random() * 8;
      // Honk when getting near, so you hear the bend.
      if (Math.abs(x - me.transform.position.x) < 60) {
        this.engine.playOneShot(SOUND("horn"), 0.8);
        this.honk = 1.2;
      }
    }
    this.honk = Math.max(0, this.honk - dt);
    if (this.shoutsAt < Number.POSITIVE_INFINITY && !this.shouted) {
      const gap = (me.transform.position.x - x) * this.direction;
      if (gap > 0 && gap < this.shoutsAt) {
        this.engine.playOneShot(SOUND("shout"), 1);
        this.shouted = true;
      }
    }
  }

  /** The pitch factor a listener standing at `x` should hear, for the HUD (the real maths: the line of sight along x only). */
  pitchAt(x: number): number {
    const to = Math.sign(this.transform.position.x - x) || 1;
    const c = Acoustics.speedOfSound;
    const away = this.direction * this.speed * to;
    const raw = c / (c + away);
    return 1 + (raw - 1) * LEVELS[levelIndex]!;
  }
}

class Verge extends Component {
  override onUpdate(dt: number) {
    const move = (input.getKey("KeyD") || input.getKey("ArrowRight") ? 1 : 0) - (input.getKey("KeyA") || input.getKey("ArrowLeft") ? 1 : 0);
    const p = this.transform.position;
    this.transform.position = new Vector3(Math.max(-60, Math.min(60, p.x + move * 4 * dt)), p.y, p.z);
    if (input.getKeyDown("KeyL")) levelIndex = (levelIndex + 1) % LEVELS.length;
    if (input.getKeyDown("KeyH")) this.getComponent(AudioSource)!.playOneShot(SOUND("horn"), 0.7);
  }
}

function box(name: string, position: Vector3, scale: Vector3, color: string): GameObject {
  const go = GameObject.createPrimitive("cube", { position, scale });
  go.name = name;
  go.getComponent(MeshRenderer)!.material = new Material(Color.hex(color), Color.black, 0.2);
  return go;
}

function makeCar(name: string, lane: number, direction: number, speed: number, color: string, x: number, size: Vector3, shouts = false) {
  const car = box(name, new Vector3(x, size.y / 2 + 0.2, lane), size, color);
  car.addComponent(AudioSource, {
    clip: audio.load(SOUND("engine")),
    loop: true,
    spatialBlend: 1,
    minDistance: 4,
    maxDistance: 70,
    pitch: 0.8 + speed / 60,
    dopplerLevel: 1,
  }).play();
  const c = car.addComponent(Car);
  c.speed = speed;
  c.direction = direction;
  if (shouts) {
    c.shoutsAt = 28;
    // A man on the back.
    const man = GameObject.createPrimitive("capsule", { parent: car, position: new Vector3(-0.3, 0.9, 0), scale: new Vector3(0.28, 0.4, 0.28) });
    man.getComponent(MeshRenderer)!.material = new Material(Color.hex("#e8c25d"), Color.black, 0.3);
  }
  return c;
}

class RoadWorld extends Component {
  override onStart() {
    RenderSettings.ambientMode = "color";
    RenderSettings.ambientLight = new Color(0.55, 0.57, 0.62, 1);
    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-50, 25, 0) });
    sun.addComponent(Light, { type: "directional", intensity: 0.7 });
    box("Verge", new Vector3(0, -0.5, 0), new Vector3(200, 1, 30), "#4f7d3a");
    box("Road", new Vector3(0, 0.05, -4), new Vector3(200, 0.1, 8), "#3a3d44");
    for (let i = -9; i <= 9; i++) box("Line", new Vector3(i * 8, 0.11, -4), new Vector3(2.5, 0.02, 0.2), "#e8eefc");

    // Two lanes: z = -2.5 goes towards +x, z = -5.5 towards -x.
    makeCar("Hatchback", -2.5, 1, 25, "#e74c3c", -55, new Vector3(3.8, 1.4, 1.7));
    makeCar("Van", -5.5, -1, 18, "#f1c40f", 60, new Vector3(5, 2, 2));
    makeCar("Truck", -2.5, 1, 22, "#3498db", -20, new Vector3(7, 2.4, 2.2), true);
    makeCar("Sports car", -5.5, -1, 38, "#9b59b6", 20, new Vector3(4.2, 1.1, 1.8));

    // You, by the verge, with the ears (and a horn of your own).
    me = new GameObject("You", { position: new Vector3(0, 1, 2.5) });
    const body = GameObject.createPrimitive("capsule", { parent: me, scale: new Vector3(0.7, 0.9, 0.7) });
    body.getComponent(MeshRenderer)!.material = new Material(Color.hex("#7bd88f"), Color.black, 0.3);
    // The listener faces -z by default: the road, so cars driving to +x pass from left to right.
    me.addComponent(AudioListener);
    me.addComponent(AudioSource, { spatialBlend: 0 });
    me.addComponent(Verge);

    const cam = new GameObject("Main Camera", { position: new Vector3(0, 3.2, 12), rotation: Quaternion.lookRotation(new Vector3(0, -2.5, -12)) });
    cam.addComponent(Camera, { fieldOfView: 55, backgroundColor: Color.hex("#9bc4e8") });
    cam.addComponent(CamFollow);
  }
}

class CamFollow extends Component {
  override onLateUpdate() {
    if (me) this.transform.position = new Vector3(me.transform.position.x, 3.2, 12);
  }
}

register({ RoadWorld });
register({ Car, Verge, CamFollow }, { spawn: false });

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
  const x = me ? me.transform.position.x : 0;
  return (
    <box style={{ width: "100%", height: "100%", padding: 18, pointerEvents: "none" }}>
      <box style={{ alignSelf: "flex-start", padding: 14, gap: 6, borderRadius: 10, backgroundColor: "rgba(10,14,22,0.72)", flexDirection: "column" }}>
        <text style={{ color: "#ffd93d", fontSize: 18, fontWeight: 700 }}>Audio: road</text>
        <text style={{ color: "#e8eefc", fontSize: 14 }}>Doppler level {LEVELS[levelIndex]} (L)</text>
        {cars.map((c) => {
          const f = c.pitchAt(x);
          return (
            <box key={c.gameObject!.name} style={{ flexDirection: "row", gap: 10 }}>
              <text style={{ color: "#c8d0e0", fontSize: 12, width: 90 }}>{c.gameObject!.name}</text>
              <text style={{ color: f > 1.005 ? "#7bd88f" : f < 0.995 ? "#e0907b" : "#8f9bb3", fontSize: 12, width: 80 }}>
                pitch ×{f.toFixed(2)}
              </text>
              <text style={{ color: "#8f9bb3", fontSize: 12 }}>{Math.round(c.speed)} m/s {c.direction > 0 ? "→" : "←"}</text>
            </box>
          );
        })}
        <text style={{ color: "#8f9bb3", fontSize: 12 }}>A / D walk · H honk · L change Doppler level</text>
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
