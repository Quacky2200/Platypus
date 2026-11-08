// A port of the Doom fire algorithm by Filipe Deschamps (MIT,
// https://github.com/filipedeschamps/doom-fire-algorithm), in two versions you switch between with G:
//
//  - Particles (the default): the fire is a row of ParticleSystems. The algorithm's pieces become
//    - the source row: one heat value per column (0..36), kept and decayed as in the original;
//    - the cooling: a particle's colour over its life walks the original's 37-colour palette from white
//      down to black, and its size shrinks as it cools;
//    - the flame height: a column's heat sets how long its particles live.
//  - Grid: the original, cell for cell. A grid of heat values, a source row, each cell taking the heat
//    of the cell below minus a random amount, moved sideways by chance. Each tick the grid becomes one
//    texture shown on a sprite (a UI box per cell, or per row, was far slower).
//
// Space puts the fire out or lights it again (the source row decays or recovers a column at a time).

/*
MIT License

Copyright (c) 2019 Filipe Deschamps

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

// The original's palette, heat 0 (black) to 36 (white).
const PALETTE = [
  [7, 7, 7], [31, 7, 7], [47, 15, 7], [71, 15, 7], [87, 23, 7], [103, 31, 7], [119, 31, 7], [143, 39, 7],
  [159, 47, 7], [175, 63, 7], [191, 71, 7], [199, 71, 7], [223, 79, 7], [223, 87, 7], [223, 87, 7],
  [215, 95, 7], [215, 95, 7], [215, 103, 15], [207, 111, 15], [207, 119, 15], [207, 127, 15],
  [207, 135, 23], [199, 135, 23], [199, 143, 23], [199, 151, 31], [191, 159, 31], [191, 159, 31],
  [191, 167, 39], [191, 167, 39], [191, 175, 47], [183, 175, 47], [183, 183, 47], [183, 183, 55],
  [207, 207, 111], [223, 223, 159], [239, 239, 199], [255, 255, 255],
] as const;
const MAX_HEAT = PALETTE.length - 1; // 36

const COLUMNS = 56; // particle emitters
const WIDTH = 20; // world units the particle fire spans
const BOTTOM = -5.2;
const MAX_RATE = 60; // particles per second from a column at full heat
const MAX_LIFE = 2.1;

const GRID_W = 256;
const GRID_H = 144;
const VIEW_H = 10; // world units the camera sees from top to bottom (orthographicSize 5)
const TICK = 1 / 30; // the fire updates at this rate

function paletteColor(heat: number, alpha = 1): Color {
  const c = PALETTE[Math.max(0, Math.min(MAX_HEAT, heat))]!;
  return new Color(c[0] / 255, c[1] / 255, c[2] / 255, alpha);
}

// One Color per heat, made once: filling a texture's pixel array then allocates nothing.
// Texture.create takes linear colours, so the palette's sRGB values are converted.
const toLinear = (v: number) => (v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4));
const HEAT_COLORS = PALETTE.map((c) => new Color(toLinear(c[0] / 255), toLinear(c[1] / 255), toLinear(c[2] / 255), 1));

type Mode = "particles" | "grid";
let mode: Mode = "particles";
const modeListeners = new Set<(m: Mode) => void>();
let flameHeight = 0.35; // grid mode: the fraction of the screen's height a flame cools out in (Up / Down)
let burning = true;
const burningListeners = new Set<(v: boolean) => void>();

class DoomFire extends Component {
  private source: number[] = []; // heat of each column's source (the bottom row)
  private grid: number[] = [];
  private pixels: Color[] = []; // the grid as colours, bottom row first
  private view: SpriteRenderer | null = null;
  private texture: Texture | null = null;
  private emitters: ParticleSystem[] = [];
  private applied: number[] = [];
  private acc = 0;

  override onStart() {
    const cam = new GameObject("Main Camera", { position: new Vector3(0, 0, 10) });
    cam.addComponent(Camera, { orthographic: true, orthographicSize: 5, backgroundColor: Color.hex("#070707") });

    for (let i = 0; i < GRID_W * GRID_H; i++) {
      this.grid.push(0);
      this.pixels.push(HEAT_COLORS[0]!);
    }
    // The grid's sprite covers the view (20 x 10 units); it is empty until grid mode shows it.
    const screen = new GameObject("grid", { position: Vector3.zero });
    this.view = screen.addComponent(SpriteRenderer, { sprite: null, sortingOrder: 10 });
    const cell = WIDTH / COLUMNS;
    for (let x = 0; x < COLUMNS; x++) {
      this.source.push(MAX_HEAT);
      this.applied.push(-1);
      const go = new GameObject("flame", { position: new Vector3(-WIDTH / 2 + (x + 0.5) * cell, BOTTOM, 0) });
      const ps = go.addComponent(ParticleSystem, {
        loop: true,
        rateOverTime: MAX_RATE,
        maxParticles: 220,
        startLifetime: [MAX_LIFE * 0.5, MAX_LIFE],
        startSpeed: [1.2, 5],
        randomSeed: 1000 + x * 7919,
        startSize: [0.9, 1.5],
        // Additive: overlapping particles add up, so each is dim (an eighth of the palette's colour) and
        // the heat shows where they pile up. The cone fans them out from the column.
        startColor: new Color(0.14, 0.14, 0.14, 1),
        shape: { type: "cone", radius: cell, angle: 22 },
        blendMode: "additive",
        drag: 0.2,
        gravityModifier: 0,
        simulationSpace: "world",
      });
      // Cooling: the palette from white-hot (heat 36) down to black, a dozen keys is enough
      // for the linear blend between them to follow it closely.
      const keys = 12;
      for (let i = 0; i <= keys; i++) {
        const t = i / keys;
        const h = Math.round((1 - t) * MAX_HEAT);
        ps.addColorKey(t, paletteColor(h, i === keys ? 0 : 1));
      }
      ps.addSizeKey(0, 1);
      ps.addSizeKey(0.5, 0.8);
      ps.addSizeKey(1, 0.3);
      this.emitters.push(ps);
    }
    this.setMode(mode);
  }

  private setMode(m: Mode) {
    mode = m;
    for (const ps of this.emitters) {
      if (m === "particles") ps.play();
      else ps.stop(true);
    }
    if (m === "particles" && this.view) this.view.sprite = null;
    for (const f of modeListeners) f(m);
  }

  override onUpdate(dt: number) {
    if (input.getKeyDown("Space")) {
      burning = !burning;
      for (const f of burningListeners) f(burning);
    }
    if (input.getKeyDown("ArrowUp")) flameHeight = Math.min(1, flameHeight + 0.05);
    if (input.getKeyDown("ArrowDown")) flameHeight = Math.max(0.1, flameHeight - 0.05);
    if (input.getKeyDown("KeyG")) this.setMode(mode === "particles" ? "grid" : "particles");

    this.acc += dt;
    if (this.acc < TICK) return;
    this.acc %= TICK;

    // The source row: it fades column by column when the fire is out (the original's `decay`) and
    // relights when it is back.
    for (let x = 0; x < COLUMNS; x++) {
      const h = this.source[x]!;
      if (burning) this.source[x] = Math.min(MAX_HEAT, h + 1 + Math.floor(Math.random() * 3));
      else this.source[x] = Math.max(0, h - Math.floor(Math.random() * 3));
    }

    if (mode === "grid") this.stepGrid();
    else this.stepParticles();
  }

  private stepParticles() {
    for (let x = 0; x < COLUMNS; x++) {
      // A column also takes some of its neighbours' heat, so flames lean into one another.
      const l = this.source[Math.max(0, x - 1)]!;
      const r = this.source[Math.min(COLUMNS - 1, x + 1)]!;
      const eff = Math.max(0, Math.min(MAX_HEAT, Math.round((this.source[x]! * 2 + l + r) / 4)));
      // Only when the heat changed: rewriting the settings every tick makes the emitters fire in lockstep.
      if (eff === this.applied[x]) continue;
      this.applied[x] = eff;
      const f = eff / MAX_HEAT;
      const ps = this.emitters[x]!;
      ps.rateOverTime = MAX_RATE * f * f;
      ps.setStartLifetimeRange(Math.max(0.05, MAX_LIFE * f * 0.5), Math.max(0.1, MAX_LIFE * f));
    }
  }

  // The original's update: each cell takes the cell below minus a random amount, moved sideways by chance.
  private stepGrid() {
    const g = this.grid;
    const cool = Math.min(1, MAX_HEAT / (GRID_H * flameHeight));
    const last = (GRID_H - 1) * GRID_W;
    for (let x = 0; x < GRID_W; x++) g[last + x] = this.source[Math.floor((x * COLUMNS) / GRID_W)]!;
    for (let row = 1; row < GRID_H; row++) {
      for (let x = 0; x < GRID_W; x++) {
        const below = g[row * GRID_W + x]!;
        const decay = Math.floor(Math.random() * 3);
        // The original only moves left and cools by `decay & 1` (a third of a step per row on average), so
        // its height depends on the grid's size. Here the chance of cooling per row is set so a flame
        // cools out after `flameHeight` of the grid's rows, and a cell moves one cell either way
        // (moving only left leaves the right edge stale).
        const dx = x + 1 - decay;
        if (dx >= 0 && dx < GRID_W) g[(row - 1) * GRID_W + dx] = Math.max(0, below - (Math.random() < cool ? 1 : 0));
      }
    }
    // Texture.create takes the bottom row first.
    const px = this.pixels;
    for (let row = 0; row < GRID_H; row++) {
      const out = (GRID_H - 1 - row) * GRID_W;
      for (let x = 0; x < GRID_W; x++) px[out + x] = HEAT_COLORS[g[row * GRID_W + x]!]!;
    }
    const old = this.texture;
    const tex = Texture.create(GRID_W, GRID_H, px, { filterMode: "point", mipmaps: false });
    const ppu = GRID_W / WIDTH;
    this.view!.sprite = Sprite.create(tex, null, new Vector2(0.5, 0.5), ppu);
    this.view!.transform.localScale = new Vector3(1, VIEW_H / (GRID_H / ppu), 1);
    this.texture = tex;
    if (old) old.destroy();
  }
}

register({ DoomFire });

function Hud() {
  const [m, setM] = useState(mode);
  const [on, setOn] = useState(burning);
  useEffect(() => {
    modeListeners.add(setM);
    burningListeners.add(setOn);
    return () => {
      modeListeners.delete(setM);
      burningListeners.delete(setOn);
    };
  }, []);
  return (
    <box style={{ width: "100%", height: "100%", pointerEvents: "none" }}>
      <box style={{ position: "absolute", left: 20, top: 20, padding: "10px 16px", borderRadius: 10, backgroundColor: "rgba(0, 0, 0, 0.55)", flexDirection: "column", gap: 4 }}>
        <text style={{ color: "#ffd27a", fontSize: 18, fontWeight: 600 }}>{`Doom fire: ${m}${on ? "" : " (out)"}`}</text>
        <text style={{ color: "#e8c9a0", fontSize: 13 }}>Space: light / put out · G: particles / grid · Up / Down: grid flame height</text>
        <text style={{ color: "#a08868", fontSize: 11 }}>after filipedeschamps/doom-fire-algorithm (MIT)</text>
      </box>
    </box>
  );
}

createRoot().render(<Hud />);
