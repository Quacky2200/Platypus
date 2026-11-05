// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Gamepad-style UI navigation (M3-PAD-01, UI side): the d-pad moves focus to the
//! nearest focusable element in a direction, and Activate clicks the focused one.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::opcodes::SET_ATTRS;
use platypus_ui::{Direction, Input, Interaction, UiTree};
use web_ui_core::NodeId;

/// A 3 x 2 grid of 50x30 focusable buttons, 10px apart:
/// ```text
/// 0 1 2
/// 3 4 5
/// ```
fn grid() -> (UiTree, Vec<NodeId>) {
    let mut d = Dom::new();
    let mut ids = Vec::new();
    for i in 0..6 {
        let (col, row) = (i % 3, i / 3);
        let id = d.add(
            0,
            "box",
            &[
                ("position", "absolute".into()),
                ("left", px(col as f64 * 60.0)),
                ("top", px(row as f64 * 40.0)),
                ("width", px(50.0)),
                ("height", px(30.0)),
            ],
        );
        d.ops
            .push(op(SET_ATTRS, &[n(id), "tabIndex".into(), px(0.0)]));
        ids.push(NodeId(id));
    }
    (d.build(""), ids)
}

fn go(ui: &mut Interaction, tree: &mut UiTree, input: Input) -> Vec<&'static str> {
    let mut names = Vec::new();
    ui.handle(tree, input, &mut |e| {
        names.push(e.kind.name());
        false
    });
    names
}

// M3-PAD-01
#[test]
fn the_d_pad_moves_focus_to_the_nearest_element_that_way() {
    let (mut tree, b) = grid();
    let mut ui = Interaction::new();

    go(&mut ui, &mut tree, Input::Navigate(Direction::Right));
    assert_eq!(
        ui.focused(),
        None,
        "nothing focused: the d-pad stays the game's"
    );

    go(&mut ui, &mut tree, Input::Focus(b[0]));
    for (dir, expect) in [
        (Direction::Right, 1),
        (Direction::Right, 2),
        (Direction::Right, 2),
        (Direction::Down, 5),
        (Direction::Left, 4),
        (Direction::Up, 1),
        (Direction::Left, 0),
        (Direction::Up, 0),
    ] {
        go(&mut ui, &mut tree, Input::Navigate(dir));
        assert_eq!(ui.focused(), Some(b[expect]), "{dir:?}");
    }
    assert!(
        tree.state(b[0]).focus_visible,
        "navigated focus shows the ring"
    );
}

// M9-VSP-10
#[test]
fn the_d_pad_is_an_arrow_key_first_and_preventing_it_keeps_focus() {
    let (mut tree, b) = grid();
    let mut ui = Interaction::new();
    go(&mut ui, &mut tree, Input::Focus(b[0]));
    // A list handling ArrowDown (preventDefault) keeps focus and moves its own selection.
    let mut keys = Vec::new();
    ui.handle(&mut tree, Input::Navigate(Direction::Down), &mut |e| {
        keys.push((e.kind.name(), e.key.clone(), e.target()));
        e.key == "ArrowDown"
    });
    assert_eq!(keys, [("keydown", "ArrowDown".to_string(), b[0])]);
    assert_eq!(ui.focused(), Some(b[0]));
    // Not prevented: focus moves as before.
    go(&mut ui, &mut tree, Input::Navigate(Direction::Down));
    assert_eq!(ui.focused(), Some(b[3]));
}

// M3-PAD-01
#[test]
fn activate_clicks_the_focused_element() {
    let (mut tree, b) = grid();
    let mut ui = Interaction::new();
    assert!(go(&mut ui, &mut tree, Input::Activate).is_empty());
    go(&mut ui, &mut tree, Input::Focus(b[4]));
    let mut clicked = Vec::new();
    ui.handle(&mut tree, Input::Activate, &mut |e| {
        clicked.push((e.kind.name(), e.target(), e.detail));
        false
    });
    assert_eq!(clicked, [("click", b[4], 0)]);
}
