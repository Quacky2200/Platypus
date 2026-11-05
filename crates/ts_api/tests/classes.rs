// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Handle classes in `#[ts_api]`: methods, properties and statics declared in ordinary `impl`
//! blocks of the handle's Rust type. IDs are in TESTS.md (M9-API-*).
use std::cell::RefCell;
use ts_api::{FnKind, Handle, Ty, ts_api};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThingId(pub u64);
impl Handle for ThingId {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        ThingId(raw)
    }
}

/// A second handle class over the same objects (like `Transform` of a `GameObject`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartId(pub u64);
impl Handle for PartId {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        PartId(raw)
    }
}

thread_local! {
    static HP: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

#[ts_api(namespace = "Things", handles(Thing = ThingId, Part = PartId))]
pub mod things {
    use super::{HP, PartId, ThingId};

    /// Makes a thing (a static function of the class).
    impl ThingId {
        pub fn create(health: f64) -> ThingId {
            HP.with(|h| {
                let mut h = h.borrow_mut();
                h.push(health);
                ThingId(h.len() as u64)
            })
        }

        /// Health.
        #[property]
        pub fn health(self) -> f64 {
            HP.with(|h| h.borrow()[self.0 as usize - 1])
        }

        pub fn set_health(self, value: f64) {
            HP.with(|h| h.borrow_mut()[self.0 as usize - 1] = value);
        }

        /// Hurts the thing and says whether it is still alive.
        pub fn damage(self, amount: f64) -> bool {
            let left = self.health() - amount;
            self.set_health(left);
            left > 0.0
        }

        /// The thing's part.
        #[property]
        pub fn part(self) -> PartId {
            PartId(self.0)
        }
    }

    impl PartId {
        #[property]
        pub fn weight(self) -> f64 {
            ThingId(self.0).health() * 2.0
        }

        pub fn owner(self) -> ThingId {
            ThingId(self.0)
        }
    }

    /// A plain namespace function still works next to the classes.
    pub fn count() -> u32 {
        HP.with(|h| h.borrow().len() as u32)
    }
}

// M9-API-16
#[test]
fn class_functions_are_listed_per_class_with_a_receiver_first() {
    let m = things::MANIFEST;
    assert_eq!(m.functions.len(), 1); // Things.count
    assert_eq!(m.classes.len(), 2);
    let thing = m.classes[0];
    assert_eq!(thing.name, "Thing");
    let by_name = |n: &str, k: FnKind| {
        thing
            .functions
            .iter()
            .find(|f| f.name == n && f.kind == k)
            .unwrap_or_else(|| panic!("{n} {k:?}"))
    };
    let create = by_name("create", FnKind::Static);
    assert_eq!(create.symbol, "platypus_Thing_create");
    assert_eq!(create.params.len(), 1);
    let get = by_name("health", FnKind::Get);
    assert_eq!(get.symbol, "platypus_Thing_health");
    assert_eq!(get.params[0].name, "this");
    assert_eq!(get.params[0].ty, Ty::Handle("Thing"));
    // `set_health` next to the `#[property] health` is the property's setter.
    let set = by_name("health", FnKind::Set);
    assert_eq!(set.symbol, "platypus_Thing_setHealth");
    assert_eq!(set.params.len(), 2);
    let damage = by_name("damage", FnKind::Method);
    assert_eq!(damage.ret, Ty::Bool);
    assert_eq!(by_name("part", FnKind::Get).ret, Ty::Handle("Part"));
    assert_eq!(m.classes[1].name, "Part");
}

// M9-API-17
#[test]
fn the_declaration_file_has_classes_with_accessors_methods_and_statics() {
    let d = things::MANIFEST.to_dts();
    assert!(d.contains("declare class Thing {"), "{d}");
    assert!(d.contains("static create(health: number): Thing;"), "{d}");
    assert!(d.contains("get health(): number;"), "{d}");
    assert!(d.contains("set health(value: number);"), "{d}");
    assert!(d.contains("damage(amount: number): boolean;"), "{d}");
    assert!(d.contains("get part(): Part;"), "{d}");
    assert!(d.contains("get weight(): number;"), "{d}");
    assert!(
        !d.contains("set weight"),
        "a getter without a setter is read-only:\n{d}"
    );
    assert!(d.contains("function count(): number;"), "{d}");
    assert!(
        !d.contains("declare type Thing"),
        "a class replaces the alias:\n{d}"
    );
    let j = things::MANIFEST.to_json();
    assert!(j.contains("\"classes\":[{\"name\":\"Thing\""), "{j}");
    assert!(j.contains("\"kind\":\"get\""), "{j}");
}

// M9-API-18
#[test]
fn the_shims_take_the_receiver_first_and_call_the_methods() {
    let a = things::platypus_Thing_create(10.0);
    assert_ne!(a, 0);
    assert_eq!(things::platypus_Thing_health(a), 10.0);
    things::platypus_Thing_setHealth(a, 7.0);
    assert!(things::platypus_Thing_damage(a, 3.0));
    assert!(!things::platypus_Thing_damage(a, 9.0));
    assert_eq!(things::platypus_Thing_health(a), -5.0);
    // A property that returns another handle class, and a method back.
    let part = things::platypus_Thing_part(a);
    assert_eq!(things::platypus_Part_weight(part), -10.0);
    assert_eq!(things::platypus_Part_owner(part), a);
    assert_eq!(things::platypus_Things_count(), 1);
}
