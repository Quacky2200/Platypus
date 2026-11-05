// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Dialogs (M3-DLG-01): closed dialogs are hidden, open ones centre in their
//! containing block, modal ones go in the top layer over a `::backdrop`, trap focus
//! and close on Escape or backdrop clicks (`cancel`, then `close`).

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::opcodes::{SET_ATTRS, SET_STYLE};
use platypus_ui::{DrawCmd, Input, Interaction, Rgba, UiEvent, UiTree};
use web_ui_core::{Commit, NodeId, Value};

/// Page: a focusable `opener` box at the top-left, a `fixed` box with a huge
/// `zIndex`, and a 100x60 dialog (closed) holding two focusable boxes.
struct Page {
    tree: UiTree,
    ui: Interaction,
    opener: NodeId,
    dialog: NodeId,
    first: NodeId,
    second: NodeId,
    events: Vec<(String, NodeId)>,
    prevent: Vec<&'static str>,
}

impl Page {
    fn new(css: &str, attrs: &[(&str, Value)]) -> Self {
        let mut d = Dom::new();
        let opener = d.add(0, "box", &[("width", px(50.0)), ("height", px(50.0))]);
        d.ops
            .push(op(SET_ATTRS, &[n(opener), "tabIndex".into(), px(0.0)]));
        d.add(
            0,
            "box",
            &[
                ("position", "fixed".into()),
                ("zIndex", px(999.0)),
                ("left", px(0.0)),
                ("top", px(0.0)),
                ("width", px(400.0)),
                ("height", px(400.0)),
                ("backgroundColor", "blue".into()),
            ],
        );
        let dialog = d.add(0, "dialog", &[("width", px(100.0)), ("height", px(60.0))]);
        let mut a = vec![n(dialog)];
        for (k, v) in attrs {
            a.push((*k).into());
            a.push(v.clone());
        }
        if !attrs.is_empty() {
            d.ops.push(op(SET_ATTRS, &a));
        }
        let first = d.add(dialog, "box", &[("width", px(10.0)), ("height", px(10.0))]);
        let second = d.add(dialog, "box", &[("width", px(10.0)), ("height", px(10.0))]);
        for id in [first, second] {
            d.ops
                .push(op(SET_ATTRS, &[n(id), "tabIndex".into(), px(0.0)]));
        }
        let mut p = Self {
            tree: d.build(css),
            ui: Interaction::new(),
            opener: NodeId(opener),
            dialog: NodeId(dialog),
            first: NodeId(first),
            second: NodeId(second),
            events: Vec::new(),
            prevent: Vec::new(),
        };
        p.input(Input::Refresh);
        p
    }

    fn input(&mut self, input: Input) {
        let (events, prevent) = (&mut self.events, &self.prevent);
        self.ui.handle(&mut self.tree, input, &mut |e: &UiEvent| {
            events.push((e.kind.name().to_owned(), e.path[0]));
            prevent.contains(&e.kind.name())
        });
        self.tree.layout_if_needed();
    }

    fn set_attr(&mut self, name: &str, value: Value) {
        let errors = self.tree.apply_commit(&Commit {
            ops: vec![op(SET_ATTRS, &[n(self.dialog.0), name.into(), value])],
        });
        assert!(errors.is_empty(), "{errors:?}");
        self.tree.layout_if_needed();
        self.input(Input::Refresh);
    }

    fn key(&mut self, key: &str) {
        self.input(Input::KeyDown {
            key: key.into(),
            code: String::new(),
            repeat: false,
            text: None,
        });
    }

    fn click(&mut self, x: f32, y: f32) {
        self.input(Input::PointerMove { x, y });
        self.input(Input::PointerDown {
            button: 0,
            time_ms: 0.0,
        });
        self.input(Input::PointerUp { button: 0 });
    }

    /// The dialog events since the last call.
    fn dialog_events(&mut self) -> Vec<String> {
        std::mem::take(&mut self.events)
            .into_iter()
            .filter(|(k, _)| k == "cancel" || k == "close")
            .map(|(k, _)| k)
            .collect()
    }

    /// Background colours of quads, in paint order.
    fn fills(&mut self) -> Vec<Rgba> {
        self.tree
            .draw_list()
            .into_iter()
            .filter_map(|c| match c {
                DrawCmd::Quad(q) => Some(q.background),
                _ => None,
            })
            .collect()
    }
}

const BLUE: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

// M3-DLG-01
#[test]
fn closed_dialogs_are_hidden_and_open_ones_centre() {
    let css = "dialog { background-color: red }";
    let mut p = Page::new(css, &[]);
    assert!(!p.tree.is_displayed(p.dialog));
    assert!(!p.tree.is_displayed(p.first));
    let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
    assert!(!p.fills().contains(&red), "a closed dialog draws nothing");

    p.set_attr("open", true.into());
    assert!(p.tree.is_displayed(p.dialog));
    assert!(p.fills().contains(&red));
    // Centred in the 400x400 root: its box is at 50%, translated back by half.
    let r = p.tree.rect(p.dialog).unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (200.0, 200.0, 100.0, 60.0));
    assert_eq!(
        p.tree.hit_test(155.0, 175.0),
        Some(NodeId(2)),
        "the fixed box is above a dialog that isn't modal"
    );
    // Not modal: the fixed box (zIndex 999) still paints above it.
    let fills = p.fills();
    let (dialog_at, blue_at) = (
        fills.iter().position(|c| *c == red).unwrap(),
        fills.iter().position(|c| *c == BLUE).unwrap(),
    );
    assert!(dialog_at < blue_at, "{fills:?}");

    // Closing hides it again; reopening restores the authored display.
    let errors = p.tree.apply_commit(&Commit {
        ops: vec![op(
            SET_STYLE,
            &[n(p.dialog.0), "display".into(), "grid".into()],
        )],
    });
    assert!(errors.is_empty());
    p.set_attr("open", false.into());
    assert!(!p.tree.is_displayed(p.dialog));
    p.set_attr("open", true.into());
    assert!(p.tree.is_displayed(p.dialog));
}

// M3-DLG-01
#[test]
fn modal_dialogs_paint_on_top_over_a_backdrop() {
    let css = "dialog { background-color: red } dialog::backdrop { background: #00ff00 }";
    let mut p = Page::new(css, &[("modal", true.into())]);
    p.set_attr("open", true.into());
    let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
    let green = Rgba::new(0.0, 1.0, 0.0, 1.0);
    let fills = p.fills();
    let at = |c: Rgba| fills.iter().position(|f| *f == c).unwrap();
    assert!(at(BLUE) < at(green) && at(green) < at(red), "{fills:?}");
    // The backdrop covers the window and belongs to the dialog.
    assert_eq!(p.tree.hit_test(5.0, 5.0), Some(p.dialog));
    assert_eq!(p.tree.hit_test(395.0, 395.0), Some(p.dialog));
    assert_eq!(p.tree.hit_test(152.0, 172.0), Some(p.first));
    // M3-DLG-03: `to_window` maps a laid-out point through the dialog's centring translate,
    // back to where it is hit (`to_local` undoes it).
    let r = p.tree.rect(p.first).unwrap();
    let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
    let (wx, wy) = p.tree.to_window(p.first, cx, cy).unwrap();
    assert_eq!((wx, wy), (155.0, 175.0));
    assert_eq!(p.tree.hit_test(wx, wy), Some(p.first));
    assert_eq!(p.tree.to_local(p.first, wx, wy), Some((cx, cy)));

    p.set_attr("open", false.into());
    let fills = p.fills();
    assert!(
        !fills.contains(&green) && !fills.contains(&red),
        "{fills:?}"
    );
    assert_ne!(p.tree.hit_test(5.0, 5.0), Some(p.dialog));
}

// M3-DLG-01
#[test]
fn the_default_backdrop_is_a_light_tint() {
    let mut p = Page::new("", &[("modal", true.into())]);
    p.set_attr("open", true.into());
    assert_eq!(p.fills().last(), Some(&Rgba::new(0.0, 0.0, 0.0, 0.1)));
}

// M3-DLG-01
#[test]
fn modals_take_focus_trap_tab_and_give_focus_back() {
    let mut p = Page::new("", &[("modal", true.into())]);
    p.input(Input::Focus(p.opener));
    assert_eq!(p.ui.focused(), Some(p.opener));
    p.set_attr("open", true.into());
    assert_eq!(p.ui.focused(), Some(p.first), "the first focusable inside");
    p.key("Tab");
    assert_eq!(p.ui.focused(), Some(p.second));
    p.key("Tab");
    assert_eq!(p.ui.focused(), Some(p.first), "Tab wraps inside the dialog");
    // Clicks outside reach only the backdrop (the dialog), not the opener.
    p.click(10.0, 10.0);
    assert_ne!(p.ui.focused(), Some(p.opener));
    p.set_attr("open", false.into());
    assert_eq!(p.ui.focused(), Some(p.opener), "focus goes back");
}

// M3-DLG-03
#[test]
fn focus_given_back_when_a_script_focused_inside_the_modal_first() {
    let mut p = Page::new("", &[("modal", true.into())]);
    p.input(Input::Focus(p.opener));
    // Opened and, before any refresh, a field inside focused by the script (as a dialog's
    // text field does when it mounts).
    let errors = p.tree.apply_commit(&Commit {
        ops: vec![op(SET_ATTRS, &[n(p.dialog.0), "open".into(), true.into()])],
    });
    assert!(errors.is_empty(), "{errors:?}");
    p.tree.layout_if_needed();
    p.input(Input::Focus(p.second));
    p.input(Input::Refresh);
    assert_eq!(p.ui.focused(), Some(p.second), "the script's choice stays");
    p.set_attr("open", false.into());
    assert_eq!(p.ui.focused(), Some(p.opener), "focus goes back");
}

// M3-DLG-01
#[test]
fn escape_and_backdrop_clicks_ask_to_close() {
    let mut p = Page::new("", &[("modal", true.into())]);
    p.set_attr("open", true.into());
    p.dialog_events();
    p.key("Escape");
    assert_eq!(p.dialog_events(), ["cancel", "close"]);
    // The default `closerequest` ignores backdrop clicks.
    p.click(10.0, 10.0);
    assert!(p.dialog_events().is_empty());

    p.set_attr("closedBy", "any".into());
    p.click(10.0, 10.0);
    assert_eq!(p.dialog_events(), ["cancel", "close"]);
    p.click(190.0, 190.0);
    assert!(p.dialog_events().is_empty(), "a click inside the dialog");

    p.prevent.push("cancel");
    p.key("Escape");
    assert_eq!(
        p.dialog_events(),
        ["cancel"],
        "a prevented cancel keeps it open"
    );
    p.prevent.clear();

    p.set_attr("closedBy", "none".into());
    p.key("Escape");
    p.click(10.0, 10.0);
    assert!(p.dialog_events().is_empty());
}

// M3-DLG-01
#[test]
fn dialog_attributes_match_selectors() {
    let css = "dialog[open] { opacity: 0.5 } dialog[modal] { color: red }";
    let mut p = Page::new(css, &[("modal", true.into())]);
    assert_eq!(p.tree.visual(p.dialog).unwrap().opacity, 1.0);
    assert_eq!(
        p.tree.computed_text(p.dialog).color,
        Rgba::new(1.0, 0.0, 0.0, 1.0)
    );
    p.set_attr("open", true.into());
    assert_eq!(p.tree.visual(p.dialog).unwrap().opacity, 0.5);
}
