# Design: multiplayer (M8)

Status: proposal, 2026-11-08. Extends DESIGN.md §8.3 and `networking-and-files.md` §7 (scripts get no raw sockets;
multiplayer is engine-side). Nothing here is built yet.

Goal: Photon Fusion / Fish-Net / Lightyear class netcode for compiled TypeScript games, with Rust owning the world.
Steam does matchmaking and relay for now; a cross-play backend (accounts, matchmaking, leaderboards) comes later.

## 1. What we take from each

| From | We take | We change |
|---|---|---|
| **Photon Fusion** | Tick-based simulation, `[Networked]` state, input authority vs state authority, Host mode, lag compensation, interest management | No cloud dependency; no per-CCU cost; no editor-time code weaving |
| **Fish-Net** | Pluggable transports, per-field `[SyncVar]` options, prediction v2 (reconcile from server state), observers / conditions | Replication is driven by a schema, not reflection |
| **Lightyear** | Replication groups, pre-spawned (predicted) entities, interpolation and prediction as separate marker roles, link conditioner for testing | Objects are `GameObject` handles, not ECS entities |

Known sore points we design around from the start:
- Prediction that fights physics: reconcile by re-simulating rapier, not by snapping (see §6).
- Authority transfer that races with in-flight inputs: transfers are tick-stamped (§5).
- Hand-written serializers that drift: one schema (§3) generates TypeScript types, the Rust codec and the d.ts.
- Testing needs a real network: the in-process loopback transport and link conditioner come first (§9).

## 2. Layers

```
scripts   @sync / @rpc / NetworkBehaviour hooks            (TypeScript, compiled)
session   roles, authority, ownership, spawn, interest      (Rust: platypus_net)
replication  schema, snapshots, deltas, ack, prediction     (Rust: platypus_net)
transport trait: Loopback | QUIC | Steam | WebRTC           (Rust: platypus_net_transport_*)
```

Scripts never see sockets or connections: only a `NetworkId` (64-bit handle), peer ids and value types.

### Transport trait
- `send(peer, channel, bytes)`, `poll() -> events`; channels: `Unreliable`, `UnreliableSequenced`, `Reliable`
  (ordered), `ReliableUnordered`.
- **Loopback**: in-process, with a link conditioner (latency, jitter, loss, duplication, reorder). Used by every test.
- **QUIC** (`quinn`, with web-ui's new QUIC / HTTP/3 work as the reference for TLS and the tokio runtime):
  datagrams for the unreliable channels, one stream per reliable channel. Self-signed certificate pinning for
  dedicated servers, normal certificates when the server has a domain.
- **Steam** (`steamworks`): `ISteamNetworkingSockets` relay (no IP exposure, NAT traversal) plus lobbies.
- **WebRTC** (existing `web_api_webrtc` data channels): browser clients later.
- MTU-aware packetization in `platypus_net`, not in transports, so every transport behaves the same.

## 3. Schema and the script surface

Decorators on component classes, read by the compiler (`web_ui_ts` extension in `platypus_components/compile.rs`):

```ts
class Player extends NetworkBehaviour {
  @sync({ interpolate: true, quantize: 0.01 }) position = Vector3.zero;
  @sync health = 100;
  @sync({ owner: "client" }) aimYaw = 0;          // written by the owning client
  @sync({ reliable: true }) name = "";

  @input move: Vector2;                            // sampled per tick on the owner, sent to the server

  @rpc.server fire(dir: Vector3) { ... }           // client -> server, validated there
  @rpc.owner  hitMarker(damage: number) { ... }    // server -> owning client
  @rpc.all({ reliable: false }) muzzleFlash() { }  // server -> everyone in interest

  onNetworkSpawn() {}  onNetworkDespawn() {}
  onTick(dt: number) {}                            // fixed network tick, the simulation step
}
```

- **Schema:** the compiler emits, per class, an ordered field list (name, type, options) plus a stable hash.
  The Rust codec is built from it at load; connecting peers compare hashes and refuse mismatched builds.
  The same declarations feed the generated `types/platypus.d.ts`.
- **Types:** numbers (f32 / f64 / int widths via options), `bool`, `string`, the math value types (`Vector2/3/4`,
  `Quaternion`, `Color`), enums, `NetworkId` references, and fixed-shape arrays / records. No arbitrary objects:
  anything dynamic goes through an RPC with a typed payload.
- **Compression:** quantisation (`quantize`), smallest-three quaternions, varints, delta against the last
  acknowledged snapshot, and a dirty bitmask per object.
- **RPC rules:** `@rpc.server` is rejected unless the caller owns the object (or `{ requireOwner: false }`);
  arguments are validated against the schema; a per-peer rate limit applies. An RPC to a despawned object is dropped, not an error.

## 4. Roles and trust

| Role | Runs | Trusts |
|---|---|---|
| **Dedicated server** (`--headless --server`) | Simulation, physics, all authority | Nothing from clients except inputs |
| **Host** (listen server) | Server and a local client in one process | Same as server; the local client skips the network |
| **Client** | Prediction for owned objects, interpolation for the rest | The server |
| **Spectator** | Receives state only, sends nothing | n/a |

- Server-authoritative is the default and the only mode in the first release. Client authority is an explicit per-field
  option (`owner: "client"`) for things that cannot cheat the game (look direction, cosmetics), still bounds-checked by an optional `validate`.
- "First client priority" (Fusion's shared mode): an object may be `authority: "firstClaim"`; the first peer to claim it
  owns it until it releases or disconnects. Used for sandbox games and shared physics props. Off by default.
- Host migration is out of scope for the first release; a client-hosted game ends when the host leaves.
- Anti-cheat stance: the server never trusts positions, only inputs; the engine ships no kernel anti-cheat.

## 5. Replication

- **Tick:** fixed network tick (default 60 Hz simulation, 20-30 Hz snapshots, configurable per game), separate from render frames.
  The server stamps every snapshot with its tick; clients keep a tick offset estimate.
- **Spawn / despawn** are reliable, ordered events carrying the prefab GUID (from `.meta` files) and initial state.
  Predicted spawns (a client fires a projectile now): the client spawns with a temporary id, the server's
  spawn carries the matching prediction key, and the client adopts the server object without a pop (Lightyear's pre-spawn).
- **Snapshots:** per peer, delta against the last snapshot that peer acknowledged; objects outside its interest are skipped.
  Large worlds: relevancy by distance grid and by `@observers(condition)` predicate (Fish-Net conditions), evaluated on the server.
- **Ownership and authority transfer:** `giveOwnership(peer)` takes effect on a stated tick; inputs sent before it
  are still applied by the old owner's rules. Prevents the Fusion-style race where two peers briefly write one object.
- **Late join:** a full snapshot over the reliable channel, then deltas.
- **Time:** `NetworkTime` (server tick, round-trip time, jitter) as a `#[ts_api]` class.

## 6. Prediction, interpolation and physics

- **Remote objects:** snapshot interpolation about two snapshots behind (`interpolationDelay`, adaptive to jitter).
  Extrapolation is capped at a few ticks, then holds.
- **Owned objects (prediction):** the client runs `onTick` immediately with its input, records `(tick, input, state)`.
  On a snapshot containing its own object, it compares the server state at that tick with its recorded one;
  on a mismatch past a tolerance it rewinds to the server state and re-simulates the stored inputs. Small errors blend out over
  a few frames (visual smoothing, separate from the simulation state, so physics stays exact).
- **Physics:** `platypus_physics` (rapier) must be rewindable: snapshot and restore of bodies (position, rotation, velocities,
  sleep state, plus joint state) at a tick, and a deterministic fixed step. Plan: a body snapshot API first
  (`Physics.saveState(tick)` / `restoreState`), re-simulation of only the predicted bodies and their neighbours
  (islands) as the optimisation. The rapier plane-lock pins (CLAUDE.md "rapier plane locks") are part of the saved state.
- **Server-authoritative physics props** are interpolated on clients and never predicted unless owned.
- **Lag compensation:** the server keeps a short history of collider poses (about 250 ms); `Physics.raycastAt(tick, ...)` for hit scans,
  with the shooter's tick clamped to the history and to a maximum rewind to bound abuse.
- **Determinism:** not required for state replication. A lockstep mode (RTS style, Quantum's niche) is a later item and
  needs fixed-point maths we do not have; do not promise it.

## 7. Headless and dedicated servers

`platypus-app --headless` runs a game with no window, surface, renderer, audio device or UI renderer:
- Skips `host.rs` renderer selection and window creation; the platform loop becomes a fixed-step timer
  (tick-accurate sleep, then spin for the last 1 ms) running `Native` (scripts), scene, physics and net.
- Keeps the UI tree only if the game asks for it (a server rarely does); input events never arrive.
- Flags: `--server` (listen on a port, no local client), `--host` (server plus a local client; needs a window, so not with `--headless`),
  `--connect <addr>`, `--port N`, `--tick-rate N`, `--max-players N`, `--log-file`. Also `project.json` `net` section for defaults.
- Exit codes and signals (Ctrl+C, SIGTERM) shut down cleanly: despawn, flush, close.
- A stats line per second (tick time, peers, bytes in / out, snapshot size) to the log; later a small HTTP metrics endpoint.
- A game written once runs as server and client: `Network.isServer`, `Network.isClient`, `Network.isHost` plus the
  `@rpc` / `@sync` split mean no separate server codebase; code that must not ship to clients goes in a `*.server.ts`
  file the build leaves out of client bundles (later; the first release ships one bundle).
- Packaging: `platypus-build package --server` could write a headless-only exe (no wgpu / ANGLE), several MB smaller. Later.

## 8. Matchmaking, now and later

**Now (Steam):** lobbies for browsing, creating and joining; lobby data carries the game version and schema hash;
the Steam transport carries the game traffic over relay. Rich presence and invites come with it. Achievements and
Workshop are separate TODO items (DESIGN §8.3).

**Later: a cross-play backend** ("Platypus Online"), written down now so the transport and session layers leave room for it:
- Accounts and identity that link Steam, and other platform ids (Epic, console later) to one player id.
- Matchmaking (skill and region queues, party support), lobbies that work across platforms, dedicated-server allocation.
- Leaderboards and stats, cloud saves, friends, and bans.
- Tradeoff to decide then: build on Nakama (Apache 2.0; matchmaker, leaderboards, storage, already proven) or our own service in Rust.
  Leaning to Nakama behind a thin client API we own, so games never depend on its wire format.
- Needs: the QUIC transport with relay servers (so non-Steam players connect without port forwarding), and auth tickets
  (`networking-and-files.md` "Later": Steam auth tickets for game backends).

## 9. Testing

All by ID in `TESTS.md` (`M8-NET-nn`):
- Loopback + link conditioner: convergence under loss, reorder, duplication; late join; schema mismatch refused.
- Replication: delta correctness against full state (property test, random mutations); interest filtering;
  authority transfer on a given tick; RPC permissions and rate limits.
- Prediction: a scripted input run with 100 ms latency and 5 % loss ends with client state equal to the server within tolerance;
  a forced misprediction reconciles without a visible snap beyond N cm.
- Physics: save / restore round trip is bit-exact; a re-simulation equals a straight run.
- Headless: server for N ticks with `--frames`, two loopback clients, no GPU (CI-able without a display).
- Soak: 32 peers, 1000 objects, snapshot size and server tick time budgets as benchmark rows in `docs/benchmarks.md`.
- A real QUIC test on localhost (one test, not the matrix) and a manual Steam run (needs the user).

### 9.1 Fault injection (debug and tests only)

A `Conditioned<T: Transport>` wrapper that implements the transport trait over any other transport (loopback, QUIC, Steam), so
the session, replication and prediction code cannot tell it from the real thing:
- Per direction and per peer: base latency, jitter (distribution: uniform, normal, spiky), loss (random and burst, Gilbert-Elliott),
  duplication, reordering, bandwidth cap with a queue, MTU clamp, and a one-way "blackhole" (the packets vanish but the connection looks open).
- Connection faults: hard disconnect, silent timeout (stop delivering in both directions), reconnect after N seconds,
  a handshake that stalls, a corrupted packet (flip bits, to check the codec rejects it), a peer that floods or sends invalid RPCs.
- Time faults: a frozen peer (the process stops ticking for N ms, as in a debugger or a stall), clock drift.
- Deterministic: a seeded RNG and a recorded script of faults (`fault-script.json`: "at tick 600, peer 2: 300 ms +- 80, 10 % loss for 5 s"),
  so a failing run replays exactly.
- Compiled only with the `net-debug` cargo feature, or when `PLATYPUS_NET_FAULTS` is set in a debug build, and never in a
  packaged release exe (`platypus-build package` refuses to build with the feature). Scripts cannot reach it; the arena example's
  panel talks to it through a debug-only `NetDebug` `#[ts_api]` class that does not exist in release.
- The server must survive all of it, and says so in tests: no panic, no unbounded queue, no stuck peer slot; a timed-out peer is
  dropped after `timeout` and its objects despawn (or are released, per object option); a returning peer within a grace window
  resumes with a full snapshot rather than a new identity (reconnect token).

## 9.2 Example: `examples/net-arena` (the testbed)

Built from `examples/fog-white-room` (the endless white room, as in Assassin's Creed's loading screen), turned into a bounded map:
- **Look:** a clinical white town: several white houses, a few white cars and a street between them, all on a white floor that
  blends into white fog, with red lasers (the room's fog and light-shaft settings reused; the lasers are `Light` beams and `LineRenderer`s
  with the volumetric haze catching them). The visual style stays "endless": the fog hides the edge.
- **Playable area:** the "run forever" of the white room is removed. A rectangle of about 60 x 60 m is the playable area, enclosed by invisible
  box colliders; the fog starts a few metres before the wall so the world still reads as unbounded (like `fog-raider`'s tomb fog distance).
  Houses, cars and crates are static colliders and a few dynamic rapier bodies (doors, crates, a car you can shove) so physics replication gets exercised.
- **Play:** a third-person character (the white room's mannequin) with a laser gun: hit-scan red beam (lag compensated, §6), a short
  cooldown, health, respawn at server-chosen spawn points; a score line. Kept small on purpose: it exists to test the netcode.
- **Match flow (server-owned, replicated):** a `Match` object with `@sync` state: `lobby` -> `warmup` -> `running` -> `roundEnd` -> back to `running`
  (or `lobby`), with a configurable round time and kill limit; the host or server starts it.
  - **Spawn:** the server picks a spawn point (farthest from living enemies, not in a laser's line of sight), spawns the player object with a
    short spawn protection, and tells the owner by `@rpc.owner`. Late joiners spawn into the running round.
  - **Die:** health reaches 0 on the server only; the player despawns into a ragdoll or a fade (cosmetic, `@rpc.all`), becomes a spectator of
    their killer for a respawn delay (e.g. 3 s), then respawns. A disconnect during the round removes the player but keeps their score row for the
    round if they return within the reconnect grace window (§9.1).
  - **Kills:** the server records `(killer, victim, weapon, tick)` for each hit it validates (lag compensated, §6); suicides (the boundary,
    a car shove into a laser) count as a death with no killer. A kill feed is an `@rpc.all` event, not state.
  - **Session leaderboard:** a replicated `Scoreboard` of per-player rows (`name`, `kills`, `deaths`, `streak`, `ping`, `team` later), sorted
    client side; held in memory for the session only, nothing saved (persistent leaderboards belong to the later cross-play backend, §8).
    Shown on Tab and on round end with the winner; `ping` comes from `NetworkTime`.
- **Roles:** the same game runs as dedicated server (`--headless --server`), host (listen server) or client.
- **UI (React, `useState` etc.):** a main menu with Host and Join, a field for the address (default `127.0.0.1:7777`), a list of
  localhost lobbies, then an in-game overlay. "Matchmaking" is localhost only for now: the host announces on a UDP loopback / LAN
  discovery port (in the engine's transport, not scripts, as in `networking-and-files.md` §7), and Join lists what it hears or takes
  an address. The lobby API is the one Steam lobbies fill in at M8.f, so the menu does not change then.
- **Network panel (debug builds):** a collapsible panel showing round-trip time, jitter, loss, snapshot rate and size, tick offset,
  prediction error and reconciliation count, interpolation delay, and objects and bytes per peer; sliders and buttons that drive the
  fault injection of §9.1 (latency, jitter, loss, duplicate, bandwidth, disconnect, freeze, reconnect) for the local peer or, on the server,
  for a chosen peer. Buttons to load a saved fault script.
- **Scenarios used by tests:** two to four peers on one machine (several windows, or headless clients driven by scripted input) for each
  fault in §9.1: 200 ms lag shooting, 20 % loss while crossing a door, a host that freezes for a second, a client that drops and returns within
  and outside the grace window, a player who joins late into a moving car, a wall run with high jitter (prediction against the invisible boundary).
- **Match scenarios:** a kill while the victim is lagging is credited once and only once; two simultaneous kills order by server tick;
  the victim disconnecting mid-death-delay respawns correctly on return; a killer who leaves still keeps the kill in the scoreboard for the round;
  late join mid-round shows the full current scoreboard; the kill limit ends the round for every peer on the same tick.
- **Screenshot runs** (`--screenshot`) render the menu and a lobby with simulated peers, for `docs/progress/`.

## 9.3 Example: `examples/net-voice` (voice and camera experiment)

Same white town and match as `net-arena` (shared source, not a copy: the arena's map and match code become a module the second example
imports), plus proximity voice chat and live camera feeds. A fun experiment that also stress-tests the media paths of web-ui.
- **Voice:** each player's microphone is captured (`getUserMedia({ audio })` from web-ui's `web_api_media` / `web_api_webrtc` follow-ups, TODO
  Part 3: media tracks and `getUserMedia` are not built yet), encoded as Opus, and played back as a **spatial** `AudioSource` at the speaker's
  character (the engine's kira spatial emitters), so voices fade with distance and come from their direction. A talking indicator over the head and
  in the scoreboard; push-to-talk (default) or voice activation; mute and volume per player; a "radio" key that makes a voice global to your team later.
- **Camera:** the webcam appears as a live image (`web_api_media` live images, like `<video>` frames) on an in-world screen: a floating
  monitor above each player's head, or the white town's wall screens. Frames are small (e.g. 160 x 120 at 10-15 fps, VP8 or similar from the media
  work) and are sent only to peers within range, so the cost scales with the neighbourhood, not the lobby.
- **Transport choice (an experiment, so both are tried and measured):**
  1. **WebRTC media tracks** through `web_api_webrtc` between peers (mesh, fine for 4 to 8 players), signalled over the game's reliable channel
     (the server relays offer / answer / ICE; it never sees the media).
  2. **Our own channel:** Opus frames (20 ms) as unreliable datagrams on the game's QUIC / Steam transport, forwarded by the server only to
     peers in range (an SFU in the dedicated server), with a jitter buffer on the receiver. Works on Steam relay and needs no ICE; video the same way as small keyframe-plus-delta packets.
  The comparison (latency, bandwidth, behaviour under the `Conditioned` faults of §9.1, CPU) goes in the doc once measured; the loser stays
  as an option only if it is useful (WebRTC for browsers).
- **Privacy and safety (not optional):** the microphone and camera are off until the player turns them on in the menu and the OS permission
  is granted; a visible "you are live" indicator is always shown while capturing; nothing is recorded or saved; a muted or blocked player's
  stream is dropped at the receiver; the server can silence a peer; camera and voice never start from a script without a user gesture (a click or key).
  Capture follows `networking-and-files.md` §5's policy of asking the player, not the script.
- **Debug and tests:** fault injection applies to voice (jitter buffer under 200 ms +- 100 ms jitter and 10 % loss; talk through a disconnect and
  reconnect); a synthetic test source (a tone or a recorded clip as the "microphone", a test pattern as the "camera") so CI needs no hardware;
  `--headless` clients can carry synthetic voice to check the server forwarding and range culling.
- **Open questions:** audio capture in `platypus_audio` (cpal input) and echo cancellation (WebRTC's AEC is the usual answer; without it,
  headphones are assumed and the example says so); Opus encoding crate choice (`opus` binding vs a pure-Rust encoder) and its licence for
  `tools/platypus_notices`; video codec availability (the media work decodes VP8; encoding is not built).

## 10. Milestones

1. **M8.a Headless:** `--headless` fixed-step host, no GPU; tests run a game N ticks.
2. **M8.b Transport + session:** `platypus_net` (trait, loopback, `Conditioned` fault injection §9.1, channels), QUIC transport, connect / disconnect / handshake with schema hash, timeouts and reconnect tokens.
3. **M8.c Replication:** schema from the compiler, `@sync`, spawn / despawn, snapshots and deltas, interest, `NetworkTime`.
4. **M8.d RPC and ownership:** `@rpc.*`, `@input`, authority and ownership transfer, roles, `NetworkBehaviour` hooks.
5. **M8.e Prediction:** rapier save / restore, reconcile, interpolation, lag compensation, predicted spawns.
6. **M8.f Steam:** `steamworks` transport and lobbies, behind the lobby API the arena's localhost menu already uses.
7. **M8.g Tools:** editor network panel (peers, bandwidth, per-object cost), server metrics.

`examples/net-arena` (§9.2) grows alongside: the map and the menu with the loopback transport at M8.b, replicated players at M8.c,
the laser gun and ownership at M8.d, prediction and lag compensation at M8.e, the debug network panel and fault scenarios as each lands.
8. **Later:** WebRTC browser clients, cross-play backend (§8), lockstep mode, host migration, server-only code split, a headless-only package.

## 11. Risks and open questions

- **Rewindable rapier** is the largest unknown: restoring a world exactly (including solver warm-start data) may not be possible;
  if not, reconciliation uses state error blending instead of exact re-simulation for physics bodies. Spike first (M8.e).
- **GC and ticks:** compiled scripts run on the script thread with a precise GC; a server tick must not stall on a collection.
  Measure under `PLATYPUS_TS_GC=stress` and keep per-tick allocation low (reuse the decoded state buffers).
- **Compiler work:** decorator metadata and the generated codec touch `web_ui_ts` (separate repo); keep the schema emission in
  `platypus_components` where possible and ask web-ui only for a generic "decorator metadata" hook.
- **Tick rates** differ between simulation and snapshot; deciding the defaults needs the arena example, not argument.
- **Licensing and Steam:** `steamworks` needs the Steam SDK redistributable (licence review in `tools/platypus_notices`).
