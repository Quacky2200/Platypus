// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The arena: a small clinical-white town (houses, parked cars, a street) with red lasers
// across it, standing in white fog. The fog is what makes it look endless: the playable square is
// closed by invisible walls a little inside where the fog is solid. Server and clients both build
// it (colliders matter on both: clients predict their own movement); only the server simulates the
// dynamic props.

/** Half the town's square, metres: where the streets and the outermost houses end. */
export const HALF = 30;
/** Where the invisible walls stand: well clear of the houses (the outer ones end at HALF). */
export const BOUND = HALF + 8;
/** Fog: clear around the player, solid before the walls. */
// Close, Tomb Raider style: the street fades into white a few strides away, so every
// house and car looms out of the mist.
export const FOG_START = 3;
export const FOG_END = 22;

export const WHITE = new Color(0.93, 0.94, 0.96, 1);

const mat = (v: number, extra: { smoothness?: number } = {}) =>
  Material.create({ color: new Color(v, v, v * 1.015, 1), smoothness: extra.smoothness ?? 0.5 });

const MATERIALS = {
  wall: mat(0.95),
  roof: mat(0.82),
  trim: mat(0.7),
  door: mat(0.88),
  ground: mat(0.9, { smoothness: 0.35 }),
  street: mat(0.74, { smoothness: 0.3 }),
  lane: mat(0.97),
  glass: Material.create({ color: new Color(0.78, 0.86, 0.95, 1), emission: new Color(0.12, 0.16, 0.22, 1), smoothness: 0.9 }),
  tyre: mat(0.3),
  car: mat(0.96, { smoothness: 0.8 }),
  crate: mat(0.86),
  laser: Material.create({ color: Color.black, emission: new Color(4, 0.12, 0.1, 1), unlit: true }),
  post: mat(0.6),
  lamp: Material.create({ color: Color.black, emission: new Color(3.2, 4.6, 7, 1), unlit: true }),
};

function box(name: string, at: Vector3, size: Vector3, material: Material, parent?: GameObject, collider = false) {
  const go = new GameObject(name, { position: at, scale: size, parent });
  const r = go.addComponent(MeshRenderer, { mesh: "cube", material });
  r.shadowCastingMode = "on";
  if (collider) go.addComponent(BoxCollider);
  return go;
}

/** A white house `w` wide (x), `d` deep (z), `h` tall, its door towards +z (rotated by `yaw` degrees). */
function house(at: Vector3, w: number, d: number, h: number, yaw: number, sign?: Color) {
  const root = new GameObject("House", { position: at, rotation: Quaternion.euler(0, yaw, 0) });
  box("Walls", new Vector3(0, h / 2, 0), new Vector3(w, h, d), MATERIALS.wall, root, true);
  box("Roof", new Vector3(0, h + 0.2, 0), new Vector3(w + 0.6, 0.4, d + 0.6), MATERIALS.roof, root);
  box("Plinth", new Vector3(0, 0.15, 0), new Vector3(w + 0.3, 0.3, d + 0.3), MATERIALS.trim, root);
  box("Door", new Vector3(0, 1.1, d / 2 + 0.02), new Vector3(1.1, 2.2, 0.08), MATERIALS.door, root);
  for (const x of [-w / 3, w / 3]) {
    box("Window", new Vector3(x, h * 0.6, d / 2 + 0.02), new Vector3(1.4, 1.2, 0.08), MATERIALS.glass, root);
  }
  box("Side window", new Vector3(w / 2 + 0.02, h * 0.6, 0), new Vector3(0.08, 1.2, 1.6), MATERIALS.glass, root);
  // A neon sign over the door.
  if (sign) neon("Neon sign", new Vector3(0, h - 0.75, d / 2 + 0.08), Math.min(w - 2, 2.8), 0.5, sign, root);
  return root;
}

/** A white car along x (rotated by `yaw`), with a solid collider and wheels. */
export function car(at: Vector3, yaw: number, physics: boolean): GameObject {
  const root = new GameObject("Car", { position: at, rotation: Quaternion.euler(0, yaw, 0) });
  box("Body", new Vector3(0, 0.65, 0), new Vector3(4.2, 0.7, 1.8), MATERIALS.car, root);
  box("Cabin", new Vector3(-0.2, 1.2, 0), new Vector3(2.2, 0.6, 1.6), MATERIALS.car, root);
  box("Windscreen", new Vector3(-0.2, 1.2, 0), new Vector3(2.25, 0.4, 1.62), MATERIALS.glass, root);
  for (const x of [-1.3, 1.3]) {
    for (const z of [-0.9, 0.9]) {
      const w = new GameObject("Wheel", { position: new Vector3(x, 0.32, z), rotation: Quaternion.euler(90, 0, 0), scale: new Vector3(0.64, 0.2, 0.64), parent: root });
      w.addComponent(MeshRenderer, { mesh: "cylinder", material: MATERIALS.tyre });
    }
  }
  if (physics) {
    // A car you can shove: the box and the rigidbody on the same object, heavy, upright.
    const col = root.addComponent(BoxCollider);
    col.size = new Vector3(4.2, 1.4, 1.8);
    col.center = new Vector3(0, 0.8, 0);
    const rb = root.addComponent(Rigidbody);
    rb.mass = 350;
    rb.constraints = RigidbodyConstraints.FreezeRotationX | RigidbodyConstraints.FreezeRotationZ;
  } else {
    const col = new GameObject("Collider", { position: new Vector3(0, 0.8, 0), scale: new Vector3(4.2, 1.4, 1.8), parent: root });
    col.addComponent(BoxCollider);
  }
  return root;
}

/** A dynamic prop the server simulates and clients draw: 0 is a crate, 1 the shovable car. */
export function makeProp(kind: number, at: Vector3, physics: boolean): GameObject {
  if (kind === 1) return car(at, 0, physics);
  const go = box("Crate", at, new Vector3(0.9, 0.9, 0.9), MATERIALS.crate, undefined, true);
  if (physics) {
    const rb = go.addComponent(Rigidbody);
    rb.mass = 12;
  }
  return go;
}

export interface Laser {
  light: Light;
  phase: number;
}

/**
 * A street light: a post with an arm reaching over the road, a flat head, and a spot light under
 * it that points straight down and throws a cool blue-white pool on the street.
 */
function lamp(at: Vector3) {
  // The arm reaches towards the street the post stands beside.
  const alongX = Math.abs(at.x) < Math.abs(at.z);
  const dx = alongX ? -Math.sign(at.x) : 0;
  const dz = alongX ? 0 : -Math.sign(at.z);
  const top = 4.2;
  const reach = 1.5;
  box("Lamp post", new Vector3(at.x, top / 2, at.z), new Vector3(0.12, top, 0.12), MATERIALS.post);
  const end = new Vector3(at.x + dx * reach, top, at.z + dz * reach);
  box("Lamp arm", new Vector3(at.x + (dx * reach) / 2, top, at.z + (dz * reach) / 2), new Vector3(dx !== 0 ? reach : 0.1, 0.1, dz !== 0 ? reach : 0.1), MATERIALS.post);
  box("Lamp head", new Vector3(end.x, top - 0.02, end.z), new Vector3(0.7, 0.1, 0.35), MATERIALS.post);
  const lens = new GameObject("Lamp lens", { position: new Vector3(end.x, top - 0.09, end.z), scale: new Vector3(0.55, 0.03, 0.25) });
  lens.addComponent(MeshRenderer, { mesh: "cube", material: MATERIALS.lamp }).shadowCastingMode = "off";
  // A light shines along its -z: turned -90 degrees about x it points down.
  const light = new GameObject("Lamp light", { position: new Vector3(end.x, top - 0.12, end.z), rotation: Quaternion.euler(-90, 0, 0) });
  light.addComponent(Light, { type: "spot", color: new Color(0.38, 0.6, 1, 1), intensity: 20, range: 12, spotAngle: 100, innerSpotAngle: 40 });
}

/** A glowing neon sign: a tube frame round a bar, lit in `color`, with a coloured glow on what is near it. */
function neon(name: string, at: Vector3, w: number, h: number, color: Color, parent?: GameObject, yaw = 0) {
  const tube = Material.create({ color: Color.black, emission: new Color(color.r * 5, color.g * 5, color.b * 5, 1), unlit: true });
  const root = new GameObject(name, { position: at, rotation: Quaternion.euler(0, yaw, 0), parent });
  const t = 0.06;
  const bar = (x: number, y: number, sx: number, sy: number) => {
    const go = new GameObject("Tube", { position: new Vector3(x, y, 0), scale: new Vector3(sx, sy, t), parent: root });
    go.addComponent(MeshRenderer, { mesh: "cube", material: tube }).shadowCastingMode = "off";
  };
  bar(0, h / 2, w, t);
  bar(0, -h / 2, w, t);
  bar(-w / 2, 0, t, h);
  bar(w / 2, 0, t, h);
  bar(0, 0, w * 0.6, t);
  const glow = new GameObject("Neon glow", { position: new Vector3(0, 0, 0.5), parent: root });
  glow.addComponent(Light, { type: "point", color, intensity: 1.6, range: 6 });
}

/** A red laser across the street between two posts, with a glow. */
function laser(a: Vector3, b: Vector3, lasers: Laser[]) {
  const mid = a.add(b).scale(0.5);
  const len = b.sub(a).magnitude;
  const go = new GameObject("Laser", { position: mid, rotation: Quaternion.lookRotation(b.sub(a)), scale: new Vector3(0.07, 0.07, len) });
  go.addComponent(MeshRenderer, { mesh: "cube", material: MATERIALS.laser }).shadowCastingMode = "off";
  for (const p of [a, b]) box("Post", new Vector3(p.x, p.y / 2, p.z), new Vector3(0.14, p.y, 0.14), MATERIALS.post);
  const glow = new GameObject("Laser glow", { position: mid });
  const light = glow.addComponent(Light, { type: "point", color: new Color(1, 0.12, 0.1, 1), intensity: 2.2, range: Math.max(4, len * 0.6) });
  lasers.push({ light, phase: Random.range(0, 6.28) });
}

export interface MapInfo {
  spawns: Vector3[];
  lasers: Laser[];
}

export function buildMap(): MapInfo {
  const lasers: Laser[] = [];
  // Ground, the street along x and the cross street along z, and lane marks.
  box("Ground", new Vector3(0, -0.25, 0), new Vector3(HALF * 3, 0.5, HALF * 3), MATERIALS.ground, undefined, true);
  box("Street", new Vector3(0, 0.005, 0), new Vector3(HALF * 2, 0.01, 6.4), MATERIALS.street);
  box("Cross street", new Vector3(0, 0.006, 0), new Vector3(6.4, 0.01, HALF * 2), MATERIALS.street);
  for (let x = -HALF + 3; x < HALF - 2; x += 4) {
    if (Math.abs(x) < 4) continue;
    box("Lane", new Vector3(x, 0.012, 0), new Vector3(1.6, 0.01, 0.12), MATERIALS.lane);
  }
  for (let z = -HALF + 3; z < HALF - 2; z += 4) {
    if (Math.abs(z) < 4) continue;
    box("Lane", new Vector3(0, 0.012, z), new Vector3(0.12, 0.01, 1.6), MATERIALS.lane);
  }

  // Houses in the four corners and at the street ends, doors to the street.
  const PINK = new Color(1, 0.15, 0.6, 1);
  const CYAN = new Color(0.1, 0.85, 1, 1);
  const VIOLET = new Color(0.62, 0.3, 1, 1);
  const RED = new Color(1, 0.1, 0.12, 1);
  house(new Vector3(-14, 0, -13), 8, 7, 5.5, 0, PINK);
  house(new Vector3(14, 0, -13), 8, 7, 6.5, 0, CYAN);
  house(new Vector3(-14, 0, 13), 8, 7, 6, 180, VIOLET);
  house(new Vector3(14, 0, 13), 8, 7, 5, 180, RED);
  house(new Vector3(-26, 0, 0), 6, 8, 5.5, 90, CYAN);
  house(new Vector3(26, 0, 0), 6, 8, 6, -90, PINK);
  // Blade signs on posts at the cross, readable from along the streets.
  for (const [x, z, c] of [[-5.2, -5.2, PINK], [5.2, 5.2, CYAN]] as [number, number, Color][]) {
    box("Sign post", new Vector3(x, 1.7, z), new Vector3(0.1, 3.4, 0.1), MATERIALS.post);
    neon("Blade sign", new Vector3(x, 3.4, z), 0.5, 1.6, c, undefined, 45);
  }

  // Parked cars (solid), a car and crates you can shove are the server's props.
  car(new Vector3(-8, 0, -4.2), 0, false);
  car(new Vector3(9, 0, 4.2), 180, false);
  car(new Vector3(-4.2, 0, 17), 90, false);
  car(new Vector3(4.2, 0, -19), -90, false);

  // The invisible walls, a little inside where the fog is solid.
  const wall = (x: number, z: number, sx: number, sz: number) => {
    const go = new GameObject("Boundary", { position: new Vector3(x, 3, z), scale: new Vector3(sx, 6, sz) });
    go.addComponent(BoxCollider);
  };
  wall(0, BOUND + 1, BOUND * 2 + 4, 2);
  wall(0, -BOUND - 1, BOUND * 2 + 4, 2);
  wall(BOUND + 1, 0, 2, BOUND * 2 + 4);
  wall(-BOUND - 1, 0, 2, BOUND * 2 + 4);

  // Street lamps: soft warm pools of light on the white pavement, glowing through the mist.
  lamp(new Vector3(-16, 0, 4.4));
  lamp(new Vector3(16, 0, -4.4));
  lamp(new Vector3(4.4, 0, 12));
  lamp(new Vector3(-4.4, 0, -12));
  lamp(new Vector3(-4.4, 0, 24));
  lamp(new Vector3(4.4, 0, -24));
  lamp(new Vector3(-24, 0, -4.4));
  lamp(new Vector3(24, 0, 4.4));

  // Lasers across the streets and between the houses, low and high.
  laser(new Vector3(-6, 0.9, -3.4), new Vector3(-6, 0.9, 3.4), lasers);
  laser(new Vector3(6, 1.4, -3.4), new Vector3(6, 1.4, 3.4), lasers);
  laser(new Vector3(-3.4, 0.5, 10), new Vector3(3.4, 0.5, 10), lasers);
  laser(new Vector3(-3.4, 1.2, -10), new Vector3(3.4, 1.2, -10), lasers);
  laser(new Vector3(-20, 1.0, -7), new Vector3(-8, 1.0, -7), lasers);
  laser(new Vector3(8, 1.0, 7), new Vector3(20, 1.0, 7), lasers);

  const spawns = [
    new Vector3(-18, 0.1, -6),
    new Vector3(18, 0.1, 6),
    new Vector3(0, 0.1, -24),
    new Vector3(0, 0.1, 24),
    new Vector3(-8, 0.1, 8),
    new Vector3(8, 0.1, -8),
    new Vector3(-22, 0.1, 22),
    new Vector3(22, 0.1, -22),
  ];
  return { spawns, lasers };
}

/** Pulses the lasers' glow. */
export function updateLasers(lasers: Laser[], t: number) {
  for (const l of lasers) l.light.intensity = 2.2 + 0.7 * Math.sin(t * 3 + l.phase);
}

/** Fog for watching from a distance (the menu, the server) or for playing (close, to hide the edge). */
export function setFog(watching: boolean) {
  RenderSettings.fogStartDistance = watching ? 45 : FOG_START;
  RenderSettings.fogEndDistance = watching ? 140 : FOG_END;
}

/** The scene's look: bright overcast, white fog, no sun. */
export function applyLook() {
  QualitySettings.shadowDistance = 40;
  RenderSettings.ambientMode = "color";
  RenderSettings.ambientLight = new Color(0.5, 0.52, 0.58, 1);
  RenderSettings.tonemapping = "neutral";
  // Emission above 1 blooms: the neon tubes, lamp lenses and beams glow.
  RenderSettings.bloom = { threshold: 1.1, intensity: 0.45, scatter: 0.75 };
  RenderSettings.postExposure = -0.25;
  RenderSettings.fog = true;
  RenderSettings.fogMode = "linear";
  RenderSettings.fogColor = WHITE;
  RenderSettings.fogStartDistance = FOG_START;
  RenderSettings.fogEndDistance = FOG_END;
  // A thick ground mist on top of the distance fog: thick at the street, thinning by head height.
  RenderSettings.heightFog = { density: 0.07, baseHeight: 0, falloff: 1.2 };
  RenderSettings.skybox = {
    zenith: new Color(0.72, 0.74, 0.78, 1),
    horizon: new Color(0.94, 0.95, 0.97, 1),
    ground: new Color(0.94, 0.95, 0.97, 1),
    sunDirection: null,
  };
  const sky = new GameObject("Sky light", { rotation: Quaternion.lookRotation(new Vector3(0.3, -1, 0.4)) });
  sky.addComponent(Light, { type: "directional", color: new Color(1, 0.99, 0.97, 1), intensity: 1.4, shadows: "soft", shadowStrength: 0.6 });
}
