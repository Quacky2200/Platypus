// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Multiplayer's lower layers (M8.b; design `docs/research/multiplayer.md`).
//!
//! - [`transport`]: the [`Transport`] trait every way of moving bytes implements.
//! - [`loopback`]: an in-process transport for tests and single-machine games.
//! - [`conditioned`] (feature `net-debug`): latency, loss, duplication, reordering, bandwidth caps,
//!   blackholes, disconnects and freezes on top of any transport, seeded so a run replays.
//! - [`session`]: the handshake (protocol, schema and game version), player ids, reconnect tokens,
//!   heartbeats and round-trip time, timeouts and the slot grace period.
//! - [`wire`]: the byte format (fixed-width, varints, strings) that reads without panicking.

#[cfg(feature = "net-debug")]
pub mod conditioned;
pub mod loopback;
pub mod session;
pub mod transport;
pub mod wire;

pub use loopback::{LoopbackHub, LoopbackTransport};
pub use session::{
    ClientPhase, PROTOCOL_VERSION, PlayerId, RejectReason, SERVER, Session, SessionConfig,
    SessionEvent, Token,
};
pub use transport::{Channel, DisconnectReason, NetError, PeerId, Time, Transport, TransportEvent};
