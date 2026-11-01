// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The TypeScript `@platypus/engine` shapes the examples use, over the native API: object-valued
// namespace properties and `new GameObject(name, { .. })` option keys. The Rust test
// (tests/engine_api/facade.rs) reads the same state back.

export function skyAndBloom(): string {
  RenderSettings.skybox = { sunDirection: new Vector3(0.5, 0.45, 0.4), zenith: new Color(1, 0, 0, 1), sunSize: 2 };
  const a = `${RenderSettings.skyboxEnabled}|${RenderSettings.skyZenith.r}|${RenderSettings.skySunSize}|${RenderSettings.skyHasSun}`;
  RenderSettings.skybox = { sunDirection: null };
  const b = `${RenderSettings.skyZenith.r === 1}|${RenderSettings.skyHasSun}`;
  RenderSettings.bloom = { threshold: 1.2, intensity: 0.45, scatter: 0.55 };
  const c = `${RenderSettings.bloomEnabled}|${RenderSettings.bloomThreshold}|${RenderSettings.bloomScatter}`;
  RenderSettings.bloom = null;
  RenderSettings.skybox = null;
  return `${a}|${b}|${c}|${RenderSettings.bloomEnabled}|${RenderSettings.skyboxEnabled}`;
}

export function gameObjectOptions(): string {
  const parent = new GameObject("P", { position: new Vector3(1, 2, 3), scale: new Vector3(2, 2, 2) });
  const child = new GameObject("C", { position: new Vector3(0, 1, 0), parent, layer: 3, active: false });
  const top = new GameObject("T", { parent: null });
  const w = child.transform.position;
  return `${child.parent === parent}|${top.parent === null}|${child.layer}|${child.enabled}|${child.transform.localPosition.y}|${w.x}|${w.y}|${w.z}`;
}

export function cameraAndScreen(): string {
  const cam = new GameObject("Cam").addComponent(Camera, { rect: new Rect(0.25, 0.5, 0.5, 0.25), fieldOfView: 60 });
  const far = cam.layerCullDistances;
  far[3] = 14;
  cam.layerCullDistances = far;
  const main = Camera.main;
  const hfov = cam.horizontalFieldOfView;
  cam.horizontalFieldOfView = hfov;
  const size = Screen.windowSize;
  return `${cam.rect.x}|${cam.rect.height}|${cam.layerCullDistances[3]}|${cam.layerCullDistances[4]}|${main === cam}|${Math.round(cam.fieldOfView)}|${size.width >= 0}|${QualitySettings.names.length > 0}`;
}

export function constructors(): string {
  const a = new Color(0.1, 0.2, 0.3);
  const m = new Material(Color.white, new Color(4, 3, 2, 1), 0.25, true);
  const t = m.withTexture(Texture.create(1, 1, [Color.white]));
  const full = new Material(Color.white, Color.black, 0.5, false, null, new Vector2(2, 3), new Vector2(0.5, 0.25), 0.8);
  const p = GameObject.createPrimitive("sphere", { position: new Vector3(1, 2, 3), scale: Vector3.one.scale(2), name: "Ball" });
  return `${a.a}|${m.smoothness}|${m.unlit}|${m.emission.r}|${t.mainTexture.width}|${p.name}|${p.transform.localScale.x}|${p.transform.position.y}|${full.mainTextureScale.y}|${full.mainTextureOffset.x}|${full.metallic}|${full.smoothness}`;
}

export function structured(): string {
  const go = new GameObject("g");
  const r1 = go.addComponent(MeshRenderer, { mesh: "cube" });
  const lod = go.addComponent(LODGroup, { size: 3 });
  lod.setLODs([{ screenRelativeTransitionHeight: 0.5, renderers: [r1] }, { screenRelativeTransitionHeight: 0.1, renderers: [] }]);
  const ball = new GameObject("ball");
  const cloth = go.addComponent(Cloth, {
    pins: [new Vector3(0, 1, 0), new Vector3(1, 1, 0)],
    sphereColliders: [ball],
    pinBox: { min: new Vector3(-1, -1, -1), max: new Vector3(1, 1, 1) },
  });
  const roots = SceneManager.getActiveScene().getRootGameObjects();
  return `${lod.lodCount}|${lod.lodRendererCount(0)}|${lod.lodRendererCount(1)}|${roots.length === SceneManager.getActiveScene().rootCount}|${cloth.rebuildRequests >= 0}`;
}

export function animatorController(): string {
  const clip = new AnimationClip("idle");
  const controller = new AnimatorController()
    .addParameter("speed", "float", 0)
    .addParameter("jump", "trigger")
    .addState("Locomotion", { blendParameter: "speed", children: [{ motion: clip, threshold: 0 }, { motion: clip, threshold: 1 }] }, { speed: 2 })
    .addState("Jump", clip)
    .addTransition({ from: "Locomotion", to: "Jump", conditions: [{ parameter: "jump", mode: "if" }], duration: 0.1 })
    .addTransition({ from: "Jump", to: "Locomotion", hasExitTime: true, exitTime: 0.95, duration: 0.15 });
  // The handle forms of the same names still work next to the descriptions.
  controller.addParameter(AnimationParameter.create("grounded", "bool", 1));
  const animator = new GameObject("a").addComponent(Animator, { controller });
  const blend = controller.nodeAt(0);
  return `${controller.parameterCount}|${controller.nodeCount}|${controller.transitionCount}|${animator.getCurrentAnimatorStateInfo().name}|${blend.speed}|${blend.blendTree.childCount}|${controller.transitionAt(0).conditionCount}|${controller.transitionAt(1).exitTime}|${controller.parameterAt(2).defaultValue}`;
}

export function sprites(): string {
  const tex = Texture.create(4, 2, [Color.white, Color.white, Color.white, Color.white, Color.white, Color.white, Color.white, Color.white]);
  const s = Sprite.create(tex, new Rect(0, 0, 2, 2), new Vector2(0.5, 0.5), 40);
  const grid = Sprite.grid(tex, 2, 1, undefined, 10);
  const whole = Sprite.create(tex, null, undefined, 25);
  const plain = Sprite.create(tex, 50);
  return `${s.pixelsPerUnit}|${s.hasRect}|${grid.length}|${grid[1]!.pixelsPerUnit}|${whole.hasRect}|${whole.pixelsPerUnit}|${plain.pixelsPerUnit}|${plain.hasRect}`;
}

export function uiResources(): string {
  const a = addStyleSheet(".a { color: red }");
  loadFont({ family: "Charm", src: "fonts/x.ttf", weight: 700 });
  removeStyleSheet(a);
  const plain = { family: "Plain", src: "fonts/p.ttf" };
  loadFont(plain);
  const update = () => {};
  const stop = input.on("gamepadconnected", update);
  stop();
  return `${a}|${input.gamepads().length}`;
}

export function uiScale(): string {
  UiScale.height(1080);
  UiScale.fit(1920, 1080);
  UiScale.fixed(0);
  UiScale.fixed(1.5);
  return "set";
}

export function uiInspector(): string {
  UiInspector.select(7);
  UiInspector.expand(7, true);
  UiInspector.setStyle(7, "width", "200px");
  const id = UiInspector.hovered();
  const rule = UiInspector.ruleSelector(id, 0);
  const applied = UiInspector.declApplied(id, 0, 0);
  return `${UiInspector.enabled()}|${id}|${UiInspector.kind(id)}|${UiInspector.childAt(id, 1)}|${rule}|${applied}|${UiInspector.inlineValue(id, 0)}`;
}

export function particlesAndPoints(): string {
  const go = new GameObject("p");
  const ps = go.addComponent(ParticleSystem, {
    startLifetime: [1.2, 1.8],
    startSpeed: 3,
    startColor: [new Color(1, 0, 0, 1), new Color(0, 0, 1, 1)],
    bursts: [{ time: 0, count: 40, cycles: 0, interval: 2 }, { time: 1, count: 5 }],
    shape: { type: "cone", angle: 14, radius: 0.08 },
    colorOverLifetime: [{ time: 0, color: Color.white }, { time: 1, color: new Color(1, 1, 1, 0) }],
    sizeOverLifetime: [{ time: 0, value: 1 }, { time: 1, value: 0 }],
  });
  const edge = go.addComponent(EdgeCollider2D, { points: [new Vector2(0, 0), new Vector2(1, 1), new Vector2(2, 1)] });
  const line = new GameObject("l").addComponent(LineRenderer, { positions: [Vector3.zero, Vector3.one], width: 0.1 });
  const sprite = new GameObject("s").addComponent(SpriteRenderer, { sprite: null });
  // A number or a list of two, by assignment too.
  ps.startSize = [2, 5];
  ps.angularVelocity = 9;
  ps.startColor = [new Color(0, 0, 1, 1), new Color(0, 1, 0, 1)];
  return `${ps.startLifetime}|${ps.startLifetimeMax}|${ps.startSpeed}|${ps.burstCount}|${ps.shapeType}|${ps.shapeAngle}|${edge.pointCount}|${line.positionCount}|${sprite.sprite === null}|${ps.startSize}|${ps.startSizeMax}|${ps.angularVelocity}|${ps.angularVelocityMax}|${ps.startColorMax.g}`;
}

// ---- round 2: the shapes the remaining examples use (M9-TS-2000..) ----

export function curveFromPairs(): string {
  const pts: number[][] = [];
  for (let i = 0; i <= 4; i++) pts.push([i, i * 2]);
  const c = AnimationCurve.fromPoints(pts, "linear");
  const d = AnimationCurve.fromPoints([[0, 0], [2, 4]]);
  return `${c.length}|${c.evaluate(1.5)}|${d.evaluate(1)}`;
}

export function unsubscribers(): string {
  let calls = 0;
  const off = QualitySettings.onLevelChanged(() => {
    calls++;
  });
  QualitySettings.setQualityLevel(0);
  const first = calls;
  off();
  QualitySettings.setQualityLevel(QualitySettings.names.length - 1);
  QualitySettings.setQualityLevel(0);
  const modes = Screen.supportedFullScreenModes;
  const names: Record<FullScreenMode, string> = { ExclusiveFullScreen: "x", FullScreenWindow: "b", MaximizedWindow: "m", Windowed: "w" };
  const label = modes.length > 0 ? names[modes[0]!] : "none";
  return `${typeof off}|${first >= 0}|${calls === first}|${label.length}`;
}

export function bareReturnThenCleanup(): string {
  const effect = (skip: boolean) => {
    if (skip) return;
    return () => 7;
  };
  const a = effect(true);
  const b = effect(false);
  return `${a === null}|${b !== null}|${b ? b() : 0}`;
}

export function tuples(): string {
  const rows = [
    [0, -2.5, 4, false],
    [1, 2.5, 8, true],
  ] as const;
  let out = "";
  for (const [x, z, length, turn] of rows) out += `${x}:${z}:${length}:${turn ? "T" : "F"},`;
  const pair = [3, "three"];
  return `${out}${rows[1][2]}|${pair[1]}`;
}

export function defaultedClosureParams(): string {
  const row = (x: number, name: string, scale = 0.75, tag = "t") => `${x}${name}${scale}${tag}`;
  return `${row(1, "a")}|${row(2, "b", 2)}|${row(3, "c", 3, "z")}`;
}

export function voidAndExtraArguments(): string {
  let n = 0;
  const bump = () => {
    n++;
  };
  const f = () => void bump();
  f();
  f();
  const g = (a: number) => a + 1;
  return `${n}|${g(1)}`;
}

export function recordByEngineString(): string {
  const names: Record<"disable" | "hardOnly" | "all", string> = { disable: "off", hardOnly: "hard", all: "soft" };
  return names[QualitySettings.shadows as "all"];
}

export function plainPointsToValues(): string {
  const mk = (n: number): { x: number; y: number }[] => Array.from({ length: n }, (_, i) => ({ x: i, y: i * 2 }));
  const go = new GameObject("e");
  const edge = go.addComponent(EdgeCollider2D);
  edge.points = mk(4);
  const a = edge.pointCount;
  edge.points = a > 3 ? mk(2) : [];
  return `${a}|${edge.pointCount}`;
}

class Box1 {
  v = 1;
  step(d: number): void {
    this.v += d;
  }
}
class Box2 {
  s = "a";
  step(d: number): void {
    this.s += d;
  }
}

export function unrolledMixedLoop(): string {
  const a = new Box1();
  const b = new Box2();
  let seen = 0;
  for (const t of [a, b]) {
    t.step(2);
    seen++;
  }
  return `${a.v}|${b.s}|${seen}`;
}

interface Prof {
  name: string;
  age: number;
}

export function keyedSetter(): string {
  let p: Prof = { name: "a", age: 1 };
  const set = <K extends keyof Prof>(key: K, value: Prof[K]) => {
    p = { ...p, [key]: value };
  };
  set("name", "b");
  set("age", 5);
  return `${p.name}|${p.age}`;
}

export function fogAndExposureObjects(): string {
  RenderSettings.heightFog = { density: 0.2, baseHeight: 3, falloff: 0.5 };
  const a = `${RenderSettings.heightFogEnabled}|${RenderSettings.heightFogBaseHeight}|${RenderSettings.heightFogFalloff}`;
  RenderSettings.heightFog = null;
  RenderSettings.volumetricFog = { intensity: 2, albedo: new Color(1, 0, 0, 1), resolution: "ultra" };
  const b = `${RenderSettings.volumetricFogEnabled}|${RenderSettings.volumetricFogIntensity}|${RenderSettings.volumetricFogAnisotropy}|${RenderSettings.volumetricFogAlbedo.g}|${RenderSettings.volumetricFogResolution}`;
  const on = true;
  RenderSettings.autoExposure = on ? { minEV: -3, compensation: 0.5, curve: [[-2, -1], [6, 0]] } : null;
  const c = `${RenderSettings.autoExposureEnabled}|${RenderSettings.autoExposureMinEv}|${RenderSettings.autoExposureMaxEv}`;
  RenderSettings.fixedExposure = 9;
  const d = `${RenderSettings.fixedExposure}`;
  RenderSettings.fixedExposure = null;
  const e = `${RenderSettings.fixedExposure}|${RenderSettings.fixedExposure !== null}`;
  RenderSettings.autoExposure = null;
  RenderSettings.volumetricFog = null;
  return `${a}|${b}|${c}|${d}|${e}|${RenderSettings.autoExposureEnabled}|${RenderSettings.volumetricFogEnabled}|${RenderSettings.heightFogEnabled}`;
}

export function flatMeshesAndPicks(): string {
  const m = Mesh.createFlat({
    positions: new Float32Array([0, 0, 0, 1, 0, 0, 0, 1, 0]),
    triangles: new Uint32Array([0, 1, 2]),
    normals: [],
    uv: new Float32Array([0, 0, 1, 0, 0, 1]),
  });
  const a = `${m.vertexCount}|${m.triangleCount}`;
  m.setFlat({ positions: [0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0], triangles: [0, 1, 2, 2, 1, 3], normals: [], uv: [] });
  const items = [4, 5, 6];
  const p = Random.pick(items);
  const none: number[] = [];
  return `${a}|${m.vertexCount}|${m.triangleCount}|${p !== undefined && p >= 4 && p <= 6}|${Random.pick(none) === undefined}`;
}

export function picksOfEveryKind(): string {
  const word = Random.pick(["a", "b"]);
  const flag = Random.pick([true, true]);
  const box = Random.pick([{ n: 7 }]);
  const noBox: { n: number }[] = [];
  const noFlag: boolean[] = [];
  return `${word === "a" || word === "b"}|${flag}|${box !== undefined ? box.n : -1}|${Random.pick(noBox) === undefined}|${Random.pick(noFlag) === undefined}`;
}

export function lightMeter(): string {
  const sun = new GameObject("Sun").addComponent(Light, { type: "directional", intensity: 3 });
  const lamp = new GameObject("Lamp", { position: new Vector3(0, 2, 0) }).addComponent(Light, { type: "point", intensity: 4 * Math.PI * 4, range: 20 });
  const total = Light.illuminanceAt(new Vector3(0, 0, 0));
  return `${Light.count()}|${sun.volumetricIntensity}|${sun.spotShape}|${Math.round(total * 100) / 100}|${lamp.aspectRatio}`;
}

function spawn(name: string, at: Vector3, rotation?: Quaternion): GameObject {
  return new GameObject(name, { position: at, rotation });
}

function paint(props: { on?: boolean }): string {
  return props.on === false ? "dim" : props.on !== false ? "lit" : "?";
}

export function optionalValuesPassedOn(): string {
  const a = spawn("a", new Vector3(1, 2, 3));
  const b = spawn("b", new Vector3(0, 0, 0), Quaternion.euler(0, 90, 0));
  return `${a.transform.localPosition.y}|${Math.round(a.transform.localRotation.w * 100)}|${Math.round(b.transform.localRotation.w * 100)}|${paint({})}|${paint({ on: false })}|${paint({ on: true })}`;
}

export function bloomFromAVariable(): string {
  const b: BloomSettings | null = { threshold: 1.5, intensity: 0.4, scatter: 0.6 };
  RenderSettings.bloom = b;
  const a = `${RenderSettings.bloomEnabled}|${RenderSettings.bloomThreshold}|${RenderSettings.bloomScatter}`;
  const off: BloomSettings | null = null;
  RenderSettings.bloom = off;
  return `${a}|${RenderSettings.bloomEnabled}`;
}


export function shapesAndSplines(): string {
  const r = Shapes.rectangle(1, 2, 3, 4);
  const c = Shapes.circle(new Vector2(0, 0), 2, 8);
  const arc = Shapes.arc(new Vector2(0, 0), 1, 1, 0, 90, 2);
  const pie = Shapes.pie(new Vector2(0, 0), 1, 1, 0, 90, 2);
  const poly = Shapes.polygon(new Vector2(0, 0), new Vector2(1, 0), new Vector2(1, 1));
  const rounded = Shapes.roundedRectangle(0, 0, 4, 2, 5, 4);
  const curve = Shapes.curve([new Vector2(0, 0), new Vector2(1, 1), new Vector2(2, 0)], 0.5, 4);
  const closed = Shapes.closedCurve([new Vector2(0, 0), new Vector2(1, 1), new Vector2(2, 0)], 0.5, 4);
  return `${r.length}|${r[2]!.x}|${r[2]!.y}|${c.length}|${Math.round(c[2]!.y)}|${arc.length}|${Math.round(arc[2]!.y)}|${pie.length}|${poly.length}|${rounded.length}|${curve.length}|${closed.length}`;
}

export function splineMath(): string {
  const s = new Spline([new Vector3(0, 0, 0), new Vector3(3, 0, 0), new Vector3(6, 0, 0)]);
  const mid = s.evaluate(0.5);
  const near = s.nearest(new Vector3(1.5, 2, 0));
  const loop = new Spline([new Vector3(0, 0, 0), new Vector3(4, 0, 0), new Vector3(4, 4, 0), new Vector3(0, 4, 0)], { closed: true });
  const before = s.version;
  s.setPosition(2, new Vector3(9, 0, 0));
  const knot = s.insertKnot(1, new Vector3(1, 1, 0));
  s.setMode(knot, "linear");
  s.removeKnot(0);
  const tan = s.tangent(0).normalized;
  return `${Math.round(s.length * 1000) / 1000 > 8}|${Math.round(mid.x * 100) / 100}|${Math.round(near.t * 100) / 100}|${Math.round(near.distance * 100) / 100}|${s.segmentCount}|${loop.segmentCount}|${Math.round(loop.length)}|${s.version > before}|${s.knots[0]!.mode}|${Math.round(tan.x * 100) / 100}|${loop.sample(4).length}|${new Spline([new Vector3(0, 0, 0), new Vector3(2, 0, 0)], { mode: "linear" }).sample(2).length}`;
}

export function fillColorAndTextMesh(): string {
  const line = new GameObject("l").addComponent(LineRenderer2D, { fillColor: new Color(1, 0, 0, 1), sortingOrder: 3 });
  const filled = line.fillColor !== null;
  const red = line.fillColor !== null ? line.fillColor.r : -1;
  line.fillColor = null;
  const t = new GameObject("t").addComponent(TextMesh, { text: "x", fontSize: 20, alignment: "left" });
  return `${filled}|${red}|${line.fillColor === null}|${line.fillColor !== null}|${line.sortingOrder}|${t.text}|${t.fontSize}|${t.alignment}`;
}

export function screenToWorld(): string {
  const ortho = new GameObject("o", { position: new Vector3(0, 0, 10) }).addComponent(Camera, { orthographic: true, orthographicSize: 5 });
  const a = ortho.screenToWorldPoint(new Vector3(400, 300, 10));
  const b = ortho.screenToWorldPoint(new Vector3(0, 0, 10));
  const persp = new GameObject("p", { position: new Vector3(0, 0, 10) }).addComponent(Camera, { fieldOfView: 60 });
  const c = persp.screenToWorldPoint({ x: 800, y: 600, z: 10 });
  const r = (v: number) => Math.round(v * 100) / 100;
  return `${r(a.x)}|${r(a.y)}|${r(a.z)}|${r(b.x)}|${r(b.y)}|${r(b.z)}|${r(c.x)}|${r(c.y)}|${r(c.z)}`;
}

function shapeWith(name: string, look: Partial<LineRenderer2D>, at: Vector3 = Vector3.zero): LineRenderer2D {
  return new GameObject(name, { position: at }).addComponent(LineRenderer2D, { width: 0.05, ...look });
}

function labelWith(look: Partial<TextMesh> = {}): TextMesh {
  return new GameObject("lab").addComponent(TextMesh, { fontSize: 24, text: "t", ...look });
}

export function optionBags(): string {
  const plain = shapeWith("a", {});
  const wide = shapeWith("b", { width: 0.5, loop: true, fillColor: new Color(0, 1, 0, 1), sortingOrder: 2 });
  const look: Partial<LineRenderer2D> = { width: 0.25, sortingOrder: 7 };
  const viaVariable = new GameObject("c").addComponent(LineRenderer2D, look);
  const withDefault = new GameObject("d").addComponent(LineRenderer2D, { width: 3, loop: true, ...look });
  const big = labelWith({ fontSize: 40 });
  const dflt = labelWith();
  const r = (v: number) => Math.round(v * 100) / 100;
  return `${r(plain.width)}|${plain.loop}|${r(wide.width)}|${wide.loop}|${wide.sortingOrder}|${wide.fillColor !== null}|${r(viaVariable.width)}|${viaVariable.sortingOrder}|${r(withDefault.width)}|${withDefault.loop}|${withDefault.sortingOrder}|${big.fontSize}|${big.text}|${dflt.fontSize}`;
}

export function engineStaticsAsValues(): string {
  const hex = Color.hex;
  const red = hex("#ff0000");
  const greens = ["#00ff00", "#008000"].map(Color.hex);
  const lerp = Mathf.lerp;
  const clamp = Mathf.clamp;
  return `${red.r}|${red.g}|${greens.length}|${greens[0]!.g}|${Math.round(greens[1]!.g * 100)}|${lerp(2, 6, 0.5)},${clamp(9, 0, 4)}`;
}

export function conditionalOptions(): string {
  const root = new GameObject("root", { position: new Vector3(10, 0, 0) });
  const make = (inside: boolean) => new GameObject("c", inside ? { position: new Vector3(1, 2, 3), parent: root } : { position: new Vector3(4, 5, 6) });
  const a = make(true);
  const b = make(false);
  return `${a.parent !== null}|${b.parent === null}|${b.transform.position.x}|${a.transform.localPosition.y}`;
}

export function enumsAndNullables(): string {
  RenderSettings.fogMode = "linear";
  const a: FogMode = RenderSettings.fogMode;
  const dynamicMode: string = "exponential";
  RenderSettings.fogMode = dynamicMode;
  const b = RenderSettings.fogMode;
  RenderSettings.fogMode = "nonsense";
  const b2 = RenderSettings.fogMode;
  RenderSettings.volumetricFogResolution = "ultra";
  const c: VolumeDetail = RenderSettings.volumetricFogResolution;
  const tex = Texture.create(1, 1, [Color.white], { filterMode: "point", wrapMode: "mirror" });
  const d = `${tex.filterMode}|${tex.wrapMode}`;
  tex.filterMode = "trilinear";
  const plain = Texture.create(1, 1, [Color.white]);
  const e = `${tex.filterMode}|${plain.filterMode}|${plain.wrapMode}`;
  const modes = Screen.supportedFullScreenModes;
  const numbers: Record<FullScreenMode, number> = { ExclusiveFullScreen: 1, FullScreenWindow: 2, MaximizedWindow: 3, Windowed: 4 };
  const f = `${modes.length}|${numbers[modes[0]!]}|${Screen.fullScreenMode}`;
  const cube = GameObject.createPrimitive(PrimitiveType.Cube);
  const mesh: PrimitiveMesh = "capsule";
  const capsule = GameObject.createPrimitive(mesh);
  const g = `${Camera.main === undefined}|${cube.name}|${capsule.name}`;
  RenderSettings.fixedExposure = 4.5;
  const h = RenderSettings.fixedExposure;
  RenderSettings.fixedExposure = null;
  const i = RenderSettings.fixedExposure;
  const line = new GameObject("l").addComponent(LineRenderer2D);
  const j = line.fillColor === null;
  line.fillColor = new Color(0, 1, 0, 1);
  const k = line.fillColor !== null ? line.fillColor.g : -1;
  const ctl: AnimatorController = AnimationController.create();
  return `${a}|${b}|${b2}|${c}|${d}|${e}|${f}|${g}|${h}|${i}|${j}|${k}|${ctl !== null}`;
}

export function resultRecords(): string {
  const size = Screen.windowSize;
  const cur = Screen.currentResolution;
  const all = Screen.resolutions;
  const animator = new GameObject("a").addComponent(Animator);
  const info = animator.getCurrentAnimatorStateInfo();
  const third: string = info.name;
  return `${size.width}x${size.height}|${cur.refreshRate}|${all.length}|${all[1]!.width}|${third === ""}|${info.length}`;
}

export function recordProperties(): string {
  RenderSettings.heightFog = { density: 0.2, baseHeight: 3, falloff: 0.5 };
  const h = RenderSettings.heightFog;
  const a = h === null ? "none" : `${h.density}|${h.baseHeight}`;
  const own: { density: number; baseHeight: number; falloff: number } = { density: 0.7, baseHeight: 1, falloff: 0.1 };
  RenderSettings.heightFog = own;
  const b = RenderSettings.heightFog !== null ? RenderSettings.heightFog.density : -1;
  RenderSettings.heightFog = null;
  const c = RenderSettings.heightFog === null;
  RenderSettings.skybox = { zenith: new Color(1, 0, 0, 1) };
  const sky = RenderSettings.skybox;
  const d = sky === null ? "none" : `${sky.sunDirection !== null}|${sky.zenith !== null ? sky.zenith.r : -1}`;
  RenderSettings.skybox = { sunDirection: null };
  const sky2 = RenderSettings.skybox;
  const e = sky2 !== null && sky2.sunDirection === null;
  const night = true;
  RenderSettings.skybox = { sunDirection: night ? null : new Vector3(0, 1, 0) };
  const e2 = RenderSettings.skyHasSun;
  RenderSettings.autoExposure = night ? { minEV: -4, curve: [[-2, -1], [6, 0]], metering: "spot" } : null;
  const ae = RenderSettings.autoExposure;
  const f = ae === null ? "none" : `${ae.minEV}|${ae.curve !== null ? ae.curve.length : -1}|${ae.metering}|${ae.maxEV}`;
  RenderSettings.skybox = null;
  RenderSettings.autoExposure = null;
  return `${a}|${b}|${c}|${d}|${e}|${e2}|${f}|${RenderSettings.skybox === null}`;
}

export function nativeLists(): string {
  const root = new GameObject("root");
  const b1 = new GameObject("b1", { parent: root });
  const b2 = new GameObject("b2");
  const skin = root.addComponent(SkinnedMeshRenderer);
  skin.bones = [b1, b2];
  const bones = skin.bones;
  skin.bones = [b2];
  const cloth = root.addComponent(Cloth);
  cloth.sphereColliders = [b1];
  cloth.sphereColliders = [b2, b1];
  cloth.pins = [new Vector3(1, 2, 3)];
  cloth.pinBox = { min: new Vector3(0, 0, 0), max: new Vector3(1, 2, 3) };
  const box = cloth.pinBox;
  const had = box !== null;
  cloth.pinBox = null;
  const cam = root.addComponent(Camera);
  cam.layerCullDistances = [1, 2, -3];
  const d = cam.layerCullDistances;
  const roots = SceneManager.getActiveScene().getRootGameObjects();
  const names = QualitySettings.names;
  const line = root.addComponent(LineRenderer2D);
  line.points = [new Vector2(1, 2), new Vector3(3, 4, 5)];
  return `${bones.length}|${bones[1] === b2}|${skin.bones.length}|${cloth.sphereColliders.length}|${cloth.sphereColliders[0] === b2}|${cloth.pins[0]!.z}|${had}|${box!.max.z}|${cloth.pinBox === null}|${d.length}|${d[1]}|${d[2]}|${roots.length > 0}|${names.length > 0}|${line.points[1]!.x}`;
}

export function unsubscribes(): string {
  let screens = 0;
  let levels = 0;
  let loads = 0;
  const offScreen = Screen.onChange(() => {
    screens += 1;
  });
  const offLevel = QualitySettings.onLevelChanged((level: number) => {
    levels += level + 1;
  });
  const offLoad = SceneManager.onSceneLoaded((scene: Scene, mode: LoadSceneMode) => {
    loads += mode === "additive" ? 10 : 1;
  });
  QualitySettings.setQualityLevel(1);
  offLevel();
  offLevel();
  QualitySettings.setQualityLevel(2);
  offScreen();
  offLoad();
  return `${screens}|${levels}|${loads}`;
}

export function recordLists(): string {
  const go = new GameObject("g");
  const r1 = go.addComponent(MeshRenderer, { mesh: "cube" });
  const r2 = go.addComponent(MeshRenderer, { mesh: "sphere" });
  const lod = go.addComponent(LODGroup);
  const same = lod.setLODs([
    { screenRelativeTransitionHeight: 0.6, renderers: [r1, r2] },
    { screenRelativeTransitionHeight: 0.2, renderers: [r2] },
    { screenRelativeTransitionHeight: 0.9, renderers: [r1] },
  ]);
  const ps = go.addComponent(ParticleSystem);
  ps.bursts = [{ time: 0, count: 5 }, { time: 1, count: 2, cycles: 3, interval: 0.5 }];
  ps.colorOverLifetime = [{ time: 0, color: new Color(1, 0, 0, 1) }, { time: 1, color: Color.white }];
  ps.sizeOverLifetime = [{ time: 0, value: 1 }, { time: 1, value: 0.5 }];
  ps.shape = { type: "box", size: new Vector3(1, 2, 3) };
  const b = ps.bursts;
  const c = ps.colorOverLifetime;
  const s = ps.shape;
  return `${lod.lodCount}|${lod.lodRendererCount(0)}|${lod.lodRendererCount(1)}|${same.lodCount}|${b.length}|${b[0]!.cycles}|${b[1]!.interval}|${c[0]!.color.r}|${ps.sizeOverLifetime[1]!.value}|${s!.type}|${s!.size!.y}|${s!.angle}`;
}

export function cameraMaths(): string {
  const go = new GameObject("c");
  go.transform.position = new Vector3(1, 2, 3);
  const cam = go.addComponent(Camera, { fieldOfView: 90 });
  const mid = cam.screenToWorldPoint({ x: Screen.width / 2, y: Screen.height / 2, z: 5 });
  const corner = cam.screenToWorldPoint(new Vector3(0, 0, 5));
  const h = cam.horizontalFieldOfView;
  cam.horizontalFieldOfView = 90;
  const r = Math.round;
  return `${r(mid.x * 1000)}|${r(mid.y * 1000)}|${r(mid.z * 1000)}|${r(corner.x * 1000)}|${r(corner.y * 1000)}|${r(h)}|${r(cam.fieldOfView)}|${r(cam.rect.width * 100)}`;
}

export function skinnedMesh(): string {
  const boneWeights: { indices: [number, number]; weights: [number, number] }[] = [
    { indices: [0, 1], weights: [0.75, 0.25] },
    { indices: [1, 0], weights: [1, 0] },
    { indices: [0, 0], weights: [1, 0] },
  ];
  const bindposes = [Matrix4x4.identity, Matrix4x4.trs(new Vector3(0, -1, 0), Quaternion.identity, Vector3.one)];
  const data = {
    vertices: [new Vector3(0, 0, 0), new Vector3(1, 0, 0), new Vector3(0, 1, 0)],
    triangles: [0, 1, 2],
    boneWeights,
    bindposes,
  };
  const m = Mesh.create(data);
  return `${m.vertexCount}|${m.triangleCount}`;
}

export function sceneSummary(): string {
  new GameObject("alpha");
  new GameObject("beta");
  const scene = SceneManager.getActiveScene();
  const d = scene.serialize();
  let names = "";
  for (let i = 0; i < d.objects.length; i++) names += d.objects[i]!.name + ",";
  return `${d.objects.length}|${names}|${scene.toJson().includes("\"alpha\"")}|${d.name === scene.name}`;
}

/** What `examples/fog-white-room` needed: Like options, flag defaults, finds over handles. */
export function whiteRoomShapes(): string {
  const m = Material.create({ mainTextureScale: { x: 4, y: 2 }, color: new Color(0.9, 0.9, 0.9, 1) });
  const controller = new AnimatorController().addParameter("grounded", "bool", true).addParameter("speed", "float", 0);
  const kids = [new GameObject("A"), new GameObject("B"), new GameObject("C")];
  const hit = kids.find((g) => g.name === "B");
  const miss = kids.find((g) => g.name === "Z");
  const longest = kids.reduce((a, b) => (b.name > a.name ? b : a));
  const go = new GameObject("animated");
  go.addComponent(Animation);
  go.getComponent(Animation)?.destroy();
  const model = Model.load("assets/none.glb");
  const [first] = model.materials;
  const clip = model.animations.find((a) => a.name === "Walk");
  return `${m.mainTextureScale.x}|${m.mainTextureScale.y}|${controller.parameterAt(0).defaultValue}|${hit?.name}|${miss === undefined}|${longest.name}|${go.getComponent(Animation) === undefined}|${first === undefined}|${clip === undefined}|${model.animations.length}`;
}
