// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The networking schema written where the data is: `@sync` fields and `@rpc` methods of a
// `NetworkBehaviour` class instead of `Network.defineClass("Counter", "value:i32;...")` text and
// hand-made RPC encodings. `docs/multiplayer.md`, "Decorators".
//
// One server and one client over QUIC (the same program, a flag apart):
//   platypus-app examples/net-decorators --headless -- --server
//   platypus-app examples/net-decorators --headless -- --connect 127.0.0.1:7790
// The client calls `bump` twice a second (a `server` RPC: its body runs on the server), the server
// announces the value to everyone (an `all` RPC), and the client reads the replicated `value`.

const PORT = 7790;
const SEP = "\u001f";

class Counter extends NetworkBehaviour {
  /** Whole numbers, replicated from the server to everyone. */
  @sync({ int: true }) value: number = 0;
  @sync name: string = "counter";
  /** Sent in centimetres and blended between ticks on clients. */
  @sync({ quantize: 0.01, interpolate: true }) pos: Vector3 = Vector3.zero;

  /** A client calls it; the body runs on the server (`anyone`: not only the object's owner may call it). */
  @rpc("server", { anyone: true }) bump(by: number, who: string): void {
    this.value += by;
    this.pos = new Vector3(this.value, 0, 0);
    console.log(`[server] ${who} bumped ${this.name} to ${this.value}`);
  }

  /** The server calls it; the body runs on every client. */
  @rpc("all") announce(text: string): void {
    console.log(`[client] ${text}`);
  }
}

const isServer = Application.args.includes("--server");
Counter.defineNetwork();
if (isServer) Network.startServer(PORT);
else Network.connect(`127.0.0.1:${PORT}`, "client");

const objects = new Map<number, Counter>();
let started = Time.time;
let nextCall = 0;

/** What `Network.takeEvents` says, one event per line, fields apart by `SEP`. */
function events(): string[][] {
  const text = Network.takeEvents();
  if (text === "") return [];
  return text.split("\n").map((line) => line.split(SEP));
}

setInterval(() => {
  const now = Time.time;
  for (const e of events()) {
    const kind = e[0];
    if (kind === "listening") {
      const c = Counter.spawnNet(0);
      objects.set(c.netId, c);
      console.log(`[server] counter ${c.netId} is up`);
    } else if (kind === "spawned") {
      // The client makes the object that stands for the spawned one.
      const c = new Counter();
      c.netId = parseFloat(e[1] ?? "0");
      objects.set(c.netId, c);
    } else if (kind === "rpc") {
      // [id, name, from, args...]: whoever the call is for runs its body.
      objects.get(parseFloat(e[1] ?? "0"))?.netDispatch(e[2] ?? "", e.slice(4));
    } else if (kind === "error") {
      console.log(`[net] ${e[1] ?? ""}`);
      Application.quit();
    }
  }
  for (const c of objects.values()) {
    if (isServer && now >= nextCall) c.announce(`the value is ${c.value}`);
    if (!isServer && now >= nextCall) c.bump(1, "the client");
  }
  if (now >= nextCall) nextCall = now + 0.5;
  if (!isServer && now - started > 4) {
    for (const c of objects.values()) console.log(`[client] done: value=${c.value} pos=${c.pos.x}`);
    Application.quit();
  }
}, 20);
