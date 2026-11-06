// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The Window API from compiled programs (`bench/windows.ts`, prefix `win_`), in the normal, stress GC
//! and collection-off modes. The test plays the host: it reads the requests and reports `opened`,
//! `resized`, `close_requested` and `closed`. IDs are in TESTS.md (M9-TS-2800..).
use platypus_engine_api::window::{self, MAIN, Opened, WindowRef, WindowRequest};
use platypus_ts_tests::windows_t::*;
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress, ts_rt_str_new};

fn reset() {
    window::reset();
}

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(web_ui_bridge::link_anchor(), 0);
    assert_ne!(platypus_ts_tests::engine_api_anchor(), 0);
    let run = || {
        reset();
        let _ = web_ui_bridge::take_mounts();
        let _ = web_ui_bridge::take_window_mounts();
        unsafe { ts_win_init() };
        f();
    };
    run();
    ts_rt_gc_stress(true);
    run();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    run();
    ts_rt_gc_enable(true);
    reset();
}

fn text(p: *mut u8) -> String {
    assert!(!p.is_null());
    String::from_utf16_lossy(&unsafe { Str::from_raw(p as *const Str) }.to_utf16())
}

fn script_str(s: &str) -> *mut u8 {
    unsafe { ts_rt_str_new(s.as_ptr(), s.len() as i64) as *mut u8 }
}

/// What the platform tells the app once it made the window.
fn opened(w: WindowRef) {
    window::opened(
        w,
        Opened {
            position: (10.0, 20.0),
            size: (220.0, 320.0),
            scale: 1.0,
            native_handle: 0,
            platform: "virtual".into(),
        },
    );
}

// M9-TS-2800
#[test]
fn create_with_an_options_object_maps_keys_by_name() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createTool();
        assert_eq!(h, 2.0);
        assert_eq!(
            text(ts_win_describe(h)),
            "Palette|tool|true|Platypus|220x320"
        );
        let st = window::state(WindowRef(2)).unwrap();
        assert_eq!(st.key, "palette");
        assert_eq!(st.parent, Some(MAIN));
        assert!(st.flags.resizable && st.flags.closable && !st.flags.always_on_top);
        assert_eq!(
            window::take_requests(),
            vec![WindowRequest::Create(WindowRef(2))]
        );
        // A dialog: the parent and a flag given by name; the rest default.
        let d = ts_win_createDialog(h);
        assert_eq!(
            text(ts_win_describe(d)),
            "Sure?|dialog|true|Palette|400x300"
        );
        let ds = window::state(WindowRef(d as u32)).unwrap();
        assert_eq!(ds.parent, Some(WindowRef(2)));
        assert!(!ds.flags.resizable && ds.flags.minimizable);
    });
}

// M9-TS-2801
#[test]
fn window_main_reads_as_a_property_and_its_title_goes_to_the_host() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_win_mainTitle()), "Platypus");
        ts_win_setMainTitle(script_str("Level 2"));
        assert_eq!(text(ts_win_mainTitle()), "Level 2");
        assert_eq!(
            window::take_requests(),
            vec![WindowRequest::Title(MAIN, "Level 2".into())]
        );
        assert_eq!(text(ts_win_mainInfo()), "1|true|false|1");
    });
}

// M9-TS-2802
#[test]
fn static_and_instance_focused_both_resolve() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createTool();
        assert_eq!(text(ts_win_focusState(h)), "true|false|0");
        window::focus_changed(WindowRef(2), true);
        assert_eq!(text(ts_win_focusState(h)), "false|true|2");
        window::focus_changed(WindowRef(2), false);
        assert_eq!(text(ts_win_focusState(h)), "true|false|0");
    });
}

// M9-TS-2803
#[test]
fn window_listeners_from_compiled_closures_run_and_cancel() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createTool();
        let w = WindowRef(2);
        opened(w);
        ts_win_listen(h);
        assert_eq!(text(ts_win_counts()), "0|0|0|0x0");
        window::resized(w, 300.0, 350.0);
        window::resized(w, 300.0, 350.0);
        assert_eq!(text(ts_win_counts()), "1|0|0|300x350");
        ts_win_setCancel(true);
        assert!(!window::close_requested(w), "the listener cancelled");
        ts_win_setCancel(false);
        assert!(window::close_requested(w));
        assert_eq!(text(ts_win_counts()), "1|0|2|300x350");
        window::closed(w);
        window::closed(w);
        assert_eq!(text(ts_win_counts()), "1|1|2|300x350");
        assert_eq!(ts_api::callback::take_errors().len(), 0);
    });
}

// M9-TS-2804
#[test]
fn count_and_at_list_the_windows_in_order() {
    in_every_gc_mode(|| unsafe {
        assert_eq!(text(ts_win_list()), "Platypus");
        let h = ts_win_createTool();
        let d = ts_win_createDialog(h);
        assert_eq!(text(ts_win_list()), "Platypus,Palette,Sure?");
        window::closed(WindowRef(h as u32));
        assert_eq!(text(ts_win_list()), "Platypus,Sure?");
        assert_eq!(
            text(ts_win_describe(d)),
            "Sure?|dialog|true|Palette|400x300"
        );
    });
}

// M9-TS-2805
#[test]
fn create_root_of_a_window_queues_a_window_mount() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createTool();
        ts_win_mount(h);
        let mounts = web_ui_bridge::take_window_mounts();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].0, 2);
        assert!(mounts[0].1 != 0);
        assert!(
            web_ui_bridge::take_mounts().is_empty(),
            "not a screen or panel mount"
        );
        // The main window is the screen: target 0.
        ts_win_mountMain();
        let screen = web_ui_bridge::take_mounts();
        assert_eq!(screen.len(), 1);
        assert_eq!(screen[0].0, 0);
        assert!(web_ui_bridge::take_window_mounts().is_empty());
    });
}

// ---- Messages (step M2) ----------------------------------------------------------------------

/// Captures what reaches a window's `onMessageJson` listener, as `from:text`.
fn capture(w: WindowRef) -> std::rc::Rc<std::cell::RefCell<Vec<String>>> {
    let log: std::rc::Rc<std::cell::RefCell<Vec<String>>> = Default::default();
    let l = log.clone();
    w.on_message_json(ts_api::Callback::from_fn(
        move |(from, text): (ts_api::callback::HandleArg<WindowRef>, String)| {
            l.borrow_mut().push(format!("{}:{text}", from.0.0))
        },
    ));
    log
}

/// Captures the named events `names` reach a window's `onEventJson` listeners, as `name:from:payload`.
fn capture_events(w: WindowRef, names: &[&str]) -> std::rc::Rc<std::cell::RefCell<Vec<String>>> {
    let log: std::rc::Rc<std::cell::RefCell<Vec<String>>> = Default::default();
    for name in names {
        let l = log.clone();
        let n = name.to_string();
        w.on_event_json(
            name,
            ts_api::Callback::from_fn(
                move |(from, body): (ts_api::callback::HandleArg<WindowRef>, String)| {
                    l.borrow_mut().push(format!("{n}:{}:{body}", from.0.0))
                },
            ),
        );
    }
    log
}

fn deliver(w: WindowRef, now: f64) {
    window::deliver(window::DomainId(w.0), now);
}

// M9-TS-2838
#[test]
fn post_message_sends_the_static_type_as_json() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        let log = capture(w);
        assert_eq!(h, 2.0);
        assert_eq!(text(ts_win_familyInfo(h)), "2|true|2|false");
        assert!(ts_win_postSelect(h));
        assert!(log.borrow().is_empty(), "queued, not delivered");
        assert_eq!(window::pending(window::DomainId(2)), 1);
        deliver(w, 0.0);
        assert_eq!(
            *log.borrow(),
            ["1:{\"kind\":\"select\",\"p\":[1,2,3],\"ids\":[1,2],\"name\":\"Player\"}"]
        );
    });
}

// M9-TS-2839
#[test]
fn on_message_parses_into_the_declared_type() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        ts_win_listenSelect(h);
        ts_win_listenInferred(h);
        let _ = ts_api::callback::take_errors();
        assert!(w.post_message_json(
            "{\"kind\":\"select\",\"p\":[1,2.5,3],\"ids\":[4,5,6],\"name\":\"N\"}"
        ));
        deliver(w, 0.0);
        assert_eq!(
            text(ts_win_receivedMessages()),
            "1:select|2.5|3|N;inferred 3;"
        );
        // A message that does not have the shape throws inside the listener, which the engine
        // logs; the listener's body does not run.
        assert!(w.post_message_json("{\"kind\":5}"));
        deliver(w, 0.0);
        assert_eq!(
            text(ts_win_receivedMessages()),
            "1:select|2.5|3|N;inferred 3;"
        );
        assert!(!ts_api::callback::take_errors().is_empty());
    });
}

// M9-TS-2840
#[test]
fn posting_a_handle_is_a_compile_error() {
    let src = "export function bad(): boolean {\n  const go = new GameObject(\"x\");\n  return Window.main.postMessage({ go: go, n: 1 });\n}\n";
    let manifests: Vec<String> = {
        let mut all = vec![platypus_scene::api::scene_api::MANIFEST.to_json()];
        all.extend(platypus_engine_api::manifests());
        all
    };
    let Err(e) = web_ui_ts::compile_with_manifests(src, "speed", "bad_", 7500, &manifests) else {
        panic!("a handle in a message compiled")
    };
    assert!(
        e.contains("`message.go`") && e.contains("GameObject handle"),
        "{e}"
    );
    // The same through a nested array and a request's data.
    let src = "export function bad2(): number {\n  const go = new GameObject(\"x\");\n  Window.main.request<number>({ list: [{ target: go }] });\n  return 0;\n}\n";
    let Err(e) = web_ui_ts::compile_with_manifests(src, "speed", "bad_", 7500, &manifests) else {
        panic!("a handle in a request compiled")
    };
    assert!(e.contains("`message.list[].target`"), "{e}");
    // A payload without handles compiles.
    let ok = "export function good(): boolean {\n  return Window.main.postMessage({ n: 1, v: new Vector3(1, 2, 3), tags: [\"a\"] });\n}\n";
    web_ui_ts::compile_with_manifests(ok, "speed", "bad_", 7500, &manifests).unwrap();
    // `request` needs its reply type, and the reply may not hold handles either.
    let src = "export function bad4(): number {
  Window.main.request({ n: 1 });
  return 0;
}
";
    let Err(e) = web_ui_ts::compile_with_manifests(src, "speed", "bad_", 7500, &manifests) else {
        panic!("a request without a reply type compiled")
    };
    assert!(e.contains("needs the reply type"), "{e}");
    let src = "export function bad5(): number {
  Window.main.request<{ who: GameObject }>(1);
  return 0;
}
";
    let Err(e) = web_ui_ts::compile_with_manifests(src, "speed", "bad_", 7500, &manifests) else {
        panic!("a handle in a reply compiled")
    };
    assert!(e.contains("`reply.who`"), "{e}");
    // The type must be given or inferable.
    let src = "export function bad3(): number {\n  return Window.main.onMessage((from, msg) => { });\n}\n";
    let Err(e) = web_ui_ts::compile_with_manifests(src, "speed", "bad_", 7500, &manifests) else {
        panic!("an untyped listener compiled")
    };
    assert!(e.contains("cannot infer the type argument `T`"), "{e}");
}

// M9-TS-2841
#[test]
fn request_resolves_with_the_typed_reply() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        ts_win_serve(h);
        ts_win_resetAsk();
        let p = ts_win_ask(h, f64::NAN);
        assert!(!p.is_null());
        // The request waits in the inspector's queue; its listener runs in its own domain.
        assert_eq!(text(ts_win_askLog()), "");
        deliver(w, 0.0);
        assert_eq!(text(ts_win_requestLog()), "1:stats;");
        assert_eq!(
            text(ts_win_askLog()),
            "",
            "the reply travels back to the main domain"
        );
        window::deliver(window::DomainId::MAIN, 1.0);
        web_ui_rt::ts_rt_microtasks_run();
        assert_eq!(text(ts_win_askLog()), "ok 5");
    });
}

// M9-TS-2842
#[test]
fn request_rejects_on_timeout_and_on_close() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        ts_win_serveQuiet(h);
        ts_win_resetAsk();
        ts_win_ask(h, 50.0);
        deliver(w, 0.0);
        window::deliver(window::DomainId::MAIN, 49.0);
        web_ui_rt::ts_rt_microtasks_run();
        assert_eq!(text(ts_win_askLog()), "", "not yet");
        window::deliver(window::DomainId::MAIN, 51.0);
        web_ui_rt::ts_rt_microtasks_run();
        assert_eq!(text(ts_win_askLog()), "error timed out");

        // The target closes while the request is outstanding.
        let h2 = ts_win_createInspector();
        let w2 = WindowRef(h2 as u32);
        ts_win_serveQuiet(h2);
        ts_win_resetAsk();
        ts_win_ask(h2, f64::NAN);
        deliver(w2, 60.0);
        window::closed(w2);
        window::deliver(window::DomainId::MAIN, 61.0);
        web_ui_rt::ts_rt_microtasks_run();
        assert_eq!(text(ts_win_askLog()), "error window closed");

        // No request listener in the domain.
        let h3 = ts_win_createInspector();
        ts_win_resetAsk();
        ts_win_ask(h3, f64::NAN);
        deliver(WindowRef(h3 as u32), 70.0);
        window::deliver(window::DomainId::MAIN, 71.0);
        web_ui_rt::ts_rt_microtasks_run();
        assert_eq!(text(ts_win_askLog()), "error no request listener");
    });
}

// M9-TS-2843
#[test]
fn broadcast_and_the_domain_listener_receive_messages() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        ts_win_listenDomain();
        // Posted from the inspector's domain: reaches the main domain's `Window.onMessage`.
        {
            let _d = window::enter_domain(window::DomainId(h as u32));
            assert_eq!(ts_win_broadcastHi(), 1.0);
        }
        window::deliver(window::DomainId::MAIN, 0.0);
        assert_eq!(text(ts_win_domainMessages()), "2:7;");
        // From the main family: the inspector gets it, the sender's own domain does not.
        assert_eq!(ts_win_broadcastHi(), 1.0);
        assert_eq!(window::pending(window::DomainId::MAIN), 0);
        assert_eq!(window::pending(window::DomainId(h as u32)), 1);
    });
}

fn with_domain_hooks(f: impl FnOnce()) {
    web_ui_rt::ts_rt_set_domain_hooks(
        Some(window::runtime_current_domain),
        Some(window::runtime_enter_domain),
        Some(window::runtime_leave_domain),
    );
    f();
    web_ui_rt::ts_rt_set_domain_hooks(None, None, None);
}

/// Timer clocks only move forward; each run starts later than the last.
fn clock() -> f64 {
    thread_local!(static RUNS: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) });
    RUNS.with(|r| {
        r.set(r.get() + 1.0);
        r.get() * 10_000_000.0
    })
}

// M9-TS-2831
#[test]
fn a_timer_fires_in_the_domain_that_set_it() {
    in_every_gc_mode(|| {
        with_domain_hooks(|| unsafe {
            let base = clock();
            web_ui_rt::ts_rt_timers_advance(base);
            let h = ts_win_createInspector();
            ts_win_timerLogClear();
            {
                let _d = window::enter_domain(window::DomainId(h as u32));
                ts_win_timerOnce();
                ts_win_microtaskHere();
            }
            ts_win_timerOnce();
            // The main domain's advance leaves the inspector's timer alone.
            web_ui_rt::ts_rt_timers_advance_domain(base + 100.0, 1);
            // (The promise job queued in the inspector's domain ran then too, in its own domain.)
            assert_eq!(text(ts_win_timerLogText()), format!("t1;m{};", h as u32));
            web_ui_rt::ts_rt_timers_advance_domain(base + 100.0, h as u32);
            assert_eq!(
                text(ts_win_timerLogText()),
                format!("t1;m{0};t{0};", h as u32),
                "it ran with its own domain current"
            );
        });
    });
}

// M9-TS-2832
#[test]
fn a_paused_domains_timers_wait_and_intervals_run_once_on_resume() {
    in_every_gc_mode(|| {
        with_domain_hooks(|| unsafe {
            let base = clock();
            web_ui_rt::ts_rt_timers_advance(base);
            let h = ts_win_createInspector();
            let d = h as u32;
            ts_win_timerLogClear();
            {
                let _g = window::enter_domain(window::DomainId(d));
                ts_win_timerEvery();
            }
            // Paused: the host does not advance the domain, whatever the clock says.
            web_ui_rt::ts_rt_timers_advance_domain(base + 5.0, 1);
            web_ui_rt::ts_rt_timers_advance_domain(base + 5_000.0, 1);
            assert_eq!(text(ts_win_timerLogText()), "");
            // The first advance after the resume fires it once, not once per missed period.
            web_ui_rt::ts_rt_timers_advance_domain(base + 5_000.0, d);
            assert_eq!(text(ts_win_timerLogText()), format!("i{d};"));
            web_ui_rt::ts_rt_timers_advance_domain(base + 5_005.0, d);
            assert_eq!(text(ts_win_timerLogText()), format!("i{d};"), "not due yet");
            web_ui_rt::ts_rt_timers_advance_domain(base + 5_011.0, d);
            assert_eq!(text(ts_win_timerLogText()), format!("i{d};i{d};"));
            ts_win_timerStop();
        });
    });
}

// M9-API-1456
#[test]
fn compiled_code_sees_open_failures_open_error_and_is_supported() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createAndWatchOpen();
        assert_eq!(h, 2.0);
        // Before the host answers: no error, supported.
        assert_eq!(text(ts_win_openErrorOfSecond()), "null");
        assert_eq!(text(ts_win_openReport()), "0|-|true|true");
        window::set_supported(false);
        let error = "Cannot open Window due to platform restrictions (platform: ios)";
        window::open_failed(WindowRef(2), error);
        assert_eq!(text(ts_win_openReport()), format!("1|{error}|false|true"));
        assert_eq!(
            window::state(WindowRef(2)).unwrap().open_error.as_deref(),
            Some(error)
        );
        assert_eq!(text(ts_win_openErrorOfSecond()), "gone");
    });
}

// M9-TS-2854
#[test]
fn request_replies_decode_declared_interfaces_aliases_arrays_and_nesting() {
    in_every_gc_mode(|| unsafe {
        type Serve = unsafe extern "C" fn(f64);
        type Ask = unsafe extern "C" fn(f64) -> *mut Promise_number;
        ts_win_typedLogClear();
        let cases: [(Serve, Ask); 5] = [
            (ts_win_serveStats, ts_win_askStats),
            (ts_win_servePair, ts_win_askPair),
            (ts_win_servePairs, ts_win_askPairs),
            (ts_win_serveNums, ts_win_askNums),
            (ts_win_serveNested, ts_win_askNested),
        ];
        for (serve, ask) in cases {
            let h = ts_win_createInspector();
            serve(h);
            let p = ask(h);
            assert!(!p.is_null());
            deliver(WindowRef(h as u32), 0.0);
            window::deliver(window::DomainId::MAIN, 1.0);
            web_ui_rt::ts_rt_microtasks_run();
        }
        assert_eq!(
            text(ts_win_typedLogText()),
            "iface 5|stats,x|true|2.5;alias 4|pair;arr 2|two;nums 3;nested 2|8|true;"
        );
    });
}

// ---- Named events (emit / on / ask / answer) -------------------------------------------------

/// Delivers to the window, then back to the main domain, then runs the microtasks (one ask round).
fn round(w: WindowRef, now: f64) {
    deliver(w, now);
    window::deliver(window::DomainId::MAIN, now + 1.0);
    web_ui_rt::ts_rt_microtasks_run();
}

// M9-TS-2855
#[test]
fn emit_reaches_on_by_name_with_the_handlers_declared_type() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        let log = capture(w);
        let wire = capture_events(w, &["ping", "tags"]);
        ts_win_listenEvents(h);
        ts_win_eventLogClear();
        let _ = ts_api::callback::take_errors();
        assert!(ts_win_emitStats(h));
        assert!(ts_win_emitPing(h));
        assert!(ts_win_emitTags(h));
        // An event nobody listens for, and a raw message that is no event: ignored without errors.
        assert!(ts_win_emitOther(h));
        assert!(w.post_message_json("{\"kind\":5}"));
        deliver(w, 0.0);
        assert_eq!(
            text(ts_win_eventLogText()),
            "stats 3|a,b|2.5|from 1;ping;tags 3;"
        );
        assert!(ts_api::callback::take_errors().is_empty());
        // The raw listener sees the raw message only, never an event envelope (M9-TS-2864).
        assert_eq!(*log.borrow(), ["1:{\"kind\":5}"]);
        // The payload text reaches the event listeners ("" without one).
        assert_eq!(*wire.borrow(), ["ping:1:", "tags:1:[\"x\",\"y\",\"z\"]"]);
        // off(token) removes just that listener.
        assert!(ts_win_stopStats(h));
        ts_win_eventLogClear();
        assert!(ts_win_emitStats(h));
        assert!(ts_win_emitPing(h));
        deliver(w, 1.0);
        assert_eq!(text(ts_win_eventLogText()), "ping;");
    });
}

// M9-TS-2856
#[test]
fn ask_resolves_with_the_answer_of_the_named_handler() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        ts_win_answerEvents(h);
        ts_win_eventLogClear();
        let _ = ts_api::callback::take_errors();
        let p = ts_win_askEvents(h);
        assert!(!p.is_null());
        for i in 0..4 {
            round(w, f64::from(i) * 10.0);
        }
        assert_eq!(
            text(ts_win_eventLogText()),
            "ask stats 3|abc|true;ask pair 42|twice;ask count 4;ask whoami inspector;"
        );
        assert!(ts_api::callback::take_errors().is_empty());
    });
}

// M9-TS-2857
#[test]
fn ask_of_a_name_nobody_answers_fails_at_once() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        ts_win_answerEvents(h);
        ts_win_eventLogClear();
        ts_win_askUnknown(h);
        round(w, 0.0);
        assert_eq!(
            text(ts_win_eventLogText()),
            "ask error no answer for \"nobody\";",
            "no waiting for the 50 ms timeout"
        );
        // A window with no listener at all, and one with only a raw request listener (M9-TS-2864):
        // the same error.
        let h2 = ts_win_createInspector();
        ts_win_serveQuiet(h2);
        for target in [h2, ts_win_createInspector()] {
            ts_win_eventLogClear();
            ts_win_askUnknown(target);
            round(WindowRef(target as u32), 100.0);
            assert_eq!(
                text(ts_win_eventLogText()),
                "ask error no answer for \"nobody\";"
            );
        }
        assert_eq!(
            text(ts_win_requestLog()),
            "",
            "the raw listener never saw it"
        );
    });
}

// M9-TS-2858
#[test]
fn event_names_must_not_be_empty_and_the_size_limit_applies() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        // Registering with an empty name throws a clear error.
        assert_eq!(
            text(ts_win_onEmpty(h)),
            "Window event names must not be empty"
        );
        // Over 1 MiB as JSON: refused like postMessage.
        assert!(!ts_win_emitHuge(h));
        assert_eq!(window::pending(window::DomainId(h as u32)), 0);
        ts_win_answerEvents(h);
        ts_win_eventLogClear();
        ts_win_askHuge(h);
        round(w, 0.0);
        assert_eq!(
            text(ts_win_eventLogText()),
            "ask error request refused: the window is closed or the message is over 1 MiB;"
        );
    });
}

// M9-TS-2859
#[test]
fn emit_all_reaches_the_other_family_as_a_named_event() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        let raw = capture(w);
        let log = capture_events(w, &["hi", "bare"]);
        assert_eq!(ts_win_emitAllHi(), 1.0);
        assert_eq!(ts_win_emitAllBare(), 1.0);
        deliver(w, 0.0);
        assert_eq!(*log.borrow(), ["hi:1:{\"n\":7}", "bare:1:"]);
        assert!(raw.borrow().is_empty(), "raw listeners never see events");
    });
}

// M9-TS-2860
#[test]
fn named_events_refuse_handles_and_ask_needs_its_reply_type() {
    let manifests: Vec<String> = {
        let mut all = vec![platypus_scene::api::scene_api::MANIFEST.to_json()];
        all.extend(platypus_engine_api::manifests());
        all
    };
    let compile =
        |src: &str| web_ui_ts::compile_with_manifests(src, "speed", "bad_", 7500, &manifests);
    let Err(e) = compile(
        "export function a(): boolean {\n  const go = new GameObject(\"x\");\n  return Window.main.emit(\"pick\", { go: go });\n}\n",
    ) else {
        panic!("a handle in an event compiled")
    };
    assert!(
        e.contains("`message.go`") && e.contains("GameObject handle"),
        "{e}"
    );
    let Err(e) =
        compile("export function b(): number {\n  Window.main.ask(\"x\", 1);\n  return 0;\n}\n")
    else {
        panic!("ask without a reply type compiled")
    };
    assert!(e.contains("needs the reply type"), "{e}");
    let Err(e) =
        compile("export function c(): number {\n  return Window.main.on(\"x\", (from) => 1);\n}\n")
    else {
        panic!("an untyped event handler compiled")
    };
    assert!(e.contains("cannot infer the type argument"), "{e}");
    compile(
        "export function ok(): number {\n  Window.main.ask<number>(\"x\", { n: 1 });\n  return Window.main.on(\"y\", (v: { n: number }) => { });\n}\n",
    )
    .unwrap();
}

// M9-TS-2894
#[test]
fn an_async_answer_handler_replies_when_its_promise_resolves_and_a_rejection_fails_the_ask() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        ts_win_answerAsync(h);
        ts_win_eventLogClear();
        let _ = ts_api::callback::take_errors();
        let p = ts_win_askAsync(h);
        assert!(!p.is_null());
        for i in 0..12 {
            round(w, f64::from(i) * 10.0);
        }
        assert_eq!(
            text(ts_win_eventLogText()),
            "double 42;label async-label;stats 4|abcd;boom error boom 5;"
        );
        assert!(ts_api::callback::take_errors().is_empty());
    });
}

// M9-TS-2895
#[test]
fn an_async_raw_request_handler_can_reply_later_and_its_rejection_fails_the_request() {
    in_every_gc_mode(|| unsafe {
        let h = ts_win_createInspector();
        let w = WindowRef(h as u32);
        ts_win_serveAsync(h);
        ts_win_eventLogClear();
        let _ = ts_api::callback::take_errors();
        let p = ts_win_requestAsync(h);
        assert!(!p.is_null());
        for i in 0..12 {
            round(w, f64::from(i) * 10.0);
        }
        assert_eq!(
            text(ts_win_eventLogText()),
            "request 42;request error negative -3;"
        );
        assert!(ts_api::callback::take_errors().is_empty());
    });
}
