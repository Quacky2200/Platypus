// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Drag and drop (M5-DND): `draggable` elements, the drag events and their order,
//! accepting drops by preventing `dragover`, cancelling, the drag image, and files
//! dragged in from the system. The script side is in `js/react/test/events.test.tsx`.

use crate::common;

use common::{Dom, n, op, px};
use platypus_ui::opcodes::SET_ATTRS;
use platypus_ui::{DrawCmd, Input, Interaction, UiEvent, UiTree};
use web_ui_core::{NodeId, Value};

/// Records dispatched events as `(type, target)`; `prevent` lists the types whose
/// default the "script" prevents.
#[derive(Default)]
struct Recorder {
    log: Vec<(&'static str, NodeId)>,
    events: Vec<UiEvent>,
    prevent: Vec<&'static str>,
}

impl Recorder {
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

fn at(x: f64, y: f64) -> [(&'static str, Value); 6] {
    [
        ("backgroundColor", "teal".into()),
        ("position", "absolute".into()),
        ("left", px(x)),
        ("top", px(y)),
        ("width", px(50.0)),
        ("height", px(50.0)),
    ]
}

/// A draggable box `a` at (0, 0) and two targets: `b` at (100, 0), `c` at (200, 0).
fn scene() -> (UiTree, NodeId, NodeId, NodeId) {
    let mut d = Dom::new();
    let a = d.add(0, "box", &at(0.0, 0.0));
    d.ops
        .push(op(SET_ATTRS, &[n(a), "draggable".into(), true.into()]));
    let b = d.add(0, "box", &at(100.0, 0.0));
    let c = d.add(0, "box", &at(200.0, 0.0));
    (d.build(""), NodeId(a), NodeId(b), NodeId(c))
}

fn press(ui: &mut Interaction, tree: &mut UiTree, rec: &mut Recorder, x: f32, y: f32) {
    handle(ui, tree, rec, Input::PointerMove { x, y });
    handle(
        ui,
        tree,
        rec,
        Input::PointerDown {
            button: 0,
            time_ms: 0.0,
        },
    );
}

fn moves(ui: &mut Interaction, tree: &mut UiTree, rec: &mut Recorder, x: f32, y: f32) {
    handle(ui, tree, rec, Input::PointerMove { x, y });
}

fn release(ui: &mut Interaction, tree: &mut UiTree, rec: &mut Recorder) {
    handle(ui, tree, rec, Input::PointerUp { button: 0 });
}

fn drag_kinds(rec: &mut Recorder) -> Vec<(&'static str, NodeId)> {
    rec.take()
        .into_iter()
        .filter(|(k, _)| k.starts_with("drag") || *k == "drop" || *k == "click")
        .collect()
}

// M5-DND-01
#[test]
fn a_press_and_move_on_a_draggable_element_drags_it_to_an_accepting_target() {
    let (mut tree, a, b, _) = scene();
    let mut ui = Interaction::new();
    let mut rec = Recorder {
        prevent: vec!["dragover"],
        ..Default::default()
    };
    press(&mut ui, &mut tree, &mut rec, 10.0, 10.0);
    rec.take();
    // Within the threshold: still a press.
    moves(&mut ui, &mut tree, &mut rec, 12.0, 12.0);
    assert!(!ui.dragging());
    assert_eq!(drag_kinds(&mut rec), []);
    moves(&mut ui, &mut tree, &mut rec, 30.0, 10.0);
    assert!(ui.dragging());
    assert_eq!(
        drag_kinds(&mut rec),
        [
            ("dragstart", a),
            ("drag", a),
            ("dragenter", a),
            ("dragover", a)
        ]
    );
    moves(&mut ui, &mut tree, &mut rec, 110.0, 10.0);
    assert_eq!(
        drag_kinds(&mut rec),
        [
            ("drag", a),
            ("dragenter", b),
            ("dragleave", a),
            ("dragover", b)
        ]
    );
    // No pointer events while dragging.
    moves(&mut ui, &mut tree, &mut rec, 112.0, 10.0);
    assert!(rec.log.iter().all(|(k, _)| !k.starts_with("pointer")));
    assert_eq!(ui.cursor(&tree), "grabbing");
    rec.take();
    release(&mut ui, &mut tree, &mut rec);
    assert!(!ui.dragging());
    let end = rec
        .events
        .iter()
        .find(|e| e.kind.name() == "dragend")
        .cloned();
    assert_eq!(drag_kinds(&mut rec), [("drop", b), ("dragend", a)]);
    assert_eq!(end.unwrap().detail, 1, "dragend says it was dropped");
}

// M5-DND-02
#[test]
fn unaccepted_or_cancelled_drags_end_without_a_drop() {
    let (mut tree, a, b, _) = scene();
    let mut ui = Interaction::new();
    let mut rec = Recorder::default();
    press(&mut ui, &mut tree, &mut rec, 10.0, 10.0);
    moves(&mut ui, &mut tree, &mut rec, 110.0, 10.0);
    assert_eq!(ui.cursor(&tree), "no-drop");
    rec.take();
    release(&mut ui, &mut tree, &mut rec);
    let end = rec
        .events
        .iter()
        .find(|e| e.kind.name() == "dragend")
        .cloned();
    assert_eq!(drag_kinds(&mut rec), [("dragleave", b), ("dragend", a)]);
    assert_eq!(end.unwrap().detail, 0);

    // Escape cancels; the later release is not a click.
    rec.prevent = vec!["dragover"];
    press(&mut ui, &mut tree, &mut rec, 10.0, 10.0);
    moves(&mut ui, &mut tree, &mut rec, 110.0, 10.0);
    rec.take();
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::KeyDown {
            key: "Escape".into(),
            code: "Escape".into(),
            repeat: false,
            text: None,
        },
    );
    assert!(!ui.dragging());
    assert_eq!(drag_kinds(&mut rec), [("dragleave", b), ("dragend", a)]);
    release(&mut ui, &mut tree, &mut rec);
    assert_eq!(drag_kinds(&mut rec), []);
}

// M5-DND-02
#[test]
fn preventing_dragstart_or_a_short_press_leaves_normal_pointer_events() {
    let (mut tree, a, b, _) = scene();
    let mut ui = Interaction::new();
    let mut rec = Recorder {
        prevent: vec!["dragstart"],
        ..Default::default()
    };
    press(&mut ui, &mut tree, &mut rec, 10.0, 10.0);
    moves(&mut ui, &mut tree, &mut rec, 110.0, 10.0);
    assert!(!ui.dragging());
    let log = rec.take();
    assert!(log.contains(&("dragstart", a)));
    assert!(log.contains(&("pointermove", b)), "{log:?}");
    release(&mut ui, &mut tree, &mut rec);

    // A plain click on a draggable element is still a click.
    rec.prevent.clear();
    press(&mut ui, &mut tree, &mut rec, 10.0, 10.0);
    release(&mut ui, &mut tree, &mut rec);
    assert_eq!(drag_kinds(&mut rec), [("click", a)]);

    // Without `draggable`, a press and move doesn't drag.
    press(&mut ui, &mut tree, &mut rec, 110.0, 10.0);
    moves(&mut ui, &mut tree, &mut rec, 210.0, 10.0);
    assert!(!ui.dragging());
    release(&mut ui, &mut tree, &mut rec);
}

// M5-DND-03
#[test]
fn the_drag_image_follows_the_pointer_and_can_be_another_element() {
    let (mut tree, a, _, c) = scene();
    let mut ui = Interaction::new();
    let mut rec = Recorder::default();
    press(&mut ui, &mut tree, &mut rec, 10.0, 10.0);
    moves(&mut ui, &mut tree, &mut rec, 40.0, 30.0);
    // The element itself, held where it was pressed: moved by the pointer's travel.
    assert_eq!(tree.drag_image(), Some((a, 30.0, 20.0)));
    let cmds = tree.draw_list();
    assert!(matches!(cmds.last(), Some(DrawCmd::PopTransform)));
    assert!(
        cmds.iter()
            .any(|c| matches!(c, DrawCmd::PushLayer { opacity, .. } if *opacity < 1.0))
    );
    // Another element, grabbed at (5, 5): its corner sits 5 px up-left of the pointer.
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::SetDragImage {
            id: Some(c),
            x: 5.0,
            y: 5.0,
        },
    );
    assert_eq!(tree.drag_image(), Some((c, 40.0 - 5.0 - 200.0, 30.0 - 5.0)));
    release(&mut ui, &mut tree, &mut rec);
    assert_eq!(tree.drag_image(), None);
}

// M5-DND-04
#[test]
fn files_from_the_system_drag_over_and_drop_with_their_paths() {
    let (mut tree, _, b, c) = scene();
    let mut ui = Interaction::new();
    let mut rec = Recorder {
        prevent: vec!["dragover"],
        ..Default::default()
    };
    let files = vec!["C:/in/a.png".to_string(), "C:/in/b.txt".to_string()];
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::FileDragEnter {
            files: files.clone(),
            x: 110.0,
            y: 10.0,
        },
    );
    assert_eq!(drag_kinds(&mut rec), [("dragenter", b), ("dragover", b)]);
    assert_eq!(ui.cursor(&tree), "copy");
    moves(&mut ui, &mut tree, &mut rec, 210.0, 10.0);
    let over = rec.events.last().cloned().unwrap();
    assert_eq!(over.files, files);
    assert_eq!(
        drag_kinds(&mut rec),
        [("dragenter", c), ("dragleave", b), ("dragover", c)]
    );
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::FileDrop {
            files: files.clone(),
        },
    );
    let drop = rec
        .events
        .iter()
        .find(|e| e.kind.name() == "drop")
        .cloned()
        .unwrap();
    assert_eq!(drag_kinds(&mut rec), [("drop", c)]);
    // Scripts get the paths as the event's strings.
    let args = drop.script_args();
    assert_eq!(
        args.last(),
        Some(&Value::Array(vec![
            "C:/in/a.png".into(),
            "C:/in/b.txt".into()
        ]))
    );

    // Hovering then leaving the window: a dragleave, nothing dropped.
    handle(
        &mut ui,
        &mut tree,
        &mut rec,
        Input::FileDragEnter {
            files: files.clone(),
            x: 110.0,
            y: 10.0,
        },
    );
    rec.take();
    handle(&mut ui, &mut tree, &mut rec, Input::FileDragCancel);
    assert_eq!(drag_kinds(&mut rec), [("dragleave", b)]);
    assert!(!ui.dragging());
}
