// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! An in-process transport: endpoints of one [`LoopbackHub`] connect by address and pass bytes
//! through queues. Every channel is reliable and in order, and delivery is immediate (the next
//! `poll`); wrap it in `Conditioned` to make it behave like a network.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::transport::{
    Channel, DisconnectReason, NetError, PeerId, Time, Transport, TransportEvent,
};

#[derive(Default)]
struct Hub {
    next_id: u64,
    /// Address to endpoint id, for endpoints that listen.
    listening: HashMap<String, u64>,
    inbox: HashMap<u64, VecDeque<TransportEvent>>,
    /// Which endpoints each endpoint is connected to.
    links: HashMap<u64, BTreeSet<u64>>,
}

/// A set of loopback endpoints that can reach each other.
#[derive(Clone, Default)]
pub struct LoopbackHub {
    inner: Arc<Mutex<Hub>>,
}

impl LoopbackHub {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> MutexGuard<'_, Hub> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A new endpoint. Its peers are named by the other endpoint's id.
    pub fn endpoint(&self) -> LoopbackTransport {
        let mut hub = self.lock();
        hub.next_id += 1;
        let id = hub.next_id;
        hub.inbox.insert(id, VecDeque::new());
        hub.links.insert(id, BTreeSet::new());
        LoopbackTransport {
            hub: self.clone(),
            id,
            address: None,
        }
    }
}

/// One end of the hub.
pub struct LoopbackTransport {
    hub: LoopbackHub,
    id: u64,
    address: Option<String>,
}

impl LoopbackTransport {
    /// Accepts connections to `address`.
    pub fn listen(&mut self, address: &str) -> Result<(), NetError> {
        let mut hub = self.hub.lock();
        if hub.listening.contains_key(address) {
            return Err(NetError::BadAddress(format!("{address} is in use")));
        }
        hub.listening.insert(address.to_owned(), self.id);
        self.address = Some(address.to_owned());
        Ok(())
    }

    /// Connects to a listening endpoint; both sides get `Connected` on their next `poll`.
    pub fn connect(&mut self, address: &str) -> Result<PeerId, NetError> {
        let mut hub = self.hub.lock();
        let server = *hub
            .listening
            .get(address)
            .ok_or_else(|| NetError::BadAddress(format!("nothing listens on {address}")))?;
        hub.links.entry(self.id).or_default().insert(server);
        hub.links.entry(server).or_default().insert(self.id);
        hub.inbox
            .entry(server)
            .or_default()
            .push_back(TransportEvent::Connected(self.id));
        hub.inbox
            .entry(self.id)
            .or_default()
            .push_back(TransportEvent::Connected(server));
        Ok(server)
    }

    /// This endpoint's own id (what the other side calls it).
    pub fn id(&self) -> PeerId {
        self.id
    }
}

impl Transport for LoopbackTransport {
    fn send(&mut self, peer: PeerId, channel: Channel, data: &[u8]) -> Result<(), NetError> {
        let mut hub = self.hub.lock();
        if !hub.links.get(&self.id).is_some_and(|l| l.contains(&peer)) {
            return Err(NetError::UnknownPeer(peer));
        }
        hub.inbox
            .entry(peer)
            .or_default()
            .push_back(TransportEvent::Data {
                peer: self.id,
                channel,
                bytes: data.to_vec(),
            });
        Ok(())
    }

    fn poll(&mut self, _now: Time) -> Vec<TransportEvent> {
        let mut hub = self.hub.lock();
        hub.inbox
            .get_mut(&self.id)
            .map(|q| q.drain(..).collect())
            .unwrap_or_default()
    }

    fn disconnect(&mut self, peer: PeerId) {
        let mut hub = self.hub.lock();
        if !hub.links.get_mut(&self.id).is_some_and(|l| l.remove(&peer)) {
            return;
        }
        if let Some(l) = hub.links.get_mut(&peer) {
            l.remove(&self.id);
        }
        hub.inbox
            .entry(self.id)
            .or_default()
            .push_back(TransportEvent::Disconnected(peer, DisconnectReason::Local));
        hub.inbox
            .entry(peer)
            .or_default()
            .push_back(TransportEvent::Disconnected(
                self.id,
                DisconnectReason::Remote,
            ));
    }

    fn peers(&self) -> Vec<PeerId> {
        self.hub
            .lock()
            .links
            .get(&self.id)
            .map(|l| l.iter().copied().collect())
            .unwrap_or_default()
    }
}

impl Drop for LoopbackTransport {
    fn drop(&mut self) {
        for peer in self.peers() {
            self.disconnect(peer);
        }
        let mut hub = self.hub.lock();
        if let Some(a) = &self.address {
            hub.listening.remove(a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (LoopbackTransport, LoopbackTransport, PeerId, PeerId) {
        let hub = LoopbackHub::new();
        let mut server = hub.endpoint();
        let mut client = hub.endpoint();
        server.listen("game").unwrap();
        let s = client.connect("game").unwrap();
        let c = client.id();
        (server, client, s, c)
    }

    // M8-NET-01
    #[test]
    fn connect_reaches_both_sides_and_data_flows_in_order() {
        let (mut server, mut client, s, c) = pair();
        assert_eq!(server.poll(Time::ZERO), vec![TransportEvent::Connected(c)]);
        assert_eq!(client.poll(Time::ZERO), vec![TransportEvent::Connected(s)]);
        client.send(s, Channel::Reliable, b"one").unwrap();
        client.send(s, Channel::Unreliable, b"two").unwrap();
        let got = server.poll(Time::ZERO);
        assert_eq!(
            got,
            vec![
                TransportEvent::Data {
                    peer: c,
                    channel: Channel::Reliable,
                    bytes: b"one".to_vec()
                },
                TransportEvent::Data {
                    peer: c,
                    channel: Channel::Unreliable,
                    bytes: b"two".to_vec()
                },
            ]
        );
    }

    // M8-NET-01
    #[test]
    fn disconnect_tells_both_sides_and_sends_then_fail() {
        let (mut server, mut client, s, c) = pair();
        server.poll(Time::ZERO);
        client.poll(Time::ZERO);
        client.disconnect(s);
        assert_eq!(
            client.poll(Time::ZERO),
            vec![TransportEvent::Disconnected(s, DisconnectReason::Local)]
        );
        assert_eq!(
            server.poll(Time::ZERO),
            vec![TransportEvent::Disconnected(c, DisconnectReason::Remote)]
        );
        assert_eq!(
            client.send(s, Channel::Reliable, b"x"),
            Err(NetError::UnknownPeer(s))
        );
        assert!(server.peers().is_empty());
    }

    // M8-NET-01
    #[test]
    fn connecting_to_nothing_or_listening_twice_fails() {
        let hub = LoopbackHub::new();
        let mut a = hub.endpoint();
        let mut b = hub.endpoint();
        assert!(a.connect("nope").is_err());
        a.listen("x").unwrap();
        assert!(b.listen("x").is_err());
    }
}
