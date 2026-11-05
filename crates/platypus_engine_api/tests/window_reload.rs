// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Hot reload and windows: keyed windows stay, the rest close. IDs are in TESTS.md (M9-API-148x).
use platypus_engine_api::window::{self, MAIN, Opened, Style, WindowRef, WindowRequest};
use std::cell::Cell;
use std::rc::Rc;
use ts_api::Callback;

fn create(title: &str, style: &str, key: &str) -> WindowRef {
    WindowRef::create(
        title,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        Style::parse(style),
        WindowRef::NONE,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        key,
        true,
    )
}

fn open(w: WindowRef, size: (f64, f64)) {
    window::opened(
        w,
        Opened {
            position: (50.0, 60.0),
            size,
            scale: 1.0,
            native_handle: 0,
            platform: "virtual".into(),
        },
    );
}

fn fresh() {
    window::reset();
    let _ = window::take_requests();
}

// M9-API-1480
#[test]
fn reload_drops_listeners_and_returns_keyless_windows_to_close() {
    fresh();
    let keyed = create("Keep", "tool", "keep");
    let keyless = create("Lose", "tool", "");
    let hits = Rc::new(Cell::new(0));
    for w in [keyed, keyless, MAIN] {
        open(w, (100.0, 100.0));
        let h = hits.clone();
        w.on_resized(Callback::from_fn(move |(_, _): (f64, f64)| {
            h.set(h.get() + 1)
        }));
    }
    let close = window::reload_begin();
    assert_eq!(
        close,
        vec![keyless],
        "only windows without a key, never the main one"
    );
    // The old code's listeners are gone, also on the windows that stay.
    window::resized(keyed, 200.0, 200.0);
    window::resized(MAIN, 300.0, 300.0);
    assert_eq!(hits.get(), 0);
    assert!(keyed.is_open() && MAIN.is_open());
    // The host closes the keyless one; its `onClosed` listeners were dropped.
    window::closed(keyless);
    assert!(!keyless.is_open());
}

// M9-API-1481
#[test]
fn create_with_the_key_of_a_kept_window_returns_that_window_with_the_new_title() {
    fresh();
    let old = create("Palette v1", "tool", "palette");
    open(old, (220.0, 320.0));
    window::moved(old, 400.0, 90.0);
    window::resized(old, 250.0, 330.0);
    let _ = window::take_requests();
    assert!(window::reload_begin().is_empty());
    let again = create("Palette v2", "tool", "palette");
    assert_eq!(again, old, "the same handle: no new window");
    assert_eq!(again.title(), "Palette v2");
    // The player's size and position stay.
    let st = window::state(old).unwrap();
    assert_eq!(st.size, (250.0, 330.0));
    assert_eq!(st.position, Some((400.0, 90.0)));
    // The host hears the new title, and no `Create`.
    assert_eq!(
        window::take_requests(),
        vec![WindowRequest::Title(old, "Palette v2".into())]
    );
    // The new code's listeners work.
    let hits = Rc::new(Cell::new(0));
    let h = hits.clone();
    assert_ne!(
        again.on_resized(Callback::from_fn(move |(_, _): (f64, f64)| {
            h.set(h.get() + 1)
        })),
        0
    );
    window::resized(old, 260.0, 340.0);
    assert_eq!(hits.get(), 1);
    // A second window with the key is a new one (the first was claimed).
    let other = create("Palette v2", "tool", "palette");
    assert_ne!(other, old);
}

// M9-API-1482
#[test]
fn keyed_windows_nobody_claimed_are_closed_after_the_reload() {
    fresh();
    let a = create("A", "tool", "a");
    let b = create("B", "tool", "b");
    open(a, (10.0, 10.0));
    open(b, (10.0, 10.0));
    assert!(window::reload_begin().is_empty());
    assert_eq!(create("A", "tool", "a"), a);
    assert_eq!(window::reload_end(), vec![b]);
    // Nothing left to close the second time.
    assert!(window::reload_end().is_empty());
}

// M9-API-1483
#[test]
fn the_main_window_survives_reload_with_its_title() {
    fresh();
    MAIN.set_title("Level 3");
    let _ = window::take_requests();
    assert!(window::reload_begin().is_empty());
    assert!(MAIN.is_open());
    assert_eq!(MAIN.title(), "Level 3");
    assert!(window::reload_end().is_empty());
    // The new code sets it again.
    MAIN.set_title("Level 4");
    assert_eq!(MAIN.title(), "Level 4");
}

// M9-API-1484
#[test]
fn a_failed_reload_gives_the_old_code_its_listeners_back_and_forgets_the_new_windows() {
    fresh();
    let keyed = create("K", "tool", "k");
    open(keyed, (100.0, 100.0));
    let hits = Rc::new(Cell::new(0));
    let h = hits.clone();
    keyed.on_resized(Callback::from_fn(move |(_, _): (f64, f64)| {
        h.set(h.get() + 1)
    }));
    let _ = window::take_requests();
    window::reload_begin();
    // The new code's top level ran: it claimed the window and made another, then failed.
    assert_eq!(create("K", "tool", "k"), keyed);
    let made = create("New", "tool", "n");
    window::reload_abort();
    window::resized(keyed, 200.0, 200.0);
    assert_eq!(hits.get(), 1, "the old listener is back");
    assert!(!made.is_open(), "the window the failed code made is gone");
    assert!(window::reload_end().is_empty());
}
