// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Window families and event domains (design section 3a): roots, domains, the cross-family guard and
//! the setup exception. IDs are in TESTS.md (M9-API-14xx).
use platypus_engine_api::log::{self, Level};
use platypus_engine_api::window::{self, DomainId, MAIN, Style, WindowRef};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use ts_api::Callback;

fn fresh() {
    window::reset();
    let _ = window::take_requests();
    window::end_frame();
}

/// `Window.create({ style, parent })` with everything else left out.
fn make(style: &str, parent: WindowRef) -> WindowRef {
    WindowRef::create(
        "t",
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        Style::parse(style),
        parent,
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

/// Collects the warnings logged while `f` runs.
fn warnings(f: impl FnOnce()) -> Vec<String> {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    log::set_sink(Some(Box::new(move |level, msg| {
        if level == Level::Warn {
            s.borrow_mut().push(msg.to_string());
        }
    })));
    f();
    log::set_sink(None);
    seen.take()
}

// M9-API-1495
#[test]
fn windows_without_a_parent_are_isolated_roots_and_parented_windows_join_the_family() {
    fresh();
    let tool = make("tool", WindowRef::NONE);
    assert_eq!(window::domain_of(tool), DomainId::MAIN);
    assert_eq!(tool.domain(), 1.0);
    assert!(!tool.is_isolated() && tool.is_child());
    assert_eq!(tool.root(), MAIN);
    assert!(!MAIN.is_isolated() && MAIN.root() == MAIN && MAIN.domain() == 1.0);

    // A normal window without a parent is the root of its own family.
    let iso = make("normal", WindowRef::NONE);
    assert_eq!(iso.domain(), f64::from(iso.handle() as u32));
    assert_eq!(window::domain_of(iso), DomainId(iso.0));
    assert!(iso.is_isolated() && !iso.is_child());
    assert_eq!(iso.root(), iso);
    // A borderless one too; a normal window with a parent joins the parent's family.
    let border = make("borderless", WindowRef::NONE);
    assert!(border.is_isolated());
    let joined = make("normal", MAIN);
    assert_eq!(window::domain_of(joined), DomainId::MAIN);
    assert!(joined.is_child() && !joined.is_isolated());
    assert_eq!(joined.root(), MAIN);

    // A tool of the isolated root, made by the root's creator during setup...
    let tool_of_iso = make("tool", iso);
    assert_eq!(window::domain_of(tool_of_iso), DomainId(iso.0));
    assert_eq!(tool_of_iso.root(), iso);
    // ...and one made from inside the isolated domain defaults to that domain's root.
    {
        let _d = window::enter_domain(DomainId(iso.0));
        let own = make("dialog", WindowRef::NONE);
        assert_eq!(own.parent(), iso);
        assert_eq!(window::domain_of(own), DomainId(iso.0));
        // Another family's window cannot be a parent from here.
        let w = warnings(|| {
            let other = make("tool", MAIN);
            assert_eq!(other.parent(), iso);
        });
        assert!(w.iter().any(|m| m.contains("another family")), "{w:?}");
    }
    assert_eq!(
        window::domains(),
        vec![DomainId::MAIN, DomainId(iso.0), DomainId(border.0)]
    );
    // The default for the main domain is still the main window.
    assert_eq!(make("popup", WindowRef::NONE).parent(), MAIN);
}

// M9-API-1496
#[test]
fn another_familys_windows_can_be_read_but_not_changed_except_during_setup() {
    fresh();
    let iso = make("normal", WindowRef::NONE);
    let d = DomainId(iso.0);
    // During setup the creator (the main domain) may add listeners and queue `run`.
    let seen = Rc::new(RefCell::new(Vec::<String>::new()));
    let s = seen.clone();
    assert!(iso.run(Callback::from_fn(move |()| {
        s.borrow_mut().push(format!(
            "run in {} as {}",
            window::current_domain().0,
            WindowRef::current().0
        ))
    })));
    let s = seen.clone();
    let tok = iso.on_resized(Callback::from_fn(move |(w, _): (f64, f64)| {
        s.borrow_mut().push(format!(
            "resized {w} in {} as {}",
            window::current_domain().0,
            WindowRef::current().0
        ))
    }));
    assert!(tok > 0);
    assert!(window::can_set_up(iso));
    // Setters are not setup: refused with a warning that names both windows' domains.
    let _ = window::take_requests();
    let w = warnings(|| iso.set_title("late"));
    assert_eq!(iso.title(), "t", "reads stay open");
    assert!(
        w.len() == 1 && w[0].contains(&format!("window {}", iso.0)),
        "{w:?}"
    );
    assert!(w[0].contains("domain 1"), "{w:?}");
    assert!(
        window::take_requests().is_empty(),
        "a refused setter queues nothing"
    );

    // The callbacks run in the root's domain, whoever registered them.
    assert_eq!(window::run_queued(d), 1);
    window::opened(
        iso,
        window::Opened {
            position: (0.0, 0.0),
            size: (100.0, 100.0),
            scale: 1.0,
            native_handle: 0,
            platform: String::new(),
        },
    );
    window::resized(iso, 120.0, 100.0);
    assert_eq!(
        *seen.borrow(),
        [
            format!("run in {} as {}", iso.0, iso.0),
            format!("resized 120 in {} as {}", iso.0, iso.0)
        ]
    );
    assert_eq!(window::current_domain(), DomainId::MAIN, "restored");

    // After the frame, setup is over.
    window::end_frame();
    assert!(!window::can_set_up(iso));
    let w = warnings(|| {
        assert_eq!(iso.on_closed(Callback::from_fn(|()| {})), 0);
        assert!(!iso.run(Callback::from_fn(|()| {})));
        iso.close();
        iso.focus();
        MAIN.set_title("still main");
    });
    assert_eq!(w.len(), 4, "{w:?}");
    assert_eq!(
        MAIN.title(),
        "still main",
        "its own family stays changeable"
    );
    assert!(window::take_requests().len() == 1, "only the main title");
    // Inside its own domain the root changes itself, and cannot change the main family.
    {
        let _g = window::enter_domain(d);
        iso.set_title("mine");
        assert!(iso.run(Callback::from_fn(|()| {})));
        let w = warnings(|| {
            MAIN.set_title("nope");
            MAIN.close();
        });
        assert_eq!(w.len(), 2, "{w:?}");
        assert_eq!(MAIN.title(), "still main");
        assert!(window::can_set_up(iso) && !window::can_set_up(MAIN));
    }
    assert_eq!(iso.title(), "mine");
}

// M9-API-1492
#[test]
fn domain_focus_is_per_domain_and_runs_the_domains_listeners() {
    fresh();
    let iso = make("normal", WindowRef::NONE);
    let d = DomainId(iso.0);
    assert!(!WindowRef::domain_focused());
    window::domain_focus_changed(DomainId::MAIN, true, false);
    assert!(WindowRef::domain_focused());
    let got = Rc::new(Cell::new((false, false, 0)));
    let g = got.clone();
    let tok = {
        let _d = window::enter_domain(d);
        assert!(
            !WindowRef::domain_focused(),
            "the isolated domain is not focused"
        );
        WindowRef::on_domain_focus_changed(Callback::from_fn(move |(f, p): (bool, bool)| {
            g.set((f, p, window::current_domain().0))
        }))
    };
    assert!(tok > 0);
    window::domain_focus_changed(DomainId::MAIN, false, true);
    assert_eq!(
        got.get(),
        (false, false, 0),
        "another domain's listener stays quiet"
    );
    window::domain_focus_changed(d, true, false);
    assert_eq!(got.get(), (true, false, iso.0));
    window::domain_focus_changed(d, false, true);
    assert_eq!(got.get(), (false, true, iso.0));
    {
        let _d = window::enter_domain(d);
        assert!(WindowRef::off_static(f64::from(tok)));
        assert!(!WindowRef::off_static(f64::from(tok)));
    }
    window::domain_focus_changed(d, true, false);
    assert_eq!(got.get(), (false, true, iso.0), "removed");
}

// M9-API-1493
#[test]
fn run_in_background_is_kept_on_the_root_and_current_defaults_to_the_domain_root() {
    fresh();
    let iso = WindowRef::create(
        "bg",
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        f64::NAN,
        Style::parse("normal"),
        WindowRef::NONE,
        true,
        true,
        true,
        true,
        false,
        true,
        true,
        "",
        false,
    );
    assert!(!window::run_in_background(DomainId(iso.0)));
    assert!(window::run_in_background(DomainId::MAIN));
    assert_eq!(WindowRef::current(), MAIN);
    {
        let _d = window::enter_domain(DomainId(iso.0));
        assert_eq!(WindowRef::current(), iso);
        let _c = window::enter_current(MAIN);
        assert_eq!(WindowRef::current(), MAIN);
    }
    assert_eq!(WindowRef::current(), MAIN);
}
