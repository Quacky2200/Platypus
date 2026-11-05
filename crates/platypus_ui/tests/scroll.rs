// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Scrolling (M3-EVT-05): scroll offsets shift content (drawing and hit testing
//! agree), the wheel, keys and focus scroll the right container, scrollbars show,
//! drag and page, and scripts get `scroll` events.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::events::Modifiers;
use platypus_ui::opcodes::{REMOVE_CHILD, SET_ATTRS};
use platypus_ui::{
    DrawCmd, Input, Interaction, Rect, ScrollAlign, ScrollBehavior, UiEvent, UiTree,
};
use web_ui_core::{Commit, NodeId};

struct Rec {
    events: Vec<UiEvent>,
    prevent: Vec<&'static str>,
}

impl Rec {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            prevent: Vec::new(),
        }
    }

    fn names(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.events)
            .iter()
            .map(|e| e.kind.name())
            .collect()
    }
}

fn handle(ui: &mut Interaction, tree: &mut UiTree, rec: &mut Rec, input: Input) {
    ui.handle(tree, input, &mut |e| {
        rec.events.push(e.clone());
        rec.prevent.contains(&e.kind.name())
    });
}

fn key(name: &str) -> Input {
    Input::KeyDown {
        key: name.into(),
        code: String::new(),
        repeat: false,
        text: None,
    }
}

/// A 100x100 scroller (`overflow`) at (0, 0) holding `count` 100x50 rows, each with
/// `tabIndex` 0. Returns the tree, the scroller and the rows.
fn list(overflow: &str, count: usize) -> (UiTree, NodeId, Vec<NodeId>) {
    let mut d = Dom::new();
    let scroller = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(100.0)),
            ("flexDirection", "column".into()),
            ("overflow", overflow.into()),
        ],
    );
    let rows = (0..count)
        .map(|_| {
            let row = d.add(
                scroller,
                "box",
                &[
                    ("height", px(50.0)),
                    ("flexShrink", px(0.0)),
                    ("backgroundColor", "red".into()),
                ],
            );
            d.ops
                .push(op(SET_ATTRS, &[n(row), "tabIndex".into(), px(0.0)]));
            NodeId(row)
        })
        .collect();
    (d.build(""), NodeId(scroller), rows)
}

// M3-EVT-05
#[test]
fn scrolling_shifts_content_and_hit_testing_follows() {
    let (mut tree, scroller, rows) = list("auto", 5);
    assert_eq!(tree.scroll_max(scroller), (0.0, 150.0));
    assert_eq!(tree.hit_test(10.0, 10.0), Some(rows[0]));

    assert!(tree.scroll_to(scroller, 0.0, 60.0, true));
    tree.layout_if_needed();
    assert_eq!(tree.rect(rows[1]), Some(Rect::new(0.0, -10.0, 100.0, 50.0)));
    assert_eq!(
        tree.rect(scroller),
        Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
        "the container stays"
    );
    assert_eq!(tree.hit_test(10.0, 10.0), Some(rows[1]));

    assert!(tree.scroll_to(scroller, 0.0, 1000.0, true));
    assert_eq!(
        tree.scroll_offset(scroller),
        (0.0, 150.0),
        "clamped to the range"
    );
    assert!(
        !tree.scroll_to(scroller, 50.0, 150.0, true),
        "nothing to scroll on x"
    );

    // Content shrinking clamps the offset at the next layout.
    let ops = rows[2..]
        .iter()
        .map(|r| op(REMOVE_CHILD, &[n(scroller.0), n(r.0)]))
        .collect();
    let errors = tree.apply_commit(&Commit { ops });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    assert_eq!(tree.scroll_offset(scroller), (0.0, 0.0));
}

// M3-EVT-05
#[test]
fn only_scroll_and_auto_scroll_by_user_hidden_by_script_too() {
    for (overflow, user, script) in [
        ("auto", true, true),
        ("scroll", true, true),
        ("hidden", false, true),
        ("clip", false, false),
        ("visible", false, false),
    ] {
        let (mut tree, scroller, _) = list(overflow, 5);
        assert_eq!(
            tree.scroll_to(scroller, 0.0, 30.0, true),
            user,
            "{overflow} by user"
        );
        tree.scroll_to(scroller, 0.0, 0.0, false);
        assert_eq!(
            tree.scroll_to(scroller, 0.0, 30.0, false),
            script,
            "{overflow} by script"
        );
    }
}

// M3-EVT-05
#[test]
fn the_wheel_scrolls_the_innermost_container_that_can_move_then_chains() {
    let mut d = Dom::new();
    let outer = d.add(
        0,
        "box",
        &[
            ("width", px(200.0)),
            ("height", px(100.0)),
            ("flexDirection", "column".into()),
            ("overflow", "auto".into()),
        ],
    );
    let inner = d.add(
        outer,
        "box",
        &[
            ("height", px(80.0)),
            ("flexShrink", px(0.0)),
            ("flexDirection", "column".into()),
            ("overflow", "auto".into()),
        ],
    );
    d.add(
        inner,
        "box",
        &[("height", px(120.0)), ("flexShrink", px(0.0))],
    );
    d.add(
        outer,
        "box",
        &[("height", px(100.0)), ("flexShrink", px(0.0))],
    );
    let mut tree = d.build("");
    let (outer, inner) = (NodeId(outer), NodeId(inner));
    let mut ui = Interaction::new();
    let mut rec = Rec::new();

    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::PointerMove { x: 50.0, y: 40.0 },
    );
    rec.names();
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::Wheel { dx: 0.0, dy: 30.0 },
    );
    assert_eq!(tree.scroll_offset(inner), (0.0, 30.0));
    assert_eq!(tree.scroll_offset(outer), (0.0, 0.0));
    let scroll = rec
        .events
        .iter()
        .find(|e| e.kind.name() == "scroll")
        .unwrap();
    assert_eq!(
        scroll.path,
        [inner],
        "scroll goes to the scroller and doesn't bubble"
    );
    let info = scroll.scroll.unwrap();
    assert_eq!(
        (info.top, info.height, info.client_height),
        (30.0, 120.0, 80.0)
    );
    rec.names();

    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::Wheel { dx: 0.0, dy: 30.0 },
    );
    assert_eq!(
        tree.scroll_offset(inner),
        (0.0, 40.0),
        "the inner one hits its end"
    );
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::Wheel { dx: 0.0, dy: 30.0 },
    );
    assert_eq!(
        tree.scroll_offset(outer),
        (0.0, 30.0),
        "then the outer one scrolls"
    );

    rec.prevent.push("wheel");
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::Wheel { dx: 0.0, dy: 30.0 },
    );
    assert_eq!(
        tree.scroll_offset(outer),
        (0.0, 30.0),
        "a prevented wheel doesn't scroll"
    );
}

#[test]
fn shift_wheel_scrolls_horizontally() {
    let mut d = Dom::new();
    let strip = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(50.0)),
            ("overflow", "auto".into()),
        ],
    );
    d.add(
        strip,
        "box",
        &[("width", px(300.0)), ("flexShrink", px(0.0))],
    );
    let mut tree = d.build("");
    let mut ui = Interaction::new();
    let mut rec = Rec::new();
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::PointerMove { x: 10.0, y: 10.0 },
    );
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::Modifiers(Modifiers {
            shift: true,
            ..Modifiers::default()
        }),
    );
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::Wheel { dx: 0.0, dy: 40.0 },
    );
    assert_eq!(tree.scroll_offset(NodeId(strip)), (40.0, 0.0));
}

// M3-EVT-05
#[test]
fn keys_scroll_the_focused_elements_container() {
    let (mut tree, scroller, rows) = list("auto", 6);
    let mut ui = Interaction::new();
    let mut rec = Rec::new();
    handle(&mut ui, &mut tree, &mut rec, Input::Focus(rows[0]));
    handle(&mut ui, &mut tree, &mut rec, key("ArrowDown"));
    assert_eq!(tree.scroll_offset(scroller), (0.0, 40.0));
    handle(&mut ui, &mut tree, &mut rec, key("PageDown"));
    assert_eq!(
        tree.scroll_offset(scroller),
        (0.0, 127.5),
        "40 + 87.5% of 100"
    );
    handle(&mut ui, &mut tree, &mut rec, key("End"));
    assert_eq!(tree.scroll_offset(scroller), (0.0, 200.0));
    handle(&mut ui, &mut tree, &mut rec, key("Home"));
    assert_eq!(tree.scroll_offset(scroller), (0.0, 0.0));
    rec.prevent.push("keydown");
    handle(&mut ui, &mut tree, &mut rec, key("ArrowDown"));
    assert_eq!(
        tree.scroll_offset(scroller),
        (0.0, 0.0),
        "prevented keys don't scroll"
    );
}

// M3-EVT-05
#[test]
fn tab_scrolls_the_focused_element_into_view() {
    let (mut tree, scroller, rows) = list("auto", 6);
    let mut ui = Interaction::new();
    let mut rec = Rec::new();
    for _ in 0..3 {
        handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    }
    assert_eq!(ui.focused(), Some(rows[2]));
    // Row 2 spans 100..150: scrolled just enough to show its bottom.
    assert_eq!(tree.scroll_offset(scroller), (0.0, 50.0));
    handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    assert_eq!(tree.scroll_offset(scroller), (0.0, 100.0));
    // Shift-Tab back to row 0 scrolls up to its top.
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::Modifiers(Modifiers {
            shift: true,
            ..Modifiers::default()
        }),
    );
    for _ in 0..3 {
        handle(&mut ui, &mut tree, &mut rec, key("Tab"));
    }
    assert_eq!(ui.focused(), Some(rows[0]));
    assert_eq!(tree.scroll_offset(scroller), (0.0, 0.0));
}

// M3-EVT-05
#[test]
fn scripts_scroll_with_scroll_to_by_and_into_view() {
    let (mut tree, scroller, rows) = list("hidden", 6);
    let mut ui = Interaction::new();
    let mut rec = Rec::new();
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::ScrollTo {
            id: scroller,
            x: 0.0,
            y: 70.0,
            behavior: ScrollBehavior::Auto,
        },
    );
    assert_eq!(tree.scroll_offset(scroller), (0.0, 70.0));
    assert_eq!(rec.names(), ["scroll"]);
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::ScrollBy {
            id: scroller,
            dx: 0.0,
            dy: -20.0,
            behavior: ScrollBehavior::Auto,
        },
    );
    assert_eq!(tree.scroll_offset(scroller), (0.0, 50.0));
    let nearest = |id| Input::ScrollIntoView {
        id,
        block: ScrollAlign::Nearest,
        inline: ScrollAlign::Nearest,
        behavior: ScrollBehavior::Auto,
    };
    handle(&mut ui, &mut tree, &mut rec, nearest(rows[5]));
    assert_eq!(tree.scroll_offset(scroller), (0.0, 200.0));
    rec.names();
    handle(&mut ui, &mut tree, &mut rec, nearest(rows[5]));
    assert!(rec.names().is_empty(), "already visible: no scroll event");
}

// M3-EVT-08
#[test]
fn scroll_into_view_aligns_to_start_center_end_or_nearest() {
    // A 100px scroller of 50px rows: row 2 spans 100..150.
    let (mut tree, scroller, rows) = list("hidden", 6);
    let mut ui = Interaction::new();
    let mut rec = Rec::new();
    let mut place = |tree: &mut UiTree, from: f32, block: ScrollAlign| {
        tree.scroll_to(scroller, 0.0, from, false);
        let input = Input::ScrollIntoView {
            id: rows[2],
            block,
            inline: ScrollAlign::Nearest,
            behavior: ScrollBehavior::Auto,
        };
        handle(&mut ui, tree, &mut rec, input);
        tree.scroll_offset(scroller).1
    };
    assert_eq!(place(&mut tree, 0.0, ScrollAlign::Start), 100.0);
    assert_eq!(place(&mut tree, 0.0, ScrollAlign::End), 50.0);
    assert_eq!(place(&mut tree, 0.0, ScrollAlign::Center), 75.0);
    assert_eq!(
        place(&mut tree, 0.0, ScrollAlign::Nearest),
        50.0,
        "from above: its end"
    );
    assert_eq!(
        place(&mut tree, 200.0, ScrollAlign::Nearest),
        100.0,
        "from below: its start"
    );
    assert_eq!(
        place(&mut tree, 80.0, ScrollAlign::Nearest),
        80.0,
        "visible: stays"
    );
    // Clamped: the last row can't reach the top.
    tree.scroll_to(scroller, 0.0, 0.0, false);
    let input = Input::ScrollIntoView {
        id: rows[5],
        block: ScrollAlign::Start,
        inline: ScrollAlign::Nearest,
        behavior: ScrollBehavior::Auto,
    };
    handle(&mut ui, &mut tree, &mut rec, input);
    assert_eq!(tree.scroll_offset(scroller), (0.0, 200.0));
    assert_eq!(ScrollAlign::parse("center"), Some(ScrollAlign::Center));
    assert_eq!(ScrollAlign::parse("middle"), None);
}

fn quads(tree: &mut UiTree) -> Vec<Rect> {
    tree.draw_list()
        .into_iter()
        .filter_map(|c| match c {
            DrawCmd::Quad(q) => Some(q.rect),
            _ => None,
        })
        .collect()
}

// M3-EVT-05
#[test]
fn scrollbars_show_when_needed_and_track_the_offset() {
    let (tree, scroller, _) = list("auto", 2);
    assert!(
        tree.scrollbars(scroller).is_empty(),
        "auto without overflow: no bar"
    );
    let (tree, scroller, _) = list("scroll", 2);
    let bars = tree.scrollbars(scroller);
    assert_eq!(bars.len(), 2, "scroll always shows both, like CSS");
    assert!(bars[0].vertical && !bars[1].vertical);
    assert_eq!(
        bars[0].track.height, 86.0,
        "the vertical track stops short of the horizontal one"
    );
    let (tree, scroller, _) = list("hidden", 6);
    assert!(
        tree.scrollbars(scroller).is_empty(),
        "hidden never shows one"
    );

    let (mut tree, scroller, _) = list("auto", 4);
    let bar = tree.scrollbars(scroller)[0];
    assert!(bar.vertical);
    // The track runs down the right edge, inset by 2px; the thumb is half of it
    // (100 visible out of 200).
    assert_eq!(bar.track, Rect::new(90.0, 2.0, 8.0, 96.0));
    assert_eq!(bar.thumb, Rect::new(90.0, 2.0, 8.0, 48.0));
    let drawn = quads(&mut tree);
    assert_eq!(drawn.last(), Some(&bar.thumb), "drawn over the content");

    tree.scroll_to(scroller, 0.0, 100.0, true);
    tree.layout_if_needed();
    assert_eq!(
        tree.scrollbars(scroller)[0].thumb.y,
        50.0,
        "at the end of the track"
    );

    // scrollbar-width.
    let (mut tree, scroller, _) = list("auto", 4);
    let set = |tree: &mut UiTree, value: &str| {
        let errors = tree.apply_commit(&Commit {
            ops: vec![op(
                platypus_ui::opcodes::SET_STYLE,
                &[n(scroller.0), "scrollbarWidth".into(), value.into()],
            )],
        });
        assert!(errors.is_empty(), "{errors:?}");
        tree.layout_if_needed();
    };
    set(&mut tree, "thin");
    assert_eq!(tree.scrollbars(scroller)[0].track.width, 5.0);
    set(&mut tree, "none");
    assert!(tree.scrollbars(scroller).is_empty());
}

// M3-EVT-05
#[test]
fn dragging_the_thumb_scrolls_and_the_track_pages() {
    let (mut tree, scroller, _) = list("auto", 4);
    let mut ui = Interaction::new();
    let mut rec = Rec::new();
    // Grab the thumb (y 2..50) at y = 10 and drag it down by 24px: half of its
    // 48px travel, so half of the 100px range.
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::PointerMove { x: 94.0, y: 10.0 },
    );
    rec.names();
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::PointerDown {
            button: 0,
            time_ms: 0.0,
        },
    );
    assert!(
        !rec.names().contains(&"pointerdown"),
        "the scrollbar takes the press"
    );
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::PointerMove { x: 94.0, y: 34.0 },
    );
    assert_eq!(tree.scroll_offset(scroller), (0.0, 50.0));
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::PointerMove { x: 300.0, y: 300.0 },
    );
    assert_eq!(
        tree.scroll_offset(scroller),
        (0.0, 100.0),
        "clamped; works outside"
    );
    handle(&mut ui, &mut tree, &mut rec, Input::PointerUp { button: 0 });
    assert!(!rec.names().contains(&"click"));

    // A press on the track above the thumb pages up.
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::PointerMove { x: 94.0, y: 10.0 },
    );
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::PointerDown {
            button: 0,
            time_ms: 1000.0,
        },
    );
    handle(&mut ui, &mut tree, &mut rec, Input::PointerUp { button: 0 });
    assert_eq!(tree.scroll_offset(scroller), (0.0, 12.5));
}

#[test]
fn scrollbar_color_is_inherited() {
    let mut d = Dom::new();
    let page = d.add(0, "box", &[("scrollbarColor", "red blue".into())]);
    let scroller = d.add(
        page,
        "box",
        &[
            ("width", px(50.0)),
            ("height", px(50.0)),
            ("overflow", "scroll".into()),
        ],
    );
    let mut tree = d.build("");
    tree.layout_if_needed();
    let bar = tree.scrollbars(NodeId(scroller))[0];
    assert_eq!(bar.thumb_color, platypus_ui::Rgba::parse("red").unwrap());
    assert_eq!(bar.track_color, platypus_ui::Rgba::parse("blue").unwrap());
}

// M3-SCR-01
#[test]
fn scrollbar_gutter_stable_reserves_the_scrollbar_space() {
    let (tree, scroller, rows) = list("hidden", 1);
    assert_eq!(tree.rect(rows[0]).unwrap().width, 100.0);
    let (mut tree, scroller2, rows2) = list("hidden", 1);
    let _ = scroller;
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(
            platypus_ui::opcodes::SET_STYLE,
            &[n(scroller2.0), "scrollbarGutter".into(), "stable".into()],
        )],
    });
    assert!(errors.is_empty(), "{errors:?}");
    tree.layout_if_needed();
    // 8px bar + 2px margins on both sides of it.
    assert_eq!(tree.rect(rows2[0]).unwrap().width, 88.0);
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(
            platypus_ui::opcodes::SET_STYLE,
            &[n(scroller2.0), "scrollbarWidth".into(), "thin".into()],
        )],
    });
    assert!(errors.is_empty());
    tree.layout_if_needed();
    assert_eq!(
        tree.rect(rows2[0]).unwrap().width,
        91.0,
        "a thin bar needs less"
    );
    // `overflow: visible` has no scrollbar, so no gutter.
    let (mut tree, scroller3, rows3) = list("visible", 1);
    tree.apply_commit(&Commit {
        ops: vec![op(
            platypus_ui::opcodes::SET_STYLE,
            &[
                n(scroller3.0),
                "scrollbarGutter".into(),
                "stable both-edges".into(),
            ],
        )],
    });
    tree.layout_if_needed();
    assert_eq!(tree.rect(rows3[0]).unwrap().width, 100.0);
}

// M3-SCR-01
#[test]
fn overscroll_contain_stops_scroll_chaining() {
    // An outer scroller holding an inner one (scrolled to its end) and a tall spacer.
    let build = |contain: bool| {
        let mut d = Dom::new();
        let outer = d.add(
            0,
            "box",
            &[
                ("width", px(100.0)),
                ("height", px(100.0)),
                ("flexDirection", "column".into()),
                ("overflow", "auto".into()),
            ],
        );
        let mut inner_style = vec![
            ("height", px(80.0)),
            ("flexShrink", px(0.0)),
            ("flexDirection", "column".into()),
            ("overflow", "auto".into()),
        ];
        if contain {
            inner_style.push(("overscrollBehavior", "contain".into()));
        }
        let inner = d.add(outer, "box", &inner_style);
        d.add(
            inner,
            "box",
            &[("height", px(100.0)), ("flexShrink", px(0.0))],
        );
        d.add(
            outer,
            "box",
            &[("height", px(300.0)), ("flexShrink", px(0.0))],
        );
        (d.build(""), NodeId(outer), NodeId(inner))
    };
    for contain in [false, true] {
        let (mut tree, outer, inner) = build(contain);
        let mut ui = Interaction::new();
        let mut rec = Rec::new();
        handle(
            &mut ui,
            &mut tree,
            &mut rec,
            Input::PointerMove { x: 50.0, y: 40.0 },
        );
        handle(
            &mut ui,
            &mut tree,
            &mut rec,
            Input::Wheel { dx: 0.0, dy: 100.0 },
        );
        assert_eq!(tree.scroll_offset(inner).1, 20.0, "the inner one first");
        handle(
            &mut ui,
            &mut tree,
            &mut rec,
            Input::Wheel { dx: 0.0, dy: 100.0 },
        );
        let outer_y = tree.scroll_offset(outer).1;
        if contain {
            assert_eq!(outer_y, 0.0, "contained: the page stays put");
        } else {
            assert!(outer_y > 0.0, "chains to the outer scroller");
        }
    }
}

/// The tops of the text drawn, in window space.
fn text_tops(tree: &mut UiTree) -> Vec<f32> {
    tree.draw_list()
        .iter()
        .filter_map(|c| match c {
            DrawCmd::Text(t) => Some(t.bounds.y),
            _ => None,
        })
        .collect()
}

// M2-DRW-09
#[test]
fn content_scrolled_out_of_view_is_not_drawn() {
    let mut d = Dom::new();
    let scroller = d.add(
        0,
        "box",
        &[
            ("height", px(200.0)),
            ("width", px(300.0)),
            ("flexDirection", "column".into()),
            ("overflowY", "auto".into()),
        ],
    );
    // A page between the scroller and its rows: culling reaches past direct children.
    let page = d.add(
        scroller,
        "box",
        &[("flexDirection", "column".into()), ("flexShrink", px(0.0))],
    );
    for i in 0..60 {
        let style: &[(&str, web_ui_core::Value)] = if i == 40 {
            // Far below the view, but a shadow could reach anywhere: the row is drawn.
            &[("height", px(20.0)), ("boxShadow", "0 0 4px black".into())]
        } else {
            &[("height", px(20.0))]
        };
        let row = d.add(page, "box", style);
        let label = d.add(row, "text", &[]);
        d.text(label, &format!("Row {i}"));
    }
    let mut tree = d.build("");

    let tops = text_tops(&mut tree);
    assert!(tops.contains(&0.0), "the first row");
    assert!(!tops.contains(&600.0), "a row well below the view");
    // The view (200px) and a margin around it.
    assert!(tops.len() < 20, "{} rows drawn of 60", tops.len());
    let shadows = tree
        .draw_list()
        .iter()
        .filter(|c| matches!(c, DrawCmd::BoxShadow(_)))
        .count();
    assert_eq!(shadows, 1, "the shadowed row");

    tree.scroll_to(NodeId(scroller), 0.0, 1000.0, false);
    let tops = text_tops(&mut tree);
    assert!(tops.contains(&0.0), "row 50 at the top of the view");
    assert!(!tops.contains(&-1000.0), "the first row, scrolled away");
}

// M3-EVT-09
#[test]
fn smooth_scrolls_ease_over_time_and_send_scroll_events() {
    let (mut tree, scroller, rows) = list("hidden", 6);
    let mut ui = Interaction::new();
    let mut rec = Rec::new();
    tree.set_time(1000.0);
    let to = |y| Input::ScrollTo {
        id: scroller,
        x: f32::NAN,
        y,
        behavior: ScrollBehavior::Smooth,
    };
    handle(&mut ui, &mut tree, &mut rec, to(200.0));
    assert_eq!(
        tree.scroll_offset(scroller),
        (0.0, 0.0),
        "starts next frame"
    );
    assert!(tree.is_animating());
    // 200px takes 250 ms, eased in and out: halfway at half time.
    tree.set_time(1125.0);
    handle(&mut ui, &mut tree, &mut rec, Input::Refresh);
    let (_, mid) = tree.scroll_offset(scroller);
    assert!((mid - 100.0).abs() < 1.0, "{mid}");
    assert!(rec.names().contains(&"scroll"));
    tree.set_time(1300.0);
    handle(&mut ui, &mut tree, &mut rec, Input::Refresh);
    assert_eq!(tree.scroll_offset(scroller), (0.0, 200.0));
    assert!(!tree.is_animating());

    // A plain scroll (the wheel, a scrollbar, `scrollTo`) stops one in flight.
    handle(&mut ui, &mut tree, &mut rec, to(0.0));
    tree.set_time(1350.0);
    tree.scroll_to(scroller, 0.0, 120.0, false);
    tree.set_time(1600.0);
    assert_eq!(tree.scroll_offset(scroller), (0.0, 120.0));

    // `scrollBy` steps add up from where it's heading, not where it is.
    let by = Input::ScrollBy {
        id: scroller,
        dx: 0.0,
        dy: 30.0,
        behavior: ScrollBehavior::Smooth,
    };
    handle(&mut ui, &mut tree, &mut rec, by.clone());
    handle(&mut ui, &mut tree, &mut rec, by);
    assert_eq!(tree.scroll_goal(scroller), (0.0, 180.0));
    tree.set_time(2200.0);
    assert_eq!(tree.scroll_offset(scroller), (0.0, 180.0));

    // `scroll-behavior: smooth` makes `auto` script scrolls smooth too.
    tree.apply_commit(&Commit {
        ops: vec![op(
            platypus_ui::opcodes::SET_STYLE,
            &[n(scroller.0), "scrollBehavior".into(), "smooth".into()],
        )],
    });
    tree.scroll_to(scroller, 0.0, 0.0, false);
    let into_view = Input::ScrollIntoView {
        id: rows[5],
        block: ScrollAlign::Nearest,
        inline: ScrollAlign::Nearest,
        behavior: ScrollBehavior::Auto,
    };
    handle(&mut ui, &mut tree, &mut rec, into_view);
    assert_eq!(tree.scroll_offset(scroller), (0.0, 0.0));
    assert_eq!(tree.scroll_goal(scroller), (0.0, 200.0));
    assert_eq!(
        ScrollBehavior::parse("smooth"),
        Some(ScrollBehavior::Smooth)
    );
}

// M3-EVT-10
#[test]
fn scroll_padding_keeps_scrolled_into_view_elements_clear_of_the_edges() {
    // A 100px scroller of 50px rows; row 2 spans 100..150.
    let (mut tree, scroller, rows) = list("hidden", 6);
    tree.apply_commit(&Commit {
        ops: vec![op(
            platypus_ui::opcodes::SET_STYLE,
            &[n(scroller.0), "scrollPadding".into(), "10px 0".into()],
        )],
    });
    let mut ui = Interaction::new();
    let mut rec = Rec::new();
    let mut place = |tree: &mut UiTree, from: f32, block: ScrollAlign| {
        tree.scroll_to(scroller, 0.0, from, false);
        let input = Input::ScrollIntoView {
            id: rows[2],
            block,
            inline: ScrollAlign::Nearest,
            behavior: ScrollBehavior::Auto,
        };
        handle(&mut ui, tree, &mut rec, input);
        tree.scroll_offset(scroller).1
    };
    // The view is 10..90 inside the scroller.
    assert_eq!(
        place(&mut tree, 0.0, ScrollAlign::Nearest),
        60.0,
        "its end at 90"
    );
    assert_eq!(
        place(&mut tree, 200.0, ScrollAlign::Nearest),
        90.0,
        "its start at 10"
    );
    assert_eq!(place(&mut tree, 0.0, ScrollAlign::Start), 90.0);
    assert_eq!(
        place(&mut tree, 55.0, ScrollAlign::Nearest),
        60.0,
        "under the bottom padding: moves clear of it"
    );
}
