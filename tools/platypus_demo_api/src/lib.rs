// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! A small engine API written with `#[ts_api]`, to prove the whole path: Rust functions, the
//! generated manifest, and TypeScript compiled by `web_ui_ts` calling the generated `extern "C"`
//! shims directly. It keeps a tiny "world" of entities behind handles, like the real engine will.

use std::cell::RefCell;
use ts_api::{Handle, ValueType, ts_api};

/// An entity id: slot index in the low 32 bits, generation in the high 32 (so a stale id misses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityId(pub u64);

impl Handle for EntityId {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        EntityId(raw)
    }
}

/// Three 32-bit floats: what scripts see as `Vector3`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl ValueType<3> for Vec3 {
    fn from_parts(p: [f64; 3]) -> Self {
        Vec3 {
            x: p[0] as f32,
            y: p[1] as f32,
            z: p[2] as f32,
        }
    }
    fn to_parts(self) -> [f64; 3] {
        [self.x as f64, self.y as f64, self.z as f64]
    }
}

#[derive(Clone, Copy)]
struct Slot {
    pos: Vec3,
    generation: u32,
    alive: bool,
    kind: u32,
    hp: f64,
}

thread_local! {
    static WORLD: RefCell<Vec<Slot>> = const { RefCell::new(Vec::new()) };
}

fn id(index: usize, generation: u32) -> EntityId {
    EntityId(((generation as u64) << 32) | index as u64)
}

fn with_slot<R>(e: EntityId, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
    WORLD.with(|w| {
        let mut w = w.borrow_mut();
        let slot = w.get_mut((e.0 & 0xFFFF_FFFF) as usize)?;
        (slot.alive && slot.generation == (e.0 >> 32) as u32).then(|| f(slot))
    })
}

#[ts_api(namespace = "Demo", handles(Entity = EntityId), values(Vector3 = Vec3 [x: f32, y: f32, z: f32]))]
pub mod demo {
    use super::{EntityId, Slot, Vec3, WORLD, id, with_slot};

    /// `x * k`.
    pub fn scale(x: f64, k: f64) -> f64 {
        x * k
    }

    /// Clamps to `lo..=hi`.
    pub fn clamp_i(v: i32, lo: i32, hi: i32) -> i32 {
        v.clamp(lo, hi)
    }

    /// Half of `v`, computed in 32-bit floating point.
    pub fn half(v: f32) -> f32 {
        v * 0.5
    }

    pub fn is_even(v: u32) -> bool {
        v.is_multiple_of(2)
    }

    /// Removes every entity.
    pub fn reset() {
        WORLD.with(|w| w.borrow_mut().clear());
    }

    /// A new entity of a kind, with 100 health.
    pub fn spawn(kind: u32) -> EntityId {
        WORLD.with(|w| {
            let mut w = w.borrow_mut();
            if let Some(i) = w.iter().position(|s| !s.alive) {
                w[i].generation += 1;
                w[i] = Slot {
                    pos: Vec3::default(),
                    generation: w[i].generation,
                    alive: true,
                    kind,
                    hp: 100.0,
                };
                id(i, w[i].generation)
            } else {
                // Generations start at 1: a handle of 0 means "no object", so no live id is 0.
                w.push(Slot {
                    pos: Vec3::default(),
                    generation: 1,
                    alive: true,
                    kind,
                    hp: 100.0,
                });
                id(w.len() - 1, 1)
            }
        })
    }

    /// Destroys an entity; its id stops working.
    pub fn destroy(e: EntityId) {
        with_slot(e, |s| s.alive = false);
    }

    pub fn alive(e: EntityId) -> bool {
        with_slot(e, |_| ()).is_some()
    }

    /// The kind an entity was spawned with, or 0 for a stale id.
    pub fn kind_of(e: EntityId) -> u32 {
        with_slot(e, |s| s.kind).unwrap_or(0)
    }

    pub fn set_hp(e: EntityId, hp: f64) {
        with_slot(e, |s| s.hp = hp);
    }

    /// Health, or -1 for a stale id.
    pub fn hp(e: EntityId) -> f64 {
        with_slot(e, |s| s.hp).unwrap_or(-1.0)
    }

    /// The length of a vector, in 32-bit floating point.
    pub fn length(v: Vec3) -> f32 {
        (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
    }

    /// `v * k`.
    pub fn scaled(v: Vec3, k: f32) -> Vec3 {
        Vec3 {
            x: v.x * k,
            y: v.y * k,
            z: v.z * k,
        }
    }

    pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
        Vec3 {
            x: a.y * b.z - a.z * b.y,
            y: a.z * b.x - a.x * b.z,
            z: a.x * b.y - a.y * b.x,
        }
    }

    pub fn set_position(e: EntityId, p: Vec3) {
        with_slot(e, |s| s.pos = p);
    }

    /// Where an entity is (the origin for a stale id).
    pub fn position(e: EntityId) -> Vec3 {
        with_slot(e, |s| s.pos).unwrap_or_default()
    }

    /// How many entities are alive.
    pub fn count() -> u32 {
        WORLD.with(|w| w.borrow().iter().filter(|s| s.alive).count() as u32)
    }
}

/// The manifest as JSON, for the compiler.
pub fn manifest_json() -> String {
    demo::MANIFEST.to_json()
}

/// The TypeScript declarations.
pub fn declarations() -> String {
    demo::MANIFEST.to_dts()
}

/// Keeps the generated shims linked into an executable that only reaches them from compiled
/// script code.
pub fn link_anchor() -> usize {
    let fns: [usize; 17] = [
        demo::platypus_Demo_scale as *const () as usize,
        demo::platypus_Demo_clampI as *const () as usize,
        demo::platypus_Demo_half as *const () as usize,
        demo::platypus_Demo_isEven as *const () as usize,
        demo::platypus_Demo_reset as *const () as usize,
        demo::platypus_Demo_spawn as *const () as usize,
        demo::platypus_Demo_destroy as *const () as usize,
        demo::platypus_Demo_alive as *const () as usize,
        demo::platypus_Demo_kindOf as *const () as usize,
        demo::platypus_Demo_setHp as *const () as usize,
        demo::platypus_Demo_hp as *const () as usize,
        demo::platypus_Demo_count as *const () as usize,
        demo::platypus_Demo_length as *const () as usize,
        demo::platypus_Demo_scaled as *const () as usize,
        demo::platypus_Demo_cross as *const () as usize,
        demo::platypus_Demo_setPosition as *const () as usize,
        demo::platypus_Demo_position as *const () as usize,
    ];
    fns.iter().sum()
}
