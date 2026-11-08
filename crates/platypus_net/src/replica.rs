// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Replication: the server's objects reach every client that should see them.
//!
//! - **Spawn / despawn / owner change** are reliable messages, so every client has the same set of
//!   objects in the same order.
//! - **State** is an unreliable message per tick holding the fields that differ from the newest
//!   state the client has *acknowledged* (a delta against a known baseline). A lost state costs
//!   nothing: the next one is still a delta against the same baseline. The client rebuilds each
//!   tick from its stored copy of the baseline, never from "whatever it holds now", so a value that
//!   changed and changed back between baseline and now cannot be missed.
//! - A state too big for one datagram is split into chunks; the client acknowledges a tick once it
//!   has every chunk, and never one that mentioned an object it does not know yet.
//! - **Interest**: a predicate decides which objects each player sees; leaving it despawns the
//!   object on that client, entering it spawns it.
//! - **Owner fields**: the owning client writes fields marked `owner_writes`; the server checks the
//!   sender owns the object and the field allows it.
//!
//! Both halves are plain data structures driven by the caller (`step`, `handle`); nothing here
//! touches a transport.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;

use crate::schema::{ClassDef, Schema, Value};
use crate::session::PlayerId;
use crate::transport::Channel;
use crate::wire::{Reader, Writer};

/// A networked object's id, assigned by the server (never 0).
pub type NetId = u64;

/// The server's simulation tick.
pub type Tick = u32;

/// First byte of the messages this module sends inside session data.
mod kind {
    pub const SPAWN: u8 = 0x10;
    pub const DESPAWN: u8 = 0x11;
    pub const STATE: u8 = 0x12;
    pub const ACK: u8 = 0x13;
    pub const OWNER_FIELDS: u8 = 0x14;
    pub const OWNER_CHANGED: u8 = 0x15;
}

/// Ticks of state kept as baselines (a client that falls further behind gets a full state).
const HISTORY: usize = 64;
/// The most a state chunk carries, under the unreliable datagram limit with room for the header.
const CHUNK_BYTES: usize = 1000;

/// A message for one player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outgoing {
    pub to: PlayerId,
    pub channel: Channel,
    pub bytes: Vec<u8>,
}

/// A networked object on the server.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectState {
    pub class: u16,
    pub owner: PlayerId,
    /// What the clients instantiate (a prefab path or name).
    pub prefab: String,
    pub values: Vec<Value>,
}

type Interest = Box<dyn Fn(PlayerId, NetId, &ObjectState) -> bool + Send>;

struct PeerSync {
    /// Objects the client has been told about, with the tick of that spawn message.
    known: BTreeMap<NetId, Tick>,
    /// The newest tick the client acknowledged.
    acked: Option<Tick>,
}

/// The server half.
pub struct ReplicaServer {
    schema: Arc<Schema>,
    objects: BTreeMap<NetId, ObjectState>,
    next_id: NetId,
    tick: Tick,
    history: VecDeque<(Tick, BTreeMap<NetId, Vec<Value>>)>,
    peers: HashMap<PlayerId, PeerSync>,
    interest: Option<Interest>,
    /// Reliable messages to send at the next `step` (owner changes).
    pending: Vec<Outgoing>,
}

impl ReplicaServer {
    pub fn new(schema: Arc<Schema>) -> Self {
        ReplicaServer {
            schema,
            objects: BTreeMap::new(),
            next_id: 1,
            tick: 0,
            history: VecDeque::new(),
            peers: HashMap::new(),
            interest: None,
            pending: Vec::new(),
        }
    }

    pub fn tick(&self) -> Tick {
        self.tick
    }

    pub fn schema(&self) -> &Arc<Schema> {
        &self.schema
    }

    /// Chooses which objects each player sees (default: all).
    pub fn set_interest(
        &mut self,
        f: impl Fn(PlayerId, NetId, &ObjectState) -> bool + Send + 'static,
    ) {
        self.interest = Some(Box::new(f));
    }

    pub fn add_player(&mut self, player: PlayerId) {
        self.peers.insert(
            player,
            PeerSync {
                known: BTreeMap::new(),
                acked: None,
            },
        );
    }

    pub fn remove_player(&mut self, player: PlayerId) {
        self.peers.remove(&player);
    }

    /// A player whose connection came back: it starts from nothing (a full set of spawns).
    pub fn reset_player(&mut self, player: PlayerId) {
        self.add_player(player);
    }

    /// Makes an object of `class` (fields at their zero values).
    pub fn spawn(&mut self, class: &str, prefab: &str, owner: PlayerId) -> Result<NetId, String> {
        let id = self
            .schema
            .class_id(class)
            .ok_or_else(|| format!("no networked class {class:?}"))?;
        let def = self.schema.class(id).expect("class_id is in range");
        let values = def.fields.iter().map(|f| f.default_value()).collect();
        let net = self.next_id;
        self.next_id += 1;
        self.objects.insert(
            net,
            ObjectState {
                class: id,
                owner,
                prefab: prefab.to_owned(),
                values,
            },
        );
        Ok(net)
    }

    pub fn despawn(&mut self, id: NetId) -> bool {
        self.objects.remove(&id).is_some()
    }

    pub fn object(&self, id: NetId) -> Option<&ObjectState> {
        self.objects.get(&id)
    }

    pub fn objects(&self) -> impl Iterator<Item = (NetId, &ObjectState)> {
        self.objects.iter().map(|(id, o)| (*id, o))
    }

    /// Sets a field by name; false for an unknown object or field or a value of the wrong type.
    pub fn set(&mut self, id: NetId, field: &str, value: Value) -> bool {
        let Some(obj) = self.objects.get_mut(&id) else {
            return false;
        };
        let Some(def) = self.schema.class(obj.class) else {
            return false;
        };
        let Some(i) = def.field_index(field) else {
            return false;
        };
        if def.fields[i].ty != value.ty() {
            return false;
        }
        obj.values[i] = def.fields[i].normalize(value);
        true
    }

    pub fn get(&self, id: NetId, field: &str) -> Option<&Value> {
        let obj = self.objects.get(&id)?;
        let i = self.schema.class(obj.class)?.field_index(field)?;
        obj.values.get(i)
    }

    /// Gives an object to another player; clients that know it are told.
    pub fn set_owner(&mut self, id: NetId, owner: PlayerId) -> bool {
        let Some(obj) = self.objects.get_mut(&id) else {
            return false;
        };
        if obj.owner == owner {
            return true;
        }
        obj.owner = owner;
        let mut w = Writer::new();
        w.u8(kind::OWNER_CHANGED)
            .varint(id)
            .u32(owner)
            .u32(self.tick);
        let bytes = w.finish();
        for (player, sync) in &self.peers {
            if sync.known.contains_key(&id) {
                self.pending.push(Outgoing {
                    to: *player,
                    channel: Channel::Reliable,
                    bytes: bytes.clone(),
                });
            }
        }
        true
    }

    /// Whether a player has been told about an object (so an RPC about it makes sense to them).
    pub fn knows(&self, player: PlayerId, id: NetId) -> bool {
        self.peers
            .get(&player)
            .is_some_and(|p| p.known.contains_key(&id))
    }

    /// The players being replicated to, in id order.
    pub fn players(&self) -> Vec<PlayerId> {
        let mut p: Vec<_> = self.peers.keys().copied().collect();
        p.sort_unstable();
        p
    }

    /// Handles a replication message from a player (acks, owner-written fields); anything else or
    /// anything malformed is ignored.
    pub fn handle(&mut self, from: PlayerId, bytes: &[u8]) {
        let mut r = Reader::new(bytes);
        match r.u8() {
            Some(kind::ACK) => {
                if let (Some(tick), Some(sync)) = (r.u32(), self.peers.get_mut(&from)) {
                    // Only a tick the server still holds can be a baseline; newest wins.
                    if self.history.iter().any(|(t, _)| *t == tick)
                        && sync.acked.is_none_or(|a| tick > a)
                    {
                        sync.acked = Some(tick);
                    }
                }
            }
            Some(kind::OWNER_FIELDS) => self.apply_owner_fields(from, r),
            _ => {}
        }
    }

    fn apply_owner_fields(&mut self, from: PlayerId, mut r: Reader<'_>) {
        let Some(count) = r.varint() else { return };
        for _ in 0..count.min(256) {
            let (Some(id), Some(mask)) = (r.varint(), r.varint()) else {
                return;
            };
            let Some(obj) = self.objects.get_mut(&id) else {
                return;
            };
            let Some(def) = self.schema.class(obj.class) else {
                return;
            };
            // Read the whole entry (to stay in step with the stream) before judging it.
            let mut updates = Vec::new();
            for (i, f) in def.fields.iter().enumerate() {
                if mask & (1 << i) != 0 {
                    let Some(v) = Value::read(&mut r, f) else {
                        return;
                    };
                    updates.push((i, v));
                }
            }
            if obj.owner != from {
                continue;
            }
            for (i, v) in updates {
                if def.fields[i].owner_writes {
                    obj.values[i] = def.fields[i].normalize(v);
                }
            }
        }
    }

    /// Advances one tick and returns what to send: spawns, despawns and owner changes (reliable),
    /// then each player's state delta (unreliable chunks).
    pub fn step(&mut self) -> Vec<Outgoing> {
        self.tick += 1;
        let snapshot: BTreeMap<NetId, Vec<Value>> = self
            .objects
            .iter()
            .map(|(id, o)| (*id, o.values.clone()))
            .collect();
        self.history.push_back((self.tick, snapshot));
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
        let mut out = std::mem::take(&mut self.pending);
        let tick = self.tick;
        let players: Vec<PlayerId> = {
            let mut p: Vec<_> = self.peers.keys().copied().collect();
            p.sort_unstable();
            p
        };
        for player in players {
            let visible: Vec<NetId> = self
                .objects
                .iter()
                .filter(|(id, o)| self.interest.as_ref().is_none_or(|f| f(player, **id, o)))
                .map(|(id, _)| *id)
                .collect();
            let visible_set: HashSet<NetId> = visible.iter().copied().collect();
            let sync = self.peers.get_mut(&player).expect("listed above");
            let gone: Vec<NetId> = sync
                .known
                .keys()
                .filter(|id| !visible_set.contains(id))
                .copied()
                .collect();
            for id in gone {
                sync.known.remove(&id);
                let mut w = Writer::new();
                w.u8(kind::DESPAWN).varint(id);
                out.push(Outgoing {
                    to: player,
                    channel: Channel::Reliable,
                    bytes: w.finish(),
                });
            }
            for id in &visible {
                if sync.known.contains_key(id) {
                    continue;
                }
                let obj = &self.objects[id];
                let Some(def) = self.schema.class(obj.class) else {
                    continue;
                };
                let mut w = Writer::new();
                w.u8(kind::SPAWN)
                    .varint(*id)
                    .varint(u64::from(obj.class))
                    .u32(obj.owner)
                    .str(&obj.prefab)
                    .u32(tick);
                for (f, v) in def.fields.iter().zip(&obj.values) {
                    v.write(&mut w, f);
                }
                sync.known.insert(*id, tick);
                out.push(Outgoing {
                    to: player,
                    channel: Channel::Reliable,
                    bytes: w.finish(),
                });
            }
            // State against the acknowledged baseline, if the server still holds it.
            let baseline = sync
                .acked
                .and_then(|a| self.history.iter().find(|(t, _)| *t == a));
            let base_tick = baseline.map_or(0, |(t, _)| *t);
            let mut entries: Vec<Vec<u8>> = Vec::new();
            for (id, since) in &sync.known {
                let Some(obj) = self.objects.get(id) else {
                    continue;
                };
                let Some(def) = self.schema.class(obj.class) else {
                    continue;
                };
                let base_values = baseline
                    .filter(|_| *since <= base_tick)
                    .and_then(|(_, s)| s.get(id));
                let mask = changed_mask(def, &obj.values, base_values);
                if mask == 0 {
                    continue;
                }
                let mut w = Writer::new();
                w.varint(*id).varint(mask);
                for (i, (f, v)) in def.fields.iter().zip(&obj.values).enumerate() {
                    if mask & (1 << i) != 0 {
                        v.write(&mut w, f);
                    }
                }
                entries.push(w.finish());
            }
            out.extend(chunk_state(player, tick, base_tick, &entries));
        }
        out
    }
}

/// The fields of `values` that differ from `base` (all of them without a base), as a bit mask.
fn changed_mask(def: &ClassDef, values: &[Value], base: Option<&Vec<Value>>) -> u64 {
    let mut mask = 0u64;
    for i in 0..def.fields.len().min(64) {
        if base.is_none_or(|b| b.get(i) != values.get(i)) {
            mask |= 1 << i;
        }
    }
    mask
}

fn chunk_state(player: PlayerId, tick: Tick, base: Tick, entries: &[Vec<u8>]) -> Vec<Outgoing> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut groups: Vec<Vec<&Vec<u8>>> = vec![Vec::new()];
    let mut size = 0;
    for e in entries {
        if size + e.len() > CHUNK_BYTES && !groups.last().is_some_and(Vec::is_empty) {
            groups.push(Vec::new());
            size = 0;
        }
        size += e.len();
        groups.last_mut().expect("never empty").push(e);
    }
    let count = groups.len().min(255) as u8;
    groups
        .iter()
        .take(255)
        .enumerate()
        .map(|(i, g)| {
            let mut w = Writer::new();
            w.u8(kind::STATE)
                .u32(tick)
                .u32(base)
                .u8(i as u8)
                .u8(count)
                .varint(g.len() as u64);
            for e in g {
                w.bytes(e);
            }
            Outgoing {
                to: player,
                channel: Channel::Unreliable,
                bytes: w.finish(),
            }
        })
        .collect()
}

/// What the client tells the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplicaEvent {
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
}

struct ClientObject {
    class: u16,
    owner: PlayerId,
    /// Values from the spawn message, until a state arrives.
    spawn_values: Vec<Value>,
}

struct Partial {
    base: Tick,
    count: u8,
    got: HashSet<u8>,
    state: BTreeMap<NetId, Vec<Value>>,
}

/// The client half.
pub struct ReplicaClient {
    schema: Arc<Schema>,
    me: PlayerId,
    objects: BTreeMap<NetId, ClientObject>,
    /// Complete states by tick.
    states: BTreeMap<Tick, BTreeMap<NetId, Vec<Value>>>,
    partial: BTreeMap<Tick, Partial>,
    latest: Option<Tick>,
    ack: Option<Tick>,
    /// Owner-written fields set since the last `take_owner_update`, and what the game last set
    /// (so reads of its own fields do not wait for the server's echo).
    owner_out: BTreeMap<NetId, BTreeMap<usize, Value>>,
    local: BTreeMap<NetId, BTreeMap<usize, Value>>,
}

impl ReplicaClient {
    pub fn new(schema: Arc<Schema>, me: PlayerId) -> Self {
        ReplicaClient {
            schema,
            me,
            objects: BTreeMap::new(),
            states: BTreeMap::new(),
            partial: BTreeMap::new(),
            latest: None,
            ack: None,
            owner_out: BTreeMap::new(),
            local: BTreeMap::new(),
        }
    }

    /// The newest complete tick.
    pub fn latest_tick(&self) -> Option<Tick> {
        self.latest
    }

    pub fn ids(&self) -> impl Iterator<Item = NetId> + '_ {
        self.objects.keys().copied()
    }

    pub fn owner(&self, id: NetId) -> Option<PlayerId> {
        self.objects.get(&id).map(|o| o.owner)
    }

    pub fn class(&self, id: NetId) -> Option<u16> {
        self.objects.get(&id).map(|o| o.class)
    }

    pub fn is_mine(&self, id: NetId) -> bool {
        self.owner(id) == Some(self.me)
    }

    fn field_index(&self, id: NetId, field: &str) -> Option<usize> {
        let o = self.objects.get(&id)?;
        self.schema.class(o.class)?.field_index(field)
    }

    /// The newest value of a field (the game's own pending write wins for an owner-written field).
    pub fn value(&self, id: NetId, field: &str) -> Option<Value> {
        let i = self.field_index(id, field)?;
        if let Some(v) = self.local.get(&id).and_then(|m| m.get(&i)) {
            return Some(v.clone());
        }
        let latest = self
            .latest
            .and_then(|t| self.states.get(&t))
            .and_then(|s| s.get(&id))
            .and_then(|v| v.get(i));
        latest
            .or_else(|| self.objects.get(&id)?.spawn_values.get(i))
            .cloned()
    }

    /// A field blended between the two complete states around `render_tick` (a fractional tick,
    /// normally the latest tick minus the interpolation delay); a field not marked `interpolate`,
    /// or a time outside what is stored, gives the nearest state's value.
    pub fn interpolated(&self, id: NetId, field: &str, render_tick: f64) -> Option<Value> {
        let i = self.field_index(id, field)?;
        let o = self.objects.get(&id)?;
        let def = &self.schema.class(o.class)?.fields[i];
        let at = |t: Tick| {
            self.states
                .get(&t)
                .and_then(|s| s.get(&id))
                .and_then(|v| v.get(i))
        };
        let lo = self
            .states
            .range(..=(render_tick.floor().max(0.0) as Tick))
            .rev()
            .find(|(_, s)| s.contains_key(&id))
            .map(|(t, _)| *t);
        let hi = self
            .states
            .range((render_tick.ceil().max(0.0) as Tick)..)
            .find(|(_, s)| s.contains_key(&id))
            .map(|(t, _)| *t);
        match (lo, hi) {
            (Some(a), Some(b)) if a != b && def.interpolate => {
                let t = ((render_tick - f64::from(a)) / f64::from(b - a)).clamp(0.0, 1.0) as f32;
                Some(at(a)?.lerp(at(b)?, t))
            }
            (Some(a), _) => at(a).cloned(),
            (None, Some(b)) => at(b).cloned(),
            (None, None) => self.value(id, field),
        }
    }

    /// Writes an owner-written field of an object this player owns; false otherwise.
    pub fn set_owned(&mut self, id: NetId, field: &str, value: Value) -> bool {
        let Some(i) = self.field_index(id, field) else {
            return false;
        };
        let o = &self.objects[&id];
        let def = &self.schema.class(o.class).expect("known class").fields[i];
        if o.owner != self.me || !def.owner_writes || def.ty != value.ty() {
            return false;
        }
        let v = def.normalize(value);
        self.local.entry(id).or_default().insert(i, v.clone());
        self.owner_out.entry(id).or_default().insert(i, v);
        true
    }

    /// The message carrying owner-written fields set since the last call (send it unreliably
    /// sequenced, each frame the game sets them).
    pub fn take_owner_update(&mut self) -> Option<Vec<u8>> {
        if self.owner_out.is_empty() {
            return None;
        }
        let mut w = Writer::new();
        w.u8(kind::OWNER_FIELDS).varint(self.owner_out.len() as u64);
        for (id, fields) in std::mem::take(&mut self.owner_out) {
            let Some(o) = self.objects.get(&id) else {
                continue;
            };
            let def = self.schema.class(o.class).expect("known class");
            let mask = fields.keys().fold(0u64, |m, i| m | (1 << i));
            w.varint(id).varint(mask);
            for (i, v) in &fields {
                v.write(&mut w, &def.fields[*i]);
            }
        }
        Some(w.finish())
    }

    /// The ack to send (unreliable) for the newest complete tick, once.
    pub fn take_ack(&mut self) -> Option<Vec<u8>> {
        let tick = self.ack.take()?;
        let mut w = Writer::new();
        w.u8(kind::ACK).u32(tick);
        Some(w.finish())
    }

    /// Handles a message from the server; malformed ones are ignored.
    pub fn handle(&mut self, bytes: &[u8]) -> Vec<ReplicaEvent> {
        let mut r = Reader::new(bytes);
        let mut events = Vec::new();
        match r.u8() {
            Some(kind::SPAWN) => {
                if let Some(e) = self.on_spawn(&mut r) {
                    events.push(e);
                }
            }
            Some(kind::DESPAWN) => {
                if let Some(id) = r.varint()
                    && self.objects.remove(&id).is_some()
                {
                    self.local.remove(&id);
                    self.owner_out.remove(&id);
                    events.push(ReplicaEvent::Despawned { id });
                }
            }
            Some(kind::OWNER_CHANGED) => {
                if let (Some(id), Some(owner)) = (r.varint(), r.u32())
                    && let Some(o) = self.objects.get_mut(&id)
                {
                    o.owner = owner;
                    self.local.remove(&id);
                    self.owner_out.remove(&id);
                    events.push(ReplicaEvent::OwnerChanged { id, owner });
                }
            }
            Some(kind::STATE) => self.on_state(r),
            _ => {}
        }
        events
    }

    fn on_spawn(&mut self, r: &mut Reader<'_>) -> Option<ReplicaEvent> {
        let id = r.varint()?;
        let class = u16::try_from(r.varint()?).ok()?;
        let owner = r.u32()?;
        let prefab = r.str(256)?;
        let _tick = r.u32()?;
        let def = self.schema.class(class)?;
        let mut values = Vec::with_capacity(def.fields.len());
        for f in &def.fields {
            values.push(Value::read(r, f)?);
        }
        self.objects.insert(
            id,
            ClientObject {
                class,
                owner,
                spawn_values: values,
            },
        );
        Some(ReplicaEvent::Spawned {
            id,
            class,
            prefab,
            owner,
        })
    }

    fn on_state(&mut self, mut r: Reader<'_>) {
        let (Some(tick), Some(base), Some(index), Some(count), Some(n)) =
            (r.u32(), r.u32(), r.u8(), r.u8(), r.varint())
        else {
            return;
        };
        if count == 0
            || index >= count
            || self.states.contains_key(&tick)
            || self.latest.is_some_and(|l| tick + HISTORY as u32 <= l)
        {
            return;
        }
        // The baseline is a state this client completed (and so acknowledged); anything else
        // means the message cannot be applied.
        let base_state = if base == 0 {
            BTreeMap::new()
        } else {
            match self.states.get(&base) {
                Some(s) => s.clone(),
                None => return,
            }
        };
        let objects = &self.objects;
        let schema = &self.schema;
        let partial = self.partial.entry(tick).or_insert_with(|| Partial {
            base,
            count,
            got: HashSet::new(),
            state: base_state
                .into_iter()
                .filter(|(id, _)| objects.contains_key(id))
                .collect(),
        });
        if partial.base != base || partial.count != count || !partial.got.insert(index) {
            return;
        }
        let mut ok = true;
        for _ in 0..n.min(100_000) {
            let (Some(id), Some(mask)) = (r.varint(), r.varint()) else {
                ok = false;
                break;
            };
            // An object not spawned yet: the whole tick is unusable (and is never acknowledged).
            let Some(obj) = objects.get(&id) else {
                ok = false;
                break;
            };
            let Some(def) = schema.class(obj.class) else {
                ok = false;
                break;
            };
            let values = partial
                .state
                .entry(id)
                .or_insert_with(|| def.fields.iter().map(|f| f.default_value()).collect());
            for (i, f) in def.fields.iter().enumerate() {
                if mask & (1 << i) != 0 {
                    match Value::read(&mut r, f) {
                        Some(v) => values[i] = v,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
            }
            if !ok {
                break;
            }
        }
        if !ok {
            self.partial.remove(&tick);
            return;
        }
        if partial.got.len() == usize::from(partial.count) {
            let done = self.partial.remove(&tick).expect("just used");
            self.states.insert(tick, done.state);
            if self.latest.is_none_or(|l| tick > l) {
                self.latest = Some(tick);
                self.ack = Some(tick);
            }
            let newest = self.latest.unwrap_or(tick);
            self.states.retain(|t, _| t + HISTORY as u32 > newest);
            self.partial.retain(|t, _| t + HISTORY as u32 > newest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{FieldDef, FieldType};

    fn schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            ClassDef::new("Player")
                .field(
                    FieldDef::new("pos", FieldType::Vec3)
                        .quantized(0.01)
                        .interpolated(),
                )
                .field(FieldDef::new("hp", FieldType::I32))
                .field(FieldDef::new("aim", FieldType::F32).owner_writes()),
            ClassDef::new("Crate").field(FieldDef::new("pos", FieldType::Vec3).quantized(0.01)),
        ]))
    }

    /// A tiny deterministic generator for the tests.
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn chance(&mut self, p: f64) -> bool {
            (self.next() as f64 / (1u64 << 31) as f64) < p
        }
    }

    struct Rig {
        server: ReplicaServer,
        clients: Vec<(PlayerId, ReplicaClient)>,
        rng: Lcg,
        /// Chance each unreliable message (state and ack) is lost.
        loss: f64,
        events: Vec<(PlayerId, ReplicaEvent)>,
        sent: Vec<usize>,
    }

    impl Rig {
        fn new(players: &[PlayerId], loss: f64) -> Rig {
            let s = schema();
            let mut server = ReplicaServer::new(s.clone());
            let clients = players
                .iter()
                .map(|p| {
                    server.add_player(*p);
                    (*p, ReplicaClient::new(s.clone(), *p))
                })
                .collect();
            Rig {
                server,
                clients,
                rng: Lcg(7),
                loss,
                events: Vec::new(),
                sent: Vec::new(),
            }
        }

        /// One tick: the server steps, messages reach clients (some lost), acks come back.
        fn tick(&mut self) {
            for m in self.server.step() {
                self.sent.push(m.bytes.len());
                let lossy = m.channel == Channel::Unreliable;
                if lossy && self.rng.chance(self.loss) {
                    continue;
                }
                let (_, c) = self
                    .clients
                    .iter_mut()
                    .find(|(p, _)| *p == m.to)
                    .expect("a client");
                let evs = c.handle(&m.bytes);
                self.events.extend(evs.into_iter().map(|e| (m.to, e)));
            }
            for (p, c) in &mut self.clients {
                if let Some(ack) = c.take_ack()
                    && !self.rng.chance(self.loss)
                {
                    self.server.handle(*p, &ack);
                }
            }
        }

        fn client(&mut self, p: PlayerId) -> &mut ReplicaClient {
            &mut self
                .clients
                .iter_mut()
                .find(|(q, _)| *q == p)
                .expect("client")
                .1
        }
    }

    // M8-REP-04
    #[test]
    fn a_spawned_object_and_its_changes_reach_the_client() {
        let mut rig = Rig::new(&[1], 0.0);
        let id = rig.server.spawn("Player", "player.prefab", 1).unwrap();
        rig.server.set(id, "hp", Value::I32(100));
        rig.tick();
        assert_eq!(
            rig.events,
            vec![(
                1,
                ReplicaEvent::Spawned {
                    id,
                    class: 0,
                    prefab: "player.prefab".into(),
                    owner: 1
                }
            )]
        );
        assert_eq!(rig.client(1).value(id, "hp"), Some(Value::I32(100)));
        rig.server.set(id, "pos", Value::Vec3([1.0, 2.0, 3.0]));
        rig.server.set(id, "hp", Value::I32(80));
        rig.tick();
        rig.tick();
        let c = rig.client(1);
        assert_eq!(c.value(id, "pos"), Some(Value::Vec3([1.0, 2.0, 3.0])));
        assert_eq!(c.value(id, "hp"), Some(Value::I32(80)));
        assert!(rig.client(1).is_mine(id));
    }

    // M8-REP-04
    #[test]
    fn once_acknowledged_only_changed_fields_are_sent() {
        let mut rig = Rig::new(&[1], 0.0);
        let id = rig.server.spawn("Player", "p", 1).unwrap();
        rig.server.set(id, "pos", Value::Vec3([5.0, 5.0, 5.0]));
        for _ in 0..3 {
            rig.tick();
        }
        rig.sent.clear();
        rig.tick();
        assert!(
            rig.sent.is_empty(),
            "nothing changed, nothing sent: {:?}",
            rig.sent
        );
        rig.server.set(id, "hp", Value::I32(7));
        rig.tick();
        // One small entry: a header and one field.
        assert_eq!(rig.sent.len(), 1);
        assert!(rig.sent[0] < 20, "{}", rig.sent[0]);
    }

    // M8-REP-05
    #[test]
    fn clients_converge_through_heavy_loss_and_values_that_change_back() {
        let mut rig = Rig::new(&[1, 2], 0.4);
        let ids: Vec<NetId> = (0..20)
            .map(|i| {
                rig.server
                    .spawn(if i % 2 == 0 { "Player" } else { "Crate" }, "p", 1)
                    .unwrap()
            })
            .collect();
        let mut mutate = Lcg(99);
        for _ in 0..400 {
            // Churn: change, and sometimes change back the next tick.
            for id in &ids {
                if mutate.chance(0.3) {
                    let x = (mutate.next() % 50) as f32;
                    rig.server.set(*id, "pos", Value::Vec3([x, 0.0, 0.0]));
                }
                if mutate.chance(0.2) && rig.server.get(*id, "hp").is_some() {
                    let hp = (mutate.next() % 5) as i32;
                    rig.server.set(*id, "hp", Value::I32(hp));
                }
            }
            rig.tick();
        }
        // Quiet: the last changes must arrive despite the loss.
        for _ in 0..80 {
            rig.tick();
        }
        for p in [1, 2] {
            for id in &ids {
                for field in ["pos", "hp"] {
                    assert_eq!(
                        rig.client(p).value(*id, field),
                        rig.server.get(*id, field).cloned(),
                        "player {p} object {id} {field}"
                    );
                }
            }
        }
    }

    // M8-REP-06
    #[test]
    fn a_state_naming_an_unknown_object_is_dropped_and_never_acknowledged() {
        let mut server = ReplicaServer::new(schema());
        server.add_player(1);
        let id = server.spawn("Crate", "c", 0).unwrap();
        let msgs = server.step();
        let state = msgs
            .iter()
            .find(|m| m.channel == Channel::Unreliable)
            .expect("a state");
        let mut client = ReplicaClient::new(schema(), 1);
        // The state arrives before the spawn.
        client.handle(&state.bytes);
        assert_eq!(client.latest_tick(), None);
        assert_eq!(client.take_ack(), None);
        let spawn = msgs
            .iter()
            .find(|m| m.channel == Channel::Reliable)
            .expect("a spawn");
        client.handle(&spawn.bytes);
        client.handle(&state.bytes);
        assert_eq!(client.latest_tick(), Some(1));
        assert!(client.take_ack().is_some());
        assert_eq!(client.value(id, "pos"), Some(Value::Vec3([0.0; 3])));
    }

    // M8-REP-07
    #[test]
    fn interest_spawns_on_entering_and_despawns_on_leaving() {
        let mut rig = Rig::new(&[1], 0.0);
        let near = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = near.clone();
        rig.server
            .set_interest(move |_, _, _| flag.load(std::sync::atomic::Ordering::Relaxed));
        let id = rig.server.spawn("Crate", "c", 0).unwrap();
        rig.tick();
        assert!(rig.events.is_empty());
        near.store(true, std::sync::atomic::Ordering::Relaxed);
        rig.tick();
        assert!(matches!(
            rig.events.as_slice(),
            [(1, ReplicaEvent::Spawned { .. })]
        ));
        near.store(false, std::sync::atomic::Ordering::Relaxed);
        rig.tick();
        assert_eq!(
            rig.events.last(),
            Some(&(1, ReplicaEvent::Despawned { id }))
        );
        assert!(rig.client(1).ids().next().is_none());
    }

    // M8-REP-08
    #[test]
    fn the_owner_writes_owner_fields_and_nobody_else_or_other_fields() {
        let mut rig = Rig::new(&[1, 2], 0.0);
        let id = rig.server.spawn("Player", "p", 1).unwrap();
        rig.tick();
        // Player 1 owns it: `aim` is owner-written, `hp` is not.
        assert!(rig.client(1).set_owned(id, "aim", Value::F32(1.5)));
        assert!(!rig.client(1).set_owned(id, "hp", Value::I32(999)));
        assert!(!rig.client(2).set_owned(id, "aim", Value::F32(9.0)));
        let msg = rig.client(1).take_owner_update().expect("an update");
        rig.server.handle(1, &msg);
        assert_eq!(rig.server.get(id, "aim"), Some(&Value::F32(1.5)));
        // A forged message from player 2 for an object it does not own is ignored.
        let mut forged = Writer::new();
        forged
            .u8(kind::OWNER_FIELDS)
            .varint(1)
            .varint(id)
            .varint(0b100);
        Value::F32(66.0).write(&mut forged, &schema().classes[0].fields[2]);
        rig.server.handle(2, &forged.buf);
        assert_eq!(rig.server.get(id, "aim"), Some(&Value::F32(1.5)));
        // A forged write to a server-only field from the owner is ignored too.
        let mut hp = Writer::new();
        hp.u8(kind::OWNER_FIELDS).varint(1).varint(id).varint(0b010);
        Value::I32(5).write(&mut hp, &schema().classes[0].fields[1]);
        rig.server.handle(1, &hp.buf);
        assert_eq!(rig.server.get(id, "hp"), Some(&Value::I32(0)));
        // The client reads its own pending write at once, and the server's echo agrees.
        assert_eq!(rig.client(1).value(id, "aim"), Some(Value::F32(1.5)));
    }

    // M8-REP-09
    #[test]
    fn ownership_transfer_reaches_clients_that_know_the_object() {
        let mut rig = Rig::new(&[1, 2], 0.0);
        let id = rig.server.spawn("Player", "p", 1).unwrap();
        rig.tick();
        assert!(rig.client(1).is_mine(id) && !rig.client(2).is_mine(id));
        rig.server.set_owner(id, 2);
        rig.tick();
        assert!(!rig.client(1).is_mine(id) && rig.client(2).is_mine(id));
        assert!(
            rig.events
                .contains(&(2, ReplicaEvent::OwnerChanged { id, owner: 2 }))
        );
        assert!(rig.client(2).set_owned(id, "aim", Value::F32(2.0)));
        assert!(!rig.client(1).set_owned(id, "aim", Value::F32(3.0)));
    }

    // M8-REP-10
    #[test]
    fn big_states_split_into_datagram_sized_chunks_and_still_converge() {
        let mut rig = Rig::new(&[1], 0.1);
        let ids: Vec<NetId> = (0..400)
            .map(|_| rig.server.spawn("Crate", "c", 0).unwrap())
            .collect();
        for (i, id) in ids.iter().enumerate() {
            rig.server
                .set(*id, "pos", Value::Vec3([i as f32, 1.0, 2.0]));
        }
        for _ in 0..60 {
            rig.tick();
        }
        // Unreliable datagrams stay within the limit.
        let mut server = ReplicaServer::new(schema());
        server.add_player(1);
        for i in 0..400 {
            let id = server.spawn("Crate", "c", 0).unwrap();
            server.set(id, "pos", Value::Vec3([i as f32 * 3.7, 1.0, 2.0]));
        }
        let msgs = server.step();
        let states: Vec<_> = msgs
            .iter()
            .filter(|m| m.channel == Channel::Unreliable)
            .collect();
        assert!(states.len() > 1);
        assert!(
            states.iter().all(|m| m.bytes.len() <= 1100),
            "{:?}",
            states.iter().map(|m| m.bytes.len()).max()
        );
        for (i, id) in ids.iter().enumerate() {
            assert_eq!(
                rig.client(1).value(*id, "pos"),
                Some(Value::Vec3([i as f32, 1.0, 2.0]))
            );
        }
    }

    // M8-REP-11
    #[test]
    fn interpolation_blends_between_ticks_for_marked_fields_only() {
        let mut rig = Rig::new(&[1], 0.0);
        let p = rig.server.spawn("Player", "p", 2).unwrap();
        let c = rig.server.spawn("Crate", "c", 0).unwrap();
        for x in [0.0f32, 10.0, 20.0] {
            rig.server.set(p, "pos", Value::Vec3([x, 0.0, 0.0]));
            rig.server.set(c, "pos", Value::Vec3([x, 0.0, 0.0]));
            rig.tick();
        }
        let client = rig.client(1);
        assert_eq!(client.latest_tick(), Some(3));
        // Between ticks 1 and 2 the player (interpolated) is halfway; the crate steps.
        assert_eq!(
            client.interpolated(p, "pos", 1.5),
            Some(Value::Vec3([5.0, 0.0, 0.0]))
        );
        assert_eq!(
            client.interpolated(c, "pos", 1.5),
            Some(Value::Vec3([0.0, 0.0, 0.0]))
        );
        // Outside the stored range: the nearest state.
        assert_eq!(
            client.interpolated(p, "pos", 99.0),
            Some(Value::Vec3([20.0, 0.0, 0.0]))
        );
    }

    // M8-REP-12
    #[test]
    fn garbage_messages_never_panic_either_half() {
        let mut rig = Rig::new(&[1], 0.0);
        let id = rig.server.spawn("Player", "p", 1).unwrap();
        rig.tick();
        let mut rng = Lcg(5);
        for len in 0..200usize {
            let mut bytes: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
            if let Some(first) = bytes.first_mut() {
                *first = 0x10 + (rng.next() % 8) as u8;
            }
            rig.client(1).handle(&bytes);
            rig.server.handle(1, &bytes);
        }
        // Still working afterwards.
        rig.server.set(id, "hp", Value::I32(3));
        rig.tick();
        rig.tick();
        assert!(rig.client(1).value(id, "hp").is_some());
    }
}
