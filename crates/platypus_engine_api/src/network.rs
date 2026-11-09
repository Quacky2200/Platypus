// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Network`: multiplayer for scripts (M8; design `docs/research/multiplayer.md`).
//!
//! The script side is plain calls and polling, as with `Assets.takeChanges()`:
//!
//! - **Define** what replicates before connecting: `Network.defineClass("Player", "pos:vec3:q=0.01:lerp;hp:i32")`
//!   and `Network.defineRpc("fire", "server")`. (The `@sync` / `@rpc` decorators will emit these calls.)
//! - **Start**: `Network.startServer(port, maxPlayers, tickRate)` or `Network.connect(address, name)`. The
//!   calls queue a request the app serves (it owns the transport) with [`take_requests`] and [`install`].
//! - **Each frame** the app calls [`update`]; scripts read what happened with `Network.takeEvents()`
//!   (lines of fields joined by the unit separator, `\u{1f}`) and read or write fields by object id.
//!
//! A game is one program: `Network.isServer` and `Network.isClient` say which half runs. The state
//! lives in this thread, like the other engine namespaces.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use platypus_net::host::{NetEvent, NetHost};
use platypus_net::replica::NetId;
use platypus_net::schema::{ClassDef, FieldDef, FieldType, RpcDef, RpcTarget, Schema, Value};
use platypus_net::{RejectReason, Transport};
use ts_api::ts_api;

/// The field separator inside an event line.
pub const SEP: char = '\u{1f}';

/// The host the app installs: any transport behind a box.
pub type Host = NetHost<Box<dyn Transport>>;

/// What a script asked the app to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    StartServer {
        port: u16,
        max_players: u32,
        tick_rate: u32,
    },
    Connect {
        address: String,
        name: String,
    },
    Disconnect,
}

/// Debug controls of a fault-injecting transport, when the app wrapped it in one.
#[cfg(feature = "net-debug")]
pub type DebugHandle = platypus_net::conditioned::Handle;
#[cfg(not(feature = "net-debug"))]
pub type DebugHandle = ();

#[derive(Default)]
struct State {
    classes: Vec<ClassDef>,
    rpcs: Vec<RpcDef>,
    host: Option<Host>,
    debug: Option<DebugHandle>,
    requests: Vec<Request>,
    lines: Vec<String>,
    last_error: String,
    /// The conditions the debug panel set, kept so later changes edit one knob.
    #[cfg(feature = "net-debug")]
    links: platypus_net::conditioned::Links,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

fn fail(s: &mut State, msg: String) -> bool {
    s.last_error = msg;
    false
}

// ---- for the app ---------------------------------------------------------------------------

/// The schema the scripts defined (checked), for the app to build the transport and host with.
pub fn schema() -> Result<Arc<Schema>, String> {
    with(|s| {
        let schema = Schema::new(s.classes.clone()).with_rpcs(s.rpcs.clone());
        schema.check()?;
        Ok(Arc::new(schema))
    })
}

/// What scripts asked for since the last call.
pub fn take_requests() -> Vec<Request> {
    with(|s| std::mem::take(&mut s.requests))
}

/// Puts a started host in place (replacing any), with the debug handle of its fault wrapper.
pub fn install(host: Host, debug: Option<DebugHandle>) {
    with(|s| {
        s.host = Some(host);
        s.debug = debug;
    });
}

/// Drops the host (the transport closes).
pub fn uninstall() {
    with(|s| {
        s.host = None;
        s.debug = None;
    });
}

/// Queues an event line for scripts (the app's own events, such as `listening`).
pub fn emit(fields: &[&str]) {
    with(|s| s.lines.push(line(fields)));
}

/// Tells scripts a request failed (a port in use, an unreachable address).
pub fn report_error(message: &str) {
    with(|s| {
        s.last_error = message.to_owned();
        s.lines.push(line(&["error", message]));
    });
}

pub fn has_host() -> bool {
    with(|s| s.host.is_some())
}

/// Moves the network on to `now` and queues its events for scripts.
pub fn update(now: Duration) {
    with(|s| {
        let Some(host) = s.host.as_mut() else { return };
        let events = host.update(now);
        for e in events {
            s.lines.push(event_line(&e));
        }
    });
}

fn line(fields: &[&str]) -> String {
    let clean: Vec<String> = fields.iter().map(|f| f.replace([SEP, '\n'], " ")).collect();
    clean.join(&SEP.to_string())
}

fn event_line(e: &NetEvent) -> String {
    let n = |x: u64| x.to_string();
    match e {
        NetEvent::PlayerJoined {
            player,
            name,
            resumed,
        } => line(&[
            "joined",
            &n(u64::from(*player)),
            name,
            &n(u64::from(*resumed)),
        ]),
        NetEvent::PlayerDropped { player } => line(&["dropped", &n(u64::from(*player))]),
        NetEvent::PlayerLeft { player } => line(&["left", &n(u64::from(*player))]),
        NetEvent::Connected {
            player, resumed, ..
        } => line(&["connected", &n(u64::from(*player)), &n(u64::from(*resumed))]),
        NetEvent::Rejected(r) => line(&["rejected", &reject_text(r)]),
        NetEvent::Disconnected(r) => line(&["disconnected", &format!("{r:?}")]),
        NetEvent::Spawned {
            id,
            class,
            prefab,
            owner,
        } => line(&[
            "spawned",
            &n(*id),
            &n(u64::from(*class)),
            prefab,
            &n(u64::from(*owner)),
        ]),
        NetEvent::Despawned { id } => line(&["despawned", &n(*id)]),
        NetEvent::OwnerChanged { id, owner } => line(&["owner", &n(*id), &n(u64::from(*owner))]),
        NetEvent::Rpc {
            id,
            name,
            args,
            from,
        } => {
            let mut fields = vec!["rpc".to_owned(), n(*id), name.clone(), n(u64::from(*from))];
            fields.extend(args.iter().map(encode_arg));
            let refs: Vec<&str> = fields.iter().map(String::as_str).collect();
            line(&refs)
        }
    }
}

fn reject_text(r: &RejectReason) -> String {
    match r {
        RejectReason::ProtocolMismatch => "protocol".into(),
        RejectReason::SchemaMismatch => "schema".into(),
        RejectReason::VersionMismatch { server } => format!("version {server}"),
        RejectReason::ServerFull => "full".into(),
        RejectReason::Malformed => "malformed".into(),
    }
}

/// `f:1.5`, `i:3`, `b:1`, `s:text`, `v:x,y,z`, `q:x,y,z,w`, `r:id`: an RPC argument as text.
pub fn encode_arg(v: &Value) -> String {
    let nums = |xs: &[f32]| {
        xs.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    match v {
        Value::Bool(b) => format!("b:{}", u8::from(*b)),
        Value::I32(n) => format!("i:{n}"),
        Value::F32(x) => format!("f:{x}"),
        Value::Vec3(a) => format!("v:{}", nums(a)),
        Value::Quat(a) => format!("q:{}", nums(a)),
        Value::Str(s) => format!("s:{s}"),
        Value::Ref(r) => format!("r:{r}"),
    }
}

/// The inverse of [`encode_arg`].
pub fn decode_arg(text: &str) -> Option<Value> {
    let (kind, rest) = text.split_once(':')?;
    let nums = |n: usize| -> Option<Vec<f32>> {
        let v: Option<Vec<f32>> = rest.split(',').map(|p| p.trim().parse().ok()).collect();
        v.filter(|v| v.len() == n)
    };
    Some(match kind {
        "b" => Value::Bool(rest == "1" || rest == "true"),
        "i" => Value::I32(rest.parse().ok()?),
        "f" => Value::F32(rest.parse().ok()?),
        "v" => Value::Vec3(nums(3)?.try_into().ok()?),
        "q" => Value::Quat(nums(4)?.try_into().ok()?),
        "s" => Value::Str(rest.to_owned()),
        "r" => Value::Ref(rest.parse().ok()?),
        _ => return None,
    })
}

// ---- schema text ---------------------------------------------------------------------------

/// `name:type[:q=0.01][:owner][:lerp]` fields joined by `;`.
fn parse_fields(spec: &str) -> Result<Vec<FieldDef>, String> {
    let mut out = Vec::new();
    for part in spec.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let mut bits = part.split(':').map(str::trim);
        let name = bits
            .next()
            .filter(|n| !n.is_empty())
            .ok_or("a field has no name")?;
        let ty = bits.next().and_then(FieldType::parse).ok_or_else(|| {
            format!("field {name}: type must be bool, i32, f32, vec3, quat, str or ref")
        })?;
        let mut f = FieldDef::new(name, ty);
        for flag in bits {
            match flag {
                "owner" => f.owner_writes = true,
                "lerp" => f.interpolate = true,
                q if q.starts_with("q=") => {
                    let step: f32 = q[2..]
                        .parse()
                        .map_err(|_| format!("field {name}: bad quantisation {q:?}"))?;
                    if step <= 0.0 {
                        return Err(format!("field {name}: quantisation must be above zero"));
                    }
                    f.quantize = Some(step);
                }
                other => return Err(format!("field {name}: unknown flag {other:?}")),
            }
        }
        out.push(f);
    }
    Ok(out)
}

fn parse_rpc(name: &str, target: &str, flags: &str) -> Result<RpcDef, String> {
    let target = RpcTarget::parse(target)
        .ok_or_else(|| format!("rpc {name}: target must be server, owner or all"))?;
    let mut def = RpcDef::new(name, target);
    for flag in flags.split(',').map(str::trim).filter(|f| !f.is_empty()) {
        match flag {
            "anyone" => def.require_owner = false,
            "unreliable" => def.reliable = false,
            other => return Err(format!("rpc {name}: unknown flag {other:?}")),
        }
    }
    Ok(def)
}

// ---- field access --------------------------------------------------------------------------

fn field_type(h: &Host, id: NetId, field: &str) -> Option<FieldType> {
    let class = match (h.server_ref(), h.client_ref()) {
        (Some(s), _) => s.object(id)?.class,
        (_, Some(c)) => c.class(id)?,
        _ => return None,
    };
    let def = h.schema().class(class)?;
    def.fields.iter().find(|f| f.name == field).map(|f| f.ty)
}

fn write(h: &mut Host, id: NetId, field: &str, v: Value) -> bool {
    if let Some(s) = h.server_mut() {
        return s.set(id, field, v);
    }
    h.client_mut().is_some_and(|c| c.set_owned(id, field, v))
}

fn set_with(id: f64, field: &str, make: impl FnOnce(FieldType) -> Option<Value>) -> bool {
    with(|s| {
        let Some(h) = s.host.as_mut() else {
            return fail(s, "not connected".into());
        };
        let id = id as NetId;
        let Some(ty) = field_type(h, id, field) else {
            return fail(s, format!("no field {field} on object {id}"));
        };
        let Some(v) = make(ty) else {
            return fail(s, format!("field {field} is {}", ty.name()));
        };
        let h = s.host.as_mut().expect("checked above");
        if write(h, id, field, v) {
            true
        } else {
            fail(
                s,
                format!(
                    "cannot write {field} on object {id} (not the server, or not the owner of an owner field)"
                ),
            )
        }
    })
}

fn read(id: f64, field: &str) -> Option<Value> {
    with(|s| s.host.as_ref()?.read(id as NetId, field))
}

#[ts_api(namespace = "Network")]
pub mod network_api {
    use super::*;

    /// Declares a networked class. `fields` is `name:type` entries joined by `;`, each with optional
    /// flags: `q=0.01` (quantise floats), `owner` (the owning client writes it), `lerp` (clients blend it).
    /// Types are bool, i32, f32, vec3, quat, str and ref. Define every class and RPC, in the same
    /// order on every peer, before `startServer` or `connect`. False (see `lastError`) if the text is bad.
    pub fn define_class(name: &str, fields: &str) -> bool {
        with(|s| match parse_fields(fields) {
            Ok(fields) => {
                s.classes.retain(|c| c.name != name);
                s.classes.push(ClassDef {
                    name: name.to_owned(),
                    fields,
                });
                true
            }
            Err(e) => fail(s, format!("class {name}: {e}")),
        })
    }

    /// Declares an RPC. `target` is `server` (a client calls it), `owner` or `all` (the server calls it);
    /// `flags` may hold `anyone` (a server RPC any player may call, not only the object's owner) and `unreliable`.
    #[optional(flags)]
    pub fn define_rpc(name: &str, target: &str, flags: &str) -> bool {
        with(|s| match parse_rpc(name, target, flags) {
            Ok(def) => {
                s.rpcs.retain(|r| r.name != name);
                s.rpcs.push(def);
                true
            }
            Err(e) => fail(s, e),
        })
    }

    /// Why the last call that returned false failed.
    pub fn last_error() -> String {
        with(|s| s.last_error.clone())
    }

    /// Starts a server on a UDP port (QUIC). The app serves the request on the next frame; a
    /// `connected`-less server event stream starts with `listening`, or `error`.
    #[optional(max_players, tick_rate)]
    pub fn start_server(port: f64, max_players: f64, tick_rate: f64) {
        with(|s| {
            s.requests.push(Request::StartServer {
                port: port as u16,
                max_players: if max_players.is_nan() {
                    16
                } else {
                    max_players as u32
                },
                tick_rate: if tick_rate.is_nan() {
                    30
                } else {
                    tick_rate as u32
                },
            })
        });
    }

    /// Connects to a server (`host:port`). Events `connected`, `rejected` or `error` follow.
    pub fn connect(address: &str, name: &str) {
        with(|s| {
            s.requests.push(Request::Connect {
                address: address.to_owned(),
                name: name.to_owned(),
            })
        });
    }

    /// Leaves the session (a client says goodbye; a server closes).
    pub fn disconnect() {
        with(|s| s.requests.push(Request::Disconnect));
    }

    pub fn is_server() -> bool {
        with(|s| s.host.as_ref().is_some_and(|h| h.is_server()))
    }

    pub fn is_client() -> bool {
        with(|s| s.host.as_ref().is_some_and(|h| !h.is_server()))
    }

    /// True on a server, and on a client once the handshake finished.
    pub fn is_connected() -> bool {
        with(|s| {
            s.host
                .as_ref()
                .is_some_and(|h| h.session().local_player().is_some())
        })
    }

    /// This peer's player id (0 on the server; -1 when not connected).
    pub fn local_player() -> f64 {
        with(|s| {
            s.host
                .as_ref()
                .and_then(|h| h.session().local_player())
                .map_or(-1.0, f64::from)
        })
    }

    /// Smoothed round-trip time to the server (a client), or to a player (the server), in ms; -1 unknown.
    #[optional(player)]
    pub fn rtt(player: f64) -> f64 {
        with(|s| {
            let Some(h) = s.host.as_ref() else {
                return -1.0;
            };
            let who = if player.is_nan() { 0 } else { player as u32 };
            h.session()
                .rtt(who)
                .map_or(-1.0, |d| d.as_secs_f64() * 1000.0)
        })
    }

    /// Replication steps a second (30 unless `startServer` said otherwise); 0 when not running.
    pub fn tick_rate() -> f64 {
        with(|s| s.host.as_ref().map_or(0.0, |h| h.tick_rate()))
    }

    /// The server's tick: its own on the server, the newest a client has received; -1 before the first.
    pub fn tick() -> f64 {
        with(|s| {
            s.host
                .as_ref()
                .and_then(|h| h.tick())
                .map_or(-1.0, f64::from)
        })
    }

    /// Network time in seconds, the server's tick clock: a client estimates the server's present,
    /// so peers agree to within the one-way delay and a timer on it (a round clock) ticks together.
    /// -1 before the first tick.
    pub fn time() -> f64 {
        with(|s| s.host.as_ref().and_then(|h| h.time()).unwrap_or(-1.0))
    }

    /// The connected players (server), in id order.
    pub fn players() -> Vec<f64> {
        with(|s| {
            s.host
                .as_ref()
                .map(|h| h.session().players().into_iter().map(f64::from).collect())
                .unwrap_or_default()
        })
    }

    pub fn player_name(player: f64) -> String {
        with(|s| {
            s.host
                .as_ref()
                .and_then(|h| h.session().player_name(player as u32).map(str::to_owned))
                .unwrap_or_default()
        })
    }

    /// Events since the last call, one per line, fields joined by `\u{1f}`:
    /// `spawned id class prefab owner`, `despawned id`, `owner id owner`, `rpc id name from arg...`
    /// (each arg `f:1.5`, `i:3`, `b:1`, `s:text`, `v:x,y,z`, `q:x,y,z,w`, `r:id`), `joined player name resumed`,
    /// `dropped player`, `left player`, `connected player resumed`, `rejected reason`, `disconnected reason`, `error message`.
    pub fn take_events() -> String {
        with(|s| std::mem::take(&mut s.lines)).join("\n")
    }

    /// The class names in definition order (the `class` number of `spawned` indexes this).
    pub fn class_name(class: f64) -> String {
        with(|s| {
            s.classes
                .get(class as usize)
                .map(|c| c.name.clone())
                .unwrap_or_default()
        })
    }

    // ---- the server's objects ----

    /// Makes a networked object (server only). `prefab` is what clients instantiate. Returns its id, 0 on failure.
    #[optional(owner)]
    pub fn spawn(class: &str, prefab: &str, owner: f64) -> f64 {
        with(|s| {
            let Some(h) = s.host.as_mut() else {
                fail(s, "not connected".into());
                return 0.0;
            };
            let Some(srv) = h.server_mut() else {
                fail(s, "only the server spawns".into());
                return 0.0;
            };
            let owner = if owner.is_nan() { 0 } else { owner as u32 };
            match srv.spawn(class, prefab, owner) {
                Ok(id) => id as f64,
                Err(e) => {
                    fail(s, e);
                    0.0
                }
            }
        })
    }

    pub fn despawn(id: f64) -> bool {
        with(|s| {
            s.host
                .as_mut()
                .and_then(|h| h.server_mut())
                .is_some_and(|srv| srv.despawn(id as NetId))
        })
    }

    pub fn set_owner(id: f64, owner: f64) -> bool {
        with(|s| {
            s.host
                .as_mut()
                .and_then(|h| h.server_mut())
                .is_some_and(|srv| srv.set_owner(id as NetId, owner as u32))
        })
    }

    /// The object's owner (player id), -1 when unknown.
    pub fn owner(id: f64) -> f64 {
        with(|s| {
            let Some(h) = s.host.as_ref() else {
                return -1.0;
            };
            let o = match (h.server_ref(), h.client_ref()) {
                (Some(srv), _) => srv.object(id as NetId).map(|o| o.owner),
                (_, Some(c)) => c.owner(id as NetId),
                _ => None,
            };
            o.map_or(-1.0, f64::from)
        })
    }

    /// True when this peer owns the object (always true on the server for objects it owns, player 0).
    pub fn is_mine(id: f64) -> bool {
        with(|s| {
            let Some(h) = s.host.as_ref() else {
                return false;
            };
            match (h.server_ref(), h.client_ref()) {
                (Some(srv), _) => srv.object(id as NetId).is_some_and(|o| o.owner == 0),
                (_, Some(c)) => c.is_mine(id as NetId),
                _ => false,
            }
        })
    }

    // ---- fields ----
    // On the server these set the value clients receive; on a client they write an `owner` field of
    // an object it owns. False (see `lastError`) when not allowed.

    /// An f32 or i32 field (or a ref).
    pub fn set_number(id: f64, field: &str, value: f64) -> bool {
        set_with(id, field, |ty| match ty {
            FieldType::F32 => Some(Value::F32(value as f32)),
            FieldType::I32 => Some(Value::I32(value as i32)),
            FieldType::Ref => Some(Value::Ref(value as u64)),
            _ => None,
        })
    }

    pub fn set_bool(id: f64, field: &str, value: bool) -> bool {
        set_with(id, field, |ty| {
            (ty == FieldType::Bool).then_some(Value::Bool(value))
        })
    }

    pub fn set_string(id: f64, field: &str, value: &str) -> bool {
        set_with(id, field, |ty| {
            (ty == FieldType::Str).then(|| Value::Str(value.to_owned()))
        })
    }

    pub fn set_vec3(id: f64, field: &str, x: f64, y: f64, z: f64) -> bool {
        set_with(id, field, |ty| {
            (ty == FieldType::Vec3).then_some(Value::Vec3([x as f32, y as f32, z as f32]))
        })
    }

    pub fn set_quat(id: f64, field: &str, x: f64, y: f64, z: f64, w: f64) -> bool {
        set_with(id, field, |ty| {
            (ty == FieldType::Quat).then_some(Value::Quat([x as f32, y as f32, z as f32, w as f32]))
        })
    }

    /// An f32, i32 or ref field as a number (NaN when unknown). Clients get it blended between ticks if the field is `lerp`.
    pub fn get_number(id: f64, field: &str) -> f64 {
        match read(id, field) {
            Some(Value::F32(x)) => f64::from(x),
            Some(Value::I32(n)) => f64::from(n),
            Some(Value::Ref(r)) => r as f64,
            _ => f64::NAN,
        }
    }

    pub fn get_bool(id: f64, field: &str) -> bool {
        matches!(read(id, field), Some(Value::Bool(true)))
    }

    pub fn get_string(id: f64, field: &str) -> String {
        match read(id, field) {
            Some(Value::Str(s)) => s,
            _ => String::new(),
        }
    }

    /// `[x, y, z]`, or an empty list when unknown.
    pub fn get_vec3(id: f64, field: &str) -> Vec<f64> {
        match read(id, field) {
            Some(Value::Vec3(v)) => v.iter().map(|x| f64::from(*x)).collect(),
            _ => Vec::new(),
        }
    }

    /// `[x, y, z, w]`, or an empty list when unknown.
    pub fn get_quat(id: f64, field: &str) -> Vec<f64> {
        match read(id, field) {
            Some(Value::Quat(v)) => v.iter().map(|x| f64::from(*x)).collect(),
            _ => Vec::new(),
        }
    }

    // ---- RPCs ----

    /// Calls an RPC with arguments as `f:1.5`, `i:3`, `b:1`, `s:text`, `v:x,y,z`, `q:x,y,z,w`, `r:id`
    /// joined by `\u{1f}` (none: an empty string). A client calls `server` RPCs, the server `owner` and `all` ones.
    #[optional(args)]
    pub fn rpc(id: f64, name: &str, args: &str) -> bool {
        with(|s| {
            let Some(h) = s.host.as_mut() else {
                return fail(s, "not connected".into());
            };
            let mut values = Vec::new();
            for a in args.split(SEP).filter(|a| !a.is_empty()) {
                match decode_arg(a) {
                    Some(v) => values.push(v),
                    None => return fail(s, format!("bad rpc argument {a:?}")),
                }
            }
            match h.rpc(id as NetId, name, &values) {
                Ok(()) => true,
                Err(e) => fail(s, e),
            }
        })
    }

    // ---- statistics and debugging ----

    /// RPCs this peer refused (unknown, not allowed, over the rate, malformed).
    pub fn dropped_rpcs() -> f64 {
        with(|s| s.host.as_ref().map_or(0.0, |h| h.dropped_rpcs as f64))
    }

    /// Packets that did not parse.
    pub fn bad_packets() -> f64 {
        with(|s| {
            s.host
                .as_ref()
                .map_or(0.0, |h| h.session().bad_packets as f64)
        })
    }

    /// Debug builds only: adds latency, jitter, loss and duplication (percent) and a bandwidth cap
    /// (kilobytes a second, 0 for none) in both directions. False in a build without `net-debug`
    /// or when the transport is not wrapped.
    #[optional(duplicate_percent, bandwidth_kbps)]
    pub fn debug_conditions(
        latency_ms: f64,
        jitter_ms: f64,
        loss_percent: f64,
        duplicate_percent: f64,
        bandwidth_kbps: f64,
    ) -> bool {
        #[cfg(feature = "net-debug")]
        {
            use platypus_net::conditioned::{Link, Links};
            let ms = |v: f64| Duration::from_secs_f64(v.max(0.0) / 1000.0);
            let pct = |v: f64| {
                if v.is_nan() {
                    0.0
                } else {
                    (v / 100.0).clamp(0.0, 1.0)
                }
            };
            let kb = if bandwidth_kbps.is_nan() {
                0.0
            } else {
                bandwidth_kbps
            };
            with(|s| {
                let Some(handle) = s.debug.as_ref() else {
                    return false;
                };
                let link = Link {
                    latency: ms(latency_ms),
                    jitter: ms(jitter_ms),
                    loss: pct(loss_percent),
                    burst: 2.0,
                    duplicate: pct(duplicate_percent),
                    bandwidth: (kb > 0.0).then_some((kb * 1000.0) as u64),
                    ..Link::default()
                };
                let links = Links::both(link);
                handle.set_default(links.clone());
                s.links = links;
                true
            })
        }
        #[cfg(not(feature = "net-debug"))]
        {
            let _ = (
                latency_ms,
                jitter_ms,
                loss_percent,
                duplicate_percent,
                bandwidth_kbps,
            );
            false
        }
    }

    /// Debug builds only: one-off faults: `disconnect`, `freeze:<ms>`, `corrupt:<n>`, and `blackhole:on` / `blackhole:off`.
    pub fn debug_fault(fault: &str) -> bool {
        #[cfg(feature = "net-debug")]
        {
            use platypus_net::conditioned::Fault;
            with(|s| {
                let Some(handle) = s.debug.as_ref() else {
                    return false;
                };
                let (kind, arg) = fault.split_once(':').unwrap_or((fault, ""));
                match kind {
                    "disconnect" => handle.fault(Fault::Disconnect(None)),
                    "freeze" => match arg.parse::<f64>() {
                        Ok(ms) => handle.fault(Fault::Freeze(Duration::from_secs_f64(ms / 1000.0))),
                        Err(_) => return false,
                    },
                    "corrupt" => match arg.parse::<u32>() {
                        Ok(n) => handle.fault(Fault::CorruptNext(n)),
                        Err(_) => return false,
                    },
                    "blackhole" => {
                        s.links.out.blackhole = arg == "on";
                        s.links.inbound.blackhole = arg == "on";
                        handle.set_default(s.links.clone());
                    }
                    _ => return false,
                }
                true
            })
        }
        #[cfg(not(feature = "net-debug"))]
        {
            let _ = fault;
            false
        }
    }

    /// Whether this build can inject network faults (debug builds with `net-debug`).
    pub fn has_debug() -> bool {
        with(|s| s.debug.is_some())
    }
}

/// Forgets everything (a test).
pub fn reset() {
    with(|s| *s = State::default());
}

#[cfg(test)]
mod tests {
    use super::network_api as net;
    use super::*;
    use platypus_net::host::HostConfig;
    use platypus_net::loopback::LoopbackHub;
    use platypus_net::schema::RpcTarget;
    use platypus_net::transport::Time;

    const MS: Duration = Duration::from_millis(1);

    fn define() {
        reset();
        assert!(net::define_class(
            "Player",
            "pos:vec3:q=0.01:lerp;hp:i32;aim:f32:owner;name:str;alive:bool"
        ));
        assert!(net::define_rpc("fire", "server", ""));
        assert!(net::define_rpc("boom", "all", "unreliable"));
    }

    // M8-API-01
    #[test]
    fn classes_and_rpcs_parse_and_bad_text_is_refused_with_a_reason() {
        reset();
        assert!(net::define_class(
            "A",
            "x:f32:q=0.5:lerp;y:vec3;z:quat;s:str;r:ref;b:bool;i:i32:owner"
        ));
        let schema = schema().unwrap();
        assert_eq!(schema.classes[0].fields.len(), 7);
        assert_eq!(schema.classes[0].fields[0].quantize, Some(0.5));
        assert!(schema.classes[0].fields[6].owner_writes);

        assert!(!net::define_class("B", "x:float"));
        assert!(net::last_error().contains("type must be"));
        assert!(!net::define_class("B", "x:f32:wat"));
        assert!(net::last_error().contains("unknown flag"));
        assert!(!net::define_class("B", "x:f32:q=0"));
        assert!(!net::define_rpc("r", "everyone", ""));
        assert!(!net::define_rpc("r", "server", "loud"));
        assert!(net::define_rpc("r", "server", "anyone,unreliable"));
        let rpc = schema_rpc("r");
        assert!(!rpc.require_owner && !rpc.reliable && rpc.target == RpcTarget::Server);
        // Redefining replaces.
        assert!(net::define_class("A", "only:bool"));
        assert_eq!(super::schema().unwrap().classes.len(), 1);
        reset();
    }

    fn schema_rpc(name: &str) -> RpcDef {
        let s = schema().unwrap();
        s.rpc(s.rpc_id(name).unwrap()).unwrap().clone()
    }

    // M8-API-02
    #[test]
    fn arguments_round_trip_through_their_text_form() {
        for v in [
            Value::Bool(true),
            Value::I32(-5),
            Value::F32(1.5),
            Value::Vec3([1.0, -2.5, 3.0]),
            Value::Quat([0.0, 0.0, 0.0, 1.0]),
            Value::Str("hi: there".into()),
            Value::Ref(77),
        ] {
            assert_eq!(decode_arg(&encode_arg(&v)), Some(v));
        }
        assert_eq!(decode_arg("v:1,2"), None);
        assert_eq!(decode_arg("x:1"), None);
        assert_eq!(decode_arg("nocolon"), None);
        assert_eq!(decode_arg("i:abc"), None);
    }

    // M8-API-03
    #[test]
    fn scripts_drive_a_server_and_a_client_through_the_namespace() {
        define();
        let schema = schema().unwrap();
        let hub = LoopbackHub::new();
        let mut st = hub.endpoint();
        st.listen("srv").unwrap();
        install(
            NetHost::server(
                Box::new(st) as Box<dyn Transport>,
                schema.clone(),
                HostConfig::default(),
            ),
            None,
        );
        // A client in the test's own hands (a second peer).
        let mut ct = hub.endpoint();
        ct.connect("srv").unwrap();
        let mut client = NetHost::client(
            Box::new(ct) as Box<dyn Transport>,
            schema,
            HostConfig::default(),
            "cat",
            None,
        );
        assert!(net::is_server() && !net::is_client() && net::is_connected());
        assert_eq!(net::local_player(), 0.0);

        let mut now = Time::ZERO;
        let mut step = |client: &mut NetHost<Box<dyn Transport>>, ms: u64| {
            for _ in 0..ms / 5 {
                now += 5 * MS;
                update(now);
                client.update(now);
            }
        };
        step(&mut client, 200);
        let events = net::take_events();
        assert!(
            events.contains(&format!("joined{SEP}1{SEP}cat{SEP}0")),
            "{events}"
        );
        assert_eq!(net::players(), vec![1.0]);
        assert_eq!(net::player_name(1.0), "cat");

        // Spawn, set fields, and the client sees them.
        let id = net::spawn("Player", "player.prefab", 1.0);
        assert!(id > 0.0);
        assert!(net::set_vec3(id, "pos", 1.234, 2.0, 3.0));
        assert!(net::set_number(id, "hp", 90.0));
        assert!(net::set_string(id, "name", "Cat"));
        assert!(net::set_bool(id, "alive", true));
        assert!(!net::set_number(id, "pos", 1.0), "pos is a vec3");
        assert!(!net::set_number(id, "nope", 1.0));
        assert_eq!(net::class_name(0.0), "Player");
        step(&mut client, 300);
        let c = client.client_ref().unwrap();
        assert_eq!(c.value(id as u64, "hp"), Some(Value::I32(90)));
        assert_eq!(c.value(id as u64, "name"), Some(Value::Str("Cat".into())));
        assert_eq!(
            c.value(id as u64, "pos"),
            Some(Value::Vec3([1.23, 2.0, 3.0]))
        );
        assert!(!net::is_mine(id) && net::owner(id) == 1.0);
        assert_eq!(net::get_number(id, "hp"), 90.0);
        assert_eq!(net::get_vec3(id, "pos"), vec![f64::from(1.23f32), 2.0, 3.0]);
        assert!(net::get_bool(id, "alive"));
        assert_eq!(net::get_string(id, "name"), "Cat");
        assert!(net::get_number(id, "unknown").is_nan());

        // The client calls an RPC; scripts on the server see an `rpc` line.
        client
            .rpc(
                id as u64,
                "fire",
                &[Value::Vec3([0.0, 0.0, 1.0]), Value::I32(2)],
            )
            .unwrap();
        step(&mut client, 100);
        let events = net::take_events();
        let want = format!("rpc{SEP}{}{SEP}fire{SEP}1{SEP}v:0,0,1{SEP}i:2", id as u64);
        assert!(events.lines().any(|l| l == want), "{events}");
        // And the server calls one back through the namespace.
        assert!(net::rpc(id, "boom", ""));
        assert!(
            !net::rpc(id, "fire", ""),
            "a server cannot call a server rpc"
        );
        assert!(net::last_error().contains("runs on the server"));
        assert!(!net::rpc(id, "boom", "q:bad"));
        step(&mut client, 100);

        uninstall();
        assert!(!net::is_server());
        reset();
    }

    // M8-API-04
    #[test]
    fn requests_queue_for_the_app_and_calls_without_a_host_fail_cleanly() {
        reset();
        net::start_server(7777.0, f64::NAN, f64::NAN);
        net::connect("127.0.0.1:7777", "ann");
        net::disconnect();
        assert_eq!(
            take_requests(),
            vec![
                Request::StartServer {
                    port: 7777,
                    max_players: 16,
                    tick_rate: 30
                },
                Request::Connect {
                    address: "127.0.0.1:7777".into(),
                    name: "ann".into()
                },
                Request::Disconnect,
            ]
        );
        assert!(take_requests().is_empty());
        assert_eq!(net::spawn("Player", "p", f64::NAN), 0.0);
        assert!(!net::set_number(1.0, "hp", 1.0));
        assert!(!net::rpc(1.0, "x", ""));
        assert_eq!(net::local_player(), -1.0);
        assert_eq!(net::rtt(f64::NAN), -1.0);
        // No host: no clock.
        assert_eq!(
            (net::tick(), net::time(), net::tick_rate()),
            (-1.0, -1.0, 0.0)
        );
        assert!(!net::debug_conditions(10.0, 0.0, 0.0, f64::NAN, f64::NAN));
        assert!(!net::debug_fault("disconnect"));
        report_error("port in use");
        assert!(net::take_events().ends_with(&format!("error{SEP}port in use")));
        reset();
    }

    // M8-API-05
    #[cfg(feature = "net-debug")]
    #[test]
    fn debug_conditions_slow_a_wrapped_transport_and_faults_disconnect_it() {
        use platypus_net::conditioned::Conditioned;
        define();
        let schema = schema().unwrap();
        let hub = LoopbackHub::new();
        let mut st = hub.endpoint();
        st.listen("srv").unwrap();
        let (wrapped, handle) = Conditioned::new(st, 1);
        install(
            NetHost::server(
                Box::new(wrapped) as Box<dyn Transport>,
                schema.clone(),
                HostConfig::default(),
            ),
            Some(handle),
        );
        assert!(net::has_debug());
        assert!(net::debug_conditions(100.0, 0.0, 0.0, f64::NAN, f64::NAN));
        let mut ct = hub.endpoint();
        ct.connect("srv").unwrap();
        let mut client = NetHost::client(
            Box::new(ct) as Box<dyn Transport>,
            schema,
            HostConfig::default(),
            "x",
            None,
        );
        let mut now = Time::ZERO;
        let mut joined_at = None;
        for _ in 0..200 {
            now += 5 * MS;
            update(now);
            client.update(now);
            if joined_at.is_none() && net::take_events().contains("joined") {
                joined_at = Some(now);
            }
        }
        // Hello travels in 100 ms: the join cannot be sooner.
        assert!(joined_at.unwrap() >= 100 * MS, "{joined_at:?}");
        assert!(net::debug_fault("freeze:50"));
        assert!(net::debug_fault("blackhole:on"));
        assert!(net::debug_fault("blackhole:off"));
        assert!(net::debug_fault("disconnect"));
        assert!(!net::debug_fault("freeze:abc"));
        assert!(!net::debug_fault("explode"));
        for _ in 0..100 {
            now += 5 * MS;
            update(now);
        }
        assert!(net::take_events().contains("dropped"));
        uninstall();
        reset();
    }
}
