# Multiplayer (`platypus_net`, `platypus_net_quic`, `Network`)

How a game's objects reach other machines. Code: `platypus_net`
([transport.rs](../crates/platypus_net/src/transport.rs), [loopback.rs](../crates/platypus_net/src/loopback.rs),
[conditioned.rs](../crates/platypus_net/src/conditioned.rs), [session.rs](../crates/platypus_net/src/session.rs),
[wire.rs](../crates/platypus_net/src/wire.rs), [schema.rs](../crates/platypus_net/src/schema.rs),
[replica.rs](../crates/platypus_net/src/replica.rs), [host.rs](../crates/platypus_net/src/host.rs)),
`platypus_net_quic` ([lib.rs](../crates/platypus_net_quic/src/lib.rs)), the script namespace
([network.rs](../crates/platypus_engine_api/src/network.rs)) and the app glue
([net_host.rs](../crates/platypus_app/src/net_host.rs)). The design, with the reasons for each choice and
the plan, is [research/multiplayer.md](research/multiplayer.md); this page says what the code does today.

## Layers

```
 script      Network.defineClass / spawn / setVec3 / rpc / takeEvents        (network.rs, thread-local state)
   |
 NetHost     one object, server or client: ticks, RPC checks, event list      (host.rs)
   |
 replica     ReplicaServer / ReplicaClient: spawns, state deltas, ownership   (replica.rs, schema.rs)
 session     handshake, player ids, tokens, heartbeats, timeouts, grace       (session.rs)
   |
 transport   Transport trait, four channels                                   (transport.rs)
   |         QuicTransport | LoopbackTransport | Conditioned<any of these>
```

Each layer sees only the one below. The replica halves are plain data structures driven by the caller
(`step`, `handle`); they never touch a transport. Everything runs on the game thread: the QUIC
transport has its own tokio runtime but hands events over inside `poll`.

Time is passed in (`Time` is a `Duration` since the run started), never read from the clock, so tests
and the fault wrapper are deterministic.

## Transport

[transport.rs](../crates/platypus_net/src/transport.rs): `send(peer, channel, bytes)`, `poll(now)`
(returns `Connected` / `Disconnected` / `Data` events in order), `disconnect`, `peers`, `max_unreliable`
(1200 bytes by default). Four channels:

| Channel | Promise | Used for |
|---|---|---|
| `Unreliable` | may be lost, duplicated, reordered | state deltas, acks, pings |
| `UnreliableSequenced` | may be lost; older than one already delivered is dropped | owner-written fields |
| `Reliable` | arrives, in order | handshake, spawns, RPCs |
| `ReliableUnordered` | arrives, any order | large one-off transfers |

`LoopbackTransport` (in-process, immediate, every channel reliable and ordered) is for tests and
single-machine games.

**QUIC** ([platypus_net_quic](../crates/platypus_net_quic/src/lib.rs)): `Unreliable` and
`UnreliableSequenced` are datagrams (the second carries a `u32` sequence; the receiver drops one older
than the newest it has seen), `Reliable` is one long-lived unidirectional stream per direction with
length-prefixed messages (so order is the stream's), `ReliableUnordered` a stream per message. ALPN is
`platypus-net/1`; keep-alive 2 s, idle timeout 15 s. A server has a self-signed `Identity`; a client
connects with a `Trust`: `Fingerprint(sha256)` pins that certificate (a different one never connects),
`InsecureDev` accepts any (local development only).

## Fault injection (`Conditioned`)

[conditioned.rs](../crates/platypus_net/src/conditioned.rs), behind the `net-debug` feature (on by
default in `platypus_net`, `platypus_engine_api` and `platypus_app`). `Conditioned<T>` wraps any
transport; each direction of each peer is a `Link`: latency, jitter, loss (independent, or in bursts:
a two-state Gilbert-Elliott model with a mean run length), duplication, reordering, a bandwidth cap and
a blackhole. The knobs live in a `Handle` that stays with the caller after the transport moves into a
session. One-off `Fault`s fire at the next `poll`: `Disconnect`, `Freeze(duration)` (nothing sent or
received, like a debugger stop), `CorruptNext(n)` (a bit flipped in each of the next `n` received
messages, which the codec must refuse).

Rules worth knowing:

- Reliable channels never lose a message, as in a real stack: a loss costs retransmit delays and the
  `Reliable` channel keeps order (a late message holds back the later ones). Only unreliable channels
  drop.
- All randomness comes from a seeded generator, so the same seed replays the same network.
- **Close waits for in-flight messages.** A `disconnect` from this side is held until what was queued
  for the peer has gone out (a goodbye or a rejection must arrive before the close), and a close
  coming from the peer is held until its already-delivered messages have been handed up. Without this,
  conditions that delay data would make a reject or a clean leave look like a bare disconnect.
- Sequenced messages older than one already delivered are dropped on the receiving lane.
- A bandwidth-capped lane tail-drops unreliable data queued more than 2 s ahead, so a flood cannot grow
  memory without bound.

## Session

[session.rs](../crates/platypus_net/src/session.rs): players over a transport. The server is player 0.
The messages are `HELLO`, `WELCOME`, `REJECT`, `PING`, `PONG`, `DATA` and `BYE`; everything above the
session travels inside `DATA`.

- **Handshake.** The client's hello carries the protocol version, the schema hash, the game version
  string, its name and an optional reconnect token. A mismatch is refused with a reason
  (`ProtocolMismatch`, `SchemaMismatch`, `VersionMismatch { server }`, `ServerFull`, `Malformed`) and the
  connection is closed. A connection that never says hello is dropped after `handshake_timeout`;
  packets that do not parse are counted in `bad_packets` and ignored.
- **Roles.** `PROTOCOL_VERSION` is 2: the hello carries a role flag. A client with
  `SessionConfig::spectator` joins as a spectator: it gets the world but its RPCs are refused. Spectators
  have their own limit, `max_spectators` (server, default 16), apart from `max_players`;
  `SessionEvent::PlayerJoined { spectator }` and `Session::is_spectator(player)` tell them apart.
- **Players and tokens.** A new player gets the next id and a random 128-bit token. When a connection
  is lost the player is `PlayerDropped` and the slot is held for `grace` (30 s by default); a client
  that says hello with that token inside the grace gets the same id (`resumed`). After the grace, or
  with a wrong token, it is a new player. A clean `leave` / `BYE` frees the slot at once
  (`PlayerLeft`). Held slots count against `max_players`, so a returning player always has a place.
- **Liveness.** Both sides ping every `heartbeat` (250 ms) on the unreliable channel and keep a
  smoothed round-trip time (80 % old, 20 % new). Silence longer than `timeout` (5 s) ends the
  connection with `TimedOut`.

`SessionEvent`s: `Connected`, `Rejected`, `Disconnected` (client); `PlayerJoined`, `PlayerDropped`,
`PlayerLeft` (server); `Message` (both).

Replication is by acknowledged-baseline deltas only: there is no per-field reliable mode, on purpose.
Every field converges because each state is a delta against what the client has acknowledged; a change
that must arrive every time and in order is an RPC.

## Schema and wire format

[schema.rs](../crates/platypus_net/src/schema.rs): a `Schema` is the classes of networked objects
(each a list of fields, in declaration order, which is the wire order; at most 64 per class because a
change mask is one number) plus the RPC definitions. It is the same on every peer, and `Schema::hash()`
(FNV-1a over a canonical text of names, types, quantisation, owner-write, interpolation and order) is
what the handshake compares. `check()` refuses duplicates and too many fields.

Field types: `bool`, `i32`, `f32`, `vec3`, `quat`, `str`, `ref` (another object's id, 0 is none). A
field can be quantised (`q=0.01`: floats go on the wire as whole multiples of the step, which also
decides when a change is big enough to send; the server stores the quantised value, so what it holds is
what clients see; `FieldDef::normalize` applies the same rounding), owner-written (`owner`) or
interpolated (`lerp`). A `quat` with `q=` is sent smallest-three (`pack_quat` / `unpack_quat`): the
three smaller parts at 10 bits each plus the index of the largest in one `u32`, 4 bytes, within 0.2
degrees; without `q=` it stays exact (16 bytes).

RPC definitions: a name, a target (`server`: a client calls it; `owner`: the server calls it on the
object's owner; `all`: the server calls it on every client that knows the object), `require_owner`
(default true for `server` RPCs; `anyone` turns it off) and `reliable` (default true; `unreliable`).

[wire.rs](../crates/platypus_net/src/wire.rs): a `Writer` and a `Reader` of fixed-width values, varints
and strings. The reader returns `None` on short or hostile input and never panics; all decoding above it
relies on that.

## Replication

[replica.rs](../crates/platypus_net/src/replica.rs). Message kinds: `SPAWN`, `DESPAWN`, `OWNER_CHANGED`
(reliable), `STATE` and `ACK` (unreliable), `OWNER_FIELDS` (unreliable sequenced).

**Server** (`ReplicaServer`). `spawn(class, prefab, owner)` makes an object with zero-value fields and
an id (never 0); `set` writes a field by name (the type must match). `step()` advances one tick and, per
player: despawns what left their interest, spawns what entered it (a spawn carries every field), then
sends a state delta.

**State deltas and the baseline rule.** A state message holds, per object, the fields that differ from
the newest state the client has *acknowledged*, not from the previous tick. A lost state therefore costs
nothing: the next one is still a delta against the same baseline. The server keeps the last 64 ticks of
values; a baseline older than that (or none acknowledged yet) means a full state (`base` tick 0). An
object spawned to the client after the baseline tick has no baseline entry, so all its fields are sent.

The client rebuilds each tick from its **stored copy of the baseline state** (the complete state it
holds for tick `base`), never from "whatever it holds now". Starting from the latest values would miss
a field that changed and changed back between the baseline and now, because the server compares
against the baseline and sees no difference. A state too large for one datagram is split into chunks
under 1000 bytes (`index` / `count` in the header); the client acknowledges a tick only once it has
every chunk.

**Why a state naming an unknown object is never acknowledged.** Spawns are reliable but travel
separately from states, so a state can arrive before the spawn it depends on. The client then cannot
apply it: it drops the partial tick and does not acknowledge it. Acknowledging would make the server
treat that state as the client's baseline, and the object's values (never received) would be missing
from every later delta. Once the spawn arrives the next state applies cleanly. Messages whose baseline
the client does not hold are ignored for the same reason.

**Interest.** `set_interest(|player, id, obj| bool)` decides per player which objects they see (default:
all). Leaving despawns the object on that client, entering spawns it.

**Owner fields.** A client may write a field marked `owner` on an object it owns
(`ReplicaClient::set_owned`); the write is visible to its own reads at once (the pending value wins over
the server's echo) and is sent as an `OWNER_FIELDS` message. The server reads the whole entry (to stay in
step with the stream) and then applies it only if the sender owns the object and the field is
owner-written; a forged message for another player's object, or for a server-only field, changes
nothing. `set_owner` on the server tells every client that knows the object and drops the old owner's
pending writes.

**Interpolation.** The client keeps complete states by tick. `interpolated(id, field, render_tick)`
blends the two states around a fractional tick for fields marked `lerp` (floats, vectors, quaternions
along the shorter arc), and gives the nearest state otherwise. `NetHost` draws `interpolation_delay`
(2 ticks) behind the newest tick plus the time since it arrived.

## NetHost

[host.rs](../crates/platypus_net/src/host.rs): one object that is a server or a client and owns the
session, the replica half and the RPC checks, so the engine makes one call a frame, `update(now)`, and
reads `NetEvent`s. A server steps replication at `tick_rate` (30 a second by default; at most five
catch-up steps per call, then it resynchronises); a client acknowledges states and sends its
owner-written fields. A returning player (`PlayerJoined` with `resumed`) is re-added to the replica
and gets every spawn again, still owning its objects. `NetEvent::PlayerJoined` carries `spectator`.

**Network time.** `tick_rate()`, `tick()` and `time()` give the server's tick clock: on the server its
own, on a client an estimate of the server's present (-1 before the first tick arrives).

**RPC checks** happen on the receiving side, against the schema:

- A client may only call `server` RPCs; the server only `owner` and `all` ones. The wrong direction or
  an unknown name is an `Err` for the caller.
- On the server, an incoming RPC is dropped (and counted in `dropped_rpcs`) unless the object exists,
  the RPC is a `server` one, and the sender owns the object or the RPC is `anyone`.
- At most `rpc_per_second` (120) client RPCs a player a second; the rest are dropped and counted.
- An RPC has at most 16 arguments; malformed packets (unknown id, absurd count, bad value tag) are
  dropped and counted, never delivered.
- `all` goes only to players that `knows` the object; a player that dropped is skipped.
- A spectator's RPCs are refused and counted in `dropped_rpcs`.

## The script API

`Network` ([network.rs](../crates/platypus_engine_api/src/network.rs), declared with `#[ts_api]`, so it is
in `types/platypus.d.ts`). The state is thread-local like the other namespaces; scripts poll.

**Defining.** Before connecting, on every peer, in the same order:

```ts
Network.defineClass("Player", "pos:vec3:q=0.01:lerp;hp:i32;aim:f32:owner;name:str;alive:bool");
Network.defineRpc("fire", "server");                 // a client calls it, the owner only
Network.defineRpc("shout", "server", "anyone");      // any player
Network.defineRpc("boom", "all", "unreliable");      // the server calls it on every client
```

Class text: `name:type[:q=0.01][:owner][:lerp]` fields joined by `;`. RPC flags: `anyone`, `unreliable`,
comma-separated. Bad text returns false and `Network.lastError()` says why; redefining a name replaces
it. The order decides the numbers on the wire, and `Network.className(n)` maps the `class` number of a
`spawned` event back to a name.

**Starting.** `Network.startServer(port, maxPlayers?, tickRate?)` (defaults 16 and 30),
`Network.connect(address, name)`, `Network.watch(address, name)` (joins as a spectator),
`Network.disconnect()`. These queue a request (`Request::Connect { spectator }`); the app serves it on the
next frame, because the app owns the sockets. `isServer`, `isClient`, `isConnected`, `isSpectator(player)`,
`role()` (`"server"`, `"client"` or `"none"`), `localPlayer` (-1 before connecting, 0 on a server),
`rtt(player?)` (ms, -1 unknown), `players()` and `playerName(p)` read the state. `Network.tickRate()`,
`Network.tick()` and `Network.time()` give the network time (the server's tick clock; -1 before the
first tick).

**Listen server.** Not a separate mode: a server whose script also plays as player 0.
`localPlayer()` is 0 on a server and `isMine` honours it. `examples/net-arena` does this with `--host`
(`host`, `hostFrame` and `send` in `src/game.ts`).

**Objects and fields.** Server: `spawn(class, prefab, owner?)` (returns the id, 0 on failure),
`despawn`, `setOwner`. Both: `owner(id)`, `isMine(id)`. `setNumber`, `setBool`, `setString`, `setVec3`,
`setQuat` write a field (on the server what clients receive; on a client an `owner` field of an object
it owns); `getNumber` (NaN unknown), `getBool`, `getString`, `getVec3`, `getQuat` read it, blended
between ticks on a client for `lerp` fields. A refused call returns false with a reason in `lastError`.

**RPCs.** `Network.rpc(id, name, args)` with `args` the arguments joined by `\u001f` (the unit
separator), each as `f:1.5`, `i:3`, `b:1`, `s:text`, `v:x,y,z`, `q:x,y,z,w` or `r:id`.

**Events.** `Network.takeEvents()` returns the events since the last call, one per line, fields joined
by `\u001f`:

| Line | Fields |
|---|---|
| `spawned` | id, class, prefab, owner |
| `despawned` | id |
| `owner` | id, owner |
| `rpc` | id, name, from, then one text-encoded argument each |
| `joined` | player, name, resumed (0 or 1), spectator (0 or 1) |
| `dropped` | player |
| `left` | player |
| `connected` | player, resumed |
| `rejected` | `protocol`, `schema`, `version <server's>`, `full` or `malformed` |
| `disconnected` | the reason (`Local`, `Remote`, `TimedOut`, `Error(..)`) |
| `listening` | port, certificate fingerprint (from the app, once a server is up) |
| `error` | message (a failed request: port in use, unreachable address) |

`Network.droppedRpcs()` and `Network.badPackets()` count refused RPCs and unparsable packets.

## How the app serves it

[net_host.rs](../crates/platypus_app/src/net_host.rs). `pump(now)` is called from `App::tick()` in
[main.rs](../crates/platypus_app/src/main.rs) each frame, before the fixed ticks: it takes the queued
requests, serves them, then calls `network::update(now)`, which runs `NetHost::update` and turns its
events into lines.

- `startServer`: makes a self-signed identity, listens on `0.0.0.0:<port>` (UDP; port 0 picks one),
  builds a `NetHost::server` and emits `listening <port> <fingerprint>`.
- `connect`: parses the address, connects a QUIC client and builds a `NetHost::client`. Addresses are
  `host:port` or `host:port#<64 hex digits>`, the second pinning the server's certificate (the
  fingerprint from its `listening` event). **Loopback addresses (`127.0.0.1`, `localhost`, `::1`) need no
  pin** (`Trust::InsecureDev`); any other address without one is refused.
- `disconnect` calls `network::close()` (a server kicks every player, a client leaves) and then drops
  the host, so the other side sees the goodbye at once instead of timing out. `net_host::shutdown`
  does the same when the app exits; a headless server also does it on Ctrl+C, Ctrl+Break or the
  console closing (Windows), SIGINT / SIGTERM elsewhere
  ([stop_signal.rs](../crates/platypus_app/src/stop_signal.rs) sets a flag, the headless loop ends).
- Failures reach scripts as `error` events and the log.

With `net-debug` the transport is wrapped in `Conditioned` so the debug calls have a handle; with no
conditions set the wrapper only passes messages through. A packaged game calls
`net_host::set_debug_allowed(false)` (in `main.rs`): no fault injector is made there and
`Network.hasDebug()` is false, so the feature can stay compiled in.

## Decorators (`@sync`, `@rpc`)

[net_decorators.rs](../crates/platypus_components/src/net_decorators.rs) writes the schema where the data
is, instead of as text passed to `defineClass` / `defineRpc`. A decorator is compile-time metadata, so
`Platypus::rewrite` (in `compile.rs`) rewrites the program text before it is parsed, as it does for
`register(..)`. Example: `examples/net-decorators/src/main.ts`.

```ts
class Player extends NetworkBehaviour {
  @sync({ quantize: 0.01, interpolate: true }) pos: Vector3 = Vector3.zero;
  @sync hp: number = 100;
  @sync({ owner: true }) yaw: number = 0;
  @rpc("server") fire(aim: number): void { /* runs on the server */ }
}
Player.defineNetwork();              // every peer, first: the schema
const p = Player.spawnNet(owner);    // the server: spawns, sets the initial values
p.hp = 50;                           // writes the replicated field
p.fire(1.5);                         // a client sends it; the server runs the body
p.netDispatch(name, args);           // an `rpc` event for this object: run the body
```

- `@sync(options)` fields: `number` (`f32`, or `i32` with `int: true`), `boolean`, `string`, `Vector3`,
  `Quaternion`. Options: `quantize`, `interpolate`, `owner`, `int`.
- `@rpc(target?, options?)` methods: target `"server"` (default), `"owner"` or `"all"`; options `anyone`,
  `unreliable`; parameters of the same types; returns `void`.
- The class must extend `NetworkBehaviour` (in the d.ts, added to the program when a decorator is used).
  The rewrite turns each `@sync` field into a getter and setter over `Network.get*` / `set*` of `netId`,
  each `@rpc` method into a wrapper that sends the call (or runs the body where the call lands here)
  beside the original, and adds `defineNetwork`, `spawnNet`, `netInit` and `netDispatch` to the class.
  `NetworkBehaviour.setListenServer(true)` makes a listen server also run the `owner` and `all` bodies.
- Limits: decorator options must be literals; an RPC string argument cannot contain the `0x1f`
  separator; spawn constructors take no required parameters; a client calling a `server` RPC needs
  `anyone: true` unless it owns the object.

## Debugging and testing

`Network.debugConditions(latencyMs, jitterMs, lossPercent, duplicatePercent?, bandwidthKbps?)` sets the
same conditions both ways (bursts of mean length 2); `Network.debugFault(text)` fires `disconnect`,
`freeze:<ms>`, `corrupt:<n>`, `blackhole:on` or `blackhole:off`; `Network.hasDebug()` says whether this
build can. Without `net-debug` they return false.

Tests do the same in Rust: replication tests run two clients through seeded loss
([replica.rs](../crates/platypus_net/src/replica.rs) tests), `NetHost` tests run over loopback wrapped
in `Conditioned` (80 ms, jitter, 15 % burst loss, duplication, reordering), and the QUIC tests use real
localhost sockets. Run them with `cargo test -j 4 -p platypus_net` (and `-p platypus_net_quic`,
`-p platypus_engine_api network`, `-p platypus_app net_host`).

## Running the arena

`examples/net-arena` is the testbed: a small arena shooter with a menu, bots and a debug panel
(schema in `src/schema.ts`, the map in `map.ts`, the game component in `game.ts`, the UI in `main.tsx`).

```bash
cd tools && cargo run -p platypus_build -- build ../examples/net-arena
# dedicated server, no window
cargo run --release -p platypus_app -- examples/net-arena --headless -- --server
# a client
cargo run --release -p platypus_app -- examples/net-arena -- --connect 127.0.0.1:7777
# a client that walks and shoots by itself
cargo run --release -p platypus_app -- examples/net-arena -- --connect 127.0.0.1:7777 --bot
# a listen server: a window that serves and plays as player 0
cargo run --release -p platypus_app -- examples/net-arena -- --host
# a spectator
cargo run --release -p platypus_app -- examples/net-arena -- --watch 127.0.0.1:7777
```

**Prediction.** `src/move.ts` is the movement as a pure function at a fixed 30 Hz step, the same code
on the server and every client (players walk by it, not a CharacterController). It knows only the houses
and parked cars (`SOLIDS`), so the server keeps a kinematic capsule on layer 8 on each player
(`ServerPlayer.body`, `makePusher`) that shoves the crates and the shovable car; players are not blocked
by props, and server shot rays skip layer 8 (mask `4294967295 - 256`). The client predicts, keeps its unconfirmed inputs (`pending`) and sends the last 10 each
step as an unreliable `input` RPC. The server walks each new one in order (`onInput`, with a step budget
so a client cannot run its clock fast) and writes `pos` (interpolated, for others), `rpos` / `rvy`
(exact) and `seq` (the last input used). The owner compares its predicted state at `seq` with `rpos`;
if it is 3 cm or more off, it takes the server's state and replays the unconfirmed inputs (`reconcile`),
fading the visual pop. `--desync` makes a client mispredict on purpose, for testing. Bots (`src/bot.ts`)
feed the same input path.

**Predicted beams.** A client draws its own beam at once when it fires (`predictBeam`: scenery by
`Physics.raycast`, other players as it sees them, the server's cylinder test) and skips the server's copy
when it arrives (the `predicted` list, expiring after 1 s; the `onRpc` beam branch). It is a predicted
*effect*; an engine-level predicted spawn of replicated objects (adopting the server's spawn by a
prediction key) is not built.

**Killcam.** The client records a tape (`src/killcam.ts`, 30 Hz, 6 s: `recordTape`) of every figure's
pose (`playerFrame`), the props' poses and the beams. When the local player dies (`alive` turns false,
seen by `watchDeath`; the `killer` field is the killer's net id) `startKillcam` / `playReplay` play the
last 3.5 s forward over the live scene with the camera behind the killer, then hold until the respawn
(`RESPAWN_SECONDS` is 5, counted from the death). Live beams and the fall animation are suppressed
meanwhile (`main.tsx` shows a killcam banner). It costs the network nothing and does not use
`Physics.snapshot` / `restore`.

Flags: `--desync` (mispredict on purpose), `--host`, `--watch <addr>`, `--verbose`.

`examples/net-decorators` is the same schema written with `@sync` / `@rpc` (see "Decorators").

A bot (`--bot`) is an ordinary client whose input comes from `src/bot.ts`, built on `platypus_ai`
([platypus-api.md](platypus-api.md), "Bot AI"): a `NavGrid` of the town (houses and cars from
`map.ts`'s `SOLIDS`) gives A* routes and cover checks, and a `Goap` planner chooses patrol, chase,
fight, take cover or recover each 0.3 s. A bot knows only what it has seen (13 m, clear ray) or heard
(a shot within 26 m), remembers it for 6 s, strafes in a ring while fighting, aims worse the further
away the target is, and retreats out of sight to heal when its health drops below 40.

Arguments after `--` reach the game as `Application.args`. With no `--server` or `--connect` the menu
chooses. `--headless` is described in [platform.md](platform.md) ("Headless runs").

## Not done yet

- An engine-level prediction / input-buffer helper: the arena's prediction (`move.ts`) is game code.
- Engine-level predicted spawns of replicated objects.
- Physics-prop rewind is an engine feature now (`Physics.snapshot` / `restore`, [platypus-api.md](platypus-api.md)), but the
  arena does not rewind props over the network.
- Steam transport, relays and matchmaking.

## Tests

| IDs | Covers | Where |
|---|---|---|
| `M8-NET-01..03` | Loopback; `Conditioned` links and faults | `loopback.rs`, `conditioned.rs` |
| `M8-NET-04` | Wire reader and writer on hostile input | `wire.rs` |
| `M8-NET-05..07` | Session handshake, liveness, tokens and grace, robustness | `session.rs` |
| `M8-NET-08..10` | QUIC channels, pinning and close, a session over QUIC | `platypus_net_quic` |
| `M8-REP-01..03` | Values, schema hash and checks, `lerp` | `schema.rs` |
| `M8-REP-13` | Smallest-three quaternions (4 bytes, within 0.2 degrees) | `schema.rs` |
| `M8-REP-04..12` | Deltas, loss convergence, unknown-object states, interest, owner fields, ownership, chunking, interpolation, garbage input | `replica.rs` |
| `M8-HOST-01..06` | Join and spawn, RPC rules, rate limit, malformed RPCs, resume, a bad link | `host.rs` |
| `M8-HOST-07..08` | Network time (`tick_rate` / `tick` / `time`), spectators (RPCs refused) | `host.rs` |
| `M8-NET-11` | Roles in the handshake, `max_spectators` | `session.rs` |
| `M8-API-01..05` | `Network` text formats, arguments, a script-driven server and client, requests, debug calls | `network.rs` |
| `M8-APP-01` | Address parsing and pinning | `net_host.rs` |
| `M8-APP-02` | Debug gating (`set_debug_allowed`) | `net_host.rs` |
| `M8-HLS-04` | Stop request ends the headless loop | `stop_signal.rs` |
| `M8-DEC-01..03` | The `@sync` / `@rpc` rewrite (unit tests) | `net_decorators.rs` |
| `M8-DEC-04` | Manual: run `examples/net-decorators` | `examples/net-decorators` |

The plan for each is in [TESTS.md](../TESTS.md).
