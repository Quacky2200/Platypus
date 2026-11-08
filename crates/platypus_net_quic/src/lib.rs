// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The QUIC transport (M8.b, design `docs/research/multiplayer.md` §2).
//!
//! Channels map onto QUIC like this:
//! - `Unreliable`: a datagram, `[0] payload`.
//! - `UnreliableSequenced`: a datagram, `[1] seq:u32 payload`; the receiver drops one older than
//!   the newest it has seen.
//! - `Reliable`: one long-lived unidirectional stream per direction, `[0]` then `len:u32 payload`
//!   messages, so order is the stream's.
//! - `ReliableUnordered`: a stream per message, `[1] payload` to the end.
//!
//! Everything network-facing runs on a private tokio runtime; the game thread only calls
//! [`Transport::send`] and [`Transport::poll`]. A server has a self-signed [`Identity`]; clients
//! pin its [`Identity::fingerprint`] (or, for local development only, accept any certificate).

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use platypus_net::{Channel, DisconnectReason, NetError, PeerId, Time, Transport, TransportEvent};
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::CryptoProvider;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// The ALPN protocol both ends speak.
const ALPN: &[u8] = b"platypus-net/1";
/// The largest reliable message (a hostile length cannot allocate more).
const MAX_MESSAGE: usize = 16 << 20;
/// Datagram header: the kind byte, plus a sequence number on sequenced ones.
const DATAGRAM_HEADER: usize = 5;
const MTU: usize = 1200;

/// How a client decides to trust a server's certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trust {
    /// The SHA-256 of the server's certificate (what [`Identity::fingerprint`] gives).
    Fingerprint([u8; 32]),
    /// Any certificate. Local development only: anyone can impersonate the server.
    InsecureDev,
}

/// A server's self-signed certificate and key.
pub struct Identity {
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
}

impl Identity {
    /// A fresh self-signed identity valid for `names` (host names or addresses).
    pub fn self_signed(names: &[&str]) -> Result<Identity, NetError> {
        let names: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
        let made = rcgen::generate_simple_self_signed(names)
            .map_err(|e| NetError::Other(format!("certificate: {e}")))?;
        let key = PrivateKeyDer::try_from(made.signing_key.serialize_der())
            .map_err(|e| NetError::Other(format!("certificate key: {e}")))?;
        Ok(Identity {
            cert: made.cert.der().clone(),
            key,
        })
    }

    /// What clients pin: the SHA-256 of the certificate.
    pub fn fingerprint(&self) -> [u8; 32] {
        sha256(&self.cert)
    }

    pub fn fingerprint_hex(&self) -> String {
        hex(&self.fingerprint())
    }
}

fn sha256(cert: &CertificateDer<'_>) -> [u8; 32] {
    let d = ring::digest::digest(&ring::digest::SHA256, cert.as_ref());
    let mut out = [0u8; 32];
    out.copy_from_slice(d.as_ref());
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads a fingerprint written by [`hex`].
pub fn parse_fingerprint(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Accepts the one certificate with the pinned fingerprint (or any, in `InsecureDev`).
#[derive(Debug)]
struct Pin {
    expected: Option<[u8; 32]>,
    provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for Pin {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        match self.expected {
            Some(fp) if sha256(end_entity) != fp => Err(rustls::Error::General(
                "the server's certificate is not the pinned one".into(),
            )),
            _ => Ok(ServerCertVerified::assertion()),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn transport_config() -> Arc<quinn::TransportConfig> {
    let mut t = quinn::TransportConfig::default();
    t.keep_alive_interval(Some(Duration::from_secs(2)));
    t.max_idle_timeout(Some(
        Duration::from_secs(15)
            .try_into()
            .expect("15 s fits an idle timeout"),
    ));
    t.datagram_receive_buffer_size(Some(1 << 20));
    t.datagram_send_buffer_size(1 << 20);
    Arc::new(t)
}

enum ConnCmd {
    Send { channel: Channel, bytes: Vec<u8> },
    Close,
}

type Registry = Arc<Mutex<HashMap<PeerId, UnboundedSender<ConnCmd>>>>;

/// QUIC as a [`Transport`]: a server made with [`QuicTransport::listen`], a client with
/// [`QuicTransport::client`] then [`QuicTransport::connect`].
pub struct QuicTransport {
    rt: Option<Runtime>,
    registry: Registry,
    events: Receiver<TransportEvent>,
    event_tx: Sender<TransportEvent>,
    next_peer: Arc<AtomicU64>,
    /// Peers whose `Connected` has been polled.
    connected: HashSet<PeerId>,
    endpoint: Option<quinn::Endpoint>,
}

impl QuicTransport {
    fn new() -> Result<Self, NetError> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("platypus-quic")
            .enable_all()
            .build()
            .map_err(|e| NetError::Other(format!("runtime: {e}")))?;
        let (event_tx, events) = channel();
        Ok(QuicTransport {
            rt: Some(rt),
            registry: Registry::default(),
            events,
            event_tx,
            next_peer: Arc::new(AtomicU64::new(1)),
            connected: HashSet::new(),
            endpoint: None,
        })
    }

    fn rt(&self) -> &Runtime {
        self.rt.as_ref().expect("the runtime lives until drop")
    }

    /// Listens on `addr` (port 0 picks one: see [`QuicTransport::local_addr`]).
    pub fn listen(addr: SocketAddr, identity: &Identity) -> Result<Self, NetError> {
        let mut t = Self::new()?;
        let mut tls = rustls::ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| NetError::Other(e.to_string()))?
            .with_no_client_auth()
            .with_single_cert(vec![identity.cert.clone()], identity.key.clone_key())
            .map_err(|e| NetError::Other(e.to_string()))?;
        tls.alpn_protocols = vec![ALPN.to_vec()];
        let crypto = QuicServerConfig::try_from(tls).map_err(|e| NetError::Other(e.to_string()))?;
        let mut config = quinn::ServerConfig::with_crypto(Arc::new(crypto));
        config.transport_config(transport_config());
        let endpoint = {
            let _guard = t.rt().enter();
            quinn::Endpoint::server(config, addr)
                .map_err(|e| NetError::BadAddress(format!("{addr}: {e}")))?
        };
        t.endpoint = Some(endpoint.clone());
        let (registry, tx, next) = (t.registry.clone(), t.event_tx.clone(), t.next_peer.clone());
        t.rt().spawn(async move {
            while let Some(incoming) = endpoint.accept().await {
                let (registry, tx, next) = (registry.clone(), tx.clone(), next.clone());
                tokio::spawn(async move {
                    let conn = match incoming.await {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::debug!("quic: incoming connection failed: {e}");
                            return;
                        }
                    };
                    let peer = next.fetch_add(1, Ordering::Relaxed);
                    let (cmd_tx, cmd_rx) = unbounded_channel();
                    registry
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(peer, cmd_tx);
                    let _ = tx.send(TransportEvent::Connected(peer));
                    run_connection(peer, conn, cmd_rx, tx, registry).await;
                });
            }
        });
        Ok(t)
    }

    /// A transport that only connects out.
    pub fn client() -> Result<Self, NetError> {
        Self::new()
    }

    /// The address a server listens on.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.endpoint.as_ref().and_then(|e| e.local_addr().ok())
    }

    /// Starts connecting to a server; `Connected` (or `Disconnected` with the failure) comes from
    /// `poll`. `server_name` is the name the certificate is for (`"localhost"` for a self-signed one).
    pub fn connect(
        &mut self,
        addr: SocketAddr,
        server_name: &str,
        trust: &Trust,
    ) -> Result<PeerId, NetError> {
        let expected = match trust {
            Trust::Fingerprint(fp) => Some(*fp),
            Trust::InsecureDev => None,
        };
        let provider = provider();
        let mut tls = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| NetError::Other(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(Pin { expected, provider }))
            .with_no_client_auth();
        tls.alpn_protocols = vec![ALPN.to_vec()];
        let crypto = QuicClientConfig::try_from(tls).map_err(|e| NetError::Other(e.to_string()))?;
        let mut config = quinn::ClientConfig::new(Arc::new(crypto));
        config.transport_config(transport_config());
        if self.endpoint.is_none() {
            let bind: SocketAddr = if addr.is_ipv6() {
                "[::]:0".parse().expect("a literal address")
            } else {
                "0.0.0.0:0".parse().expect("a literal address")
            };
            let _guard = self.rt().enter();
            self.endpoint = Some(
                quinn::Endpoint::client(bind)
                    .map_err(|e| NetError::Other(format!("socket: {e}")))?,
            );
        }
        let endpoint = self.endpoint.clone().expect("made above");
        let connecting = {
            let _guard = self.rt().enter();
            endpoint
                .connect_with(config, addr, server_name)
                .map_err(|e| NetError::BadAddress(e.to_string()))?
        };
        let peer = self.next_peer.fetch_add(1, Ordering::Relaxed);
        let (cmd_tx, cmd_rx) = unbounded_channel();
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(peer, cmd_tx);
        let (tx, registry) = (self.event_tx.clone(), self.registry.clone());
        self.rt().spawn(async move {
            match connecting.await {
                Ok(conn) => {
                    let _ = tx.send(TransportEvent::Connected(peer));
                    run_connection(peer, conn, cmd_rx, tx, registry).await;
                }
                Err(e) => {
                    registry
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&peer);
                    let _ = tx.send(TransportEvent::Disconnected(
                        peer,
                        DisconnectReason::Error(e.to_string()),
                    ));
                }
            }
        });
        Ok(peer)
    }
}

impl Drop for QuicTransport {
    fn drop(&mut self) {
        // Tell every peer, give the close a moment to leave, then stop the runtime.
        for tx in self
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
        {
            let _ = tx.send(ConnCmd::Close);
        }
        if let Some(e) = &self.endpoint {
            let rt = self.rt();
            rt.block_on(async {
                let _ = tokio::time::timeout(Duration::from_millis(500), e.wait_idle()).await;
            });
        }
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}

impl Transport for QuicTransport {
    fn send(&mut self, peer: PeerId, channel: Channel, data: &[u8]) -> Result<(), NetError> {
        let limit = self.max_unreliable();
        match channel {
            Channel::Unreliable | Channel::UnreliableSequenced
                if data.len() + DATAGRAM_HEADER > limit =>
            {
                return Err(NetError::TooLarge {
                    len: data.len(),
                    max: limit - DATAGRAM_HEADER,
                });
            }
            _ if data.len() > MAX_MESSAGE => {
                return Err(NetError::TooLarge {
                    len: data.len(),
                    max: MAX_MESSAGE,
                });
            }
            _ => {}
        }
        let registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        let tx = registry.get(&peer).ok_or(NetError::UnknownPeer(peer))?;
        tx.send(ConnCmd::Send {
            channel,
            bytes: data.to_vec(),
        })
        .map_err(|_| NetError::UnknownPeer(peer))
    }

    fn poll(&mut self, _now: Time) -> Vec<TransportEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            match &event {
                TransportEvent::Connected(p) => {
                    self.connected.insert(*p);
                }
                TransportEvent::Disconnected(p, _) => {
                    self.connected.remove(p);
                }
                TransportEvent::Data { .. } => {}
            }
            out.push(event);
        }
        out
    }

    fn disconnect(&mut self, peer: PeerId) {
        if let Some(tx) = self
            .registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&peer)
        {
            let _ = tx.send(ConnCmd::Close);
        }
    }

    fn peers(&self) -> Vec<PeerId> {
        let mut v: Vec<_> = self.connected.iter().copied().collect();
        v.sort_unstable();
        v
    }

    fn max_unreliable(&self) -> usize {
        MTU
    }
}

fn reason(e: &quinn::ConnectionError) -> DisconnectReason {
    match e {
        quinn::ConnectionError::ApplicationClosed(_)
        | quinn::ConnectionError::ConnectionClosed(_) => DisconnectReason::Remote,
        quinn::ConnectionError::LocallyClosed => DisconnectReason::Local,
        quinn::ConnectionError::TimedOut => DisconnectReason::TimedOut,
        other => DisconnectReason::Error(other.to_string()),
    }
}

/// One connection's life: sends what the game queues, delivers what arrives, reports the end.
async fn run_connection(
    peer: PeerId,
    conn: quinn::Connection,
    mut cmds: UnboundedReceiver<ConnCmd>,
    events: Sender<TransportEvent>,
    registry: Registry,
) {
    let mut ordered: Option<quinn::SendStream> = None;
    let mut seq_out: u32 = 0;
    let mut seq_in: Option<u32> = None;
    let end = loop {
        tokio::select! {
            cmd = cmds.recv() => match cmd {
                None | Some(ConnCmd::Close) => {
                    // Reliable data written before the close still goes out.
                    if let Some(mut s) = ordered.take() {
                        let _ = s.finish();
                        let _ = tokio::time::timeout(Duration::from_millis(300), s.stopped()).await;
                    }
                    conn.close(0u32.into(), b"bye");
                    break DisconnectReason::Local;
                }
                Some(ConnCmd::Send { channel, bytes }) => {
                    send_one(&conn, &mut ordered, &mut seq_out, channel, bytes).await;
                }
            },
            datagram = conn.read_datagram() => match datagram {
                Ok(d) => on_datagram(peer, &d, &mut seq_in, &events),
                Err(e) => break reason(&e),
            },
            stream = conn.accept_uni() => match stream {
                Ok(rs) => {
                    tokio::spawn(read_stream(peer, rs, events.clone()));
                }
                Err(e) => break reason(&e),
            },
        }
    };
    registry
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&peer);
    let _ = events.send(TransportEvent::Disconnected(peer, end));
}

async fn send_one(
    conn: &quinn::Connection,
    ordered: &mut Option<quinn::SendStream>,
    seq_out: &mut u32,
    channel: Channel,
    bytes: Vec<u8>,
) {
    let result: Result<(), String> = async {
        match channel {
            Channel::Unreliable => {
                let mut b = Vec::with_capacity(bytes.len() + 1);
                b.push(0);
                b.extend_from_slice(&bytes);
                conn.send_datagram(Bytes::from(b))
                    .map_err(|e| e.to_string())
            }
            Channel::UnreliableSequenced => {
                *seq_out = seq_out.wrapping_add(1);
                let mut b = Vec::with_capacity(bytes.len() + DATAGRAM_HEADER);
                b.push(1);
                b.extend_from_slice(&seq_out.to_le_bytes());
                b.extend_from_slice(&bytes);
                conn.send_datagram(Bytes::from(b))
                    .map_err(|e| e.to_string())
            }
            Channel::Reliable => {
                if ordered.is_none() {
                    let mut s = conn.open_uni().await.map_err(|e| e.to_string())?;
                    s.write_all(&[0]).await.map_err(|e| e.to_string())?;
                    *ordered = Some(s);
                }
                let s = ordered.as_mut().expect("opened above");
                s.write_all(&(bytes.len() as u32).to_le_bytes())
                    .await
                    .map_err(|e| e.to_string())?;
                s.write_all(&bytes).await.map_err(|e| e.to_string())
            }
            Channel::ReliableUnordered => {
                let conn = conn.clone();
                tokio::spawn(async move {
                    let Ok(mut s) = conn.open_uni().await else {
                        return;
                    };
                    if s.write_all(&[1]).await.is_ok() && s.write_all(&bytes).await.is_ok() {
                        let _ = s.finish();
                        let _ = tokio::time::timeout(Duration::from_secs(10), s.stopped()).await;
                    }
                });
                Ok(())
            }
        }
    }
    .await;
    if let Err(e) = result {
        tracing::debug!("quic: send on {channel:?} failed: {e}");
    }
}

fn on_datagram(peer: PeerId, d: &[u8], seq_in: &mut Option<u32>, events: &Sender<TransportEvent>) {
    match d.first() {
        Some(0) => {
            let _ = events.send(TransportEvent::Data {
                peer,
                channel: Channel::Unreliable,
                bytes: d[1..].to_vec(),
            });
        }
        Some(1) if d.len() >= DATAGRAM_HEADER => {
            let seq = u32::from_le_bytes([d[1], d[2], d[3], d[4]]);
            // Newer than the newest seen, allowing the counter to wrap.
            let newer = seq_in.is_none_or(|last| seq.wrapping_sub(last) as i32 > 0);
            if newer {
                *seq_in = Some(seq);
                let _ = events.send(TransportEvent::Data {
                    peer,
                    channel: Channel::UnreliableSequenced,
                    bytes: d[DATAGRAM_HEADER..].to_vec(),
                });
            }
        }
        _ => tracing::debug!("quic: ignoring a malformed datagram from peer {peer}"),
    }
}

async fn read_stream(peer: PeerId, mut rs: quinn::RecvStream, events: Sender<TransportEvent>) {
    let mut kind = [0u8; 1];
    if rs.read_exact(&mut kind).await.is_err() {
        return;
    }
    match kind[0] {
        0 => loop {
            let mut len = [0u8; 4];
            if rs.read_exact(&mut len).await.is_err() {
                return;
            }
            let len = u32::from_le_bytes(len) as usize;
            if len > MAX_MESSAGE {
                tracing::warn!("quic: peer {peer} sent a {len}-byte message; closing its stream");
                return;
            }
            let mut bytes = vec![0u8; len];
            if rs.read_exact(&mut bytes).await.is_err() {
                return;
            }
            let _ = events.send(TransportEvent::Data {
                peer,
                channel: Channel::Reliable,
                bytes,
            });
        },
        1 => {
            if let Ok(bytes) = rs.read_to_end(MAX_MESSAGE).await {
                let _ = events.send(TransportEvent::Data {
                    peer,
                    channel: Channel::ReliableUnordered,
                    bytes,
                });
            }
        }
        _ => tracing::debug!("quic: unknown stream kind from peer {peer}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use platypus_net::{ClientPhase, SERVER, Session, SessionConfig, SessionEvent};
    use std::time::Instant;

    fn local() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }

    /// Polls `t` until `done` says so or five seconds pass.
    fn wait(
        t: &mut QuicTransport,
        mut done: impl FnMut(&[TransportEvent]) -> bool,
    ) -> Vec<TransportEvent> {
        let start = Instant::now();
        let mut all = Vec::new();
        while start.elapsed() < Duration::from_secs(5) {
            all.extend(t.poll(start.elapsed()));
            if done(&all) {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        all
    }

    fn pair(trust: impl FnOnce(&Identity) -> Trust) -> (QuicTransport, QuicTransport, PeerId) {
        let id = Identity::self_signed(&["localhost"]).unwrap();
        let server = QuicTransport::listen(local(), &id).unwrap();
        let mut client = QuicTransport::client().unwrap();
        let addr = server.local_addr().unwrap();
        let peer = client.connect(addr, "localhost", &trust(&id)).unwrap();
        (server, client, peer)
    }

    // M8-NET-08
    #[test]
    fn fingerprints_round_trip_through_text() {
        let id = Identity::self_signed(&["localhost"]).unwrap();
        let fp = id.fingerprint();
        assert_eq!(parse_fingerprint(&id.fingerprint_hex()), Some(fp));
        assert_eq!(parse_fingerprint("zz"), None);
        assert_eq!(parse_fingerprint(&"g".repeat(64)), None);
    }

    // M8-NET-08
    #[test]
    fn every_channel_carries_data_both_ways_over_real_quic() {
        let (mut server, mut client, c_peer) = pair(|id| Trust::Fingerprint(id.fingerprint()));
        let events = wait(&mut client, |e| {
            e.contains(&TransportEvent::Connected(c_peer))
        });
        assert!(events.contains(&TransportEvent::Connected(c_peer)));
        let s_events = wait(&mut server, |e| {
            e.iter().any(|e| matches!(e, TransportEvent::Connected(_)))
        });
        let s_peer = match s_events.iter().find_map(|e| match e {
            TransportEvent::Connected(p) => Some(*p),
            _ => None,
        }) {
            Some(p) => p,
            None => panic!("the server never saw a connection"),
        };

        for (i, ch) in Channel::ALL.into_iter().enumerate() {
            client
                .send(c_peer, ch, format!("up {i}").as_bytes())
                .unwrap();
            server
                .send(s_peer, ch, format!("down {i}").as_bytes())
                .unwrap();
        }
        let got_server = wait(&mut server, |e| {
            e.iter()
                .filter(|e| matches!(e, TransportEvent::Data { .. }))
                .count()
                >= 4
        });
        let got_client = wait(&mut client, |e| {
            e.iter()
                .filter(|e| matches!(e, TransportEvent::Data { .. }))
                .count()
                >= 4
        });
        for (got, prefix) in [(got_server, "up"), (got_client, "down")] {
            for (i, ch) in Channel::ALL.into_iter().enumerate() {
                let want = format!("{prefix} {i}").into_bytes();
                assert!(
                    got.iter().any(|e| matches!(e, TransportEvent::Data { channel, bytes, .. } if *channel == ch && *bytes == want)),
                    "{prefix} on {ch:?} missing from {got:?}"
                );
            }
        }
    }

    // M8-NET-08
    #[test]
    fn reliable_messages_keep_their_order_and_large_ones_get_through() {
        let (mut server, mut client, c_peer) = pair(|_| Trust::InsecureDev);
        wait(&mut client, |e| {
            e.contains(&TransportEvent::Connected(c_peer))
        });
        let events = wait(&mut server, |e| {
            e.iter().any(|e| matches!(e, TransportEvent::Connected(_)))
        });
        let s_peer = events
            .iter()
            .find_map(|e| match e {
                TransportEvent::Connected(p) => Some(*p),
                _ => None,
            })
            .expect("connected");
        for i in 0..200u32 {
            client
                .send(c_peer, Channel::Reliable, &i.to_le_bytes())
                .unwrap();
        }
        let big = vec![7u8; 3 << 20];
        client
            .send(c_peer, Channel::ReliableUnordered, &big)
            .unwrap();
        let got = wait(&mut server, |e| {
            e.iter()
                .filter(|e| matches!(e, TransportEvent::Data { .. }))
                .count()
                >= 201
        });
        let ordered: Vec<u32> = got
            .iter()
            .filter_map(|e| match e {
                TransportEvent::Data {
                    channel: Channel::Reliable,
                    bytes,
                    ..
                } => Some(u32::from_le_bytes(bytes[..4].try_into().unwrap())),
                _ => None,
            })
            .collect();
        assert_eq!(ordered, (0..200).collect::<Vec<_>>());
        assert!(got.iter().any(|e| matches!(e, TransportEvent::Data { channel: Channel::ReliableUnordered, bytes, .. } if bytes.len() == big.len())));
        let _ = s_peer;
    }

    // M8-NET-08
    #[test]
    fn an_oversize_datagram_is_refused_and_unknown_peers_error() {
        let (_server, mut client, c_peer) = pair(|_| Trust::InsecureDev);
        wait(&mut client, |e| {
            e.contains(&TransportEvent::Connected(c_peer))
        });
        let too_big = vec![0u8; MTU];
        assert!(matches!(
            client.send(c_peer, Channel::Unreliable, &too_big),
            Err(NetError::TooLarge { .. })
        ));
        assert_eq!(
            client.send(999, Channel::Reliable, b"x"),
            Err(NetError::UnknownPeer(999))
        );
    }

    // M8-NET-09
    #[test]
    fn a_client_pinning_the_wrong_fingerprint_does_not_connect() {
        let (_server, mut client, c_peer) = pair(|_| Trust::Fingerprint([9; 32]));
        let events = wait(&mut client, |e| {
            e.iter()
                .any(|e| matches!(e, TransportEvent::Disconnected(..)))
        });
        assert!(
            events.iter().any(|e| matches!(e, TransportEvent::Disconnected(p, DisconnectReason::Error(_)) if *p == c_peer)),
            "{events:?}"
        );
        assert!(!events.contains(&TransportEvent::Connected(c_peer)));
    }

    // M8-NET-09
    #[test]
    fn a_disconnect_reaches_the_other_side_with_the_reliable_goodbye_first() {
        let (mut server, mut client, c_peer) = pair(|_| Trust::InsecureDev);
        wait(&mut client, |e| {
            e.contains(&TransportEvent::Connected(c_peer))
        });
        wait(&mut server, |e| {
            e.iter().any(|e| matches!(e, TransportEvent::Connected(_)))
        });
        client
            .send(c_peer, Channel::Reliable, b"last words")
            .unwrap();
        client.disconnect(c_peer);
        let got = wait(&mut server, |e| {
            e.iter()
                .any(|e| matches!(e, TransportEvent::Disconnected(..)))
        });
        let data = got.iter().position(
            |e| matches!(e, TransportEvent::Data { bytes, .. } if bytes == b"last words"),
        );
        let end = got
            .iter()
            .position(|e| matches!(e, TransportEvent::Disconnected(..)));
        assert!(data.is_some(), "{got:?}");
        assert!(data < end);
    }

    // M8-NET-10
    #[test]
    fn a_session_handshakes_and_exchanges_messages_over_quic() {
        let id = Identity::self_signed(&["localhost"]).unwrap();
        let cfg = SessionConfig {
            schema_hash: 5,
            game_version: "t".into(),
            ..SessionConfig::default()
        };
        let server_t = QuicTransport::listen(local(), &id).unwrap();
        let addr = server_t.local_addr().unwrap();
        let mut server = Session::server(server_t, cfg.clone());
        let mut client_t = QuicTransport::client().unwrap();
        client_t
            .connect(addr, "localhost", &Trust::Fingerprint(id.fingerprint()))
            .unwrap();
        let mut client = Session::client(client_t, cfg, "quinn", None);

        let start = Instant::now();
        let (mut joined, mut connected, mut got_up, mut got_down) = (false, false, false, false);
        while start.elapsed() < Duration::from_secs(5) && !(got_up && got_down) {
            let now = start.elapsed();
            for e in server.update(now) {
                match e {
                    SessionEvent::PlayerJoined { player, .. } => {
                        joined = true;
                        server
                            .send(player, Channel::Reliable, b"hello player")
                            .unwrap();
                    }
                    SessionEvent::Message { bytes, .. } if bytes == b"hello server" => {
                        got_up = true
                    }
                    _ => {}
                }
            }
            for e in client.update(now) {
                match e {
                    SessionEvent::Connected { .. } => {
                        connected = true;
                        client
                            .send(SERVER, Channel::Reliable, b"hello server")
                            .unwrap();
                    }
                    SessionEvent::Message { bytes, .. } if bytes == b"hello player" => {
                        got_down = true
                    }
                    _ => {}
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(joined && connected && got_up && got_down);
        assert_eq!(client.phase(), Some(ClientPhase::Connected));
        assert!(client.rtt(SERVER).is_some() || start.elapsed() < Duration::from_secs(1));
    }
}
