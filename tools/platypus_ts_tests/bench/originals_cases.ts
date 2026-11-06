// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Language features the original (QuickJS-era) example sources need, for tests/lang/originals.rs.
// Each case (split at `//## name`) defines `function t(): string`; the test compiles it with
// web_ui_ts (normal, GC-stress, GC-off) and compares the result with bun's
// (`bun bench/gen_cases_expected.ts originals_cases.ts`).
//## generic_void_from_closure_body
function under<T>(root: number, build: () => T): T {
  return build();
}
let n = 0;
class Face {
  private total = 0;
  constructor() {
    under(1, () => this.build());
  }
  private build(): void {
    this.total += 5;
    n++;
  }
  get(): number {
    return this.total;
  }
}
function t(): string {
  const f = new Face();
  const v = under(2, () => 7);
  return `${f.get()} ${n} ${v}`;
}
//## tagged_unions
interface O {
  scale?: number | [number, number];
  life?: number;
}
function size(o: O, t: number): number {
  return typeof o.scale === "number"
    ? o.scale
    : o.scale
      ? o.scale[0] + (o.scale[1] - o.scale[0]) * Math.min(1, t / (o.life ?? 1))
      : 1;
}
function make(s?: number | [number, number]): O {
  return { scale: s ?? 0.4 };
}
function describe(x: number | string | string[]): string {
  if (typeof x === "string") return "s:" + x;
  if (Array.isArray(x)) return "a:" + x.join("+");
  return "n:" + (x + 1);
}
function t(): string {
  const a = size({ scale: 2 }, 0.5);
  const b = size({ scale: [1, 3], life: 2 }, 1);
  const c = size({}, 1);
  const d = make([5, 6]);
  const e = make();
  let k = 0;
  if (Array.isArray(d.scale)) k = d.scale[1]!;
  const kinds = [typeof d.scale, typeof e.scale];
  return `${a} ${b} ${c} ${size(d, 0)} ${size(e, 0)} ${k} ${kinds.join(",")} ${describe(3)} ${describe("x")} ${describe(["p", "q"])}`;
}
//## callback_return_annotation
type PoseName = "bow" | "wink" | "pump" | "watch";
interface Star {
  code: string;
  name: string;
  poses: PoseName[];
  look?: "eyes" | "googly";
  chip: string;
}
const ROWS: [string, string][] = [
  ["a", "A"],
  ["b", "B"],
];
const CAST: Star[] = [
  ...ROWS.map(([code, name]): Star => ({ code, name, poses: ["bow", "pump"], chip: code })),
  { code: "eyes", name: "Eyes", poses: ["watch"], look: "eyes", chip: "1f440" },
];
function t(): string {
  return CAST.map((s) => s.name + (s.look ?? "-") + s.poses.join("/")).join(",");
}
//## return_type_and_utility_types
function store<T>(init: T) {
  let value = init;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set(next: T) {
      value = next;
      listeners.forEach((f) => f());
    },
    on(f: () => void) {
      listeners.add(f);
    },
  };
}
function makePair(a: number, b: string): { n: number; s: string } {
  return { n: a, s: b };
}
interface Slot {
  code: ReturnType<typeof store<string>>;
  pair: ReturnType<typeof makePair>;
  args: Parameters<typeof makePair>;
}
type Mode = "linear" | "exp" | "off";
type Fog = Exclude<Mode, "off">;
type OnlyOff = Extract<Mode, "off" | "other">;
function describe(m: Fog): string {
  return m === "linear" ? "L" : "E";
}
async function later(): Promise<number> {
  return 4;
}
function t(): string {
  const code = store<string>("a");
  const seen: string[] = [];
  code.on(() => seen.push(code.get()));
  const s: Slot = { code, pair: makePair(2, "x"), args: [7, "y"] };
  s.code.set("b");
  s.code.set("c");
  const off: OnlyOff = "off";
  const maybe: string | null = "m";
  const sure: NonNullable<typeof maybe> = maybe!;
  let w: Awaited<Promise<number>> = 3;
  w += 1;
  return `${s.code.get()} ${seen.join("")} ${s.pair.n}${s.pair.s} ${s.args[0]}${s.args[1]} ${describe("linear")}${describe("exp")} ${off} ${sure} ${w}`;
}
//## typed_array_constructor_and_local_generic_arrow
class Box {
  constructor(public n: number) {}
}
function t(): string {
  const more = <T extends Float32Array | Uint32Array>(a: T): T => {
    const b = new (a.constructor as new (n: number) => T)(a.length * 2);
    b.set(a);
    return b;
  };
  let f = new Float32Array(2);
  f[0] = 1.5;
  f[1] = 2.5;
  let u = new Uint32Array(1);
  u[0] = 7;
  f = more(f);
  u = more(u);
  u = more(u);
  const bx = new Box(3);
  const bx2 = new (bx.constructor as new (n: number) => Box)(9);
  return `${f.length} ${f[0]} ${f[1]} ${f[3]} ${u.length} ${u[0]} ${u[3]} ${bx2.n}`;
}
//## self_referential_const_object
const log: string[] = [];
const sfx = {
  preload(): void {
    log.push("preload");
  },
  play(name: string, volume = 1, rate = 1): number {
    sfx.preload();
    return volume * rate * name.length;
  },
  stopAll(fade = 0.4): void {
    for (const n of ["a", "bb"]) sfx.play(n, fade);
  },
};
function t(): string {
  const a = sfx.play("abc", 2);
  sfx.stopAll();
  return `${a} ${log.length} ${log[0]}`;
}
//## object_literal_getter
interface Effects {
  count: number;
}
interface Ctx {
  at(): number;
  scene: Effects;
}
class Show {
  ctx: Ctx;
  effects: Effects | undefined;
  constructor() {
    const self = this;
    this.ctx = {
      at: () => 5,
      get scene() {
        return self.effects!;
      },
    };
    this.effects = { count: 3 };
  }
}
function t(): string {
  const show = new Show();
  const first = show.ctx.scene.count;
  show.effects = { count: 8 };
  return `${first} ${show.ctx.scene.count} ${show.ctx.at()}`;
}
//## string_and_object_logical_and
interface Act {
  name: string;
}
function named(list: Act[], n: string): Act | undefined {
  return list.find((a) => a.name === n);
}
const ACTS: Act[] = [{ name: "a" }, { name: "b" }];
function weighted(list: Act[]): Act {
  return list[1]!;
}
function t(): string {
  const none: string | undefined = undefined;
  const some: string | undefined = "a";
  const r1 = (none && named(ACTS, none)) || weighted(ACTS);
  const r2 = (some && named(ACTS, some)) || weighted(ACTS);
  const r3 = ("" && named(ACTS, "a")) || weighted(ACTS);
  return `${r1.name}${r2.name}${r3.name}`;
}
//## large_record_as_dictionary
const NAMES = [
  "a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9",
  "b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7", "b8", "b9",
  "c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8", "c9",
  "d0", "d1", "d2", "d3", "d4", "d5", "d6", "d7", "d8", "d9",
  "e0", "e1", "e2", "e3", "e4", "e5", "e6", "e7", "e8", "e9",
  "f0", "f1", "f2",
] as const;
type Name = (typeof NAMES)[number];
const FILE: Partial<Record<Name, string>> = { a3: "mp3" };
const CUT: Partial<Record<Name, number>> = { f2: 6 };
function t(): string {
  let s = "";
  for (const n of NAMES) s += FILE[n] ?? "wav";
  const cut = CUT["f2"];
  const none = CUT["a0"];
  return `${s.length} ${s.slice(0, 12)} ${cut} ${none === undefined}`;
}
//## strings_become_union_members
type Sfx = "tick" | "ding" | "tada";
const sounds = new Map<Sfx, number>();
function play(n: Sfx): string {
  return n === "ding" ? "D" : n === "tick" ? "T" : "?";
}
const MODES: (Sfx | "off")[] = ["tada", "ding", "tick", "off"];
const state = { mode: MODES[0]! };
function cycle(): void {
  state.mode = MODES[(MODES.indexOf(state.mode) + 1) % MODES.length]!;
}
function t(): string {
  sounds.set("tick", 1);
  sounds.set("ding", 2);
  let s = "";
  for (const n of [...sounds.keys()]) s += play(n);
  const seen: string[] = [];
  for (let i = 0; i < 5; i++) {
    seen.push(state.mode);
    cycle();
  }
  return `${s} ${seen.join(",")}`;
}
