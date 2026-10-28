// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// M5.a2 2D showcase: a side-on platformer. An orthographic camera follows a fox
// (a Rigidbody2D with a CapsuleCollider2D and an animated SpriteRenderer) across tiled
// ground (an EdgeCollider2D), stone platforms (BoxCollider2D) and a stack of crates
// that tumble (Rigidbody2D boxes). Spinning coins are triggers that burst into sparks
// (a ParticleSystem) when collected. Sprites sort by `sortingOrder`, not distance.
//
// Arrows / A-D run, Space jumps. Left alone for a few seconds, the fox runs and jumps
// by itself (so screenshots show it moving). The HUD shows the game's name and version
// from its project.json (`Application`) and the OS (`SystemInfo`).

const PPU = 16;
const tiles = Texture.load("assets/tiles.png", { filterMode: "point", mipmaps: false });
const foxSheet = Texture.load("assets/fox.png", { filterMode: "point", mipmaps: false });
const coinSheet = Texture.load("assets/coin.png", { filterMode: "point", mipmaps: false });
const tile = (i: number) => Sprite.create(tiles, new Rect(i * 16, 0, 16, 16), new Vector2(0.5, 0.5), PPU);
const GRASS = tile(0);
const DIRT = tile(1);
const CRATE = tile(2);
const STONE = tile(3);
const FOX = [0, 1, 2, 3, 4].map((i) => Sprite.create(foxSheet, new Rect(i * 16, 0, 16, 16), new Vector2(0.5, 0.5), PPU));
const COIN = [0, 1, 2, 3].map((i) => Sprite.create(coinSheet, new Rect(i * 16, 0, 16, 16), new Vector2(0.5, 0.5), PPU * 1.6));

/**
 * A round blob for hills and clouds, made in script: big enough to stay crisp when
 * stretched across the screen, with a one-texel anti-aliased edge.
 */
function circleTexture(): Texture {
  const n = 256;
  const px: Color[] = [];
  for (let y = 0; y < n; y++) {
    for (let x = 0; x < n; x++) {
      const d = Math.hypot(x + 0.5 - n / 2, y + 0.5 - n / 2);
      px.push(new Color(1, 1, 1, Mathf.clamp01(n / 2 - 1 - d)));
    }
  }
  return Texture.create(n, n, px);
}

let score = 0;
const scoreListeners = new Set<(n: number) => void>();

function sprite(name: string, sprite: Sprite, at: Vector3, order = 0, color = Color.white, scale = 1): GameObject {
  const go = new GameObject(name, { position: at, scale: Vector3.one.scale(scale) });
  go.addComponent(SpriteRenderer, { sprite, sortingOrder: order, color });
  return go;
}

class Fox extends Component {
  speed = 5.5;
  jumpSpeed = 9.5;
  private rb!: Rigidbody2D;
  private renderer!: SpriteRenderer;
  private ground = 0;
  private jump = false;
  private t = 0;
  private idle = 0;

  override onStart() {
    this.rb = this.getComponent(Rigidbody2D)!;
    this.renderer = this.getComponent(SpriteRenderer)!;
  }

  onCollisionEnter2D(c: Collision2D) {
    if (c.gameObject.name !== "coin") this.ground++;
  }

  onCollisionExit2D(c: Collision2D) {
    if (c.gameObject.name !== "coin") this.ground = Math.max(0, this.ground - 1);
  }

  override onUpdate(dt: number) {
    let move = input.getAxisRaw("Horizontal");
    let jump = input.getButtonDown("Jump");
    this.idle = move !== 0 || jump ? 0 : this.idle + dt;
    if (this.idle > 3 || Time.time < 6) {
      // Attract mode: run right, hopping now and then.
      move = Math.sin(Time.time * 0.35) > -0.6 ? 1 : -1;
      jump = Time.frameCount % 70 === 0;
    }
    if (jump && this.ground > 0) this.jump = true;
    this.rb.velocity = new Vector2(move * this.speed, this.rb.velocity.y);
    if (move !== 0) this.renderer.flipX = move < 0;
    // Run cycle while moving on the ground, the idle frame otherwise.
    this.t += dt;
    const running = Math.abs(move) > 0.1 && this.ground > 0;
    const frame = running ? FOX[Math.floor(this.t * 12) % 4]! : FOX[4]!;
    if (this.renderer.sprite !== frame) this.renderer.sprite = frame;
  }

  override onFixedUpdate() {
    if (this.jump) {
      this.jump = false;
      this.rb.velocity = new Vector2(this.rb.velocity.x, this.jumpSpeed);
    }
  }
}

class Coin extends Component {
  private t = Math.random() * 4;
  override onUpdate(dt: number) {
    this.t += dt;
    this.getComponent(SpriteRenderer)!.sprite = COIN[Math.floor(this.t * 8) % 4]!;
  }
  onTriggerEnter2D(other: Collider2D) {
    if (other.gameObject.name !== "fox") return;
    score++;
    for (const f of scoreListeners) f(score);
    const burst = new GameObject("sparkle", { position: this.transform.position });
    burst.addComponent(ParticleSystem, {
      loop: false,
      duration: 0.5,
      rateOverTime: 0,
      bursts: [{ time: 0, count: 24 }],
      startLifetime: [0.3, 0.6],
      startSpeed: [2, 4],
      startSize: [0.08, 0.16],
      startColor: [Color.hex("#fff3a0"), Color.hex("#ffb52e")],
      shape: { type: "sphere", radius: 0.1 },
      blendMode: "additive",
      gravityModifier: 0.5,
      simulationSpace: "world",
      stopAction: "destroy",
      colorOverLifetime: [
        { time: 0, color: Color.white },
        { time: 1, color: new Color(1, 1, 1, 0) },
      ],
    });
    this.gameObject.destroy();
  }
}

class CameraFollow2D extends Component {
  target: GameObject | null = null;
  override onLateUpdate(dt: number) {
    if (!this.target) return;
    const p = this.transform.position;
    const t = this.target.transform.position;
    // Kept inside the level (it runs from x = -14 to 40; the view is about 18 m wide).
    const x = Mathf.lerp(p.x, Mathf.clamp(t.x, -5, 31), 1 - Math.exp(-dt * 4));
    const y = Mathf.lerp(p.y, Math.max(2.5, t.y + 1), 1 - Math.exp(-dt * 3));
    this.transform.position = new Vector3(x, y, p.z);
  }
}

class Level extends Component {
  override onStart() {
    const cam = new GameObject("Main Camera", { position: new Vector3(0, 2.5, 10) });
    cam.addComponent(Camera, { orthographic: true, orthographicSize: 5, backgroundColor: Color.hex("#86c9ff") });

    // Scenery behind everything: hills and clouds.
    const blob = Sprite.create(circleTexture(), null, new Vector2(0.5, 0.5), 256);
    for (let i = 0; i < 9; i++) {
      const x = -14 + i * 6.5;
      sprite("hill", blob, new Vector3(x, -1, -1), -20, Color.hex(i % 2 ? "#5aa860" : "#4b9a55"), 7 + (i % 3) * 2);
    }
    for (let i = 0; i < 7; i++) {
      const at = new Vector3(-10 + i * 7.5, 6 + (i % 3) * 0.8, -1);
      for (const [dx, dy, s] of [
        [0, 0, 1.6],
        [0.9, 0.3, 1.2],
        [-0.9, 0.2, 1.1],
      ] as const) {
        sprite("cloud", blob, at.add(new Vector3(dx, dy, 0)), -10, new Color(1, 1, 1, 0.92), s);
      }
    }

    // Ground: grass on dirt, with its top edge at y = 0.
    for (let x = -14; x < 40; x++) {
      sprite("grass", GRASS, new Vector3(x + 0.5, -0.5, 0));
      for (let y = 1; y < 4; y++) sprite("dirt", DIRT, new Vector3(x + 0.5, -0.5 - y, 0));
    }
    const ground = new GameObject("ground");
    ground.addComponent(EdgeCollider2D, { points: [new Vector2(-14, 0), new Vector2(40, 0)] });
    // Walls at the ends.
    for (const x of [-14.5, 40.5]) {
      const wall = new GameObject("wall", { position: new Vector3(x, 5, 0) });
      wall.addComponent(BoxCollider2D, { size: new Vector2(1, 10) });
    }

    // Stone platforms.
    for (const [x0, y, n] of [
      [3, 2.5, 3],
      [8, 4, 4],
      [15, 2.5, 3],
      [21, 4.5, 3],
    ] as const) {
      const p = new GameObject("platform", { position: new Vector3(x0 + n / 2, y, 0) });
      p.addComponent(BoxCollider2D, { size: new Vector2(n, 1) });
      for (let i = 0; i < n; i++) sprite("stone", STONE, new Vector3(x0 + i + 0.5, y, 0));
    }

    // A stack of crates to knock over.
    for (let row = 0; row < 3; row++) {
      for (let i = 0; i < 3 - row; i++) {
        const c = sprite("crate", CRATE, new Vector3(27 + i + row * 0.5, 0.5 + row, 0), 1);
        c.addComponent(Rigidbody2D, { mass: 0.6 });
        c.addComponent(BoxCollider2D);
      }
    }

    // Coins over the platforms.
    for (const [x, y] of [
      [4.5, 4], [9, 5.5], [10, 5.5], [16.5, 4], [22.5, 6], [-2, 1.2], [0, 1.2], [12.5, 1.2],
    ] as const) {
      const coin = sprite("coin", COIN[0]!, new Vector3(x, y, 0), 2);
      coin.addComponent(CircleCollider2D, { radius: 0.35, isTrigger: true });
      coin.addComponent(Coin);
    }

    const fox = sprite("fox", FOX[4]!, new Vector3(-4, 1, 0), 5, Color.white, 1.3);
    fox.addComponent(Rigidbody2D, { freezeRotation: true, gravityScale: 2.2, collisionDetectionMode: "continuous" });
    fox.addComponent(CapsuleCollider2D, { size: new Vector2(0.6, 0.62), offset: new Vector2(0, -0.08) });
    fox.addComponent(Fox);
    cam.addComponent(CameraFollow2D, { target: fox });
  }
}

register({ Level });

function Hud() {
  const [coins, setCoins] = useState(score);
  useEffect(() => {
    scoreListeners.add(setCoins);
    return () => void scoreListeners.delete(setCoins);
  }, []);
  return (
    <box style={{ width: "100%", height: "100%", padding: 20, pointerEvents: "none" }}>
      <box
        style={{
          alignSelf: "flex-start",
          padding: "10px 16px",
          borderRadius: 10,
          backgroundColor: "rgba(20, 30, 60, 0.55)",
          flexDirection: "column",
          gap: 4,
        }}
      >
        <text style={{ color: "#ffffff", fontSize: 18, fontWeight: 600 }}>{`Coins: ${coins}`}</text>
        <text style={{ color: "#d6e6ff", fontSize: 13 }}>Arrows / A-D to run, Space to jump</text>
        <text style={{ color: "#b8cff0", fontSize: 11 }}>{`${Application.productName} ${Application.version} · ${SystemInfo.operatingSystem}`}</text>
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
