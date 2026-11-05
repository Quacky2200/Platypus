// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `Rng` (a seedable generator, C#'s `System.Random` shaped) and Unity's static `Random`. The
//! generator is mulberry32, the one `js/engine/src/random.ts` uses, so a seed gives the same
//! numbers in both. Not for security.
//!
//! The vector-valued members (`insideUnitCircle`, `onUnitSphere`, `insideUnitSphere`, `rotation`)
//! draw their numbers in the order `random.ts` does.

use platypus_math::{Q, V2, V3};
use std::cell::RefCell;
use ts_api::{Handle, ts_api};

/// mulberry32: 32 bits of state.
#[derive(Clone, Copy, Debug)]
pub struct Mulberry(pub u32);

impl Mulberry {
    /// A seed from the clock and a counter, for generators made without one.
    pub fn from_clock() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static COUNT: AtomicU32 = AtomicU32::new(0x9e37_79b9);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u32);
        let n = COUNT.fetch_add(0x6d2b_79f5, Ordering::Relaxed);
        Mulberry(nanos ^ n.rotate_left(13))
    }

    /// 0 (inclusive) to 1 (exclusive).
    pub fn next_double(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut t = self.0;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }

    /// A point inside a circle of radius 1.
    pub fn inside_unit_circle(&mut self) -> V2 {
        let angle = self.next_double() * std::f64::consts::PI * 2.0;
        let r = self.next_double().sqrt();
        V2 {
            x: angle.cos() * r,
            y: angle.sin() * r,
        }
    }

    /// A point on a sphere of radius 1.
    pub fn on_unit_sphere(&mut self) -> V3 {
        let z = self.next_double() * 2.0 - 1.0;
        let angle = self.next_double() * std::f64::consts::PI * 2.0;
        let r = (1.0 - z * z).sqrt();
        V3 {
            x: angle.cos() * r,
            y: angle.sin() * r,
            z,
        }
    }

    /// A point inside a sphere of radius 1.
    pub fn inside_unit_sphere(&mut self) -> V3 {
        let p = self.on_unit_sphere();
        p.scale(self.next_double().cbrt())
    }

    /// A uniformly random rotation.
    pub fn rotation(&mut self) -> Q {
        let (u1, u2, u3) = (self.next_double(), self.next_double(), self.next_double());
        let a = (1.0 - u1).sqrt();
        let b = u1.sqrt();
        let tau = 2.0 * std::f64::consts::PI;
        Q {
            x: a * (tau * u2).sin(),
            y: a * (tau * u2).cos(),
            z: b * (tau * u3).sin(),
            w: b * (tau * u3).cos(),
        }
    }

    /// An integer from `lo` (inclusive) to `hi` (exclusive); `lo` when the range is empty.
    pub fn range_int(&mut self, lo: f64, hi: f64) -> f64 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_double() * (hi - lo)).floor()
    }
}

thread_local! {
    /// The generators scripts made (`new Rng(seed)`): a handle is the index plus one. They live
    /// for the thread, like the scene.
    static RNGS: RefCell<Vec<Mulberry>> = const { RefCell::new(Vec::new()) };
    static SHARED: RefCell<Mulberry> = RefCell::new(Mulberry::from_clock());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RngRef(pub u64);

impl Handle for RngRef {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        RngRef(raw)
    }
}

fn with_rng<R>(r: RngRef, f: impl FnOnce(&mut Mulberry) -> R) -> R {
    RNGS.with(|v| {
        let mut v = v.borrow_mut();
        let i = (r.0 as usize).wrapping_sub(1);
        match v.get_mut(i) {
            Some(g) => f(g),
            // A handle that is not one of ours: a throwaway generator, not a crash.
            None => f(&mut Mulberry(0)),
        }
    })
}

/// The seed a script passes: NaN (left out) means the clock; others wrap to 32 bits like `>>> 0`.
fn seed_of(seed: f64) -> Mulberry {
    if seed.is_nan() {
        Mulberry::from_clock()
    } else {
        Mulberry((seed.trunc() as i64) as u32)
    }
}

#[ts_api(namespace = "Rng", handles(Rng = RngRef), values(
    Vector2 = V2 [x: f64, y: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64],
    Quaternion = Q [x: f64, y: f64, z: f64, w: f64]))]
pub mod rng_api {
    use super::{Mulberry, Q, RNGS, RngRef, V2, V3, seed_of, with_rng};

    impl RngRef {
        /// A generator; the same seed gives the same sequence. Seeded from the clock without one.
        #[optional(seed)]
        pub fn create(seed: f64) -> RngRef {
            RNGS.with(|v| {
                let mut v = v.borrow_mut();
                v.push(seed_of(seed));
                RngRef(v.len() as u64)
            })
        }

        /// 0 (inclusive) to 1 (exclusive).
        pub fn next_double(self) -> f64 {
            with_rng(self, Mulberry::next_double)
        }

        /// C#'s `Next`: an integer from `min` (inclusive) to `max` (exclusive); with one argument,
        /// 0 to `max`; with none, 0 to 2^31-1.
        #[optional(min_or_max, max)]
        pub fn next(self, min_or_max: f64, max: f64) -> f64 {
            let (lo, hi) = if max.is_nan() {
                (
                    0.0,
                    if min_or_max.is_nan() {
                        2_147_483_647.0
                    } else {
                        min_or_max
                    },
                )
            } else {
                (if min_or_max.is_nan() { 0.0 } else { min_or_max }, max)
            };
            with_rng(self, |g| g.range_int(lo, hi))
        }

        /// A float from `min` (inclusive) to `max` (exclusive), like Unity's float `Random.Range`.
        pub fn float(self, min: f64, max: f64) -> f64 {
            min + with_rng(self, Mulberry::next_double) * (max - min)
        }

        /// An integer from `min` (inclusive) to `max` (exclusive), like Unity's int `Random.Range`.
        pub fn int(self, min: f64, max: f64) -> f64 {
            with_rng(self, |g| g.range_int(min.ceil(), max.floor()))
        }

        /// A point inside a circle of radius 1.
        pub fn inside_unit_circle(self) -> V2 {
            with_rng(self, Mulberry::inside_unit_circle)
        }

        /// A point on a sphere of radius 1.
        pub fn on_unit_sphere(self) -> V3 {
            with_rng(self, Mulberry::on_unit_sphere)
        }

        /// A point inside a sphere of radius 1.
        pub fn inside_unit_sphere(self) -> V3 {
            with_rng(self, Mulberry::inside_unit_sphere)
        }

        /// A uniformly random rotation.
        pub fn rotation(self) -> Q {
            with_rng(self, Mulberry::rotation)
        }

        /// True with the given chance (0.5 without one).
        #[optional(chance)]
        pub fn bool(self, chance: f64) -> bool {
            let chance = if chance.is_nan() { 0.5 } else { chance };
            with_rng(self, Mulberry::next_double) < chance
        }
    }
}

#[ts_api(namespace = "Random", values(
    Vector2 = V2 [x: f64, y: f64],
    Vector3 = V3 [x: f64, y: f64, z: f64],
    Quaternion = Q [x: f64, y: f64, z: f64, w: f64]))]
pub mod random_api {
    use super::{Q, SHARED, V2, V3};

    /// Reseeds the shared generator, for repeatable runs.
    pub fn init_state(seed: f64) {
        SHARED.with(|g| *g.borrow_mut() = super::seed_of(seed));
    }

    /// 0 to 1.
    #[property]
    pub fn value() -> f64 {
        SHARED.with(|g| g.borrow_mut().next_double())
    }

    /// A float from `min` to `max`. For integers use `rangeInt` (max exclusive).
    pub fn range(min: f64, max: f64) -> f64 {
        min + SHARED.with(|g| g.borrow_mut().next_double()) * (max - min)
    }

    pub fn range_int(min: f64, max: f64) -> f64 {
        SHARED.with(|g| g.borrow_mut().range_int(min.ceil(), max.floor()))
    }

    #[optional(chance)]
    pub fn bool(chance: f64) -> bool {
        let chance = if chance.is_nan() { 0.5 } else { chance };
        SHARED.with(|g| g.borrow_mut().next_double()) < chance
    }

    #[property]
    pub fn inside_unit_circle() -> V2 {
        SHARED.with(|g| g.borrow_mut().inside_unit_circle())
    }

    #[property]
    pub fn inside_unit_sphere() -> V3 {
        SHARED.with(|g| g.borrow_mut().inside_unit_sphere())
    }

    #[property]
    pub fn on_unit_sphere() -> V3 {
        SHARED.with(|g| g.borrow_mut().on_unit_sphere())
    }

    #[property]
    pub fn rotation() -> Q {
        SHARED.with(|g| g.borrow_mut().rotation())
    }
}
