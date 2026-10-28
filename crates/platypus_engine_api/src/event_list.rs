// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! `EventList`: a list of callbacks run together (Unity's `UnityEngine.Events.UnityEvent`, also
//! s&box's `Action`). Ported code wires `onSomething.addListener(f)` and calls `invoke()`.
//!
//! Differences to Unity: it takes no arguments (there is no `EventList<T>`), and `addListener`
//! returns a token that `removeListener` takes, since callbacks cannot be compared.

use platypus_api::callback::{Callback, Listeners, emit};
use platypus_api::{Handle, platypus_api};
use std::cell::RefCell;

/// An `EventList` (its place in the registry plus one; 0 is none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventListRef(pub u32);

impl Handle for EventListRef {
    fn to_raw(self) -> u64 {
        u64::from(self.0)
    }
    fn from_raw(raw: u64) -> Self {
        EventListRef(raw as u32)
    }
}

thread_local! {
    static EVENTS: RefCell<Vec<Option<Listeners<()>>>> = const { RefCell::new(Vec::new()) };
}

/// Forgets every event (a new game).
pub fn reset() {
    EVENTS.with(|e| e.borrow_mut().clear());
}

fn with<R>(e: EventListRef, f: impl FnOnce(&mut Listeners<()>) -> R) -> Option<R> {
    EVENTS.with(|v| {
        v.borrow_mut()
            .get_mut((e.0 as usize).wrapping_sub(1))?
            .as_mut()
            .map(f)
    })
}

#[platypus_api(namespace = "EventList", handles(EventList = EventListRef))]
pub mod event_list_api {
    use super::*;

    /// A list of callbacks to run together: `new EventList()`, `addListener(() => ..)`, `invoke()`.
    impl EventListRef {
        /// An event with no listeners.
        pub fn create() -> EventListRef {
            EVENTS.with(|v| {
                let mut v = v.borrow_mut();
                v.push(Some(Listeners::default()));
                EventListRef(v.len() as u32)
            })
        }

        /// Adds a callback (the same function twice runs twice, as in Unity); the token it
        /// returns is what `removeListener` takes.
        pub fn add_listener(self, f: Callback<()>) -> u32 {
            with(self, |l| l.add(f)).unwrap_or(0)
        }

        /// Removes the callback `addListener` gave `token` for; false when it is unknown.
        pub fn remove_listener(self, token: u32) -> bool {
            with(self, |l| l.remove(token)).unwrap_or(false)
        }

        pub fn remove_all_listeners(self) {
            with(self, Listeners::clear);
        }

        #[property]
        pub fn listener_count(self) -> u32 {
            with(self, |l| l.len() as u32).unwrap_or(0)
        }

        /// Calls every callback in the order added. A callback that throws is reported and the
        /// rest still run; callbacks added or removed meanwhile take effect next time.
        pub fn invoke(self) {
            if let Some(snapshot) = with(self, |l| l.snapshot()) {
                emit(&snapshot, ());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    // RDY-EVT-01
    #[test]
    fn listeners_run_in_order_and_can_be_removed() {
        reset();
        let log = Rc::new(RefCell::new(String::new()));
        let event = EventListRef::create();
        let add = |name: &'static str| {
            let log = log.clone();
            event.add_listener(Callback::from_fn(move |()| log.borrow_mut().push_str(name)))
        };
        let (a, _b) = (add("a"), add("b"));
        add("a");
        event.invoke();
        assert_eq!(*log.borrow(), "aba");
        assert_eq!(event.listener_count(), 3);
        assert!(event.remove_listener(a));
        assert!(!event.remove_listener(a));
        log.borrow_mut().clear();
        event.invoke();
        assert_eq!(*log.borrow(), "ba");
        event.remove_all_listeners();
        assert_eq!(event.listener_count(), 0);
        let hits = Rc::new(Cell::new(0));
        let h = hits.clone();
        event.add_listener(Callback::from_fn(move |()| h.set(h.get() + 1)));
        event.invoke();
        assert_eq!(hits.get(), 1);
    }
}
