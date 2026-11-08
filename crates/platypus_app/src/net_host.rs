// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The app's half of `Network` (M8): serves what scripts ask for (`Network.startServer`,
//! `Network.connect`, `Network.disconnect`) by making QUIC transports, and moves the network on
//! every frame. Scripts and replication live in `platypus_engine_api::network` and `platypus_net`;
//! this file only owns the sockets.
//!
//! Addresses are `host:port`; a client of a remote server adds the server's pinned certificate
//! fingerprint as `host:port#<64 hex digits>` (the server reports it in its `listening` event).
//! Loopback addresses (`127.0.0.1`, `localhost`, `::1`) skip the pin: development only.

use std::net::{SocketAddr, ToSocketAddrs};
use std::time::Duration;

use platypus_engine_api::network::{self, Host, Request};
use platypus_net::Transport;
use platypus_net::host::{HostConfig, NetHost};
use platypus_net_quic::{Identity, QuicTransport, Trust, parse_fingerprint};

/// Serves script requests, then moves the network on to `now` (time since the app started).
pub(crate) fn pump(now: Duration) {
    for request in network::take_requests() {
        if let Err(e) = serve(request) {
            tracing::warn!("network: {e}");
            network::report_error(&e);
        }
    }
    network::update(now);
}

fn serve(request: Request) -> Result<(), String> {
    match request {
        Request::StartServer {
            port,
            max_players,
            tick_rate,
        } => {
            let schema = network::schema()?;
            let identity = Identity::self_signed(&["localhost"]).map_err(|e| e.to_string())?;
            let fingerprint = identity.fingerprint_hex();
            let addr: SocketAddr = ([0, 0, 0, 0], port).into();
            let quic = QuicTransport::listen(addr, &identity).map_err(|e| e.to_string())?;
            let port = quic.local_addr().map_or(port, |a| a.port());
            let mut config = HostConfig {
                tick_rate: tick_rate.clamp(1, 240),
                ..HostConfig::default()
            };
            config.session.max_players = max_players.clamp(1, 256);
            let (transport, debug) = wrap(quic);
            let host: Host = NetHost::server(transport, schema, config);
            network::install(host, debug);
            tracing::info!("network: serving on UDP port {port}, certificate {fingerprint}");
            network::emit(&["listening", &port.to_string(), &fingerprint]);
            Ok(())
        }
        Request::Connect { address, name } => {
            let schema = network::schema()?;
            let (addr, trust) = parse_address(&address)?;
            let mut quic = QuicTransport::client().map_err(|e| e.to_string())?;
            quic.connect(addr, "localhost", &trust)
                .map_err(|e| e.to_string())?;
            let (transport, debug) = wrap(quic);
            let host: Host = NetHost::client(transport, schema, HostConfig::default(), &name, None);
            network::install(host, debug);
            tracing::info!("network: connecting to {addr}");
            Ok(())
        }
        Request::Disconnect => {
            network::uninstall();
            Ok(())
        }
    }
}

/// The transport as the host takes it, wrapped in the fault injector when the build has one
/// (`net-debug`, on by default) so `Network.debugConditions` has something to drive. With no
/// conditions set the wrapper only passes messages through.
fn wrap(quic: QuicTransport) -> (Box<dyn Transport>, Option<network::DebugHandle>) {
    #[cfg(feature = "net-debug")]
    {
        let (wrapped, handle) = platypus_net::conditioned::Conditioned::new(quic, 0x504c_4159);
        (Box::new(wrapped), Some(handle))
    }
    #[cfg(not(feature = "net-debug"))]
    {
        (Box::new(quic), None)
    }
}

/// `host:port` or `host:port#fingerprint`, resolved; loopback needs no fingerprint.
fn parse_address(text: &str) -> Result<(SocketAddr, Trust), String> {
    let (hostport, pin) = match text.split_once('#') {
        Some((h, p)) => (h, Some(p)),
        None => (text, None),
    };
    let addr = hostport
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve {hostport}: {e}"))?
        .next()
        .ok_or_else(|| format!("{hostport} has no address"))?;
    let trust = match pin {
        Some(p) => Trust::Fingerprint(
            parse_fingerprint(p)
                .ok_or_else(|| format!("the pin after # must be 64 hex digits, got {p:?}"))?,
        ),
        None if addr.ip().is_loopback() => Trust::InsecureDev,
        None => {
            return Err(format!(
                "{hostport} is not a local address: add the server's certificate as host:port#<64 hex digits>"
            ));
        }
    };
    Ok((addr, trust))
}

#[cfg(test)]
mod tests {
    use super::*;

    // M8-APP-01
    #[test]
    fn loopback_addresses_need_no_pin_and_remote_ones_do() {
        let (a, t) = parse_address("127.0.0.1:7777").unwrap();
        assert_eq!(a.port(), 7777);
        assert_eq!(t, Trust::InsecureDev);
        assert!(parse_address("localhost:1").is_ok());
        assert!(parse_address("203.0.113.9:7777").unwrap_err().contains("not a local address"));
        let fp = "ab".repeat(32);
        let (_, t) = parse_address(&format!("203.0.113.9:7777#{fp}")).unwrap();
        assert_eq!(t, Trust::Fingerprint([0xab; 32]));
        assert!(parse_address("203.0.113.9:7777#xyz").unwrap_err().contains("64 hex"));
        assert!(parse_address("not an address").is_err());
    }
}
