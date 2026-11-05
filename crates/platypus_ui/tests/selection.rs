// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Selecting static text (M3-SEL-01): presses, double and triple clicks, drags,
//! `user-select`, Ctrl+C; `::selection` and `::placeholder` styles (M3-SEL-02).

use crate::common;

use common::{Dom, px};
use platypus_ui::events::Modifiers;
use platypus_ui::{Clipboard, DrawCmd, Input, Interaction, Rgba, UiTree};
use web_ui_core::Value;

struct Page {
    tree: UiTree,
    ui: Interaction,
}

impl Page {
    /// Text blocks, one per `(text, style)`, stacked from the top-left corner.
    fn new(blocks: &[(&str, &[(&str, Value)])], css: &str) -> Self {
        let mut d = Dom::new();
        let col = d.add(
            0,
            "box",
            &[
                ("flexDirection", "column".into()),
                ("alignItems", "flex-start".into()),
            ],
        );
        for (text, style) in blocks {
            let mut style = style.to_vec();
            style.push(("fontSize", px(20.0)));
            let t = d.add(col, "text", &style);
            d.text(t, text);
        }
        Self {
            tree: d.build(css),
            ui: Interaction::new(),
        }
    }

    fn input(&mut self, input: Input) {
        self.ui.handle(&mut self.tree, input, &mut |_| false);
        self.tree.layout_if_needed();
    }

    fn press(&mut self, x: f32, y: f32, t: f64) {
        self.input(Input::PointerMove { x, y });
        self.input(Input::PointerDown {
            button: 0,
            time_ms: t,
        });
    }

    fn release(&mut self) {
        self.input(Input::PointerUp { button: 0 });
    }

    fn click(&mut self, x: f32, y: f32, t: f64) {
        self.press(x, y, t);
        self.release();
    }

    fn selected(&self) -> Option<String> {
        self.tree.text_selection().map(|(_, t)| t)
    }

    fn copy(&mut self) -> Option<Clipboard> {
        self.input(Input::Modifiers(Modifiers {
            ctrl: true,
            ..Modifiers::default()
        }));
        self.input(Input::KeyDown {
            key: "c".into(),
            code: "KeyC".into(),
            repeat: false,
            text: None,
        });
        self.input(Input::Modifiers(Modifiers::default()));
        self.ui.take_clipboard()
    }

    fn quads(&mut self) -> Vec<(platypus_ui::Rect, Rgba)> {
        self.tree
            .draw_list()
            .into_iter()
            .filter_map(|c| match c {
                DrawCmd::Quad(q) => Some((q.rect, q.background)),
                _ => None,
            })
            .collect()
    }
}

// M3-SEL-01
#[test]
fn dragging_selects_text_and_ctrl_c_copies_it() {
    let mut p = Page::new(&[("hello world", &[])], "");
    p.press(1.0, 10.0, 0.0);
    p.input(Input::PointerMove { x: 300.0, y: 10.0 });
    p.release();
    assert_eq!(p.selected().as_deref(), Some("hello world"));
    assert_eq!(p.copy(), Some(Clipboard::Copy("hello world".into())));

    // A plain click collapses it: nothing to copy.
    p.click(5.0, 10.0, 1000.0);
    assert_eq!(p.selected(), None);
    assert_eq!(p.copy(), None);
}

// M3-SEL-01
#[test]
fn double_click_selects_a_word_and_triple_click_a_line() {
    let mut p = Page::new(
        &[("alpha beta\ngamma", &[("whiteSpace", "pre".into())])],
        "",
    );
    // Somewhere in "beta" (the second word on the first line).
    let x = p.tree.rect(web_ui_core::NodeId(2)).unwrap().x;
    let beta_x = x + 75.0;
    p.click(beta_x, 10.0, 0.0);
    p.click(beta_x, 10.0, 100.0);
    assert_eq!(p.selected().as_deref(), Some("beta"));
    p.click(beta_x, 10.0, 200.0);
    let line = p.selected().unwrap();
    assert!(line.starts_with("alpha beta"), "{line:?}");
    assert!(!line.contains("gamma"), "{line:?}");
}

// M3-SEL-01
#[test]
fn user_select_none_and_all() {
    let mut p = Page::new(
        &[
            ("not me", &[("userSelect", "none".into())]),
            ("all of me", &[("userSelect", "all".into())]),
        ],
        "",
    );
    p.press(1.0, 10.0, 0.0);
    p.input(Input::PointerMove { x: 300.0, y: 10.0 });
    p.release();
    assert_eq!(p.selected(), None, "user-select: none");
    let y = p.tree.rect(web_ui_core::NodeId(4)).unwrap().y + 10.0;
    p.click(30.0, y, 1000.0);
    assert_eq!(
        p.selected().as_deref(),
        Some("all of me"),
        "one click takes all"
    );
    // `none` text shows the arrow, selectable text the I-beam.
    assert_eq!(p.tree.cursor(Some(web_ui_core::NodeId(2))), "default");
    assert_eq!(p.tree.cursor(Some(web_ui_core::NodeId(4))), "text");
}

// M3-SEL-01
#[test]
fn a_child_can_opt_back_into_selection() {
    let mut d = Dom::new();
    let outer = d.add(0, "box", &[("userSelect", "none".into())]);
    let t = d.add(
        outer,
        "text",
        &[("userSelect", "text".into()), ("fontSize", px(20.0))],
    );
    d.text(t, "pick me");
    let mut p = Page {
        tree: d.build(""),
        ui: Interaction::new(),
    };
    p.press(1.0, 10.0, 0.0);
    p.input(Input::PointerMove { x: 300.0, y: 10.0 });
    p.release();
    assert_eq!(p.selected().as_deref(), Some("pick me"));
}

// M3-SEL-01
#[test]
fn selections_follow_transforms() {
    let mut p = Page::new(&[("hello world", &[("translate", "100px 0".into())])], "");
    // The text is drawn 100px to the right; pressing at its drawn start selects from 0.
    p.press(101.0, 10.0, 0.0);
    p.input(Input::PointerMove { x: 400.0, y: 10.0 });
    p.release();
    assert_eq!(p.selected().as_deref(), Some("hello world"));
}

// M3-SEL-02
#[test]
fn selection_draws_a_highlight_and_selection_rules_restyle_it() {
    let mut p = Page::new(&[("hello", &[])], "");
    assert!(p.quads().is_empty());
    p.click(5.0, 10.0, 0.0);
    p.click(5.0, 10.0, 100.0);
    let quads = p.quads();
    assert_eq!(quads.len(), 1, "one highlight: {quads:?}");
    assert!(quads[0].0.width > 20.0);
    assert!(quads[0].1.b > quads[0].1.r, "the default blue");

    let mut p = Page::new(
        &[("hello", &[])],
        "box::selection { background: #00ff00; color: red }",
    );
    p.click(5.0, 10.0, 0.0);
    p.click(5.0, 10.0, 100.0);
    let cmds = p.tree.draw_list();
    let green = cmds
        .iter()
        .any(|c| matches!(c, DrawCmd::Quad(q) if q.background == Rgba::new(0.0, 1.0, 0.0, 1.0)));
    assert!(green, "inherited from the ancestor's ::selection");
    // The selected glyphs again, in red, clipped to the highlight.
    let red = cmds.windows(2).any(|w| {
        matches!(&w[0], DrawCmd::PushClip(_))
            && matches!(&w[1], DrawCmd::Text(t) if t.runs[0].color == Rgba::new(1.0, 0.0, 0.0, 1.0))
    });
    assert!(red, "{cmds:#?}");
}

// M3-SEL-02
#[test]
fn placeholder_rules_style_the_placeholder() {
    let mut d = Dom::new();
    let input = d.add(0, "input", &[("color", "white".into())]);
    d.ops.push(common::op(
        platypus_ui::opcodes::SET_ATTRS,
        &[common::n(input), "placeholder".into(), "Name".into()],
    ));
    let mut tree = d.build("input::placeholder { color: #ff0000; opacity: 0.5; font-size: 30px }");
    let text = tree
        .draw_list()
        .into_iter()
        .find_map(|c| match c {
            DrawCmd::Text(t) => Some(t),
            _ => None,
        })
        .expect("placeholder");
    assert_eq!(text.runs[0].color, Rgba::new(1.0, 0.0, 0.0, 0.5));
    assert!(text.bounds.height > 25.0, "30px font: {:?}", text.bounds);
}
