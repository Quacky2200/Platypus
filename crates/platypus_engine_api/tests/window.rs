// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The `Window` API's host side: requests, events, closing. IDs are in TESTS.md (M9-API-14xx).
//! The Rust API and the generated shims are called directly: no display, no GPU, no script runtime.
use platypus_engine_api::app;
use platypus_engine_api::window::{
    self, MAIN, Opened, Style, WindowRef, WindowRequest, window_api,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use ts_api::Callback;

fn fresh() {
    window::reset();
    let _ = window::take_requests();
}

fn create_tool() -> WindowRef {
    // The compiled call: every optional parameter left out is NaN / empty / the manifest default.
    WindowRef::create(
        "Palette",
        220.0,
        320.0,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        Style::parse("tool"),
        WindowRef::NONE,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        "palette",
        true,
    )
}

fn create_plain(style: &str) -> WindowRef {
    WindowRef::create(
        "",
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
        "",
        true,
    )
}

fn opened(w: WindowRef, size: (f64, f64)) {
    window::opened(
        w,
        Opened {
            position: (100.0, 80.0),
            size,
            scale: 1.5,
            native_handle: 0xBEEF,
            platform: "win32".into(),
        },
    );
}

// M9-API-1410
#[test]
fn the_main_window_exists_from_the_start_and_takes_the_manifest_title() {
    fresh();
    assert_eq!(WindowRef::main(), MAIN);
    assert_eq!(MAIN.title(), "Platypus");
    window::set_main("Game", (800.0, 600.0), 2.0);
    assert_eq!(WindowRef::main().title(), "Game");
    assert_eq!(
        (MAIN.width(), MAIN.height(), MAIN.scale()),
        (800.0, 600.0, 2.0)
    );
    assert!(MAIN.is_main() && MAIN.is_open() && !MAIN.is_child());
    assert_eq!(MAIN.handle(), 1.0);
    assert_eq!(MAIN.parent(), WindowRef::NONE);
}

// M9-API-1411
#[test]
fn setting_the_title_asks_the_host_and_reads_back_at_once() {
    fresh();
    MAIN.set_title("Level 2");
    assert_eq!(MAIN.title(), "Level 2");
    assert_eq!(
        window::take_requests(),
        vec![WindowRequest::Title(MAIN, "Level 2".into())]
    );
    assert!(window::take_requests().is_empty());
}

// M9-API-1412
#[test]
fn create_returns_a_handle_at_once_and_queues_one_request() {
    fresh();
    let w = create_tool();
    assert_eq!(w, WindowRef(2));
    assert_eq!(
        window::take_requests(),
        vec![WindowRequest::Create(WindowRef(2))]
    );
    let st = window::state(w).unwrap();
    assert!(!st.created && st.open);
    assert_eq!(st.native_handle, 0);
    assert_eq!(w.platform(), "");
    assert_eq!(create_tool(), WindowRef(3), "handles are never reused");
}

// M9-API-1413
#[test]
fn create_fills_defaults_per_style() {
    fresh();
    let tool = create_plain("tool");
    assert_eq!(window::state(tool).unwrap().parent, Some(MAIN));
    let normal = create_plain("");
    assert_eq!(window::state(normal).unwrap().parent, None);
    assert_eq!(window::state(normal).unwrap().style, Style::Normal);
    let st = window::state(tool).unwrap();
    assert_eq!(st.size, (400.0, 300.0), "NaN size");
    assert_eq!(st.position, None, "NaN position");
    assert_eq!(st.min_size, None);
    assert!(st.flags.resizable && st.flags.focus_on_show);
    let popup = create_plain("popup");
    let ps = window::state(popup).unwrap();
    assert!(!ps.flags.focus_on_show, "popups never take focus");
    assert_eq!(ps.parent, Some(MAIN));
    // An explicit parent is kept; a size is clamped.
    let child = WindowRef::create(
        "c",
        0.0,
        99999.0,
        10.0,
        20.0,
        100.0,
        f64::NAN,
        Style::parse("dialog"),
        tool,
        false,
        true,
        true,
        true,
        false,
        true,
        true,
        "",
        true,
    );
    let cs = window::state(child).unwrap();
    assert_eq!(cs.parent, Some(tool));
    assert_eq!(cs.size, (1.0, 16384.0));
    assert_eq!(cs.position, Some((10.0, 20.0)));
    assert_eq!(cs.min_size, Some((100.0, 0.0)));
    assert!(!cs.flags.resizable);
    // The pending window reads a position at once: centred on its parent, cascading.
    window::opened(
        MAIN,
        Opened {
            position: (0.0, 0.0),
            size: (1000.0, 800.0),
            scale: 1.0,
            native_handle: 0,
            platform: String::new(),
        },
    );
    assert_eq!(tool.x(), (1000.0 - 400.0) / 2.0);
    assert_eq!(
        popup.x(),
        (1000.0 - 400.0) / 2.0 + 24.0,
        "the second child cascades"
    );
}

// M9-API-1414
#[test]
fn unknown_styles_fall_back_to_normal() {
    fresh();
    let w = create_plain("fancy");
    assert_eq!(w.style().name(), "normal");
    assert_eq!(window::state(w).unwrap().parent, None);
    MAIN.set_style(Style::parse("fancy"));
    assert_eq!(MAIN.style().name(), "normal");
    let t = create_plain("tool");
    t.set_style(Style::parse("dialog"));
    assert_eq!(t.style().name(), "dialog");
    t.set_style(Style::parse("child"));
    assert_eq!(
        t.style().name(),
        "dialog",
        "child cannot be set after creation"
    );
    assert_eq!(Style::parse("Tool"), None);
    assert_eq!(Style::parse("borderless").unwrap().name(), "borderless");
}

// M9-API-1415
#[test]
fn setters_of_the_same_field_coalesce_but_create_and_close_keep_their_order() {
    fresh();
    let a = create_plain("tool");
    a.set_title("one");
    MAIN.set_title("m1");
    a.set_title("two");
    MAIN.set_title("m2");
    a.set_resizable(false);
    a.set_always_on_top(true);
    let b = create_plain("tool");
    a.close();
    a.close();
    b.close();
    let reqs = window::take_requests();
    let flags = window::state(a).unwrap().flags;
    assert_eq!(
        reqs,
        vec![
            WindowRequest::Create(a),
            WindowRequest::Title(a, "two".into()),
            WindowRequest::Title(MAIN, "m2".into()),
            WindowRequest::Flags(a, flags),
            WindowRequest::Create(b),
            WindowRequest::Close(a),
            WindowRequest::Close(b),
        ]
    );
}

// M9-API-1416
#[test]
fn opened_fills_the_native_handle_platform_and_size() {
    fresh();
    let w = create_tool();
    opened(w, (230.0, 330.0));
    let st = window::state(w).unwrap();
    assert!(st.created);
    assert_eq!(
        (w.native_handle(), w.platform().as_str()),
        (48879.0, "win32")
    );
    assert_eq!(
        (w.width(), w.height()),
        (230.0, 330.0),
        "the OS answer wins"
    );
    assert_eq!((w.x(), w.y(), w.scale()), (100.0, 80.0, 1.5));
    // Setting the size asks the OS and keeps the old value until it answers.
    w.set_width(300.0);
    assert_eq!(w.width(), 230.0);
    let _ = window::take_requests();
    w.set_size(310.0, 340.0);
    assert_eq!(
        window::take_requests(),
        vec![WindowRequest::Size(w, 310.0, 340.0)]
    );
    window::resized(w, 311.0, 340.0);
    assert_eq!(w.width(), 311.0);
}

// M9-API-1417
#[test]
fn resize_move_focus_and_scale_events_reach_their_listeners_once_per_change() {
    fresh();
    let w = create_tool();
    opened(w, (200.0, 100.0));
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let l = log.clone();
    w.on_resized(Callback::from_fn(move |(a, b): (f64, f64)| {
        l.borrow_mut().push(format!("resized {a}x{b}"))
    }));
    let l = log.clone();
    w.on_moved(Callback::from_fn(move |(a, b): (f64, f64)| {
        l.borrow_mut().push(format!("moved {a},{b}"))
    }));
    let l = log.clone();
    w.on_focus_changed(Callback::from_fn(move |(f,): (bool,)| {
        l.borrow_mut().push(format!("focus {f}"))
    }));
    let l = log.clone();
    w.on_scale_changed(Callback::from_fn(move |(s,): (f64,)| {
        l.borrow_mut().push(format!("scale {s}"))
    }));
    window::resized(w, 200.0, 100.0);
    window::resized(w, 210.0, 100.0);
    window::resized(w, 210.0, 100.0);
    window::moved(w, 100.0, 80.0);
    window::moved(w, 5.0, 6.0);
    window::focus_changed(w, true);
    window::focus_changed(w, true);
    assert!(w.focused_now());
    assert_eq!(WindowRef::focused(), w);
    window::focus_changed(w, false);
    assert_eq!(WindowRef::focused(), WindowRef::NONE);
    window::scale_changed(w, 1.5);
    window::scale_changed(w, 2.0);
    assert_eq!(
        *log.borrow(),
        [
            "resized 210x100",
            "moved 5,6",
            "focus true",
            "focus false",
            "scale 2"
        ]
    );
    assert_eq!((w.x(), w.y()), (5.0, 6.0));
}

// M9-API-1418
#[test]
fn a_close_request_listener_can_cancel_the_close() {
    fresh();
    let w = create_tool();
    assert!(window::close_requested(w), "no listener: allowed");
    let cancel = Rc::new(Cell::new(true));
    let c = cancel.clone();
    let tok = w.on_close_requested(Callback::from_fn(move |()| {
        if c.get() {
            w.cancel_close();
        }
    }));
    assert!(tok > 0);
    assert!(!window::close_requested(w));
    cancel.set(false);
    assert!(window::close_requested(w), "the cancel does not stick");
}

// M9-API-1419
#[test]
fn cancel_close_outside_a_close_request_does_nothing() {
    fresh();
    let w = create_tool();
    w.cancel_close();
    assert!(window::close_requested(w));
    // A listener of another window cancelling a different window's request is ignored too.
    let other = create_tool();
    w.on_close_requested(Callback::from_fn(move |()| other.cancel_close()));
    assert!(window::close_requested(w));
}

// M9-API-1420
#[test]
fn closed_runs_on_closed_once_and_drops_the_listeners() {
    fresh();
    let w = create_tool();
    let n = Rc::new(Cell::new(0));
    let c = n.clone();
    w.on_closed(Callback::from_fn(move |()| c.set(c.get() + 1)));
    let c = n.clone();
    let resized = w.on_resized(Callback::from_fn(move |_| c.set(c.get() + 100)));
    window::closed(w);
    window::closed(w);
    assert_eq!(n.get(), 1);
    assert!(!w.is_open());
    assert!(!w.off(f64::from(resized)), "the listeners are gone");
    window::resized(w, 5.0, 5.0);
    assert_eq!(n.get(), 1);
    assert_eq!(
        w.on_closed(Callback::from_fn(|()| {})),
        0,
        "no listeners on a closed window"
    );
}

// M9-API-1421
#[test]
fn a_closed_window_reads_its_last_values_and_ignores_setters() {
    fresh();
    let w = create_tool();
    opened(w, (200.0, 100.0));
    let _ = window::take_requests();
    window::closed(w);
    w.set_title("late");
    w.set_resizable(false);
    w.focus();
    w.close();
    assert_eq!(w.title(), "Palette");
    assert!(w.resizable());
    assert_eq!((w.width(), w.height()), (200.0, 100.0));
    assert!(window::take_requests().is_empty());
    assert_eq!(WindowRef::at(1.0), WindowRef::NONE);
}

// M9-API-1422
#[test]
fn count_and_at_list_open_windows_main_first() {
    fresh();
    assert_eq!(WindowRef::count(), 1);
    let a = create_tool();
    let b = create_plain("dialog");
    assert_eq!(WindowRef::count(), 3);
    assert_eq!(
        [0.0, 1.0, 2.0, 3.0, -1.0, f64::NAN].map(WindowRef::at),
        [
            MAIN,
            a,
            b,
            WindowRef::NONE,
            WindowRef::NONE,
            WindowRef::NONE
        ]
    );
    assert_eq!(WindowRef::find("palette"), a);
    assert_eq!(WindowRef::find("nope"), WindowRef::NONE);
    assert_eq!(WindowRef::find(""), WindowRef::NONE);
    assert_eq!(WindowRef::by_handle(3.0), b);
    assert_eq!(WindowRef::by_handle(9.0), WindowRef::NONE);
    window::closed(a);
    assert_eq!(WindowRef::count(), 2);
    assert_eq!(WindowRef::at(1.0), b);
    assert_eq!(WindowRef::find("palette"), WindowRef::NONE);
    assert_eq!(WindowRef::by_handle(2.0), WindowRef::NONE);
    assert_eq!(window::open_windows(), vec![MAIN, b]);
    // A taken key still creates the window, and find returns the first.
    let c1 = WindowRef::create(
        "",
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        None,
        WindowRef::NONE,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        "k",
        true,
    );
    let c2 = WindowRef::create(
        "",
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        None,
        WindowRef::NONE,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        "k",
        true,
    );
    assert_ne!(c1, c2);
    assert_eq!(WindowRef::find("k"), c1);
}

// M9-API-1423
#[test]
fn parent_children_and_is_child() {
    fresh();
    let tool = create_plain("tool");
    let normal = create_plain("normal");
    let dlg = WindowRef::create(
        "d",
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        Style::parse("dialog"),
        tool,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        "",
        true,
    );
    assert_eq!(tool.parent(), MAIN);
    assert!(tool.is_child() && dlg.is_child());
    assert!(!normal.is_child() && !MAIN.is_child());
    assert_eq!(normal.parent(), WindowRef::NONE);
    assert_eq!(MAIN.child_count(), 1);
    assert_eq!(MAIN.child_at(0.0), tool);
    assert_eq!(MAIN.child_at(1.0), WindowRef::NONE);
    assert_eq!(tool.child_count(), 1);
    assert_eq!(tool.child_at(0.0), dlg);
    window::closed(dlg);
    assert_eq!(tool.child_count(), 0);
    // A closed or missing parent is ignored.
    let orphan = WindowRef::create(
        "",
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        Style::parse("normal"),
        dlg,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        "",
        true,
    );
    assert!(!orphan.is_child());
}

// M9-API-1424
#[test]
fn current_is_the_window_whose_event_is_dispatched_else_main() {
    fresh();
    let w = create_tool();
    assert_eq!(WindowRef::current(), MAIN);
    {
        let _g = window::enter_current(w);
        assert_eq!(WindowRef::current(), w);
        {
            let _g2 = window::enter_current(MAIN);
            assert_eq!(WindowRef::current(), MAIN);
        }
        assert_eq!(WindowRef::current(), w);
    }
    assert_eq!(WindowRef::current(), MAIN);
}

// M9-API-1425
#[test]
fn off_removes_any_listener_by_token() {
    fresh();
    let w = create_tool();
    let n = Rc::new(Cell::new(0));
    let c = n.clone();
    let t1 = w.on_resized(Callback::from_fn(move |_| c.set(c.get() + 1)));
    let c = n.clone();
    let t2 = w.on_focus_changed(Callback::from_fn(move |_| c.set(c.get() + 10)));
    assert!(t1 > 0 && t2 > t1);
    assert!(w.off(f64::from(t2)));
    assert!(!w.off(f64::from(t2)), "once");
    assert!(!w.off(0.0) && !w.off(f64::NAN));
    window::focus_changed(w, true);
    window::resized(w, 9.0, 9.0);
    assert_eq!(n.get(), 1);
    assert!(w.off(f64::from(t1)));
    window::resized(w, 8.0, 8.0);
    assert_eq!(n.get(), 1);
}

// M9-API-1453
#[test]
fn a_failed_open_sets_open_error_runs_on_open_failed_once_and_leaves_the_main_window_alone() {
    fresh();
    assert!(WindowRef::is_supported());
    window::set_supported(false);
    assert!(!WindowRef::is_supported());
    let w = create_tool();
    assert_eq!(w.open_error(), None);
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let s = seen.clone();
    assert_ne!(
        w.on_open_failed(Callback::from_fn(move |(e,): (String,)| {
            s.borrow_mut().push(e)
        })),
        0
    );
    let error = "Cannot open Window due to platform restrictions (platform: android)";
    window::open_failed(w, error);
    // A second report (the window is gone) does nothing.
    window::open_failed(w, error);
    assert_eq!(*seen.borrow(), vec![error.to_string()]);
    assert_eq!(w.open_error().as_deref(), Some(error));
    assert!(!w.is_open());
    assert!(MAIN.is_open() && MAIN.open_error().is_none());
    window::reset();
    assert!(WindowRef::is_supported());
}

// M9-API-1454
#[test]
fn on_opened_runs_with_the_size_and_scale_when_the_host_makes_the_window() {
    fresh();
    let w = create_tool();
    let seen = Rc::new(RefCell::new(Vec::<(f64, f64, f64)>::new()));
    let s = seen.clone();
    w.on_opened(Callback::from_fn(move |(a, b, c): (f64, f64, f64)| {
        s.borrow_mut().push((a, b, c))
    }));
    opened(w, (230.0, 330.0));
    assert_eq!(*seen.borrow(), vec![(230.0, 330.0, 1.5)]);
}

// M9-API-1426
#[test]
fn closing_the_main_window_from_script_asks_to_quit() {
    fresh();
    let _ = app::take_requests();
    MAIN.close();
    assert!(window::take_requests().is_empty(), "no Close(MAIN) request");
    assert!(app::take_requests().contains(&app::Request::Quit(0)));
    assert!(MAIN.is_open(), "the host ends the game");
    let _ = window_api::MANIFEST;
}
