// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! The pointer state machine: hover with enter/leave, `:hover` / `:active`, clicks on
//! the common ancestor, double clicks, wheel and cursors (M3-EVT-02, M3-EVT-03; the
//! script side is in `react_quickjs.rs` and `js/react/test/events.test.tsx`).

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::events::Modifiers;
use platypus_ui::opcodes::{SET_ATTRS, SET_STYLE};
use platypus_ui::{Pointer, Rgba, UiEvent, UiTree};
use web_ui_core::{Commit, NodeId, Value};

/// `a` (200x200 at 0,0, padding 20) holds `b` (100x100 at 20,20); `c` (100x100) is
/// below `a` at (0,200).
fn tree(css: &str) -> (UiTree, [NodeId; 3]) {
    let mut d = Dom::new();
    let a = d.add(
        0,
        "box",
        &[
            ("width", px(200.0)),
            ("height", px(200.0)),
            ("padding", px(20.0)),
        ],
    );
    let b = d.add(a, "box", &[("width", px(100.0)), ("height", px(100.0))]);
    let c = d.add(
        0,
        "box",
        &[
            ("position", "absolute".into()),
            ("top", px(200.0)),
            ("width", px(100.0)),
            ("height", px(100.0)),
        ],
    );
    (d.build(css), [NodeId(a), NodeId(b), NodeId(c)])
}

/// Events as `(type, path)` for compact assertions.
fn summary(events: &[UiEvent]) -> Vec<(&'static str, Vec<NodeId>)> {
    events
        .iter()
        .map(|e| (e.kind.name(), e.path.clone()))
        .collect()
}

fn commit(tree: &mut UiTree, ops: Vec<Value>) {
    let errors = tree.apply_commit(&Commit { ops });
    assert!(errors.is_empty(), "{errors:?}");
}

// M3-EVT-03
#[test]
fn hover_follows_the_pointer_with_enter_and_leave() {
    let (mut tree, [a, b, c]) = tree("");
    let mut p = Pointer::new();

    let e = p.move_to(&mut tree, 50.0, 50.0);
    assert_eq!(
        summary(&e),
        [
            ("pointerenter", vec![a]),
            ("pointerenter", vec![b]),
            ("pointermove", vec![b, a]),
        ],
        "enter outermost first, then the move bubbles from the target"
    );
    assert!(tree.state(a).hover && tree.state(b).hover);
    assert_eq!(p.hovered(), Some(b));

    let e = p.move_to(&mut tree, 55.0, 55.0);
    assert_eq!(summary(&e), [("pointermove", vec![b, a])]);

    let e = p.move_to(&mut tree, 10.0, 10.0);
    assert_eq!(
        summary(&e),
        [("pointerleave", vec![b]), ("pointermove", vec![a])]
    );
    assert!(tree.state(a).hover && !tree.state(b).hover);

    let e = p.move_to(&mut tree, 50.0, 250.0);
    assert_eq!(
        summary(&e),
        [
            ("pointerleave", vec![a]),
            ("pointerenter", vec![c]),
            ("pointermove", vec![c]),
        ]
    );

    let e = p.move_to(&mut tree, 300.0, 300.0);
    assert_eq!(
        summary(&e),
        [("pointerleave", vec![c])],
        "over the root alone: no target, no move"
    );
    p.move_to(&mut tree, 50.0, 50.0);
    let e = p.leave(&mut tree);
    assert_eq!(
        summary(&e),
        [("pointerleave", vec![b]), ("pointerleave", vec![a])],
        "leave innermost first"
    );
    assert!(!tree.state(a).hover && !tree.state(b).hover);
}

// M3-EVT-03
#[test]
fn hover_and_active_styles_apply() {
    let (mut tree, [a, b, _]) =
        tree("box:hover { background-color: #00ff00 } box:active { background-color: #ff0000 }");
    let green = Rgba::parse("#00ff00").unwrap();
    let red = Rgba::parse("#ff0000").unwrap();
    let mut p = Pointer::new();

    p.move_to(&mut tree, 50.0, 50.0);
    tree.layout_if_needed();
    assert_eq!(
        tree.visual(a).unwrap().background,
        green,
        "ancestors hover too"
    );
    assert_eq!(tree.visual(b).unwrap().background, green);

    p.down(&mut tree, 0, 0.0);
    tree.layout_if_needed();
    assert!(tree.state(a).active && tree.state(b).active);
    assert_eq!(tree.visual(b).unwrap().background, red);

    p.up(&mut tree, 0);
    tree.layout_if_needed();
    assert!(!tree.state(a).active && !tree.state(b).active);
    assert_eq!(tree.visual(b).unwrap().background, green);

    // Only the primary button activates.
    p.down(&mut tree, 2, 1000.0);
    assert!(!tree.state(b).active);
}

// M3-EVT-02
#[test]
fn clicks_go_to_the_common_ancestor_of_press_and_release() {
    let (mut tree, [a, b, c]) = tree("");
    let mut p = Pointer::new();

    p.move_to(&mut tree, 50.0, 50.0);
    let e = p.down(&mut tree, 0, 0.0);
    assert_eq!(summary(&e), [("pointerdown", vec![b, a])]);
    assert_eq!((e[0].button, e[0].buttons, e[0].detail), (0, 1, 1));

    p.move_to(&mut tree, 10.0, 10.0);
    let e = p.up(&mut tree, 0);
    assert_eq!(
        summary(&e),
        [("pointerup", vec![a]), ("click", vec![a])],
        "pressed on b, released on a: a gets the click"
    );
    assert_eq!(e[0].buttons, 0);

    p.move_to(&mut tree, 50.0, 50.0);
    p.down(&mut tree, 0, 1000.0);
    p.move_to(&mut tree, 50.0, 250.0);
    let e = p.up(&mut tree, 0);
    assert_eq!(
        summary(&e),
        [("pointerup", vec![c])],
        "no common ancestor: no click"
    );
}

// M3-EVT-02
#[test]
fn quick_second_clicks_are_double_clicks() {
    let (mut tree, [a, b, _]) = tree("");
    let mut p = Pointer::new();
    p.move_to(&mut tree, 50.0, 50.0);

    let click = |p: &mut Pointer, tree: &mut UiTree, t: f64| {
        p.down(tree, 0, t);
        p.up(tree, 0)
    };
    let e = click(&mut p, &mut tree, 0.0);
    assert_eq!(e[1].detail, 1);
    let e = click(&mut p, &mut tree, 200.0);
    assert_eq!(
        summary(&e),
        [
            ("pointerup", vec![b, a]),
            ("click", vec![b, a]),
            ("dblclick", vec![b, a]),
        ]
    );
    assert_eq!(e[1].detail, 2);
    let e = click(&mut p, &mut tree, 400.0);
    assert_eq!(e.len(), 2, "a triple click is a click with detail 3");
    assert_eq!(e[1].detail, 3);

    let e = click(&mut p, &mut tree, 2000.0);
    assert_eq!(e[1].detail, 1, "too slow: the count restarts");

    p.move_to(&mut tree, 60.0, 60.0);
    let e = click(&mut p, &mut tree, 2100.0);
    assert_eq!(e[1].detail, 1, "moved too far: the count restarts");
}

#[test]
fn secondary_button_opens_a_context_menu_instead_of_clicking() {
    let (mut tree, [a, b, _]) = tree("");
    let mut p = Pointer::new();
    p.move_to(&mut tree, 50.0, 50.0);
    let e = p.down(&mut tree, 2, 0.0);
    assert_eq!((e[0].button, e[0].buttons), (2, 2));
    let e = p.up(&mut tree, 2);
    assert_eq!(
        summary(&e),
        [("pointerup", vec![b, a]), ("contextmenu", vec![b, a])]
    );
}

#[test]
fn wheel_and_modifiers_are_reported() {
    let (mut tree, [a, b, _]) = tree("");
    let mut p = Pointer::new();
    p.set_modifiers(Modifiers {
        shift: true,
        ctrl: false,
        alt: true,
        meta: false,
    });
    p.move_to(&mut tree, 50.0, 50.0);
    let e = p.wheel(&mut tree, 0.0, 120.0);
    assert_eq!(summary(&e), [("wheel", vec![b, a])]);
    assert_eq!((e[0].delta_x, e[0].delta_y), (0.0, 120.0));

    let [ty, path, data] = e[0].script_args().try_into().expect("no key data");
    assert_eq!(ty, Value::from("wheel"));
    assert_eq!(path, Value::Array(vec![n(b.0), n(a.0)]));
    assert_eq!(
        data,
        Value::Array(
            [
                50.0, 50.0, 0.0, 0.0, 0.0, 0.0, 120.0, 5.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 0.0,
            ]
            .into_iter()
            .map(Value::Number)
            .collect()
        ),
        "x, y, button, buttons, detail, deltaX, deltaY, modifiers (shift | alt), repeat, related, \n         then six scroll fields, then the selection (start, end)"
    );
}

#[test]
fn refresh_moves_hover_when_the_tree_changes_under_a_still_pointer() {
    let (mut tree, [a, b, _]) = tree("");
    let mut p = Pointer::new();
    p.move_to(&mut tree, 50.0, 50.0);
    p.down(&mut tree, 0, 0.0);

    commit(
        &mut tree,
        vec![op(SET_STYLE, &[n(b.0), "display".into(), "none".into()])],
    );
    let e = p.refresh(&mut tree);
    assert_eq!(summary(&e), [("pointerleave", vec![b])]);
    assert_eq!(p.hovered(), Some(a));
    assert!(!tree.state(b).hover);
    assert!(
        tree.state(b).active,
        "a hidden element stays :active until release, like browsers"
    );
    assert!(p.refresh(&mut tree).is_empty(), "nothing changed");
}

#[test]
fn cursor_is_inherited_and_auto_depends_on_the_element() {
    let mut d = Dom::new();
    let button = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("cursor", "pointer".into()),
        ],
    );
    let icon = d.add(button, "box", &[("width", px(10.0)), ("height", px(10.0))]);
    let label = d.add(0, "text", &[]);
    d.text(label, "Label");
    let reset = d.add(
        button,
        "box",
        &[
            ("width", px(10.0)),
            ("height", px(10.0)),
            ("cursor", "auto".into()),
        ],
    );
    d.ops.push(op(
        SET_ATTRS,
        &[n(reset), "className".into(), "locked".into()],
    ));
    let mut tree = d.build(".locked { cursor: not-allowed }");

    assert_eq!(tree.cursor(Some(NodeId(button))), "pointer");
    assert_eq!(tree.cursor(Some(NodeId(icon))), "pointer", "inherited");
    assert_eq!(tree.cursor(Some(NodeId(label))), "text");
    assert_eq!(tree.cursor(None), "default");
    assert_eq!(
        tree.cursor(Some(NodeId(reset))),
        "default",
        "inline auto beats the stylesheet and stops inheritance"
    );

    let mut p = Pointer::new();
    p.move_to(&mut tree, 5.0, 5.0);
    assert_eq!(p.cursor(&tree), "pointer");
}

// M3-CUR-01
#[test]
fn cursor_images_come_with_a_fallback_keyword_and_inherit() {
    use platypus_ui::CursorImage;
    let mut d = Dom::new();
    let outer = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(100.0)),
            (
                "cursor",
                "url(hand.cur), url('sword.png') 4 6, pointer".into(),
            ),
        ],
    );
    let inner = d.add(outer, "box", &[("width", px(10.0)), ("height", px(10.0))]);
    let plain = d.add(0, "box", &[("cursor", "move".into())]);
    let mut tree = d.build("");
    let images = [
        CursorImage {
            url: "hand.cur".into(),
            hotspot: None,
        },
        CursorImage {
            url: "sword.png".into(),
            hotspot: Some((4.0, 6.0)),
        },
    ];
    let (keyword, got) = tree.cursor_with_images(Some(NodeId(inner)));
    assert_eq!((keyword, got), ("pointer", &images[..]), "inherited");
    let (keyword, got) = tree.cursor_with_images(Some(NodeId(plain)));
    assert_eq!((keyword, got.len()), ("move", 0));

    // A keyword is required last, and a hotspot needs both coordinates.
    for bad in [
        "url(a.png)",
        "url(a.png) 1, pointer",
        "pointer, url(a.png)",
        "url() , move",
    ] {
        let errors = tree.apply_commit(&Commit {
            ops: vec![op(SET_STYLE, &[n(plain), "cursor".into(), bad.into()])],
        });
        assert!(!errors.is_empty(), "{bad} should be rejected");
        assert_eq!(
            tree.cursor(Some(NodeId(plain))),
            "move",
            "{bad} keeps the old value"
        );
    }
}

#[test]
fn removed_targets_get_no_click() {
    let (mut tree, [a, b, _]) = tree("");
    let mut p = Pointer::new();
    p.move_to(&mut tree, 50.0, 50.0);
    p.down(&mut tree, 0, 0.0);
    commit(
        &mut tree,
        vec![op(platypus_ui::opcodes::REMOVE_CHILD, &[n(a.0), n(b.0)])],
    );
    let e = p.up(&mut tree, 0);
    assert_eq!(
        summary(&e),
        [("pointerup", vec![a])],
        "no leave for the removed node and no click"
    );
}

// M3-EVT-06
#[test]
fn pointer_capture_routes_moves_and_the_release_to_the_capturer() {
    let (mut tree, [a, b, c]) = tree("");
    let mut p = Pointer::new();
    p.move_to(&mut tree, 30.0, 30.0);
    // Capture needs a held button.
    assert!(p.set_capture(&mut tree, b).is_empty());
    p.down(&mut tree, 0, 0.0);
    let e = p.set_capture(&mut tree, b);
    assert_eq!(summary(&e)[0], ("gotpointercapture", vec![b, a]));
    assert_eq!(p.capture(), Some(b));

    // Over `c`, events still go to `b`, which stays hovered.
    let e = p.move_to(&mut tree, 50.0, 250.0);
    assert_eq!(summary(&e), [("pointermove", vec![b, a])]);
    assert!(tree.state(b).hover);
    assert!(!tree.state(c).hover);

    // The release goes to `b` (and clicks it), then capture ends and hover moves to `c`.
    let e = p.up(&mut tree, 0);
    let names: Vec<_> = summary(&e)
        .into_iter()
        .map(|(n, path)| (n, path[0]))
        .collect();
    assert_eq!(
        names,
        [
            ("pointerup", b),
            ("click", b),
            ("lostpointercapture", b),
            ("pointerleave", b),
            ("pointerleave", a),
            ("pointerenter", c),
        ]
    );
    assert_eq!(p.capture(), None);
    assert!(tree.state(c).hover);
}

// M3-EVT-06
#[test]
fn capture_ends_on_release_call_removal_or_window_blur() {
    let (mut tree, [_, b, _]) = tree("");
    let mut p = Pointer::new();
    p.move_to(&mut tree, 30.0, 30.0);
    p.down(&mut tree, 0, 0.0);
    p.set_capture(&mut tree, b);
    let e = p.release_capture(&mut tree, b);
    assert_eq!(summary(&e)[0].0, "lostpointercapture");
    assert_eq!(p.capture(), None);

    p.set_capture(&mut tree, b);
    commit(
        &mut tree,
        vec![op(platypus_ui::opcodes::REMOVE_CHILD, &[n(1), n(b.0)])],
    );
    p.refresh(&mut tree);
    assert_eq!(p.capture(), None, "a removed element loses capture");

    let (mut tree, [_, b, _]) = tree_again();
    let mut p = Pointer::new();
    p.move_to(&mut tree, 30.0, 30.0);
    p.down(&mut tree, 0, 0.0);
    p.set_capture(&mut tree, b);
    p.cancel(&mut tree);
    assert_eq!(p.capture(), None, "window blur");
}

fn tree_again() -> (UiTree, [NodeId; 3]) {
    tree("")
}
