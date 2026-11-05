// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Messages between windows (design section 3b): the queue per domain, delivery order, requests and
//! replies, broadcast. IDs are in TESTS.md (M9-API-14xx).
use platypus_engine_api::log::{self, Level};
use platypus_engine_api::window::{self, DomainId, MAIN, MAX_MESSAGE_BYTES, Style, WindowRef};
use std::cell::RefCell;
use std::rc::Rc;
use ts_api::Callback;
use ts_api::callback::HandleArg;

fn fresh() {
    window::reset();
    let _ = window::take_requests();
    window::end_frame();
}

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

type Log = Rc<RefCell<Vec<String>>>;

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

/// A message listener that logs `who:from:text`.
fn logger(log: &Log, who: &'static str) -> Callback<(HandleArg<WindowRef>, String)> {
    let l = log.clone();
    Callback::from_fn(move |(from, text): (HandleArg<WindowRef>, String)| {
        l.borrow_mut().push(format!("{who}:{}:{text}", from.0.0))
    })
}

// M9-API-1498
#[test]
fn messages_arrive_at_the_next_delivery_in_order_per_sender() {
    fresh();
    let tool = make("tool", WindowRef::NONE);
    let iso = make("normal", WindowRef::NONE);
    let d = DomainId(iso.0);
    let log: Log = Rc::default();
    iso.on_message_json(logger(&log, "win"));
    {
        let _d = window::enter_domain(d);
        WindowRef::on_message_json_all(logger(&log, "dom"));
    }
    // Two senders interleave; nothing arrives before `deliver`.
    assert!(iso.post_message_json("1"));
    {
        let _c = window::enter_current(tool);
        assert!(iso.post_message_json("2"));
    }
    assert!(iso.post_message_json("3"));
    assert_eq!(window::pending(d), 3);
    assert!(log.borrow().is_empty());
    window::deliver(d, 0.0);
    // The target's listeners first, then the domain's, message by message; `from` is the window
    // current when it was posted.
    assert_eq!(
        *log.borrow(),
        [
            "win:1:1", "dom:1:1", "win:2:2", "dom:2:2", "win:1:3", "dom:1:3"
        ]
    );
    assert_eq!(window::pending(d), 0);

    // A message posted by a listener during delivery waits for the next delivery.
    log.borrow_mut().clear();
    let again = Rc::new(std::cell::Cell::new(true));
    let a = again.clone();
    iso.on_message_json(Callback::from_fn(
        move |_: (HandleArg<WindowRef>, String)| {
            if a.replace(false) {
                iso.post_message_json("late");
            }
        },
    ));
    iso.post_message_json("first");
    window::deliver(d, 0.0);
    assert_eq!(
        log.borrow().len(),
        2,
        "win and dom saw 'first' only: {:?}",
        log.borrow()
    );
    assert_eq!(window::pending(d), 1);
    window::deliver(d, 0.0);
    assert!(log.borrow().iter().any(|m| m.ends_with(":late")));
    assert_eq!(window::pending(d), 0);

    // Over 1 MiB is refused and logged; exactly 1 MiB is not.
    let big = "x".repeat(MAX_MESSAGE_BYTES + 1);
    let w = warnings(|| assert!(!iso.post_message_json(&big)));
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(window::pending(d), 0);
    assert!(iso.post_message_json(&"x".repeat(MAX_MESSAGE_BYTES)));
    window::deliver(d, 0.0);

    // A closed target refuses; a message queued before it closed is dropped.
    iso.post_message_json("doomed");
    window::closed(iso);
    log.borrow_mut().clear();
    window::deliver(d, 0.0);
    assert!(log.borrow().is_empty());
    let w = warnings(|| assert!(!iso.post_message_json("x")));
    assert_eq!(w.len(), 1, "{w:?}");
}

/// What `onReplyJson` saw: `id|text|error`.
fn reply_log(log: &Log) {
    let l = log.clone();
    WindowRef::on_reply_json(Callback::from_fn(
        move |(id, text, err): (f64, String, String)| {
            l.borrow_mut().push(format!("{id}|{text}|{err}"));
        },
    ));
}

// M9-API-1499
#[test]
fn requests_get_one_reply_or_time_out_and_broadcast_reaches_other_roots() {
    fresh();
    let server = make("normal", WindowRef::NONE);
    let sd = DomainId(server.0);
    let replies: Log = Rc::default();
    reply_log(&replies);

    // A request is answered once; the reply travels to the sender's domain.
    let seen_by_server: Log = Rc::default();
    let l = seen_by_server.clone();
    server.on_request_json(Callback::from_fn(
        move |(from, text, id): (HandleArg<WindowRef>, String, f64)| {
            l.borrow_mut().push(format!("{}:{text}:{id}", from.0.0));
            assert!(WindowRef::reply_json(id, "\"pong\""));
            assert!(!WindowRef::reply_json(id, "\"again\""), "once");
        },
    ));
    let id = server.post_request_json("\"ping\"", f64::NAN);
    assert_eq!(id, 1.0);
    window::deliver(sd, 0.0);
    assert_eq!(*seen_by_server.borrow(), ["1:\"ping\":1"]);
    assert!(
        replies.borrow().is_empty(),
        "the sender's domain has not run yet"
    );
    assert_eq!(window::pending(DomainId::MAIN), 1);
    window::deliver(DomainId::MAIN, 0.0);
    assert_eq!(*replies.borrow(), ["1|\"pong\"|"]);
    assert!(
        !WindowRef::reply_json(id, "\"late\""),
        "an answered request is gone"
    );

    // Nobody listens: "no request listener".
    replies.borrow_mut().clear();
    let mute = make("normal", WindowRef::NONE);
    let id2 = mute.post_request_json("1", 5000.0);
    assert_eq!(id2, 2.0);
    window::deliver(DomainId(mute.0), 0.0);
    window::deliver(DomainId::MAIN, 0.0);
    assert_eq!(*replies.borrow(), ["2||no request listener"]);

    // A listener that never replies: the timeout fails it, once, in the sender's own delivery.
    replies.borrow_mut().clear();
    let quiet = make("normal", WindowRef::NONE);
    quiet.on_request_json(Callback::from_fn(
        |_: (HandleArg<WindowRef>, String, f64)| {},
    ));
    window::deliver(DomainId::MAIN, 100.0);
    let id3 = quiet.post_request_json("1", 1000.0);
    window::deliver(DomainId(quiet.0), 200.0);
    window::deliver(DomainId::MAIN, 1099.0);
    assert!(replies.borrow().is_empty(), "not yet: the deadline is 1100");
    window::deliver(DomainId::MAIN, 1100.0);
    assert_eq!(*replies.borrow(), [format!("{id3}||timed out")]);
    window::deliver(DomainId::MAIN, 5000.0);
    assert_eq!(replies.borrow().len(), 1, "failed once");
    assert!(!WindowRef::reply_json(id3, "\"too late\""));

    // The target closes before it answers: "window closed".
    replies.borrow_mut().clear();
    let gone = make("normal", WindowRef::NONE);
    gone.on_request_json(Callback::from_fn(
        |_: (HandleArg<WindowRef>, String, f64)| {},
    ));
    let id4 = gone.post_request_json("1", f64::NAN);
    window::deliver(DomainId(gone.0), 5000.0);
    window::closed(gone);
    window::deliver(DomainId::MAIN, 5001.0);
    assert_eq!(*replies.borrow(), [format!("{id4}||window closed")]);
    // And a request to a window already closed is refused at once.
    let w = warnings(|| assert_eq!(gone.post_request_json("1", f64::NAN), 0.0));
    assert_eq!(w.len(), 1);

    // A reply may come later, from the domain-wide listener, after the request was delivered.
    replies.borrow_mut().clear();
    let slow = make("normal", WindowRef::NONE);
    let held = Rc::new(RefCell::new(None));
    {
        let h = held.clone();
        let _d = window::enter_domain(DomainId(slow.0));
        WindowRef::on_request_json_all(Callback::from_fn(
            move |(_, _, id): (HandleArg<WindowRef>, String, f64)| *h.borrow_mut() = Some(id),
        ));
    }
    let id5 = slow.post_request_json("1", f64::NAN);
    window::deliver(DomainId(slow.0), 6000.0);
    assert!(held.borrow().is_some());
    window::deliver(DomainId::MAIN, 6001.0);
    assert!(replies.borrow().is_empty(), "still waiting");
    assert!(WindowRef::reply_json(held.borrow().unwrap(), "{\"n\":1}"));
    window::deliver(DomainId::MAIN, 6002.0);
    assert_eq!(*replies.borrow(), [format!("{id5}|{{\"n\":1}}|")]);

    // Broadcast reaches the root of every other family, never the sender's own.
    fresh();
    let a = make("normal", WindowRef::NONE);
    let b = make("borderless", WindowRef::NONE);
    let tool = make("tool", WindowRef::NONE);
    let log: Log = Rc::default();
    for (w, who) in [(MAIN, "main"), (a, "a"), (b, "b"), (tool, "tool")] {
        w.on_message_json(logger(&log, who));
    }
    // From the main family: a and b, not main or its tool window.
    assert_eq!(WindowRef::broadcast_json("\"hi\""), 2);
    assert_eq!(window::pending(DomainId::MAIN), 0);
    window::deliver(DomainId(a.0), 0.0);
    window::deliver(DomainId(b.0), 0.0);
    assert_eq!(*log.borrow(), ["a:1:\"hi\"", "b:1:\"hi\""]);
    // From an isolated family: main and the other isolated one.
    log.borrow_mut().clear();
    {
        let _d = window::enter_domain(DomainId(a.0));
        assert_eq!(WindowRef::broadcast_json("\"yo\""), 2);
    }
    window::deliver(DomainId::MAIN, 0.0);
    window::deliver(DomainId(b.0), 0.0);
    window::deliver(DomainId(a.0), 0.0);
    assert_eq!(
        *log.borrow(),
        [format!("main:{}:\"yo\"", a.0), format!("b:{}:\"yo\"", a.0)]
    );
    assert_eq!(window::pending(DomainId(a.0)), 0);
}

// M9-TS-2866
#[test]
fn named_events_are_their_own_layer_and_unanswered_names_fail_at_once() {
    fresh();
    let w = make("normal", WindowRef::NONE);
    let wd = DomainId(w.0);
    let replies: Log = Rc::default();
    reply_log(&replies);
    let raw: Log = Rc::default();
    w.on_message_json(logger(&raw, "raw"));
    w.on_request_json(Callback::from_fn(
        |(_, _, id): (HandleArg<WindowRef>, String, f64)| {
            WindowRef::reply_json(id, "\"raw\"");
        },
    ));
    let events: Log = Rc::default();
    w.on_event_json("a", logger(&events, "a"));
    w.on_event_json("b", logger(&events, "b"));
    let tok = w.on_answer_json(
        "q",
        Callback::from_fn(|(_, body, id): (HandleArg<WindowRef>, String, f64)| {
            WindowRef::reply_json(id, &format!("[{body}]"));
        }),
    );
    assert_ne!(tok, 0);
    assert_eq!(w.on_event_json("", logger(&events, "x")), 0, "empty name");

    assert!(w.post_message_json("{\"$e\":\"a\",\"d\":{\"n\":1}}"));
    assert!(w.post_message_json("{\"$e\":\"b\"}"));
    assert!(w.post_message_json("{\"$e\":\"zzz\"}"));
    assert!(w.post_message_json("{\"kind\":1}"));
    assert!(w.post_message_json("{\"$e\":\"a\" oops"));
    assert_eq!(w.post_request_json("{\"$e\":\"q\",\"d\":5}", f64::NAN), 1.0);
    assert_eq!(w.post_request_json("{\"$e\":\"nope\"}", f64::NAN), 2.0);
    assert_eq!(w.post_request_json("{\"plain\":true}", f64::NAN), 3.0);
    window::deliver(wd, 0.0);
    window::deliver(DomainId::MAIN, 1.0);
    // Events go to their name's listeners; malformed envelopes and plain texts are raw messages.
    assert_eq!(*events.borrow(), ["a:1:{\"n\":1}", "b:1:"]);
    assert_eq!(
        *raw.borrow(),
        ["raw:1:{\"kind\":1}", "raw:1:{\"$e\":\"a\" oops"]
    );
    // An answer for `q`; `nope` fails at once; the plain request reaches the raw listener only.
    assert_eq!(
        *replies.borrow(),
        ["1|[5]|", "2||no answer for \"nope\"", "3|\"raw\"|"]
    );
    // off(token) removes an answer: the name then fails like an unknown one.
    assert!(w.off(f64::from(tok)));
    replies.borrow_mut().clear();
    assert_eq!(w.post_request_json("{\"$e\":\"q\"}", f64::NAN), 4.0);
    window::deliver(wd, 2.0);
    window::deliver(DomainId::MAIN, 3.0);
    assert_eq!(*replies.borrow(), ["4||no answer for \"q\""]);
}

// M9-TS-2867
#[test]
fn parse_event_reads_the_envelope_and_nothing_else() {
    use platypus_engine_api::window::parse_event;
    assert_eq!(parse_event("{\"$e\":\"x\"}"), Some(("x".into(), "")));
    assert_eq!(
        parse_event("{\"$e\":\"a\\\"b\",\"d\":{\"k\":[1,2]}}"),
        Some(("a\"b".into(), "{\"k\":[1,2]}"))
    );
    assert_eq!(
        parse_event("{\"$e\":\"x\",\"d\":null}"),
        Some(("x".into(), "null"))
    );
    for not in [
        "{\"kind\":1}",
        "\"x\"",
        "{\"$e\":1}",
        "{\"$e\":\"x\"",
        "{\"$e\":\"x\",\"d\":1",
        "{\"$e\":\"x\",\"e\":1}",
        "{\"d\":1,\"$e\":\"x\"}",
    ] {
        assert_eq!(parse_event(not), None, "{not}");
    }
}

/// Promise callbacks that log `ok:text` / `err:reason`.
fn settle_log(log: &Log) -> (Callback<(String,)>, Callback<(String,)>) {
    let (a, b) = (log.clone(), log.clone());
    (
        Callback::from_fn(move |(t,): (String,)| a.borrow_mut().push(format!("ok:{t}"))),
        Callback::from_fn(move |(e,): (String,)| b.borrow_mut().push(format!("err:{e}"))),
    )
}

// M9-API-1646
#[test]
fn the_engine_writes_event_envelopes_and_settles_request_promises_itself() {
    use platypus_engine_api::window::event_text;
    assert_eq!(event_text("a\"b", ""), "{\"$e\":\"a\\\"b\"}");
    assert_eq!(event_text("x", "[1]"), "{\"$e\":\"x\",\"d\":[1]}");
    fresh();
    let w = make("normal", WindowRef::NONE);
    let wd = DomainId(w.0);
    let events: Log = Rc::default();
    w.on_event_json("hi", logger(&events, "hi"));
    w.on_answer_json(
        "double",
        Callback::from_fn(|(_, body, id): (HandleArg<WindowRef>, String, f64)| {
            let n: f64 = body.parse().unwrap();
            WindowRef::reply_json(id, &(n * 2.0).to_string());
        }),
    );
    w.on_request_json(Callback::from_fn(
        |(_, text, id): (HandleArg<WindowRef>, String, f64)| {
            WindowRef::reply_json(id, &format!("[{text}]"));
        },
    ));
    // `onReplyJson` listeners never see what a promise settles.
    let replies: Log = Rc::default();
    reply_log(&replies);
    let settled: Log = Rc::default();
    assert!(w.emit_json("hi", "1") && w.emit_json("hi", ""));
    assert!(!w.emit_json("", "1"), "an empty name is refused");
    let (ok, err) = settle_log(&settled);
    w.ask_json("double", "21", f64::NAN, ok, err);
    let (ok, err) = settle_log(&settled);
    w.ask_json("nobody", "", f64::NAN, ok, err);
    let (ok, err) = settle_log(&settled);
    w.request_json("5", f64::NAN, ok, err);
    let (ok, err) = settle_log(&settled);
    w.ask_json("", "", f64::NAN, ok, err);
    let (ok, err) = settle_log(&settled);
    w.request_json(&"x".repeat(MAX_MESSAGE_BYTES + 1), f64::NAN, ok, err);
    // Refusals settle at once.
    assert_eq!(
        *settled.borrow(),
        [
            "err:Window event names must not be empty",
            "err:request refused: the window is closed or the message is over 1 MiB"
        ]
    );
    settled.borrow_mut().clear();
    window::deliver(wd, 0.0);
    window::deliver(DomainId::MAIN, 1.0);
    assert_eq!(*events.borrow(), ["hi:1:1", "hi:1:"]);
    assert_eq!(
        *settled.borrow(),
        ["ok:42", "err:no answer for \"nobody\"", "ok:[5]"]
    );
    assert!(replies.borrow().is_empty());
    // A request nobody answers times out in the sender's domain.
    settled.borrow_mut().clear();
    let quiet = make("normal", WindowRef::NONE);
    let (ok, err) = settle_log(&settled);
    quiet.ask_json("x", "", 50.0, ok, err);
    window::deliver(DomainId::MAIN, 100.0);
    assert_eq!(*settled.borrow(), ["err:timed out"]);
    assert_eq!(WindowRef::emit_all_json("theme", "true"), 2);
    assert_eq!(WindowRef::emit_all_json("", "true"), 0);
    assert_eq!(
        WindowRef::event_name_error(""),
        window::EMPTY_EVENT_NAME.to_string()
    );
    assert_eq!(WindowRef::event_name_error("x"), "");
}
