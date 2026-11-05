// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The native reconciler against a real `UiTree`. IDs are in TESTS.md (M2-NAT-*).
use platypus_ui::{ROOT, UiTree};
use platypus_ui_native::{Context, Dep, Element, Event, HostProps, NodeRef, Runtime};
use std::cell::RefCell;
use std::rc::Rc;
use web_ui_core::{NodeId, Value};

const FIRST: u32 = 1 << 30;

fn tree() -> UiTree {
    UiTree::new(200.0, 200.0)
}

fn b(children: Vec<Element>) -> Element {
    Element::host("box", HostProps::default(), children)
}

fn w(width: f64, children: Vec<Element>) -> Element {
    Element::host(
        "box",
        HostProps {
            style: vec![("width".into(), Value::Number(width))],
            ..HostProps::default()
        },
        children,
    )
}

/// The tree below `id` as text: `box(text:hi,box())`.
fn dump(tree: &UiTree, id: NodeId) -> String {
    let kind = tree.kind(id).unwrap_or("?");
    if kind == "#text" {
        return format!("text:{}", tree.text(id).unwrap_or(""));
    }
    let kids: Vec<String> = tree.children(id).iter().map(|c| dump(tree, *c)).collect();
    format!("{kind}({})", kids.join(","))
}

fn ok(errors: Vec<platypus_ui::UiError>) {
    assert!(errors.is_empty(), "{errors:?}");
}

// M2-NAT-01
//
// Elements become nodes: a box with text and a nested box, appended to the root container.
#[test]
fn a_description_becomes_nodes_in_the_tree() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    ok(rt.render(
        root,
        b(vec![Element::text("hi"), b(vec![]), Element::None]),
        &mut tree,
    ));
    assert_eq!(dump(&tree, ROOT), "root(box(text:hi,box()))");
    assert_eq!(rt.instance_count(), 5);
}

// M2-NAT-02
//
// State: the setter re-renders only its component; the text node is updated in place (same node).
#[test]
fn state_updates_change_the_text_in_place() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let setter = Rc::new(RefCell::new(None));
    let s2 = setter.clone();
    ok(rt.render(
        root,
        Element::component(1, move |cx| {
            let (n, set) = cx.use_state(|| 0i32);
            *s2.borrow_mut() = Some(set);
            b(vec![Element::text(format!("n={n}"))])
        }),
        &mut tree,
    ));
    let text_node = tree.children(tree.children(ROOT)[0])[0];
    assert_eq!(dump(&tree, ROOT), "root(box(text:n=0))");
    setter.borrow().as_ref().unwrap().set(5);
    ok(rt.flush(&mut tree));
    assert_eq!(dump(&tree, ROOT), "root(box(text:n=5))");
    assert_eq!(tree.children(tree.children(ROOT)[0])[0], text_node);
    // Setting the same value again renders nothing.
    setter.borrow().as_ref().unwrap().set(5);
    ok(rt.flush(&mut tree));
    assert_eq!(dump(&tree, ROOT), "root(box(text:n=5))");
}

// M2-NAT-03
//
// Keys: a reordered keyed list keeps its nodes (they move, they are not recreated); inserting
// and removing in the middle touches only those items.
#[test]
fn keyed_children_keep_their_nodes_when_reordered_inserted_and_removed() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let item = |k: &str| w(k.len() as f64, vec![Element::text(k)]).keyed(k);
    let list = |ks: &[&str]| b(ks.iter().map(|k| item(k)).collect());
    ok(rt.render(root, list(&["a", "bb", "ccc"]), &mut tree));
    let container = tree.children(ROOT)[0];
    let ids: Vec<NodeId> = tree.children(container).to_vec();
    ok(rt.render(root, list(&["ccc", "a", "bb"]), &mut tree));
    let now: Vec<NodeId> = tree.children(container).to_vec();
    assert_eq!(now, vec![ids[2], ids[0], ids[1]]);
    assert_eq!(
        dump(&tree, container),
        "box(box(text:ccc),box(text:a),box(text:bb))"
    );
    // Insert "x" in the middle, drop "a".
    ok(rt.render(root, list(&["ccc", "x", "bb"]), &mut tree));
    let after: Vec<NodeId> = tree.children(container).to_vec();
    assert_eq!(after[0], ids[2]);
    assert_eq!(after[2], ids[1]);
    assert!(!tree.contains(ids[0]), "the removed item is gone");
    assert_eq!(
        dump(&tree, container),
        "box(box(text:ccc),box(text:x),box(text:bb))"
    );
    // Reverse a longer list.
    let ks: Vec<String> = (0..8).map(|i| format!("k{i}")).collect();
    let refs: Vec<&str> = ks.iter().map(String::as_str).collect();
    ok(rt.render(root, list(&refs), &mut tree));
    let rev: Vec<&str> = refs.iter().rev().copied().collect();
    ok(rt.render(root, list(&rev), &mut tree));
    let want: Vec<String> = rev.iter().map(|k| format!("box(text:{k})")).collect();
    assert_eq!(dump(&tree, container), format!("box({})", want.join(",")));
}

// M2-NAT-04
//
// Props: style and attribute diffs only send what changed (a removed property resets).
#[test]
fn style_changes_apply_and_removed_properties_reset() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let el = |style: Vec<(&str, Value)>| {
        Element::host(
            "box",
            HostProps {
                style: style.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
                ..HostProps::default()
            },
            vec![],
        )
    };
    ok(rt.render(
        root,
        el(vec![
            ("width", Value::Number(50.0)),
            ("height", Value::Number(20.0)),
        ]),
        &mut tree,
    ));
    tree.layout_if_needed();
    let node = tree.children(ROOT)[0];
    assert_eq!(tree.rect(node).unwrap().width, 50.0);
    ok(rt.render(root, el(vec![("width", Value::Number(80.0))]), &mut tree));
    tree.layout_if_needed();
    let l = tree.rect(node).unwrap();
    assert_eq!(l.width, 80.0);
    assert_ne!(l.height, 20.0, "the removed height was reset");
}

// M2-NAT-05
//
// A component that switches what it renders replaces the old nodes in place; fragments flatten
// into their container.
#[test]
fn components_and_fragments_place_their_nodes_in_the_container() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let setter = Rc::new(RefCell::new(None));
    let s2 = setter.clone();
    let inner = Element::component(2, move |cx| {
        let (n, set) = cx.use_state(|| 0i32);
        *s2.borrow_mut() = Some(set);
        if n == 0 {
            Element::fragment(vec![Element::text("a"), Element::text("b")])
        } else {
            Element::host("span", HostProps::default(), vec![Element::text("c")])
        }
    });
    ok(rt.render(
        root,
        b(vec![Element::text("<"), inner, Element::text(">")]),
        &mut tree,
    ));
    let container = tree.children(ROOT)[0];
    assert_eq!(dump(&tree, container), "box(text:<,text:a,text:b,text:>)");
    setter.borrow().as_ref().unwrap().set(1);
    ok(rt.flush(&mut tree));
    assert_eq!(dump(&tree, container), "box(text:<,span(text:c),text:>)");
}

// M2-NAT-06
//
// Effect order equals real React's (the same scenarios, with the logs real React produced, are
// in js/react/test/native_oracle.test.tsx).
#[test]
fn effects_and_cleanups_run_in_reacts_order() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let log: Rc<RefCell<Vec<String>>> = Rc::default();

    fn eff(log: &Rc<RefCell<Vec<String>>>, name: &'static str, n: u32, key: bool) -> Element {
        let l = log.clone();
        let e = Element::component(10, move |cx| {
            let l = l.clone();
            cx.use_effect(Some(&[Dep::Num(f64::from(n))]), move || {
                l.borrow_mut().push(format!("{name}+{n}"));
                let l = l.clone();
                Some(Box::new(move || l.borrow_mut().push(format!("{name}-{n}"))))
            });
            Element::None
        });
        if key { e.keyed(name) } else { e }
    }
    let p = |log: &Rc<RefCell<Vec<String>>>, n: u32, kids: &[&'static str]| {
        let l = log.clone();
        let kids: Vec<&'static str> = kids.to_vec();
        Element::component(11, move |cx| {
            let l2 = l.clone();
            cx.use_effect(Some(&[Dep::Num(f64::from(n))]), move || {
                l2.borrow_mut().push(format!("P+{n}"));
                let l3 = l2.clone();
                Some(Box::new(move || l3.borrow_mut().push(format!("P-{n}"))))
            });
            Element::fragment(kids.iter().map(|k| eff(&l, k, n, true)).collect())
        })
    };
    let nest = |log: &Rc<RefCell<Vec<String>>>, n: u32| {
        let l = log.clone();
        Element::component(12, move |cx| {
            let l2 = l.clone();
            cx.use_effect(Some(&[Dep::Num(f64::from(n))]), move || {
                l2.borrow_mut().push(format!("outer+{n}"));
                let l3 = l2.clone();
                Some(Box::new(move || l3.borrow_mut().push(format!("outer-{n}"))))
            });
            let l4 = l.clone();
            Element::component(13, move |cx| {
                let l5 = l4.clone();
                cx.use_effect(Some(&[Dep::Num(f64::from(n))]), move || {
                    l5.borrow_mut().push(format!("inner+{n}"));
                    let l6 = l5.clone();
                    Some(Box::new(move || l6.borrow_mut().push(format!("inner-{n}"))))
                });
                Element::None
            })
        })
    };
    let step = |rt: &mut Runtime, tree: &mut UiTree, el: Element| -> Vec<String> {
        log.borrow_mut().clear();
        ok(rt.render(root, el, tree));
        log.borrow().clone()
    };
    let want: Vec<Vec<&str>> = vec![
        vec!["a+0", "b+0", "P+0"],
        vec!["a-0", "b-0", "P-0", "a+1", "b+1", "P+1"],
        vec!["b-1"],
        vec!["P-1", "a-1", "inner+0", "outer+0"],
        vec!["inner-0", "outer-0", "inner+1", "outer+1"],
        vec!["outer-1", "inner-1", "P+2"],
        vec!["P-2"],
    ];
    let got = vec![
        step(&mut rt, &mut tree, p(&log, 0, &["a", "b"])),
        step(&mut rt, &mut tree, p(&log, 1, &["a", "b"])),
        step(&mut rt, &mut tree, p(&log, 1, &["a"])),
        step(&mut rt, &mut tree, nest(&log, 0)),
        step(&mut rt, &mut tree, nest(&log, 1)),
        step(&mut rt, &mut tree, p(&log, 2, &[])),
        step(&mut rt, &mut tree, Element::None),
    ];
    assert_eq!(got, want);
}

// M2-NAT-07
//
// useRef/useMemo/useCallback keep values across renders; memo recomputes only when its
// dependencies change; effects with no dependency list run every render.
#[test]
fn ref_memo_and_callback_keep_their_values() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let computed = Rc::new(RefCell::new(0));
    let runs = Rc::new(RefCell::new(0));
    let setter = Rc::new(RefCell::new(None));
    let (c2, r2, s2) = (computed.clone(), runs.clone(), setter.clone());
    ok(rt.render(
        root,
        Element::component(1, move |cx| {
            let (n, set) = cx.use_state(|| 1i32);
            *s2.borrow_mut() = Some(set);
            let renders = cx.use_ref(|| 0);
            *renders.borrow_mut() += 1;
            let c3 = c2.clone();
            let sq = cx.use_memo(&[Dep::Num(f64::from(n / 10))], move || {
                *c3.borrow_mut() += 1;
                n * n
            });
            let r3 = r2.clone();
            cx.use_effect(None, move || {
                *r3.borrow_mut() += 1;
                None
            });
            Element::text(format!("{}:{}:{}", n, sq, renders.borrow()))
        }),
        &mut tree,
    ));
    let set = setter.borrow().clone().unwrap();
    assert_eq!(dump(&tree, ROOT), "root(text:1:1:1)");
    set.set(2);
    ok(rt.flush(&mut tree));
    // The memo's dependency (n / 10) did not change: the old square is kept.
    assert_eq!(dump(&tree, ROOT), "root(text:2:1:2)");
    set.set(30);
    ok(rt.flush(&mut tree));
    assert_eq!(dump(&tree, ROOT), "root(text:30:900:3)");
    assert_eq!((*computed.borrow(), *runs.borrow()), (2, 3));
}

// M2-NAT-08
//
// Portals mount their children under another node; events bubble through the component tree.
#[test]
fn portals_mount_elsewhere_and_events_bubble_to_the_logical_parent() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    // A separate container node, made by a second root.
    let elsewhere = rt.create_root(ROOT);
    ok(rt.render(elsewhere, w(7.0, vec![]), &mut tree));
    let target = tree.children(ROOT)[0];
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let (l1, l2) = (log.clone(), log.clone());
    let click_outer: platypus_ui_native::Handler =
        Rc::new(move |_| l1.borrow_mut().push("outer".into()));
    let click_inner: platypus_ui_native::Handler =
        Rc::new(move |_| l2.borrow_mut().push("inner".into()));
    let inner = Element::host(
        "box",
        HostProps {
            handlers: vec![("click".into(), click_inner)],
            ..HostProps::default()
        },
        vec![Element::text("in portal")],
    );
    ok(rt.render(
        root,
        Element::host(
            "box",
            HostProps {
                handlers: vec![("click".into(), click_outer)],
                ..HostProps::default()
            },
            vec![Element::portal(target, vec![inner])],
        ),
        &mut tree,
    ));
    // The portal's box is under `target`, not under the outer box.
    assert_eq!(dump(&tree, target), "box(box(text:in portal))");
    let outer_box = tree.children(ROOT)[1];
    assert_eq!(dump(&tree, outer_box), "box()");
    let portal_box = tree.children(target)[0];
    let mut ev = Event::new("click", portal_box);
    rt.dispatch(&mut ev, &mut tree);
    assert_eq!(*log.borrow(), ["inner", "outer"]);
}

// M2-NAT-09
//
// Capture runs outside-in before bubble; stopPropagation stops after the current element;
// preventDefault is reported; a handler's state change commits before dispatch returns.
#[test]
fn events_capture_then_bubble_and_commit_state_synchronously() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let setter = Rc::new(RefCell::new(None));
    let s2 = setter.clone();
    let l = log.clone();
    ok(rt.render(
        root,
        Element::component(1, move |cx| {
            let (n, set) = cx.use_state(|| 0i32);
            *s2.borrow_mut() = Some(set.clone());
            let mk =
                |name: &'static str, l: &Rc<RefCell<Vec<String>>>| -> platypus_ui_native::Handler {
                    let l = l.clone();
                    Rc::new(move |_| l.borrow_mut().push(name.into()))
                };
            let l_stop = l.clone();
            let stop: platypus_ui_native::Handler = Rc::new(move |e: &mut Event| {
                l_stop.borrow_mut().push("mid".into());
                e.prevent_default();
                e.stop_propagation();
            });
            let inc: platypus_ui_native::Handler = {
                let set = set.clone();
                Rc::new(move |_| set.update(|v| v + 1))
            };
            Element::host(
                "box",
                HostProps {
                    handlers: vec![
                        ("click:capture".into(), mk("outer-capture", &l)),
                        ("click".into(), mk("outer", &l)),
                    ],
                    ..HostProps::default()
                },
                vec![Element::host(
                    "box",
                    HostProps {
                        handlers: vec![
                            ("click".into(), stop),
                            ("click:capture".into(), mk("mid-capture", &l)),
                        ],
                        ..HostProps::default()
                    },
                    vec![Element::host(
                        "box",
                        HostProps {
                            handlers: vec![("click".into(), inc), ("click".into(), mk("leaf", &l))],
                            ..HostProps::default()
                        },
                        vec![Element::text(format!("n={n}"))],
                    )],
                )],
            )
        }),
        &mut tree,
    ));
    let leaf = {
        let mut n = tree.children(ROOT)[0];
        while !tree.children(n).is_empty() && tree.kind(tree.children(n)[0]) != Some("#text") {
            n = tree.children(n)[0];
        }
        n
    };
    let mut ev = Event::new("click", leaf);
    let prevented = rt.dispatch(&mut ev, &mut tree);
    assert!(prevented);
    assert_eq!(
        *log.borrow(),
        ["outer-capture", "mid-capture", "leaf", "mid"]
    );
    // The inc handler ran too, and the new text is in the tree already.
    assert_eq!(dump(&tree, leaf), "box(text:n=1)");
}

// M2-NAT-10
//
// Refs are set when the element is attached and cleared when it unmounts; context reaches
// nested components and changes with the provider; unmounting runs cleanups and frees all.
#[test]
fn refs_context_and_unmount() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let node_ref = NodeRef::new();
    let theme = Rc::new(Context::new(7, "light".to_string()));
    let cleaned = Rc::new(RefCell::new(0));
    let view = |value: &str, node_ref: &NodeRef| {
        let theme2 = theme.clone();
        let c = cleaned.clone();
        let nr = node_ref.clone();
        theme.provider(
            value.to_string(),
            vec![Element::component(5, move |cx| {
                let t = cx.use_context(&theme2);
                let c = c.clone();
                cx.use_effect(Some(&[]), move || {
                    Some(Box::new(move || *c.borrow_mut() += 1))
                });
                Element::host(
                    "box",
                    HostProps {
                        node_ref: Some(nr.clone()),
                        ..HostProps::default()
                    },
                    vec![Element::text(t)],
                )
            })],
        )
    };
    assert_eq!(node_ref.get(), None);
    ok(rt.render(root, view("dark", &node_ref), &mut tree));
    let node = node_ref.get().expect("ref set");
    assert_eq!(dump(&tree, node), "box(text:dark)");
    ok(rt.render(root, view("blue", &node_ref), &mut tree));
    assert_eq!(node_ref.get(), Some(node), "same element, same node");
    assert_eq!(dump(&tree, node), "box(text:blue)");
    ok(rt.unmount_root(root, &mut tree));
    assert_eq!(node_ref.get(), None);
    assert_eq!(*cleaned.borrow(), 1);
    assert!(!rt.is_mounted(node));
    assert!(tree.children(ROOT).is_empty(), "everything was removed");
}

// M2-NAT-11
//
// An effect that sets state on every render ends with an error, not a hang.
#[test]
fn a_render_loop_is_reported() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let errors = rt.render(
        root,
        Element::component(1, |cx| {
            let (n, set) = cx.use_state(|| 0u32);
            cx.use_effect(None, move || {
                set.set(n + 1);
                None
            });
            Element::None
        }),
        &mut tree,
    );
    assert!(
        errors.iter().any(|e| e.0.contains("chained updates")),
        "{errors:?}"
    );
}

fn field(value: &str, on_input: platypus_ui_native::Handler) -> Element {
    Element::host(
        "input",
        HostProps {
            attrs: vec![("value".into(), Value::String(value.into()))],
            handlers: vec![("input".into(), on_input)],
            ..HostProps::default()
        },
        vec![],
    )
}

fn typed(text: &str, node: NodeId) -> Event {
    let mut ev = Event::new("input", node);
    ev.strings.push(text.into());
    ev
}

// M2-NAT-13
//
// A controlled text field: when the handler accepts the typed text (even transformed) the new
// value prop is what the tree holds; when it does not, the old value goes back.
#[test]
fn a_controlled_field_keeps_its_value_prop() {
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let accept = Rc::new(RefCell::new(true));
    let a2 = accept.clone();
    ok(rt.render(
        root,
        Element::component(1, move |cx| {
            let (text, set) = cx.use_state(|| "init".to_string());
            let a3 = a2.clone();
            let on_input: platypus_ui_native::Handler = Rc::new(move |e: &mut Event| {
                if *a3.borrow() {
                    set.set(e.strings[0].to_uppercase());
                }
            });
            field(&text, on_input)
        }),
        &mut tree,
    ));
    let node = tree.children(ROOT)[0];
    let value = |tree: &UiTree| tree.attrs(node).unwrap().value.clone();
    assert_eq!(value(&tree), Some("init".to_string()));
    // Accepted, and transformed.
    rt.dispatch(&mut typed("abc", node), &mut tree);
    assert_eq!(value(&tree), Some("ABC".to_string()));
    // Rejected: the old value is sent back (one op).
    *accept.borrow_mut() = false;
    let before = rt.ops_applied();
    rt.dispatch(&mut typed("ABCD", node), &mut tree);
    assert_eq!(rt.ops_applied(), before + 1);
    assert_eq!(value(&tree), Some("ABC".to_string()));
    // Accepted again after a rejection.
    *accept.borrow_mut() = true;
    rt.dispatch(&mut typed("xyz", node), &mut tree);
    assert_eq!(value(&tree), Some("XYZ".to_string()));
}

// M9-TS-3332
//
// Focus through the interaction layer: `focusin` / `focusout` bubble to the form (capture first),
// `event_of` carries `relatedTarget` in number 9, and a non-bubbling `scroll` stays on its element
// while its own handler still runs.
#[test]
fn focus_events_from_the_interaction_layer_reach_handlers_and_scroll_does_not_bubble() {
    use platypus_ui::{Input, Interaction};
    let mut tree = tree();
    let mut rt = Runtime::new(FIRST);
    let root = rt.create_root(ROOT);
    let log: Rc<RefCell<Vec<String>>> = Rc::default();
    let note = |name: &'static str| -> platypus_ui_native::Handler {
        let log = log.clone();
        Rc::new(move |e: &mut Event| {
            let related = e.data.get(9).copied().unwrap_or(0.0);
            log.borrow_mut().push(format!("{name}:{related}"));
        })
    };
    let field = |tag: &'static str| {
        Element::host(
            "input",
            HostProps {
                handlers: vec![
                    (
                        "focusin".into(),
                        note(if tag == "a" { "a-in" } else { "b-in" }),
                    ),
                    (
                        "focusout".into(),
                        note(if tag == "a" { "a-out" } else { "b-out" }),
                    ),
                ],
                ..HostProps::default()
            },
            vec![],
        )
    };
    let form = Element::host(
        "box",
        HostProps {
            handlers: vec![
                ("focusin:capture".into(), note("cap-in")),
                ("focusin".into(), note("form-in")),
                ("focusout".into(), note("form-out")),
                ("scroll".into(), note("form-scroll")),
            ],
            ..HostProps::default()
        },
        vec![field("a"), field("b")],
    );
    ok(rt.render(root, form, &mut tree));
    let form_id = tree.children(ROOT)[0];
    let (a, b) = (tree.children(form_id)[0], tree.children(form_id)[1]);
    let mut ui = Interaction::new();
    let mut send = |ui: &mut Interaction, tree: &mut UiTree, input: Input| {
        ui.handle(
            tree,
            input,
            &mut |e| match platypus_ui_native::event_of(e) {
                Some(mut ev) => rt.dispatch_handlers(&mut ev),
                None => false,
            },
        );
    };
    send(&mut ui, &mut tree, Input::Focus(a));
    assert_eq!(*log.borrow(), ["cap-in:0", "a-in:0", "form-in:0"]);
    log.borrow_mut().clear();
    send(&mut ui, &mut tree, Input::Focus(b));
    assert_eq!(
        *log.borrow(),
        [
            format!("a-out:{}", b.0),
            format!("form-out:{}", b.0),
            "cap-in:".to_string() + &a.0.to_string(),
            format!("b-in:{}", a.0),
            format!("form-in:{}", a.0),
        ]
    );
    // A scroll event of the field does not reach the form's `onScroll`.
    log.borrow_mut().clear();
    let scroll = platypus_ui::UiEvent::new(
        platypus_ui::EventKind::Scroll,
        tree.event_path(b),
        platypus_ui::events::Modifiers::default(),
    );
    let mut ev = platypus_ui_native::event_of(&scroll).unwrap();
    assert!(!ev.bubbles);
    rt.dispatch_handlers(&mut ev);
    assert!(log.borrow().is_empty(), "{:?}", log.borrow());
}
