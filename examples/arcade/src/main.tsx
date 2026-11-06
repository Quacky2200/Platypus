// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Arcade: a small game with a React HUD. Steer the ball (WASD / arrow keys / left stick)
// to collect coins (1 point) and gems (5) before the round ends; quick pickups build a
// combo that multiplies points. Leave the controls alone and it plays itself.
//
// Two looks, each its own scene: Classic (a sky, plain floor and walls) and Deluxe (black
// space with glowing bubbles, a stainless floor with soft reflections, a riveted steel
// frame). The Style button builds the other arena in a new scene and unloads the old;
// the round (score, timer) lives on through it (`dontDestroyOnLoad`).
//
// The game lives in components (`GameManager`, `Player`, `Pickup`); the HUD is React
// (`ScoreBoard`, `Timer`, `RoundOver`, `Settings`), fed by `useGame()`. Esc (or the
// Settings button) pauses and opens the settings: style, window mode, resolution,
// vsync, frame cap and quality, saved for next time (`PlayerPrefs`). Both reload while it runs:
// `bun run dev` and edit a HUD component (it keeps the score on screen) or a component's
// code (the round, the score and every coin stay where they are).

const ARENA = 9;

// ---- Game ----

/** The round: score, combo, time left. The HUD listens to it. */
class GameManager extends Component {
  @property roundLength = 45;
  /** Pickups this close together in time keep the combo going. */
  @property comboWindow = 1;
  score = 0;
  best = 0;
  /** The settings menu is open: the game stands still. */
  paused = false;
  combo = 0;
  timeLeft = 45;
  lastPickup = -100;
  /** Recent pickups, for the HUD's "+N" popups. */
  recent: { id: number; points: number; at: number }[] = [];
  private nextPopup = 1;

  static get current(): GameManager | undefined {
    return GameObject.findObjectOfType(GameManager);
  }

  get over(): boolean {
    return this.timeLeft <= 0;
  }

  setPaused(on: boolean): void {
    this.paused = on;
    Time.timeScale = on ? 0 : 1;
    if (!on) PlayerPrefs.save();
    notify();
  }

  /** Points times the combo (x1, x2 ... up to x5). */
  collect(value: number): void {
    if (this.over) return;
    this.combo = Time.time - this.lastPickup <= this.comboWindow ? Math.min(this.combo + 1, 5) : 1;
    this.lastPickup = Time.time;
    const points = value * this.combo;
    this.score += points;
    this.best = Math.max(this.best, this.score);
    this.recent = [...this.recent.slice(-3), { id: this.nextPopup++, points, at: Time.time }];
    notify();
  }

  restart(): void {
    this.score = 0;
    this.combo = 0;
    this.timeLeft = this.roundLength;
    this.recent = [];
    notify();
  }

  override onStart() {
    this.timeLeft = this.roundLength;
  }

  /** Brought back by a reload (the HUD is new): show it the round. */
  override onReload() {
    notify();
  }

  override onUpdate(dt: number) {
    if (input.getButtonDown("Cancel")) this.setPaused(!this.paused);
    if (this.over || this.paused) return;
    const before = Math.ceil(this.timeLeft);
    this.timeLeft = Math.max(0, this.timeLeft - dt);
    if ((this.over || Time.time - this.lastPickup > this.comboWindow) && this.combo > 0) {
      this.combo = 0;
      notify();
    }
    // The HUD shows whole seconds.
    if (Math.ceil(this.timeLeft) !== before) notify();
  }
}

/** Rolls around the arena; plays itself when nobody's steering. */
class Player extends Component {
  @property speed = 6;
  velocity = Vector3.zero;
  idle = 0;

  override onUpdate(dt: number) {
    let steer = new Vector3(input.getAxisRaw("Horizontal"), 0, -input.getAxisRaw("Vertical"));
    this.idle = steer.magnitude > 0.1 ? 0 : this.idle + dt;
    if (this.idle > 1.5) {
      // Attract mode: head for the nearest pickup.
      const here = this.transform.position;
      const target = pickups()
        .map((p) => p.transform.position)
        .sort((a, b) => a.distance(here) - b.distance(here))[0];
      if (target) steer = target.sub(here).scale(1 / Math.max(1e-3, target.distance(here)));
    }
    if (steer.magnitude > 1) steer = steer.normalized;
    this.velocity = this.velocity.lerp(steer.scale(this.speed), Math.min(1, dt * 6));
    const p = this.transform.position.add(this.velocity.scale(dt));
    this.transform.position = new Vector3(Math.max(-ARENA, Math.min(ARENA, p.x)), 0.5, Math.max(-ARENA, Math.min(ARENA, p.z)));
    // Roll: turn about the axis across the motion.
    const along = this.velocity.magnitude * dt;
    if (along > 1e-5) {
      const axis = new Vector3(this.velocity.z, 0, -this.velocity.x).normalized;
      this.transform.rotation = Quaternion.angleAxis(-(along / 0.5) * (180 / Math.PI), axis).mul(this.transform.rotation);
    }
  }
}

/** A coin or a gem: spins, and moves somewhere else once collected. */
class Pickup extends Component {
  @property value = 1;
  /** Degrees a second. */
  @property spin = 120;
  /** How far it bobs up and down (metres; 0 stays put). */
  @property bob = 0;
  /** Where it floats. */
  @property height = 0.6;

  override onUpdate(dt: number) {
    this.transform.rotate(new Vector3(0, this.spin * dt, 0), "world");
    const p = this.transform.position;
    const y = this.bob > 0 ? this.height + Math.sin(Time.time * 3 + p.x) * this.bob : this.height;
    if (y !== p.y) this.transform.position = new Vector3(p.x, y, p.z);
    const player = GameObject.findObjectOfType(Player);
    const game = GameManager.current;
    if (!player || !game || game.over) return;
    if (player.transform.position.distance(this.transform.position) < 0.9) {
      game.collect(this.value);
      this.transform.position = randomSpot(this.height);
    }
  }
}

function pickups(): Pickup[] {
  return GameObject.roots.flatMap((g) => g.getComponents(Pickup));
}

function randomSpot(y: number): Vector3 {
  return new Vector3(Random.range(-ARENA + 1, ARENA - 1), y, Random.range(-ARENA + 1, ARENA - 1));
}

// ---- The two arenas ----

type Style = "classic" | "deluxe";

/** Starts the game: the round (kept through style switches) and the saved style's arena. */
class Arcade extends Component {
  override onStart() {
    // The frame cap the player chose last time (quality comes back on its own).
    Application.targetFrameRate = PlayerPrefs.getInt("arcade.frameCap", -1);
    Application.runInBackground = true;
    const game = new GameObject("Game");
    game.addComponent(GameManager);
    GameObject.dontDestroyOnLoad(game);
    switchStyle(PlayerPrefs.getString("arcade.style", "classic") === "deluxe" ? "deluxe" : "classic");
  }
}

let arenaCount = 0;

/** The arena showing now, read from its scene's name. */
function currentStyle(): Style {
  return SceneManager.getActiveScene().name.startsWith("Deluxe") ? "deluxe" : "classic";
}

/** Where the ball and each pickup are, so a new arena can put them back. */
interface Positions {
  ball: { position: Vector3; rotation: Quaternion; velocity: Vector3; idle: number };
  pickups: { value: number; x: number; z: number }[];
}

function positionsNow(): Positions | null {
  const ball = GameObject.findObjectOfType(Player);
  if (!ball) return null;
  return {
    ball: { position: ball.transform.position, rotation: ball.transform.rotation, velocity: ball.velocity, idle: ball.idle },
    pickups: pickups().map((p) => ({ value: p.value, x: p.transform.position.x, z: p.transform.position.z })),
  };
}

/** Moves the new arena's ball and pickups to where the old one's were (pickups by kind). */
function restorePositions(scene: Scene, saved: Positions) {
  const roots = scene.getRootGameObjects();
  const ball = roots.map((g) => g.getComponent(Player)).find((p) => p);
  if (ball) {
    ball.transform.position = saved.ball.position;
    ball.transform.rotation = saved.ball.rotation;
    ball.velocity = saved.ball.velocity;
    ball.idle = saved.ball.idle;
  }
  const fresh = roots.map((g) => g.getComponent(Pickup)).filter((p): p is Pickup => !!p);
  for (const old of saved.pickups) {
    const i = fresh.findIndex((p) => p.value === old.value);
    if (i < 0) continue;
    const [p] = fresh.splice(i, 1);
    p!.transform.position = new Vector3(old.x, p!.height, old.z);
  }
}

/**
 * Builds `style`'s arena in a new scene, makes it active (its lighting with it) and
 * unloads the arenas before it. The round goes on, and the ball and pickups stay where
 * they were: only the look changes.
 */
function switchStyle(style: Style) {
  const saved = positionsNow();
  PlayerPrefs.setString("arcade.style", style);
  const old: Scene[] = [];
  for (let i = 0; i < SceneManager.sceneCount; i++) {
    const scene = SceneManager.getSceneAt(i);
    if (/^(Classic|Deluxe)/.test(scene.name)) old.push(scene);
  }
  const scene = SceneManager.createScene(`${style === "deluxe" ? "Deluxe" : "Classic"} ${++arenaCount}`);
  SceneManager.setActiveScene(scene);
  buildArena(style);
  if (saved) restorePositions(scene, saved);
  for (const s of old) SceneManager.unloadScene(s);
  notify();
}

function buildArena(style: Style) {
  const deluxe = style === "deluxe";
  const sun = new GameObject("Sun", { rotation: Quaternion.lookRotation(new Vector3(0.4, -0.8, -0.3)) });
  sun.addComponent(Light, { type: "directional", intensity: 1.1, shadows: "soft" });
  const cam = new GameObject("Main Camera", { position: new Vector3(0, 17, 15.5), rotation: Quaternion.euler(-50, 0, 0) });
  cam.addComponent(Camera, { fieldOfView: 50, clearFlags: "skybox" });
  if (deluxe) buildDeluxe();
  else buildClassic();

  const ball = new GameObject("Player", { position: new Vector3(0, 0.5, 0) });
  ball.addComponent(MeshRenderer, { mesh: "sphere", material: Material.create({ color: new Color(0.95, 0.4, 0.2, 1), smoothness: 0.7 }) });
  ball.addComponent(Player);

  // Deluxe pickups spin slower and bob gently.
  const gold = Material.create({ color: new Color(1, 0.75, 0.2, 1), emission: new Color(0.35, 0.22, 0.02, 1), metallic: 0.5, smoothness: 0.7 });
  for (let i = 0; i < 10; i++) {
    const height = deluxe ? 0.65 : 0.6;
    const coin = new GameObject(`Coin ${i}`, { position: randomSpot(height) });
    const disc = new GameObject("Disc", { parent: coin, rotation: Quaternion.euler(90, 0, 0), scale: new Vector3(0.7, 0.08, 0.7) });
    disc.addComponent(MeshRenderer, { mesh: "cylinder", material: gold });
    coin.addComponent(Pickup, { value: 1, spin: deluxe ? 72 : 120, bob: deluxe ? 0.12 : 0, height });
  }
  const amethyst = Material.create({ color: new Color(0.6, 0.3, 1, 1), emission: new Color(0.25, 0.1, 0.5, 1), smoothness: 0.9 });
  for (let i = 0; i < 2; i++) {
    const height = 0.7;
    const gem = new GameObject(`Gem ${i}`, { position: randomSpot(height), scale: Vector3.one.scale(0.55) });
    gem.addComponent(MeshRenderer, { mesh: "sphere", material: amethyst });
    gem.addComponent(Pickup, { value: 5, spin: deluxe ? 120 : 200, bob: deluxe ? 0.12 : 0, height });
  }
}

/** The original look: a sunny sky, a dark floor and plain walls. */
function buildClassic() {
  RenderSettings.skybox = { sunDirection: new Vector3(-0.4, 0.8, 0.3) };
  RenderSettings.ambientMode = "skybox";
  RenderSettings.bloom = null;
  const floor = new GameObject("Floor", { scale: new Vector3(2, 1, 2) });
  floor.addComponent(MeshRenderer, { mesh: "plane", material: Material.create({ color: new Color(0.07, 0.09, 0.14, 1), smoothness: 0.5 }) });
  const wall = Material.create({ color: new Color(0.16, 0.2, 0.3, 1), smoothness: 0.3 });
  for (const [x, z, w, d] of [
    [0, -ARENA - 0.5, ARENA * 2 + 2, 1],
    [0, ARENA + 0.5, ARENA * 2 + 2, 1],
    [-ARENA - 0.5, 0, 1, ARENA * 2],
    [ARENA + 0.5, 0, 1, ARENA * 2],
  ] as const) {
    new GameObject("Wall", { position: new Vector3(x, 0.3, z), scale: new Vector3(w, 0.6, d) }).addComponent(MeshRenderer, { mesh: "cube", material: wall });
  }
}

/**
 * Space: a black sky with glowing bubbles behind the board, a polished stainless floor
 * under a faint mirror sheet (exact reflections right below the coins), and the borders
 * as a riveted steel frame.
 */
function buildDeluxe() {
  RenderSettings.skybox = { zenith: Color.black, horizon: Color.black, ground: Color.black, sunDirection: null };
  // A black sky lights nothing: a fixed ambient keeps the board visible.
  RenderSettings.ambientMode = "color";
  RenderSettings.ambientLight = new Color(0.25, 0.25, 0.3, 1);
  RenderSettings.bloom = { threshold: 0.9, intensity: 1.1, scatter: 0.7 };

  // The floor (a 10 m plane, scaled) ends at the frame's outer edge. Stainless: bright and
  // shiny, but mostly its own colour (a black sky gives pure metal nothing to show).
  const span = ((ARENA + 0.6) * 2) / 10;
  const floor = new GameObject("Floor", { scale: new Vector3(span, 1, span) });
  floor.addComponent(MeshRenderer, {
    mesh: "plane",
    material: Material.create({ color: Color.white, mainTexture: brushedMetal(), mainTextureScale: new Vector2(2, 2), smoothness: 0.88, metallic: 0.35 }),
  });
  // Reflections: a see-through mirror sheet just above the floor (a reflection probe
  // sees the room from one point and stretches things near the floor). Its opacity is
  // how strongly it reflects.
  const sheet = new GameObject("Floor mirror", { position: new Vector3(0, 0.01, 0), scale: new Vector3(span, 1, span) });
  sheet.addComponent(MeshRenderer, {
    mesh: "plane",
    material: Material.create({ color: new Color(1, 1, 1, 0.09), metallic: 1, smoothness: 1 }),
    shadowCastingMode: "off",
  });
  sheet.addComponent(Mirror);

  // The frame: 0.6 × 0.6 beams running along their own x (a square cross-section keeps
  // the rivets round on every face; the texture runs along x, so the side beams are
  // turned 90°), tiled along their length.
  const metal = rivetedMetal();
  const out = ARENA + 0.3;
  for (const [x, z, length, turn] of [
    [0, -out, (ARENA + 0.6) * 2, false],
    [0, out, (ARENA + 0.6) * 2, false],
    [-out, 0, ARENA * 2, true],
    [out, 0, ARENA * 2, true],
  ] as const) {
    const beam = new GameObject("Wall", {
      position: new Vector3(x, 0.3, z),
      rotation: turn ? Quaternion.euler(0, 90, 0) : Quaternion.identity,
      scale: new Vector3(length, 0.6, 0.6),
    });
    beam.addComponent(MeshRenderer, {
      mesh: "cube",
      material: Material.create({ color: Color.white, mainTexture: metal, mainTextureScale: new Vector2(length / 0.6, 1), smoothness: 0.75, metallic: 0.35 }),
    });
  }

  // Glowing bubbles floating in a wide layer far below and beyond the board: the camera
  // sees them all around it, and the board hides the middle.
  const bubbles = new GameObject("Bubbles", { position: new Vector3(0, -30, -20) });
  bubbles.addComponent(ParticleSystem, {
    shape: { type: "box", size: { x: 140, y: 20, z: 120 } },
    rateOverTime: 60,
    maxParticles: 600,
    startLifetime: [6, 10],
    // Barely drifting: floating, not rising.
    startSpeed: [0, 0.4],
    startSize: [2, 5],
    // Colours above 1 glow through the bloom.
    startColor: [hue(0.55, 2.5), hue(0.85, 2.5)],
    drag: 0.05,
    simulationSpace: "world",
    blendMode: "additive",
    sizeOverLifetime: [
      { time: 0, value: 0.3 },
      { time: 0.3, value: 1 },
      { time: 1, value: 1.3 },
    ],
    colorOverLifetime: [
      { time: 0, color: new Color(1, 1, 1, 0) },
      { time: 0.25, color: new Color(1, 1, 1, 1) },
      { time: 0.75, color: new Color(1, 1, 1, 0.8) },
      { time: 1, color: new Color(1, 1, 1, 0) },
    ],
  });
}

/** A colour around the hue wheel (0..1), fully saturated, at `value` brightness. */
function hue(h: number, value: number): Color {
  const k = (n: number) => (n + h * 6) % 6;
  const f = (n: number) => value * (1 - Math.max(0, Math.min(k(n), 4 - k(n), 1)));
  return new Color(f(5), f(3), f(1), 1);
}

/**
 * The borders as a riveted frame: brushed steel with a strap along each long edge and a
 * row of small rivets down its middle. It repeats seamlessly along x (no seams across),
 * so each wall reads as one piece; one tile is a 0.6 m stretch with two rivets per row.
 */
function rivetedMetal(): Texture {
  const size = 256;
  const colors: Color[] = [];
  const rng = new Rng(7);
  // Grain runs along the wall (x), so it repeats without a seam.
  const grain = Array.from({ length: size }, () => rng.nextDouble() * 0.04);
  const strap = 56; // px from each long edge
  const rivetsY = [strap / 2, size - strap / 2];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let v = 0.66 + grain[y]! + (rng.nextDouble() - 0.5) * 0.02;
      // The straps: a touch lighter, with a thin groove on their inner edge.
      const fromEdge = Math.min(y, size - 1 - y);
      if (fromEdge < strap) v += 0.04;
      if (Math.abs(fromEdge - strap) < 3) v -= 0.24;
      else if (Math.abs(fromEdge - strap - 2.5) < 1) v += 0.05;
      // Rivets every half tile along each strap (x wraps, so the row is continuous).
      for (const ry of rivetsY) {
        for (const rx of [size / 4, (size * 3) / 4]) {
          const dx = x - rx;
          const dy = y - ry;
          const d = Math.hypot(dx, dy);
          // A domed head lit from the top left: bright on that side, dark on the
          // other, a small highlight, and a dark ring where it meets the strap.
          if (d < 19) {
            v = 0.62 + ((-dx - dy) / 38) * 0.6;
            if (Math.hypot(dx + 6, dy + 6) < 4) v = 1;
          } else if (d < 23) v = 0.22;
        }
      }
      colors.push(new Color(v * 0.95, v * 0.97, v, 1));
    }
  }
  return Texture.create(size, size, colors, { filterMode: "bilinear", wrapMode: "repeat" });
}

/** Polished, brushed steel: fine grain running along x, no seams. */
function brushedMetal(): Texture {
  const size = 256;
  const rng = new Rng(11);
  const grain = Array.from({ length: size }, () => rng.nextDouble());
  const colors: Color[] = [];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      // Long streaks (per row) with a little fine noise across them.
      const v = 0.8 + grain[y]! * 0.06 + (rng.nextDouble() - 0.5) * 0.02;
      colors.push(new Color(v * 0.97, v * 0.98, v, 1));
    }
  }
  return Texture.create(size, size, colors, { filterMode: "bilinear", wrapMode: "repeat" });
}

// Registered so hot reload can bring them back with their state.
register({ GameManager, Player, Pickup }, { spawn: false });
register({ Arcade });

// ---- HUD (React) ----

const listeners = new Set<() => void>();
function notify() {
  for (const f of listeners) f();
}

interface GameView {
  style: Style;
  paused: boolean;
  score: number;
  best: number;
  combo: number;
  timeLeft: number;
  over: boolean;
  recent: { id: number; points: number; at: number }[];
}

/** The round as React state: re-renders when the game says something changed. */
function useGame(): GameView | null {
  const read = (): GameView | null => {
    const g = GameManager.current;
    return g ? { style: currentStyle(), paused: g.paused, score: g.score, best: g.best, combo: g.combo, timeLeft: g.timeLeft, over: g.over, recent: g.recent } : null;
  };
  const [view, setView] = useState(read);
  useEffect(() => {
    const update = () => setView(read());
    listeners.add(update);
    update();
    return () => {
      listeners.delete(update);
    };
  }, []);
  return view;
}

/** How the HUD looks in each style. */
interface Theme {
  panel: Record<string, unknown>;
  /** Small labels (TIME, Best, settings rows). */
  label: string;
  /** The score's title and colour. */
  scoreTitle: string;
  score: string;
}

const THEMES: Record<Style, Theme> = {
  classic: {
    panel: { padding: "10px 16px", borderRadius: 12, backgroundColor: "rgba(10, 12, 20, 0.72)", flexDirection: "column" },
    label: "#9fb0c8",
    scoreTitle: "SCORE",
    score: "#ffffff",
  },
  deluxe: {
    // Tinted glass (a backdrop blur costs a full-screen copy and blur each frame).
    panel: {
      padding: "10px 16px",
      borderRadius: 12,
      backgroundColor: "rgba(20, 24, 40, 0.55)",
      border: "1px solid rgba(255, 255, 255, 0.12)",
      flexDirection: "column",
    },
    label: "#ffffff",
    scoreTitle: "POINTS",
    score: "#ffcf4a",
  },
};

/** The current score, the best and the combo, with "+N" popups for the last pickups. */
function ScoreBoard(props: { t: Theme; score: number; best: number; combo: number; recent: GameView["recent"] }) {
  const { t } = props;
  return (
    <box style={{ ...t.panel, gap: 2, minWidth: 180 }}>
      <text style={{ color: t.scoreTitle === "SCORE" ? t.label : t.score, fontSize: 12, letterSpacing: 1 }}>{t.scoreTitle}</text>
      <box style={{ flexDirection: "row", alignItems: "baseline", gap: 10 }}>
        <text style={{ color: t.score, fontSize: 40, fontWeight: 700 }}>{String(props.score)}</text>
        {props.combo > 1 && <text style={{ color: "#ffcf4a", fontSize: 18, fontWeight: 700 }}>{`x${props.combo}`}</text>}
      </box>
      <text style={{ color: t.label, fontSize: 12 }}>{`Best ${props.best}`}</text>
      <box style={{ flexDirection: "row", gap: 6, height: 18 }}>
        {props.recent
          .filter((p) => Time.time - p.at < 1.2)
          .map((p) => (
            <text key={p.id} style={{ color: p.points >= 5 ? "#c89bff" : "#ffcf4a", fontSize: 14, fontWeight: 600 }}>
              {`+${p.points}`}
            </text>
          ))}
      </box>
    </box>
  );
}

/** Seconds left, red for the last ten. */
function Timer(props: { t: Theme; timeLeft: number }) {
  const s = Math.ceil(props.timeLeft);
  return (
    <box style={{ ...props.t.panel, alignItems: "center", minWidth: 90 }}>
      <text style={{ color: props.t.label, fontSize: 12, letterSpacing: 1 }}>TIME</text>
      <text style={{ color: s <= 10 ? "#ff6b5e" : "#ffffff", fontSize: 32, fontWeight: 700 }}>{String(s)}</text>
    </box>
  );
}

const MODE_NAMES: Record<FullScreenMode, string> = {
  ExclusiveFullScreen: "Fullscreen",
  FullScreenWindow: "Borderless",
  MaximizedWindow: "Maximized",
  Windowed: "Windowed",
};
const WINDOW_SIZES = [
  [1280, 720],
  [1600, 900],
  [1920, 1080],
  [2560, 1440],
] as const;
const FRAME_CAPS = [30, 60, 120, 144, -1];

function Choice(props: { label: string; active?: boolean; disabled?: boolean; onClick: () => void }) {
  return (
    <box
      onClick={props.disabled ? undefined : props.onClick}
      style={{
        padding: "5px 11px",
        borderRadius: 6,
        cursor: props.disabled ? "default" : "pointer",
        opacity: props.disabled ? 0.4 : 1,
        backgroundColor: props.active ? "#4d8dff" : "rgba(255,255,255,0.1)",
      }}
    >
      <text style={{ color: "#f0f3f8", fontSize: 13 }}>{props.label}</text>
    </box>
  );
}

function Row(props: { t: Theme; label: string; children: React.ReactNode }) {
  return (
    <box style={{ flexDirection: "row", alignItems: "center", gap: 10 }}>
      <text style={{ color: props.t.label, fontSize: 13, width: 100, flexShrink: 0 }}>{props.label}</text>
      <box style={{ flexDirection: "row", gap: 6, flexWrap: "wrap" }}>{props.children}</box>
    </box>
  );
}

/** Window mode, resolution, frame cap and quality; saved when it closes. */
function Settings(props: { t: Theme; style: Style }) {
  const { t } = props;
  // The engine applies window changes after this frame: look again shortly after.
  const [, setTick] = useState(0);
  const refresh = () => setTimeout(() => setTick((t) => t + 1), 100);
  // Choices made in the engine's video-settings dialog show here too.
  useEffect(() => Application.onVideoSettingsChanged(() => setTick((t) => t + 1)), []);
  const mode = Screen.fullScreenMode;
  const size = Screen.windowSize;
  const monitor = Screen.currentResolution;
  // Window sizes that fit the monitor; exclusive fullscreen uses the monitor's own modes.
  const sizes =
    mode === "ExclusiveFullScreen"
      ? Screen.resolutions.slice(0, 6).map((r) => [r.width, r.height, r.refreshRate] as const)
      : WINDOW_SIZES.filter(([w, h]) => monitor.width === 0 || (w <= monitor.width && h <= monitor.height)).map(([w, h]) => [w, h, 0] as const);
  const cap = Application.targetFrameRate;
  return (
    <box style={{ ...t.panel, padding: 22, gap: 12, pointerEvents: "auto", minWidth: 460 }}>
      <text style={{ color: "#ffffff", fontSize: 22, fontWeight: 700 }}>Settings</text>
      <Row t={t} label="Style">
        <Choice label="Classic" active={props.style === "classic"} onClick={() => switchStyle("classic")} />
        <Choice label="Deluxe" active={props.style === "deluxe"} onClick={() => switchStyle("deluxe")} />
      </Row>
      <Row t={t} label="Window">
        {Screen.supportedFullScreenModes.map((m) => (
          <Choice
            key={m}
            label={MODE_NAMES[m]}
            active={m === mode}
            onClick={() => {
              Screen.fullScreenMode = m;
              refresh();
            }}
          />
        ))}
      </Row>
      <Row t={t} label="Resolution">
        {sizes.map(([w, h, hz]) => (
          <Choice
            key={`${w}x${h}@${hz}`}
            label={hz ? `${w}×${h} ${Math.round(hz)}Hz` : `${w}×${h}`}
            active={w === size.width && h === size.height}
            disabled={mode === "FullScreenWindow" || mode === "MaximizedWindow"}
            onClick={() => {
              Screen.setResolution(w, h, mode, hz);
              refresh();
            }}
          />
        ))}
      </Row>
      <Row t={t} label="Frame cap">
        {FRAME_CAPS.map((fps) => (
          <Choice
            key={fps}
            label={fps > 0 ? String(fps) : "None"}
            active={cap === fps}
            onClick={() => {
              Application.targetFrameRate = fps;
              PlayerPrefs.setInt("arcade.frameCap", fps);
              refresh();
            }}
          />
        ))}
      </Row>
      <Row t={t} label="VSync">
        {[1, 0].map((v) => (
          <Choice
            key={v}
            label={v ? "On" : "Off"}
            active={QualitySettings.vSyncCount === v}
            onClick={() => {
              QualitySettings.vSyncCount = v;
              refresh();
            }}
          />
        ))}
      </Row>
      <Row t={t} label="Quality">
        {QualitySettings.names.map((name, i) => (
          <Choice
            key={name}
            label={name}
            active={i === QualitySettings.getQualityLevel()}
            onClick={() => {
              QualitySettings.setQualityLevel(i);
              refresh();
            }}
          />
        ))}
      </Row>
      <text style={{ color: t.label, fontSize: 12 }}>VSync paces frames to the display; with it off, the frame cap keeps the GPU from running flat out.</text>
      <box style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        {/* The engine's own dialog (window mode, resolution, vsync, quality); this menu shows its choice after. */}
        <Choice label="Video settings..." onClick={() => Application.showVideoSettings()} />
        <Choice label="Back to the game (Esc)" active onClick={() => GameManager.current?.setPaused(false)} />
      </box>
    </box>
  );
}

function RoundOver(props: { t: Theme; score: number; best: number }) {
  return (
    <box style={{ width: "100%", height: "100%", alignItems: "center", justifyContent: "center" }}>
      <box style={{ ...props.t.panel, padding: 28, gap: 10, alignItems: "center", pointerEvents: "auto" }}>
        <text style={{ color: "#ffffff", fontSize: 28, fontWeight: 700 }}>Round over</text>
        <text style={{ color: "#ffcf4a", fontSize: 20 }}>{`${props.score} points${props.score >= props.best && props.score > 0 ? " — a new best!" : ""}`}</text>
        <box
          onClick={() => GameManager.current?.restart()}
          style={{ marginTop: 6, padding: "8px 18px", borderRadius: 8, backgroundColor: "#4d8dff", cursor: "pointer" }}
        >
          <text style={{ color: "#ffffff", fontSize: 15, fontWeight: 600 }}>Play again</text>
        </box>
      </box>
    </box>
  );
}

function Hud() {
  const game = useGame();
  // The popups fade by time, so redraw while any are showing.
  const [, setFrame] = useState(0);
  useEffect(() => {
    if (!game?.recent.some((p) => Time.time - p.at < 1.2)) return;
    const t = setTimeout(() => setFrame((f) => f + 1), 100);
    return () => clearTimeout(t);
  });
  if (!game) return null;
  const t = THEMES[game.style];
  const other: Style = game.style === "deluxe" ? "classic" : "deluxe";
  return (
    <box style={{ width: "100%", height: "100%", padding: 18, pointerEvents: "none", flexDirection: "column" }}>
      <box style={{ flexDirection: "row", justifyContent: "space-between", alignItems: "flex-start" }}>
        <ScoreBoard t={t} score={game.score} best={game.best} combo={game.combo} recent={game.recent} />
        <box style={{ flexDirection: "column", alignItems: "flex-end", gap: 8 }}>
          <Timer t={t} timeLeft={game.timeLeft} />
          <box style={{ pointerEvents: "auto", flexDirection: "row", gap: 6 }}>
            <Choice label={other === "deluxe" ? "Deluxe" : "Classic"} onClick={() => switchStyle(other)} />
            <Choice label="Settings (Esc)" onClick={() => GameManager.current?.setPaused(true)} />
          </box>
        </box>
      </box>
      {game.paused && (
        <box style={{ position: "absolute", left: 0, top: 0, width: "100%", height: "100%", alignItems: "center", justifyContent: "center" }}>
          <Settings t={t} style={game.style} />
        </box>
      )}
      {game.over && !game.paused && (
        <box style={{ position: "absolute", left: 0, top: 0, width: "100%", height: "100%" }}>
          <RoundOver t={t} score={game.score} best={game.best} />
        </box>
      )}
    </box>
  );
}

createRoot().render(<Hud />);
