// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Compiled TSX components rendered by the native reconciler (`bench/ui.ts`): JSX, hooks, keyed
//! lists and events written as ordinary TypeScript, run through `web_ui_bridge` and `web_ui_react`
//! into a real `UiTree`. Each test runs normally and with a collection at every allocation.
//! IDs are in TESTS.md (M9-TS-575..).
use platypus_core::NodeId;
use platypus_ts_tests::uic::*;
use std::sync::Mutex;
use web_ui_dom::{Input, Interaction, ROOT, UiTree};
use web_ui_react::{Event, Runtime};
use web_ui_rt::{Str, link_anchor, ts_rt_gc_enable, ts_rt_gc_stress};

/// The scripts' globals are shared; one test at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn in_every_gc_mode(f: impl Fn()) {
    let _serial = platypus_ts_tests::serial();
    assert_ne!(link_anchor(), 0);
    assert_ne!(web_ui_bridge::link_anchor(), 0);
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { ts_uic_init() };
    f();
    ts_rt_gc_stress(true);
    unsafe { ts_uic_init() };
    f();
    ts_rt_gc_stress(false);
    ts_rt_gc_enable(false);
    unsafe { ts_uic_init() };
    f();
    ts_rt_gc_enable(true);
}

fn text(s: *mut u8) -> String {
    String::from_utf16_lossy(&unsafe { Str::from_raw(s as *const Str) }.to_utf16())
}

fn dump(tree: &UiTree, id: NodeId) -> String {
    let kind = tree.kind(id).unwrap_or("?");
    if kind == "#text" {
        return format!("text:{}", tree.text(id).unwrap_or(""));
    }
    let kids: Vec<String> = tree.children(id).iter().map(|c| dump(tree, *c)).collect();
    format!("{kind}({})", kids.join(","))
}

fn no_errors(errors: Vec<web_ui_dom::UiError>) {
    assert!(errors.is_empty(), "{errors:?}");
    let script = web_ui_bridge::take_errors();
    assert!(script.is_empty(), "{script:?}");
}

struct Ui {
    tree: UiTree,
    rt: Runtime,
    root: web_ui_react::RootId,
    inter: Interaction,
}

impl Ui {
    fn new() -> Ui {
        let mut rt = Runtime::new(1 << 30);
        let root = rt.create_root(ROOT);
        Ui {
            tree: UiTree::new(300.0, 300.0),
            rt,
            root,
            inter: Interaction::new(),
        }
    }

    fn show(&mut self, handle: u64) {
        let el = web_ui_bridge::element_from(handle);
        no_errors(self.rt.render(self.root, el, &mut self.tree));
    }

    fn click(&mut self, node: NodeId) {
        let mut ev = Event::new("click", node);
        self.rt.dispatch(&mut ev, &mut self.tree);
        no_errors(Vec::new());
    }

    fn dump(&self) -> String {
        dump(&self.tree, ROOT)
    }

    /// One input through the interaction layer, its events running the compiled handlers like the
    /// app does (`web_ui_react::event_of`). Returns the event types it sent, in order.
    fn input(&mut self, input: Input) -> Vec<&'static str> {
        let rt = &mut self.rt;
        let mut sent = Vec::new();
        self.inter.handle(&mut self.tree, input, &mut |e| {
            sent.push(e.kind.name());
            match web_ui_react::event_of(e) {
                Some(mut ev) => rt.dispatch_handlers(&mut ev),
                None => false,
            }
        });
        no_errors(self.rt.settle(&mut self.tree));
        sent
    }

    /// The compiled app's notes since the last call.
    fn notes(&self) -> String {
        text(unsafe { ts_uic_evLogText() })
    }

    fn at(&mut self, ms: f64) {
        self.tree.set_time(ms);
        self.tree.layout_if_needed();
    }
}

// M9-TS-575
//
// A component written as TSX: `useState`, a host element with a style and a click handler, text
// and a number as children. A click inside the compiled handler changes the state and the text.
#[test]
fn a_counter_component_renders_and_handles_a_click() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_counterApp() });
        assert_eq!(ui.dump(), "root(box(text(text:n=),text:3))");
        let boxn = ui.tree.children(ROOT)[0];
        ui.click(boxn);
        assert_eq!(ui.dump(), "root(box(text(text:n=),text:4))");
        ui.click(boxn);
        ui.click(boxn);
        assert_eq!(ui.dump(), "root(box(text(text:n=),text:6))");
        ui.tree.layout_if_needed();
        assert_eq!(ui.tree.rect(boxn).unwrap().width, 100.0);
    });
}

// M9-TS-576
//
// A keyed list from `items.map(...)`: reversing it moves the nodes, it does not recreate them.
#[test]
fn a_keyed_list_keeps_its_nodes_when_reordered() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_listApp(4.0, false) });
        let list = ui.tree.children(ROOT)[0];
        let before: Vec<NodeId> = ui.tree.children(list).to_vec();
        assert_eq!(
            ui.dump(),
            "root(box(text(text:k0),text(text:k1),text(text:k2),text(text:k3)))"
        );
        ui.show(unsafe { ts_uic_listApp(4.0, true) });
        let after: Vec<NodeId> = ui.tree.children(list).to_vec();
        let want: Vec<NodeId> = before.iter().rev().copied().collect();
        assert_eq!(after, want);
        ui.show(unsafe { ts_uic_listApp(2.0, false) });
        assert_eq!(ui.dump(), "root(box(text(text:k0),text(text:k1)))");
    });
}

// M9-TS-577
//
// `useEffect` with dependencies and a cleanup: the log shows +1, then -1 +2 when the dependency
// changes, nothing when it does not, and the cleanup on unmount.
#[test]
fn effects_run_with_dependencies_and_clean_up() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_fxApp(1.0) });
        assert_eq!(unsafe { ts_uic_getLog() }, 11.0);
        ui.show(unsafe { ts_uic_fxApp(1.0) });
        assert_eq!(
            unsafe { ts_uic_getLog() },
            11.0,
            "same dependency: no rerun"
        );
        ui.show(unsafe { ts_uic_fxApp(2.0) });
        assert_eq!(unsafe { ts_uic_getLog() }, 112_112.0);
        no_errors(ui.rt.unmount_root(ui.root, &mut ui.tree));
        assert_eq!(unsafe { ts_uic_getLog() }, 11_211_222.0);
    });
}

// M9-TS-578
//
// `useMemo` with dependencies: recomputed only when they change.
#[test]
fn use_memo_recomputes_only_when_its_dependencies_change() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_memoApp(3.0, 0.0) });
        assert_eq!(ui.dump(), "root(box(text:9,text:0))");
        ui.show(unsafe { ts_uic_memoApp(3.0, 5.0) });
        assert_eq!(ui.dump(), "root(box(text:9,text:5))");
        assert_eq!(unsafe { ts_uic_memoCount() }, 1.0);
        ui.show(unsafe { ts_uic_memoApp(4.0, 5.0) });
        assert_eq!(ui.dump(), "root(box(text:16,text:5))");
        assert_eq!(unsafe { ts_uic_memoCount() }, 2.0);
    });
}

// M9-TS-579
//
// Boolean and string state and `useRef`: a click flips both, the ref counts renders and survives.
#[test]
fn bool_and_string_state_and_use_ref_persist_across_renders() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_toggleApp() });
        assert_eq!(ui.dump(), "root(box(text:off,text:1))");
        let b = ui.tree.children(ROOT)[0];
        ui.click(b);
        assert_eq!(ui.dump(), "root(box(text:on,text:2))");
        ui.click(b);
        assert_eq!(ui.dump(), "root(box(text:off,text:3))");
    });
}

// M9-TS-580
//
// Context: a consumer reads the nearest provider's value, the default without one, and follows
// the provider when it changes.
#[test]
fn context_reaches_consumers_and_follows_the_provider() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_ctxNone() });
        assert_eq!(ui.dump(), "root(text(text:light))");
        ui.show(unsafe { ts_uic_ctxApp(true) });
        assert_eq!(ui.dump(), "root(text(text:dark))");
        ui.show(unsafe { ts_uic_ctxApp(false) });
        assert_eq!(ui.dump(), "root(text(text:light))");
    });
}

// M9-TS-581
//
// `ref={r}` writes the node into the ref when the element attaches (the effect sees it) and
// clears it on unmount.
#[test]
fn a_node_ref_is_set_when_the_element_attaches() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_refApp() });
        let node = ui.tree.children(ROOT)[0];
        assert_eq!(unsafe { ts_uic_getSeenNode() }, f64::from(node.raw()));
    });
}

// M9-TS-582
//
// `useImperativeHandle`: a child fills the ref its parent passed; the parent effect (which runs
// after the child's) reads it.
#[test]
fn use_imperative_handle_fills_the_ref_before_the_parents_effect() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_handleApp() });
        assert_eq!(unsafe { ts_uic_getHandleSeen() }, 42.0);
    });
}

// M9-TS-583
//
// `useSyncExternalStore`: the component shows the store, subscribes once, re-renders when the
// store notifies, and unsubscribes on unmount.
#[test]
fn use_sync_external_store_follows_a_store() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_storeApp() });
        assert_eq!(ui.dump(), "root(box(text:5))");
        assert_eq!(unsafe { ts_uic_listenerCount() }, 1.0);
        unsafe { ts_uic_bumpStore() };
        no_errors(ui.rt.flush(&mut ui.tree));
        assert_eq!(ui.dump(), "root(box(text:6))");
        assert_eq!(
            unsafe { ts_uic_listenerCount() },
            1.0,
            "not subscribed again"
        );
        no_errors(ui.rt.unmount_root(ui.root, &mut ui.tree));
        assert_eq!(unsafe { ts_uic_listenerCount() }, 0.0);
    });
}

// M9-TS-584
//
// `memo`: a child whose props are equal is not rendered when its parent renders again; a changed
// prop renders it.
#[test]
fn memo_skips_a_child_with_equal_props() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_memoParentApp(1.0, 0.0) });
        assert_eq!(ui.dump(), "root(box(text:0,box(text:1)))");
        assert_eq!(unsafe { ts_uic_leafRenders() }, 1.0);
        ui.show(unsafe { ts_uic_memoParentApp(1.0, 1.0) });
        assert_eq!(ui.dump(), "root(box(text:1,box(text:1)))");
        assert_eq!(unsafe { ts_uic_leafRenders() }, 1.0, "skipped");
        ui.show(unsafe { ts_uic_memoParentApp(2.0, 1.0) });
        assert_eq!(ui.dump(), "root(box(text:1,box(text:2)))");
        assert_eq!(unsafe { ts_uic_leafRenders() }, 2.0);
    });
}

// M9-TS-585
//
// `forwardRef`: the component gets the parent's ref as its second parameter and fills it with
// `useImperativeHandle`.
#[test]
fn forward_ref_passes_the_ref_to_the_component() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_fwdApp() });
        assert_eq!(ui.dump(), "root(text(text:hello))");
        assert_eq!(unsafe { ts_uic_getFwdSeen() }, 5.0);
    });
}

// M9-TS-586
//
// `useReducer`: dispatch applies the reducer to the latest state, so two dispatches in one
// handler both count.
#[test]
fn use_reducer_applies_actions_to_the_latest_state() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_reducerApp() });
        assert_eq!(ui.dump(), "root(box(text:10))");
        let b = ui.tree.children(ROOT)[0];
        ui.click(b);
        assert_eq!(ui.dump(), "root(box(text:11))");
        let mut ev = Event::new("pointerdown", b);
        ui.rt.dispatch(&mut ev, &mut ui.tree);
        assert_eq!(ui.dump(), "root(box(text:9))");
    });
}

/// The component table the compiler exported: name -> (id, hooks).
fn signatures() -> std::collections::HashMap<String, (u32, String)> {
    let text = unsafe { ts_api::script_str(ts_uic___uiSignatures() as *const u8) }.into_owned();
    text.split(';')
        .map(|e| {
            let mut p = e.splitn(3, ':');
            let name = p.next().unwrap().to_string();
            let id: u32 = p.next().unwrap().parse().unwrap();
            (name, (id, p.next().unwrap().to_string()))
        })
        .collect()
}

// M9-TS-587
//
// The compiler exports each component's stable id and its ordered hook list.
#[test]
fn the_compiler_exports_component_ids_and_hook_lists() {
    in_every_gc_mode(|| {
        let sig = signatures();
        assert_eq!(sig["Counter"].1, "useState");
        assert_eq!(sig["Fx"].1, "useEffect");
        assert_eq!(sig["Toggle"].1, "useState,useState,useRef");
        assert_eq!(sig["Memo"].1, "useMemo");
        assert_eq!(sig["Reader"].1, "useContext");
        assert_ne!(sig["Counter"].0, sig["Fx"].0);
    });
}

// M9-TS-588
//
// Hot reload: rendering the new code's element over the old tree keeps the state of components
// whose hook list is unchanged and starts over those in the remount list.
#[test]
fn refresh_keeps_state_unless_the_component_is_remounted() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_counterApp() });
        let b = ui.tree.children(ROOT)[0];
        ui.click(b);
        ui.click(b);
        assert_eq!(ui.dump(), "root(box(text(text:n=),text:5))");
        let id = signatures()["Counter"].0;
        let el = web_ui_bridge::element_from(unsafe { ts_uic_counterApp() });
        no_errors(ui.rt.refresh(ui.root, el, &[], &mut ui.tree));
        assert_eq!(ui.dump(), "root(box(text(text:n=),text:5))", "state kept");
        let el = web_ui_bridge::element_from(unsafe { ts_uic_counterApp() });
        no_errors(ui.rt.refresh(ui.root, el, &[id], &mut ui.tree));
        assert_eq!(
            ui.dump(),
            "root(box(text(text:n=),text:3))",
            "remounted: initial state"
        );
    });
}

// M9-TS-589
//
// The updater form of a setter: two updaters in one handler both apply to the latest state.
#[test]
fn updater_setters_apply_to_the_latest_state() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_plusApp() });
        assert_eq!(ui.dump(), "root(box(text:0))");
        let b = ui.tree.children(ROOT)[0];
        ui.click(b);
        assert_eq!(ui.dump(), "root(box(text:11))");
        ui.click(b);
        assert_eq!(ui.dump(), "root(box(text:22))");
    });
}

// M9-TS-590
//
// `style={obj}` with a typed object: its fields are the style, an optional field left out is not
// set, and a field that goes away is reset.
#[test]
fn a_dynamic_style_object_sets_its_fields() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_styledApp(40.0, true) });
        let b = ui.tree.children(ROOT)[0];
        ui.tree.layout_if_needed();
        assert_eq!(ui.tree.rect(b).unwrap().width, 40.0);
        ui.show(unsafe { ts_uic_styledApp(70.0, false) });
        ui.tree.layout_if_needed();
        assert_eq!(ui.tree.rect(b).unwrap().width, 70.0);
    });
}

// M9-TS-3525
//
// `PlatypusStyle["position"]` and other style properties' declared types (keywords, `Length`,
// numbers, `Cursor`) type values that reach the style; a property the style lacks is an error with
// the line.
#[test]
fn style_property_types_come_from_the_declared_style() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        for (abs, y, w) in [(true, 0.0, 50.0), (false, 20.0, 30.0)] {
            ui.show(unsafe { ts_uic_placedApp(abs) });
            ui.tree.layout_if_needed();
            let frame = ui.tree.children(ROOT)[0];
            let placed = ui.tree.children(frame)[1];
            let r = ui.tree.rect(placed).unwrap();
            assert_eq!((r.x, r.y, r.width), (12.0, y, w), "absolute: {abs}");
        }
    });
    let src =
        "type W = PlatypusStyle[\"nope\"];\nexport function f(w: W): UiElement { return <box />; }";
    let e = web_ui_ts::lower::lower_source(src).err().unwrap();
    assert!(
        e.contains("line 1: property \"nope\" does not exist on type PlatypusStyle"),
        "{e}"
    );
}

// M9-TS-591
//
// `{...obj}` on a host element sets the object's fields as attributes; on a component it spreads
// into the props (a later attribute wins).
#[test]
fn spread_attributes_and_props() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_spreadApp() });
        let b = ui.tree.children(ROOT)[0];
        let a = ui.tree.attrs(b).unwrap();
        assert_eq!(a.id.as_deref(), Some("main"));
        assert_eq!(a.classes, ["a", "b"]);
        assert_eq!(a.tab_index, Some(3));
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_outerApp() });
        assert_eq!(ui.dump(), "root(box(text:1,text:9))");
    });
}

// M9-TS-2011
//
// `ball ? "50%" : 4` as a style value with no type around it.
#[test]
fn a_style_value_that_is_a_string_or_a_number_by_condition() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_radiusApp(true) });
        let b = ui.tree.children(ROOT)[0];
        ui.tree.layout_if_needed();
        assert_eq!(ui.tree.rect(b).unwrap().width, 50.0);
    });
}

// M9-TS-3148
//
// A computed key in a style object (`[side]: 8`, the name a string-literal union member).
#[test]
fn a_style_key_can_be_computed() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_edgeApp(false) });
        let b = ui.tree.children(ROOT)[0];
        ui.tree.layout_if_needed();
        let r = ui.tree.rect(b).unwrap();
        assert_eq!((r.x, r.y), (8.0, 0.0));
        ui.show(unsafe { ts_uic_edgeApp(true) });
        let b = ui.tree.children(ROOT)[0];
        ui.tree.layout_if_needed();
        let r = ui.tree.rect(b).unwrap();
        assert_eq!((r.x, r.y), (0.0, 8.0));
    });
}

// M9-TS-3171
//
// A prop typed `object` is a bag of style properties (`extra?: object`, spread into `style`).
#[test]
fn an_object_typed_style_prop_is_spread_into_a_style() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_objectStyleApp(true) });
        let b = ui.tree.children(ROOT)[0];
        ui.tree.layout_if_needed();
        let r = ui.tree.rect(b).unwrap();
        assert_eq!((r.width, r.height), (20.0, 30.0));
        ui.show(unsafe { ts_uic_objectStyleApp(false) });
        let b = ui.tree.children(ROOT)[0];
        ui.tree.layout_if_needed();
        assert_eq!(ui.tree.rect(b).unwrap().width, 20.0);
    });
}

// M9-TS-2847
//
// A function that returns a number, a string, a class object, an object literal or an element is
// a valid `onClick` (TypeScript lets any return type stand for `void`); the result is dropped,
// also under a collection at every allocation.
#[test]
fn a_function_that_returns_a_value_is_an_event_handler() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_valueHandlersApp() });
        let list = ui.tree.children(ROOT)[0];
        let boxes: Vec<NodeId> = ui.tree.children(list).to_vec();
        assert_eq!(boxes.len(), 7);
        let before = text(unsafe { ts_uic_tapLogText() });
        for b in &boxes {
            ui.click(*b);
            web_ui_rt::ts_rt_gc_collect();
        }
        let after = text(unsafe { ts_uic_tapLogText() });
        assert_eq!(after[before.len()..], *"nsmoenm");
    });
}

// M9-TS-2848
//
// The same functions as callback arguments (`runTwice(f: () => void)`), as top-level functions and
// as arrows.
#[test]
fn a_function_that_returns_a_value_is_a_void_callback() {
    in_every_gc_mode(|| {
        let s = text(unsafe { ts_uic_valueCallbacks() });
        assert_eq!(s, "nnssmmooeemmoo:14");
    });
}

fn key(name: &str) -> Input {
    Input::KeyDown {
        key: name.into(),
        code: name.into(),
        repeat: false,
        text: None,
    }
}

fn mv(x: f32, y: f32) -> Input {
    Input::PointerMove { x, y }
}

fn press() -> Input {
    Input::PointerDown {
        button: 0,
        time_ms: 0.0,
    }
}

// M9-TS-3324
//
// `onFocus` / `onBlur` and their capture twins in a compiled game: the interaction layer sends
// `focusin` / `focusout` (on Tab, `Input::Focus` and `Input::Blur`), they bubble from the input to
// the form, capture runs outside-in first, and `relatedTarget` names the element focus came from
// or went to.
#[test]
fn focus_and_blur_handlers_run_with_capture_bubbling_and_related_targets() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_focusApp() });
        let form = ui.tree.children(ROOT)[0];
        let (a, b) = (ui.tree.children(form)[0], ui.tree.children(form)[1]);
        ui.notes();
        assert_eq!(ui.input(Input::Focus(a)), ["focusin"]);
        assert_eq!(ui.notes(), "cap-focus:focusin a-focus:0 form-focus:focusin");
        assert_eq!(ui.input(Input::Focus(b)), ["focusout", "focusin"]);
        assert_eq!(
            ui.notes(),
            format!(
                "cap-blur:focusout a-blur:{} form-blur:focusout cap-focus:focusin b-focus:{} form-focus:focusin",
                b.0, a.0
            )
        );
        assert_eq!(ui.input(Input::Blur(b)), ["focusout"]);
        assert_eq!(ui.notes(), "cap-blur:focusout b-blur:0 form-blur:focusout");
        // Tab starts at the first field, then moves on.
        assert_eq!(
            ui.input(key("Tab")),
            ["focusin"],
            "no focus, no keydown target"
        );
        assert_eq!(ui.notes(), "cap-focus:focusin a-focus:0 form-focus:focusin");
        assert_eq!(ui.input(key("Tab")), ["keydown", "focusout", "focusin"]);
        assert_eq!(ui.inter.focused(), Some(b));
        ui.notes();
    });
}

// M9-TS-3325
//
// `onTransitionRun` / `Start` / `End` / `Cancel` with `propertyName` (kebab-case), `elapsedTime`
// (seconds) and `pseudoElement` (empty), from the transitions the interaction layer reports.
#[test]
fn transition_handlers_get_the_property_and_the_elapsed_time() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_fadeApp() });
        let boxn = ui.tree.children(ROOT)[0];
        ui.at(0.0);
        ui.input(Input::Refresh);
        ui.notes();
        ui.click(boxn);
        ui.at(0.0);
        let kinds = ui.input(Input::Refresh);
        assert!(kinds.contains(&"transitionrun"), "{kinds:?}");
        assert!(ui.notes().starts_with("run:opacity:0"));
        ui.at(1000.0);
        let kinds = ui.input(Input::Refresh);
        assert!(kinds.contains(&"transitionend"), "{kinds:?}");
        assert!(ui.notes().ends_with("end:opacity:1:0"));
        // Reversed half way: the running transition is cancelled.
        ui.click(boxn);
        ui.at(1000.0);
        ui.input(Input::Refresh);
        ui.notes();
        ui.at(1500.0);
        ui.click(boxn);
        ui.at(1500.0);
        let kinds = ui.input(Input::Refresh);
        assert!(kinds.contains(&"transitioncancel"), "{kinds:?}");
        assert!(ui.notes().contains("cancel:opacity"));
    });
}

// M9-TS-3326
//
// `onAnimationStart` / `Iteration` / `End` with `animationName` and `elapsedTime`.
#[test]
fn animation_handlers_get_the_name_and_the_elapsed_time() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.tree.add_stylesheet(
            ".pulse { animation: glow 1s linear 2 } @keyframes glow { from { opacity: 1 } to { opacity: 0.5 } }",
        );
        ui.show(unsafe { ts_uic_pulseApp() });
        ui.at(0.0);
        assert_eq!(ui.input(Input::Refresh), ["animationstart"]);
        assert_eq!(ui.notes(), "astart:glow");
        ui.at(1100.0);
        assert_eq!(ui.input(Input::Refresh), ["animationiteration"]);
        assert_eq!(ui.notes(), "aiter:glow:1");
        ui.at(2100.0);
        assert_eq!(ui.input(Input::Refresh), ["animationend"]);
        assert_eq!(ui.notes(), "aend:glow:2");
    });
}

// M9-TS-3327
//
// Drag and drop in a compiled game: `dataTransfer` is one object from `dragstart` to `dragend`
// (`setData` in the first, `getData` in the others), a drop is accepted by preventing `dragover`,
// `dropped` says so on `dragend`; files dragged in from the system arrive as `dataTransfer.files`.
#[test]
fn drag_handlers_share_one_data_transfer_and_files_arrive_in_it() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_dragApp() });
        ui.tree.layout_if_needed();
        ui.input(mv(10.0, 10.0));
        ui.input(press());
        ui.notes();
        let kinds = ui.input(mv(30.0, 10.0));
        assert_eq!(kinds, ["dragstart", "drag", "dragenter", "dragover"]);
        assert_eq!(ui.notes(), "dragstart:text/plain");
        ui.input(mv(110.0, 10.0));
        assert_eq!(ui.notes(), "enter:hello");
        let kinds = ui.input(Input::PointerUp { button: 0 });
        assert_eq!(&kinds[..2], ["drop", "dragend"]);
        assert_eq!(ui.notes(), "drop:hello::text/plain dragend:true:hello:move");
        // A drag nobody accepts ends with `dropped` false and the effect reset.
        ui.input(mv(10.0, 10.0));
        ui.input(press());
        ui.input(mv(30.0, 10.0));
        ui.input(mv(250.0, 200.0));
        ui.notes();
        ui.input(Input::PointerUp { button: 0 });
        assert_eq!(ui.notes(), "dragend:false:hello:none");
        // Files from the system.
        let files: Vec<String> = vec!["C:\\games\\a.png".into(), "/tmp/dir/b.txt/".into()];
        ui.input(Input::FileDragEnter {
            files: files.clone(),
            x: 110.0,
            y: 10.0,
        });
        ui.notes();
        let kinds = ui.input(Input::FileDrop { files });
        assert!(kinds.contains(&"drop"), "{kinds:?}");
        assert_eq!(ui.notes(), "drop::a.png+b.txt:Files");
    });
}

// M9-TS-3328
//
// `onMouseEnter` / `onMouseLeave` / `onMouseMove` (the pointer events under React's mouse names);
// `scroll` does not bubble to the outer `onScroll`.
#[test]
fn mouse_aliases_and_non_bubbling_events_follow_the_dom() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_hoverApp() });
        ui.tree.layout_if_needed();
        ui.input(mv(10.0, 10.0));
        let notes = ui.notes();
        assert!(
            notes.contains("menter") && notes.contains("mmove"),
            "{notes}"
        );
        ui.input(mv(150.0, 150.0));
        assert_eq!(ui.notes(), "mleave");
        ui.show(unsafe { ts_uic_scrollApp() });
        ui.tree.layout_if_needed();
        ui.input(mv(10.0, 10.0));
        ui.notes();
        ui.input(Input::Wheel { dx: 0.0, dy: 50.0 });
        assert_eq!(ui.notes(), "scroll:50");
    });
}

// M9-TS-3380
//
// A callback ref (`ref={(el) => ..}`) is called with the element when it mounts; a re-render
// with a new function calls the old one with null and the new one with the element, as React
// does; removing the element calls it with null.
#[test]
fn a_callback_ref_hears_the_element_and_null() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_callbackRefApp() });
        assert_eq!(ui.notes(), "ref:el");
        let outer = ui.tree.children(ROOT)[0];
        ui.click(outer);
        no_errors(ui.rt.settle(&mut ui.tree));
        assert_eq!(ui.notes(), "ref:null ref:el");
        ui.click(outer);
        no_errors(ui.rt.settle(&mut ui.tree));
        assert_eq!(ui.notes(), "ref:null");
    });
}

// M9-TS-3381
//
// `<Fragment key={s}>` siblings in a list, components returning text, nothing or a list
// (`ReactNode`), a component that renders itself without a written return type, and
// `memo(function Name(..) {..})`.
#[test]
fn fragments_with_keys_and_components_returning_any_node() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_nodesApp() });
        assert_eq!(
            ui.dump(),
            "root(box(text(text:a),text(text:!),text(text:b),text(text:!),text:hi,text:x,text(text:y),box(box(text(text:leaf))),text(text:7)))"
        );
    });
}

// M9-TS-3382
//
// `useSyncExternalStore` re-renders when the store says it changed, and `useState` holds a
// string-literal union.
#[test]
fn an_external_store_and_a_literal_union_state_re_render() {
    in_every_gc_mode(|| {
        unsafe { ts_uic_setTheme(false) };
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_themedApp() });
        let b = ui.tree.children(ROOT)[0];
        let text = |ui: &Ui| ui.dump();
        assert!(text(&ui).contains("text:dark/browse"), "{}", text(&ui));
        ui.click(b);
        no_errors(ui.rt.settle(&mut ui.tree));
        assert!(text(&ui).contains("text:dark/details"), "{}", text(&ui));
        unsafe { ts_uic_setTheme(true) };
        no_errors(ui.rt.settle(&mut ui.tree));
        assert!(text(&ui).contains("text:light/details"), "{}", text(&ui));
    });
}

// M9-TS-3520
//
// `currentTarget` is the element whose handler runs (the outer box for a press on the inner
// one), pointer capture through it becomes a node command for the host, and `defaultPrevented`
// reads the event as it is now: false, then true once a handler prevented it, also further out.
#[test]
fn current_target_default_prevented_and_pointer_capture() {
    use web_ui_bridge::NodeCommand;
    in_every_gc_mode(|| {
        let _ = web_ui_bridge::take_node_commands();
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_captureApp() });
        let outer = ui.tree.children(ROOT)[0];
        let inner = ui.tree.children(outer)[0];
        ui.notes();
        let mut down = Event::new("pointerdown", inner);
        ui.rt.dispatch(&mut down, &mut ui.tree);
        let mut up = Event::new("pointerup", inner);
        ui.rt.dispatch(&mut up, &mut ui.tree);
        assert_eq!(ui.notes(), "down:true");
        assert_eq!(
            web_ui_bridge::take_node_commands(),
            [
                NodeCommand::SetPointerCapture(outer.0),
                NodeCommand::ReleasePointerCapture(outer.0)
            ]
        );
        assert_eq!(
            down.current, None,
            "no currentTarget once the dispatch is over"
        );
        let mut key = Event::new("keydown", inner);
        assert!(ui.rt.dispatch(&mut key, &mut ui.tree));
        assert_eq!(ui.notes(), "before:false after:true outer:true");
    });
}

// M9-TS-3521
//
// A function component's `ref` prop (`Ref<T>`, React 19) is filled by `useImperativeHandle`, and
// one without a ref renders too; a `"a" | "b" | null` state starts null and keeps each tag.
#[test]
fn ref_props_with_imperative_handles_and_nullable_literal_state() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_pickerApp() });
        assert_eq!(ui.notes(), "size:3");
        let parent = ui.tree.children(ROOT)[0];
        let first = ui.tree.children(parent)[0];
        assert_eq!(
            ui.dump(),
            "root(box(box(text(text:none)),box(text(text:none))))"
        );
        ui.click(first);
        assert_eq!(
            ui.dump(),
            "root(box(box(text(text:a)),box(text(text:none))))"
        );
        ui.click(first);
        assert_eq!(
            ui.dump(),
            "root(box(box(text(text:b)),box(text(text:none))))"
        );
        ui.click(first);
        assert_eq!(
            ui.dump(),
            "root(box(box(text(text:none)),box(text(text:none))))"
        );
    });
}

// M9-TS-3533
//
// `useState(name)` takes a local value named like a top-level function as the value (it was
// called as a lazy initialiser), calls a local arrow, and calls a top-level function.
#[test]
fn use_state_of_a_local_value_named_like_a_function_is_the_value() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_shadowApp() });
        assert_eq!(ui.dump(), "root(text(text:local lazy top))");
    });
}

// M9-TS-3532
//
// A spread of a conditional in JSX (`{...(live ? handlers(name) : {})}`) gives the element the
// taken arm's handlers; the `{}` arm gives none.
#[test]
fn a_conditional_spread_in_jsx_sets_the_taken_arms_props() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_condSpreadApp() });
        let parent = ui.tree.children(ROOT)[0];
        let rows = ui.tree.children(parent).to_vec();
        ui.notes();
        ui.click(rows[0]);
        ui.click(rows[1]);
        assert_eq!(ui.notes(), "click:a");
    });
}

// M9-TS-3522
//
// Setters, updaters and a reducer's `dispatch` kept after their component unmounted do nothing
// (React's behaviour), instead of aborting the process; while it is mounted they still work.
#[test]
fn state_setters_called_after_unmount_do_nothing() {
    in_every_gc_mode(|| {
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_lateApp() });
        assert_eq!(ui.dump(), "root(box(text(text:00x1)))");
        unsafe { ts_uic_callLateSetters() };
        no_errors(ui.rt.settle(&mut ui.tree));
        assert_eq!(ui.dump(), "root(box(text(text:11y2)))");
        assert_eq!(ui.notes(), "called");
        let parent = ui.tree.children(ROOT)[0];
        ui.click(parent);
        no_errors(ui.rt.settle(&mut ui.tree));
        assert_eq!(ui.dump(), "root(box())");
        unsafe { ts_uic_callLateSetters() };
        no_errors(ui.rt.settle(&mut ui.tree));
        assert_eq!(ui.notes(), "called");
        assert_eq!(ui.dump(), "root(box())");
    });
}

// M9-TS-3362
//
// `scrollIntoView`, `scrollTo` and `scrollBy` on a ref in compiled code, positional and with the
// DOM's option objects: the commands the host applies, with the DOM's defaults for keys left out
// (`block: "start"`, `inline: "nearest"`, `behavior: "auto"`; `scrollTo` keeps a missing axis).
#[test]
fn scroll_calls_on_a_ref_take_the_doms_option_objects() {
    use web_ui_bridge::NodeCommand;
    use web_ui_dom::{ScrollAlign as A, ScrollBehavior as B};
    in_every_gc_mode(|| {
        let _ = web_ui_bridge::take_node_commands();
        let mut ui = Ui::new();
        ui.show(unsafe { ts_uic_scrollCallsApp() });
        let id = ui.tree.children(ROOT)[0].0;
        let got = web_ui_bridge::take_node_commands();
        let into = |block, behavior| NodeCommand::ScrollIntoView {
            id,
            block,
            inline: A::Nearest,
            behavior,
        };
        assert_eq!(
            &got[..2],
            &[into(A::Center, B::Smooth), into(A::Start, B::Auto)]
        );
        match got[2] {
            NodeCommand::ScrollTo(i, x, y, B::Auto) => {
                assert_eq!(i, id);
                assert!(x.is_nan(), "a left-out axis stays: {x}");
                assert_eq!(y, 80.0);
            }
            ref c => panic!("{c:?}"),
        }
        assert_eq!(
            &got[3..],
            &[
                NodeCommand::ScrollTo(id, 1.0, 2.0, B::Auto),
                NodeCommand::ScrollBy(id, 5.0, 0.0, B::Instant),
                NodeCommand::ScrollBy(id, 0.0, -10.0, B::Auto),
            ]
        );
    });
}
