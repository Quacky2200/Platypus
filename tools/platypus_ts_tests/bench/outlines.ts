// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Differential oracle for the native `Shapes` and `Spline`: the TypeScript the compiled facade used
// (`tools/web_ui_ts/src/lower/facade.ts` before they became native Rust, renamed `Ref*`), run next to
// the native API on the same inputs. Every point must be the same double (bit for bit). Test
// code only; the engine has one implementation (platypus_math). tests/engine_api/outlines.rs reads the results.

function __rshapes_at(cx: number, cy: number, rx: number, ry: number, degrees: number): Vector2 {
  return new Vector2(cx + Math.cos(degrees * 0.017453292519943295) * rx, cy + Math.sin(degrees * 0.017453292519943295) * ry);
}
function __rshapes_pick(points: Vector2[], i: number, closed: boolean): Vector2 {
  const n = points.length;
  if (closed) return points[((i % n) + n) % n]!;
  return points[Math.max(0, Math.min(n - 1, i))]!;
}
/** A cardinal spline through `points`, `segments` points per span (Hermite form). */
function __rshapes_cardinal(points: Vector2[], tension: number, segments: number, closed: boolean): Vector2[] {
  const n = points.length;
  const out: Vector2[] = [];
  if (n < 2) {
    for (let i = 0; i < n; i++) out.push(new Vector2(points[i]!.x, points[i]!.y));
    return out;
  }
  const spans = closed ? n : n - 1;
  for (let i = 0; i < spans; i++) {
    const p0 = __rshapes_pick(points, i - 1, closed);
    const p1 = __rshapes_pick(points, i, closed);
    const p2 = __rshapes_pick(points, i + 1, closed);
    const p3 = __rshapes_pick(points, i + 2, closed);
    // GDI+ tangents: `tension` times the chord across each point (0.5 is Catmull-Rom).
    const m1x = tension * (p2.x - p0.x);
    const m1y = tension * (p2.y - p0.y);
    const m2x = tension * (p3.x - p1.x);
    const m2y = tension * (p3.y - p1.y);
    for (let k = 0; k < segments; k++) {
      const t = k / segments;
      const t2 = t * t;
      const t3 = t2 * t;
      const h00 = 2 * t3 - 3 * t2 + 1;
      const h10 = t3 - 2 * t2 + t;
      const h01 = -2 * t3 + 3 * t2;
      const h11 = t3 - t2;
      out.push(new Vector2(h00 * p1.x + h10 * m1x + h01 * p2.x + h11 * m2x, h00 * p1.y + h10 * m1y + h01 * p2.y + h11 * m2y));
    }
  }
  if (!closed) out.push(new Vector2(points[n - 1]!.x, points[n - 1]!.y));
  return out;
}
/** GDI-style shape outlines for `LineRenderer2D.points`. */
class RefShapes {
  /** A rectangle from its bottom-left corner (GDI DrawRectangle). */
  static rectangle(x: number, y: number, width: number, height: number): Vector2[] {
    return [new Vector2(x, y), new Vector2(x + width, y), new Vector2(x + width, y + height), new Vector2(x, y + height)];
  }

  /** A rectangle with corners rounded to `radius` (at most half the shorter side). */
  static roundedRectangle(x: number, y: number, width: number, height: number, radius: number, segments: number = 8): Vector2[] {
    const r = Math.max(0, Math.min(radius, Math.min(width / 2, height / 2)));
    if (r === 0) return RefShapes.rectangle(x, y, width, height);
    const out: Vector2[] = [];
    const corners: number[][] = [
      [x + width - r, y + r, -90],
      [x + width - r, y + height - r, 0],
      [x + r, y + height - r, 90],
      [x + r, y + r, 180],
    ];
    for (let c = 0; c < 4; c++) {
      const k = corners[c]!;
      for (let i = 0; i <= segments; i++) out.push(__rshapes_at(k[0]!, k[1]!, r, r, k[2]! + (90 * i) / segments));
    }
    return out;
  }

  /** An ellipse around `center` with radii `rx` and `ry` (GDI DrawEllipse). */
  static ellipse(center: Vector2, rx: number, ry: number, segments: number = 64): Vector2[] {
    const out: Vector2[] = [];
    for (let i = 0; i < segments; i++) out.push(__rshapes_at(center.x, center.y, rx, ry, (360 * i) / segments));
    return out;
  }

  static circle(center: Vector2, radius: number, segments: number = 64): Vector2[] {
    return RefShapes.ellipse(center, radius, radius, segments);
  }

  /** Part of an ellipse from `startAngle`, `sweepAngle` degrees round (negative goes clockwise), GDI DrawArc. */
  static arc(center: Vector2, rx: number, ry: number, startAngle: number, sweepAngle: number, segments?: number): Vector2[] {
    const n = Math.max(1, segments !== undefined ? segments : Math.ceil((Math.abs(sweepAngle) / 360) * 64));
    const out: Vector2[] = [];
    for (let i = 0; i <= n; i++) out.push(__rshapes_at(center.x, center.y, rx, ry, startAngle + (sweepAngle * i) / n));
    return out;
  }

  /** A pie slice: the centre, then the arc (GDI DrawPie / FillPie). */
  static pie(center: Vector2, rx: number, ry: number, startAngle: number, sweepAngle: number, segments?: number): Vector2[] {
    const out: Vector2[] = [new Vector2(center.x, center.y)];
    const arc = RefShapes.arc(center, rx, ry, startAngle, sweepAngle, segments);
    for (let i = 0; i < arc.length; i++) out.push(arc[i]!);
    return out;
  }

  /** `sides` corners evenly round a circle, the first at `rotation` degrees (90: on top). */
  static regularPolygon(center: Vector2, radius: number, sides: number, rotation: number = 90): Vector2[] {
    const n = Math.max(3, Math.round(sides));
    const out: Vector2[] = [];
    for (let i = 0; i < n; i++) out.push(__rshapes_at(center.x, center.y, radius, radius, rotation + (360 * i) / n));
    return out;
  }

  /** The points as given (GDI DrawPolygon). */
  static polygon(...points: Vector2[]): Vector2[] {
    const out: Vector2[] = [];
    for (let i = 0; i < points.length; i++) out.push(new Vector2(points[i]!.x, points[i]!.y));
    return out;
  }

  static triangle(a: Vector2, b: Vector2, c: Vector2): Vector2[] {
    return [new Vector2(a.x, a.y), new Vector2(b.x, b.y), new Vector2(c.x, c.y)];
  }

  /** A cubic Bezier from `p0` to `p3`, pulled toward `p1` and `p2` (GDI DrawBezier). */
  static bezier(p0: Vector2, p1: Vector2, p2: Vector2, p3: Vector2, segments: number = 32): Vector2[] {
    const out: Vector2[] = [];
    for (let i = 0; i <= segments; i++) {
      const t = i / segments;
      const u = 1 - t;
      const a = u * u * u;
      const b = 3 * u * u * t;
      const c = 3 * u * t * t;
      const d = t * t * t;
      out.push(new Vector2(a * p0.x + b * p1.x + c * p2.x + d * p3.x, a * p0.y + b * p1.y + c * p2.y + d * p3.y));
    }
    return out;
  }

  /** A smooth curve through every point (GDI DrawCurve): `tension` 0 is straight lines, 0.5 a Catmull-Rom spline. */
  static curve(points: Vector2[], tension: number = 0.5, segments: number = 8): Vector2[] {
    return __rshapes_cardinal(points, tension, segments, false);
  }

  /** The same, closed back to the first point (GDI DrawClosedCurve / FillClosedCurve). */
  static closedCurve(points: Vector2[], tension: number = 0.5, segments: number = 8): Vector2[] {
    return __rshapes_cardinal(points, tension, segments, true);
  }
}


const same = (a: number, b: number): boolean => a === b || (a !== a && b !== b);

function samePoints(name: string, a: Vector2[], b: Vector2[]): string {
  if (a.length !== b.length) return `${name}: ${a.length} points, expected ${b.length}`;
  for (let i = 0; i < a.length; i++) {
    if (!same(a[i]!.x, b[i]!.x) || !same(a[i]!.y, b[i]!.y)) {
      return `${name}[${i}]: (${a[i]!.x}, ${a[i]!.y}) expected (${b[i]!.x}, ${b[i]!.y})`;
    }
  }
  return "";
}

const sample: Vector2[] = [new Vector2(0, 0), new Vector2(1, 2), new Vector2(3, 1), new Vector2(4, 4), new Vector2(-2, 3), new Vector2(0.5, -1.5)];

/** Runs every shape on edge cases; "" when all agree, else the first difference. */
export function shapesMatch(): string {
  const o = new Vector2(1.5, -2.5);
  const checks: string[] = [];
  for (const r of [[1, 2, 3, 4], [0, 0, 0, 0], [-1, -2, -3, -4], [0.1, 0.2, 1e-7, 1e9]]) {
    checks.push(samePoints("rectangle", Shapes.rectangle(r[0]!, r[1]!, r[2]!, r[3]!), RefShapes.rectangle(r[0]!, r[1]!, r[2]!, r[3]!)));
  }
  for (const r of [[0, 0, 4, 2, 5], [1, 2, 10, 6, 1.5], [0, 0, 4, 2, 0], [0, 0, 0, 0, 1], [0, 0, 4, 2, -3], [-3, 5, 7, 7, 3.5]]) {
    checks.push(samePoints("roundedRectangle", Shapes.roundedRectangle(r[0]!, r[1]!, r[2]!, r[3]!, r[4]!), RefShapes.roundedRectangle(r[0]!, r[1]!, r[2]!, r[3]!, r[4]!)));
    for (const seg of [1, 3, 8, 12]) {
      checks.push(samePoints("roundedRectangle", Shapes.roundedRectangle(r[0]!, r[1]!, r[2]!, r[3]!, r[4]!, seg), RefShapes.roundedRectangle(r[0]!, r[1]!, r[2]!, r[3]!, r[4]!, seg)));
    }
  }
  checks.push(samePoints("ellipse", Shapes.ellipse(o, 2, 1), RefShapes.ellipse(o, 2, 1)));
  checks.push(samePoints("circle", Shapes.circle(o, 3), RefShapes.circle(o, 3)));
  for (const seg of [0, 1, 2, 7, 64, 5.5, 200]) {
    checks.push(samePoints("ellipse", Shapes.ellipse(o, 2.5, 0.5, seg), RefShapes.ellipse(o, 2.5, 0.5, seg)));
    checks.push(samePoints("circle", Shapes.circle(o, 0, seg), RefShapes.circle(o, 0, seg)));
    checks.push(samePoints("circle", Shapes.circle(o, -2, seg), RefShapes.circle(o, -2, seg)));
  }
  for (const a of [[0, 90], [0, -90], [30, 0], [10, 720], [-45, 1e-9], [0, 360], [100, -250.5]]) {
    checks.push(samePoints("arc", Shapes.arc(o, 2, 3, a[0]!, a[1]!), RefShapes.arc(o, 2, 3, a[0]!, a[1]!)));
    checks.push(samePoints("pie", Shapes.pie(o, 2, 3, a[0]!, a[1]!), RefShapes.pie(o, 2, 3, a[0]!, a[1]!)));
    for (const seg of [1, 2, 13, 0, 4.5]) {
      checks.push(samePoints("arc", Shapes.arc(o, 2, 3, a[0]!, a[1]!, seg), RefShapes.arc(o, 2, 3, a[0]!, a[1]!, seg)));
      checks.push(samePoints("pie", Shapes.pie(o, 2, 3, a[0]!, a[1]!, seg), RefShapes.pie(o, 2, 3, a[0]!, a[1]!, seg)));
    }
  }
  for (const sides of [3, 4, 5, 6.4, 6.5, 2, 0, -1, 12]) {
    checks.push(samePoints("regularPolygon", Shapes.regularPolygon(o, 2, sides), RefShapes.regularPolygon(o, 2, sides)));
    for (const rot of [0, 45, -90, 90]) {
      checks.push(samePoints("regularPolygon", Shapes.regularPolygon(o, 2, sides, rot), RefShapes.regularPolygon(o, 2, sides, rot)));
    }
  }
  checks.push(samePoints("polygon0", Shapes.polygon(), RefShapes.polygon()));
  checks.push(samePoints("polygon1", Shapes.polygon(sample[1]!), RefShapes.polygon(sample[1]!)));
  checks.push(samePoints("polygon3", Shapes.polygon(sample[0]!, sample[1]!, { x: 9, y: 8 }), RefShapes.polygon(sample[0]!, sample[1]!, { x: 9, y: 8 })));
  checks.push(samePoints("polygon spread", Shapes.polygon(...sample), RefShapes.polygon(...sample)));
  checks.push(samePoints("triangle", Shapes.triangle(sample[0]!, sample[2]!, { x: -1, y: 7 }), RefShapes.triangle(sample[0]!, sample[2]!, { x: -1, y: 7 })));
  for (const seg of [32, 1, 2, 4, 5.5, 100]) {
    checks.push(samePoints("bezier", Shapes.bezier(sample[0]!, sample[1]!, sample[2]!, sample[3]!, seg), RefShapes.bezier(sample[0]!, sample[1]!, sample[2]!, sample[3]!, seg)));
  }
  checks.push(samePoints("bezier", Shapes.bezier(sample[0]!, sample[1]!, sample[2]!, sample[3]!), RefShapes.bezier(sample[0]!, sample[1]!, sample[2]!, sample[3]!)));
  for (let n = 0; n <= 6; n++) {
    const pts = sample.slice(0, n);
    checks.push(samePoints("curve", Shapes.curve(pts), RefShapes.curve(pts)));
    checks.push(samePoints("closedCurve", Shapes.closedCurve(pts), RefShapes.closedCurve(pts)));
    for (const tension of [0, 0.5, 1, -0.25, 2]) {
      for (const seg of [1, 4, 8, 3.5]) {
        checks.push(samePoints("curve", Shapes.curve(pts, tension, seg), RefShapes.curve(pts, tension, seg)));
        checks.push(samePoints("closedCurve", Shapes.closedCurve(pts, tension, seg), RefShapes.closedCurve(pts, tension, seg)));
      }
    }
  }
  for (let i = 0; i < checks.length; i++) {
    if (checks[i] !== "") return checks[i]!;
  }
  return "";
}

/** The shapes are `Vector2` objects the program can use like any other. */
export function shapePointsAreVectors(): string {
  const c = Shapes.circle(new Vector2(1, 1), 2, 4);
  const d = c[1]!.add(c[2]!).scale(0.5);
  const poly = Shapes.polygon({ x: 0, y: 0 }, { x: 2, y: 0 });
  return `${c.length}|${Math.round(c[0]!.x)}|${Math.round(d.y * 100)}|${poly.length}|${poly[1]!.x}`;
}

// ---- Spline ----
type RefTangentMode = "auto" | "bezier" | "linear";
interface RefKnot {
  position: Vector3;
  tangentIn: Vector3;
  tangentOut: Vector3;
  mode: RefTangentMode;
}
interface RefOptions {
  closed?: boolean;
  tension?: number;
  mode?: RefTangentMode;
}
interface RefHit {
  segment: number;
  u: number;
}
interface RefNearest {
  t: number;
  point: Vector3;
  distance: number;
}

/** A path through knots (Unity Splines, the core of it): see `js/engine/src/spline.ts`. */
class RefSpline {
  private _knots: RefKnot[] = [];
  private _closed: boolean;
  private _tension: number;
  private table: number[] | null = null;
  private ctrl: Float64Array | null = null;
  /** Scratch for one point, so evaluating allocates nothing. */
  private scratch: Float64Array = new Float64Array(3);
  /** Goes up on every change, so anything drawing the spline can tell it moved. */
  version: number = 0;

  constructor(points?: Vector3[], options?: RefOptions) {
    this._closed = false;
    this._tension = 0.5;
    let mode: RefTangentMode = "auto";
    if (options !== undefined) {
      if (options.closed !== undefined) this._closed = options.closed;
      if (options.tension !== undefined) this._tension = options.tension;
      if (options.mode !== undefined) mode = options.mode;
    }
    if (points !== undefined) {
      for (let i = 0; i < points.length; i++) this.addKnot(points[i]!, mode);
    }
  }

  get knots(): RefKnot[] {
    return this._knots;
  }

  get count(): number {
    return this._knots.length;
  }

  /** Whether the last knot joins back to the first. */
  get closed(): boolean {
    return this._closed;
  }
  set closed(on: boolean) {
    this._closed = on;
    this.changed();
  }

  /** How strongly `auto` knots bend: 0 is straight lines, 0.5 Catmull-Rom. */
  get tension(): number {
    return this._tension;
  }
  set tension(t: number) {
    this._tension = t;
    this.changed();
  }

  get segmentCount(): number {
    const n = this._knots.length;
    if (n < 2) return 0;
    return this._closed ? n : n - 1;
  }

  /** Adds a knot at the end; returns its index. */
  addKnot(position: Vector3, mode: RefTangentMode = "auto", tangentIn?: Vector3, tangentOut?: Vector3): number {
    return this.insertKnot(this._knots.length, position, mode, tangentIn, tangentOut);
  }

  /** Puts a knot before index `index` (the count: at the end); returns its index. */
  insertKnot(index: number, position: Vector3, mode: RefTangentMode = "auto", tangentIn?: Vector3, tangentOut?: Vector3): number {
    const i = Math.max(0, Math.min(this._knots.length, Math.trunc(index)));
    const out = tangentOut !== undefined ? new Vector3(tangentOut.x, tangentOut.y, tangentOut.z) : new Vector3(0, 0, 0);
    const tin = tangentIn !== undefined ? new Vector3(tangentIn.x, tangentIn.y, tangentIn.z) : out.scale(-1);
    const knot: RefKnot = { position: new Vector3(position.x, position.y, position.z), tangentIn: tin, tangentOut: out, mode: mode };
    this._knots.splice(i, 0, knot);
    this.changed();
    return i;
  }

  removeKnot(index: number): void {
    if (index < 0 || index >= this._knots.length) return;
    this._knots.splice(index, 1);
    this.changed();
  }

  setPosition(index: number, position: Vector3): void {
    if (index < 0 || index >= this._knots.length) return;
    this._knots[index]!.position = new Vector3(position.x, position.y, position.z);
    this.changed();
  }

  /** Sets a knot's handles (relative to it) and makes it a `bezier` knot. */
  setTangents(index: number, tangentIn: Vector3, tangentOut: Vector3): void {
    if (index < 0 || index >= this._knots.length) return;
    const k = this._knots[index]!;
    k.tangentIn = new Vector3(tangentIn.x, tangentIn.y, tangentIn.z);
    k.tangentOut = new Vector3(tangentOut.x, tangentOut.y, tangentOut.z);
    k.mode = "bezier";
    this.changed();
  }

  /** Changes a knot's mode; a knot turning `bezier` keeps the handles it had (auto ones included). */
  setMode(index: number, mode: RefTangentMode): void {
    if (index < 0 || index >= this._knots.length) return;
    const k = this._knots[index]!;
    if (k.mode === mode) return;
    if (mode === "bezier" && k.mode !== "bezier") {
      k.tangentIn = this.handleIn(index);
      k.tangentOut = this.handleOut(index);
    }
    k.mode = mode;
    this.changed();
  }

  /** A knot's handle after it, relative to it, whatever its mode (zero when straight). */
  handleOut(index: number): Vector3 {
    if (index < 0 || index >= this._knots.length) return new Vector3(0, 0, 0);
    const k = this._knots[index]!;
    if (k.mode === "bezier") return k.tangentOut;
    // Straight: a third of the way to the next knot, so the segment keeps an even speed.
    if (k.mode === "linear") return this.neighbour(index, 1).sub(k.position).scale(1 / 3);
    // GDI+ cardinal handles: a third of `tension` times the chord across the knot.
    return this.neighbour(index, 1).sub(this.neighbour(index, -1)).scale(this._tension / 3);
  }

  /** A knot's handle before it, relative to it, whatever its mode. */
  handleIn(index: number): Vector3 {
    if (index < 0 || index >= this._knots.length) return new Vector3(0, 0, 0);
    const k = this._knots[index]!;
    if (k.mode === "bezier") return k.tangentIn;
    if (k.mode === "linear") return this.neighbour(index, -1).sub(k.position).scale(1 / 3);
    return this.handleOut(index).scale(-1);
  }

  /** The knot `step` along from `index` (wrapping when closed, else the end knot itself). */
  private neighbour(index: number, step: number): Vector3 {
    const n = this._knots.length;
    const i = index + step;
    return this._closed ? this._knots[((i % n) + n) % n]!.position : this._knots[Math.max(0, Math.min(n - 1, i))]!.position;
  }

  /** Every segment's four Bezier points as plain numbers (12 each), cached until a change. */
  private controls(): Float64Array {
    if (this.ctrl !== null) return this.ctrl;
    const n = this._knots.length;
    const segments = this.segmentCount;
    const c = new Float64Array(segments * 12);
    for (let i = 0; i < segments; i++) {
      const j = (i + 1) % n;
      const p0 = this._knots[i]!.position;
      const p3 = this._knots[j]!.position;
      const h1 = this.handleOut(i);
      const h2 = this.handleIn(j);
      const o = i * 12;
      c[o] = p0.x;
      c[o + 1] = p0.y;
      c[o + 2] = p0.z;
      c[o + 3] = p0.x + h1.x;
      c[o + 4] = p0.y + h1.y;
      c[o + 5] = p0.z + h1.z;
      c[o + 6] = p3.x + h2.x;
      c[o + 7] = p3.y + h2.y;
      c[o + 8] = p3.z + h2.z;
      c[o + 9] = p3.x;
      c[o + 10] = p3.y;
      c[o + 11] = p3.z;
    }
    this.ctrl = c;
    return c;
  }

  /** Segment `i`'s point at `u` into `out` (x, y, z); no allocation. */
  private pointInto(i: number, u: number, out: Float64Array): void {
    const c = this.controls();
    const o = Math.max(0, Math.min(this.segmentCount - 1, i)) * 12;
    const v = 1 - u;
    const a = v * v * v;
    const b = 3 * v * v * u;
    const d = 3 * v * u * u;
    const e = u * u * u;
    out[0] = a * c[o]! + b * c[o + 3]! + d * c[o + 6]! + e * c[o + 9]!;
    out[1] = a * c[o + 1]! + b * c[o + 4]! + d * c[o + 7]! + e * c[o + 10]!;
    out[2] = a * c[o + 2]! + b * c[o + 5]! + d * c[o + 8]! + e * c[o + 11]!;
  }

  /** A point on segment `i` (0 to `segmentCount` - 1), `u` 0 to 1 by the segment's own parameter. */
  segmentPoint(i: number, u: number): Vector3 {
    if (this.segmentCount === 0) return this._knots.length > 0 ? this._knots[0]!.position : new Vector3(0, 0, 0);
    this.pointInto(i, u, this.scratch);
    return new Vector3(this.scratch[0]!, this.scratch[1]!, this.scratch[2]!);
  }

  private derivativeAxis(o: number, axis: number, u: number): number {
    const c = this.controls();
    const v = 1 - u;
    const a = 3 * v * v;
    const b = 6 * v * u;
    const e = 3 * u * u;
    return a * (c[o + 3 + axis]! - c[o + axis]!) + b * (c[o + 6 + axis]! - c[o + 3 + axis]!) + e * (c[o + 9 + axis]! - c[o + 6 + axis]!);
  }

  /** The direction (not normalized) segment `i` is going at `u`. */
  segmentDerivative(i: number, u: number): Vector3 {
    if (this.segmentCount === 0) return new Vector3(0, 0, 0);
    const c = this.controls();
    const o = Math.max(0, Math.min(this.segmentCount - 1, i)) * 12;
    const x = this.derivativeAxis(o, 0, u);
    const y = this.derivativeAxis(o, 1, u);
    const z = this.derivativeAxis(o, 2, u);
    // Where the handles sit on the knots, the direction comes from the chord.
    if (x * x + y * y + z * z > 1e-18) return new Vector3(x, y, z);
    return new Vector3(c[o + 9]! - c[o]!, c[o + 10]! - c[o + 1]!, c[o + 11]! - c[o + 2]!);
  }

  private lengths(): number[] {
    if (this.table !== null) return this.table;
    const table: number[] = [0];
    let total = 0;
    for (let i = 0; i < this.segmentCount; i++) {
      this.pointInto(i, 0, this.scratch);
      let px = this.scratch[0]!;
      let py = this.scratch[1]!;
      let pz = this.scratch[2]!;
      for (let s = 1; s <= 32; s++) {
        this.pointInto(i, s / 32, this.scratch);
        const dx = this.scratch[0]! - px;
        const dy = this.scratch[1]! - py;
        const dz = this.scratch[2]! - pz;
        total += Math.sqrt(dx * dx + dy * dy + dz * dz);
        table.push(total);
        px = this.scratch[0]!;
        py = this.scratch[1]!;
        pz = this.scratch[2]!;
      }
    }
    this.table = table;
    return table;
  }

  /** The whole length. */
  get length(): number {
    const t = this.lengths();
    return t[t.length - 1]!;
  }

  /** Where `t` (0 to 1 by distance) falls: a segment and its own parameter. */
  locate(t: number): RefHit {
    const segments = this.segmentCount;
    if (segments === 0) return { segment: 0, u: 0 };
    const table = this.lengths();
    const total = table[table.length - 1]!;
    const want = Math.max(0, Math.min(1, t)) * total;
    // The first sample at or past `want` (binary search over the increasing distances).
    let lo = 0;
    let hi = table.length - 1;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (table[mid]! < want) lo = mid + 1;
      else hi = mid;
    }
    const j = Math.max(1, lo);
    const a = table[j - 1]!;
    const b = table[j]!;
    const frac = b > a ? (want - a) / (b - a) : 0;
    const step = j - 1 + frac;
    const segment = Math.min(segments - 1, Math.floor(step / 32));
    return { segment: segment, u: (step - segment * 32) / 32 };
  }

  /** The point `t` of the way along (0 to 1, by distance). */
  evaluate(t: number): Vector3 {
    const h = this.locate(t);
    return this.segmentPoint(h.segment, h.u);
  }

  /** The direction the spline goes at `t` (by distance), normalized. */
  tangent(t: number): Vector3 {
    const h = this.locate(t);
    return this.segmentDerivative(h.segment, h.u).normalized;
  }

  /** Points along the whole spline, `perSegment` to a segment; open ones include the last knot. */
  sample(perSegment: number = 16): Vector3[] {
    const out: Vector3[] = [];
    const segments = this.segmentCount;
    if (segments === 0) {
      for (let i = 0; i < this._knots.length; i++) out.push(this._knots[i]!.position);
      return out;
    }
    for (let i = 0; i < segments; i++) {
      for (let s = 0; s < perSegment; s++) out.push(this.segmentPoint(i, s / perSegment));
    }
    if (!this._closed) out.push(this._knots[this._knots.length - 1]!.position);
    return out;
  }

  private distanceAt(t: number, target: Vector3): number {
    const h = this.locate(t);
    this.pointInto(h.segment, h.u, this.scratch);
    const dx = this.scratch[0]! - target.x;
    const dy = this.scratch[1]! - target.y;
    const dz = this.scratch[2]! - target.z;
    return Math.sqrt(dx * dx + dy * dy + dz * dz);
  }

  /** The nearest point on the spline to `p`: its `t` (by distance), the point and how far. */
  nearest(p: Vector3): RefNearest {
    const target = new Vector3(p.x, p.y, p.z);
    const total = this.length;
    if (this.segmentCount === 0 || total === 0) {
      const point = this._knots.length > 0 ? this._knots[0]!.position : new Vector3(0, 0, 0);
      return { t: 0, point: point, distance: point.distance(target) };
    }
    // Coarse: every table sample; then narrow down around the best.
    const samples = this.segmentCount * 32;
    let best = 0;
    let bestD = Infinity;
    for (let s = 0; s <= samples; s++) {
      const d = this.distanceAt(s / samples, target);
      if (d < bestD) {
        bestD = d;
        best = s / samples;
      }
    }
    let step = 1 / samples;
    for (let k = 0; k < 20; k++) {
      step /= 2;
      for (let side = 0; side < 2; side++) {
        const t = side === 0 ? best - step : best + step;
        const c = this._closed ? ((t % 1) + 1) % 1 : Math.max(0, Math.min(1, t));
        const d = this.distanceAt(c, target);
        if (d < bestD) {
          bestD = d;
          best = c;
        }
      }
    }
    const point = this.evaluate(best);
    return { t: best, point: point, distance: point.distance(target) };
  }

  private changed(): void {
    this.table = null;
    this.ctrl = null;
    this.version++;
  }
}


const spts: Vector3[] = [new Vector3(0, 0, 0), new Vector3(3, 1, 0), new Vector3(6, -2, 1), new Vector3(8, 4, -1), new Vector3(2, 5, 2), new Vector3(-1, 2, 0.5)];

function sameV3(name: string, a: Vector3, b: Vector3): string {
  return same(a.x, b.x) && same(a.y, b.y) && same(a.z, b.z) ? "" : `${name}: (${a.x}, ${a.y}, ${a.z}) expected (${b.x}, ${b.y}, ${b.z})`;
}

function sameNum(name: string, a: number, b: number): string {
  return same(a, b) ? "" : `${name}: ${a} expected ${b}`;
}

/** Everything a spline can tell, native against the TypeScript one, as the first difference ("" when equal). */
function compareSplines(label: string, s: Spline, r: RefSpline): string {
  const out: string[] = [];
  out.push(sameNum(`${label} count`, s.count, r.count));
  out.push(sameNum(`${label} segmentCount`, s.segmentCount, r.segmentCount));
  out.push(sameNum(`${label} version`, s.version, r.version));
  out.push(sameNum(`${label} tension`, s.tension, r.tension));
  out.push(s.closed === r.closed ? "" : `${label} closed`);
  out.push(sameNum(`${label} length`, s.length, r.length));
  const k = s.knots;
  if (k.length !== r.knots.length) return `${label} knots: ${k.length} expected ${r.knots.length}`;
  for (let i = 0; i < k.length; i++) {
    out.push(sameV3(`${label} knot ${i} position`, k[i]!.position, r.knots[i]!.position));
    out.push(sameV3(`${label} knot ${i} tangentIn`, k[i]!.tangentIn, r.knots[i]!.tangentIn));
    out.push(sameV3(`${label} knot ${i} tangentOut`, k[i]!.tangentOut, r.knots[i]!.tangentOut));
    out.push(k[i]!.mode === r.knots[i]!.mode ? "" : `${label} knot ${i} mode ${k[i]!.mode} expected ${r.knots[i]!.mode}`);
  }
  for (const i of [-1, 0, 1, 2, 3, 4, 5, 6]) {
    out.push(sameV3(`${label} handleOut(${i})`, s.handleOut(i), r.handleOut(i)));
    out.push(sameV3(`${label} handleIn(${i})`, s.handleIn(i), r.handleIn(i)));
  }
  for (const i of [-1, 0, 1, 2, 5, 7]) {
    for (const u of [0, 0.25, 0.5, 1, 1.5]) {
      out.push(sameV3(`${label} segmentPoint(${i}, ${u})`, s.segmentPoint(i, u), r.segmentPoint(i, u)));
      out.push(sameV3(`${label} segmentDerivative(${i}, ${u})`, s.segmentDerivative(i, u), r.segmentDerivative(i, u)));
    }
  }
  for (const t of [0, 0.1, 0.25, 1 / 3, 0.5, 0.75, 0.999, 1, -0.5, 2, NaN]) {
    const h = s.locate(t);
    const rh = r.locate(t);
    out.push(sameNum(`${label} locate(${t}).segment`, h.segment, rh.segment));
    out.push(sameNum(`${label} locate(${t}).u`, h.u, rh.u));
    // (A NaN `t` makes NaN points; the compiled typed-array read the old code did gives 0 there.)
    if (t === t) {
      out.push(sameV3(`${label} evaluate(${t})`, s.evaluate(t), r.evaluate(t)));
      out.push(sameV3(`${label} tangent(${t})`, s.tangent(t), r.tangent(t)));
    }
  }
  for (const per of [16, 1, 4, 3.5, 0]) {
    const a = s.sample(per);
    const b = r.sample(per);
    if (a.length !== b.length) return `${label} sample(${per}): ${a.length} expected ${b.length}`;
    for (let i = 0; i < a.length; i++) out.push(sameV3(`${label} sample(${per})[${i}]`, a[i]!, b[i]!));
  }
  out.push(s.sample().length === r.sample().length ? "" : `${label} sample() length`);
  for (const p of [new Vector3(1.5, 2, 0), new Vector3(-5, -5, -5), new Vector3(6, -2, 1), new Vector3(3, 3, 3), new Vector3(100, 0, 0)]) {
    const n = s.nearest(p);
    const rn = r.nearest(p);
    out.push(sameNum(`${label} nearest.t`, n.t, rn.t));
    out.push(sameNum(`${label} nearest.distance`, n.distance, rn.distance));
    out.push(sameV3(`${label} nearest.point`, n.point, rn.point));
  }
  for (let i = 0; i < out.length; i++) {
    if (out[i] !== "") return out[i]!;
  }
  return "";
}

/** Builds splines both ways, edits them the same way, and compares everything after each step. */
export function splineMatch(): string {
  for (let n = 0; n <= 6; n++) {
    const pts = spts.slice(0, n);
    for (const closed of [false, true]) {
      for (const tension of [0.5, 0, 1]) {
        const s = new Spline(pts, { closed, tension });
        const r = new RefSpline(pts, { closed: closed, tension: tension });
        let d = compareSplines(`n=${n} closed=${closed} tension=${tension}`, s, r);
        if (d !== "") return d;
        const lin = new Spline(pts, { closed, tension, mode: "linear" });
        const rlin = new RefSpline(pts, { closed: closed, tension: tension, mode: "linear" });
        d = compareSplines(`linear n=${n} closed=${closed}`, lin, rlin);
        if (d !== "") return d;
      }
    }
  }
  // Edits: every mutator, in an order that mixes the modes.
  const s = new Spline(spts.slice(0, 4));
  const r = new RefSpline(spts.slice(0, 4));
  const steps: string[] = [];
  s.setMode(1, "bezier");
  r.setMode(1, "bezier");
  steps.push(compareSplines("setMode bezier", s, r));
  s.setTangents(2, new Vector3(-1, 0.5, 0), new Vector3(1.5, -0.5, 0.25));
  r.setTangents(2, new Vector3(-1, 0.5, 0), new Vector3(1.5, -0.5, 0.25));
  steps.push(compareSplines("setTangents", s, r));
  const i1 = s.insertKnot(2, new Vector3(4, 4, 4), "linear");
  const j1 = r.insertKnot(2, new Vector3(4, 4, 4), "linear");
  steps.push(i1 === j1 ? "" : `insertKnot index ${i1} expected ${j1}`);
  steps.push(compareSplines("insertKnot", s, r));
  const i2 = s.insertKnot(99, new Vector3(1, 1, 1), "bezier", new Vector3(0, 1, 0), new Vector3(0, -1, 0));
  const j2 = r.insertKnot(99, new Vector3(1, 1, 1), "bezier", new Vector3(0, 1, 0), new Vector3(0, -1, 0));
  steps.push(i2 === j2 ? "" : `insertKnot(99) index ${i2} expected ${j2}`);
  steps.push(compareSplines("insertKnot at end", s, r));
  const i3 = s.insertKnot(-3, new Vector3(9, 9, 9), "auto", new Vector3(1, 2, 3));
  const j3 = r.insertKnot(-3, new Vector3(9, 9, 9), "auto", new Vector3(1, 2, 3));
  steps.push(i3 === j3 ? "" : `insertKnot(-3) index ${i3} expected ${j3}`);
  steps.push(compareSplines("insertKnot at start", s, r));
  s.setPosition(1, new Vector3(5, 5, 5));
  r.setPosition(1, new Vector3(5, 5, 5));
  steps.push(compareSplines("setPosition", s, r));
  s.setMode(1, "linear");
  r.setMode(1, "linear");
  s.setMode(2, "auto");
  r.setMode(2, "auto");
  steps.push(compareSplines("setMode back", s, r));
  s.removeKnot(0);
  r.removeKnot(0);
  s.removeKnot(77);
  r.removeKnot(77);
  steps.push(compareSplines("removeKnot", s, r));
  s.closed = true;
  r.closed = true;
  s.tension = 0.25;
  r.tension = 0.25;
  steps.push(compareSplines("closed, tension", s, r));
  s.addKnot(new Vector3(-4, 0, 0));
  r.addKnot(new Vector3(-4, 0, 0));
  s.setPosition(-1, new Vector3(0, 0, 0));
  r.setPosition(-1, new Vector3(0, 0, 0));
  steps.push(compareSplines("addKnot", s, r));
  // Duplicated points: zero-length segments and a zero total length.
  const dup = new Spline([new Vector3(1, 1, 1), new Vector3(1, 1, 1), new Vector3(1, 1, 1)]);
  const rdup = new RefSpline([new Vector3(1, 1, 1), new Vector3(1, 1, 1), new Vector3(1, 1, 1)]);
  steps.push(compareSplines("duplicates", dup, rdup));
  for (let i = 0; i < steps.length; i++) {
    if (steps[i] !== "") return steps[i]!;
  }
  return "";
}

/** `SplineAnimate.pose` against the old component's `t` and direction, for every loop mode. */
export function animatePoseMatches(): string {
  const s = new Spline(spts.slice(0, 5));
  const r = new RefSpline(spts.slice(0, 5));
  const modes: SplineLoopMode[] = ["loop", "pingPong", "once"];
  for (const mode of modes) {
    for (const duration of [5, 2, 0, -1, 0.3]) {
      for (const elapsed of [0, 0.1, 1, 2.5, 4.99, 5, 7.5, 10, 12.5, 99.9, -1, -6]) {
        // The old component: `t`, then the point and the direction (turned round on the way back).
        const d = Math.max(1e-6, duration);
        const x = elapsed / d;
        let t: number;
        if (mode === "once") t = Math.max(0, Math.min(1, x));
        else if (mode === "pingPong") {
          const m = ((x % 2) + 2) % 2;
          t = m > 1 ? 2 - m : m;
        } else t = ((x % 1) + 1) % 1;
        const at = r.evaluate(t);
        let dir = r.tangent(t);
        if (mode === "pingPong" && (((elapsed / Math.max(1e-6, duration)) % 2) + 2) % 2 > 1) dir = dir.scale(-1);
        const pose = SplineAnimate.pose(s, elapsed, duration, mode);
        const diff = sameV3(`pose ${mode} ${duration} ${elapsed} position`, pose.position, at) + sameV3(`pose ${mode} ${duration} ${elapsed} direction`, pose.direction, dir);
        if (diff !== "") return diff;
      }
    }
  }
  return "";
}

/** A spline made, read back through its snapshot and used as a plain object. */
export function splineSnapshotsAreObjects(): string {
  const s = new Spline([new Vector3(0, 0, 0), new Vector3(2, 0, 0)], { closed: false, tension: 0.25, mode: "linear" });
  const k = s.knots;
  const mode: TangentMode = k[1]!.mode;
  const hit = s.locate(0.5);
  const near = s.nearest(new Vector3(1, 3, 0));
  const parsed = TangentMode.parse("bezier");
  const name = TangentMode.name(2);
  s.setMode(0, mode === "linear" ? "bezier" : "auto");
  return `${k.length}|${mode}|${k[1]!.position.x}|${Math.round(hit.u * 100)}|${hit.segment}|${Math.round(near.distance * 100)}|${parsed}|${name}|${s.knots[0]!.mode}|${TangentMode.parse("nope")}|${s.tension}`;
}

let car: GameObject | null = null;
let anim: SplineAnimate | null = null;

/** A `SplineAnimate` added with options, found again, given a spline; the Rust test then runs the host update. */
export function animateSetup(): string {
  car = new GameObject("car");
  anim = car.addComponent(SplineAnimate, { duration: 2, loopMode: "pingPong", alignment: "none" });
  anim.spline = new Spline([new Vector3(0, 0, 0), new Vector3(4, 0, 0)], { mode: "linear" });
  return `${car.getComponent(SplineAnimate) !== undefined}|${anim.duration}|${anim.loopMode}|${anim.alignment}|${anim.playing}|${anim.container === null}`;
}

export function animateWhere(): string {
  const p = car!.transform.position;
  return `${Math.round(p.x * 1000) / 1000}|${Math.round(anim!.t * 1000) / 1000}|${Math.round(anim!.elapsed * 100) / 100}`;
}

/** What `js/engine`'s `SplineAnimate` test checked: positions on the path, facing, loop modes, a container. */
export function animateBehaviour(): string {
  const path = new Spline([new Vector3(0, 0, 0), new Vector3(4, 0, 0), new Vector3(4, 4, 0)], { mode: "linear" });
  const go = new GameObject("car2");
  const a = go.addComponent(SplineAnimate, { duration: 4 });
  a.spline = path;
  const r = (v: number): number => Math.round(v * 1000) / 1000;
  const at = (): string => {
    const p = go.transform.position;
    return `${r(p.x)},${r(p.y)}`;
  };
  const face = (): string => {
    const d = go.transform.rotation.rotate(Vector3.right);
    return `${r(d.x)},${r(d.y)}`;
  };
  const out: string[] = [];
  a.elapsed = 1;
  a.place();
  out.push(at(), face());
  a.elapsed = 3.5;
  a.place();
  out.push(at(), face());
  a.elapsed = 4.5;
  a.place();
  out.push(at());
  a.loopMode = "pingPong";
  a.elapsed = 5;
  a.place();
  out.push(at());
  a.loopMode = "once";
  a.elapsed = 9;
  a.place();
  out.push(at());
  const container = new GameObject("track", { position: new Vector3(10, 0, 0) });
  a.container = container;
  a.elapsed = 0;
  a.loopMode = "loop";
  a.place();
  out.push(at());
  return out.join("|");
}
