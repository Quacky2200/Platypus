// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Closure parameters (`Callback`), their manifest type and the listener table (`M9-API-300..302`).
use std::cell::RefCell;
use std::rc::Rc;
use ts_api::callback::{HandleArg, Listeners, emit};
use ts_api::{Callback, Handle, ts_api};

#[derive(Clone, Copy)]
pub struct Thing(pub u64);
impl Handle for Thing {
    fn to_raw(self) -> u64 {
        self.0
    }
    fn from_raw(raw: u64) -> Self {
        Thing(raw)
    }
}

thread_local! {
    static LISTENERS: RefCell<Listeners<(f64, String)>> = RefCell::new(Listeners::default());
}

#[ts_api(namespace = "Events", handles(Thing = Thing))]
pub mod events_api {
    use super::{LISTENERS, Thing};
    use ts_api::Callback;
    use ts_api::callback::HandleArg;

    /// Calls `f(n, text)` on every event.
    pub fn on_event(f: Callback<(f64, String)>) -> u32 {
        LISTENERS.with(|l| l.borrow_mut().add(f))
    }

    pub fn on_thing(f: Callback<(HandleArg<Thing>, bool)>) {
        drop(f);
    }
}

// M9-API-300
#[test]
fn a_callback_parameter_has_a_closure_type_in_the_manifest_and_declarations() {
    let json = events_api::MANIFEST.to_json();
    assert!(json.contains("\"fn(f64,string):void\""), "{json}");
    assert!(json.contains("\"fn(handle:Thing,bool):void\""), "{json}");
    let dts = events_api::MANIFEST.to_dts();
    assert!(
        dts.contains("function onEvent(f: (a: number, b: string) => void): number;"),
        "{dts}"
    );
    assert!(
        dts.contains("function onThing(f: (a: Thing, b: boolean) => void): void;"),
        "{dts}"
    );
}

// M9-API-301
#[test]
fn listeners_get_events_in_order_and_tokens_are_unique() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s1 = seen.clone();
    let s2 = seen.clone();
    let a = events_api::on_event(Callback::from_fn(move |(n, t): (f64, String)| {
        s1.borrow_mut().push(format!("a{n}{t}"))
    }));
    let b = events_api::on_event(Callback::from_fn(move |(n, t): (f64, String)| {
        s2.borrow_mut().push(format!("b{n}{t}"))
    }));
    assert!(a != 0 && b != 0 && a != b);
    let cbs = LISTENERS.with(|l| l.borrow().snapshot());
    emit(&cbs, (1.0, "x".to_string()));
    assert_eq!(*seen.borrow(), ["a1x", "b1x"]);
    assert!(LISTENERS.with(|l| l.borrow_mut().remove(a)));
    assert!(!LISTENERS.with(|l| l.borrow_mut().remove(a)));
    let cbs = LISTENERS.with(|l| l.borrow().snapshot());
    emit(&cbs, (2.0, "y".to_string()));
    assert_eq!(seen.borrow().last().unwrap(), "b2y");
}

// M9-API-302
#[test]
fn a_script_callback_without_a_runtime_bridge_does_nothing() {
    // No `platypus_ts_rt` in this test binary: nothing is installed, so a closure pointer is ignored
    // instead of called.
    let cb = unsafe { Callback::<(f64,)>::from_script(0x10 as *const u8) };
    cb.call((1.0,));
    let h = HandleArg(Thing(7));
    assert_eq!(h.0.to_raw(), 7);
}
