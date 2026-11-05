// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `#[ts_api]`: the manifest, the `.d.ts` and the `extern "C"` shims all come from one
//! module of ordinary Rust. IDs are in TESTS.md (M9-API-*).
use std::cell::Cell;
use ts_api::{Handle, Ty, ts_api};

/// A stand-in for the engine's generational object id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectId(pub u64);

impl Handle for ObjectId {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        ObjectId(raw)
    }
}

thread_local! {
    static SCALE: Cell<f32> = const { Cell::new(1.0) };
    static LOG: Cell<u32> = const { Cell::new(0) };
}

#[ts_api(namespace = "Demo", handles(GameObject = ObjectId))]
pub mod demo {
    use super::{LOG, ObjectId, SCALE};

    /// Adds two numbers.
    ///
    /// The doc comment becomes the declaration's JSDoc.
    pub fn add(a: f64, b: f64) -> f64 {
        a + b
    }

    /// Sets the time scale (stored as a 32-bit float).
    pub fn set_time_scale(scale: f32) {
        SCALE.with(|s| s.set(scale));
    }

    pub fn time_scale() -> f32 {
        SCALE.with(|s| s.get())
    }

    pub fn is_big(value: i32) -> bool {
        value > 100
    }

    pub fn count_up(by: u32) -> u32 {
        LOG.with(|l| {
            l.set(l.get() + by);
            l.get()
        })
    }

    /// The parent of an object (here: the next id).
    pub fn parent_of(object: ObjectId) -> ObjectId {
        ObjectId(object.0 + 1)
    }

    pub fn same_object(a: ObjectId, b: ObjectId) -> bool {
        a == b
    }

    /// Not part of the API: not `pub`.
    #[allow(dead_code)]
    fn internal() {}
}

// M9-API-01
#[test]
fn the_manifest_describes_every_public_function() {
    let m = demo::MANIFEST;
    assert_eq!(m.name, "Demo");
    assert_eq!(m.handles, &["GameObject"]);
    let names: Vec<&str> = m.functions.iter().map(|f| f.name).collect();
    assert_eq!(
        names,
        [
            "add",
            "setTimeScale",
            "timeScale",
            "isBig",
            "countUp",
            "parentOf",
            "sameObject"
        ]
    );
    let add = &m.functions[0];
    assert_eq!(add.rust_name, "add");
    assert_eq!(add.symbol, "platypus_Demo_add");
    assert_eq!(add.ret, Ty::F64);
    assert_eq!(add.params.len(), 2);
    assert_eq!(add.params[1].name, "b");
    assert!(add.doc.starts_with("Adds two numbers."));
    let parent = &m.functions[5];
    assert_eq!(parent.params[0].ty, Ty::Handle("GameObject"));
    assert_eq!(parent.ret, Ty::Handle("GameObject"));
    assert_eq!(m.functions[1].params[0].ty, Ty::F32);
    assert_eq!(m.functions[1].ret, Ty::Void);
}

// M9-API-02
#[test]
fn the_declaration_file_is_generated_from_the_manifest() {
    let d = demo::MANIFEST.to_dts();
    assert!(d.contains("declare namespace Demo {"), "{d}");
    assert!(
        d.contains("function add(a: number, b: number): number;"),
        "{d}"
    );
    assert!(
        d.contains("function setTimeScale(scale: number): void;"),
        "{d}"
    );
    assert!(d.contains("function isBig(value: number): boolean;"), "{d}");
    assert!(
        d.contains("function parentOf(object: GameObject): GameObject;"),
        "{d}"
    );
    assert!(d.contains("declare type GameObject = number &"), "{d}");
    assert!(
        d.contains("   * The doc comment becomes the declaration's JSDoc."),
        "{d}"
    );
    assert!(!d.contains("internal"), "{d}");
}

// M9-API-03
#[test]
fn the_json_manifest_names_the_symbols_and_types_the_compiler_needs() {
    let j = demo::MANIFEST.to_json();
    assert!(j.starts_with("{\"namespace\":\"Demo\""), "{j}");
    assert!(
        j.contains("{\"name\":\"add\",\"symbol\":\"platypus_Demo_add\",\"kind\":\"static\",\"params\":[{\"name\":\"a\",\"type\":\"f64\"},{\"name\":\"b\",\"type\":\"f64\"}],\"returns\":\"f64\",\"doc\":\"Adds two numbers.\\n\\nThe doc comment becomes the declaration's JSDoc.\"}"),
        "{j}"
    );
    assert!(j.contains("\"returns\":\"handle:GameObject\""), "{j}");
}

// M9-API-04
#[test]
fn the_extern_c_shims_call_the_functions_with_the_c_abi() {
    assert_eq!(demo::platypus_Demo_add(1.5, 2.0), 3.5);
    demo::platypus_Demo_setTimeScale(0.5);
    assert_eq!(demo::platypus_Demo_timeScale(), 0.5);
    assert!(demo::platypus_Demo_isBig(101));
    assert!(!demo::platypus_Demo_isBig(7));
    assert_eq!(demo::platypus_Demo_countUp(3), 3);
    assert_eq!(demo::platypus_Demo_countUp(4), 7);
    // Handles cross as raw 64-bit ids.
    assert_eq!(demo::platypus_Demo_parentOf(41), 42);
    assert!(demo::platypus_Demo_sameObject(5, 5));
    // The symbols have the C ABI and can be taken as plain function pointers.
    let f: extern "C" fn(f64, f64) -> f64 = demo::platypus_Demo_add;
    assert_eq!(f(2.0, 2.0), 4.0);
}
