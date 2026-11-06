// The sine-circle demo's curve, copied VERBATIM from commit 9dc2f51
// (examples/sine-circle-demo/src/main.tsx: "a point every degree with the original's smoothing"),
// before LineRenderer2D and the poly shapes. It is the metric for the old, unoptimised path: the
// same source runs in QuickJS (bundled by bun, with the engine's `Vector3` and `Color`) and
// compiled by web_ui_ts (where `Vector3` is the engine API's value class).
// Changes from the original: the `color = Color.white` field and the `Color` import are removed
// (`Color` is not in the compiled engine API yet); this header and the driver at the end are new; see docs/research/platypus-ts-sine-demo.md.
import { Vector3 } from "@platypus/engine";

// ---- Geometry: form pixels, as in the original ----

const FORM_W = 1008;
const FORM_H = 729;
const CENTER = { x: FORM_W / 2, y: FORM_H / 2 };
/** The whole form height spans 10 world units (the camera's orthographic size is 5). */
const UNITS_PER_PX = 10 / FORM_H;

/** Spline samples between each pair of the 361 points (about 1,100 segments in all). */
const SPLINE_STEPS = 3;

type SineType = "pi" | "normal" | "doubleSine" | "doubleCos";

/** The four formulas, in the order left click cycles them; fast or slow transitions. */
const SINE_TYPES_UNUSED = 0;

// ---- The curve ----

/** A sine circle in form pixels: its radius wobbles by `depth` at `frequency` around `center`. */
class SineCircle {
  radius = FORM_H / 3;
  frequency = 40;
  depth = 40;
  /** Degrees. */
  rotation = 0;
  center = { x: CENTER.x, y: CENTER.y };
  type: SineType = "normal";
  maxDegrees = 360;

  /** The point for one degree, in form pixels (y down, as the original's WinForms space). */
  private at(d: number): { x: number; y: number } {
    const rad = (d * Math.PI) / 180;
    const rot = (this.rotation * Math.PI) / 180;
    const R = this.radius;
    const f = this.frequency;
    const k = this.depth;
    let tr: number;
    switch (this.type) {
      case "normal": {
        const r = R + k * Math.sin(f * (rad + rot));
        return { x: r * Math.cos(rad) + this.center.x, y: r * Math.sin(rad) + this.center.y };
      }
      case "pi":
        tr = R + f * Math.cos(Math.PI * rad) * Math.cos(Math.PI + k * rad);
        break;
      case "doubleSine":
        tr = R + f * Math.sin(k * d + d) * Math.sin(k * d - d);
        break;
      case "doubleCos":
        tr = R + f * Math.cos(k * d + d) * Math.sin(k * d - d);
        break;
    }
    return { x: this.center.x + tr * Math.cos(rad + rot), y: this.center.y + tr * Math.sin(rad + rot) };
  }

  /**
   * The closed curve in world units, built as the original did: a point every degree (0 to
   * 360), joined by a smooth closed curve. GDI+'s `DrawClosedCurve` is a cardinal spline at
   * tension 0.5, which is a Catmull-Rom spline; each segment is sampled `SPLINE_STEPS` times.
   */
  points(): Vector3[] {
    const raw: { x: number; y: number }[] = [];
    for (let d = 0; d <= this.maxDegrees; d++) raw.push(this.at(d));
    const n = raw.length;
    const out: Vector3[] = [];
    for (let i = 0; i < n; i++) {
      const p0 = raw[(i - 1 + n) % n]!;
      const p1 = raw[i]!;
      const p2 = raw[(i + 1) % n]!;
      const p3 = raw[(i + 2) % n]!;
      for (let k = 0; k < SPLINE_STEPS; k++) {
        const t = k / SPLINE_STEPS;
        const t2 = t * t;
        const t3 = t2 * t;
        const x = 0.5 * (2 * p1.x + (p2.x - p0.x) * t + (2 * p0.x - 5 * p1.x + 4 * p2.x - p3.x) * t2 + (3 * p1.x - p0.x - 3 * p2.x + p3.x) * t3);
        const y = 0.5 * (2 * p1.y + (p2.y - p0.y) * t + (2 * p0.y - 5 * p1.y + 4 * p2.y - p3.y) * t2 + (3 * p1.y - p0.y - 3 * p2.y + p3.y) * t3);
        out.push(new Vector3((x - CENTER.x) * UNITS_PER_PX, (CENTER.y - y) * UNITS_PER_PX, 0));
      }
    }
    out.push(out[0]!);
    return out;
  }
}


// ---- Driver (new) ----

const TYPES: SineType[] = ["pi", "normal", "doubleSine", "doubleCos"];

/** One frame of the demo's work: build the curve for frame f's parameters and fold it to a number. */
export function frame(f: number): number {
  const s = new SineCircle();
  s.type = TYPES[Math.floor(f / 50) % 4]!;
  s.radius = 100 + (f % 120);
  s.frequency = 10 + (f % 70);
  s.depth = 5 + (f % 60);
  s.rotation = (f * 3) % 360;
  s.center = { x: 504 + (f % 40), y: 364 + (f % 30) };
  const pts = s.points();
  const last = pts[pts.length - 1]!;
  return last.x + last.y + pts.length;
}

export function frames(n: number): number {
  let sum = 0;
  for (let f = 0; f < n; f++) sum += frame(f);
  return sum;
}
