// The sine-circle demo's curve maths (examples/sine-circle-demo/src/main.tsx `at` and `points`),
// as free functions in the subset web_ui_ts compiles: a point every degree, joined by a
// Catmull-Rom spline (3 samples per segment), then flattened to [x0, y0, x1, y1, ...] as
// LineRenderer2D.send() does before its `render.stroke` op.
const FORM_W = 1008;
const FORM_H = 729;
const CX = FORM_W / 2;
const CY = FORM_H / 2;
const UNITS_PER_PX = 10 / FORM_H;
const SPLINE_STEPS = 3;

export type SineType = "pi" | "normal" | "doubleSine" | "doubleCos";

export interface Pt {
  x: number;
  y: number;
}

function at(type: SineType, radius: number, frequency: number, depth: number, rotation: number, cx: number, cy: number, d: number): Pt {
  const rad = (d * Math.PI) / 180;
  const rot = (rotation * Math.PI) / 180;
  if (type === "normal") {
    const r = radius + depth * Math.sin(frequency * (rad + rot));
    return { x: r * Math.cos(rad) + cx, y: r * Math.sin(rad) + cy };
  }
  let tr = 0;
  if (type === "pi") tr = radius + frequency * Math.cos(Math.PI * rad) * Math.cos(Math.PI + depth * rad);
  else if (type === "doubleSine") tr = radius + frequency * Math.sin(depth * d + d) * Math.sin(depth * d - d);
  else tr = radius + frequency * Math.cos(depth * d + d) * Math.sin(depth * d - d);
  return { x: cx + tr * Math.cos(rad + rot), y: cy + tr * Math.sin(rad + rot) };
}

export function points(type: SineType, radius: number, frequency: number, depth: number, rotation: number, cx: number, cy: number): Pt[] {
  const raw: Pt[] = [];
  let d = 0;
  while (d <= 360) {
    raw.push(at(type, radius, frequency, depth, rotation, cx, cy, d));
    d += 1;
  }
  const n = raw.length;
  const out: Pt[] = [];
  let i = 0;
  while (i < n) {
    const p0 = raw[(i - 1 + n) % n]!;
    const p1 = raw[i]!;
    const p2 = raw[(i + 1) % n]!;
    const p3 = raw[(i + 2) % n]!;
    let k = 0;
    while (k < SPLINE_STEPS) {
      const t = k / SPLINE_STEPS;
      const t2 = t * t;
      const t3 = t2 * t;
      const x = 0.5 * (2 * p1.x + (p2.x - p0.x) * t + (2 * p0.x - 5 * p1.x + 4 * p2.x - p3.x) * t2 + (3 * p1.x - p0.x - 3 * p2.x + p3.x) * t3);
      const y = 0.5 * (2 * p1.y + (p2.y - p0.y) * t + (2 * p0.y - 5 * p1.y + 4 * p2.y - p3.y) * t2 + (3 * p1.y - p0.y - 3 * p2.y + p3.y) * t3);
      out.push({ x: (x - CX) * UNITS_PER_PX, y: (CY - y) * UNITS_PER_PX });
      k += 1;
    }
    i += 1;
  }
  return out;
}

export function flatten(pts: Pt[]): number[] {
  const flat: number[] = [];
  pts.forEach((p) => {
    flat.push(p.x);
    flat.push(p.y);
  });
  return flat;
}

/** What one frame of the demo sends to Rust: the curve as a flat list of coordinates. */
export function curveFlat(type: SineType, radius: number, frequency: number, depth: number, rotation: number, cx: number, cy: number): number[] {
  return flatten(points(type, radius, frequency, depth, rotation, cx, cy));
}
