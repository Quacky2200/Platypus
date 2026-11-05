// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Static properties of handle classes: `Demo.current` is a getter on the class in the manifest
//! (`Window.main`). IDs are in TESTS.md (M9-API-*).
use std::cell::Cell;
use ts_api::{FnKind, Handle, Ty, ts_api};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemoId(pub u64);
impl Handle for DemoId {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        DemoId(raw)
    }
}

thread_local! {
    static CURRENT: Cell<u64> = const { Cell::new(1) };
    static LEVEL: Cell<u32> = const { Cell::new(3) };
    static READS: Cell<u32> = const { Cell::new(0) };
}

#[ts_api(namespace = "Demo", handles(Demo = DemoId))]
pub mod demo {
    use super::{CURRENT, DemoId, LEVEL, READS};

    impl DemoId {
        /// Moves the current one on and returns it.
        pub fn advance() -> DemoId {
            CURRENT.with(|c| c.set(c.get() + 1));
            DemoId(CURRENT.with(|c| c.get()))
        }

        /// The current one, read through the getter each time.
        #[property]
        pub fn current() -> DemoId {
            READS.with(|r| r.set(r.get() + 1));
            DemoId(CURRENT.with(|c| c.get()))
        }

        /// A writable static.
        #[property]
        pub fn level() -> u32 {
            LEVEL.with(|l| l.get())
        }

        pub fn set_level(v: u32) {
            LEVEL.with(|l| l.set(v));
        }

        #[property]
        pub fn id(self) -> f64 {
            self.0 as f64
        }

        #[property]
        #[js_name = "level"]
        pub fn level_of(self) -> f64 {
            f64::from(LEVEL.with(|l| l.get())) * 10.0
        }
    }
}

// M9-API-1490
#[test]
fn a_static_property_of_a_handle_class_is_a_getter_without_a_receiver() {
    CURRENT.with(|c| c.set(1));
    READS.with(|r| r.set(0));
    // The manifest: a getter of the class without a receiver.
    let class = demo::MANIFEST
        .classes
        .iter()
        .find(|c| c.name == "Demo")
        .unwrap();
    let cur = class
        .functions
        .iter()
        .find(|f| f.name == "current")
        .unwrap();
    assert_eq!(cur.kind, FnKind::Get);
    assert!(cur.params.is_empty(), "no `this`");
    assert_eq!(cur.ret, Ty::Handle("Demo"));
    let d = demo::MANIFEST.to_dts();
    assert!(d.contains("static readonly current: Demo;"), "{d}");
    assert!(d.contains("static get level(): number;"), "{d}");
    assert!(d.contains("static set level(value: number);"), "{d}");
    assert!(d.contains("get id(): number;"), "{d}");
    assert!(d.contains("get level(): number;"), "{d}");
    let j = demo::MANIFEST.to_json();
    assert!(
        j.contains("{\"name\":\"current\",\"symbol\":\"platypus_Demo_current\",\"kind\":\"get\",\"params\":[],\"returns\":\"handle:Demo\",\"doc\":\"The current one, read through the getter each time.\"}"),
        "{j}"
    );
    // The shim has no receiver either.
    assert_eq!(demo::platypus_Demo_current(), 1);
    assert!(READS.with(|r| r.get()) >= 1);
}
