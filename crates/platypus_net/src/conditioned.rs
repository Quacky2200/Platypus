// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Conditioned<T>`: any transport made to behave like a bad network, for tests and the arena's
//! debug panel (design: `docs/research/multiplayer.md` §9.1). Debug only: this module is the
//! `net-debug` feature, and scripts cannot reach it.
//!
//! Each direction of each peer is a [`Link`]: latency and jitter, loss (independent or in bursts),
//! duplication, reordering, a bandwidth cap and a blackhole. Reliable channels never lose a message,
//! as in a real stack: a loss costs a retransmit delay and keeps order. Everything random comes from
//! a seeded generator, so a failing run replays exactly. The knobs live in a [`Handle`] that stays
//! with the caller after the transport moves into a session.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::transport::{
    Channel, DisconnectReason, NetError, PeerId, Time, Transport, TransportEvent,
};

/// One direction's conditions.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub latency: Duration,
    /// Each message's delay moves by up to this much either way (uniform).
    pub jitter: Duration,
    /// Chance an unreliable message is lost (0 to 1).
    pub loss: f64,
    /// Mean run of consecutive losses; 0 or 1 means each loss is independent.
    pub burst: f64,
    /// Chance an unreliable message arrives twice.
    pub duplicate: f64,
    /// Chance an unreliable message is held back by up to twice the latency (+10 ms).
    pub reorder: f64,
    /// Bytes a second the link carries; `None` for unlimited.
    pub bandwidth: Option<u64>,
    /// Everything vanishes, though the connection looks open.
    pub blackhole: bool,
}

impl Default for Link {
    fn default() -> Self {
        Link {
            latency: Duration::ZERO,
            jitter: Duration::ZERO,
            loss: 0.0,
            burst: 0.0,
            duplicate: 0.0,
            reorder: 0.0,
            bandwidth: None,
            blackhole: false,
        }
    }
}

impl Link {
    pub fn with_latency(latency: Duration) -> Self {
        Link {
            latency,
            ..Link::default()
        }
    }
}

/// Both directions of a peer: `out` (what this side sends), `inbound` (what it receives).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Links {
    pub out: Link,
    pub inbound: Link,
}

impl Links {
    /// The same conditions both ways.
    pub fn both(link: Link) -> Self {
        Links {
            out: link.clone(),
            inbound: link,
        }
    }
}

/// Things that happen once, at the next `poll`.
#[derive(Debug, Clone, PartialEq)]
pub enum Fault {
    /// Closes the connection to a peer (`None`: every peer).
    Disconnect(Option<PeerId>),
    /// The process stalls (a debugger stop, a hitch): nothing is sent or received for this long.
    Freeze(Duration),
    /// Flips a bit in each of the next `n` received messages (the codec must refuse them).
    CorruptNext(u32),
}

#[derive(Default)]
struct Control {
    default: Links,
    per_peer: HashMap<PeerId, Links>,
    faults: Vec<Fault>,
    /// Counters for the panel and tests.
    stats: Stats,
}

/// What the wrapper did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub sent: u64,
    pub lost: u64,
    pub duplicated: u64,
    pub delayed_reliable: u64,
    pub corrupted: u64,
}

/// The caller's grip on a [`Conditioned`] transport's conditions.
#[derive(Clone, Default)]
pub struct Handle {
    inner: Arc<Mutex<Control>>,
}

impl Handle {
    fn lock(&self) -> MutexGuard<'_, Control> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The conditions for peers without their own.
    pub fn set_default(&self, links: Links) {
        self.lock().default = links;
    }

    pub fn set_peer(&self, peer: PeerId, links: Links) {
        self.lock().per_peer.insert(peer, links);
    }

    pub fn clear_peer(&self, peer: PeerId) {
        self.lock().per_peer.remove(&peer);
    }

    pub fn fault(&self, fault: Fault) {
        self.lock().faults.push(fault);
    }

    pub fn stats(&self) -> Stats {
        self.lock().stats
    }

    fn links_for(&self, peer: PeerId) -> Links {
        let c = self.lock();
        c.per_peer.get(&peer).unwrap_or(&c.default).clone()
    }
}

/// A seeded generator (splitmix64): the same seed gives the same network.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// In `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        p > 0.0 && self.next_f64() < p
    }
}

/// How far ahead a bandwidth-capped link may queue unreliable data before it drops (tail drop),
/// so a flood cannot grow memory without bound.
const MAX_QUEUE_DELAY: Duration = Duration::from_secs(2);
/// Retransmit delay of a lost reliable message, beyond the latency's round trip.
const RTO_FLOOR: Duration = Duration::from_millis(20);
/// A reliable message is retransmitted at most this often before it gets through.
const MAX_RETRANSMITS: u32 = 8;

/// One direction's queue of messages in flight.
struct Lane<T> {
    queue: BTreeMap<(Time, u64), T>,
    seq: u64,
    /// Burst-loss state (Gilbert-Elliott): in a bad run every message is lost.
    bad: bool,
    /// When the link finishes sending what it has taken (the bandwidth cap).
    free_at: Time,
    /// The latest delivery time given to an in-order reliable message.
    last_ordered: Time,
    /// The newest sequenced message delivered (later arrivals older than it are dropped).
    last_sequenced: Option<u64>,
}

impl<T> Default for Lane<T> {
    fn default() -> Self {
        Lane {
            queue: BTreeMap::new(),
            seq: 0,
            bad: false,
            free_at: Time::ZERO,
            last_ordered: Time::ZERO,
            last_sequenced: None,
        }
    }
}

/// What a lane decided for a message.
enum Fate {
    Lost,
    Deliver {
        at: Time,
        copies: u32,
        retransmits: u32,
    },
}

impl<T: Clone> Lane<T> {
    fn fate(
        &mut self,
        link: &Link,
        rng: &mut Rng,
        now: Time,
        channel: Channel,
        size: usize,
    ) -> Fate {
        if link.blackhole {
            return Fate::Lost;
        }
        let mut retransmits = 0;
        if channel.is_reliable() {
            while retransmits < MAX_RETRANSMITS && rng.chance(link.loss) {
                retransmits += 1;
            }
        } else if self.lose(link, rng) {
            return Fate::Lost;
        }
        // The cap: the message waits for the link, then takes size / bandwidth to send.
        let mut start = now.max(self.free_at);
        if let Some(bw) = link.bandwidth.filter(|b| *b > 0) {
            if !channel.is_reliable() && start.saturating_sub(now) > MAX_QUEUE_DELAY {
                return Fate::Lost;
            }
            let send = Duration::from_secs_f64(size as f64 / bw as f64);
            self.free_at = start + send;
            start += send;
        }
        let jitter = link.jitter.as_secs_f64() * (rng.next_f64() * 2.0 - 1.0);
        let mut delay = (link.latency.as_secs_f64() + jitter).max(0.0);
        let rto = (link.latency * 2).max(RTO_FLOOR).as_secs_f64();
        delay += rto * f64::from(retransmits);
        if !channel.is_reliable() && rng.chance(link.reorder) {
            delay += rng.next_f64() * (link.latency.as_secs_f64() * 2.0 + 0.010);
        }
        let mut at = start + Duration::from_secs_f64(delay);
        if channel == Channel::Reliable {
            // In order: nothing overtakes an earlier message (head-of-line blocking).
            at = at.max(self.last_ordered);
            self.last_ordered = at;
        }
        let copies = if !channel.is_reliable() && rng.chance(link.duplicate) {
            2
        } else {
            1
        };
        Fate::Deliver {
            at,
            copies,
            retransmits,
        }
    }

    fn lose(&mut self, link: &Link, rng: &mut Rng) -> bool {
        if link.loss <= 0.0 {
            self.bad = false;
            return false;
        }
        if link.burst <= 1.0 {
            return rng.chance(link.loss);
        }
        // Enter a bad run so that the long-run loss rate is `loss`; leave with 1 / burst.
        let leave = 1.0 / link.burst;
        let enter = (link.loss * leave / (1.0 - link.loss.min(0.99))).min(1.0);
        if self.bad {
            if rng.chance(leave) {
                self.bad = false;
            }
        } else if rng.chance(enter) {
            self.bad = true;
        }
        self.bad
    }

    fn push(&mut self, at: Time, item: T) -> u64 {
        self.seq += 1;
        self.queue.insert((at, self.seq), item);
        self.seq
    }

    /// Takes what is due at `now`, in delivery order (with each item's sequence number).
    fn due(&mut self, now: Time) -> Vec<(u64, T)> {
        let mut out = Vec::new();
        while let Some(entry) = self.queue.first_entry() {
            if entry.key().0 > now {
                break;
            }
            let seq = entry.key().1;
            out.push((seq, entry.remove()));
        }
        out
    }
}

#[derive(Clone)]
struct Outgoing {
    channel: Channel,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct PeerState {
    out: Lane<Outgoing>,
    inbound: Lane<Outgoing>,
}

/// A transport with conditions applied to what it sends and receives.
pub struct Conditioned<T: Transport> {
    inner: T,
    handle: Handle,
    rng: Rng,
    now: Time,
    frozen_until: Time,
    corrupt: u32,
    peers: HashMap<PeerId, PeerState>,
    /// Closed connections whose last messages are still in flight.
    closing: HashMap<PeerId, DisconnectReason>,
    /// Peers this side closed, once what it queued for them has gone out.
    closing_local: std::collections::HashSet<PeerId>,
}

impl<T: Transport> Conditioned<T> {
    /// Wraps `inner`; the returned [`Handle`] sets conditions and fires faults.
    pub fn new(inner: T, seed: u64) -> (Self, Handle) {
        let handle = Handle::default();
        (
            Conditioned {
                inner,
                handle: handle.clone(),
                rng: Rng::new(seed),
                now: Time::ZERO,
                frozen_until: Time::ZERO,
                corrupt: 0,
                peers: HashMap::new(),
                closing: HashMap::new(),
                closing_local: Default::default(),
            },
            handle,
        )
    }

    pub fn inner(&mut self) -> &mut T {
        &mut self.inner
    }

    fn apply_faults(&mut self) {
        let faults = std::mem::take(&mut self.handle.lock().faults);
        for fault in faults {
            match fault {
                Fault::Disconnect(Some(peer)) => self.inner.disconnect(peer),
                Fault::Disconnect(None) => {
                    for peer in self.inner.peers() {
                        self.inner.disconnect(peer);
                    }
                }
                Fault::Freeze(d) => self.frozen_until = self.now + d,
                Fault::CorruptNext(n) => self.corrupt += n,
            }
        }
    }

    fn add_stats(&self, f: impl FnOnce(&mut Stats)) {
        f(&mut self.handle.lock().stats);
    }
}

impl<T: Transport> Transport for Conditioned<T> {
    fn send(&mut self, peer: PeerId, channel: Channel, data: &[u8]) -> Result<(), NetError> {
        if !self.inner.peers().contains(&peer) {
            return Err(NetError::UnknownPeer(peer));
        }
        let links = self.handle.links_for(peer);
        self.add_stats(|s| s.sent += 1);
        let fate = self.peers.entry(peer).or_default().out.fate(
            &links.out,
            &mut self.rng,
            self.now,
            channel,
            data.len(),
        );
        match fate {
            Fate::Lost => self.add_stats(|s| s.lost += 1),
            Fate::Deliver {
                at,
                copies,
                retransmits,
            } => {
                let state = self.peers.entry(peer).or_default();
                for _ in 0..copies {
                    state.out.push(
                        at,
                        Outgoing {
                            channel,
                            bytes: data.to_vec(),
                        },
                    );
                }
                if copies > 1 {
                    self.add_stats(|s| s.duplicated += 1);
                }
                if retransmits > 0 {
                    self.add_stats(|s| s.delayed_reliable += 1);
                }
            }
        }
        Ok(())
    }

    fn poll(&mut self, now: Time) -> Vec<TransportEvent> {
        self.now = now;
        self.apply_faults();
        if now < self.frozen_until {
            return Vec::new();
        }
        // Outgoing messages whose time has come go to the real transport.
        for (peer, state) in &mut self.peers {
            for (_, m) in state.out.due(now) {
                let _ = self.inner.send(*peer, m.channel, &m.bytes);
            }
        }
        let drained: Vec<PeerId> = self
            .closing_local
            .iter()
            .filter(|p| self.peers.get(p).is_none_or(|s| s.out.queue.is_empty()))
            .copied()
            .collect();
        for peer in drained {
            self.closing_local.remove(&peer);
            self.inner.disconnect(peer);
        }
        let mut events = Vec::new();
        for event in self.inner.poll(now) {
            match event {
                TransportEvent::Data {
                    peer,
                    channel,
                    mut bytes,
                } => {
                    if self.corrupt > 0 && !bytes.is_empty() {
                        self.corrupt -= 1;
                        let i = (self.rng.next_u64() as usize) % bytes.len();
                        bytes[i] ^= 1 << (self.rng.next_u64() % 8);
                        self.add_stats(|s| s.corrupted += 1);
                    }
                    let links = self.handle.links_for(peer);
                    let state = self.peers.entry(peer).or_default();
                    match state.inbound.fate(
                        &links.inbound,
                        &mut self.rng,
                        now,
                        channel,
                        bytes.len(),
                    ) {
                        Fate::Lost => self.add_stats(|s| s.lost += 1),
                        Fate::Deliver { at, copies, .. } => {
                            let state = self.peers.entry(peer).or_default();
                            for _ in 0..copies {
                                state.inbound.push(
                                    at,
                                    Outgoing {
                                        channel,
                                        bytes: bytes.clone(),
                                    },
                                );
                            }
                        }
                    }
                }
                TransportEvent::Disconnected(peer, reason) => {
                    // What arrived before the close is still in flight: the close waits for it.
                    if self
                        .peers
                        .get(&peer)
                        .is_some_and(|s| !s.inbound.queue.is_empty())
                    {
                        self.closing.insert(peer, reason);
                    } else {
                        self.peers.remove(&peer);
                        events.push(TransportEvent::Disconnected(peer, reason));
                    }
                }
                other => events.push(other),
            }
        }
        // Received messages whose time has come, newest-wins for sequenced ones.
        for (peer, state) in &mut self.peers {
            let ready = state.inbound.due(now);
            for (seq, m) in ready {
                if m.channel == Channel::UnreliableSequenced {
                    if state.inbound.last_sequenced.is_some_and(|last| seq < last) {
                        continue;
                    }
                    state.inbound.last_sequenced = Some(seq);
                }
                events.push(TransportEvent::Data {
                    peer: *peer,
                    channel: m.channel,
                    bytes: m.bytes,
                });
            }
        }
        // Closes that were waiting for their last messages.
        let finished: Vec<PeerId> = self
            .closing
            .keys()
            .filter(|p| self.peers.get(p).is_none_or(|s| s.inbound.queue.is_empty()))
            .copied()
            .collect();
        for peer in finished {
            if let Some(reason) = self.closing.remove(&peer) {
                self.peers.remove(&peer);
                events.push(TransportEvent::Disconnected(peer, reason));
            }
        }
        events
    }

    fn disconnect(&mut self, peer: PeerId) {
        // What was sent before the close still goes out first (a goodbye, a rejection).
        if self
            .peers
            .get(&peer)
            .is_some_and(|s| !s.out.queue.is_empty())
        {
            self.closing_local.insert(peer);
        } else {
            self.inner.disconnect(peer);
        }
    }

    fn peers(&self) -> Vec<PeerId> {
        self.inner.peers()
    }

    fn max_unreliable(&self) -> usize {
        self.inner.max_unreliable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loopback::{LoopbackHub, LoopbackTransport};
    use crate::transport::DisconnectReason;

    const MS: Duration = Duration::from_millis(1);

    /// A client wrapped in conditions and a plain server, connected.
    fn pair(
        seed: u64,
    ) -> (
        Conditioned<LoopbackTransport>,
        Handle,
        LoopbackTransport,
        PeerId,
        PeerId,
    ) {
        let hub = LoopbackHub::new();
        let mut server = hub.endpoint();
        let mut client = hub.endpoint();
        server.listen("g").unwrap();
        let s = client.connect("g").unwrap();
        let c = client.id();
        let (mut client, handle) = Conditioned::new(client, seed);
        client.poll(Time::ZERO);
        server.poll(Time::ZERO);
        (client, handle, server, s, c)
    }

    fn data(events: Vec<TransportEvent>) -> Vec<Vec<u8>> {
        events
            .into_iter()
            .filter_map(|e| match e {
                TransportEvent::Data { bytes, .. } => Some(bytes),
                _ => None,
            })
            .collect()
    }

    // M8-NET-02
    #[test]
    fn latency_delays_delivery_to_exactly_the_configured_time() {
        let (mut client, handle, mut server, s, _) = pair(1);
        handle.set_default(Links::both(Link::with_latency(100 * MS)));
        client.send(s, Channel::Reliable, b"hi").unwrap();
        client.poll(99 * MS);
        assert!(data(server.poll(99 * MS)).is_empty());
        client.poll(100 * MS);
        assert_eq!(data(server.poll(100 * MS)), vec![b"hi".to_vec()]);
    }

    // M8-NET-02
    #[test]
    fn unreliable_loss_drops_but_reliable_still_arrives_late_and_in_order() {
        let (mut client, handle, mut server, s, _) = pair(2);
        handle.set_default(Links::both(Link {
            loss: 1.0,
            latency: 10 * MS,
            ..Link::default()
        }));
        client.send(s, Channel::Unreliable, b"u").unwrap();
        client.send(s, Channel::Reliable, b"r1").unwrap();
        client.send(s, Channel::Reliable, b"r2").unwrap();
        let mut got = Vec::new();
        for t in 0..400 {
            client.poll(t * 10 * MS);
            got.extend(data(server.poll(t * 10 * MS)));
        }
        assert_eq!(got, vec![b"r1".to_vec(), b"r2".to_vec()]);
        let stats = handle.stats();
        assert_eq!(stats.lost, 1);
        assert_eq!(stats.delayed_reliable, 2);
    }

    // M8-NET-02
    #[test]
    fn bandwidth_cap_spaces_messages_by_their_size() {
        let (mut client, handle, mut server, s, _) = pair(3);
        handle.set_default(Links::both(Link {
            bandwidth: Some(1000),
            ..Link::default()
        }));
        client.send(s, Channel::Reliable, &[0; 500]).unwrap();
        client.send(s, Channel::Reliable, &[1; 500]).unwrap();
        client.poll(500 * MS);
        assert_eq!(data(server.poll(500 * MS)).len(), 1);
        client.poll(999 * MS);
        assert!(data(server.poll(999 * MS)).is_empty());
        client.poll(1000 * MS);
        assert_eq!(data(server.poll(1000 * MS)).len(), 1);
    }

    // M8-NET-02
    #[test]
    fn a_blackhole_swallows_everything_while_the_connection_stays_open() {
        let (mut client, handle, mut server, s, _) = pair(4);
        handle.set_default(Links::both(Link {
            blackhole: true,
            ..Link::default()
        }));
        client.send(s, Channel::Reliable, b"gone").unwrap();
        client.poll(10_000 * MS);
        assert!(data(server.poll(10_000 * MS)).is_empty());
        assert_eq!(client.peers(), vec![s]);
    }

    // M8-NET-02
    #[test]
    fn a_conditioned_receiver_never_delivers_an_older_sequenced_message_after_a_newer_one() {
        for seed in 0..40 {
            let hub = LoopbackHub::new();
            let mut server = hub.endpoint();
            let mut client = hub.endpoint();
            server.listen("g").unwrap();
            let s = client.connect("g").unwrap();
            let (mut server, handle) = Conditioned::new(server, seed);
            server.poll(Time::ZERO);
            handle.set_default(Links::both(Link {
                reorder: 1.0,
                latency: 50 * MS,
                ..Link::default()
            }));
            for i in 0..10u8 {
                client.send(s, Channel::UnreliableSequenced, &[i]).unwrap();
            }
            let mut got = Vec::new();
            for t in 0..100 {
                got.extend(data(server.poll(t * 10 * MS)));
            }
            assert!(!got.is_empty());
            assert!(
                got.windows(2).all(|w| w[0][0] < w[1][0]),
                "seed {seed}: {got:?}"
            );
        }
    }

    // M8-NET-02
    #[test]
    fn the_same_seed_gives_the_same_losses_and_delays() {
        let run = |seed| {
            let (mut client, handle, mut server, s, _) = pair(seed);
            handle.set_default(Links::both(Link {
                loss: 0.3,
                burst: 3.0,
                jitter: 20 * MS,
                latency: 40 * MS,
                duplicate: 0.1,
                reorder: 0.2,
                ..Link::default()
            }));
            let mut got = Vec::new();
            for i in 0..200u32 {
                client
                    .send(s, Channel::Unreliable, &i.to_le_bytes())
                    .unwrap();
                let t = Duration::from_millis(u64::from(i) * 5);
                client.poll(t);
                got.push(data(server.poll(t)));
            }
            (got, handle.stats())
        };
        assert_eq!(run(9), run(9));
        assert_ne!(run(9).0, run(10).0);
    }

    // M8-NET-02
    #[test]
    fn burst_loss_comes_in_runs_near_the_configured_rate() {
        let mut lane: Lane<()> = Lane::default();
        let mut rng = Rng::new(7);
        let link = Link {
            loss: 0.2,
            burst: 5.0,
            ..Link::default()
        };
        let (mut lost, mut runs, mut prev) = (0u32, 0u32, false);
        let n = 20_000;
        for _ in 0..n {
            let l = lane.lose(&link, &mut rng);
            lost += u32::from(l);
            runs += u32::from(l && !prev);
            prev = l;
        }
        let rate = f64::from(lost) / f64::from(n);
        assert!((0.14..0.26).contains(&rate), "loss rate {rate}");
        let mean_run = f64::from(lost) / f64::from(runs);
        assert!(mean_run > 3.0, "mean run {mean_run}");
    }

    // M8-NET-03
    #[test]
    fn disconnect_fault_closes_the_connection_and_freeze_holds_everything() {
        let (mut client, handle, mut server, s, c) = pair(8);
        client.send(s, Channel::Reliable, b"a").unwrap();
        handle.fault(Fault::Freeze(500 * MS));
        assert!(client.poll(10 * MS).is_empty());
        assert!(data(server.poll(10 * MS)).is_empty());
        assert!(client.poll(400 * MS).is_empty());
        client.poll(600 * MS);
        assert_eq!(data(server.poll(600 * MS)), vec![b"a".to_vec()]);

        handle.fault(Fault::Disconnect(None));
        let events = client.poll(700 * MS);
        assert!(events.contains(&TransportEvent::Disconnected(s, DisconnectReason::Local)));
        assert!(
            server
                .poll(700 * MS)
                .contains(&TransportEvent::Disconnected(c, DisconnectReason::Remote))
        );
    }

    // M8-NET-03
    #[test]
    fn corrupt_fault_flips_a_bit_in_the_next_received_messages_only() {
        let hub = LoopbackHub::new();
        let mut server = hub.endpoint();
        let mut client = hub.endpoint();
        server.listen("g").unwrap();
        let s = client.connect("g").unwrap();
        let (mut server, handle) = Conditioned::new(server, 11);
        server.poll(Time::ZERO);
        handle.fault(Fault::CorruptNext(1));
        client.send(s, Channel::Reliable, &[0u8; 8]).unwrap();
        client.send(s, Channel::Reliable, &[0u8; 8]).unwrap();
        let got = data(server.poll(MS));
        assert_eq!(got.len(), 2);
        let ones: u32 = got[0].iter().map(|b| b.count_ones()).sum();
        assert_eq!(ones, 1);
        assert_eq!(got[1], vec![0u8; 8]);
        assert_eq!(handle.stats().corrupted, 1);
    }
}
