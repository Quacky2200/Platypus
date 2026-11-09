// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The arena's game: one component that is a dedicated server (`--server`, also with `--headless`)
// or a client (`--connect host:port`), or waits for the menu to choose.
//
// Roles. The server owns the match (spawns, health, kills, the scoreboard) and every prop. Each
// client owns its own player's position, yaw and speed (they are `owner` fields): it moves at
// once, with no waiting for the server, and the server validates what arrives (a move faster than
// anyone can run is refused and the player is put back). Shots are RPCs the server resolves,
// rewinding the players it tests to where the shooter saw them (lag compensation). Other
// players and props are drawn from the interpolated replicated state.

import { HALF, WHITE, applyLook, buildMap, makeProp, setFog, updateLasers } from "./map";
import type { Laser, MapInfo } from "./map";
import {
  KILL_LIMIT, MAX_REWIND, MAX_SPEED, RESPAWN_SECONDS, ROUND_SECONDS, SHIELD_SECONDS, SHOT_DAMAGE, SPRINT,
  START_HP, TICK_RATE, WALK, defineNetwork,
} from "./schema";

const SEP = "\u001f";
/** Network classes by their order in `defineNetwork`. */
const PLAYER = 0;
const PROP = 1;
const MATCH = 2;
const PORT = 7777;

export interface Row {
  name: string;
  kills: number;
  deaths: number;
  ping: number;
  me: boolean;
}
export interface FeedLine {
  text: string;
  until: number;
}

/** What the HUD draws; the arena fills it in, the HUD reads it. */
export const hud = {
  mode: "menu" as "menu" | "server" | "client",
  status: "",
  error: "",
  hp: START_HP,
  alive: true,
  shield: false,
  rtt: -1,
  match: "waiting",
  timeLeft: 0,
  winner: "",
  round: 0,
  rows: [] as Row[],
  feed: [] as FeedLine[],
  hurt: 0,
  fingerprint: "",
  players: 0,
  dropped: 0,
  showScores: false,
  debug: false,
};
export let refreshHud = () => {};
export function onHud(f: () => void) {
  refreshHud = f;
}

/** What the menu asks for. */
export const request = { mode: "menu" as "menu" | "server" | "client", address: `127.0.0.1:${PORT}`, name: "Player" };

const v3 = (a: number[]) => new Vector3(a[0] ?? 0, a[1] ?? 0, a[2] ?? 0);
const fmt = (v: Vector3) => `v:${v.x},${v.y},${v.z}`;
const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));

interface Parsed {
  kind: string;
  f: string[];
}
function events(): Parsed[] {
  const text = Network.takeEvents();
  if (text === "") return [];
  return text.split("\n").map((line) => {
    const f = line.split(SEP);
    return { kind: f[0] ?? "", f: f.slice(1) };
  });
}
/** The arguments of an `rpc` event, by position: numbers for f/i/b, vectors as arrays. */
function argNum(s: string | undefined): number {
  return parseFloat((s ?? "").slice(2));
}
function argVec(s: string | undefined): Vector3 {
  return v3((s ?? "").slice(2).split(",").map((x) => parseFloat(x)));
}

interface History {
  t: number;
  p: Vector3;
}
interface ServerPlayer {
  net: number;
  player: number;
  name: string;
  last: Vector3;
  lastAt: number;
  hist: History[];
  lastShot: number;
  respawnAt: number;
  shieldUntil: number;
  warnAt: number;
}

/** A figure in the world: a player (the mannequin) or a prop. */
interface View {
  go: GameObject;
  kind: number;
  local: boolean;
  animator: Animator | null;
  body: GameObject | null;
  lastPos: Vector3;
  shown: Vector3;
  /** The flashlight's pivot, the hand bone it follows, and where its lens is in the world. */
  torch?: GameObject;
  hand?: GameObject;
  lens?: Vector3;
  /** Despawned: an unfinished model load must not dress it. */
  gone: boolean;
}

let figure: Promise<Model> | null = null;
function loadFigure(): Promise<Model> {
  figure ??= Model.load("assets/mannequin.glb").whenLoaded();
  return figure;
}

export class Arena extends Component {
  private map!: MapInfo;
  private lasers: Laser[] = [];
  private mode: "menu" | "server" | "client" = "menu";
  private t0 = 0;
  private nextHud = 0;
  private nextInfo = 0;

  // --- server ---
  private players = new Map<number, ServerPlayer>(); // player id -> record
  private matchNet = 0;
  private props: { net: number; go: GameObject }[] = [];
  private roundEnds = 0;
  private roundEndAt = 0;
  private serverReady = false;

  // --- client ---
  private me = -1;
  private views = new Map<number, View>(); // net id -> view
  private matchId = 0;
  private camera!: Camera;
  private yaw = 0;
  private pitch = 8;
  private airTime = 0;
  private prevPos: Vector3 | null = null;
  private speedNow = 0;
  private pointerLocked = false;
  private pointerFree = false;
  private distance = 2.6;
  // The model faces +z, the game's forward is -z, so a fresh player looks away from the camera.
  private facing = Math.PI;
  private facingDeg = 180;
  private vy = 0;
  private controller: CharacterController | null = null;
  private localId = 0;
  private bot = false;
  private verbose = false;
  private botDist = 99;
  private playingFog = true;
  private lastLog = 0;
  private fireAt = 0;
  private beams: { gos: GameObject[]; lights: { go: GameObject; light: Light; peak: number }[]; start: number; until: number }[] = [];
  private pivot = new Vector3(0, 1.4, 0);

  override onStart() {
    this.t0 = Time.time;
    applyLook();
    this.map = buildMap();
    this.lasers = this.map.lasers;
    this.camera = new GameObject("Main Camera").addComponent(Camera, {
      fieldOfView: 60, backgroundColor: WHITE, nearClipPlane: 0.1, farClipPlane: 120,
    });
    this.camera.transform.position = new Vector3(0, 12, -30);
    this.camera.transform.rotation = Quaternion.lookRotation(new Vector3(0, -0.4, 1));
    const args = Application.args;
    this.bot = args.includes("--bot");
    this.verbose = args.includes("--verbose");
    const at = args.indexOf("--connect");
    if (args.includes("--server")) this.begin("server");
    else if (at >= 0) {
      request.address = args[at + 1] ?? request.address;
      const nm = args.indexOf("--name");
      if (nm >= 0) request.name = args[nm + 1] ?? request.name;
      this.begin("client");
    }
  }

  private begin(mode: "server" | "client") {
    this.mode = hud.mode = mode;
    hud.error = "";
    defineNetwork();
    if (mode === "server") {
      hud.status = "starting the server";
      Network.startServer(PORT, 16, TICK_RATE);
    } else {
      hud.status = `connecting to ${request.address}`;
      Network.connect(request.address, request.name);
    }
    refreshHud();
  }

  private end(message: string) {
    Network.disconnect();
    for (const v of this.views.values()) {
      v.gone = true;
      v.go.destroy();
    }
    this.views.clear();
    this.players.clear();
    this.props.length = 0;
    this.serverReady = false;
    this.me = -1;
    this.localId = 0;
    this.controller = null;
    this.mode = hud.mode = "menu";
    request.mode = "menu";
    hud.error = message;
    hud.status = "";
    hud.rows = [];
    refreshHud();
  }

  override onUpdate(dt: number) {
    updateLasers(this.lasers, Time.time - this.t0);
    hud.showScores = input.getKey("Tab");
    if (input.getKeyDown("F1")) hud.debug = !hud.debug;
    // Esc frees the pointer (menus, the debug panel); a click on the game takes it again.
    if (this.pointerFree && input.getMouseButtonDown(0) && this.mode === "client" && !hud.debug) this.pointerFree = false;
    else if (input.getKeyDown("Escape")) this.pointerFree = true;
    // Mouse look needs the pointer locked while playing; menus, the debug panel and Esc free it.
    const lock = this.mode === "client" && this.localId !== 0 && !hud.debug && !this.pointerFree;
    if (lock !== this.pointerLocked) {
      this.pointerLocked = lock;
      Cursor.lockState = lock ? CursorLockMode.Locked : CursorLockMode.None;
      Cursor.visible = !lock;
    }
    if (this.mode === "menu" && request.mode !== "menu") {
      const m = request.mode;
      request.mode = "menu";
      this.begin(m);
    }
    if (this.mode === "server") this.serverFrame(dt);
    else if (this.mode === "client") this.clientFrame(dt);
    else this.orbit(dt);
    // Players see a small foggy world; the menu and the server watch it from afar.
    const playing = this.mode === "client" && this.localId !== 0;
    if (playing !== this.playingFog) {
      this.playingFog = playing;
      setFog(!playing);
    }
    this.alignTorches();
    this.updateBeams();
    // A line a second for logs and tests: who is here and how the network is doing.
    if (this.mode !== "menu" && Time.time - this.lastLog >= 1) {
      this.lastLog = Time.time;
      const kills = hud.rows.map((r) => `${r.name}:${r.kills}/${r.deaths}@${r.ping}`).join(" ");
      const an = this.views.get(this.localId)?.animator;
      if (this.mode === "client") {
        console.log(`[net view] anim=${an ? `${an.currentStateName} t=${an.currentNormalizedTime.toFixed(2)} speed=${an.getFloat("speed").toFixed(2)} air=${an.getBool("air")}` : "none"} lock=${this.pointerLocked} free=${this.pointerFree} cursor=${Cursor.lockState}`);
      }
      console.log(`[net ${this.mode}] ${hud.status} players=${hud.rows.length} match=${hud.match} rtt=${Math.round(hud.rtt)} refused=${hud.dropped} ${kills}`);
    }
    if (Time.time >= this.nextHud) {
      this.nextHud = Time.time + 0.1;
      hud.rtt = Network.rtt();
      hud.dropped = Network.droppedRpcs();
      refreshHud();
    }
  }

  /** The menu's camera: a slow turn round the town. */
  private orbit(dt: number) {
    this.yaw += 6 * dt;
    this.placeCamera(new Vector3(0, 2, 0), 34, 22, dt, true);
  }

  private placeCamera(target: Vector3, distance: number, pitchDeg: number, dt: number, snap = false) {
    this.pivot = snap ? target : this.pivot.add(target.sub(this.pivot).scale(Math.min(1, 10 * dt)));
    const yaw = (this.yaw * Math.PI) / 180;
    const pitch = (pitchDeg * Math.PI) / 180;
    const back = new Vector3(-Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch));
    const cam = this.camera.transform;
    cam.position = this.pivot.add(back.scale(distance));
    cam.rotation = Quaternion.lookRotation(this.pivot.sub(cam.position));
  }

  private updateBeams() {
    this.beams = this.beams.filter((b) => {
      if (Time.time < b.until) {
        // Quick fade: the lights die first, the core lingers a frame or two longer.
        const k = 1 - (Time.time - b.start) / (b.until - b.start);
        for (const l of b.lights) l.light.intensity = l.peak * k * k;
        return true;
      }
      for (const g of b.gos) g.destroy();
      for (const l of b.lights) l.go.destroy();
      return false;
    });
  }

  /**
   * A shot: a hot white core with a red bloom (emission above 1 blooms), a flash at the muzzle, a
   * burst where it lands (bigger and bluish-white on a player), and red lights along it that wash
   * the white town for a moment and fade out.
   */
  private drawBeam(from: Vector3, to: Vector3, hit: boolean) {
    const span = to.sub(from);
    const len = Math.max(0.01, span.magnitude);
    const rot = Quaternion.lookRotation(span);
    const root = new GameObject("Beam", { position: from.add(span.scale(0.5)), rotation: rot, scale: new Vector3(0.05, 0.05, len) });
    root.addComponent(MeshRenderer, { mesh: "cube", material: beamMaterial() }).shadowCastingMode = "off";
    const lights: { go: GameObject; light: Light; peak: number }[] = [];
    const addLight = (at: Vector3, color: Color, peak: number, range: number) => {
      const go = new GameObject("BeamLight", { position: at });
      const light = go.addComponent(Light, { type: "point", color, intensity: peak, range });
      lights.push({ go, light, peak });
    };
    const red = new Color(1, 0.15, 0.1, 1);
    addLight(from, new Color(1, 0.55, 0.4, 1), 6, 5);
    const n = Math.min(4, Math.floor(len / 6));
    for (let i = 1; i <= n; i++) addLight(from.add(span.scale(i / (n + 1))), red, 1.6, 5);
    addLight(to, hit ? new Color(1, 0.8, 0.8, 1) : red, hit ? 8 : 3.5, hit ? 6 : 4);
    const burst = new GameObject("BeamBurst", { position: to, scale: new Vector3(hit ? 0.5 : 0.24, hit ? 0.5 : 0.24, hit ? 0.5 : 0.24) });
    burst.addComponent(MeshRenderer, { mesh: "sphere", material: beamMaterial() }).shadowCastingMode = "off";
    const start = Time.time;
    this.beams.push({ gos: [root, burst], lights, start, until: start + 0.2 });
  }

  // ===================================================================================== server

  private serverFrame(dt: number) {
    const now = Time.time;
    for (const e of events()) {
      switch (e.kind) {
        case "listening":
          hud.status = `serving on UDP ${e.f[0]}`;
          hud.fingerprint = e.f[1] ?? "";
          this.serverReady = true;
          this.startMatchObjects();
          break;
        case "error":
          this.end(e.f[0] ?? "network error");
          return;
        case "joined":
          this.join(parseFloat(e.f[0]), e.f[1] ?? "player", e.f[2] === "1", now);
          break;
        case "dropped":
          break;
        case "left":
          this.leave(parseFloat(e.f[0]));
          break;
        case "rpc":
          if (e.f[1] === "fire") this.onFire(parseFloat(e.f[2]), argNum(e.f[3]), argNum(e.f[4]), now);
          break;
      }
    }
    if (!this.serverReady) return;
    for (const p of this.players.values()) this.checkPlayer(p, now);
    this.syncProps();
    this.matchFrame(now);
    // The server's own view of the world.
    hud.players = this.players.size;
    this.fillRows();
    this.yaw += 6 * dt;
    this.placeCamera(new Vector3(0, 2, 0), 38, 24, dt);
  }

  private startMatchObjects() {
    this.matchNet = Network.spawn("Match", "match", 0);
    Network.setString(this.matchNet, "state", "waiting");
    // Crates and the shovable car: the server simulates them, clients draw them.
    const spots = [
      [-3, 0.45, -6], [-2, 0.45, -6.2], [-2.6, 1.35, -6.1], [3, 0.45, 6], [3.1, 0.45, 7], [-12, 0.45, 2], [12, 0.45, -2], [0, 0.45, 15],
    ];
    for (const s of spots) this.addProp(0, new Vector3(s[0]!, s[1]!, s[2]!));
    this.addProp(1, new Vector3(-6, 0, 1.5));
  }

  private addProp(kind: number, at: Vector3) {
    const net = Network.spawn("Prop", kind === 1 ? "car" : "crate", 0);
    Network.setNumber(net, "kind", kind);
    Network.setVec3(net, "pos", at.x, at.y, at.z);
    this.props.push({ net, go: makeProp(kind, at, true) });
  }

  private syncProps() {
    for (const p of this.props) {
      const t = p.go.transform;
      Network.setVec3(p.net, "pos", t.position.x, t.position.y, t.position.z);
      const r = t.rotation;
      Network.setQuat(p.net, "rot", r.x, r.y, r.z, r.w);
    }
  }

  private join(player: number, name: string, resumed: boolean, now: number) {
    let p = this.players.get(player);
    if (!p) {
      const net = Network.spawn("Player", "player", player);
      p = { net, player, name, last: Vector3.zero, lastAt: now, hist: [], lastShot: 0, respawnAt: 0, shieldUntil: 0, warnAt: 0 };
      this.players.set(player, p);
      Network.setString(net, "name", name);
      Network.setNumber(net, "hp", START_HP);
      this.respawn(p, now);
    }
    hud.status = `${this.players.size} player${this.players.size === 1 ? "" : "s"}${resumed ? " (one returned)" : ""}`;
  }

  private leave(player: number) {
    const p = this.players.get(player);
    if (!p) return;
    Network.despawn(p.net);
    this.players.delete(player);
  }

  private spawnPoint(except: ServerPlayer): Vector3 {
    let best = this.map.spawns[0]!;
    let bestDist = -1;
    for (const s of this.map.spawns) {
      let d = Infinity;
      for (const o of this.players.values()) {
        if (o === except || Network.getBool(o.net, "alive") === false) continue;
        d = Math.min(d, o.last.sub(s).magnitude);
      }
      if (d > bestDist) {
        bestDist = d;
        best = s;
      }
    }
    return best;
  }

  private respawn(p: ServerPlayer, now: number) {
    const at = this.spawnPoint(p);
    p.last = at;
    p.lastAt = now;
    p.hist = [];
    p.shieldUntil = now + SHIELD_SECONDS;
    Network.setNumber(p.net, "hp", START_HP);
    Network.setBool(p.net, "alive", true);
    Network.setBool(p.net, "shield", true);
    Network.rpc(p.net, "teleport", fmt(at));
  }

  /** Per frame: validate the owner's move, keep the history lag compensation reads, timers. */
  private checkPlayer(p: ServerPlayer, now: number) {
    const pos = v3(Network.getVec3(p.net, "pos"));
    const alive = Network.getBool(p.net, "alive");
    if (alive) {
      const moved = pos.sub(p.last).magnitude;
      const allowed = MAX_SPEED * Math.max(0.05, now - p.lastAt) + 0.8;
      const outside = Math.abs(pos.x) > HALF + 1 || Math.abs(pos.z) > HALF + 1;
      if (moved > allowed || outside) {
        // Too fast, or through a wall: put the player back (not more than four times a second).
        if (now >= p.warnAt) {
          p.warnAt = now + 0.25;
          Network.rpc(p.net, "teleport", fmt(p.last));
        }
      } else {
        p.last = pos;
        p.lastAt = now;
      }
      p.hist.push({ t: now, p: p.last });
      while (p.hist.length > 0 && now - p.hist[0]!.t > 1) p.hist.shift();
    } else if (p.respawnAt > 0 && now >= p.respawnAt && this.matchRunning()) {
      p.respawnAt = 0;
      this.respawn(p, now);
    }
    if (p.shieldUntil > 0 && now >= p.shieldUntil) {
      p.shieldUntil = 0;
      Network.setBool(p.net, "shield", false);
    }
    Network.setNumber(p.net, "ping", Math.max(0, Math.round(Network.rtt(p.player))));
  }

  private matchRunning(): boolean {
    return Network.getString(this.matchNet, "state") !== "roundEnd";
  }

  /** Where `p` was `back` seconds ago (clamped to what the history holds). */
  private positionAt(p: ServerPlayer, t: number): Vector3 {
    const h = p.hist;
    if (h.length === 0) return p.last;
    if (t <= h[0]!.t) return h[0]!.p;
    for (let i = h.length - 1; i > 0; i--) {
      const a = h[i - 1]!;
      const b = h[i]!;
      if (t >= a.t && t <= b.t) return a.p.add(b.p.sub(a.p).scale((t - a.t) / Math.max(1e-6, b.t - a.t)));
    }
    return h[h.length - 1]!.p;
  }

  private onFire(player: number, yawDeg: number, rewindMs: number, now: number) {
    if (this.verbose) console.log(`[net server] fire from ${player} yaw=${yawDeg} rewind=${rewindMs}`);
    const shooter = this.players.get(player);
    if (!shooter || !Network.getBool(shooter.net, "alive") || now - shooter.lastShot < 0.22) return;
    if (!this.matchRunning() || !Number.isFinite(yawDeg)) return;
    shooter.lastShot = now;
    const yaw = (yawDeg * Math.PI) / 180;
    // Heading convention (see docs/world-rendering.md "Coordinates"): forward is -z at yaw 0.
    const dir = new Vector3(Math.sin(yaw), 0, -Math.cos(yaw));
    const origin = shooter.last.add(new Vector3(0, 1.3, 0)).add(dir.scale(0.4));
    const t = now - clamp((Number.isFinite(rewindMs) ? rewindMs : 0) / 1000, 0, MAX_REWIND);
    // Walls and houses stop a beam; players are tested against where they were at time `t`.
    const wall = Physics.raycast(origin, dir, 60);
    let reach = wall ? wall.distance : 60;
    let victim: ServerPlayer | null = null;
    for (const o of this.players.values()) {
      if (o === shooter || !Network.getBool(o.net, "alive") || Network.getBool(o.net, "shield")) continue;
      const c = this.positionAt(o, t);
      // A ray against a vertical cylinder of radius 0.45: a circle in the ground plane.
      const ox = origin.x - c.x;
      const oz = origin.z - c.z;
      const b = ox * dir.x + oz * dir.z;
      const k = ox * ox + oz * oz - 0.45 * 0.45;
      const disc = b * b - k;
      if (this.verbose) {
        console.log(`[net server] target ${o.name} at (${c.x},${c.z}) across=${ox * dir.z - oz * dir.x} along=${-b} hist=${o.hist.length}`);
      }
      if (disc < 0) continue;
      const d = -b - Math.sqrt(disc);
      if (d > 0 && d < reach) {
        reach = d;
        victim = o;
      }
    }
    const to = origin.add(dir.scale(reach));
    if (this.verbose) {
      const o = origin;
      console.log(`[net server] shot from (${o.x},${o.z}) dir=(${dir.x},${dir.z}) wall=${wall ? wall.distance : -1} reach=${reach} victim=${victim ? victim.name : "none"}`);
    }
    Network.rpc(shooter.net, "beam", [fmt(origin), fmt(to), `b:${victim ? 1 : 0}`].join(SEP));
    if (!victim) return;
    const hp = Network.getNumber(victim.net, "hp") - SHOT_DAMAGE;
    Network.setNumber(victim.net, "hp", Math.max(0, hp));
    Network.rpc(victim.net, "hurt", `f:${SHOT_DAMAGE}`);
    if (hp <= 0) this.kill(shooter, victim, now);
  }

  private kill(killer: ServerPlayer, victim: ServerPlayer, now: number) {
    Network.setBool(victim.net, "alive", false);
    victim.respawnAt = now + RESPAWN_SECONDS;
    Network.setNumber(victim.net, "deaths", Network.getNumber(victim.net, "deaths") + 1);
    Network.setNumber(killer.net, "kills", Network.getNumber(killer.net, "kills") + 1);
    Network.rpc(this.matchNet, "feed", [`s:${killer.name}`, `s:${victim.name}`].join(SEP));
  }

  private matchFrame(now: number) {
    const state = Network.getString(this.matchNet, "state");
    const alive = [...this.players.values()];
    if (state === "waiting") {
      if (alive.length >= 2) this.startRound(now);
      return;
    }
    if (state === "running") {
      this.roundEnds = this.roundEnds || now + ROUND_SECONDS;
      Network.setNumber(this.matchNet, "timeLeft", Math.max(0, this.roundEnds - now));
      let top: ServerPlayer | null = null;
      for (const p of alive) {
        if (top === null || Network.getNumber(p.net, "kills") > Network.getNumber(top.net, "kills")) top = p;
      }
      const won = top !== null && Network.getNumber(top.net, "kills") >= KILL_LIMIT;
      if (won || now >= this.roundEnds || alive.length < 2) {
        Network.setString(this.matchNet, "winner", top ? top.name : "");
        Network.setString(this.matchNet, "state", "roundEnd");
        this.roundEndAt = now + 6;
      }
    } else if (state === "roundEnd" && now >= this.roundEndAt) {
      for (const p of alive) {
        Network.setNumber(p.net, "kills", 0);
        Network.setNumber(p.net, "deaths", 0);
      }
      this.roundEnds = 0;
      Network.setString(this.matchNet, "state", alive.length >= 2 ? "running" : "waiting");
      if (alive.length >= 2) this.startRound(now);
    }
  }

  private startRound(now: number) {
    Network.setString(this.matchNet, "state", "running");
    Network.setString(this.matchNet, "winner", "");
    Network.setNumber(this.matchNet, "round", Network.getNumber(this.matchNet, "round") + 1);
    this.roundEnds = now + ROUND_SECONDS;
    for (const p of this.players.values()) this.respawn(p, now);
  }

  private fillRows() {
    const rows: Row[] = [];
    for (const p of this.players.values()) {
      rows.push({
        name: p.name, kills: Network.getNumber(p.net, "kills"), deaths: Network.getNumber(p.net, "deaths"),
        ping: Network.getNumber(p.net, "ping"), me: false,
      });
    }
    hud.rows = rows.sort((a, b) => b.kills - a.kills);
    hud.match = Network.getString(this.matchNet, "state");
    hud.timeLeft = Network.getNumber(this.matchNet, "timeLeft");
    hud.winner = Network.getString(this.matchNet, "winner");
    hud.round = Network.getNumber(this.matchNet, "round");
  }

  // ===================================================================================== client

  private clientFrame(dt: number) {
    for (const e of events()) this.clientEvent(e);
    if (this.mode !== "client") return;
    if (this.localId !== 0) this.moveLocal(dt);
    this.updateOthers();
    this.fillClientHud();
    this.followCamera(dt);
  }

  private clientEvent(e: Parsed) {
    switch (e.kind) {
      case "connected":
        this.me = parseFloat(e.f[0]);
        hud.status = `connected as player ${this.me}`;
        // `--lag <ms round trip>` and `--loss <percent>` make this client's network bad, for tests.
        {
          const a = Application.args;
          const lag = a.indexOf("--lag");
          const loss = a.indexOf("--loss");
          if (lag >= 0 || loss >= 0) {
            const rtt = lag >= 0 ? parseFloat(a[lag + 1] ?? "0") : 0;
            const pct = loss >= 0 ? parseFloat(a[loss + 1] ?? "0") : 0;
            Network.debugConditions(rtt / 4, rtt / 10, pct);
          }
        }
        break;
      case "rejected":
        this.end(`the server refused us: ${e.f[0]} (is it the same build?)`);
        return;
      case "disconnected":
        this.end(`disconnected: ${e.f[0]}`);
        return;
      case "error":
        this.end(e.f[0] ?? "network error");
        return;
      case "spawned":
        this.onSpawned(parseFloat(e.f[0]), parseFloat(e.f[1]), parseFloat(e.f[3]));
        break;
      case "despawned": {
        const id = parseFloat(e.f[0]);
        const gone = this.views.get(id);
        if (gone) gone.gone = true;
        gone?.go.destroy();
        this.views.delete(id);
        if (id === this.localId) {
          this.localId = 0;
          this.controller = null;
        }
        break;
      }
      case "rpc":
        this.onRpc(e.f[1] ?? "", e.f.slice(3));
        break;
    }
  }

  private onSpawned(id: number, cls: number, owner: number) {
    if (cls === MATCH) {
      this.matchId = id;
    } else if (cls === PROP) {
      const kind = Network.getNumber(id, "kind");
      const go = makeProp(kind === 1 ? 1 : 0, v3(Network.getVec3(id, "pos")), false);
      this.views.set(id, { go, kind: 1, local: false, animator: null, body: null, lastPos: Vector3.zero, shown: Vector3.zero, gone: false });
    } else if (cls === PLAYER) {
      const local = owner === this.me;
      const go = new GameObject(local ? "Me" : "Player", { position: v3(Network.getVec3(id, "pos")) });
      const view: View = { go, kind: 0, local, animator: null, body: null, lastPos: go.transform.position, shown: go.transform.position, gone: false };
      this.views.set(id, view);
      if (local) {
        this.localId = id;
        this.controller = go.addComponent(CharacterController, { radius: 0.3, height: 1.8 });
        this.controller.center = new Vector3(0, 0.95, 0);
      }
      void this.dress(view, local);
    }
  }

  private async dress(view: View, local: boolean) {
    const model = await loadFigure();
    if (view.gone) return;
    const body = model.instantiate({ parent: view.go, position: Vector3.zero });
    body.getComponent(Animation)?.destroy();
    const [main, joints] = model.materials;
    const white = Material.create({ color: new Color(0.93, 0.94, 0.96, 1), smoothness: 0.55 });
    const tint = local ? new Color(0.35, 0.6, 1.1, 1) : new Color(1.3, 0.2, 0.15, 1);
    const glow = Material.create({ color: new Color(0.9, 0.9, 0.95, 1), emission: tint, smoothness: 0.7 });
    const recolour = (m: Material | null) => (m === joints ? glow : m === main ? white : m);
    const paint = (go: GameObject) => {
      const s = go.getComponent(SkinnedMeshRenderer);
      if (s) s.material = recolour(s.material);
      const r = go.getComponent(MeshRenderer);
      if (r) r.material = recolour(r.material);
      for (const c of go.children) paint(c);
    };
    paint(body);
    const clip = (name: string) => {
      const c = model.animations.find((a) => a.name === name);
      if (!c) throw new Error(`mannequin.glb has no ${name}`);
      c.wrapMode = "loop";
      return c;
    };
    // Walk, jog and sprint blend on speed; a jump (airborne) plays the jump start then its loop,
    // and landing plays the landing and returns to locomotion.
    const once = (name: string) => {
      const c = clip(name);
      c.wrapMode = "once";
      return c;
    };
    const controller = new AnimatorController()
      .addParameter("speed", "float", 0)
      .addParameter("air", "bool", false)
      .addState("Locomotion", {
        blendParameter: "speed",
        children: [
          { motion: clip("Idle_Loop"), threshold: 0 },
          { motion: clip("Walk_Loop"), threshold: 1.5, timeScale: 1.5 / 0.97 },
          { motion: clip("Jog_Fwd_Loop"), threshold: WALK, timeScale: WALK / 5.36 },
          { motion: clip("Sprint_Loop"), threshold: SPRINT, timeScale: SPRINT / 8.25 },
        ],
      })
      .addState("JumpStart", once("Jump_Start"))
      .addState("JumpLoop", clip("Jump_Loop"))
      .addState("JumpLand", once("Jump_Land"))
      .addTransition({ from: "Locomotion", to: "JumpStart", conditions: [{ parameter: "air", mode: "if" }], duration: 0.08 })
      .addTransition({ from: "JumpStart", to: "JumpLoop", hasExitTime: true, exitTime: 0.95, duration: 0.08 })
      .addTransition({ from: "JumpLoop", to: "JumpLand", conditions: [{ parameter: "air", mode: "ifNot" }], duration: 0.05 })
      .addTransition({ from: "JumpStart", to: "JumpLand", conditions: [{ parameter: "air", mode: "ifNot" }], duration: 0.05 })
      .addTransition({ from: "JumpLand", to: "Locomotion", hasExitTime: true, exitTime: 0.7, duration: 0.12 });
    view.animator = body.addComponent(Animator, { controller });
    view.body = body;
    const held = holdFlashlight(body);
    if (held) {
      view.torch = held.pivot;
      view.hand = held.hand;
    }
  }

  /**
   * The torch rides the hand's position but points where the figure faces, whatever the pose of
   * the wrist, so it always sits level in the fist; its lens is where beams start.
   */
  private alignTorches() {
    for (const [id, v] of this.views) {
      if (!v.torch || !v.hand || !v.body) continue;
      const deg = v.local ? this.facingDeg : Network.getNumber(id, "yaw");
      const r = (deg * Math.PI) / 180;
      const fwd = new Vector3(Math.sin(r), 0, Math.cos(r));
      v.torch.transform.rotation = Quaternion.euler(0, deg, 0).mul(Quaternion.euler(0, -90, 0));
      v.torch.transform.position = v.hand.transform.position.add(fwd.scale(0.04)).add(new Vector3(0, -0.02, 0));
      v.lens = v.torch.transform.position.add(fwd.scale(0.22));
    }
  }

  /** Where a shot should start: the lens of whoever is standing at `from`. */
  private muzzleNear(from: Vector3): Vector3 {
    let best = 2;
    let at = from;
    for (const v of this.views.values()) {
      if (v.kind !== 0 || !v.lens) continue;
      const d = Math.hypot(v.go.transform.position.x - from.x, v.go.transform.position.z - from.z);
      if (d < best) {
        best = d;
        at = v.lens;
      }
    }
    return at;
  }

  /**
   * The heading to shoot along so the shot lands where the crosshair is: the camera sits over the
   * shoulder, so a shot sent along the camera's heading would miss by that offset. It aims at a
   * player close to the crosshair ray, else at what the ray hits.
   */
  private aimYaw(me: Vector3): number {
    const y = (this.yaw * Math.PI) / 180;
    const p = (this.pitch * Math.PI) / 180;
    const f = new Vector3(Math.sin(y) * Math.cos(p), -Math.sin(p), -Math.cos(y) * Math.cos(p));
    const c = this.camera.transform.position;
    let target: Vector3 | null = null;
    let bestAngle = 0.06;
    for (const [id, v] of this.views) {
      if (v.local || v.kind !== 0 || !Network.getBool(id, "alive")) continue;
      const to = v.shown.add(new Vector3(0, 1, 0)).sub(c);
      const along = to.x * f.x + to.y * f.y + to.z * f.z;
      if (along < 2) continue;
      const angle = to.sub(f.scale(along)).magnitude / along;
      if (angle < bestAngle) {
        bestAngle = angle;
        target = v.shown;
      }
    }
    if (!target) {
      const hit = Physics.raycast(c.add(f.scale(1)), f, 80);
      target = c.add(f.scale(hit ? hit.distance + 1 : 60));
    }
    const dx = target.x - me.x;
    const dz = target.z - me.z;
    if (Math.hypot(dx, dz) < 1.2) return this.yaw;
    return (Math.atan2(dx, -dz) * 180) / Math.PI;
  }

  private onRpc(name: string, args: string[]) {
    if (name === "beam") {
      const from = argVec(args[0]);
      this.drawBeam(this.muzzleNear(from), argVec(args[1]), args[2] === "b:1");
    }
    else if (name === "feed") hud.feed.push({ text: `${(args[0] ?? "").slice(2)}  >  ${(args[1] ?? "").slice(2)}`, until: Time.time + 6 });
    else if (name === "hurt") hud.hurt = Time.time + 0.25;
    else if (name === "teleport" && this.localId !== 0) {
      const p = argVec(args[0]);
      if (this.verbose) console.log(`[net client] teleport to (${p.x},${p.z})`);
      // A controller keeps its own position: make a fresh one where the player is put.
      const go = this.views.get(this.localId)!.go;
      this.controller?.destroy();
      go.transform.position = p;
      this.controller = go.addComponent(CharacterController, { radius: 0.3, height: 1.8 });
      this.controller.center = new Vector3(0, 0.95, 0);
      this.vy = 0;
    }
  }

  private moveLocal(dt: number) {
    const view = this.views.get(this.localId);
    const cc = this.controller;
    if (!view || !cc) return;
    const alive = Network.getBool(this.localId, "alive");
    // Camera: the locked mouse turns it (Esc or F1 frees the pointer), Q / E too, the wheel zooms.
    if (this.pointerLocked && !this.bot) {
      const d = input.mouseDelta;
      this.yaw += d.x * 0.12;
      this.pitch = clamp(this.pitch - d.y * 0.1, -5, 60);
    }
    if (input.getKey("KeyQ")) this.yaw -= 90 * dt;
    if (input.getKey("KeyE")) this.yaw += 90 * dt;
    const wheel = input.mouseScrollDelta.y;
    if (wheel) this.distance = clamp(this.distance - wheel * 0.3, 1.6, 8);

    const yawR = (this.yaw * Math.PI) / 180;
    // Platypus is right-handed and looks down -z: yaw 0 faces -z, turning right (yaw up) faces +x.
    const fwd = new Vector3(Math.sin(yawR), 0, -Math.cos(yawR));
    const right = new Vector3(Math.cos(yawR), 0, Math.sin(yawR));
    // A bot (`--bot`) walks and shoots by itself, for tests with no one at the keyboard.
    const bt = Time.time;
    // (Camera yaw faces the target, so "forward" walks at it; it strafes while it goes.)
    const x = this.bot ? Math.sin(bt * 1.3 + this.me) * 0.6 : (input.getKey("KeyD") || input.getKey("ArrowRight") ? 1 : 0) - (input.getKey("KeyA") || input.getKey("ArrowLeft") ? 1 : 0);
    const z = this.bot ? (this.botDist > 6 ? 1 : -0.3) : (input.getKey("KeyW") || input.getKey("ArrowUp") ? 1 : 0) - (input.getKey("KeyS") || input.getKey("ArrowDown") ? 1 : 0);
    if (this.bot) {
      // Aim at the nearest other player.
      let best = Infinity;
      const me = view.go.transform.position;
      for (const [id, o] of this.views) {
        if (o.local || o.kind !== 0 || !Network.getBool(id, "alive")) continue;
        const dx = o.shown.x - me.x;
        const dz = o.shown.z - me.z;
        const d = Math.hypot(dx, dz);
        if (d < best) {
          best = d;
          this.yaw = (Math.atan2(dx, -dz) * 180) / Math.PI;
        }
      }
      this.botDist = best;
    }
    let dir = fwd.scale(z).add(right.scale(x));
    const moving = alive && dir.magnitude > 0.01;
    if (moving) dir = dir.normalized;
    const sprint = input.getKey("ShiftLeft") || input.getKey("ShiftRight");
    const speed = moving ? (sprint ? SPRINT : WALK) : 0;
    if (cc.isGrounded && this.vy <= 0) {
      this.vy = -1;
      if (alive && input.getKeyDown("Space")) this.vy = 5.5;
    } else this.vy -= 16 * dt;
    const before = view.go.transform.position;
    cc.move(dir.scale(speed * dt).add(new Vector3(0, this.vy * dt, 0)));
    const after = view.go.transform.position;
    // The controller applies a move after the script turn, so `after` equals `before` here: the speed
    // is how far the figure went since the last frame, smoothed so the blend does not flicker.
    const last = this.prevPos ?? before;
    const raw = new Vector3(before.x - last.x, 0, before.z - last.z).magnitude / Math.max(dt, 1e-4);
    this.prevPos = before;
    this.speedNow += (Math.min(raw, SPRINT + 2) - this.speedNow) * Math.min(1, 12 * dt);
    const moved = this.speedNow < 0.15 ? 0 : this.speedNow;

    // Face where it runs, or where it shoots.
    const firing = alive && (this.bot ? Math.sin(bt * 2.3) > 0.3 : input.getMouseButton(0) || input.getKey("KeyF"));
    // The mannequin model faces +z, so its body turn is 180 - heading when it aims.
    const aim = firing && !this.bot ? this.aimYaw(after) : this.yaw;
    if (firing) this.facing = Math.PI - (aim * Math.PI) / 180;
    else if (moving) this.facing = Math.atan2(dir.x, dir.z);
    let d = this.facing - ((this.facingDeg * Math.PI) / 180);
    d = Math.atan2(Math.sin(d), Math.cos(d));
    this.facingDeg += (d * 180) / Math.PI * Math.min(1, 14 * dt);
    if (view.body) view.body.transform.localRotation = Quaternion.euler(0, this.facingDeg, 0);
    view.animator?.setFloat("speed", moved);
    // In the air only after a moment off the ground: the controller flickers while walking.
    this.airTime = cc.isGrounded ? 0 : this.airTime + dt;
    view.animator?.setBool("air", this.airTime > 0.18);

    // What the others need: where I am, which way, how fast. The server checks the position.
    const wrote = Network.setVec3(this.localId, "pos", after.x, after.y, after.z);
    if (this.verbose && Time.time - this.lastLog < 0.02) console.log(`[net client] wrote pos (${after.x},${after.z}) ok=${wrote} ${wrote ? "" : Network.lastError()}`);
    Network.setNumber(this.localId, "yaw", this.facingDeg);
    Network.setNumber(this.localId, "speed", moved);
    Network.setBool(this.localId, "sprint", sprint);

    // Fire: yaw of the camera, and how long ago I saw the world the others are in (half the round
    // trip plus the interpolation delay).
    if (firing && Time.time - this.fireAt > 0.22) {
      this.fireAt = Time.time;
      const behind = Math.max(0, Network.rtt()) / 2 + (2 / TICK_RATE) * 1000;
      Network.rpc(this.localId, "fire", [`f:${aim}`,`f:${behind}`].join(SEP));
    }
  }

  private updateOthers() {
    for (const [id, v] of this.views) {
      if (v.local) continue;
      const pos = Network.getVec3(id, "pos");
      if (pos.length === 3) {
        v.shown = v3(pos);
        v.go.transform.position = v.shown;
      }
      if (v.kind === 1) {
        const q = Network.getQuat(id, "rot");
        if (q.length === 4) v.go.transform.rotation = new Quaternion(q[0]!, q[1]!, q[2]!, q[3]!);
      } else {
        v.go.enabled = Network.getBool(id, "alive");
        if (v.body) v.body.transform.localRotation = Quaternion.euler(0, Network.getNumber(id, "yaw"), 0);
        v.animator?.setFloat("speed", Network.getNumber(id, "speed"));
        // Others have no controller to ask: they are in the air when well above the ground.
        v.animator?.setBool("air", v.shown.y > 0.35);
      }
    }
  }

  private fillClientHud() {
    if (this.localId !== 0) {
      hud.hp = Network.getNumber(this.localId, "hp");
      hud.alive = Network.getBool(this.localId, "alive");
      hud.shield = Network.getBool(this.localId, "shield");
    }
    if (this.matchId !== 0) {
      hud.match = Network.getString(this.matchId, "state");
      hud.timeLeft = Network.getNumber(this.matchId, "timeLeft");
      hud.winner = Network.getString(this.matchId, "winner");
      hud.round = Network.getNumber(this.matchId, "round");
    }
    if (Time.time >= this.nextInfo) {
      this.nextInfo = Time.time + 0.25;
      const rows: Row[] = [];
      for (const [id, v] of this.views) {
        if (v.kind !== 0) continue;
        rows.push({
          name: Network.getString(id, "name"), kills: Network.getNumber(id, "kills"), deaths: Network.getNumber(id, "deaths"),
          ping: Network.getNumber(id, "ping"), me: id === this.localId,
        });
      }
      hud.rows = rows.sort((a, b) => b.kills - a.kills);
      hud.feed = hud.feed.filter((f) => f.until > Time.time);
      hud.status = Network.isConnected() ? "" : hud.status;
    }
  }

  private followCamera(dt: number) {
    const v = this.views.get(this.localId);
    if (!v) {
      this.yaw += 6 * dt;
      this.placeCamera(new Vector3(0, 2, 0), 34, 22, dt);
      return;
    }
    // Over the right shoulder: the player sits left of centre so the crosshair is on the world.
    const yawR = (this.yaw * Math.PI) / 180;
    const shoulder = new Vector3(Math.cos(yawR), 0, Math.sin(yawR)).scale(0.75);
    this.placeCamera(v.go.transform.position.add(new Vector3(0, 1.55, 0)).add(shoulder), this.distance, this.pitch, dt);
  }
}

/**
 * The weapon: a flashlight built from primitives, parented to the right hand bone so the run and
 * jump animations carry it. (The Unity Asset Store "Flashlight" package cannot be loaded by the
 * engine: it is a .unitypackage behind a login. Export it as glTF into assets/ and swap this for
 * `Model.load` + `instantiate({ parent: hand })` to use it.)
 */
/** A bone by name anywhere under `go` (`findChild` only looks at direct children). */
function findDeep(go: GameObject, name: string): GameObject | null {
  for (const c of go.children) {
    if (c.name === name) return c;
    const hit = findDeep(c, name);
    if (hit) return hit;
  }
  return null;
}

let flashMaterials:{ body: Material; head: Material; lens: Material } | null = null;
function holdFlashlight(body: GameObject): { pivot: GameObject; hand: GameObject } | null {
  const hand = findDeep(body, "hand_r");
  if (!hand) return null;
  flashMaterials ??= {
    body: Material.create({ color: new Color(0.12, 0.12, 0.14, 1), metallic: 0.6, smoothness: 0.6 }),
    head: Material.create({ color: new Color(0.7, 0.7, 0.74, 1), metallic: 0.9, smoothness: 0.8 }),
    lens: Material.create({ color: Color.black, emission: new Color(10, 2, 1.6, 1), unlit: true }),
  };
  const m = flashMaterials;
  // The hand bone's x axis runs along the fingers: the torch lies along it, lens forward.
  const pivot = new GameObject("Flashlight", { parent: hand, position: new Vector3(0.05, 0, 0.0) });
  const part = (mesh: string, mat: Material, x: number, length: number, width: number) => {
    const go = new GameObject("FlashlightPart", {
      parent: pivot,
      position: new Vector3(x, 0, 0),
      rotation: Quaternion.euler(0, 0, 90),
      scale: new Vector3(width, length / 2, width),
    });
    go.addComponent(MeshRenderer, { mesh, material: mat }).shadowCastingMode = "off";
  };
  part("cylinder", m.body, 0.0, 0.2, 0.045);
  part("cylinder", m.head, 0.13, 0.07, 0.07);
  part("cylinder", m.lens, 0.17, 0.01, 0.062);
  return { pivot, hand };
}

let beamCore: Material | null = null;
function beamMaterial(): Material {
  return (beamCore ??= Material.create({ color: Color.black, emission: new Color(14, 2.5, 2, 1), unlit: true }));
}

register({ Arena });
