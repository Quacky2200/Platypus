// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! [`NetHost`]: what the engine drives. One object that is either a server or a client and owns the
//! session, the replica and RPCs, so the engine makes one call a frame (`update`) and reads events.
//!
//! The server steps replication at the network tick rate and sends each player its spawns and
//! state; the client acknowledges states and sends its owner-written fields. RPCs are checked on
//! the receiving side against the schema: a client may only call `Server` RPCs, only on objects it
//! owns unless the RPC allows anyone, and no more than a fixed number a second.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::replica::{NetId, ReplicaClient, ReplicaEvent, ReplicaServer, Tick};
use crate::schema::{RpcTarget, Schema, Value};
use crate::session::{PlayerId, RejectReason, SERVER, Session, SessionConfig, SessionEvent, Token};
use crate::transport::{Channel, DisconnectReason, Time, Transport};
use crate::wire::{Reader, Writer};

const RPC_TO_SERVER: u8 = 0x20;
const RPC_TO_CLIENT: u8 = 0x21;
const MAX_RPC_ARGS: u64 = 16;

pub struct HostConfig {
    pub session: SessionConfig,
    /// Replication steps a second.
    pub tick_rate: u32,
    /// Client-to-server RPCs a player may make a second; the rest are dropped.
    pub rpc_per_second: u32,
    /// How many ticks behind the newest a client draws interpolated fields.
    pub interpolation_delay: f64,
}

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig {
            session: SessionConfig::default(),
            tick_rate: 30,
            rpc_per_second: 120,
            interpolation_delay: 2.0,
        }
    }
}

/// What the game should know after `update`.
#[derive(Debug, Clone, PartialEq)]
pub enum NetEvent {
    // Server.
    PlayerJoined {
        player: PlayerId,
        name: String,
        resumed: bool,
        /// Here to watch: the game should not give it a body, and its RPCs are refused.
        spectator: bool,
    },
    /// Connection lost; the slot is held for the grace period.
    PlayerDropped {
        player: PlayerId,
    },
    PlayerLeft {
        player: PlayerId,
    },
    // Client.
    Connected {
        player: PlayerId,
        token: Token,
        resumed: bool,
    },
    Rejected(RejectReason),
    Disconnected(DisconnectReason),
    // Both: replication and RPCs.
    Spawned {
        id: NetId,
        class: u16,
        prefab: String,
        owner: PlayerId,
    },
    Despawned {
        id: NetId,
    },
    OwnerChanged {
        id: NetId,
        owner: PlayerId,
    },
    /// An RPC arrived (on the server: from `from`; on a client: from the server, 0).
    Rpc {
        id: NetId,
        name: String,
        args: Vec<Value>,
        from: PlayerId,
    },
}

enum Half {
    Server(ReplicaServer),
    Client(Option<ReplicaClient>),
}

pub struct NetHost<T: Transport> {
    session: Session<T>,
    schema: Arc<Schema>,
    half: Half,
    tick_interval: Duration,
    next_tick: Option<Time>,
    rpc_limit: u32,
    /// Per player: start of the current second and RPCs counted in it.
    rpc_window: HashMap<PlayerId, (Time, u32)>,
    /// RPCs refused (unknown, not allowed, too many, malformed): for the log and the panel.
    pub dropped_rpcs: u64,
    /// The client's newest complete tick and when it was first seen (the interpolation clock).
    seen: Option<(Tick, Time)>,
    now: Time,
    interp_delay: f64,
}

impl<T: Transport> NetHost<T> {
    pub fn server(transport: T, schema: Arc<Schema>, mut config: HostConfig) -> Self {
        config.session.schema_hash = schema.hash();
        config.session.tick_rate = config.tick_rate;
        NetHost {
            session: Session::server(transport, config.session),
            half: Half::Server(ReplicaServer::new(schema.clone())),
            schema,
            tick_interval: Duration::from_secs_f64(1.0 / f64::from(config.tick_rate.max(1))),
            next_tick: None,
            rpc_limit: config.rpc_per_second,
            rpc_window: HashMap::new(),
            dropped_rpcs: 0,
            seen: None,
            now: Time::ZERO,
            interp_delay: config.interpolation_delay,
        }
    }

    /// A field's current value: on the server what it holds; on a client the newest state, blended
    /// between ticks (`interpolation_delay` behind) for fields marked `interpolate`.
    pub fn read(&self, id: NetId, field: &str) -> Option<Value> {
        match &self.half {
            Half::Server(rep) => rep.get(id, field).cloned(),
            Half::Client(Some(rep)) => match self.render_tick() {
                Some(t) => rep.interpolated(id, field, t),
                None => rep.value(id, field),
            },
            Half::Client(None) => None,
        }
    }

    /// The fractional tick a client draws: the newest tick plus the time since it arrived, less the delay.
    pub fn render_tick(&self) -> Option<f64> {
        let (tick, at) = self.seen?;
        let frac =
            (self.now.saturating_sub(at).as_secs_f64() / self.tick_interval.as_secs_f64()).min(1.0);
        Some((f64::from(tick) + frac - self.interp_delay).max(0.0))
    }

    /// Replication steps a second.
    pub fn tick_rate(&self) -> f64 {
        // The interval is whole nanoseconds, so 30 Hz comes back as 30.0000003 without the rounding.
        (1000.0 / self.tick_interval.as_secs_f64()).round() / 1000.0
    }

    /// The server's tick as this peer knows it: the server's own, or the newest complete one a
    /// client has been sent (`None` before the first).
    pub fn tick(&self) -> Option<Tick> {
        match &self.half {
            Half::Server(rep) => Some(rep.tick()),
            Half::Client(_) => self.seen.map(|(t, _)| t),
        }
    }

    /// Network time in seconds: the server's tick clock. A client estimates the server's present
    /// (its newest tick plus the time since it arrived), so every peer's value agrees to within
    /// the one-way delay, which is what timers shared between peers (a round clock, a door that
    /// opens at second N) need. `None` before the first tick.
    pub fn time(&self) -> Option<f64> {
        let tick = self.tick()?;
        let since = match (&self.half, self.seen) {
            (Half::Client(_), Some((_, at))) => self.now.saturating_sub(at).as_secs_f64(),
            (Half::Server(_), _) => self.next_tick.map_or(0.0, |next| {
                (self.tick_interval.as_secs_f64() - next.saturating_sub(self.now).as_secs_f64())
                    .max(0.0)
            }),
            _ => 0.0,
        };
        Some((f64::from(tick) * self.tick_interval.as_secs_f64()) + since)
    }

    pub fn client(
        transport: T,
        schema: Arc<Schema>,
        mut config: HostConfig,
        name: &str,
        token: Option<Token>,
    ) -> Self {
        config.session.schema_hash = schema.hash();
        NetHost {
            session: Session::client(transport, config.session, name, token),
            half: Half::Client(None),
            schema,
            tick_interval: Duration::from_secs_f64(1.0 / f64::from(config.tick_rate.max(1))),
            next_tick: None,
            rpc_limit: config.rpc_per_second,
            rpc_window: HashMap::new(),
            dropped_rpcs: 0,
            seen: None,
            now: Time::ZERO,
            interp_delay: config.interpolation_delay,
        }
    }

    pub fn is_server(&self) -> bool {
        matches!(self.half, Half::Server(_))
    }

    pub fn schema(&self) -> &Arc<Schema> {
        &self.schema
    }

    pub fn session(&self) -> &Session<T> {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut Session<T> {
        &mut self.session
    }

    /// The server's objects (None on a client).
    pub fn server_mut(&mut self) -> Option<&mut ReplicaServer> {
        match &mut self.half {
            Half::Server(r) => Some(r),
            Half::Client(_) => None,
        }
    }

    pub fn server_ref(&self) -> Option<&ReplicaServer> {
        match &self.half {
            Half::Server(r) => Some(r),
            Half::Client(_) => None,
        }
    }

    /// The client's view of the world (None on a server, or before the handshake).
    pub fn client_ref(&self) -> Option<&ReplicaClient> {
        match &self.half {
            Half::Client(r) => r.as_ref(),
            Half::Server(_) => None,
        }
    }

    pub fn client_mut(&mut self) -> Option<&mut ReplicaClient> {
        match &mut self.half {
            Half::Client(r) => r.as_mut(),
            Half::Server(_) => None,
        }
    }

    /// Calls an RPC: a client calls `Server` ones, the server `Owner` and `All` ones.
    pub fn rpc(&mut self, id: NetId, name: &str, args: &[Value]) -> Result<(), String> {
        let idx = self
            .schema
            .rpc_id(name)
            .ok_or_else(|| format!("no RPC named {name:?}"))?;
        let def = self.schema.rpc(idx).expect("rpc_id is in range").clone();
        if args.len() as u64 > MAX_RPC_ARGS {
            return Err(format!("RPC {name}: at most {MAX_RPC_ARGS} arguments"));
        }
        let channel = if def.reliable {
            Channel::Reliable
        } else {
            Channel::Unreliable
        };
        let packet = |kind: u8| {
            let mut w = Writer::new();
            w.u8(kind)
                .varint(u64::from(idx))
                .varint(id)
                .varint(args.len() as u64);
            args.iter().for_each(|a| a.write_tagged(&mut w));
            w.finish()
        };
        match (&self.half, def.target) {
            (Half::Client(_), RpcTarget::Server) => self
                .session
                .send(SERVER, channel, &packet(RPC_TO_SERVER))
                .map_err(|e| e.to_string()),
            (Half::Client(_), _) => Err(format!(
                "RPC {name} runs on a client: only the server calls it"
            )),
            (Half::Server(_), RpcTarget::Server) => {
                Err(format!("RPC {name} runs on the server: a client calls it"))
            }
            (Half::Server(rep), target) => {
                let obj = rep.object(id).ok_or_else(|| format!("no object {id}"))?;
                let to: Vec<PlayerId> = match target {
                    RpcTarget::Owner => vec![obj.owner],
                    _ => rep
                        .players()
                        .into_iter()
                        .filter(|p| rep.knows(*p, id))
                        .collect(),
                };
                let bytes = packet(RPC_TO_CLIENT);
                for p in to {
                    // A player that dropped is simply not reachable.
                    let _ = self.session.send(p, channel, &bytes);
                }
                Ok(())
            }
        }
    }

    /// Moves everything on to `now`: transport, session, replication ticks, acknowledgements.
    pub fn update(&mut self, now: Time) -> Vec<NetEvent> {
        self.now = now;
        let mut out = Vec::new();
        for event in self.session.update(now) {
            self.on_session_event(event, now, &mut out);
        }
        match &mut self.half {
            Half::Server(rep) => {
                let next = self.next_tick.get_or_insert(now);
                let mut steps = 0;
                while now >= *next && steps < 5 {
                    for m in rep.step() {
                        let _ = self.session.send(m.to, m.channel, &m.bytes);
                    }
                    *next += self.tick_interval;
                    steps += 1;
                }
                if now >= *next {
                    *next = now + self.tick_interval;
                }
            }
            Half::Client(Some(rep)) => {
                let latest = rep.latest_tick();
                if latest != self.seen.map(|(t, _)| t) {
                    self.seen = latest.map(|t| (t, now));
                }
                if let Some(ack) = rep.take_ack() {
                    let _ = self.session.send(SERVER, Channel::Unreliable, &ack);
                }
                if let Some(update) = rep.take_owner_update() {
                    let _ = self
                        .session
                        .send(SERVER, Channel::UnreliableSequenced, &update);
                }
            }
            Half::Client(None) => {}
        }
        out
    }

    fn on_session_event(&mut self, event: SessionEvent, now: Time, out: &mut Vec<NetEvent>) {
        match event {
            SessionEvent::PlayerJoined {
                player,
                name,
                resumed,
                spectator,
            } => {
                if let Half::Server(rep) = &mut self.half {
                    // A returning player starts from nothing: every spawn again. (A spectator is
                    // replicated to like anyone: it sees everything the interest rules allow.)
                    rep.add_player(player);
                }
                out.push(NetEvent::PlayerJoined {
                    player,
                    name,
                    resumed,
                    spectator,
                });
            }
            SessionEvent::PlayerDropped { player, .. } => {
                if let Half::Server(rep) = &mut self.half {
                    rep.remove_player(player);
                }
                out.push(NetEvent::PlayerDropped { player });
            }
            SessionEvent::PlayerLeft { player } => {
                if let Half::Server(rep) = &mut self.half {
                    rep.remove_player(player);
                }
                self.rpc_window.remove(&player);
                out.push(NetEvent::PlayerLeft { player });
            }
            SessionEvent::Connected {
                player,
                token,
                resumed,
                ..
            } => {
                if let Half::Client(slot) = &mut self.half {
                    *slot = Some(ReplicaClient::new(self.schema.clone(), player));
                }
                out.push(NetEvent::Connected {
                    player,
                    token,
                    resumed,
                });
            }
            SessionEvent::Rejected(reason) => out.push(NetEvent::Rejected(reason)),
            SessionEvent::Disconnected(reason) => {
                if let Half::Client(slot) = &mut self.half {
                    *slot = None;
                }
                out.push(NetEvent::Disconnected(reason));
            }
            SessionEvent::Message { from, bytes, .. } => self.on_message(from, &bytes, now, out),
        }
    }

    fn on_message(&mut self, from: PlayerId, bytes: &[u8], now: Time, out: &mut Vec<NetEvent>) {
        match bytes.first().copied() {
            Some(RPC_TO_SERVER) if self.is_server() => self.on_server_rpc(from, bytes, now, out),
            Some(RPC_TO_CLIENT) if !self.is_server() => match parse_rpc(&self.schema, bytes) {
                Some((def, id, args)) if def.target != RpcTarget::Server => {
                    out.push(NetEvent::Rpc {
                        id,
                        name: def.name,
                        args,
                        from: SERVER,
                    });
                }
                _ => self.dropped_rpcs += 1,
            },
            Some(0x10..=0x1f) => match &mut self.half {
                Half::Server(rep) => rep.handle(from, bytes),
                Half::Client(Some(rep)) => {
                    for e in rep.handle(bytes) {
                        out.push(match e {
                            ReplicaEvent::Spawned {
                                id,
                                class,
                                prefab,
                                owner,
                            } => NetEvent::Spawned {
                                id,
                                class,
                                prefab,
                                owner,
                            },
                            ReplicaEvent::Despawned { id } => NetEvent::Despawned { id },
                            ReplicaEvent::OwnerChanged { id, owner } => {
                                NetEvent::OwnerChanged { id, owner }
                            }
                        });
                    }
                }
                Half::Client(None) => {}
            },
            _ => {}
        }
    }

    fn on_server_rpc(&mut self, from: PlayerId, bytes: &[u8], now: Time, out: &mut Vec<NetEvent>) {
        // At most `rpc_limit` a second per player.
        let w = self.rpc_window.entry(from).or_insert((now, 0));
        if now.saturating_sub(w.0) >= Duration::from_secs(1) {
            *w = (now, 0);
        }
        w.1 += 1;
        if w.1 > self.rpc_limit {
            self.dropped_rpcs += 1;
            return;
        }
        let Some((def, id, args)) = parse_rpc(&self.schema, bytes) else {
            self.dropped_rpcs += 1;
            return;
        };
        let owner = self
            .server_ref()
            .and_then(|r| r.object(id))
            .map(|o| o.owner);
        // Spectators watch: they may not call anything.
        let allowed = def.target == RpcTarget::Server
            && !self.session.is_spectator(from)
            && owner.is_some_and(|o| !def.require_owner || o == from);
        if !allowed {
            self.dropped_rpcs += 1;
            return;
        }
        out.push(NetEvent::Rpc {
            id,
            name: def.name,
            args,
            from,
        });
    }
}

fn parse_rpc(schema: &Schema, bytes: &[u8]) -> Option<(crate::schema::RpcDef, NetId, Vec<Value>)> {
    let mut r = Reader::new(bytes);
    r.u8()?;
    let def = schema.rpc(u16::try_from(r.varint()?).ok()?)?.clone();
    let id = r.varint()?;
    let argc = r.varint()?;
    if argc > MAX_RPC_ARGS {
        return None;
    }
    let mut args = Vec::new();
    for _ in 0..argc {
        args.push(Value::read_tagged(&mut r)?);
    }
    Some((def, id, args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loopback::{LoopbackHub, LoopbackTransport};
    use crate::schema::{ClassDef, FieldDef, FieldType, RpcDef};

    const MS: Duration = Duration::from_millis(1);

    fn schema() -> Arc<Schema> {
        Arc::new(
            Schema::new(vec![
                ClassDef::new("Player")
                    .field(FieldDef::new("pos", FieldType::Vec3).quantized(0.01))
                    .field(FieldDef::new("hp", FieldType::I32))
                    .field(FieldDef::new("aim", FieldType::F32).owner_writes()),
            ])
            .with_rpcs(vec![
                RpcDef::new("fire", RpcTarget::Server),
                RpcDef::new("shout", RpcTarget::Server).from_anyone(),
                RpcDef::new("hitMarker", RpcTarget::Owner),
                RpcDef::new("boom", RpcTarget::All),
            ]),
        )
    }

    struct Rig {
        server: NetHost<LoopbackTransport>,
        clients: Vec<NetHost<LoopbackTransport>>,
        hub: LoopbackHub,
        now: Time,
    }

    impl Rig {
        fn new(clients: usize) -> Rig {
            let hub = LoopbackHub::new();
            let mut t = hub.endpoint();
            t.listen("srv").unwrap();
            let server = NetHost::server(t, schema(), HostConfig::default());
            let mut rig = Rig {
                server,
                clients: Vec::new(),
                hub,
                now: Time::ZERO,
            };
            for i in 0..clients {
                rig.add_client(&format!("p{i}"), None);
            }
            rig
        }

        fn add_client(&mut self, name: &str, token: Option<Token>) -> usize {
            let mut t = self.hub.endpoint();
            t.connect("srv").unwrap();
            self.clients.push(NetHost::client(
                t,
                schema(),
                HostConfig::default(),
                name,
                token,
            ));
            self.clients.len() - 1
        }

        /// Runs `ms` milliseconds; returns the server's events and each client's.
        fn run(&mut self, ms: u64) -> (Vec<NetEvent>, Vec<Vec<NetEvent>>) {
            let mut s = Vec::new();
            let mut c = vec![Vec::new(); self.clients.len()];
            for _ in 0..ms / 5 {
                self.now += 5 * MS;
                s.extend(self.server.update(self.now));
                for (i, cl) in self.clients.iter_mut().enumerate() {
                    c[i].extend(cl.update(self.now));
                }
            }
            (s, c)
        }
    }

    // M8-HOST-01
    #[test]
    fn players_join_and_the_server_spawns_objects_they_see() {
        let mut rig = Rig::new(2);
        let (se, ce) = rig.run(300);
        assert_eq!(
            se.iter()
                .filter(|e| matches!(e, NetEvent::PlayerJoined { .. }))
                .count(),
            2
        );
        assert!(
            ce.iter()
                .all(|c| c.iter().any(|e| matches!(e, NetEvent::Connected { .. })))
        );
        let id = rig
            .server
            .server_mut()
            .unwrap()
            .spawn("Player", "player.prefab", 1)
            .unwrap();
        rig.server
            .server_mut()
            .unwrap()
            .set(id, "hp", Value::I32(75));
        let (_, ce) = rig.run(300);
        for c in &ce {
            assert!(c.contains(&NetEvent::Spawned {
                id,
                class: 0,
                prefab: "player.prefab".into(),
                owner: 1
            }));
        }
        for cl in &rig.clients {
            assert_eq!(
                cl.client_ref().unwrap().value(id, "hp"),
                Some(Value::I32(75))
            );
        }
        assert!(rig.clients[0].client_ref().unwrap().is_mine(id));
        assert!(!rig.clients[1].client_ref().unwrap().is_mine(id));
    }

    // M8-HOST-02
    #[test]
    fn rpcs_obey_the_schema_ownership_and_direction() {
        let mut rig = Rig::new(2);
        rig.run(200);
        let id = rig
            .server
            .server_mut()
            .unwrap()
            .spawn("Player", "p", 1)
            .unwrap();
        rig.run(200);

        // The owner may fire; a non-owner may not, but may shout.
        rig.clients[0]
            .rpc(id, "fire", &[Value::Vec3([0.0, 0.0, 1.0]), Value::I32(3)])
            .unwrap();
        rig.clients[1].rpc(id, "fire", &[]).unwrap();
        rig.clients[1]
            .rpc(id, "shout", &[Value::Str("hi".into())])
            .unwrap();
        let (se, _) = rig.run(100);
        let rpcs: Vec<_> = se
            .iter()
            .filter_map(|e| match e {
                NetEvent::Rpc {
                    name, args, from, ..
                } => Some((name.as_str(), args.clone(), *from)),
                _ => None,
            })
            .collect();
        assert_eq!(
            rpcs,
            vec![
                ("fire", vec![Value::Vec3([0.0, 0.0, 1.0]), Value::I32(3)], 1),
                ("shout", vec![Value::Str("hi".into())], 2),
            ]
        );
        assert_eq!(rig.server.dropped_rpcs, 1);

        // Server calls: Owner reaches the owner only, All reaches both.
        rig.server.rpc(id, "hitMarker", &[Value::I32(10)]).unwrap();
        rig.server.rpc(id, "boom", &[]).unwrap();
        let (_, ce) = rig.run(100);
        let names = |c: &[NetEvent]| -> Vec<String> {
            c.iter()
                .filter_map(|e| match e {
                    NetEvent::Rpc { name, .. } => Some(name.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(names(&ce[0]), vec!["hitMarker", "boom"]);
        assert_eq!(names(&ce[1]), vec!["boom"]);

        // Wrong direction is an error on the caller.
        assert!(rig.clients[0].rpc(id, "boom", &[]).is_err());
        assert!(rig.server.rpc(id, "fire", &[]).is_err());
        assert!(rig.clients[0].rpc(id, "nope", &[]).is_err());
    }

    // M8-HOST-03
    #[test]
    fn a_client_over_the_rpc_rate_is_dropped_not_obeyed() {
        let mut rig = Rig::new(1);
        rig.run(200);
        let id = rig
            .server
            .server_mut()
            .unwrap()
            .spawn("Player", "p", 1)
            .unwrap();
        rig.run(200);
        for _ in 0..300 {
            rig.clients[0].rpc(id, "fire", &[]).unwrap();
        }
        let (se, _) = rig.run(100);
        let got = se
            .iter()
            .filter(|e| matches!(e, NetEvent::Rpc { .. }))
            .count();
        assert_eq!(got, 120);
        assert_eq!(rig.server.dropped_rpcs, 180);
    }

    // M8-HOST-04
    #[test]
    fn malformed_rpcs_are_dropped_and_counted() {
        let mut rig = Rig::new(1);
        rig.run(200);
        let id = rig
            .server
            .server_mut()
            .unwrap()
            .spawn("Player", "p", 1)
            .unwrap();
        rig.run(100);
        let session = rig.clients[0].session_mut();
        for bad in [
            vec![RPC_TO_SERVER],
            vec![RPC_TO_SERVER, 99, 1, 0],
            vec![RPC_TO_SERVER, 0, id as u8, 200],
            vec![RPC_TO_SERVER, 0, id as u8, 1, 77],
        ] {
            session.send(SERVER, Channel::Reliable, &bad).unwrap();
        }
        let (se, _) = rig.run(100);
        assert!(se.iter().all(|e| !matches!(e, NetEvent::Rpc { .. })));
        assert_eq!(rig.server.dropped_rpcs, 4);
    }

    // M8-HOST-05
    #[test]
    fn a_returning_player_gets_every_spawn_again() {
        let mut rig = Rig::new(1);
        let (_, ce) = rig.run(200);
        let id = rig
            .server
            .server_mut()
            .unwrap()
            .spawn("Player", "p", 1)
            .unwrap();
        rig.run(200);
        let token = ce[0]
            .iter()
            .find_map(|e| match e {
                NetEvent::Connected { token, .. } => Some(*token),
                _ => None,
            })
            .unwrap();

        // The connection drops, the slot is held, the player comes back with its token.
        let server_peer = rig.clients[0].session().local_player().is_some();
        assert!(server_peer);
        drop(rig.clients.remove(0));
        let (se, _) = rig.run(100);
        assert!(
            se.iter()
                .any(|e| matches!(e, NetEvent::PlayerDropped { player: 1 }))
        );
        let n = rig.add_client("p0", Some(token));
        let (se, ce) = rig.run(300);
        assert!(se.iter().any(|e| matches!(
            e,
            NetEvent::PlayerJoined {
                player: 1,
                resumed: true,
                ..
            }
        )));
        assert!(
            ce[n]
                .iter()
                .any(|e| matches!(e, NetEvent::Spawned { id: i, .. } if *i == id))
        );
        assert!(rig.clients[n].client_ref().unwrap().is_mine(id));
    }

    // M8-HOST-06
    #[cfg(feature = "net-debug")]
    #[test]
    fn replication_and_rpcs_work_over_a_laggy_lossy_link() {
        use crate::conditioned::{Conditioned, Link, Links};
        let hub = LoopbackHub::new();
        let mut st = hub.endpoint();
        st.listen("srv").unwrap();
        let mut server = NetHost::server(st, schema(), HostConfig::default());
        let mut ct = hub.endpoint();
        ct.connect("srv").unwrap();
        let (ct, handle) = Conditioned::new(ct, 42);
        handle.set_default(Links::both(Link {
            latency: 80 * MS,
            jitter: 30 * MS,
            loss: 0.15,
            burst: 2.0,
            duplicate: 0.05,
            reorder: 0.1,
            ..Link::default()
        }));
        let mut client = NetHost::client(ct, schema(), HostConfig::default(), "laggy", None);
        let mut now = Time::ZERO;
        let mut got_rpc = false;
        let mut id = 0;
        for step in 0..1200u64 {
            now += 5 * MS;
            for e in server.update(now) {
                if let NetEvent::PlayerJoined { player, .. } = e {
                    id = server
                        .server_mut()
                        .unwrap()
                        .spawn("Player", "p", player)
                        .unwrap();
                }
                if matches!(e, NetEvent::Rpc { .. }) {
                    got_rpc = true;
                }
            }
            client.update(now);
            if id != 0 && step % 40 == 0 {
                let x = (step / 40) as f32;
                server
                    .server_mut()
                    .unwrap()
                    .set(id, "pos", Value::Vec3([x, 0.0, 0.0]));
                let _ = client.rpc(id, "fire", &[]);
            }
        }
        // Let the last value settle.
        for _ in 0..400 {
            now += 5 * MS;
            server.update(now);
            client.update(now);
        }
        assert!(got_rpc, "reliable RPCs get through the loss");
        assert_eq!(
            client.client_ref().unwrap().value(id, "pos"),
            server.server_ref().unwrap().get(id, "pos").cloned()
        );
    }

    // M8-HOST-08
    #[test]
    fn a_spectator_sees_the_world_but_cannot_call_the_server() {
        let mut rig = Rig::new(1);
        let mut t = rig.hub.endpoint();
        t.connect("srv").unwrap();
        let mut config = HostConfig::default();
        config.session.spectator = true;
        rig.clients
            .push(NetHost::client(t, schema(), config, "watcher", None));
        let (se, _) = rig.run(300);
        assert!(se.iter().any(|e| matches!(
            e,
            NetEvent::PlayerJoined {
                player: 2,
                spectator: true,
                ..
            }
        )));
        let id = rig
            .server
            .server_mut()
            .unwrap()
            .spawn("Player", "p", 1)
            .unwrap();
        rig.run(300);
        // It sees the object...
        assert!(
            rig.clients[1]
                .client_ref()
                .unwrap()
                .value(id, "hp")
                .is_some()
        );
        // ...and a call anyone else may make is refused when it makes it.
        rig.clients[0].rpc(id, "shout", &[]).unwrap();
        rig.clients[1].rpc(id, "shout", &[]).unwrap();
        let (se, _) = rig.run(300);
        let shouts: Vec<_> = se
            .iter()
            .filter_map(|e| match e {
                NetEvent::Rpc { name, from, .. } if name == "shout" => Some(*from),
                _ => None,
            })
            .collect();
        assert_eq!(shouts, vec![1]);
        assert_eq!(rig.server.dropped_rpcs, 1);
    }

    // M8-HOST-07
    #[test]
    fn network_time_is_the_server_clock_on_every_peer() {
        let mut rig = Rig::new(1);
        assert!(rig.clients[0].time().is_none(), "no time before a tick");
        assert_eq!(rig.server.tick_rate(), 30.0);
        rig.run(300);
        rig.server
            .server_mut()
            .unwrap()
            .spawn("Player", "p", 1)
            .unwrap();
        rig.run(1000);
        let (s0, c0) = (rig.server.time().unwrap(), rig.clients[0].time().unwrap());
        // Loopback has no delay: the estimates agree to within a tick.
        assert!((s0 - c0).abs() < 0.07, "server {s0} client {c0}");
        rig.run(2000);
        let (s1, c1) = (rig.server.time().unwrap(), rig.clients[0].time().unwrap());
        assert!((s1 - s0 - 2.0).abs() < 0.07, "server clock ran {}", s1 - s0);
        assert!((c1 - c0 - 2.0).abs() < 0.07, "client clock ran {}", c1 - c0);
        assert!(rig.clients[0].tick().unwrap() > 60);
    }
}
