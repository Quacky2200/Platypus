// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! What replicates: classes of networked objects and their fields, and the values those hold.
//!
//! A [`Schema`] comes from the compiler (each `@sync` field of each networked class) and is the
//! same on every peer; its [`Schema::hash`] is what the session handshake compares. A [`Value`] is
//! one of a few plain types, written compactly: floats may be quantised (`quantize`), which also
//! decides when a change is big enough to send.

use crate::wire::{Reader, Writer};

/// The type of a replicated field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    Bool,
    I32,
    F32,
    Vec3,
    Quat,
    Str,
    /// A reference to another networked object (its id; 0 is none).
    Ref,
}

impl FieldType {
    pub fn name(self) -> &'static str {
        match self {
            FieldType::Bool => "bool",
            FieldType::I32 => "i32",
            FieldType::F32 => "f32",
            FieldType::Vec3 => "vec3",
            FieldType::Quat => "quat",
            FieldType::Str => "str",
            FieldType::Ref => "ref",
        }
    }

    pub fn parse(s: &str) -> Option<FieldType> {
        Some(match s {
            "bool" => FieldType::Bool,
            "i32" => FieldType::I32,
            "f32" => FieldType::F32,
            "vec3" => FieldType::Vec3,
            "quat" => FieldType::Quat,
            "str" => FieldType::Str,
            "ref" => FieldType::Ref,
            _ => return None,
        })
    }

    fn tag(self) -> u8 {
        match self {
            FieldType::Bool => 0,
            FieldType::I32 => 1,
            FieldType::F32 => 2,
            FieldType::Vec3 => 3,
            FieldType::Quat => 4,
            FieldType::Str => 5,
            FieldType::Ref => 6,
        }
    }

    fn from_tag(t: u8) -> Option<FieldType> {
        Some(match t {
            0 => FieldType::Bool,
            1 => FieldType::I32,
            2 => FieldType::F32,
            3 => FieldType::Vec3,
            4 => FieldType::Quat,
            5 => FieldType::Str,
            6 => FieldType::Ref,
            _ => return None,
        })
    }

    /// Whether neighbouring snapshots can be blended (`@sync({ interpolate: true })`).
    pub fn blendable(self) -> bool {
        matches!(self, FieldType::F32 | FieldType::Vec3 | FieldType::Quat)
    }
}

/// One replicated field of a class.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldDef {
    pub name: String,
    pub ty: FieldType,
    /// Floats are sent as whole multiples of this (0.01 keeps centimetres); `None`: exact `f32`.
    pub quantize: Option<f32>,
    /// The owning client writes it (and the server accepts that) instead of the server.
    pub owner_writes: bool,
    /// Clients blend it between snapshots.
    pub interpolate: bool,
}

impl FieldDef {
    pub fn new(name: &str, ty: FieldType) -> FieldDef {
        FieldDef {
            name: name.to_owned(),
            ty,
            quantize: None,
            owner_writes: false,
            interpolate: false,
        }
    }

    pub fn quantized(mut self, q: f32) -> FieldDef {
        self.quantize = Some(q);
        self
    }

    pub fn owner_writes(mut self) -> FieldDef {
        self.owner_writes = true;
        self
    }

    pub fn interpolated(mut self) -> FieldDef {
        self.interpolate = true;
        self
    }

    /// The value as the wire will carry it: quantised, so that what the server holds is what clients see.
    pub fn normalize(&self, v: Value) -> Value {
        let q = |x: f32| match self.quantize {
            Some(step) if step > 0.0 => (x / step).round() * step,
            _ => x,
        };
        match v {
            Value::F32(x) => Value::F32(q(x)),
            Value::Vec3([x, y, z]) => Value::Vec3([q(x), q(y), q(z)]),
            Value::Quat(r) if self.quantize.is_some() => Value::Quat(unpack_quat(pack_quat(&r))),
            other => other,
        }
    }

    /// The type's zero value.
    pub fn default_value(&self) -> Value {
        match self.ty {
            FieldType::Bool => Value::Bool(false),
            FieldType::I32 => Value::I32(0),
            FieldType::F32 => Value::F32(0.0),
            FieldType::Vec3 => Value::Vec3([0.0; 3]),
            FieldType::Quat => Value::Quat([0.0, 0.0, 0.0, 1.0]),
            FieldType::Str => Value::Str(String::new()),
            FieldType::Ref => Value::Ref(0),
        }
    }
}

/// A networked class: a name and its fields, in declaration order (the order is the wire order).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClassDef {
    pub name: String,
    pub fields: Vec<FieldDef>,
}

impl ClassDef {
    pub fn new(name: &str) -> ClassDef {
        ClassDef {
            name: name.to_owned(),
            fields: Vec::new(),
        }
    }

    pub fn field(mut self, f: FieldDef) -> ClassDef {
        self.fields.push(f);
        self
    }

    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
}

/// Every networked class of a game. At most 64 fields a class (a change mask is one number).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub classes: Vec<ClassDef>,
    pub rpcs: Vec<RpcDef>,
}

/// Who an RPC goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcTarget {
    /// Client to server.
    Server,
    /// Server to the object's owner.
    Owner,
    /// Server to every client that knows the object.
    All,
}

impl RpcTarget {
    pub fn name(self) -> &'static str {
        match self {
            RpcTarget::Server => "server",
            RpcTarget::Owner => "owner",
            RpcTarget::All => "all",
        }
    }

    pub fn parse(s: &str) -> Option<RpcTarget> {
        Some(match s {
            "server" => RpcTarget::Server,
            "owner" => RpcTarget::Owner,
            "all" => RpcTarget::All,
            _ => return None,
        })
    }
}

/// A remote procedure: a name, where it runs and (for client-to-server calls) who may call it.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcDef {
    pub name: String,
    pub target: RpcTarget,
    /// For `Server` RPCs: only the object's owner may call it (the default).
    pub require_owner: bool,
    /// Sent on the reliable channel (the default) or unreliably.
    pub reliable: bool,
}

impl RpcDef {
    pub fn new(name: &str, target: RpcTarget) -> RpcDef {
        RpcDef {
            name: name.to_owned(),
            target,
            require_owner: true,
            reliable: true,
        }
    }

    /// Anyone may call this server RPC, not only the owner.
    pub fn from_anyone(mut self) -> RpcDef {
        self.require_owner = false;
        self
    }

    pub fn unreliable(mut self) -> RpcDef {
        self.reliable = false;
        self
    }
}

impl Schema {
    pub fn new(classes: Vec<ClassDef>) -> Schema {
        Schema {
            classes,
            rpcs: Vec::new(),
        }
    }

    pub fn with_rpcs(mut self, rpcs: Vec<RpcDef>) -> Schema {
        self.rpcs = rpcs;
        self
    }

    pub fn rpc_id(&self, name: &str) -> Option<u16> {
        self.rpcs
            .iter()
            .position(|r| r.name == name)
            .map(|i| i as u16)
    }

    pub fn rpc(&self, id: u16) -> Option<&RpcDef> {
        self.rpcs.get(usize::from(id))
    }

    pub fn class_id(&self, name: &str) -> Option<u16> {
        self.classes
            .iter()
            .position(|c| c.name == name)
            .map(|i| i as u16)
    }

    pub fn class(&self, id: u16) -> Option<&ClassDef> {
        self.classes.get(usize::from(id))
    }

    /// A stable 64-bit digest of everything that changes the wire format (FNV-1a over a canonical text).
    pub fn hash(&self) -> u64 {
        let mut text = String::new();
        for c in &self.classes {
            text.push_str(&c.name);
            text.push('{');
            for f in &c.fields {
                text.push_str(&format!(
                    "{}:{}:{}:{}:{};",
                    f.name,
                    f.ty.name(),
                    f.quantize
                        .map_or(String::from("-"), |q| q.to_bits().to_string()),
                    u8::from(f.owner_writes),
                    u8::from(f.interpolate),
                ));
            }
            text.push('}');
        }
        for r in &self.rpcs {
            text.push_str(&format!(
                "rpc {}:{}:{}:{};",
                r.name,
                r.target.name(),
                u8::from(r.require_owner),
                u8::from(r.reliable)
            ));
        }
        text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        })
    }

    /// Problems that make a schema unusable (the compiler reports them; the runtime refuses it).
    pub fn check(&self) -> Result<(), String> {
        for (i, c) in self.classes.iter().enumerate() {
            if c.fields.len() > 64 {
                return Err(format!(
                    "class {} has {} synced fields (at most 64)",
                    c.name,
                    c.fields.len()
                ));
            }
            if self.classes[..i].iter().any(|o| o.name == c.name) {
                return Err(format!("class {} is declared twice", c.name));
            }
            for (j, f) in c.fields.iter().enumerate() {
                if c.fields[..j].iter().any(|o| o.name == f.name) {
                    return Err(format!("{}.{} is declared twice", c.name, f.name));
                }
            }
        }
        Ok(())
    }
}

/// A replicated value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    I32(i32),
    F32(f32),
    Vec3([f32; 3]),
    Quat([f32; 4]),
    Str(String),
    Ref(u64),
}

impl Value {
    pub fn ty(&self) -> FieldType {
        match self {
            Value::Bool(_) => FieldType::Bool,
            Value::I32(_) => FieldType::I32,
            Value::F32(_) => FieldType::F32,
            Value::Vec3(_) => FieldType::Vec3,
            Value::Quat(_) => FieldType::Quat,
            Value::Str(_) => FieldType::Str,
            Value::Ref(_) => FieldType::Ref,
        }
    }

    /// Writes the value as `field` says (quantised floats are whole steps, as zigzag varints).
    pub fn write(&self, w: &mut Writer, field: &FieldDef) {
        let float = |w: &mut Writer, x: f32| match field.quantize {
            Some(step) if step > 0.0 => {
                let n = (x / step).round() as i64;
                w.varint(zigzag(n));
            }
            _ => {
                w.u32(x.to_bits());
            }
        };
        match self {
            Value::Bool(b) => {
                w.u8(u8::from(*b));
            }
            Value::I32(n) => {
                w.varint(zigzag(i64::from(*n)));
            }
            Value::F32(x) => float(w, *x),
            Value::Vec3(v) => v.iter().for_each(|x| float(w, *x)),
            // A quantised rotation is one u32 in smallest-three form; otherwise it is exact.
            Value::Quat(q) if field.quantize.is_some() => {
                w.u32(pack_quat(q));
            }
            Value::Quat(q) => q.iter().for_each(|x| {
                w.u32(x.to_bits());
            }),
            Value::Str(s) => {
                w.str(s);
            }
            Value::Ref(r) => {
                w.varint(*r);
            }
        }
    }

    /// Reads a value of `field`'s type; `None` on truncated or invalid data.
    pub fn read(r: &mut Reader<'_>, field: &FieldDef) -> Option<Value> {
        let float = |r: &mut Reader<'_>| -> Option<f32> {
            match field.quantize {
                Some(step) if step > 0.0 => Some(unzigzag(r.varint()?) as f32 * step),
                _ => Some(f32::from_bits(r.u32()?)),
            }
        };
        Some(match field.ty {
            FieldType::Bool => Value::Bool(match r.u8()? {
                0 => false,
                1 => true,
                _ => return None,
            }),
            FieldType::I32 => Value::I32(i32::try_from(unzigzag(r.varint()?)).ok()?),
            FieldType::F32 => Value::F32(float(r)?),
            FieldType::Vec3 => Value::Vec3([float(r)?, float(r)?, float(r)?]),
            FieldType::Quat if field.quantize.is_some() => Value::Quat(unpack_quat(r.u32()?)),
            FieldType::Quat => Value::Quat([
                f32::from_bits(r.u32()?),
                f32::from_bits(r.u32()?),
                f32::from_bits(r.u32()?),
                f32::from_bits(r.u32()?),
            ]),
            FieldType::Str => Value::Str(r.str(1024)?),
            FieldType::Ref => Value::Ref(r.varint()?),
        })
    }

    /// Writes the value with its type in front (RPC arguments, which have no field to say).
    pub fn write_tagged(&self, w: &mut Writer) {
        let tag = self.ty().tag();
        w.u8(tag);
        self.write(w, &FieldDef::new("", self.ty()));
    }

    /// Reads a value written by `write_tagged`.
    pub fn read_tagged(r: &mut Reader<'_>) -> Option<Value> {
        let ty = FieldType::from_tag(r.u8()?)?;
        Value::read(r, &FieldDef::new("", ty))
    }

    /// Blends two values (`t` 0 to 1) for interpolation; types that cannot blend take `self` until `t` passes a half.
    pub fn lerp(&self, other: &Value, t: f32) -> Value {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        match (self, other) {
            (Value::F32(a), Value::F32(b)) => Value::F32(mix(*a, *b)),
            (Value::Vec3(a), Value::Vec3(b)) => {
                Value::Vec3([mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])])
            }
            (Value::Quat(a), Value::Quat(b)) => {
                // Normalised lerp along the shorter arc.
                let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
                let s = if dot < 0.0 { -1.0 } else { 1.0 };
                let mut q = [0.0; 4];
                for i in 0..4 {
                    q[i] = mix(a[i], b[i] * s);
                }
                let len = q.iter().map(|x| x * x).sum::<f32>().sqrt();
                if len > 1e-6 {
                    q.iter_mut().for_each(|x| *x /= len);
                }
                Value::Quat(q)
            }
            _ => {
                if t < 0.5 {
                    self.clone()
                } else {
                    other.clone()
                }
            }
        }
    }
}

fn zigzag(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

fn unzigzag(n: u64) -> i64 {
    ((n >> 1) as i64) ^ -((n & 1) as i64)
}

/// The two parts of a unit quaternion other than its largest are at most 1/sqrt(2) in size.
const QUAT_RANGE: f32 = std::f32::consts::FRAC_1_SQRT_2;
const QUAT_BITS: u32 = 10;
const QUAT_MAX: f32 = ((1 << QUAT_BITS) - 1) as f32;

/// Smallest-three form: 2 bits say which part is the largest, then the other three at 10 bits
/// each. The largest is rebuilt from them (its sign is made positive first, since `q` and `-q` are
/// the same rotation). Four bytes instead of sixteen, within about a sixth of a degree.
fn pack_quat(q: &[f32; 4]) -> u32 {
    let len = q.iter().map(|x| x * x).sum::<f32>().sqrt();
    let unit = if len > 1e-6 {
        q.map(|x| x / len)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    };
    // The first part within a hair of the biggest: a near tie must pick the same part again after
    // the value has been through here once, so that packing what was unpacked changes nothing.
    let biggest = unit.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    let largest = unit
        .iter()
        .position(|x| x.abs() >= biggest - 0.004)
        .unwrap_or(0);
    let sign = if unit[largest] < 0.0 { -1.0 } else { 1.0 };
    let mut bits = (largest as u32) << (3 * QUAT_BITS);
    let mut shift = 3 * QUAT_BITS;
    for (i, x) in unit.iter().enumerate() {
        if i == largest {
            continue;
        }
        shift -= QUAT_BITS;
        let n = (((x * sign / QUAT_RANGE).clamp(-1.0, 1.0) * 0.5 + 0.5) * QUAT_MAX).round() as u32;
        bits |= n << shift;
    }
    bits
}

fn unpack_quat(bits: u32) -> [f32; 4] {
    let largest = (bits >> (3 * QUAT_BITS)) as usize & 3;
    let mut q = [0.0f32; 4];
    let mut shift = 3 * QUAT_BITS;
    let mut sum = 0.0;
    for (i, part) in q.iter_mut().enumerate() {
        if i == largest {
            continue;
        }
        shift -= QUAT_BITS;
        let n = (bits >> shift) & ((1 << QUAT_BITS) - 1);
        *part = ((n as f32 / QUAT_MAX) * 2.0 - 1.0) * QUAT_RANGE;
        sum += *part * *part;
    }
    q[largest] = (1.0 - sum).max(0.0).sqrt();
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(field: &FieldDef, v: Value) -> Value {
        let mut w = Writer::new();
        v.write(&mut w, field);
        let bytes = w.finish();
        let mut r = Reader::new(&bytes);
        let out = Value::read(&mut r, field).expect("reads");
        assert!(r.is_empty());
        out
    }

    // M8-REP-01
    #[test]
    fn every_type_round_trips_and_quantised_floats_keep_whole_steps() {
        let plain = |ty| FieldDef::new("f", ty);
        for (ty, v) in [
            (FieldType::Bool, Value::Bool(true)),
            (FieldType::I32, Value::I32(-123_456)),
            (FieldType::F32, Value::F32(3.25)),
            (FieldType::Vec3, Value::Vec3([1.5, -2.0, 1e9])),
            (FieldType::Quat, Value::Quat([0.0, 0.6, 0.0, 0.8])),
            (FieldType::Str, Value::Str("héllo".into())),
            (FieldType::Ref, Value::Ref(u64::MAX)),
        ] {
            assert_eq!(round_trip(&plain(ty), v.clone()), v);
        }
        let q = FieldDef::new("p", FieldType::Vec3).quantized(0.01);
        let v = q.normalize(Value::Vec3([1.23456, -0.004, 99.999]));
        assert_eq!(round_trip(&q, v.clone()), v);
        let Value::Vec3(n) = v else { panic!() };
        assert!((n[0] - 1.23).abs() < 1e-5 && n[1].abs() < 1e-5 && (n[2] - 100.0).abs() < 1e-3);
    }

    // M8-REP-13
    #[test]
    fn a_quantised_rotation_is_four_bytes_within_a_fifth_of_a_degree() {
        let field = FieldDef::new("r", FieldType::Quat).quantized(0.001);
        for raw in [
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 0.6, 0.0, 0.8],
            [0.5, -0.5, 0.5, -0.5],
            [0.1, 0.2, -0.3, -0.9],
            [-0.7, 0.1, 0.1, 0.7],
        ] {
            let len = raw.iter().map(|x: &f32| x * x).sum::<f32>().sqrt();
            let q = raw.map(|x| x / len);
            let v = field.normalize(Value::Quat(q));
            let mut w = Writer::new();
            v.write(&mut w, &field);
            let bytes = w.finish();
            assert_eq!(bytes.len(), 4);
            // What was written is what the server holds, so both sides agree exactly.
            assert_eq!(round_trip(&field, v.clone()), v);
            let Value::Quat(n) = v else { panic!() };
            let dot: f32 = n.iter().zip(&q).map(|(a, b)| a * b).sum();
            // q and -q are one rotation: the angle is 2 * acos(|dot|).
            let degrees = 2.0 * dot.abs().min(1.0).acos().to_degrees();
            assert!(degrees < 0.2, "{raw:?} off by {degrees} degrees");
        }
    }

    // M8-REP-01
    #[test]
    fn quantising_makes_small_changes_equal_and_shrinks_the_wire() {
        let q = FieldDef::new("x", FieldType::F32).quantized(0.01);
        assert_eq!(
            q.normalize(Value::F32(1.2300)),
            q.normalize(Value::F32(1.2349))
        );
        assert_ne!(q.normalize(Value::F32(1.23)), q.normalize(Value::F32(1.25)));
        let mut exact = Writer::new();
        Value::F32(123.45).write(&mut exact, &FieldDef::new("x", FieldType::F32));
        let mut small = Writer::new();
        q.normalize(Value::F32(123.45)).write(&mut small, &q);
        assert!(small.buf.len() < exact.buf.len());
    }

    // M8-REP-01
    #[test]
    fn bad_values_read_as_none() {
        let f = FieldDef::new("b", FieldType::Bool);
        assert_eq!(Value::read(&mut Reader::new(&[2]), &f), None);
        assert_eq!(
            Value::read(&mut Reader::new(&[]), &FieldDef::new("n", FieldType::I32)),
            None
        );
        // An i32 field holding a number past i32.
        let mut w = Writer::new();
        w.varint(zigzag(i64::from(i32::MAX) + 1));
        assert_eq!(
            Value::read(
                &mut Reader::new(&w.buf),
                &FieldDef::new("n", FieldType::I32)
            ),
            None
        );
    }

    // M8-REP-02
    #[test]
    fn the_schema_hash_follows_everything_that_changes_the_wire() {
        let base = || {
            Schema::new(vec![
                ClassDef::new("Player")
                    .field(FieldDef::new("pos", FieldType::Vec3).quantized(0.01))
                    .field(FieldDef::new("hp", FieldType::I32)),
            ])
        };
        assert_eq!(base().hash(), base().hash());
        let mut renamed = base();
        renamed.classes[0].fields[1].name = "health".into();
        let mut retyped = base();
        retyped.classes[0].fields[1].ty = FieldType::F32;
        let mut quant = base();
        quant.classes[0].fields[0].quantize = Some(0.1);
        let mut owner = base();
        owner.classes[0].fields[1].owner_writes = true;
        let mut reordered = base();
        reordered.classes[0].fields.reverse();
        for other in [renamed, retyped, quant, owner, reordered] {
            assert_ne!(base().hash(), other.hash());
        }
    }

    // M8-REP-02
    #[test]
    fn check_refuses_duplicates_and_too_many_fields() {
        let mut s = Schema::new(vec![ClassDef::new("A"), ClassDef::new("A")]);
        assert!(s.check().is_err());
        s = Schema::new(vec![
            ClassDef::new("A")
                .field(FieldDef::new("x", FieldType::Bool))
                .field(FieldDef::new("x", FieldType::Bool)),
        ]);
        assert!(s.check().is_err());
        let mut big = ClassDef::new("Big");
        for i in 0..65 {
            big = big.field(FieldDef::new(&format!("f{i}"), FieldType::Bool));
        }
        assert!(Schema::new(vec![big]).check().is_err());
        assert!(
            Schema::new(vec![
                ClassDef::new("Ok").field(FieldDef::new("x", FieldType::Bool))
            ])
            .check()
            .is_ok()
        );
    }

    // M8-REP-03
    #[test]
    fn lerp_blends_floats_vectors_and_rotations_and_steps_the_rest() {
        assert_eq!(
            Value::F32(0.0).lerp(&Value::F32(10.0), 0.25),
            Value::F32(2.5)
        );
        assert_eq!(
            Value::Vec3([0.0; 3]).lerp(&Value::Vec3([2.0, 4.0, 6.0]), 0.5),
            Value::Vec3([1.0, 2.0, 3.0])
        );
        let a = Value::Quat([0.0, 0.0, 0.0, 1.0]);
        let b = Value::Quat([0.0, 1.0, 0.0, 0.0]);
        let Value::Quat(mid) = a.lerp(&b, 0.5) else {
            panic!()
        };
        let len: f32 = mid.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((len - 1.0).abs() < 1e-5 && (mid[1] - mid[3]).abs() < 1e-5);
        assert_eq!(Value::I32(1).lerp(&Value::I32(9), 0.4), Value::I32(1));
        assert_eq!(Value::I32(1).lerp(&Value::I32(9), 0.6), Value::I32(9));
    }
}
