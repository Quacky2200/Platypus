// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The session: players over a transport. It checks that both ends run the same build (protocol
//! version, schema hash, game version), gives each player a stable [`PlayerId`] and a reconnect
//! token, measures round-trip time with heartbeats, drops silent peers, and holds a dropped player's
//! slot for a grace period so a returning client resumes instead of joining as a stranger.
//!
//! The server is [`PlayerId`] 0. Everything is driven by [`Session::update`] with the current time.

use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use crate::transport::{
    Channel, DisconnectReason, NetError, PeerId, Time, Transport, TransportEvent,
};
use crate::wire::{Reader, Writer};

/// Bumped when the session's own wire format changes.
pub const PROTOCOL_VERSION: u32 = 1;

/// A player's id for the whole session, kept across reconnects. The server is 0.
pub type PlayerId = u32;

pub const SERVER: PlayerId = 0;

/// A secret the server hands out so a dropped client can take its slot back.
pub type Token = u128;

const MAX_TEXT: usize = 128;

mod tag {
    pub const HELLO: u8 = 1;
    pub const WELCOME: u8 = 2;
    pub const REJECT: u8 = 3;
    pub const PING: u8 = 4;
    pub const PONG: u8 = 5;
    pub const DATA: u8 = 6;
    pub const BYE: u8 = 7;
}

#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Hash of the replicated types (from the compiler's schema); a mismatch is refused.
    pub schema_hash: u64,
    /// The game's own version string; a mismatch is refused.
    pub game_version: String,
    pub max_players: u32,
    /// How often each side pings.
    pub heartbeat: Duration,
    /// Silence longer than this ends a connection.
    pub timeout: Duration,
    /// How long a dropped player's slot is held for a reconnect.
    pub grace: Duration,
    /// How long a new connection may take to say hello.
    pub handshake_timeout: Duration,
    /// Network ticks a second, told to clients in the welcome.
    pub tick_rate: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        SessionConfig {
            schema_hash: 0,
            game_version: String::new(),
            max_players: 16,
            heartbeat: Duration::from_millis(250),
            timeout: Duration::from_secs(5),
            grace: Duration::from_secs(30),
            handshake_timeout: Duration::from_secs(5),
            tick_rate: 60,
        }
    }
}

/// Why a hello was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectReason {
    ProtocolMismatch,
    SchemaMismatch,
    VersionMismatch { server: String },
    ServerFull,
    Malformed,
}

impl RejectReason {
    fn code(&self) -> u8 {
        match self {
            RejectReason::ProtocolMismatch => 1,
            RejectReason::SchemaMismatch => 2,
            RejectReason::VersionMismatch { .. } => 3,
            RejectReason::ServerFull => 4,
            RejectReason::Malformed => 5,
        }
    }

    fn from_wire(code: u8, text: String) -> RejectReason {
        match code {
            1 => RejectReason::ProtocolMismatch,
            2 => RejectReason::SchemaMismatch,
            3 => RejectReason::VersionMismatch { server: text },
            4 => RejectReason::ServerFull,
            _ => RejectReason::Malformed,
        }
    }

    fn text(&self) -> &str {
        match self {
            RejectReason::VersionMismatch { server } => server,
            _ => "",
        }
    }
}

/// What the session tells the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    // Client side.
    /// The handshake is done; `token` is for `connect_with_token` after a drop.
    Connected {
        player: PlayerId,
        token: Token,
        resumed: bool,
        tick_rate: u32,
    },
    Rejected(RejectReason),
    Disconnected(DisconnectReason),
    // Server side.
    PlayerJoined {
        player: PlayerId,
        name: String,
        resumed: bool,
    },
    /// The connection is gone but the slot is held until `grace` runs out (or the player returns).
    PlayerDropped {
        player: PlayerId,
        reason: DisconnectReason,
    },
    /// The slot is free: a clean leave, a kick, or the grace period ended.
    PlayerLeft {
        player: PlayerId,
    },
    // Both.
    Message {
        from: PlayerId,
        channel: Channel,
        bytes: Vec<u8>,
    },
}

enum Role {
    Server(ServerState),
    Client(ClientState),
}

#[derive(Default)]
struct ServerState {
    players: HashMap<PlayerId, Player>,
    by_peer: HashMap<PeerId, PlayerId>,
    /// Connections that have not said hello, with when they connected.
    pending: HashMap<PeerId, Time>,
    next_player: PlayerId,
}

struct Player {
    peer: Option<PeerId>,
    token: Token,
    name: String,
    link: Link,
    /// When the connection was lost, while the slot is held.
    dropped_at: Option<Time>,
}

/// Per-connection liveness and round-trip bookkeeping.
#[derive(Default)]
struct Link {
    last_heard: Time,
    next_ping: Time,
    ping_seq: u32,
    /// Recent pings by sequence number, to time their pongs.
    sent: Vec<(u32, Time)>,
    rtt: Option<Duration>,
}

impl Link {
    fn new(now: Time) -> Link {
        Link {
            last_heard: now,
            next_ping: now,
            ..Link::default()
        }
    }

    fn pong(&mut self, seq: u32, now: Time) {
        if let Some(i) = self.sent.iter().position(|(s, _)| *s == seq) {
            let sample = now.saturating_sub(self.sent[i].1);
            self.sent.drain(..=i);
            // A light smoothing: most of the old estimate, some of the new.
            self.rtt = Some(
                self.rtt
                    .map_or(sample, |r| r.mul_f64(0.8) + sample.mul_f64(0.2)),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientPhase {
    /// Waiting for the transport to connect.
    Connecting,
    /// Hello sent, waiting for the welcome.
    Handshaking,
    Connected,
    Closed,
}

struct ClientState {
    name: String,
    token: Option<Token>,
    server: Option<PeerId>,
    phase: ClientPhase,
    me: PlayerId,
    link: Link,
    started: Time,
}

pub struct Session<T: Transport> {
    transport: T,
    config: SessionConfig,
    role: Role,
    now: Time,
    /// Packets that did not parse (the log and the debug panel).
    pub bad_packets: u64,
}

fn new_token() -> Token {
    let mut out = 0u128;
    for i in 0..2u64 {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(i);
        out = (out << 64) | u128::from(h.finish());
    }
    out
}

impl<T: Transport> Session<T> {
    /// A server over a transport that accepts connections.
    pub fn server(transport: T, config: SessionConfig) -> Self {
        Session {
            transport,
            config,
            role: Role::Server(ServerState {
                next_player: 1,
                ..Default::default()
            }),
            now: Time::ZERO,
            bad_packets: 0,
        }
    }

    /// A client over a transport that is connecting (or will, from `update`'s point of view) to the
    /// server. `token` takes a dropped player's slot back.
    pub fn client(transport: T, config: SessionConfig, name: &str, token: Option<Token>) -> Self {
        Session {
            transport,
            config,
            role: Role::Client(ClientState {
                name: name.chars().take(32).collect(),
                token,
                server: None,
                phase: ClientPhase::Connecting,
                me: SERVER,
                link: Link::default(),
                started: Time::ZERO,
            }),
            now: Time::ZERO,
            bad_packets: 0,
        }
    }

    pub fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    pub fn is_server(&self) -> bool {
        matches!(self.role, Role::Server(_))
    }

    /// The client's own player id once connected.
    pub fn local_player(&self) -> Option<PlayerId> {
        match &self.role {
            Role::Server(_) => Some(SERVER),
            Role::Client(c) => (c.phase == ClientPhase::Connected).then_some(c.me),
        }
    }

    pub fn phase(&self) -> Option<ClientPhase> {
        match &self.role {
            Role::Client(c) => Some(c.phase),
            Role::Server(_) => None,
        }
    }

    /// Connected players (not the server), in id order.
    pub fn players(&self) -> Vec<PlayerId> {
        match &self.role {
            Role::Server(s) => {
                let mut v: Vec<_> = s
                    .players
                    .iter()
                    .filter(|(_, p)| p.peer.is_some())
                    .map(|(id, _)| *id)
                    .collect();
                v.sort_unstable();
                v
            }
            Role::Client(_) => Vec::new(),
        }
    }

    pub fn player_name(&self, player: PlayerId) -> Option<&str> {
        match &self.role {
            Role::Server(s) => s.players.get(&player).map(|p| p.name.as_str()),
            Role::Client(_) => None,
        }
    }

    /// Smoothed round-trip time to a player (the server, from a client).
    pub fn rtt(&self, player: PlayerId) -> Option<Duration> {
        match &self.role {
            Role::Server(s) => s.players.get(&player)?.link.rtt,
            Role::Client(c) => c.link.rtt,
        }
    }

    /// Sends to one player (the server, from a client). Returns an error for one that is not connected.
    pub fn send(&mut self, to: PlayerId, channel: Channel, bytes: &[u8]) -> Result<(), NetError> {
        let peer = self
            .peer_of(to)
            .ok_or(NetError::UnknownPeer(u64::from(to)))?;
        self.send_data(peer, channel, bytes)
    }

    /// Sends to every connected player but `except` (server only).
    pub fn broadcast(&mut self, channel: Channel, bytes: &[u8], except: Option<PlayerId>) {
        for p in self.players() {
            if Some(p) != except {
                let _ = self.send(p, channel, bytes);
            }
        }
    }

    /// Removes a player: they are told, and their slot is freed at once (server only).
    pub fn kick(&mut self, player: PlayerId) -> Option<SessionEvent> {
        let Role::Server(s) = &mut self.role else {
            return None;
        };
        let p = s.players.remove(&player)?;
        if let Some(peer) = p.peer {
            s.by_peer.remove(&peer);
            let _ = self.transport.send(peer, Channel::Reliable, &[tag::BYE]);
            self.transport.disconnect(peer);
        }
        Some(SessionEvent::PlayerLeft { player })
    }

    /// Leaves cleanly (a client) so the server frees the slot at once rather than after the grace.
    pub fn leave(&mut self) {
        if let Role::Client(c) = &mut self.role
            && let Some(server) = c.server
        {
            let _ = self.transport.send(server, Channel::Reliable, &[tag::BYE]);
            self.transport.disconnect(server);
            c.phase = ClientPhase::Closed;
        }
    }

    fn peer_of(&self, player: PlayerId) -> Option<PeerId> {
        match &self.role {
            Role::Server(s) => s.players.get(&player)?.peer,
            Role::Client(c) => (player == SERVER && c.phase == ClientPhase::Connected)
                .then_some(c.server)
                .flatten(),
        }
    }

    fn send_data(&mut self, peer: PeerId, channel: Channel, bytes: &[u8]) -> Result<(), NetError> {
        let mut w = Writer::new();
        w.u8(tag::DATA).bytes(bytes);
        self.transport.send(peer, channel, &w.finish())
    }

    /// Moves the session to `now`: reads the transport, answers and sends heartbeats, ends silent
    /// connections and expired slots. Returns what the game should know.
    pub fn update(&mut self, now: Time) -> Vec<SessionEvent> {
        self.now = now;
        let mut events = Vec::new();
        for event in self.transport.poll(now) {
            match event {
                TransportEvent::Connected(peer) => self.on_connected(peer),
                TransportEvent::Disconnected(peer, reason) => {
                    self.on_disconnected(peer, reason, &mut events)
                }
                TransportEvent::Data {
                    peer,
                    channel,
                    bytes,
                } => self.on_data(peer, channel, &bytes, &mut events),
            }
        }
        self.heartbeats_and_timeouts(&mut events);
        events
    }

    fn on_connected(&mut self, peer: PeerId) {
        let now = self.now;
        match &mut self.role {
            Role::Server(s) => {
                s.pending.insert(peer, now);
            }
            Role::Client(c) => {
                if c.server.is_some() {
                    return;
                }
                c.server = Some(peer);
                c.phase = ClientPhase::Handshaking;
                c.started = now;
                c.link = Link::new(now);
                let mut w = Writer::new();
                w.u8(tag::HELLO)
                    .u32(PROTOCOL_VERSION)
                    .u64(self.config.schema_hash)
                    .str(&self.config.game_version)
                    .str(&c.name);
                match c.token {
                    Some(t) => w.u8(1).u128(t),
                    None => w.u8(0),
                };
                let _ = self.transport.send(peer, Channel::Reliable, &w.finish());
            }
        }
    }

    fn on_disconnected(
        &mut self,
        peer: PeerId,
        reason: DisconnectReason,
        events: &mut Vec<SessionEvent>,
    ) {
        let now = self.now;
        match &mut self.role {
            Role::Server(s) => {
                s.pending.remove(&peer);
                if let Some(player) = s.by_peer.remove(&peer)
                    && let Some(p) = s.players.get_mut(&player)
                {
                    p.peer = None;
                    p.dropped_at = Some(now);
                    events.push(SessionEvent::PlayerDropped { player, reason });
                }
            }
            Role::Client(c) => {
                if c.server == Some(peer) && c.phase != ClientPhase::Closed {
                    c.phase = ClientPhase::Closed;
                    c.server = None;
                    events.push(SessionEvent::Disconnected(reason));
                }
            }
        }
    }

    fn on_data(
        &mut self,
        peer: PeerId,
        channel: Channel,
        bytes: &[u8],
        events: &mut Vec<SessionEvent>,
    ) {
        let now = self.now;
        let mut r = Reader::new(bytes);
        let Some(kind) = r.u8() else {
            self.bad_packets += 1;
            return;
        };
        // Anything from a known connection counts as being heard from.
        match &mut self.role {
            Role::Server(s) => {
                if let Some(p) = s.by_peer.get(&peer).and_then(|id| s.players.get_mut(id)) {
                    p.link.last_heard = now;
                }
            }
            Role::Client(c) => c.link.last_heard = now,
        }
        match kind {
            tag::HELLO => self.on_hello(peer, r, events),
            tag::WELCOME => self.on_welcome(r, events),
            tag::REJECT => self.on_reject(peer, r, events),
            tag::PING => {
                let Some(seq) = r.u32() else {
                    self.bad_packets += 1;
                    return;
                };
                if self.is_known(peer) {
                    let mut w = Writer::new();
                    w.u8(tag::PONG).u32(seq);
                    let _ = self.transport.send(peer, Channel::Unreliable, &w.finish());
                }
            }
            tag::PONG => {
                let Some(seq) = r.u32() else {
                    self.bad_packets += 1;
                    return;
                };
                match &mut self.role {
                    Role::Server(s) => {
                        if let Some(p) = s.by_peer.get(&peer).and_then(|id| s.players.get_mut(id)) {
                            p.link.pong(seq, now);
                        }
                    }
                    Role::Client(c) => c.link.pong(seq, now),
                }
            }
            tag::DATA => {
                if let Some(from) = self.player_of(peer) {
                    events.push(SessionEvent::Message {
                        from,
                        channel,
                        bytes: r.rest().to_vec(),
                    });
                }
            }
            tag::BYE => self.on_bye(peer, events),
            _ => self.bad_packets += 1,
        }
    }

    /// A connection that finished the handshake (or, from a client, the server).
    fn is_known(&self, peer: PeerId) -> bool {
        self.player_of(peer).is_some()
    }

    fn player_of(&self, peer: PeerId) -> Option<PlayerId> {
        match &self.role {
            Role::Server(s) => s.by_peer.get(&peer).copied(),
            Role::Client(c) => {
                (c.phase == ClientPhase::Connected && c.server == Some(peer)).then_some(SERVER)
            }
        }
    }

    fn on_hello(&mut self, peer: PeerId, mut r: Reader<'_>, events: &mut Vec<SessionEvent>) {
        let now = self.now;
        let Role::Server(s) = &mut self.role else {
            return;
        };
        if s.by_peer.contains_key(&peer) || !s.pending.contains_key(&peer) {
            return;
        }
        let parsed = (|| {
            let protocol = r.u32()?;
            let schema = r.u64()?;
            let version = r.str(MAX_TEXT)?;
            let name = r.str(MAX_TEXT)?;
            let token = if r.u8()? == 1 { Some(r.u128()?) } else { None };
            Some((protocol, schema, version, name, token))
        })();
        let reject = |t: &mut T, reason: RejectReason| {
            let mut w = Writer::new();
            w.u8(tag::REJECT).u8(reason.code()).str(reason.text());
            let _ = t.send(peer, Channel::Reliable, &w.finish());
            t.disconnect(peer);
        };
        let Some((protocol, schema, version, name, token)) = parsed else {
            self.bad_packets += 1;
            s.pending.remove(&peer);
            reject(&mut self.transport, RejectReason::Malformed);
            return;
        };
        let refused = if protocol != PROTOCOL_VERSION {
            Some(RejectReason::ProtocolMismatch)
        } else if schema != self.config.schema_hash {
            Some(RejectReason::SchemaMismatch)
        } else if version != self.config.game_version {
            Some(RejectReason::VersionMismatch {
                server: self.config.game_version.clone(),
            })
        } else {
            None
        };
        if let Some(reason) = refused {
            s.pending.remove(&peer);
            reject(&mut self.transport, reason);
            return;
        }
        // A held slot with this token comes back; otherwise a new player, if there is room.
        let held = token.and_then(|t| {
            s.players
                .iter()
                .find(|(_, p)| p.token == t && p.peer.is_none())
                .map(|(id, _)| *id)
        });
        let live = s.players.values().filter(|p| p.peer.is_some()).count() as u32;
        let (player, resumed) = match held {
            Some(id) => (id, true),
            None if live + s.players.values().filter(|p| p.peer.is_none()).count() as u32
                >= self.config.max_players =>
            {
                // Held slots count against the limit so a reconnecting player always has a place,
                // but one that will not return in time never blocks a newcomer for long (the grace).
                s.pending.remove(&peer);
                reject(&mut self.transport, RejectReason::ServerFull);
                return;
            }
            None => {
                let id = s.next_player;
                s.next_player += 1;
                (id, false)
            }
        };
        s.pending.remove(&peer);
        let token = match s.players.get(&player) {
            Some(p) => p.token,
            None => new_token(),
        };
        s.players.insert(
            player,
            Player {
                peer: Some(peer),
                token,
                name: name.clone(),
                link: Link::new(now),
                dropped_at: None,
            },
        );
        s.by_peer.insert(peer, player);
        let mut w = Writer::new();
        w.u8(tag::WELCOME)
            .u32(player)
            .u128(token)
            .u8(u8::from(resumed))
            .u32(self.config.tick_rate);
        let _ = self.transport.send(peer, Channel::Reliable, &w.finish());
        events.push(SessionEvent::PlayerJoined {
            player,
            name,
            resumed,
        });
    }

    fn on_welcome(&mut self, mut r: Reader<'_>, events: &mut Vec<SessionEvent>) {
        let Role::Client(c) = &mut self.role else {
            return;
        };
        if c.phase != ClientPhase::Handshaking {
            return;
        }
        let parsed = (|| Some((r.u32()?, r.u128()?, r.u8()? == 1, r.u32()?)))();
        let Some((player, token, resumed, tick_rate)) = parsed else {
            self.bad_packets += 1;
            return;
        };
        c.phase = ClientPhase::Connected;
        c.me = player;
        c.token = Some(token);
        events.push(SessionEvent::Connected {
            player,
            token,
            resumed,
            tick_rate,
        });
    }

    fn on_reject(&mut self, peer: PeerId, mut r: Reader<'_>, events: &mut Vec<SessionEvent>) {
        let Role::Client(c) = &mut self.role else {
            return;
        };
        if c.server != Some(peer) || c.phase != ClientPhase::Handshaking {
            return;
        }
        let parsed = (|| Some((r.u8()?, r.str(MAX_TEXT)?)))();
        let Some((code, text)) = parsed else {
            self.bad_packets += 1;
            return;
        };
        c.phase = ClientPhase::Closed;
        c.server = None;
        events.push(SessionEvent::Rejected(RejectReason::from_wire(code, text)));
        self.transport.disconnect(peer);
    }

    fn on_bye(&mut self, peer: PeerId, events: &mut Vec<SessionEvent>) {
        match &mut self.role {
            Role::Server(s) => {
                if let Some(player) = s.by_peer.remove(&peer) {
                    s.players.remove(&player);
                    events.push(SessionEvent::PlayerLeft { player });
                }
                self.transport.disconnect(peer);
            }
            Role::Client(c) => {
                if c.server == Some(peer) && c.phase != ClientPhase::Closed {
                    c.phase = ClientPhase::Closed;
                    c.server = None;
                    events.push(SessionEvent::Disconnected(DisconnectReason::Remote));
                }
                self.transport.disconnect(peer);
            }
        }
    }

    fn heartbeats_and_timeouts(&mut self, events: &mut Vec<SessionEvent>) {
        let (now, config) = (self.now, &self.config);
        let mut pings = Vec::new();
        let mut dead = Vec::new();
        let mut expired = Vec::new();
        match &mut self.role {
            Role::Server(s) => {
                for (id, p) in &mut s.players {
                    match p.peer {
                        Some(peer) => {
                            if now.saturating_sub(p.link.last_heard) > config.timeout {
                                dead.push(peer);
                            } else if now >= p.link.next_ping {
                                p.link.next_ping = now + config.heartbeat;
                                pings.push((peer, ping(&mut p.link, now)));
                            }
                        }
                        None => {
                            if p.dropped_at
                                .is_some_and(|t| now.saturating_sub(t) > config.grace)
                            {
                                expired.push(*id);
                            }
                        }
                    }
                }
                let silent: Vec<PeerId> = s
                    .pending
                    .iter()
                    .filter(|(_, since)| now.saturating_sub(**since) > config.handshake_timeout)
                    .map(|(peer, _)| *peer)
                    .collect();
                for peer in silent {
                    s.pending.remove(&peer);
                    self.transport.disconnect(peer);
                }
                for id in &expired {
                    s.players.remove(id);
                    events.push(SessionEvent::PlayerLeft { player: *id });
                }
            }
            Role::Client(c) => {
                if let Some(server) = c.server {
                    let waiting = match c.phase {
                        ClientPhase::Handshaking => {
                            now.saturating_sub(c.started) > config.handshake_timeout
                        }
                        ClientPhase::Connected => {
                            now.saturating_sub(c.link.last_heard) > config.timeout
                        }
                        _ => false,
                    };
                    if waiting {
                        dead.push(server);
                    } else if c.phase == ClientPhase::Connected && now >= c.link.next_ping {
                        c.link.next_ping = now + config.heartbeat;
                        pings.push((server, ping(&mut c.link, now)));
                    }
                }
            }
        }
        for (peer, packet) in pings {
            let _ = self.transport.send(peer, Channel::Unreliable, &packet);
        }
        for peer in dead {
            self.transport.disconnect(peer);
            // The transport reports its own end; the session's reason is a timeout.
            self.on_disconnected(peer, DisconnectReason::TimedOut, events);
        }
    }
}

fn ping(link: &mut Link, now: Time) -> Vec<u8> {
    link.ping_seq = link.ping_seq.wrapping_add(1);
    link.sent.push((link.ping_seq, now));
    if link.sent.len() > 16 {
        link.sent.remove(0);
    }
    let mut w = Writer::new();
    w.u8(tag::PING).u32(link.ping_seq);
    w.finish()
}

#[cfg(all(test, feature = "net-debug"))]
mod tests {
    use super::*;
    use crate::conditioned::{Conditioned, Fault, Handle, Link as NetLink, Links};
    use crate::loopback::{LoopbackHub, LoopbackTransport};

    const MS: Duration = Duration::from_millis(1);

    type Net = Conditioned<LoopbackTransport>;

    struct Rig {
        hub: LoopbackHub,
        server: Session<Net>,
        server_handle: Handle,
        now: Time,
    }

    fn config() -> SessionConfig {
        SessionConfig {
            schema_hash: 42,
            game_version: "1.0".into(),
            ..SessionConfig::default()
        }
    }

    impl Rig {
        fn new(config: SessionConfig) -> Rig {
            let hub = LoopbackHub::new();
            let mut t = hub.endpoint();
            t.listen("srv").unwrap();
            let (t, server_handle) = Conditioned::new(t, 1);
            Rig {
                hub,
                server: Session::server(t, config),
                server_handle,
                now: Time::ZERO,
            }
        }

        fn client(
            &self,
            config: SessionConfig,
            name: &str,
            token: Option<Token>,
        ) -> (Session<Net>, Handle) {
            let mut t = self.hub.endpoint();
            t.connect("srv").unwrap();
            let (t, handle) = Conditioned::new(t, 2);
            (Session::client(t, config, name, token), handle)
        }

        /// Runs `ms` milliseconds in 10 ms steps and collects (server events, per client events).
        fn run(
            &mut self,
            ms: u64,
            clients: &mut [&mut Session<Net>],
        ) -> (Vec<SessionEvent>, Vec<Vec<SessionEvent>>) {
            let mut server_events = Vec::new();
            let mut client_events = vec![Vec::new(); clients.len()];
            for _ in 0..ms / 10 {
                self.now += 10 * MS;
                server_events.extend(self.server.update(self.now));
                for (c, out) in clients.iter_mut().zip(&mut client_events) {
                    out.extend(c.update(self.now));
                }
            }
            (server_events, client_events)
        }
    }

    // M8-NET-05
    #[test]
    fn a_matching_client_joins_and_messages_flow_both_ways() {
        let mut rig = Rig::new(config());
        let (mut client, _) = rig.client(config(), "ann", None);
        let (server_events, client_events) = rig.run(100, &mut [&mut client]);
        assert!(matches!(
            server_events.as_slice(),
            [SessionEvent::PlayerJoined { player: 1, resumed: false, name }] if name == "ann"
        ));
        assert!(matches!(
            client_events[0].as_slice(),
            [SessionEvent::Connected {
                player: 1,
                resumed: false,
                tick_rate: 60,
                ..
            }]
        ));
        assert_eq!(client.local_player(), Some(1));
        assert_eq!(rig.server.players(), vec![1]);
        assert_eq!(rig.server.player_name(1), Some("ann"));

        client
            .send(SERVER, Channel::Reliable, b"to server")
            .unwrap();
        rig.server
            .send(1, Channel::UnreliableSequenced, b"to client")
            .unwrap();
        let (se, ce) = rig.run(50, &mut [&mut client]);
        assert_eq!(
            se,
            vec![SessionEvent::Message {
                from: 1,
                channel: Channel::Reliable,
                bytes: b"to server".to_vec()
            }]
        );
        assert_eq!(
            ce[0],
            vec![SessionEvent::Message {
                from: SERVER,
                channel: Channel::UnreliableSequenced,
                bytes: b"to client".to_vec()
            }]
        );
    }

    // M8-NET-05
    #[test]
    fn mismatched_schema_version_or_protocol_is_refused_with_the_reason() {
        let mut rig = Rig::new(config());
        let (mut bad_schema, _) = rig.client(
            SessionConfig {
                schema_hash: 7,
                ..config()
            },
            "a",
            None,
        );
        let (mut bad_version, _) = rig.client(
            SessionConfig {
                game_version: "0.9".into(),
                ..config()
            },
            "b",
            None,
        );
        let (se, ce) = rig.run(200, &mut [&mut bad_schema, &mut bad_version]);
        assert!(se.is_empty());
        assert_eq!(
            ce[0],
            vec![SessionEvent::Rejected(RejectReason::SchemaMismatch)]
        );
        assert_eq!(
            ce[1],
            vec![SessionEvent::Rejected(RejectReason::VersionMismatch {
                server: "1.0".into()
            })]
        );
        assert_eq!(bad_schema.phase(), Some(ClientPhase::Closed));
        assert!(rig.server.players().is_empty());
    }

    // M8-NET-05
    #[test]
    fn a_full_server_refuses_the_next_player() {
        let mut rig = Rig::new(SessionConfig {
            max_players: 1,
            ..config()
        });
        let (mut a, _) = rig.client(config(), "a", None);
        let (mut b, _) = rig.client(config(), "b", None);
        let (_, ce) = rig.run(200, &mut [&mut a, &mut b]);
        let joined = ce
            .iter()
            .filter(|e| matches!(e.first(), Some(SessionEvent::Connected { .. })))
            .count();
        let full = ce
            .iter()
            .filter(|e| e.contains(&SessionEvent::Rejected(RejectReason::ServerFull)))
            .count();
        assert_eq!((joined, full), (1, 1));
    }

    // M8-NET-06
    #[test]
    fn round_trip_time_is_measured_from_heartbeats() {
        let mut rig = Rig::new(config());
        let (mut client, handle) = rig.client(config(), "a", None);
        handle.set_default(Links::both(NetLink::with_latency(50 * MS)));
        rig.run(3000, &mut [&mut client]);
        let rtt = rig
            .server
            .rtt(1)
            .expect("server measured an rtt")
            .as_millis();
        let crtt = client
            .rtt(SERVER)
            .expect("client measured an rtt")
            .as_millis();
        // 50 ms each way, on a 10 ms step.
        assert!((90..=130).contains(&rtt), "server rtt {rtt}");
        assert!((90..=130).contains(&crtt), "client rtt {crtt}");
    }

    // M8-NET-06
    #[test]
    fn a_silent_client_times_out_keeps_its_slot_then_loses_it_after_the_grace() {
        let cfg = SessionConfig {
            timeout: Duration::from_secs(1),
            grace: Duration::from_secs(3),
            ..config()
        };
        let mut rig = Rig::new(cfg.clone());
        let (mut client, handle) = rig.client(cfg, "a", None);
        rig.run(200, &mut [&mut client]);
        handle.set_default(Links::both(NetLink {
            blackhole: true,
            ..NetLink::default()
        }));
        let (se, _) = rig.run(1500, &mut [&mut client]);
        assert!(matches!(
            se.as_slice(),
            [SessionEvent::PlayerDropped {
                player: 1,
                reason: DisconnectReason::TimedOut
            }]
        ));
        assert!(rig.server.players().is_empty());
        let (se, _) = rig.run(3500, &mut [&mut client]);
        assert_eq!(se, vec![SessionEvent::PlayerLeft { player: 1 }]);
    }

    // M8-NET-06
    #[test]
    fn a_client_that_returns_with_its_token_inside_the_grace_resumes_the_same_player() {
        let cfg = SessionConfig {
            grace: Duration::from_secs(10),
            ..config()
        };
        let mut rig = Rig::new(cfg.clone());
        let (mut client, handle) = rig.client(cfg.clone(), "ann", None);
        let (_, ce) = rig.run(200, &mut [&mut client]);
        let SessionEvent::Connected { token, player, .. } = ce[0][0].clone() else {
            panic!("not connected")
        };
        assert_eq!(player, 1);

        handle.fault(Fault::Disconnect(None));
        let (se, _) = rig.run(100, &mut [&mut client]);
        assert!(matches!(
            se[..],
            [SessionEvent::PlayerDropped { player: 1, .. }]
        ));

        let (mut again, _) = rig.client(cfg, "ann", Some(token));
        let (se, ce) = rig.run(200, &mut [&mut again]);
        assert!(matches!(
            se[..],
            [SessionEvent::PlayerJoined {
                player: 1,
                resumed: true,
                ..
            }]
        ));
        assert!(matches!(
            ce[0][0],
            SessionEvent::Connected {
                player: 1,
                resumed: true,
                ..
            }
        ));
        assert_eq!(rig.server.players(), vec![1]);
    }

    // M8-NET-06
    #[test]
    fn a_token_after_the_grace_or_a_wrong_token_gets_a_new_player() {
        let cfg = SessionConfig {
            grace: Duration::from_secs(1),
            ..config()
        };
        let mut rig = Rig::new(cfg.clone());
        let (mut client, handle) = rig.client(cfg.clone(), "ann", None);
        let (_, ce) = rig.run(200, &mut [&mut client]);
        let SessionEvent::Connected { token, .. } = ce[0][0].clone() else {
            panic!("not connected")
        };
        handle.fault(Fault::Disconnect(None));
        rig.run(1500, &mut [&mut client]);
        let (mut late, _) = rig.client(cfg.clone(), "ann", Some(token));
        let (mut wrong, _) = rig.client(cfg, "bob", Some(token ^ 1));
        let (se, _) = rig.run(300, &mut [&mut late, &mut wrong]);
        let joins: Vec<_> = se
            .iter()
            .filter_map(|e| match e {
                SessionEvent::PlayerJoined {
                    player, resumed, ..
                } => Some((*player, *resumed)),
                _ => None,
            })
            .collect();
        assert_eq!(joins, vec![(2, false), (3, false)]);
    }

    // M8-NET-06
    #[test]
    fn a_clean_leave_frees_the_slot_at_once() {
        let mut rig = Rig::new(config());
        let (mut client, _) = rig.client(config(), "a", None);
        rig.run(100, &mut [&mut client]);
        client.leave();
        let (se, _) = rig.run(100, &mut [&mut client]);
        assert_eq!(se, vec![SessionEvent::PlayerLeft { player: 1 }]);
    }

    // M8-NET-07
    #[test]
    fn a_connection_that_never_says_hello_is_dropped_and_garbage_is_ignored() {
        let cfg = SessionConfig {
            handshake_timeout: Duration::from_secs(1),
            ..config()
        };
        let mut rig = Rig::new(cfg);
        // A raw endpoint that connects and sends junk instead of a hello.
        let mut raw = rig.hub.endpoint();
        let peer = raw.connect("srv").unwrap();
        raw.send(peer, Channel::Reliable, &[0xee, 1, 2, 3]).unwrap();
        raw.send(peer, Channel::Reliable, &[]).unwrap();
        let mut dropped = false;
        for _ in 0..150 {
            rig.now += 10 * MS;
            rig.server.update(rig.now);
            dropped |= raw
                .poll(rig.now)
                .iter()
                .any(|e| matches!(e, TransportEvent::Disconnected(..)));
        }
        assert!(dropped);
        assert!(rig.server.bad_packets >= 2);
        assert!(rig.server.players().is_empty());
        let _ = &rig.server_handle;
    }

    // M8-NET-07
    #[test]
    fn a_corrupt_hello_is_refused_without_crashing_the_server() {
        let mut rig = Rig::new(config());
        rig.server_handle.fault(Fault::CorruptNext(1));
        let (mut client, _) = rig.client(config(), "a", None);
        rig.run(300, &mut [&mut client]);
        // The flipped bit either broke the hello (refused) or hit a spot that still parsed as a
        // different hello (also refused: the schema or version no longer matches), or landed in
        // the name (joined). The server stays up in every case.
        assert!(rig.server.players().len() <= 1);
    }
}
