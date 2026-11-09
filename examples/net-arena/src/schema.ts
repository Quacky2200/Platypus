// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// What the server and every client agree on: the networked classes and RPCs. Both sides run this
// before connecting, in the same order (the handshake compares a hash of it, so a build that
// differs is refused). The `@sync` / `@rpc` decorators will write these calls for you.

/** Metres a player walks and sprints a second. */
export const WALK = 5;
export const SPRINT = 8;
/** The most a player may cover in a second before the server calls it a teleport (with slack). */
export const MAX_SPEED = SPRINT * 1.6;
export const START_HP = 100;
export const SHOT_DAMAGE = 34;
/** Health comes back slowly: after REGEN_DELAY seconds unhurt, REGEN_RATE points a second. */
export const REGEN_DELAY = 4;
export const REGEN_RATE = 6;
export const KILL_LIMIT = 10;
export const ROUND_SECONDS = 180;
export const RESPAWN_SECONDS = 3;
export const SHIELD_SECONDS = 2;
/** Server ticks a second of replication. */
export const TICK_RATE = 30;
/** The furthest back in time a shot may rewind its targets, seconds. */
export const MAX_REWIND = 0.25;

export function defineNetwork(): void {
  const ok = [
    // The player: its owner writes where it is and what it does; the server owns the score.
    Network.defineClass(
      "Player",
      [
        "pos:vec3:q=0.01:owner:lerp",
        "yaw:f32:q=0.5:owner:lerp",
        "speed:f32:q=0.1:owner",
        "sprint:bool:owner",
        "hp:i32",
        "kills:i32",
        "deaths:i32",
        "alive:bool",
        "shield:bool",
        "ping:i32",
        "name:str",
      ].join(";"),
    ),
    // A prop the server simulates (crates, a shovable car): clients only draw it.
    Network.defineClass("Prop", "pos:vec3:q=0.01:lerp;rot:quat:lerp;kind:i32"),
    // The match: one object everybody can read.
    Network.defineClass("Match", "state:str;timeLeft:f32:q=0.1;winner:str;round:i32"),
    // A client asks to shoot: yaw (degrees), and how far back in time it saw the world (ms).
    Network.defineRpc("fire", "server"),
    // The server shows everyone a beam: from, to, shooter id, whether it hit a player.
    Network.defineRpc("beam", "all", "unreliable"),
    // The kill feed.
    Network.defineRpc("feed", "all"),
    // You were hit (owner only) / the server moved you (respawn, or a rejected move).
    Network.defineRpc("hurt", "owner"),
    Network.defineRpc("teleport", "owner"),
  ];
  if (ok.includes(false)) throw new Error(`network schema: ${Network.lastError()}`);
}
