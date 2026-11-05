// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Focus and keyboard (M3-EVT-04): Tab order from `tabIndex`, focus by pointer and
//! script, key events to the focused element, and `:focus` / `:focus-visible` /
//! `:focus-within`. The script side is in `events_quickjs.rs` and
//! `js/react/test/events.test.tsx`.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::events::Modifiers;
use platypus_ui::interaction::tab_order;
use platypus_ui::opcodes::{REMOVE_CHILD, SET_ATTRS, SET_STYLE};
use platypus_ui::{Input, Interaction, Rgba, UiEvent, UiTree};
use web_ui_core::{Commit, NodeId, Value};

fn attrs(d: &mut Dom, id: u32, attrs: &[(&str, Value)]) {
    let mut a = vec![n(id)];
    for (k, v) in attrs {
        a.push((*k).into());
        a.push(v.clone());
    }
    d.ops.push(op(SET_ATTRS, &a));
}

fn sized() -> [(&'static str, Value); 2] {
    [("width", px(50.0)), ("height", px(50.0))]
}

/// Records dispatched events as `(type, target)`; `prevent` lists the types whose
/// default the "script" prevents.
struct Recorder {
    log: Vec<(&'static str, NodeId)>,
    events: Vec<UiEvent>,
    prevent: Vec<&'static str>,
}

impl Recorder {
    fn new() -> Self {
        Self {
            log: Vec::new(),
            events: Vec::new(),
            prevent: Vec::new(),
        }
    }

    fn take(&mut self) -> Vec<(&'static str, NodeId)> {
        self.events.clear();
        std::mem::take(&mut self.log)
    }
}

fn handle(ui: &mut Interaction, tree: &mut UiTree, rec: &mut Recorder, input: Input) {
    ui.handle(tree, input, &mut |e| {
        rec.log.push((e.kind.name(), e.target()));
        rec.events.push(e.clone());
        rec.prevent.contains(&e.kind.name())
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

fn shift(on: bool) -> Input {
    Input::Modifiers(Modifiers {
        shift: on,
        ..Modifiers::default()
    })
}

/// A row of 50x50 boxes with these `tabIndex` values, plus a disabled one, one inside
/// a `display: none` box and one with no `tabIndex`.
fn row(indexes: &[i32]) -> (UiTree, Vec<NodeId>) {
    let mut d = Dom::new();
    let mut ids = Vec::new();
    for index in indexes {
        let id = d.add(0, "box", &sized());
        attrs(&mut d, id, &[("tabIndex", px(*index as f64))]);
        ids.push(NodeId(id));
    }
    let disabled = d.add(0, "box", &sized());
    attrs(
        &mut d,
        disabled,
        &[("tabIndex", px(0.0)), ("disabled", true.into())],
    );
    let hidden = d.add(0, "box", &[("display", "none".into())]);
    let inner = d.add(hidden, "box", &sized());
    attrs(&mut d, inner, &[("tabIndex", px(0.0))]);
    d.add(0, "box", &sized());
    (d.build(""), ids)
}

// M3-EVT-04
#[test]
fn tab_order_puts_positive_indexes_first_then_zero_in_tree_order() {
    let (tree, ids) = row(&[0, 2, -1, 1, 0, 2]);
    assert_eq!(
        tab_order(&tree),
        [ids[3], ids[1], ids[5], ids[0], ids[4]],
        "1, then the 2s in tree order, then the 0s; -1, disabled and hidden are skipped"
    );
}

// M3-EVT-04
#[test]
fn tab_and_shift_tab_move_focus_and_wrap() {
    let (mut tree, ids) = row(&[0, 0, 0]);
    let (a, b, c) = (ids[0], ids[1], ids[2]);
    let mut ui = Interaction::new();
    let mut rec = Recorder::new();

    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    assert_eq!(
        rec.take(),
        [("focusin", a)],
        "nothing focused: no keydown target, Tab starts at the first element"
    );
    assert_eq!(ui.focused(), Some(a));
    let s = tree.state(a);
    assert!(s.focus && s.focus_visible, "keyboard focus shows the ring");

    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    assert_eq!(
        rec.take(),
        [("keydown", a), ("focusout", a), ("focusin", b)]
    );
    assert!(!tree.state(a).focus && !tree.state(a).focus_visible);

    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    assert_eq!(ui.focused(), Some(a), "wraps from the last to the first");
    rec.take();

    handle(&mut ui, &mut tree, &mut rec, shift(true));
    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    assert_eq!(ui.focused(), Some(c), "Shift-Tab wraps backwards");
    let events = std::mem::take(&mut rec.events);
    assert_eq!(
        events[1].related,
        Some(c),
        "focusout knows where focus went"
    );
    assert_eq!(
        events[2].related,
        Some(a),
        "focusin knows where it came from"
    );
    assert!(events[0].modifiers.shift);
}

// M3-EVT-04
#[test]
fn a_prevented_tab_keydown_keeps_focus() {
    let (mut tree, ids) = row(&[0, 0]);
    let mut ui = Interaction::new();
    let mut rec = Recorder::new();
    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    rec.prevent.push("keydown");
    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    assert_eq!(ui.focused(), Some(ids[0]));
}

// M3-EVT-04
#[test]
fn keys_go_to_the_focused_element_and_bubble() {
    let mut d = Dom::new();
    let panel = d.add(0, "box", &[("width", px(200.0)), ("height", px(200.0))]);
    let field = d.add(panel, "box", &sized());
    attrs(&mut d, field, &[("tabIndex", px(0.0))]);
    let mut tree = d.build("");
    let (panel, field) = (NodeId(panel), NodeId(field));
    let mut ui = Interaction::new();
    let mut rec = Recorder::new();

    handle(&mut ui, &mut tree, &mut rec, key("a"));
    assert!(rec.take().is_empty(), "no focus, no key events");

    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    rec.take();
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::KeyDown {
            key: "a".into(),
            code: "KeyA".into(),
            repeat: true,
            text: None,
        },
    );
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::KeyUp {
            key: "a".into(),
            code: "KeyA".into(),
        },
    );
    let events = std::mem::take(&mut rec.events);
    assert_eq!(events[0].path, [field, panel]);
    assert_eq!(
        (
            events[0].key.as_str(),
            events[0].code.as_str(),
            events[0].repeat
        ),
        ("a", "KeyA", true)
    );
    assert_eq!(events[1].kind.name(), "keyup");
    let args = events[0].script_args();
    assert_eq!(
        args[3],
        Value::Array(vec!["a".into(), "KeyA".into()]),
        "key and code go to scripts as a fourth argument"
    );
}

// M3-EVT-04
#[test]
fn pressing_focuses_the_nearest_focusable_ancestor_and_elsewhere_blurs() {
    let mut d = Dom::new();
    let button = d.add(0, "box", &[("width", px(100.0)), ("height", px(100.0))]);
    attrs(&mut d, button, &[("tabIndex", px(-1.0))]);
    d.add(button, "box", &sized());
    let mut tree = d.build("");
    let button = NodeId(button);
    let mut ui = Interaction::new();
    let mut rec = Recorder::new();

    let press = |ui: &mut Interaction, tree: &mut UiTree, rec: &mut Recorder, x: f32| {
        handle(ui, tree, rec, Input::PointerMove { x, y: 10.0 });
        handle(
            ui,
            tree,
            rec,
            Input::PointerDown {
                button: 0,
                time_ms: x as f64 * 1000.0,
            },
        );
        handle(ui, tree, rec, Input::PointerUp { button: 0 });
    };

    press(&mut ui, &mut tree, &mut rec, 10.0);
    assert_eq!(
        ui.focused(),
        Some(button),
        "the inner box isn't focusable; its tabIndex -1 parent is"
    );
    let s = tree.state(button);
    assert!(s.focus && !s.focus_visible, "pointer focus shows no ring");
    let log = rec.take();
    let down = log.iter().position(|e| e.0 == "pointerdown").unwrap();
    assert_eq!(
        log[down + 1],
        ("focusin", button),
        "focus follows pointerdown"
    );

    press(&mut ui, &mut tree, &mut rec, 300.0);
    assert_eq!(ui.focused(), None, "pressing on nothing focusable blurs");

    rec.prevent.push("pointerdown");
    press(&mut ui, &mut tree, &mut rec, 10.0);
    assert_eq!(
        ui.focused(),
        None,
        "preventing pointerdown keeps focus where it was"
    );
}

// M3-EVT-04
#[test]
fn scripts_focus_and_blur_and_visibility_follows_the_last_input() {
    let (mut tree, ids) = row(&[0, 0, -1]);
    let mut ui = Interaction::new();
    let mut rec = Recorder::new();

    handle(&mut ui, &mut tree, &mut rec, Input::Focus(ids[2]));
    assert_eq!(
        ui.focused(),
        Some(ids[2]),
        "tabIndex -1 can be focused by script"
    );
    assert!(
        !tree.state(ids[2]).focus_visible,
        "no keyboard use yet: no ring"
    );

    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    handle(&mut ui, &mut tree, &mut rec, Input::Focus(ids[1]));
    assert!(
        tree.state(ids[1]).focus_visible,
        "after keyboard use, script focus shows the ring"
    );

    let unfocusable = NodeId(ids[2].0 + 1);
    handle(&mut ui, &mut tree, &mut rec, Input::Focus(unfocusable));
    assert_eq!(
        ui.focused(),
        Some(ids[1]),
        "a disabled element can't take focus"
    );

    handle(&mut ui, &mut tree, &mut rec, Input::Blur(ids[0]));
    assert_eq!(
        ui.focused(),
        Some(ids[1]),
        "blurring another element does nothing"
    );
    handle(&mut ui, &mut tree, &mut rec, Input::Blur(ids[1]));
    assert_eq!(ui.focused(), None);
    assert!(!tree.state(ids[1]).focus);
}

// M3-EVT-04
#[test]
fn hidden_or_removed_focused_elements_lose_focus_on_refresh() {
    let (mut tree, ids) = row(&[0, 0]);
    let mut ui = Interaction::new();
    let mut rec = Recorder::new();
    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    rec.take();

    let commit = |tree: &mut UiTree, ops| {
        let errors = tree.apply_commit(&Commit { ops });
        assert!(errors.is_empty(), "{errors:?}");
    };
    commit(
        &mut tree,
        vec![op(
            SET_STYLE,
            &[n(ids[0].0), "display".into(), "none".into()],
        )],
    );
    handle(&mut ui, &mut tree, &mut rec, Input::Refresh);
    assert_eq!(rec.take(), [("focusout", ids[0])]);
    assert_eq!(ui.focused(), None);

    handle(&mut ui, &mut tree, &mut rec, Input::Focus(ids[1]));
    rec.take();
    commit(&mut tree, vec![op(REMOVE_CHILD, &[n(0), n(ids[1].0)])]);
    handle(&mut ui, &mut tree, &mut rec, Input::Refresh);
    assert!(rec.take().is_empty(), "removed elements get no events");
    assert_eq!(ui.focused(), None);
}

// M3-EVT-04
#[test]
fn focus_styles_apply() {
    let mut d = Dom::new();
    let form = d.add(0, "box", &[("width", px(200.0)), ("height", px(200.0))]);
    attrs(&mut d, form, &[("className", "form".into())]);
    let field = d.add(form, "box", &sized());
    attrs(&mut d, field, &[("tabIndex", px(0.0))]);
    let mut tree = d.build(
        ".form:focus-within { background-color: #0000ff }
         box:focus { background-color: #00ff00 }
         box:focus-visible { border: 2px solid #ff0000 }",
    );
    let (form, field) = (NodeId(form), NodeId(field));
    let blue = Rgba::parse("#0000ff").unwrap();
    let green = Rgba::parse("#00ff00").unwrap();
    let red = Rgba::parse("#ff0000").unwrap();
    let mut ui = Interaction::new();
    let mut rec = Recorder::new();

    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    tree.layout_if_needed();
    assert_eq!(tree.visual(form).unwrap().background, blue);
    assert_eq!(tree.visual(field).unwrap().background, green);
    assert_eq!(tree.visual(field).unwrap().border_colors[0], red);

    handle(&mut ui, &mut tree, &mut rec, Input::Blur(field));
    tree.layout_if_needed();
    assert_ne!(tree.visual(form).unwrap().background, blue);
    assert_ne!(tree.visual(field).unwrap().background, green);
}
