// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

//! Drag and drop, like HTML's: a primary press on a `draggable` element that moves
//! more than [`DRAG_THRESHOLD`] pixels starts a drag (`dragstart`, which scripts can
//! prevent). While it lasts, pointer events stop: the dragged element gets `drag`, the
//! element under the pointer `dragenter` / `dragover` / `dragleave`, and the release
//! sends `drop` (if the last `dragover` was prevented, which accepts it) and
//! `dragend`. Escape or losing the window cancels. Files dragged in from the system
//! ([`Input::FileDragEnter`] / [`Input::FileDrop`]) run the same way without a
//! dragged element. The data being dragged lives in the scripts (`dataTransfer`);
//! the engine only carries the file paths.

use web_ui_core::NodeId;

use super::{Input, Interaction, send};
use crate::events::{EventKind, UiEvent};
use crate::tree::UiTree;

/// How far (logical pixels) a press must move before it becomes a drag.
pub const DRAG_THRESHOLD: f32 = 4.0;

#[derive(Debug, Default)]
pub(super) struct Dnd {
    /// A press on a draggable element that may become a drag.
    pending: Option<Pending>,
    active: Option<Active>,
}

#[derive(Clone, Copy, Debug)]
struct Pending {
    source: NodeId,
    x: f32,
    y: f32,
}

#[derive(Debug)]
struct Active {
    /// The dragged element; `None` for files from the system.
    source: Option<NodeId>,
    files: Vec<String>,
    /// The element under the pointer (its event path), which gets `dragover`.
    target: Vec<NodeId>,
    /// Whether the last `dragover` was prevented: a drop there is accepted.
    accepted: bool,
    /// The drag image and the point in it that stays under the pointer.
    image: Option<(NodeId, f32, f32)>,
}

impl Interaction {
    /// Whether a drag is in progress.
    pub fn dragging(&self) -> bool {
        self.dnd.active.is_some()
    }

    /// During a drag, the cursor: whether the element under the pointer takes the drop.
    pub(super) fn drag_cursor(&self) -> Option<&'static str> {
        let a = self.dnd.active.as_ref()?;
        Some(match (a.accepted, a.source) {
            (false, _) => "no-drop",
            (true, None) => "copy",
            (true, Some(_)) => "grabbing",
        })
    }

    /// After a primary press the scripts didn't prevent: remembers the nearest
    /// draggable element on its path. Returns whether there is one.
    pub(super) fn dnd_press(&mut self, tree: &UiTree, event: &UiEvent) -> bool {
        let source = event.path.iter().copied().find(|id| {
            tree.attrs(*id).is_some_and(|a| a.draggable && !a.disabled) && !tree.is_editor(*id)
        });
        self.dnd.pending = source.map(|source| Pending {
            source,
            x: event.x,
            y: event.y,
        });
        source.is_some()
    }

    /// Runs the inputs drag and drop takes over. Returns true when the input was
    /// handled here (and must not go through the normal path).
    pub(super) fn dnd_input(
        &mut self,
        tree: &mut UiTree,
        input: &Input,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) -> bool {
        let active = self.dnd.active.is_some();
        match input {
            Input::PointerMove { x, y } if active => {
                self.pointer.set_position(*x, *y);
                self.drag_over(tree, dispatch);
                true
            }
            Input::PointerMove { x, y } => {
                let Some(p) = self.dnd.pending else {
                    return false;
                };
                if (x - p.x).hypot(y - p.y) <= DRAG_THRESHOLD {
                    return false;
                }
                self.dnd.pending = None;
                // The button went up without us seeing it.
                if self.pointer.buttons() & 1 == 0 {
                    return false;
                }
                self.drag_start(tree, p, *x, *y, dispatch)
            }
            Input::PointerDown { .. } => {
                self.dnd.pending = None;
                false
            }
            Input::PointerUp { button: 0 } if active => {
                self.drop_here(tree, dispatch);
                true
            }
            Input::PointerUp { button: 0 } => {
                self.dnd.pending = None;
                false
            }
            Input::KeyDown { key, .. } if active && key == "Escape" => {
                self.dnd_cancel(tree, dispatch);
                true
            }
            Input::FileDragEnter { files, x, y } => {
                match &mut self.dnd.active {
                    // A drag of an element is already on: it wins.
                    Some(a) if a.source.is_some() => return true,
                    Some(a) => a.files = files.clone(),
                    None => self.dnd.active = Some(Active::files(files.clone())),
                }
                self.pointer.set_position(*x, *y);
                self.drag_over(tree, dispatch);
                true
            }
            Input::FileDrop { files } => {
                match &mut self.dnd.active {
                    Some(a) if a.source.is_some() => return true,
                    Some(a) => a.files = files.clone(),
                    // Dropped without a hover first (some platforms): check the target now.
                    None => {
                        self.dnd.active = Some(Active::files(files.clone()));
                        self.drag_over(tree, dispatch);
                    }
                }
                self.drop_here(tree, dispatch);
                true
            }
            Input::FileDragCancel => {
                if self.dnd.active.as_ref().is_some_and(|a| a.source.is_none()) {
                    self.dnd_cancel(tree, dispatch);
                }
                true
            }
            Input::SetDragImage { id, x, y } => {
                if let Some(a) = &mut self.dnd.active {
                    a.image = id.filter(|id| tree.contains(*id)).map(|id| (id, *x, *y));
                    if a.image.is_none() {
                        tree.set_drag_image(None);
                    }
                    self.update_drag_image(tree);
                }
                true
            }
            _ => false,
        }
    }

    /// Ends a drag without a drop (`dragleave`, then `dragend` with `detail` 0).
    pub(super) fn dnd_cancel(
        &mut self,
        tree: &mut UiTree,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) {
        self.dnd.pending = None;
        let Some(a) = self.dnd.active.take() else {
            return;
        };
        tree.set_drag_image(None);
        if let Some(&target) = a.target.first()
            && tree.contains(target)
        {
            dispatch(&self.drag_event(&a, EventKind::DragLeave, tree.event_path(target)));
        }
        if let Some(source) = a.source.filter(|s| tree.contains(*s)) {
            dispatch(&self.drag_event(&a, EventKind::DragEnd, tree.event_path(source)));
        }
    }

    fn drag_start(
        &mut self,
        tree: &mut UiTree,
        p: Pending,
        x: f32,
        y: f32,
        dispatch: &mut dyn FnMut(&UiEvent) -> bool,
    ) -> bool {
        if !tree.contains(p.source) {
            return false;
        }
        let active = Active {
            source: Some(p.source),
            files: Vec::new(),
            target: Vec::new(),
            accepted: false,
            // By default the element itself, held where it was pressed.
            image: tree
                .rect(p.source)
                .map(|r| (p.source, p.x - r.x, p.y - r.y)),
        };
        self.pointer.set_position(x, y);
        let start = self.drag_event(&active, EventKind::DragStart, tree.event_path(p.source));
        if dispatch(&start) {
            // Prevented: no drag; the move goes on as a normal one.
            return false;
        }
        // The press is a drag now: no click, no selection, no capture.
        self.selecting = None;
        self.selecting_text = false;
        tree.clear_text_selection();
        send(self.pointer.cancel_press(tree), dispatch);
        self.dnd.active = Some(active);
        self.drag_over(tree, dispatch);
        true
    }

    /// The pointer moved during a drag: `drag` to the dragged element, `dragenter` /
    /// `dragleave` when the element under the pointer changed, then `dragover`.
    fn drag_over(&mut self, tree: &mut UiTree, dispatch: &mut dyn FnMut(&UiEvent) -> bool) {
        let Some(mut a) = self.dnd.active.take() else {
            return;
        };
        self.update_image_of(tree, &a);
        let path = self
            .pointer
            .position()
            .and_then(|(x, y)| tree.hit_test(x, y))
            .map(|t| tree.event_path(t))
            .unwrap_or_default();
        if let Some(source) = a.source.filter(|s| tree.contains(*s)) {
            dispatch(&self.drag_event(&a, EventKind::Drag, tree.event_path(source)));
        }
        if path.first() != a.target.first() {
            let old = std::mem::replace(&mut a.target, path.clone());
            if !path.is_empty() {
                dispatch(&self.drag_event(&a, EventKind::DragEnter, path.clone()));
            }
            if let Some(&left) = old.first()
                && tree.contains(left)
            {
                dispatch(&self.drag_event(&a, EventKind::DragLeave, tree.event_path(left)));
            }
        }
        a.accepted = !path.is_empty() && dispatch(&self.drag_event(&a, EventKind::DragOver, path));
        self.dnd.active = Some(a);
    }

    /// The release (or a system drop): `drop` where it was accepted, else
    /// `dragleave`; then `dragend` to the dragged element.
    fn drop_here(&mut self, tree: &mut UiTree, dispatch: &mut dyn FnMut(&UiEvent) -> bool) {
        let Some(a) = self.dnd.active.take() else {
            return;
        };
        tree.set_drag_image(None);
        let target = a.target.first().copied().filter(|t| tree.contains(*t));
        let dropped = a.accepted && target.is_some();
        if let Some(target) = target {
            let kind = if dropped {
                EventKind::Drop
            } else {
                EventKind::DragLeave
            };
            dispatch(&self.drag_event(&a, kind, tree.event_path(target)));
        }
        if let Some(source) = a.source.filter(|s| tree.contains(*s)) {
            let mut end = self.drag_event(&a, EventKind::DragEnd, tree.event_path(source));
            end.detail = dropped as u32;
            dispatch(&end);
        }
        // The button is up; hover goes back to what's under the pointer (no
        // `pointerup` or `click` after a drag).
        if a.source.is_some() {
            for e in self.pointer.up(tree, 0) {
                if matches!(e.kind, EventKind::PointerEnter | EventKind::PointerLeave) {
                    dispatch(&e);
                }
            }
        } else {
            send(self.pointer.refresh(tree), dispatch);
        }
    }

    fn update_drag_image(&self, tree: &mut UiTree) {
        if let Some(a) = &self.dnd.active {
            self.update_image_of(tree, a);
        }
    }

    fn update_image_of(&self, tree: &mut UiTree, a: &Active) {
        let image = a.image.and_then(|(id, gx, gy)| {
            let r = tree.rect(id)?;
            let (x, y) = self.pointer.position()?;
            Some((id, x - gx - r.x, y - gy - r.y))
        });
        tree.set_drag_image(image);
    }

    fn drag_event(&self, a: &Active, kind: EventKind, path: Vec<NodeId>) -> UiEvent {
        let (x, y) = self.pointer.position().unwrap_or_default();
        UiEvent {
            x,
            y,
            buttons: self.pointer.buttons(),
            files: a.files.clone(),
            ..UiEvent::new(kind, path, self.modifiers)
        }
    }
}

impl Active {
    fn files(files: Vec<String>) -> Self {
        Active {
            source: None,
            files,
            target: Vec::new(),
            accepted: false,
            image: None,
        }
    }
}
