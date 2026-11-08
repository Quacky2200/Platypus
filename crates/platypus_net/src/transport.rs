// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The transport trait: how bytes move between peers. Everything above it (session, replication)
//! sees only this, so loopback, QUIC, Steam and the fault-injecting wrapper are interchangeable.

use std::fmt;
use std::time::Duration;

/// A transport's name for the other end of a connection. Unique within one transport.
pub type PeerId = u64;

/// Monotonic time since the run started. Passed in, never read from the clock, so tests and the
/// fault wrapper are deterministic.
pub type Time = Duration;

/// What a message promises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// May be lost, duplicated or reordered (state snapshots, voice).
    Unreliable,
    /// May be lost; a message older than one already delivered is dropped (inputs, positions).
    UnreliableSequenced,
    /// Arrives, in order (spawns, RPCs).
    Reliable,
    /// Arrives, in any order (large one-off transfers).
    ReliableUnordered,
}

impl Channel {
    pub const ALL: [Channel; 4] = [
        Channel::Unreliable,
        Channel::UnreliableSequenced,
        Channel::Reliable,
        Channel::ReliableUnordered,
    ];

    pub fn is_reliable(self) -> bool {
        matches!(self, Channel::Reliable | Channel::ReliableUnordered)
    }

    pub fn is_ordered(self) -> bool {
        matches!(self, Channel::Reliable | Channel::UnreliableSequenced)
    }
}

/// Why a connection ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectReason {
    /// This side asked.
    Local,
    /// The other side asked or closed.
    Remote,
    /// Nothing arrived for too long.
    TimedOut,
    /// The transport failed (the text is for the log).
    Error(String),
}

/// Something that happened on a transport since the last `poll`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportEvent {
    /// A connection is up (made by `connect`, or accepted).
    Connected(PeerId),
    Disconnected(PeerId, DisconnectReason),
    Data {
        peer: PeerId,
        channel: Channel,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetError {
    /// No such connection.
    UnknownPeer(PeerId),
    /// The message does not fit the channel (an unreliable datagram past the MTU).
    TooLarge {
        len: usize,
        max: usize,
    },
    /// The address could not be used.
    BadAddress(String),
    /// The transport has stopped.
    Closed,
    Other(String),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::UnknownPeer(p) => write!(f, "no connection to peer {p}"),
            NetError::TooLarge { len, max } => {
                write!(f, "message of {len} bytes over the {max} limit")
            }
            NetError::BadAddress(a) => write!(f, "bad address {a:?}"),
            NetError::Closed => write!(f, "the transport is closed"),
            NetError::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for NetError {}

/// A way to move bytes. Polled from one thread (the game's); implementations that use threads
/// hand their events over inside `poll`.
pub trait Transport: Send {
    /// Sends `data` to `peer` on `channel`. Reliable channels queue; unreliable ones may drop.
    fn send(&mut self, peer: PeerId, channel: Channel, data: &[u8]) -> Result<(), NetError>;

    /// Moves the transport on to `now` and returns what happened since the last call, in order.
    fn poll(&mut self, now: Time) -> Vec<TransportEvent>;

    /// Closes a connection. The peer sees `Disconnected(.., Remote)`; this side sees
    /// `Disconnected(.., Local)` on the next `poll`.
    fn disconnect(&mut self, peer: PeerId);

    /// The live connections.
    fn peers(&self) -> Vec<PeerId>;

    /// The largest message an unreliable channel carries.
    fn max_unreliable(&self) -> usize {
        1200
    }
}

impl<T: Transport + ?Sized> Transport for Box<T> {
    fn send(&mut self, peer: PeerId, channel: Channel, data: &[u8]) -> Result<(), NetError> {
        (**self).send(peer, channel, data)
    }
    fn poll(&mut self, now: Time) -> Vec<TransportEvent> {
        (**self).poll(now)
    }
    fn disconnect(&mut self, peer: PeerId) {
        (**self).disconnect(peer)
    }
    fn peers(&self) -> Vec<PeerId> {
        (**self).peers()
    }
    fn max_unreliable(&self) -> usize {
        (**self).max_unreliable()
    }
}
