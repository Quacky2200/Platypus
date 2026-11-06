// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Tables: objects indexed by a key (`state[key] = !state[key]`) and `Record<string, V>` as a
// dictionary, for tests/lang/tables.rs. Each case (split at `//## name`) defines `function t():
// string`; the test compiles it with web_ui_ts (normal, GC-stress, GC-off) and compares the result
// with bun's (`bun bench/gen_cases_expected.ts tables_cases.ts`).
//## keyed_writes
const state = { fog: true, shafts: false, bloom: true };
const counts = { a: 0, b: 0, c: 0 };
function toggle(key: keyof typeof state): void {
  state[key] = !state[key];
}
function bump(k: "a" | "b" | "c", n: number): number {
  counts[k] += n;
  counts[k]++;
  return counts[k];
}
function t(): string {
  toggle("fog");
  toggle("shafts");
  toggle("shafts");
  const r = bump("b", 5) + bump("c", 1) + bump("b", 1);
  const keys: (keyof typeof state)[] = ["fog", "bloom"];
  for (const k of keys) toggle(k);
  return `${state.fog}${state.shafts}${state.bloom} ${counts.a} ${counts.b} ${counts.c} ${r}`;
}
//## tables_by_tag
const TINTS = { warm: 1.5, cool: 0.5, neutral: 1 };
type Tint = keyof typeof TINTS;
const NAMES: Record<Tint, string> = { warm: "Warm", cool: "Cool", neutral: "Neutral" };
const LIT: Record<Tint, number[]> = { warm: [1], cool: [], neutral: [2, 3] };
function t(): string {
  const order: Tint[] = ["cool", "warm", "neutral"];
  let s = "";
  for (const k of order) s += NAMES[k] + TINTS[k] + LIT[k].length + ",";
  NAMES.warm = "W";
  return s + NAMES["warm"];
}
//## string_dictionary
const MODE_NAMES: Record<string, string> = {
  linear: "Linear",
  exponentialSquared: "Exponential squared",
  off: "Off",
};
const scores: Record<string, number> = {};
function t(): string {
  const mode = "linear";
  scores["a"] = 1;
  scores["b"] = 2;
  scores[mode] = (scores[mode] ?? 0) + 10;
  scores["a"]! += 5;
  const missing = MODE_NAMES["nope"] ?? "dflt";
  const dynamic = ["off", "linear", "x"].map((k) => MODE_NAMES[k] ?? "?").join("|");
  return `${MODE_NAMES[mode]!} ${missing} ${dynamic} ${scores["a"]} ${scores["b"]} ${scores.linear} ${scores["zz"] === undefined}`;
}
//## record_struct_as_dictionary
type Pose = "bow" | "wink" | "spin";
const POSES: Record<Pose, number> = { bow: 1, wink: 2, spin: 3 };
function lookup(name: string): number {
  return (POSES as Record<string, number>)[name] ?? -1;
}
function t(): string {
  const dict: Record<string, number> = POSES;
  dict["extra"] = 9;
  return `${lookup("wink")} ${lookup("nope")} ${dict["spin"]} ${dict["extra"]} ${POSES.bow}`;
}
//## keys_that_name_some_fields
const state = { fog: true, lowRes: false, end: 12, mode: "linear", bloom: true };
function toggle(key: "fog" | "lowRes"): void {
  state[key] = !state[key];
}
function flip(key: "bloom" | "lowRes", times: number): void {
  for (let i = 0; i < times; i++) state[key] = !state[key];
}
function t(): string {
  toggle("fog");
  toggle("lowRes");
  flip("bloom", 3);
  flip("lowRes", 2);
  state.end += 1;
  return `${state.fog}${state.lowRes}${state.bloom} ${state.end} ${state.mode}`;
}
//## types_from_values_and_small_forms
const NAMES = ["boom", "pop", "ding"] as const;
type Sfx = (typeof NAMES)[number];
const FILE: Partial<Record<Sfx, string>> = { boom: "mp3" };
const TINTS = { warm: 1.5, cool: 0.5 };
type Tint = keyof typeof TINTS;
function mult(t: (typeof TINTS)[Tint]): number {
  return t * 2;
}
type Knot = [number, number, "c"?];
const KNOTS: Knot[] = [[0.5, 2], [1, 3, "c"], [2, 4]];
function ext(s: Sfx): string {
  return FILE[s] ?? "wav";
}
function t(): string {
  const corner = KNOTS.map((k) => (k[2] === "c" ? "C" : "-")).join("");
  const word = "héllo";
  const letters = `${word[0]}${word[1]}${word[4]}${word[5] ?? "!"}${word[10] ?? "?"}`;
  const lights: (string | null)[] = Array.from({ length: 3 }, () => null);
  lights[1] = "x";
  const sfx: Sfx = "pop";
  return `${ext("boom")}${ext(sfx)} ${mult(TINTS.warm)}${mult(TINTS.cool)} ${corner} ${KNOTS[1]![0]}${KNOTS.length} ${letters} ${lights.map((l) => l ?? "n").join("")}`;
}
