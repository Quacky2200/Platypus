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
/** A killcam of 3.5 seconds fits in it, and a moment to look at the end of it. */
export const RESPAWN_SECONDS = 5;
export const SHIELD_SECONDS = 2;
/** Server ticks a second of replication. */
export const TICK_RATE = 30;
/** The furthest back in time a shot may rewind its targets, seconds. */
export const MAX_REWIND = 0.25;

export function defineNetwork(): void {
  const ok = [
    // The player. The server walks it from the inputs its owner sends (`move.ts`) and writes where it
    // is: `pos` (blended between ticks for everyone to draw) and, for the owner to compare with its
    // prediction, `rpos` / `rvy` (the exact state) and `seq` (the last input they are the result of).
    // The owner writes only how it faces and how fast it goes (for the others' animation); the
    // server owns the score.
    Network.defineClass(
      "Player",
      [
        "pos:vec3:q=0.01:lerp",
        "rpos:vec3:q=0.001",
        "rvy:f32:q=0.01",
        "seq:i32",
        "yaw:f32:q=0.5:owner:lerp",
        "speed:f32:q=0.1:owner",
        "sprint:bool:owner",
        // The torch is pointed (trigger held, or just fired) and how high (degrees): the arm rises.
        "firing:bool:owner",
        "pitch:f32:q=1:owner",
        "hp:i32",
        // The net id of whoever killed it last (for its killcam).
        "killer:i32",
        "kills:i32",
        "deaths:i32",
        "alive:bool",
        "shield:bool",
        "ping:i32",
        "name:str",
      ].join(";"),
    ),
    // A prop the server simulates (crates, a shovable car): clients only draw it.
    Network.defineClass("Prop", "pos:vec3:q=0.01:lerp;rot:quat:q=0.001:lerp;kind:i32"),
    // The match: one object everybody can read.
    Network.defineClass("Match", "state:str;timeLeft:f32:q=0.1;winner:str;round:i32"),
    // A client asks to shoot: yaw (degrees), and how far back in time it saw the world (ms).
    Network.defineRpc("fire", "server"),
    // A client's last few movement inputs (`move.ts`), unreliable: each packet repeats the ones
    // the server has not confirmed, so a lost one is made up by the next.
    Network.defineRpc("input", "server", "unreliable"),
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
