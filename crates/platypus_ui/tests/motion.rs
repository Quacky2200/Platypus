// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Transitions (M3-ANI-02): started by stylesheet and inline changes, timed with
//! delays and easing, reversed mid-way, cancelled, and reported to scripts.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::opcodes::SET_STYLE;
use platypus_ui::{ElementState, Input, Interaction, Rgba, UiEvent, UiTree};
use web_ui_core::{Commit, NodeId, Value};

fn set_style(tree: &mut UiTree, id: u32, prop: &str, value: Value) {
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(SET_STYLE, &[n(id), prop.into(), value])],
    });
    assert!(errors.is_empty(), "{errors:?}");
}

fn at(tree: &mut UiTree, ms: f64) {
    tree.set_time(ms);
    tree.layout_if_needed();
}

fn opacity(tree: &UiTree, id: u32) -> f32 {
    tree.visual(NodeId(id)).unwrap().opacity
}

fn hover(tree: &mut UiTree, id: u32, on: bool) {
    tree.set_state(
        NodeId(id),
        ElementState {
            hover: on,
            ..ElementState::default()
        },
    );
}

// M3-ANI-02
#[test]
fn hover_rules_transition_over_time() {
    let mut d = Dom::new();
    let b = d.add(0, "box", &[]);
    d.set_class(b, "b");
    let mut tree =
        d.build(".b { opacity: 1; transition: opacity 1s linear } .b:hover { opacity: 0 }");
    at(&mut tree, 0.0);
    assert_eq!(opacity(&tree, b), 1.0);
    hover(&mut tree, b, true);
    at(&mut tree, 0.0);
    assert_eq!(opacity(&tree, b), 1.0, "starts from the old value");
    at(&mut tree, 250.0);
    assert!(
        (opacity(&tree, b) - 0.75).abs() < 1e-3,
        "{}",
        opacity(&tree, b)
    );
    assert!(tree.is_animating());
    at(&mut tree, 1000.0);
    assert_eq!(opacity(&tree, b), 0.0);
    assert!(!tree.is_animating(), "finished");
    let kinds: Vec<&str> = tree
        .take_motion_events()
        .iter()
        .map(|e| e.kind.name())
        .collect();
    assert_eq!(kinds, ["transitionrun", "transitionstart", "transitionend"]);
}

// M3-ANI-02
#[test]
fn reversing_midway_starts_from_the_current_value() {
    let mut d = Dom::new();
    let b = d.add(0, "box", &[]);
    d.set_class(b, "b");
    let mut tree =
        d.build(".b { opacity: 1; transition: opacity 1s linear } .b:hover { opacity: 0 }");
    at(&mut tree, 0.0);
    hover(&mut tree, b, true);
    at(&mut tree, 0.0);
    at(&mut tree, 400.0);
    assert!((opacity(&tree, b) - 0.6).abs() < 1e-3);
    hover(&mut tree, b, false);
    at(&mut tree, 400.0);
    assert!((opacity(&tree, b) - 0.6).abs() < 1e-3, "no jump");
    at(&mut tree, 900.0);
    assert!(
        (opacity(&tree, b) - 0.8).abs() < 1e-3,
        "{}",
        opacity(&tree, b)
    );
    let kinds: Vec<&str> = tree
        .take_motion_events()
        .iter()
        .map(|e| e.kind.name())
        .collect();
    assert!(kinds.contains(&"transitioncancel"), "{kinds:?}");
}

// M3-ANI-02
#[test]
fn inline_styles_transition_without_stylesheets() {
    let mut d = Dom::new();
    let b = d.add(
        0,
        "box",
        &[
            ("width", px(100.0)),
            ("height", px(10.0)),
            ("transition", "width 1s linear 0.5s".into()),
        ],
    );
    let mut tree = d.build("");
    at(&mut tree, 0.0);
    set_style(&mut tree, b, "width", px(200.0));
    at(&mut tree, 0.0);
    assert_eq!(tree.rect(NodeId(b)).unwrap().width, 100.0);
    at(&mut tree, 400.0);
    assert_eq!(
        tree.rect(NodeId(b)).unwrap().width,
        100.0,
        "still in its delay"
    );
    at(&mut tree, 1000.0);
    assert_eq!(tree.rect(NodeId(b)).unwrap().width, 150.0);
    at(&mut tree, 1500.0);
    assert_eq!(tree.rect(NodeId(b)).unwrap().width, 200.0);
}

// M3-ANI-02
#[test]
fn colours_transforms_and_easing() {
    let mut d = Dom::new();
    let b = d.add(
        0,
        "box",
        &[
            ("backgroundColor", "red".into()),
            ("transform", "none".into()),
            (
                "transition",
                "background-color 1s linear, transform 1s ease-in".into(),
            ),
        ],
    );
    let mut tree = d.build("");
    at(&mut tree, 0.0);
    set_style(&mut tree, b, "backgroundColor", "blue".into());
    set_style(&mut tree, b, "transform", "translateX(100px)".into());
    at(&mut tree, 0.0);
    at(&mut tree, 500.0);
    let v = tree.visual(NodeId(b)).unwrap();
    assert_eq!(v.background, Rgba::new(0.5, 0.0, 0.5, 1.0));
    assert!(v.transform.is_some(), "a transform mid-way");
    // ease-in at half time is well short of half way: probe through hit testing.
    let r = tree.rect(NodeId(b)).unwrap();
    let local = tree.to_local(NodeId(b), r.x + 40.0, r.y + 1.0).unwrap();
    assert!(local.0 > r.x + 1.0, "moved right a little: {local:?}");
    assert!(local.0 < r.x + 35.0, "less than halfway: {local:?}");
}

// M3-ANI-02
#[test]
fn only_listed_properties_transition() {
    let mut d = Dom::new();
    let b = d.add(
        0,
        "box",
        &[
            ("opacity", px(1.0)),
            ("width", px(10.0)),
            ("transition", "opacity 1s".into()),
        ],
    );
    let mut tree = d.build("");
    at(&mut tree, 0.0);
    set_style(&mut tree, b, "width", px(50.0));
    at(&mut tree, 0.0);
    assert_eq!(tree.rect(NodeId(b)).unwrap().width, 50.0, "width snaps");
    set_style(&mut tree, b, "opacity", px(0.0));
    at(&mut tree, 0.0);
    at(&mut tree, 100.0);
    assert!(opacity(&tree, b) > 0.5);
    // Dropping the transition finishes it at once.
    set_style(&mut tree, b, "transition", "none".into());
    at(&mut tree, 150.0);
    assert_eq!(opacity(&tree, b), 0.0);
}

// M3-ANI-02
#[test]
fn transition_events_reach_scripts_with_the_property() {
    let mut d = Dom::new();
    let b = d.add(
        0,
        "box",
        &[("opacity", px(1.0)), ("transition", "opacity 100ms".into())],
    );
    let mut tree = d.build("");
    let mut ui = Interaction::new();
    at(&mut tree, 0.0);
    set_style(&mut tree, b, "opacity", px(0.5));
    let mut seen = Vec::new();
    for ms in [0.0, 50.0, 100.0] {
        at(&mut tree, ms);
        ui.handle(&mut tree, Input::Refresh, &mut |e: &UiEvent| {
            seen.push((
                e.kind.name(),
                e.path[0],
                e.value.clone().unwrap_or_default(),
                e.delta_x,
            ));
            false
        });
    }
    let ends: Vec<_> = seen.iter().filter(|e| e.0 == "transitionend").collect();
    assert_eq!(ends.len(), 1, "{seen:?}");
    assert_eq!((ends[0].1, ends[0].2.as_str()), (NodeId(b), "opacity"));
    assert!((ends[0].3 - 0.1).abs() < 1e-6, "elapsed seconds");
}

fn events(tree: &mut UiTree) -> Vec<String> {
    tree.take_motion_events()
        .iter()
        .map(|e| format!("{} {}", e.kind.name(), e.name))
        .collect()
}

// M3-ANI-03
#[test]
fn keyframe_animations_run_and_report() {
    let mut d = Dom::new();
    let b = d.add(0, "box", &[]);
    d.set_class(b, "b");
    let mut tree = UiTree::new(400.0, 400.0);
    tree.set_time(0.0);
    let errors = tree.apply_commit(&Commit { ops: d.ops });
    assert!(errors.is_empty());
    let added = tree.add_stylesheet(
        "@keyframes fade { from { opacity: 0 } 50% { opacity: 0.2 } }
         .b { animation: fade 1s linear 2 }",
    );
    assert!(added.warnings.is_empty(), "{:?}", added.warnings);
    at(&mut tree, 0.0);
    assert_eq!(opacity(&tree, b), 0.0);
    at(&mut tree, 250.0);
    assert!(
        (opacity(&tree, b) - 0.1).abs() < 1e-3,
        "{}",
        opacity(&tree, b)
    );
    at(&mut tree, 750.0);
    assert!(
        (opacity(&tree, b) - 0.6).abs() < 1e-3,
        "to the element's own 1 at 100%"
    );
    at(&mut tree, 1250.0);
    assert!((opacity(&tree, b) - 0.1).abs() < 1e-3, "second iteration");
    assert!(tree.is_animating());
    at(&mut tree, 2100.0);
    assert_eq!(opacity(&tree, b), 1.0, "no fill: back to its own value");
    assert!(!tree.is_animating());
    assert_eq!(
        events(&mut tree),
        [
            "animationstart fade",
            "animationiteration fade",
            "animationend fade"
        ]
    );
}

// M3-ANI-03
#[test]
fn fill_modes_pause_and_inline_animation_names() {
    let mut d = Dom::new();
    let b = d.add(
        0,
        "box",
        &[
            ("animation", "slide 1s linear 0.5s both".into()),
            ("width", px(10.0)),
            ("height", px(10.0)),
        ],
    );
    let mut tree = UiTree::new(400.0, 400.0);
    tree.set_time(0.0);
    assert!(tree.apply_commit(&Commit { ops: d.ops }).is_empty());
    tree.add_stylesheet(
        "@keyframes slide { from { width: 100px } to { width: var(--end, 300px) } }",
    );
    let width = |tree: &mut UiTree| tree.rect(NodeId(b)).unwrap().width;
    at(&mut tree, 0.0);
    assert_eq!(width(&mut tree), 100.0, "backwards fill during the delay");
    at(&mut tree, 1000.0);
    assert_eq!(width(&mut tree), 200.0);
    set_style(&mut tree, b, "animationPlayState", "paused".into());
    at(&mut tree, 1000.0);
    at(&mut tree, 5000.0);
    assert_eq!(width(&mut tree), 200.0, "frozen while paused");
    set_style(&mut tree, b, "animationPlayState", "running".into());
    at(&mut tree, 5000.0);
    at(&mut tree, 5250.0);
    assert_eq!(width(&mut tree), 250.0, "resumes where it paused");
    at(&mut tree, 9000.0);
    assert_eq!(width(&mut tree), 300.0, "forwards fill");
    // Removing the animation drops it.
    set_style(&mut tree, b, "animation", "none".into());
    at(&mut tree, 9000.0);
    assert_eq!(width(&mut tree), 10.0);
}

// M3-ANI-03
#[test]
fn infinite_alternating_transform_animation() {
    let mut d = Dom::new();
    let b = d.add(0, "box", &[("width", px(10.0)), ("height", px(10.0))]);
    d.set_class(b, "spin");
    let mut tree = d.build(
        "@keyframes spin { to { transform: rotate(180deg) } }
         .spin { animation: spin 1s linear infinite alternate }",
    );
    tree.set_time(0.0);
    for ms in [0.0, 500.0, 1500.0, 2500.0, 100_000.0] {
        at(&mut tree, ms);
        assert!(tree.is_animating(), "still running at {ms}");
    }
    let ev = events(&mut tree);
    assert!(!ev.iter().any(|e| e.starts_with("animationend")), "{ev:?}");
}

const FADE_IN: &str = ".card { opacity: 1; transition: opacity 1s linear }
    @starting-style { .card { opacity: 0 } }";

/// Adds a box with `class` (and optional extra ops) under the root at time 0.
fn add_card(tree: &mut UiTree, id: u32, class: &str) {
    let mut d = Dom::new();
    d.next = id;
    let b = d.add(0, "box", &[]);
    if !class.is_empty() {
        d.set_class(b, class);
    }
    assert!(tree.apply_commit(&Commit { ops: d.ops }).is_empty());
}

// M3-ANI-04
#[test]
fn new_elements_transition_from_their_starting_style() {
    let mut tree = UiTree::new(400.0, 400.0);
    tree.add_stylesheet(FADE_IN);
    at(&mut tree, 0.0);
    add_card(&mut tree, 1, "card");
    at(&mut tree, 0.0);
    assert_eq!(opacity(&tree, 1), 0.0, "starts from @starting-style");
    at(&mut tree, 500.0);
    assert!((opacity(&tree, 1) - 0.5).abs() < 1e-3);
    at(&mut tree, 1000.0);
    assert_eq!(opacity(&tree, 1), 1.0);
}

// M3-ANI-04
#[test]
fn existing_elements_ignore_starting_style() {
    let mut tree = UiTree::new(400.0, 400.0);
    tree.add_stylesheet(FADE_IN);
    at(&mut tree, 0.0);
    add_card(&mut tree, 1, "");
    at(&mut tree, 0.0);
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(
            platypus_ui::opcodes::SET_ATTRS,
            &[n(1), "className".into(), "card".into()],
        )],
    });
    assert!(errors.is_empty());
    at(&mut tree, 0.0);
    at(&mut tree, 100.0);
    assert_eq!(opacity(&tree, 1), 1.0, "already styled: no fade in");
}

// M3-ANI-04
#[test]
fn opening_a_dialog_runs_its_starting_style() {
    let mut d = Dom::new();
    let dialog = d.add(0, "dialog", &[]);
    let mut tree = d.build(
        "dialog { opacity: 1; transition: opacity 1s linear }
         @starting-style { dialog[open] { opacity: 0 } }",
    );
    at(&mut tree, 0.0);
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(
            platypus_ui::opcodes::SET_ATTRS,
            &[n(dialog), "open".into(), true.into()],
        )],
    });
    assert!(errors.is_empty());
    at(&mut tree, 0.0);
    at(&mut tree, 250.0);
    assert!(
        (opacity(&tree, dialog) - 0.25).abs() < 1e-3,
        "{}",
        opacity(&tree, dialog)
    );
}

/// A 100x100 `.item` box under the root, at time 0.
fn item_tree(css: &str) -> UiTree {
    let mut d = Dom::new();
    let b = d.add(0, "box", &[("width", px(100.0)), ("height", px(100.0))]);
    d.set_class(b, "item");
    let mut tree = UiTree::new(400.0, 400.0);
    tree.set_time(0.0);
    assert!(tree.apply_commit(&Commit { ops: d.ops }).is_empty());
    tree.add_stylesheet(css);
    at(&mut tree, 0.0);
    tree
}

fn remove_item(tree: &mut UiTree) {
    let errors = tree.apply_commit(&Commit {
        ops: vec![op(platypus_ui::opcodes::REMOVE_CHILD, &[n(0), n(1)])],
    });
    assert!(errors.is_empty(), "{errors:?}");
}

// M3-ANI-05
#[test]
fn removed_elements_play_their_outro_first() {
    let mut tree =
        item_tree(".item { opacity: 1; transition: opacity 1s linear } .item:outro { opacity: 0 }");
    remove_item(&mut tree);
    at(&mut tree, 0.0);
    at(&mut tree, 500.0);
    assert!(tree.contains(NodeId(1)), "still fading out");
    assert!((opacity(&tree, 1) - 0.5).abs() < 1e-3);
    assert_eq!(tree.hit_test(50.0, 50.0), None, "but no longer hit");
    assert!(!tree.is_displayed(NodeId(1)), "nor focusable");
    at(&mut tree, 1000.0);
    assert!(!tree.contains(NodeId(1)), "gone once it finished");
}

// M3-ANI-05
#[test]
fn removal_is_immediate_without_an_exit() {
    // No :outro rules at all.
    let mut tree = item_tree(".item { opacity: 1; transition: opacity 1s }");
    remove_item(&mut tree);
    assert!(!tree.contains(NodeId(1)));
    // :outro rules that change nothing animated.
    let mut tree = item_tree(".item:outro { width: 10px }");
    remove_item(&mut tree);
    at(&mut tree, 0.0);
    assert!(!tree.contains(NodeId(1)));
}

// M3-ANI-05
#[test]
fn a_transition_running_before_removal_doesnt_hold_it() {
    // The item is still fading in when it's removed; an `:outro` rule elsewhere turns
    // deferred removal on, but nothing of the item's exits, so it goes at once.
    let css = ".item { opacity: 1; transition: opacity 1s linear }
        @starting-style { .item { opacity: 0 } }
        .other:outro { opacity: 0 }";
    let mut tree = item_tree(css);
    at(&mut tree, 300.0);
    assert!(opacity(&tree, 1) < 1.0, "fading in");
    remove_item(&mut tree);
    at(&mut tree, 310.0);
    assert!(!tree.contains(NodeId(1)));
}

// M3-ANI-05
#[test]
fn exit_animations_hold_removal_but_ongoing_ones_dont() {
    let css = "@keyframes spin { to { transform: rotate(360deg) } }
        @keyframes out { to { opacity: 0 } }
        .item { animation: spin 1s linear infinite }
        .item:outro { animation: spin 1s linear infinite, out 300ms forwards }";
    let mut tree = item_tree(css);
    at(&mut tree, 100.0);
    remove_item(&mut tree);
    at(&mut tree, 100.0);
    at(&mut tree, 300.0);
    assert!(tree.contains(NodeId(1)), "the exit animation runs");
    at(&mut tree, 450.0);
    assert!(!tree.contains(NodeId(1)), "the spinner doesn't keep it");
}
